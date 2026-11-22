//! fastsim tooling: A's placement tables as bit tables, one per block state whose table is not "anything goes".
//! Record: u16 state id (LE), then TABLE_SIZE bits (1728 bytes); bit `oracle::index(face, quad, yaw class, pitch class,
//! sneak)` is set when that click on a lone support block gives the state (up to what the judge ignores).
use mcsim::oracle::TABLE_SIZE;
use mcsim::tables::{self, t};
use std::io::Write;
fn main() {
    tables::init(&mcsim::paths::reference(), &mcsim::paths::data());
    let tb = t();
    let mut oracle = mcsim::oracle::Oracle::new();
    let mut out = std::io::BufWriter::new(std::fs::File::create(std::env::args().nth(1).expect("output file")).unwrap());
    let mut n = 0;
    for id in 0..tb.n {
        let item = tb.item[id];
        let b = tb.binfo(id as u16);
        if item == 0 || tb.item_count[id] != 1 || b.name == "bamboo" || b.name.starts_with("potted_") { continue; }
        let table = oracle.table(item);
        let want = tb.loose[id];
        let mut bits = vec![0u8; TABLE_SIZE / 8];
        let mut cnt = 0;
        for (k, &e) in table.iter().enumerate() { if e != 0 && tb.loose[e as usize] == want { bits[k >> 3] |= 1 << (k & 7); cnt += 1; } }
        if cnt == 0 || cnt == TABLE_SIZE { continue; }
        out.write_all(&(id as u16).to_le_bytes()).unwrap();
        out.write_all(&bits).unwrap();
        n += 1;
    }
    eprintln!("{n} states with a table");
}
