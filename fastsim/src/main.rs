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
        _ => panic!("unknown cost {name}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 { eprintln!("usage: fastsim prep <list> <out.fsb> | fastsim run <set.fsb> [--repeat N] [--only HASH] [--seed K] [--cost name=v,...] [--trace]"); std::process::exit(2); }
    match args[1].as_str() {
        "prep" => prep::prep(&args[2], &args[3], "/a/corpus", "/work/data/states.tsv"),
        "run" => {
            let data = std::fs::read(&args[2]).unwrap_or_else(|e| panic!("{}: {e}", args[2]));
            let (mut repeat, mut only, mut trace) = (1u32, String::new(), false);
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
                    "--trace" => trace = true,
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
                    let t1 = cpu_ns();
                    bt.run();
                    let j = world::judge(&bt.w);
                    let t2 = cpu_ns();
                    setup_ns += t1 - t0;
                    sim_ns += t2 - t1;
                    last = Some((bt.ticks, bt.status, bt.st.clone(), j, bt.nav.searches, bt.nav.expanded));
                }
                let (ticks, status, st, j, searches, expanded) = last.unwrap();
                let ns = (sim_ns + setup_ns) / repeat as u64;
                let rtf = ticks as f64 / 20.0 / (ns as f64 / 1e9);
                rtfs.push(rtf);
                all_ticks += ticks; all_ns += ns;
                writeln!(out, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t0\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}\tmain={};escapes={};helpers={};putOff={};fromHere={};self={};moves={};plans={};planFails={};searches={};expanded={};noWay={};outOfStock={};broken={};tripTicks={};moveTicks={};cleanTicks={};worst={:.3}",
                    b.hash, if j.pass { "PASS" } else { "FAIL" }, status, b.blocks, ticks, st.placed + st.scaffold_placed, st.skipped, j.missing, j.leftover, j.sections, j.failed,
                    st.trips, st.scaffold_placed, sim_ns / repeat as u64, setup_ns / repeat as u64, rtf,
                    st.main_ticks, st.escapes, st.helpers, st.put_off, st.from_here, st.self_placed, st.moves, st.plans, st.plan_fails, searches, expanded, st.no_way, st.out_of_stock, st.broken,
                    st.trip_ticks, st.move_ticks, st.cleanup_ticks, j.worst).unwrap();
            }
            rtfs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if !rtfs.is_empty() {
                eprintln!("builds={} median_rtf={:.0} p10={:.0} p90={:.0} aggregate_rtf={:.0} cpu_ms={:.1}", rtfs.len(), rtfs[rtfs.len() / 2], rtfs[rtfs.len() / 10], rtfs[rtfs.len() * 9 / 10],
                    all_ticks as f64 / 20.0 / (all_ns as f64 / 1e9), all_ns as f64 / 1e6);
            }
        }
        _ => { eprintln!("unknown command"); std::process::exit(2); }
    }
}
