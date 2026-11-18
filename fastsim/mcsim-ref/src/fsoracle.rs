//! For fastsim (the /work project; not part of A's simulator): A's bot set up for a build but never run, used as a
//! route oracle. fastsim mirrors its world into the bot's picture (`set`) and asks A's own route search (`route`).
use crate::bot::state::Bot;
use crate::schem::Schem;

thread_local! { pub static SETUP_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; pub static STASH: std::cell::RefCell<Option<Bot>> = const { std::cell::RefCell::new(None) }; }

pub struct RouteOracle { pub bot: Bot, pub searches: u64, pub expanded: u64, pub why: [u64; 6] }

/// What `route` says: 0 = A's search finds no way, 1 = it finds one, 2 = the question cannot be put (a cell outside
/// A's picture, or a start A's bot could not stand on).
pub struct Answer { pub kind: u8, pub exhausted: bool, pub expanded: i32, pub cost: i32, pub scaffold_moves: i32, pub tower_moves: i32, pub length: usize }

impl RouteOracle {
    pub fn new(schem: &Schem) -> Option<RouteOracle> {
        SETUP_ONLY.with(|f| f.set(true));
        let _ = crate::build::run_until(schem, crate::build::TICK_LIMIT, 0, false, crate::oracle::Oracle::new(), None);
        SETUP_ONLY.with(|f| f.set(false));
        let mut bot = STASH.with(|s| s.borrow_mut().take())?;
        // the picture as the bot has it after its first look at the site (the chests)
        let g = &bot.g;
        let (x0, y0, z0, nx, ny, nz) = (g.x0, g.y0, g.z0, g.nx, g.ny, g.nz);
        for y in y0..y0 + ny { for z in z0..z0 + nz { for x in x0..x0 + nx {
            let s = bot.c.w.get(crate::geom::p(x, y, z));
            if s != bot.g.get(x, y, z) { bot.on_block_change(x, y, z, s); }
        } } }
        bot.appeared.clear(); bot.changed_again.clear();
        Some(RouteOracle { bot, searches: 0, expanded: 0, why: [0; 6] })
    }
    pub fn origin(&self) -> (i32, i32, i32) { (self.bot.g.ox, self.bot.g.oy, self.bot.g.oz) }
    /// kind: 0 nothing there, 1 the build's own block, 2 a loose temporary block, 3 a scaffolding block
    pub fn set(&mut self, x: i32, y: i32, z: i32, kind: u8) {
        if !self.bot.g.inside(x, y, z) { return; }
        let s = match kind { 1 => self.bot.g.target(x, y, z), 2 => self.bot.scaffold_state, 3 => self.bot.tower_state, _ => 0 };
        if self.bot.g.get(x, y, z) != s { self.bot.on_block_change(x, y, z, s); }
        if kind >= 2 { self.bot.add_scaffold(x, y, z); }
        if self.bot.appeared.len() > 1000 { self.bot.appeared.clear(); }
        if self.bot.changed_again.len() > 1000 { self.bot.changed_again.clear(); }
    }
    pub fn route(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, towers: bool, mine: bool) -> Answer {
        let no = Answer { kind: 2, exhausted: false, expanded: 0, cost: 0, scaffold_moves: 0, tower_moves: 0, length: 0 };
        if !self.bot.g.inside(sx, sy, sz) || !self.bot.g.inside(gx, gy, gz) { return no; }
        let h = self.bot.g.stand16(sx, sy, sz);
        if h < 0 { return no; }
        // a goal A's bot could not stand on as the picture is (a stand yet to be built: fastsim's column stances) is not a question for the search
        if self.bot.g.stand16(gx, gy, gz) < 0 { self.why[1] += 1; return no; }
        let b = &mut self.bot;
        b.nav.start_h = h.clamp(0, 15);
        b.nav.towers_on = towers;
        b.nav.mine = mine;
        for k in b.nav.kept.iter_mut() { k.live = false; }
        let ok = b.nav_search(sx, sy, sz, gx, gy, gz, max_expand, true);
        b.nav.mine = false;
        self.searches += 1; self.expanded += b.nav.expanded as u64;
        // (diagnosis) why not: 0 found, 1 goal is no place to stand in A's picture, 2 dry within 64, 3 dry, 4 budget
        let gs = b.g.stand16(gx, gy, gz);
        self.why[if ok { 0 } else if gs < 0 { 1 } else if b.nav.exhausted && b.nav.expanded <= 64 { 2 } else if b.nav.exhausted { 3 } else { 4 }] += 1;
        Answer { kind: ok as u8, exhausted: b.nav.exhausted, expanded: b.nav.expanded, cost: b.nav.goal_cost, scaffold_moves: b.nav.scaffold_moves, tower_moves: b.nav.tower_moves, length: b.nav.length }
    }
    /// A's stance planner for the build's block at (x, y, z), the bot standing in (sx, sy, sz): 0 = it finds no way to
    /// place the block, 1 = it finds a stance, 2 = cannot be asked.
    pub fn can_place(&mut self, sx: i32, sy: i32, sz: i32, x: i32, y: i32, z: i32) -> u8 {
        if !self.bot.g.inside(sx, sy, sz) || !self.bot.g.inside(x, y, z) { return 2; }
        let h = self.bot.g.stand16(sx, sy, sz);
        let tg = self.bot.g.target(x, y, z);
        if h < 0 || tg == 0 { return 2; }
        let b = &mut self.bot;
        b.c.pl.teleport(sx as f64 + 0.5, sy as f64 + h as f64 / 16.0, sz as f64 + 0.5);
        b.c.pl.on_ground = true;
        b.unreachable.clear(); b.unreachable_from = -1;
        for k in b.nav.kept.iter_mut() { k.live = false; }
        let sp = b.spec(tg);
        let r = b.plan_stance(x, y, z, Some(&sp), false);
        self.searches += 1;
        r.is_some() as u8
    }
}
