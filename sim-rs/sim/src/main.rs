use mcsim::*;

fn main() {
    let t0 = std::time::Instant::now();
    tables::init("/work/reference", "/work/data");
    let tb = tables::t();
    eprintln!("tables: {} states, {} blocks, {} shapes in {:?}", tb.n, tb.blocks.len(), tb.shapes.len(), t0.elapsed());
    // self-check of the shape code against the reference table
    let mut bad = 0;
    for s in 0..tb.n {
        let sh = &tb.shapes[tb.col_shape[s] as usize];
        if sh.full != tb.full_cube[s] || sh.empty != tb.no_collision[s] { bad += 1; }
        let st = rules::face_sturdy(s as u16, geom::UP, 0);
        if st != tb.sturdy_top[s] { bad += 1; if bad < 40 { eprintln!("sturdyTop differs: {} table={} computed={}", tb.name(s as u16), tb.sturdy_top[s], st); } }
    }
    eprintln!("shape self-check: {bad} differences");
    let _ = world::World::new(0, 0, 0, 4, 4, 4, 0, 319);
}
