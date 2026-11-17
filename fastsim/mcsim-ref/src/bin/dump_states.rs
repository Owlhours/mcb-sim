//! fastsim tooling: one line per block state with the facts fastsim's coarse model needs (not part of A's simulator).
use mcsim::kinds::Kind;
use mcsim::tables::{self, t};
fn main() {
    tables::init("/a/reference", "/a/data");
    let tb = t();
    let mut oracle = mcsim::oracle::Oracle::new();
    println!("id\tblock\tflags\tdewater\tcanon\tloose\titem\titem_count\tstack\tbreak_ticks\tcol_top\tkind");
    for id in 0..tb.n {
        let b = tb.binfo(id as u16);
        let gm = matches!(b.kind, Kind::CommandBlock | Kind::GameMasterBlock | Kind::JigsawBlock) || b.name == "test_block";
        let item = tb.item[id];
        let mut placeable = false;
        if item != 0 && tb.item_count[id] != 0 {
            let table = oracle.table(item);
            // (as Bot::spec, without the multi / potted "first" refinements: the loose class is the same)
            let want = tb.loose[id];
            placeable = table.iter().any(|&e| e != 0 && tb.loose[e as usize] == want) || b.name == "bamboo" || b.name.starts_with("potted_");
        }
        let h = tb.destroy_time[b.default as usize];
        let brk: i32 = if h < 0.0 { -1 } else { ((20.0f32 + h * (if tb.requires_tool[id] { 100.0 } else { 30.0 }) * 1.2f32).min(6000.0f32)) as i32 };
        let bits = [tb.air[id], tb.fluid[id], tb.technical[id], tb.gravity[id], tb.replaceable[id], tb.no_collision[id], tb.full_cube[id], tb.sturdy_top[id],
            tb.leaves[id], tb.secondary[id], tb.late[id], tb.scaffolding[id], tb.walk_through[id], gm, placeable, tb.interactive[id], tb.climbable[id]];
        let mut flags = 0u32;
        for (k, &v) in bits.iter().enumerate() { if v { flags |= 1 << k; } }
        let stack = if item != 0 { tb.items[item as usize].stack } else { 0 };
        println!("{id}\t{}\t{flags}\t{}\t{}\t{}\t{item}\t{}\t{stack}\t{brk}\t{}\t{:?}", b.name, tb.dewater[id], tb.canon[id], tb.loose[id], tb.item_count[id], tb.col_top[id], b.kind);
    }
}
