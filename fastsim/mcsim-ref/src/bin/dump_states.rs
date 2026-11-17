//! fastsim tooling: one line per block state with the facts fastsim's coarse model needs (not part of A's simulator).
use mcsim::kinds::Kind;
use mcsim::tables::{self, t};
fn main() {
    tables::init("/a/reference", "/a/data");
    let tb = t();
    let mut oracle = mcsim::oracle::Oracle::new();
    println!("id\tblock\tflags\tdewater\tcanon\tloose\titem\titem_count\tstack\tbreak_ticks\tcol_top\tkind\tclick_mask");
    for id in 0..tb.n {
        let b = tb.binfo(id as u16);
        let gm = matches!(b.kind, Kind::CommandBlock | Kind::GameMasterBlock | Kind::JigsawBlock) || b.name == "test_block";
        let item = tb.item[id];
        let mut placeable = false;
        // click_mask: bit (face * 8 + yaw octant) set when a click on that face of a support block, looking that way
        // (any part of the face, any pitch, not sneaking), gives this state up to what the judge ignores
        let mut mask: u64 = 0;
        if item != 0 && tb.item_count[id] != 0 {
            let table = oracle.table(item);
            // (as Bot::spec, without the multi / potted "first" refinements: the loose class is the same)
            let want = tb.loose[id];
            // (blocks made of several items - double slabs, candles, layers - start as their one-item state: Bot::spec's `first`)
            placeable = table.iter().any(|&e| e != 0 && tb.loose[e as usize] == want) || b.name == "bamboo" || b.name.starts_with("potted_")
                || (tb.item_count[id] > 1 && table.iter().any(|&e| e != 0 && tb.block[e as usize] == tb.block[id]));
            let free = tb.item_count[id] > 1 || b.name == "bamboo" || b.name.starts_with("potted_");
            for face in 0..6 { for yc in 0..mcsim::oracle::NYAW {
                let mut ok = free;
                for quad in 0..4 { for pc in 0..mcsim::oracle::NPITCH {
                    let e = table[mcsim::oracle::index(face, quad, yc, pc, false)];
                    if e != 0 && tb.loose[e as usize] == want { ok = true; }
                } }
                if ok { mask |= 1u64 << (face * 8 + yc / 4); }
            } }
        }
        let h = tb.destroy_time[b.default as usize];
        let brk: i32 = if h < 0.0 { -1 } else { ((20.0f32 + h * (if tb.requires_tool[id] { 100.0 } else { 30.0 }) * 1.2f32).min(6000.0f32)) as i32 };
        let bits = [tb.air[id], tb.fluid[id], tb.technical[id], tb.gravity[id], tb.replaceable[id], tb.no_collision[id], tb.full_cube[id], tb.sturdy_top[id],
            tb.leaves[id], tb.secondary[id], tb.late[id], tb.scaffolding[id], tb.walk_through[id], gm, placeable, tb.interactive[id], tb.climbable[id],
            !tb.air[id] && !tb.replaceable[id] && !tb.out_boxes_f32[id].is_empty(),
            // (bit 18) stays where it is with nothing around it: A lends a helper block only to these
            oracle.floats(id as u16)];
        let mut flags = 0u32;
        for (k, &v) in bits.iter().enumerate() { if v { flags |= 1 << k; } }
        let stack = if item != 0 { tb.items[item as usize].stack } else { 0 };
        println!("{id}\t{}\t{flags}\t{}\t{}\t{}\t{item}\t{}\t{stack}\t{brk}\t{}\t{:?}\t{mask}", b.name, tb.dewater[id], tb.canon[id], tb.loose[id], tb.item_count[id], tb.col_top[id], b.kind);
    }
}
