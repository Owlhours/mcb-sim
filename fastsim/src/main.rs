//! fastsim: a speed-first simulator of the schematic-building bot. See /work/STATUS.md.
mod bot;
mod mcb;
mod nav;
mod prep;
mod world;

use std::io::Write;

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
extern "C" { fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32; }
/// CPU time of this thread, ns (CLOCK_THREAD_CPUTIME_ID).
fn cpu_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(3, &mut ts); }
    ts.sec as u64 * 1_000_000_000 + ts.nsec as u64
}

fn set_cost(k: &mut nav::Costs, t: &mut nav::Times, name: &str, v: u32) {
    match name {
        "walk" => k.walk = v, "jump" => k.jump = v, "drop" => k.drop = v, "fall2" => k.fall2 = v, "tower" => k.tower = v, "climb" => k.climb = v,
        "descend" => k.descend = v, "bridge" => k.bridge = v, "mine" => k.mine = v,
        "t_walk" => t.mv[0] = v, "t_jump" => t.mv[1] = v, "t_drop" => t.mv[2] = v, "t_tower" => t.mv[3] = v, "t_climb" => t.mv[4] = v, "t_descend" => t.mv[5] = v,
        "t_bridge" => t.mv[6] = v, "t_mine" => t.mv[7] = v, "t_route" => t.route = v, "t_arrive" => t.arrive = v, "t_place" => t.place = v,
        "t_self_place" => t.self_place = v, "t_lean" => t.lean = v, "t_break_loose" => t.break_loose = v, "t_break_tower" => t.break_tower = v,
        "t_chest_open" => t.chest_open = v, "t_chest_stack" => t.chest_stack = v,
        "t_wfail" => t.wfail = v, "t_wfail_pct" => t.wfail_pct = v, "t_wfail_lean_pct" => t.wfail_lean_pct = v, "t_wfail_clean" => t.wfail_clean = v,
        "t_cfail" => t.cfail = v, "t_cfail_after" => t.cfail_after = v,
        _ => panic!("unknown cost {name}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 { eprintln!("usage: fastsim prep <list> <out.fsb> | fastsim run <set.fsb> [--repeat N] [--only HASH] [--seed K] [--cost name=v,...] [--trace]"); std::process::exit(2); }
    match args[1].as_str() {
        "prep" => prep::prep(&args[2], &args[3], "/a/corpus", "/work/data/states.tsv"),
        "kinds" => prep::kinds(&args[2], "/a/corpus", "/work/data/states.tsv"),
        "run" => {
            let data = std::fs::read(&args[2]).unwrap_or_else(|e| panic!("{}: {e}", args[2]));
            let (mut repeat, mut only, mut trace) = (1u32, String::new(), false);
            let mut weight = 16u32;
            // --level 2: fastsim (level 1) screens each build; a flagged build is run again in A's simulator (the copy in
            // ref/mcsim-src, linked in) and its row is A's. --level 3: every build is run in A's simulator.
            // --screen bad (default): flagged = fails or has a bad cell; --screen virtual: also any route from the cost table.
            let (mut exact, mut screen_virtual) = (0u32, false);
            let (mut n_exact, mut exact_ns, mut exact_ticks) = (0u32, 0u64, 0u64);
            let mut oracle = None;
            let mut pol = bot::Policy::default();
            let mut costs = nav::Costs::default();
            let mut times = nav::Times::default();
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--repeat" => { repeat = args[i + 1].parse().unwrap(); i += 1; }
                    "--only" => { only = args[i + 1].clone(); i += 1; }
                    "--seed" => { pol.seed = args[i + 1].parse().unwrap(); i += 1; }
                    "--cost" => { for kv in args[i + 1].split(',') { let (k, v) = kv.split_once('=').unwrap(); set_cost(&mut costs, &mut times, k, v.parse().unwrap()); } i += 1; }
                    "--wide-retry" => pol.wide_retry = true,
                    "--no-sweep" => pol.sweep = false,
                    "--no-fail" => pol.fail = false,
                    "--centre-reach" => pol.face_reach = false,
                    "--sweep-waiting" => pol.sweep_all = false,
                    "--no-forced-mine" => pol.forced_mine = false,
                    "--any-lean" => pol.lean_full = false,
                    "--keep-bridges" => pol.rope = false,
                    "--charge-climb" => pol.free_climb = false,
                    "--strips" => { pol.strips = args[i + 1].parse().unwrap(); i += 1; }
                    "--strip-tick10" => { pol.strip_tick10 = args[i + 1].parse().unwrap(); i += 1; }
                    "--strip-min" => { pol.strip_min = args[i + 1].parse().unwrap(); i += 1; }
                    "--strip-no-floor" => pol.strip_floor = false,
                    "--ledger" => pol.ledger = true,
                    "--any-floor" => pol.sturdy_floor = false,
                    "--no-thin" => pol.thin_below = false,
                    "--chain-retry" => pol.chain_retry = true,
                    "--trace" => trace = true,
                    "--level" => { exact = args[i + 1].parse().unwrap(); i += 1; pol.level = exact.min(1); if exact >= 2 { pol.strict_helpers = true; } }
                    "--screen" => { screen_virtual = match args[i + 1].as_str() { "bad" => false, "virtual" => true, o => panic!("--screen bad|virtual, not {o}") }; i += 1; }
                    "--a-rev" => { mcsim::rev::set(&args[i + 1]); i += 1; }
                    "--strict-helpers" => pol.strict_helpers = true,
                    "--no-virtual" => pol.virtual_routes = false,
                    "--no-fetch-scaffold" => pol.fetch_scaffold = false,
                    "--slots" => pol.slots = true,
                    "--a-routes" => { pol.a_routes = args[i + 1].parse().unwrap(); i += 1; }
                    "--a-place" => { pol.a_place = args[i + 1].parse().unwrap(); i += 1; }
                    "--no-orphan" => pol.orphan = false,
                    "--no-close" => pol.close = false,
                    "--no-chain" => pol.chain = false,
                    "--shut-in" => pol.shut_in = true,
                    "--no-escape" => pol.escape = false,
                    "--pick-dy" => { pol.pick_dy = args[i + 1].parse().unwrap(); i += 1; }
                    "--pick-uncarried" => { pol.pick_uncarried = args[i + 1].parse().unwrap(); i += 1; }
                    "--no-self-place" => pol.self_place = false,
                    "--pick-near" => { pol.pick_near = args[i + 1].parse().unwrap(); i += 1; }
                    "--weight" => { weight = args[i + 1].parse().unwrap(); i += 1; }
                    "--budget" => { let (a, b) = args[i + 1].split_once(',').unwrap(); pol.search_base = a.parse().unwrap(); pol.search_per_cell = b.parse().unwrap(); i += 1; }
                    a => panic!("unknown argument {a}"),
                }
                i += 1;
            }
            let mut rd = prep::Reader { d: &data, p: 0 };
            let n = rd.header();
            let out = std::io::stdout();
            let mut out = std::io::BufWriter::new(out.lock());
            let mut rtfs: Vec<f64> = Vec::new();
            let (mut all_ticks, mut all_ns) = (0u64, 0u64);
            for _ in 0..n {
                let b = rd.build();
                if !only.is_empty() && !b.hash.starts_with(&only) { continue; }
                let (mut sim_ns, mut setup_ns) = (0u64, 0u64);
                let mut last = None;
                for _ in 0..repeat {
                    let t0 = cpu_ns();
                    let mut bt = bot::Bot::new(&b, pol.clone(), costs.clone(), times.clone());
                    bt.trace = trace;
                    if pol.a_routes | pol.a_place != 0 {
                        mcsim::tables::init("/a/reference", "/a/data");
                        let s = mcsim::schem::Schem::load(&mcsim::schem::Schem::cache_file("/a/corpus", &b.hash)).unwrap_or_else(|e| panic!("{}: {e}", b.hash));
                        bt.oracle = mcsim::fsoracle::RouteOracle::new(&s);
                    }
                    bt.nav.weight = weight;
                    if std::env::var_os("FS_MEM").is_some() {
                        // bytes the run works on: planes, judge words, palette indices (and the 64-byte lines of them that hold a target cell), per-column arrays, palette, click tables
                        let ncol = bt.w.nx * bt.w.nz;
                        let lines = b.packed.chunks(64).filter(|l| l.iter().any(|&v| v != 0)).count() * 64;
                        eprintln!("MEM {} blocks {} cols {} planes {} judged {} cells {} cellLines {} bits {} percol {} pal {} tables {}", &b.hash[..8], b.blocks, ncol, bt.w.pl.len() * 8, bt.w.judged.len() * 8, b.packed.len(), lines, 8 << b.lb,
                            ncol * 15 + (ncol / 8) * 10, b.pal.len() * std::mem::size_of::<prep::Pal>(), b.tables.len());
                    }
                    let t1 = cpu_ns();
                    bt.run();
                    if std::env::var_os("FS_SCSTAT").is_some() { let t = bt.st.scaffold_placed; eprintln!("SC {} total {} tableTower {} tower {} bridge {} helper {} feet {} mainEnd {} inCleanup {} tripMaterial {} tripLoose {} tripRoute {} mv {:?} cmp {:?}", &b.hash[..8], t, bt.st.sc[0], bt.st.sc[1], bt.st.sc[2], t - bt.st.sc[0] - bt.st.sc[1] - bt.st.sc[2], bt.st.sc[4], bt.st.ph[0], bt.st.ph[1], bt.st.tw[0], bt.st.tw[1], bt.st.tw[2], bt.st.mvh, bt.st.cmp); }
                    if std::env::var_os("FS_MPSTAT").is_some() { eprintln!("MP {} {} {:?} rope={},{}", &b.hash[..8], bt.st.main_ticks, bt.st.mp, bt.st.rope_undone, bt.st.rope_ticks); }
                    if std::env::var_os("FS_PHSTAT").is_some() { eprintln!("PH {} {},{} swept={} walkFails={} clickFails={} failTicks={} moves={} walkTries={} clickTries={} {}", &b.hash[..8], bt.st.main_ticks, bt.ticks, bt.st.swept, bt.st.walk_fails, bt.st.click_fails, bt.st.fail_ticks, bt.st.moves, bt.st.walk_tries, bt.st.click_tries, bt.phlog.iter().enumerate().map(|(i, p)| format!("{}{}:{},{},{},{},{}", p.0, i, p.1, p.2, p.3, p.4, p.5)).collect::<Vec<_>>().join(" ")); }
                    if let Some(o) = &bt.oracle { if std::env::var_os("FS_AWHY").is_some() { eprintln!("{} A answers: found {} goal-unstandable {} dry<=64 {} dry {} budget {}", &b.hash[..8], o.why[0], o.why[1], o.why[2], o.why[3], o.why[4]); } }
                    let j = world::judge(&bt.w);
                    let t2 = cpu_ns();
                    setup_ns += t1 - t0;
                    sim_ns += t2 - t1;
                    last = Some((bt.ticks, bt.status, bt.st.clone(), j, bt.nav.searches, bt.nav.expanded, bt.nav.fails, bt.nav.expanded_fail));
                }
                let (ticks, status, st, j, searches, expanded, fails, expanded_fail) = last.unwrap();
                let ns = (sim_ns + setup_ns) / repeat as u64;
                if exact >= 3 || (exact == 2 && (!j.pass || j.missing + j.leftover > 0 || (screen_virtual && st.virtual_routes > 0))) {
                    if oracle.is_none() { mcsim::tables::init("/a/reference", "/a/data"); oracle = Some(mcsim::oracle::Oracle::new()); }
                    let t0 = cpu_ns();
                    let s = mcsim::schem::Schem::load(&mcsim::schem::Schem::cache_file("/a/corpus", &b.hash)).unwrap_or_else(|e| panic!("{}: {e}", b.hash));
                    let (o, orc) = mcsim::build::run_until(&s, mcsim::build::TICK_LIMIT, 0, false, oracle.take().unwrap(), None);
                    oracle = Some(orc);
                    let ens = cpu_ns() - t0;
                    let tot = ns + ens;
                    let rtf = o.ticks as f64 / 20.0 / (tot as f64 / 1e9);
                    rtfs.push(rtf); all_ticks += o.ticks as u64; all_ns += tot; n_exact += 1; exact_ns += ens; exact_ticks += o.ticks as u64;
                    writeln!(out, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}\texact=1;screenNs={};screenPass={};screenBad={};screenVirtual={}",
                        b.hash, if o.judge.pass { "PASS" } else { "FAIL" }, o.status, o.target_blocks, o.ticks, o.placed, o.skipped, o.judge.missing, o.judge.wrong, o.judge.leftover, o.judge.sections,
                        o.judge.failed_sections, o.trips, o.scaffold_placed, ens, ns, rtf, ns, j.pass as u8, j.missing + j.leftover, st.virtual_routes).unwrap();
                    continue;
                }
                let rtf = ticks as f64 / 20.0 / (ns as f64 / 1e9);
                rtfs.push(rtf);
                all_ticks += ticks; all_ns += ns;
                writeln!(out, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t0\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}\tmain={};shut={};picks={};forced={};virtual={};chained={};helpers={};putOff={};fromHere={};self={};moves={};plans={};planFails={};searches={};expanded={};fails={};expandedFail={};noWay={};outOfStock={};broken={};tripTicks={};moveTicks={};cleanTicks={};leftLoose={};worst={:.3};aAsked={};aNo={};aUnknown={};aMined={};aNs={};pAsked={};pNo={};pUnknown={}",
                    b.hash, if j.pass { "PASS" } else { "FAIL" }, status, b.blocks, ticks, st.placed + st.scaffold_placed, st.skipped, j.missing, j.leftover, j.sections, j.failed,
                    st.trips, st.scaffold_placed, sim_ns / repeat as u64, setup_ns / repeat as u64, rtf,
                    st.main_ticks, st.shut, st.picks, st.forced, st.virtual_routes, st.chained, st.helpers, st.put_off, st.from_here, st.self_placed, st.moves, st.plans, st.plan_fails, searches, expanded, fails, expanded_fail, st.no_way, st.out_of_stock, st.broken,
                    st.trip_ticks, st.move_ticks, st.cleanup_ticks, j.left_loose, j.worst, st.a_asked, st.a_no, st.a_unknown, st.a_mined, st.a_ns, st.p_asked, st.p_no, st.p_unknown).unwrap();
                if pol.ledger || pol.strips > 0 { eprintln!("LED\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", b.hash, if j.pass { "PASS" } else { "FAIL" }, status, b.blocks, ticks, st.main_ticks, st.led[0][0], st.led[0][1], st.led[1][0], st.led[1][1], st.led[2][0], st.led[2][1], st.strip_runs, st.strip_walk, st.ph[0], st.cleanup_ticks); }
            }
            rtfs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if !rtfs.is_empty() {
                eprintln!("builds={} median_rtf={:.0} p10={:.0} p90={:.0} aggregate_rtf={:.0} cpu_ms={:.1}", rtfs.len(), rtfs[rtfs.len() / 2], rtfs[rtfs.len() / 10], rtfs[rtfs.len() * 9 / 10],
                    all_ticks as f64 / 20.0 / (all_ns as f64 / 1e9), all_ns as f64 / 1e6);
                if exact >= 2 { eprintln!("exact: {} of {} builds, {:.1} % of the CPU time, {:.1} % of the game time", n_exact, rtfs.len(), 100.0 * exact_ns as f64 / all_ns.max(1) as f64, 100.0 * exact_ticks as f64 / all_ticks.max(1) as f64); }
            }
        }
        "replay" => {
            // replay <set.fsb> <dir with A's event logs <hash>.txt>: A's questions asked of fastsim on A's states
            let data = std::fs::read(&args[2]).unwrap_or_else(|e| panic!("{}: {e}", args[2]));
            let mut rd = prep::Reader { d: &data, p: 0 };
            let n = rd.header();
            let mut tot = [[0u64; 4]; 2];
            let mut q2: std::collections::BTreeMap<String, u64> = Default::default();
            for _ in 0..n {
                let b = rd.build();
                let Ok(log) = std::fs::read_to_string(format!("{}/{}.txt", args[3], b.hash)) else { continue };
                let mut pol = bot::Policy::default();
                if args.len() > 5 && args[4] == "--level" { pol.level = args[5].parse().unwrap(); }
                if args.iter().any(|a| a == "--strict-helpers") { pol.strict_helpers = true; }
                if args.iter().any(|a| a == "--no-virtual") { pol.virtual_routes = false; }
                let mut bt = bot::Bot::new(&b, pol, nav::Costs::default(), nav::Times::default());
                bt.trace = args.iter().any(|a| a == "--trace");
                let r = bt.replay(&log);
                println!("{}\t{}\t{:?}\t{:?}", b.hash, b.blocks, r[0], r[1]);
                for q in 0..2 { for k in 0..4 { tot[q][k] += r[q][k]; } }
                for (k, v) in &bt.q2 { *q2.entry(k.clone()).or_insert(0) += v; }
            }
            let pc = |a: u64, b: u64| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
            eprintln!("Q1 A placed it from its stance; fastsim can click from there: plain {}/{} ({:.1} %), oriented {}/{} ({:.1} %)", tot[0][1], tot[0][0], pc(tot[0][1], tot[0][0]), tot[0][3], tot[0][2], pc(tot[0][3], tot[0][2]));
            let mut v: Vec<_> = q2.iter().collect(); v.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in v.iter().take(60) { eprintln!("  Q2 {n:6} {k}"); }
            eprintln!("Q2 A gave up (no way to place); fastsim finds a way: plain {}/{} ({:.1} %), oriented {}/{} ({:.1} %)", tot[1][1], tot[1][0], pc(tot[1][1], tot[1][0]), tot[1][3], tot[1][2], pc(tot[1][3], tot[1][2]));
        }
        _ => { eprintln!("unknown command"); std::process::exit(2); }
    }
}
