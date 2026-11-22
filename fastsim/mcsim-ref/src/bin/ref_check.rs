//! Differential check of the placement rules and state facts against the Java version's dumps
//! (reference/java-extra: placement tables, floats, states-extra.tsv). usage: ref_check [item-name-to-detail]
use mcsim::*;
use std::collections::BTreeMap;

fn main() {
    tables::init(&mcsim::paths::reference(), &mcsim::paths::data());
    let tb = tables::t();
    let detail: Option<String> = std::env::args().nth(1);
    let raw = std::fs::read(format!("{}/extra/tables.bin", mcsim::paths::tmp())).expect("gunzip work/reference/java-extra/placement-tables.bin.gz to work/tmp/extra/tables.bin first");
    let idx = std::fs::read_to_string(format!("{}/java-extra/placement-tables.index.tsv", mcsim::paths::reference())).unwrap();
    let mut orc = oracle::Oracle::new();
    let (mut items_ok, mut items_bad, mut cells_bad) = (0, 0, 0u64);
    let mut worst: Vec<(u64, String, String)> = Vec::new();
    for l in idx.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        let k: usize = f[0].parse().unwrap();
        if f[2] != "1" { continue; }
        let Some(&item) = tb.item_by_name.get(f[1]) else { println!("item {} unknown to the simulator", f[1]); continue };
        let mine = orc.table(item);
        let base = k * oracle::TABLE_SIZE * 2;
        let mut bad = 0u64;
        let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
        for e in 0..oracle::TABLE_SIZE {
            let r = u16::from_be_bytes([raw[base + e * 2], raw[base + e * 2 + 1]]);
            if r != mine[e] {
                bad += 1;
                let key = format!("{} -> ref {} / sim {}", describe(e), if r == 0 { "-".into() } else { tb.name(r) }, if mine[e] == 0 { "-".into() } else { tb.name(mine[e]) });
                if detail.as_deref() == Some(f[1]) { *kinds.entry(key).or_insert(0) += 1; }
                else if bad == 1 { kinds.insert(key, 1); }
            }
        }
        if bad == 0 { items_ok += 1; } else {
            items_bad += 1; cells_bad += bad;
            let ex = kinds.iter().next().map(|(k, _)| k.clone()).unwrap_or_default();
            worst.push((bad, f[1].to_string(), ex));
            if detail.as_deref() == Some(f[1]) { for (k, v) in kinds.iter().take(60) { println!("  {v:6}  {k}"); } }
        }
    }
    worst.sort_by(|a, b| b.0.cmp(&a.0));
    println!("placement tables: {items_ok} items identical, {items_bad} differ ({cells_bad} entries)");
    for (n, name, ex) in worst.iter().take(60) { println!("  {n:6} {name}: e.g. {ex}"); }
    // floats
    let fl = std::fs::read(format!("{}/extra/floats.bin", mcsim::paths::tmp())).unwrap();
    let mut by_block: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for s in 0..tb.n {
        let mine = orc.floats(s as u16);
        if mine != (fl[s] == 1) { let e = by_block.entry(tb.binfo(s as u16).name.clone()).or_insert((0, 0)); if mine { e.0 += 1; } else { e.1 += 1; } }
    }
    println!("floats: {} blocks differ (block: states where sim says floats but ref not / the reverse)", by_block.len());
    for (b, (a, c)) in by_block.iter().take(80) { println!("  {b}: {a} / {c}"); }
    // states-extra
    let ex = std::fs::read_to_string(format!("{}/java-extra/states-extra.tsv", mcsim::paths::reference())).unwrap();
    let mut lines = ex.lines();
    let hdr: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let col = |n: &str| hdr.iter().position(|x| *x == n).unwrap();
    let (c_cond, c_suf, c_def) = (col("isRedstoneConductor"), col("isSuffocating"), col("defaultStateId"));
    let c_st = col("sturdy_down_FULL");
    let mut diffs: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for (s, l) in lines.enumerate() {
        let f: Vec<&str> = l.split('\t').collect();
        let name = tb.binfo(s as u16).name.clone();
        let mut note = |what: &str| { *diffs.entry(what.to_string()).or_default().entry(name.clone()).or_insert(0) += 1; };
        if f[c_def].parse::<u16>().ok() != Some(tb.binfo(s as u16).default) { note("defaultState"); }
        if f[c_suf] != "E" && (f[c_suf] == "1") != physics::is_suffocating(s as u16) { note("isSuffocating"); }
        if f[c_cond] != "E" && (f[c_cond] == "1") != rules::place::is_redstone_conductor(s as u16) { note("isRedstoneConductor(place)"); }
        for d in 0..6 { for k in 0..3 {
            let v = f[c_st + d * 3 + k];
            if v != "E" && (v == "1") != rules::face_sturdy(s as u16, d as u8, k as u8) { note(&format!("sturdy_{}_{}", ["down", "up", "north", "south", "west", "east"][d], ["FULL", "CENTER", "RIGID"][k])); }
        } }
    }
    for (what, m) in &diffs {
        let n: u32 = m.values().sum();
        println!("{what}: {n} states differ in {} blocks: {}", m.len(), m.iter().take(12).map(|(k, v)| format!("{k}({v})")).collect::<Vec<_>>().join(" "));
    }
    if diffs.is_empty() { println!("states-extra: defaultState, isSuffocating, isRedstoneConductor, isFaceSturdy all agree"); }
}

fn describe(e: usize) -> String {
    let sneak = e & 1;
    let pc = (e >> 1) % 9;
    let yc = (e / 18) % 32;
    let quad = (e / (18 * 32)) % 4;
    let face = e / (18 * 32 * 4);
    format!("face {} quad {} yaw {:.1} pitch {} sneak {}", ["down", "up", "north", "south", "west", "east"][face], quad, oracle::yaw_centre(yc), oracle::pitch_centre(pc), sneak)
}
