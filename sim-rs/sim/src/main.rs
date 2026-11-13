use mcsim::*;

fn usage() -> ! {
    eprintln!("usage: mcsim run <hash|list-file> [out.tsv] [--verbose] [--ticks N] [--max N] [--events FILE | --events-dir DIR] [--stop-at TICK] [--rev {}]\n       mcsim cells <hash> x0 x1 y0 y1 z0 z1\n       mcsim selfcheck", rev::NAMES.join("|"));
    std::process::exit(2)
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // --rev NAME: which commit of the Java bot to predict (default: the newest the port has been brought to)
    if let Some(k) = args.iter().position(|a| a == "--rev") {
        if k + 1 >= args.len() { usage(); }
        rev::set(&args[k + 1]);
        args.drain(k..k + 2);
    }
    if args.len() < 2 { usage(); }
    let t0 = std::time::Instant::now();
    tables::init("/work/reference", "/work/data");
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    // the time limit is a panic payload: keep it quiet
    std::panic::set_hook(Box::new(|info| {
        if info.payload().downcast_ref::<bot::OutOfTime>().is_none() { eprintln!("panic: {info}"); }
    }));
    let prof_path = std::env::var("MCSIM_PROF").ok();
    if prof_path.is_some() { prof::start(); }
    match args[1].as_str() {
        "selfcheck" => {
            let tb = tables::t();
            let mut bad = 0;
            for s in 0..tb.n {
                if rules::face_sturdy(s as u16, geom::UP, 0) != tb.sturdy_top[s] { bad += 1; }
            }
            println!("tables: {} states in {:.0} ms; sturdyTop differences: {}", tb.n, load_ms, bad);
        }
        "run" => {
            if args.len() < 3 { usage(); }
            let mut verbose = false;
            let mut ticks = build::TICK_LIMIT;
            let mut max = usize::MAX;
            let mut out_path: Option<String> = None;
            let mut events: Option<String> = None;
            // (one placement log per build, DIR/ev-<hash8>.txt: what the Java side writes with MCB_EVENTS=<dir>)
            let mut events_dir: Option<String> = None;
            let mut stop_at: i64 = 0;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--verbose" => verbose = true,
                    "--ticks" => { i += 1; ticks = args[i].parse().unwrap(); }
                    "--max" => { i += 1; max = args[i].parse().unwrap(); }
                    "--events" => { i += 1; events = Some(args[i].clone()); }
                    "--events-dir" => { i += 1; events_dir = Some(args[i].clone()); }
                    "--stop-at" => { i += 1; stop_at = args[i].parse().unwrap(); }
                    o => out_path = Some(o.to_string()),
                }
                i += 1;
            }
            // a list file has one hash per line, optionally followed by the tick at which the Java run hit its wall limit
            let hashes: Vec<(String, i64)> = if std::path::Path::new(&args[2]).exists() {
                std::fs::read_to_string(&args[2]).unwrap().lines().filter_map(|l| { let mut f = l.split_whitespace(); f.next().map(|s| (s.to_string(), f.next().and_then(|v| v.parse().ok()).unwrap_or(0))) }).collect()
            } else { vec![(args[2].clone(), stop_at)] };
            let mut oracle = oracle::Oracle::new();
            let mut out_lines = Vec::new();
            for (h, stop) in hashes.iter().take(max) {
                let stop = if *stop > 0 { *stop } else { stop_at };
                let s = match schem::Schem::load(&schem::Schem::cache_file("/work/corpus", h)) { Ok(s) => s, Err(e) => { eprintln!("{h}: {e}"); continue; } };
                let ev_path = events_dir.as_ref().map(|d| format!("{d}/ev-{}.txt", &h[..8])).or_else(|| events.clone());
                let (o, orc) = build::run_until(&s, ticks, stop, verbose, oracle, ev_path.as_deref());
                oracle = orc;
                let line = o.tsv(h);
                eprintln!("{} {} {} | {} | blocks={} ticks={} placed={} skipped={} missing={} wrong={} leftover={} trips={} scaffold={} left={} wall={:.0}ms cpu={:.0}ms rtf={:.0}x searches={} expanded={} nav={:.0}% unmodelled={} lost={} random={}{} {}",
                    &h[..8], o.dims, if o.judge.pass { "PASS" } else { "FAIL" }, o.status, o.target_blocks, o.ticks, o.placed, o.skipped, o.judge.missing, o.judge.wrong, o.judge.leftover,
                    o.trips, o.scaffold_placed, o.scaffold_left, o.wall_ms, o.cpu_ms, o.ticks as f64 / 20.0 / (o.cpu_ms / 1000.0).max(1e-9), o.searches, o.expanded, 100.0 * o.search_ms / o.wall_ms.max(1e-9), o.unmodelled, o.items_lost, o.rand_draws, if o.died { " DIED" } else { "" }, o.extra);
                out_lines.push(line);
                // keep the output file current, so that a long run can be read (or stopped) part-way
                if let Some(pth) = &out_path { std::fs::write(pth, out_lines.join("\n") + "\n").unwrap(); }
            }
            if std::env::var_os("MCSIM_NAVSTAT").is_some() {
                bot::nav::NAVSTAT.with(|st| {
                    let st = st.borrow();
                    for (k, name) in ["found", "exhausted", "budget"].iter().enumerate() {
                        for b in 0..24 { if st[k][b].0 > 0 { eprintln!("navstat {name:9} <2^{b:<2} searches={:>9} expanded={:>11}", st[k][b].0, st[k][b].1); } }
                    }
                });
            }
            if std::env::var_os("MCSIM_REGSTAT").is_some() {
                bot::nav::REGSTAT.with(|m| { let m = m.borrow(); eprintln!("regstat: failing searches >96: {} with {} expansions; settled by the last exhausted region: {} saving {} expansions; regions recorded {}", m.2, m.3, m.4, m.5, m.6); });
            }
            if std::env::var_os("MCSIM_MEMOSTAT").is_some() {
                bot::nav::MEMOSTAT.with(|m| { let m = m.borrow(); eprintln!("memostat: repeated searches={} their expansions={} (failing: {}) of {} total", m.1, m.2, m.3, m.4); });
            }
            if std::env::var_os("MCSIM_CERTSTAT").is_some() {
                bot::nav::CERTSTAT.with(|c| { let c = c.borrow(); eprintln!("certstat: budget failures (>200 exp, no mining) {} with {} expansions; provable by a backward flood: {} with {} expansions (flood cells {})", c.0, c.1, c.2, c.3, c.4); });
            }
            match out_path {
                Some(pth) => std::fs::write(pth, out_lines.join("\n") + "\n").unwrap(),
                None => for l in out_lines { println!("{l}"); },
            }
        }
        "target" => {
            // target normalisation check: hash, target blocks, sections, exempt cells
            let hashes: Vec<String> = std::fs::read_to_string(&args[2]).unwrap().lines().filter_map(|l| l.split_whitespace().next().map(|s| s.to_string())).collect();
            for h in hashes {
                let s = schem::Schem::load(&schem::Schem::cache_file("/work/corpus", &h)).unwrap();
                let tg = build::Target::new(&s);
                let j = build::judge(&tg, &vec![0u16; tg.ids.len()]);
                println!("{h}\t{}\t{}\t{}", tg.blocks, j.sections, j.exempt_cells);
            }
        }
        "cells" => {
            // the target cells of a box of a build: mcsim cells <hash> x0 x1 y0 y1 z0 z1 (inclusive, build coordinates)
            if args.len() < 9 { usage(); }
            let s = schem::Schem::load(&schem::Schem::cache_file("/work/corpus", &args[2])).unwrap();
            let tg = build::Target::new(&s);
            let v: Vec<i32> = args[3..9].iter().map(|a| a.parse().unwrap()).collect();
            println!("{}x{}x{}", tg.sx, tg.sy, tg.sz);
            for y in v[2].max(0)..=v[3].min(tg.sy - 1) { for z in v[4].max(0)..=v[5].min(tg.sz - 1) { for x in v[0].max(0)..=v[1].min(tg.sx - 1) {
                let id = tg.ids[((y * tg.sz + z) * tg.sx + x) as usize];
                if id != 0 { println!("{x} {y} {z} {}", tables::t().name(id)); }
            } } }
        }
        _ => usage(),
    }
    if let Some(p) = prof_path { prof::finish(&p); }
}
