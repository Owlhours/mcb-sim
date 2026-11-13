use mcsim::*;
fn main() {
    tables::init("/work/reference", "/work/data");
    let tb = tables::t();
    let g = tb.default_of("grass_block") as usize;
    println!("grass: walk={} colTop={} anyTop={} full={} | air walk={} colTop={}", tb.walk_through[g], tb.col_top[g], tb.any_top[g], tb.full_cube[g], tb.walk_through[0], tb.col_top[0]);
    let w = world::World::new(50, -61, 50, 30, 12, 30, -61, 319);
    let mut s = sim::Sim::new(w, 64.5, -60.0, 62.5);
    for i in 0..5 { s.tick(); println!("tick {i}: pos {:?} vel {:?} ground {}", s.pl.pos, s.pl.vel, s.pl.on_ground); }
    s.look(180.0, 0.0);
    s.keys(true, false, false, false, false, false, false);
    for i in 0..8 { s.tick(); println!("walk {i}: pos {:?} vel {:?} ground {}", s.pl.pos, s.pl.vel, s.pl.on_ground); }
}
