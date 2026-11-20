use mcsim::*;

fn usage() -> ! {
    eprintln!("usage: mcsim run <hash|list-file> [out.tsv] [--verbose] [--ticks N] [--max N] [--events FILE | --events-dir DIR] [--stop-at TICK] [--rev {}]\n                [--timeline FILE|--timeline-dir DIR] [--derail] [--derail-col] [--checkpoint-at derail|TICK [--margin TICKS] (--save FILE | --save-dir DIR)]\n       mcsim resume <checkpoint-file> [out.tsv] [--verbose] [--events FILE] [--timeline FILE]\n       mcsim cells <hash> x0 x1 y0 y1 z0 z1\n       mcsim selfcheck", rev::NAMES.join("|"));
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
        "detect" => {
            // the derailment rules over stored timelines (--timeline / --timeline-dir output)
            for f in &args[2..] {
                let tl = ckpt::Timeline::from_tsv(&std::fs::read_to_string(f).unwrap());
                let last = tl.samples.last().map(|s| s.tick).unwrap_or(0);
                match ckpt::detect(&tl) { Some(d) => println!("{f}\t{}\t{}\t{last}\t{}", d.rule, d.tick, d.why), None => println!("{f}\tnone\t0\t{last}\t") }
            }
        }
        "resume" => {
            // a checkpoint is replayed (see ckpt.rs): same build, revision, limits and seed; switches gated on
            // ckpt::armed() act from its tick on; the state at that tick is checked against the saved fingerprint
            if args.len() < 3 { usage(); }
            let ck = ckpt::Checkpoint::read(&args[2]).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(2) });
            // the run's environment again; variables set on top of it (an experiment's switches) are kept and named
            let extra: Vec<String> = ckpt::run_env().into_iter().filter(|kv| !ck.env.contains(kv)).map(|(k, v)| format!("{k}={v}")).collect();
            for (k, v) in &ck.env { std::env::set_var(k, v); }
            if !extra.is_empty() { eprintln!("resume: set on top of the checkpoint's environment (armed from t{} if gated on ckpt::armed): {}", ck.tick, extra.join(" ")); }
            // (after the environment: the revision reads its MCB_* switches when it is made)
            if !ck.rev.is_empty() { rev::set(&ck.rev); }
            std::env::set_var("MCSIM_FROM_TICK", ck.tick.to_string());
            ckpt::set_from_tick(ck.tick);
            ckpt::set_enabled(true);
            let (mut verbose, mut out_path, mut events, mut tl_path) = (false, None::<String>, None::<String>, None::<String>);
            let mut stop_over = None::<i64>;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--verbose" => verbose = true,
                    "--events" => { i += 1; events = Some(args[i].clone()); }
                    "--timeline" => { i += 1; tl_path = Some(args[i].clone()); }
                    // (a local test: stop at this tick instead of the saved run's; the row is then not the saved one)
                    "--stop-at" => { i += 1; stop_over = Some(args[i].parse::<i64>().expect("--stop-at TICK")); }
                    o => out_path = Some(o.to_string()),
                }
                i += 1;
            }
            let s = schem::Schem::load(&schem::Schem::cache_file("/work/corpus", &ck.hash)).unwrap_or_else(|e| { eprintln!("{}: {e}", ck.hash); std::process::exit(2) });
            let (o, _) = build::run_until(&s, ck.ticks_limit, stop_over.unwrap_or(ck.stop_at), verbose, oracle::Oracle::new(), events.as_deref());
            let tl = o.timeline.as_ref().unwrap();
            if let Some(p) = tl_path { std::fs::write(p, tl.tsv()).unwrap(); }
            let fp = tl.fp_at(ck.tick);
            let line = o.tsv(&ck.hash);
            if let Ok(dir) = std::env::var("MCSIM_CAUSES") { let hh: &str = &ck.hash; let _ = std::fs::write(format!("{dir}/{hh}.causes"), o.causes.join("\n") + "\n"); }
            let state_ok = ck.tick == 0 || fp == Some(ck.fp);
            let row_ok = ck.row.is_empty() || stop_over.is_some() || ckpt::row_key(&ck.row) == ckpt::row_key(&line);
            eprintln!("{} resume from t{} (rule {} at t{}): state at t{} {}; row {} the saved uninterrupted row",
                &ck.hash[..8], ck.tick, ck.rule, ck.derail_tick, ck.tick,
                match fp { Some(f) if f == ck.fp => "MATCHES the checkpoint".to_string(), Some(f) => format!("DIFFERS from the checkpoint ({f:016x} != {:016x})", ck.fp), None => "was never reached".to_string() },
                if ck.row.is_empty() { "(none saved) vs" } else if row_ok { "IDENTICAL to" } else { "DIFFERS from" });
            match out_path { Some(p) => std::fs::write(p, line + "\n").unwrap(), None => println!("{line}") }
            if !state_ok { std::process::exit(3); }
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
            // checkpoint tooling (ckpt.rs)
            let (mut tl_file, mut tl_dir, mut derail, mut derail_col) = (None::<String>, None::<String>, false, false);
            let (mut ck_at, mut margin, mut save, mut save_dir) = (None::<String>, 2400i64, None::<String>, None::<String>);
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--verbose" => verbose = true,
                    "--ticks" => { i += 1; ticks = args[i].parse().unwrap(); }
                    "--max" => { i += 1; max = args[i].parse().unwrap(); }
                    "--events" => { i += 1; events = Some(args[i].clone()); }
                    "--events-dir" => { i += 1; events_dir = Some(args[i].clone()); }
                    "--stop-at" => { i += 1; stop_at = args[i].parse().unwrap(); }
                    "--timeline" => { i += 1; tl_file = Some(args[i].clone()); }
                    "--timeline-dir" => { i += 1; tl_dir = Some(args[i].clone()); }
                    "--derail" => derail = true,
                    "--derail-col" => derail_col = true,
                    "--checkpoint-at" => { i += 1; ck_at = Some(args[i].clone()); }
                    "--margin" => { i += 1; margin = args[i].parse().unwrap(); }
                    "--save" => { i += 1; save = Some(args[i].clone()); }
                    "--save-dir" => { i += 1; save_dir = Some(args[i].clone()); }
                    o => out_path = Some(o.to_string()),
                }
                i += 1;
            }
            // a list file has one hash per line, optionally followed by the tick at which the Java run hit its wall limit
            let hashes: Vec<(String, i64)> = if std::path::Path::new(&args[2]).exists() {
                std::fs::read_to_string(&args[2]).unwrap().lines().filter_map(|l| { let mut f = l.split_whitespace(); f.next().map(|s| (s.to_string(), f.next().and_then(|v| v.parse().ok()).unwrap_or(0))) }).collect()
            } else { vec![(args[2].clone(), stop_at)] };
            let tooling = tl_file.is_some() || tl_dir.is_some() || derail || derail_col || ck_at.is_some();
            if ck_at.is_some() && save.is_none() && save_dir.is_none() { eprintln!("--checkpoint-at needs --save FILE or --save-dir DIR"); std::process::exit(2); }
            ckpt::set_enabled(tooling);
            let mut oracle = oracle::Oracle::new();
            let mut out_lines = Vec::new();
            for (h, stop) in hashes.iter().take(max) {
                let stop = if *stop > 0 { *stop } else { stop_at };
                let s = match schem::Schem::load(&schem::Schem::cache_file("/work/corpus", h)) { Ok(s) => s, Err(e) => { eprintln!("{h}: {e}"); continue; } };
                let ev_path = events_dir.as_ref().map(|d| format!("{d}/ev-{}.txt", &h[..8])).or_else(|| events.clone());
                let (o, orc) = build::run_until(&s, ticks, stop, verbose, oracle, ev_path.as_deref());
                oracle = orc;
                let mut line = o.tsv(h);
                if let Ok(dir) = std::env::var("MCSIM_CAUSES") { let hh: &str = h; let _ = std::fs::write(format!("{dir}/{hh}.causes"), o.causes.join("\n") + "\n"); }
                if let Some(tl) = o.timeline.as_ref() {
                    if let Some(p) = &tl_file { std::fs::write(p, tl.tsv()).unwrap(); }
                    if let Some(d) = &tl_dir { std::fs::write(format!("{d}/{h}.tl.tsv"), tl.tsv()).unwrap(); }
                    let d = ckpt::detect(tl);
                    // the checkpoint tick: `margin` ticks before the derailment (on the sampling grid), or the tick given
                    let ck_tick = match ck_at.as_deref() {
                        Some("derail") => d.as_ref().map(|d| ((d.tick - margin).max(ckpt::EVERY) / ckpt::EVERY) * ckpt::EVERY),
                        Some(v) => Some(v.parse::<i64>().expect("--checkpoint-at derail|TICK")),
                        None => d.as_ref().map(|d| ((d.tick - margin).max(ckpt::EVERY) / ckpt::EVERY) * ckpt::EVERY),
                    };
                    let fp = ck_tick.and_then(|t| tl.fp_at(t));
                    let dtxt = match &d { Some(d) => format!("{}\t{}\t{}", d.rule, d.tick, d.why), None => "none\t0\t".to_string() };
                    if derail || derail_col || ck_at.is_some() { eprintln!("{} derail: {} | checkpoint tick {} | ticks {}", &h[..8], dtxt.replace('\t', " "), ck_tick.unwrap_or(0), o.ticks); }
                    if ck_at.is_some() {
                        match (ck_tick, fp) {
                            (Some(t), Some(fp)) => {
                                let path = save.clone().unwrap_or_else(|| format!("{}/{h}.{t}.ckpt", save_dir.as_ref().unwrap()));
                                let ck = ckpt::Checkpoint { hash: h.clone(), rev: rev::rev().name.to_string(), tick: t, ticks_limit: ticks, stop_at: stop, env: ckpt::run_env(), fp,
                                    rule: d.as_ref().map(|d| d.rule.to_string()).unwrap_or_else(|| "tick".into()), derail_tick: d.as_ref().map(|d| d.tick).unwrap_or(0), row: line.clone() };
                                ck.write(&path).unwrap();
                                eprintln!("{} checkpoint saved: {path}", &h[..8]);
                            }
                            (Some(t), None) => eprintln!("{} no checkpoint: tick {t} is not a sample tick (multiple of {}) reached by the run", &h[..8], ckpt::EVERY),
                            _ => eprintln!("{} no checkpoint: no derailment detected", &h[..8]),
                        }
                    }
                    if derail_col { line += &format!("\t{}\t{}\t{:016x}", dtxt.replace('\t', "|"), ck_tick.unwrap_or(0), fp.unwrap_or(0)); }
                } else if derail_col { line += "\tnone|0|no timeline\t0\t0000000000000000"; }
                eprintln!("{} {} {} | {} | blocks={} ticks={} placed={} skipped={} missing={} wrong={} leftover={} trips={} scaffold={} left={} wall={:.0}ms cpu={:.0}ms rtf={:.0}x searches={} expanded={} nav={:.0}% unmodelled={} lost={} random={}{} {}",
                    &h[..8], o.dims, if o.judge.pass { "PASS" } else { "FAIL" }, o.status, o.target_blocks, o.ticks, o.placed, o.skipped, o.judge.missing, o.judge.wrong, o.judge.leftover,
                    o.trips, o.scaffold_placed, o.scaffold_left, o.wall_ms, o.cpu_ms, o.ticks as f64 / 20.0 / (o.cpu_ms / 1000.0).max(1e-9), o.searches, o.expanded, 100.0 * o.search_ms / o.wall_ms.max(1e-9), o.unmodelled, o.items_lost, o.rand_draws, if o.died { " DIED" } else { "" }, o.extra);
                out_lines.push(line);
                // keep the output file current, so that a long run can be read (or stopped) part-way
                if let Some(pth) = &out_path { std::fs::write(pth, out_lines.join("\n") + "\n").unwrap(); }
            }
            if bot::nav::callstat() { bot::nav::dump_callstat(); }
            bot::body::dump_walkstat();
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
        "impossible" => {
            // the cells whose target cannot exist in survival (build::impossible_cells): mcsim impossible <hash|list-file>
            // prints per build `I <hash> <class>=<n> ...` and one line per cell `E <hash> x y z <class> <block>`
            if args.len() < 3 { usage(); }
            let hs: Vec<String> = if std::path::Path::new(&args[2]).exists() { std::fs::read_to_string(&args[2]).unwrap().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect() } else { vec![args[2].clone()] };
            for h in hs {
                let s = match schem::Schem::load(&schem::Schem::cache_file("/work/corpus", &h)) { Ok(s) => s, Err(e) => { eprintln!("{h}: {e}"); continue; } };
                let tg = build::Target::new(&s);
                let cells = build::impossible_cells(&tg);
                let mut n: std::collections::BTreeMap<&str, usize> = Default::default();
                for &(_, c) in &cells { *n.entry(c).or_insert(0) += 1; }
                println!("I {h} blocks={} {}", tg.blocks, n.iter().map(|(k, v)| format!("{}={v}", k.replace(' ', "_"))).collect::<Vec<_>>().join(" "));
                for (i, c) in cells {
                    let i = i as i32; let (x, y, z) = (i % tg.sx, i / (tg.sx * tg.sz), (i / tg.sx) % tg.sz);
                    // (whether the schematic had a fluid next to the cell: the judged target drops fluids)
                    let tb = tables::t();
                    let fl = [(0, -1, 0), (0, 1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)].iter().any(|&(dx, dy, dz)| {
                        let (qx, qy, qz) = (x + dx, y + dy, z + dz);
                        if qx < 0 || qy < 0 || qz < 0 || qx >= s.sx || qy >= s.sy || qz >= s.sz { return false; }
                        let id = s.ids[((qy * s.sz + qz) * s.sx + qx) as usize] as usize;
                        id < tb.n && (tb.fluid[id] || tb.dewater[id] as usize != id)
                    });
                    println!("E {h} {x} {y} {z} {} {} {}", c.replace(' ', "_"), tb.name(tg.ids[i as usize]), if fl { "by_fluid" } else { "-" });
                }
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
