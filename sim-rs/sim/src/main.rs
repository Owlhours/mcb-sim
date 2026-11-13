use mcsim::*;

fn usage() -> ! {
    eprintln!("usage: mcsim run <hash|list-file> [out.tsv] [--verbose] [--ticks N] [--max N]\n       mcsim selfcheck");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { usage(); }
    let t0 = std::time::Instant::now();
    tables::init("/work/reference", "/work/data");
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    // the time limit is a panic payload: keep it quiet
    std::panic::set_hook(Box::new(|info| {
        if info.payload().downcast_ref::<bot::OutOfTime>().is_none() { eprintln!("panic: {info}"); }
    }));
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
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--verbose" => verbose = true,
                    "--ticks" => { i += 1; ticks = args[i].parse().unwrap(); }
                    "--max" => { i += 1; max = args[i].parse().unwrap(); }
                    o => out_path = Some(o.to_string()),
                }
                i += 1;
            }
            let hashes: Vec<String> = if std::path::Path::new(&args[2]).exists() {
                std::fs::read_to_string(&args[2]).unwrap().lines().filter_map(|l| l.split_whitespace().next().map(|s| s.to_string())).collect()
            } else { vec![args[2].clone()] };
            let mut oracle = oracle::Oracle::new();
            let mut out_lines = Vec::new();
            for h in hashes.iter().take(max) {
                let s = match schem::Schem::load(&schem::Schem::cache_file("/corpus", h)) { Ok(s) => s, Err(e) => { eprintln!("{h}: {e}"); continue; } };
                let (o, orc) = build::run(&s, ticks, verbose, oracle);
                oracle = orc;
                let line = o.tsv(h);
                eprintln!("{} {} {} | {} | blocks={} ticks={} placed={} skipped={} missing={} wrong={} leftover={} trips={} scaffold={} left={} wall={:.0}ms rtf={:.0}x searches={} expanded={} nav={:.0}% unmodelled={} lost={}",
                    &h[..8], o.dims, if o.judge.pass { "PASS" } else { "FAIL" }, o.status, o.target_blocks, o.ticks, o.placed, o.skipped, o.judge.missing, o.judge.wrong, o.judge.leftover,
                    o.trips, o.scaffold_placed, o.scaffold_left, o.wall_ms, o.ticks as f64 / 20.0 / (o.wall_ms / 1000.0).max(1e-9), o.searches, o.expanded, 100.0 * o.search_ms / o.wall_ms.max(1e-9), o.unmodelled, o.items_lost);
                out_lines.push(line);
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
                let s = schem::Schem::load(&schem::Schem::cache_file("/corpus", &h)).unwrap();
                let tg = build::Target::new(&s);
                let j = build::judge(&tg, &vec![0u16; tg.ids.len()]);
                println!("{h}\t{}\t{}\t{}", tg.blocks, j.sections, j.exempt_cells);
            }
        }
        _ => usage(),
    }
}
