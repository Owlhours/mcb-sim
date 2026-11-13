//! Getting about (the Java version's `Moves`): routes and walking them, doors, columns, pillars and bridges, leaning
//! over an edge, and the checks that keep the bot from going where it could not get out again.
use super::nav::{self, edge};
use super::state::{Bot, Stance};
use super::*;
use crate::kinds::Kind;
use crate::tables::*;
use crate::fasthash::FastMap;

/// MCSIM_EVMORE=1: the event log also gets every trap check (`W`) and every block change the bot is told of (`C`).
pub fn ev_more() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_EVMORE").is_some())
}

impl Bot {
    pub fn escape_from_half_floor(&mut self) -> bool {
        let tb = t();
        let (x, y, z) = (self.cx(), self.cy(), self.cz());
        if !self.c.on_ground() || self.g.stand16(x, y, z) <= 0 || self.count(self.scaffold_item) == 0 { return false; }
        for d in 0..4 {
            let (nx, nz) = (x + DXH[d], z + DZH[d]);
            if !self.g.inside(nx, y, nz) { continue; }
            let i = self.g.idx(nx, y, nz);
            let w = self.g.cur[i];
            if w == 0 || self.scaffold.contains(i as i32) || tb.interactive[w as usize] || tb.any_top[w as usize] > 9 { continue; }
            let bt = self.break_ticks(w);
            if !(0..=400).contains(&bt) { continue; }
            if !self.g.passable(nx, y + 1, nz) || !self.g.passable(nx, y + 2, nz) || !self.g.passable(x, y + 2, z) || !nav::flat_top(self.g.get(nx, y - 1, nz)) { continue; }
            let sp = self.spec(self.scaffold_state);
            self.settle();
            let e = self.c.eye();
            if !self.find_break(nx, y, nz, e[0], e[1], e[2]) { continue; }
            let cl = self.click.clone();
            if !self.break_block(nx, y, nz, cl.ax, cl.ay, cl.az, bt + 20) { continue; }
            self.escapes += 1;
            if self.g.tgt[i] != 0 { self.deferred.add(i as i32); }
            if !self.place_from_here(nx, y, nz, self.scaffold_state, &sp) { return false; }
            self.add_scaffold(nx, y, nz);
            self.one = [nx, y + 1, nz];
            return self.follow_one();
        }
        false
    }

    /// Bits for the four sides by which the bot could leave cell (x,y,z); bit 4: head room to jump.
    pub fn exits(&self, x: i32, y: i32, z: i32) -> i32 {
        let mut h = self.g.stand16(x, y, z);
        if h < 0 { h = 0; }
        let head_up = self.g.passable(x, y + 2, z);
        let mut m = if head_up { 16 } else { 0 };
        for d in 0..4 {
            let (nx, nz) = (x + DXH[d], z + DZH[d]);
            let hl = self.g.stand16(nx, y, nz);
            if hl >= 0 { if hl - h <= 9 || (hl - h <= 19 && head_up) { m |= 1 << d; } continue; }
            let hu = if head_up { self.g.stand16(nx, y + 1, nz) } else { -1 };
            if hu >= 0 { if 16 + hu - h <= 19 { m |= 1 << d; } continue; }
            if self.g.passable(nx, y, nz) && self.g.passable(nx, y + 1, nz) { m |= 1 << d; }
        }
        m
    }

    /// Saves the route in hand around a search that is only a question (Java: the copies of nav.path / nav.move).
    fn keep_route(&self) -> (Vec<i32>, Vec<u8>, usize, i32, i32, i32) {
        let len = self.nav.length;
        (self.nav.path[..len * 3].to_vec(), self.nav.mv[..len].to_vec(), len, self.nav.goal_cost, self.nav.scaffold_moves, self.nav.tower_moves)
    }
    fn restore_route(&mut self, kept: (Vec<i32>, Vec<u8>, usize, i32, i32, i32)) {
        let (path, mv, len, cost, sm, tm) = kept;
        if self.nav.path.len() < path.len() { self.nav.path.resize(path.len(), 0); }
        if self.nav.mv.len() < mv.len() { self.nav.mv.resize(mv.len(), 0); }
        self.nav.path[..path.len()].copy_from_slice(&path);
        self.nav.mv[..mv.len()].copy_from_slice(&mv);
        self.nav.length = len; self.nav.goal_cost = cost; self.nav.scaffold_moves = sm; self.nav.tower_moves = tm;
    }

    /// Is there a way from cell (x,y,z) back to the supply chests (optionally breaking through the build)?
    pub fn way_out(&mut self, x: i32, y: i32, z: i32, breaking: bool) -> bool {
        let rv = crate::rev::rev();
        self.trap_checks += 1;
        let (ax, ay, az) = (self.stock[0].chest_x, self.g.oy, self.stock[0].chest_z + 1);
        let kept = self.keep_route();
        self.nav.start_h = 0;
        let keep = self.nav.mine;
        self.nav.mine = false;
        // First with what stands (walking, dropping, the columns and ladders that are there): the usual case. Every
        // way found is remembered - the places on it, and every cell the body passes through - so that
        //  - a block that touches none of those cells needs no search at all (seals_in),
        //  - and a search from somewhere else only has to reach any remembered place, not the chests.
        // (Before 2d9a193 / without X_EXIT: "room for a hundred steps means not shut in".)
        let relax_was = self.relax_next;
        self.relax_next = rv.x_next;
        self.towers_off = true;
        let join = rv.x_exit && self.exit_valid && self.witness_usable && !self.exit_nodes.is_empty();
        self.nav.stop_at = join;
        if rv.a1 { self.nav.effort = 4; }
        let mut ok = if rv.x_exit { self.nav_search(x, y, z, ax, ay, az, 20000, false) } else {
            // (touch_limit: an exact early exit for a caller that reads only "found, or more than 96 expanded")
            self.nav.touch_limit = 97;
            let r = self.nav_search(x, y, z, ax, ay, az, 96, false) || self.nav.expanded > 96;
            self.nav.touch_limit = 0;
            r
        };
        self.nav.stop_at = false;
        if rv.a1 { self.nav.effort = 1; }
        self.towers_off = false;
        let mut certain_no = !ok && self.nav.exhausted;
        if ok && rv.x_exit && self.witness_learn {
            if !join { self.exit_nodes.clear(); self.exit_cells.clear(); } else { self.exit_joined += 1; }
            for k in 0..self.nav.length {
                let (px, py, pz) = (self.nav.path[k * 3], self.nav.path[k * 3 + 1], self.nav.path[k * 3 + 2]);
                if !self.g.inside(px, py, pz) { continue; }
                self.exit_nodes.insert(self.g.idx(px, py, pz) as i32);
                let (mut lo, mut hi) = (py, py + 2);
                if k > 0 { lo = lo.min(self.nav.path[k * 3 - 2]); hi = hi.max(self.nav.path[k * 3 - 2] + 2); }
                for yy in lo..=hi {
                    if self.g.inside(px, yy, pz) { self.exit_cells.insert(self.g.idx(px, yy, pz) as i32); }
                    if k > 0 && self.g.inside(self.nav.path[k * 3 - 3], yy, self.nav.path[k * 3 - 1]) { self.exit_cells.insert(self.g.idx(self.nav.path[k * 3 - 3], yy, self.nav.path[k * 3 - 1]) as i32); }
                }
            }
            self.exit_valid = true;
        }
        if !ok {
            self.trap_searches += 1;
            if rv.a1 { self.nav.effort = 4; }
            ok = self.nav_search(x, y, z, ax, ay, az, 30000, true);
            if rv.a1 { self.nav.effort = 1; }
            certain_no = !ok && self.nav.exhausted;
        }
        if !ok && breaking {
            self.nav.mine = true;
            self.nav.effort = 20;
            ok = self.nav_search(x, y, z, ax, ay, az, 60000, true);
            self.nav.effort = 1;
            certain_no = !ok && self.nav.exhausted;
        }
        // (a search that ran out of budget has not shown that there is no way: only "every reachable place was looked
        // at and none is the way" counts as shut in)
        if !ok && !certain_no && rv.x_exit { self.exit_unknown += 1; ok = true; }
        self.nav.mine = keep;
        self.relax_next = relax_was;
        self.restore_route(kept);
        // (MCSIM_EVMORE: "W cell ok join witnessLearn", as asked of the Java side under MCB_EVENTS_MORE)
        if self.events.is_some() && ev_more() { let l = format!("{} W {} {} {} {}\n", self.ticks, self.rel(x, y, z), ok, join, self.witness_learn); self.ev_write(&l); }
        ok
    }

    /// A way from cell (x,y,z) to the supply chests that needs nothing new: walking, jumping, dropping, doors,
    /// ladders and the scaffolding columns that stand. (Searched in full: for the clean-up, where a room with a
    /// column in it must not pass for open.)
    pub fn plain_way_out(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.trap_checks += 1;
        let (ax, ay, az) = (self.stock[0].chest_x, self.g.oy, self.stock[0].chest_z + 1);
        let kept = self.keep_route();
        self.nav.start_h = 0;
        let keep = self.nav.mine;
        self.nav.mine = false;
        let was_off = self.towers_off;
        self.towers_off = true;
        self.nav.effort = 8;
        let ok = self.nav_search(x, y, z, ax, ay, az, 20000, false);
        self.towers_off = was_off; self.nav.effort = 1; self.nav.mine = keep;
        self.restore_route(kept);
        ok
    }

    /// How the bot could get from cell (x,y,z) to the chests: 2 = with nothing new (walking, the columns that stand,
    /// ladders, doors), 1 = only by putting up new scaffolding, 0 = not at all short of breaking through.
    pub fn exit_level(&mut self, x: i32, y: i32, z: i32) -> i32 {
        if self.plain_way_out(x, y, z) { return 2; }
        let (ax, ay, az) = (self.stock[0].chest_x, self.g.oy, self.stock[0].chest_z + 1);
        let kept = self.keep_route();
        self.nav.start_h = 0;
        let keep = self.nav.mine;
        self.nav.mine = false;
        self.nav.effort = 8;
        let relax_was = self.relax_next;
        self.relax_next = crate::rev::rev().x_next;
        let ok = self.nav_search(x, y, z, ax, ay, az, 60000, true);
        self.nav.effort = 1; self.nav.mine = keep; self.relax_next = relax_was;
        self.last_exit_expanded = self.nav.expanded;
        self.restore_route(kept);
        if ok { 1 } else { 0 }
    }

    /// With the cells in going_away gone: can the bot, standing at (sx,sy,sz) (or wherever it lands from there when
    /// what carries it is among them), still get back to the chests without putting up anything new?
    pub fn safe_after_removal(&mut self, sx: i32, sy: i32, sz: i32) -> bool {
        let Some(going) = self.going_away.clone() else { return true };
        let was: Vec<u16> = going.iter().map(|&k| self.g.cur[k as usize]).collect();
        for &k in &going { self.g.put(k as usize, 0); }
        let mut ty = sy;
        while ty > self.g.y0 && self.g.stand16(sx, ty, sz) < 0 && self.g.passable(sx, ty, sz) { ty -= 1; }
        let ok = self.g.stand16(sx, ty, sz) >= 0 && self.plain_way_out(sx, ty, sz);
        for (n, &k) in going.iter().enumerate() { self.g.put(k as usize, was[n]); }
        ok
    }

    /// way_out for the world as it is, with the helper block (if any) taken away in thought.
    fn way_out_plain_world(&mut self, x: i32, y: i32, z: i32) -> bool {
        let (usable_was, learn_was) = (self.witness_usable, self.witness_learn);
        self.witness_usable = self.helper_cell < 0;
        self.witness_learn = self.helper_cell < 0;
        let r = self.way_out(x, y, z, self.accept_traps);
        self.witness_usable = usable_was; self.witness_learn = learn_was;
        r
    }

    /// Would putting the block into (x,y,z) from where the bot stands close its own way out? (Java `sealsIn0`)
    pub fn seals_in(&mut self, x: i32, y: i32, z: i32, tg: u16) -> bool {
        let rv = crate::rev::rev();
        if t().walk_through[tg as usize] || !self.c.on_ground() { return false; }
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        // (whatever is being closed off may be some way off: a hatch in the ceiling of the room the bot stands in)
        if (x - bx).abs() > 6 || (z - bz).abs() > 6 || y < by - 4 || y > by + 6 || !self.g.inside(bx, by, bz) { return false; }
        let i = self.g.idx(x, y, z);
        // a block that takes the room the bot itself stands in (over its head while it sneaks on a fence or a slab:
        // the game allows that, and the bot could never stand up again)
        if rv.x_rel && self.g.stand16(bx, by, bz) >= 0 {
            let was = self.g.cur[i];
            self.g.put(i, tg);
            let wedged = self.g.stand16(bx, by, bz) < 0;
            self.g.put(i, was);
            if wedged { self.traps_avoided += 1; return true; }
        }
        // (the way out found last time still stands and this block is not in it)
        if self.exit_valid && !self.exit_cells.contains(&(i as i32)) && self.exit_nodes.contains(&(self.g.idx(bx, by, bz) as i32)) {
            self.trap_checks += 1;
            return false;
        }
        let old = self.g.cur[i];
        self.g.put(i, tg);
        // (a block lent for the click is about to go again: no way out may count on it)
        let hc = if rv.helper { self.helper_cell } else { -1 };
        let helper_gone = hc >= 0 && hc as usize != i;
        let mut helper_was = 0;
        if helper_gone { helper_was = self.g.cur[hc as usize]; self.g.put(hc as usize, 0); }
        let (usable_was, learn_was) = (self.witness_usable, self.witness_learn);
        self.witness_usable = self.exit_valid && !self.exit_cells.contains(&(i as i32)) && !helper_gone;
        self.witness_learn = !helper_gone;
        let mut trapped = !self.way_out(bx, by, bz, self.accept_traps);
        self.witness_usable = usable_was; self.witness_learn = learn_was;
        self.g.put(i, old);
        // (no way out without the block either - shut in already, or too far for the search: then it is not this
        // block that does it, and refusing everything from here on would only stop the work)
        if rv.x_rel && trapped && !self.way_out_plain_world(bx, by, bz) { trapped = false; self.already_shut += 1; }
        if helper_gone { self.g.put(hc as usize, helper_was); }
        if trapped { self.traps_avoided += 1; }
        trapped
    }

    fn start_h_now(&self) -> i32 {
        if self.c.on_ground() { (jround((self.c.y() - self.cy() as f64) * 16.0) as i32).clamp(0, 15) } else { -1 }
    }

    pub fn search(&mut self, x: i32, y: i32, z: i32, max_expand: i32) -> bool {
        if !self.c.on_ground() { self.settle(); }
        self.nav.start_h = self.start_h_now();
        self.nav.towers_on = self.have_tower && !self.towers_off && !self.no_scaffold_nav;
        let thrifty = self.cleaning && self.force_mine;
        if thrifty { self.nav.mine = true; self.nav.tower_penalty = 120; self.nav.scaffold_penalty = 170; self.nav.effort = 4; }
        let mut ok = self.nav_search(self.cx(), self.cy(), self.cz(), x, y, z, max_expand, true);
        if thrifty { self.nav.mine = false; self.nav.tower_penalty = 3; self.nav.scaffold_penalty = 40; self.nav.effort = 1; }
        if !ok && self.nav.expanded <= 12 && !self.loose_blocked.is_empty() && !(self.cx() == x && self.cy() == y && self.cz() == z) {
            for e in self.loose_blocked.iter() { self.nav.blocked_edges.remove(e); }
            self.loose_blocked.clear();
            self.loose_fails.clear();
            self.unreachable.clear();
            if thrifty { self.nav.mine = true; self.nav.tower_penalty = 120; self.nav.scaffold_penalty = 170; self.nav.effort = 4; }
            ok = self.nav_search(self.cx(), self.cy(), self.cz(), x, y, z, max_expand, true);
            if thrifty { self.nav.mine = false; self.nav.tower_penalty = 3; self.nav.scaffold_penalty = 40; self.nav.effort = 1; }
        }
        let rv = crate::rev::rev();
        if !ok && rv.x_next && !self.relax_next && self.nav.expanded <= 400 && !(self.cx() == x && self.cy() == y && self.cz() == z) {
            // no route by the usual rules, and hardly anywhere to go from here (a bot that can roam fails to reach a
            // goal because of the goal, and searching twice only costs time): one more try with temporary blocks
            // allowed in a column's next cell
            self.relax_next = true;
            if thrifty { self.nav.mine = true; self.nav.tower_penalty = 120; self.nav.scaffold_penalty = 170; self.nav.effort = 4; }
            ok = self.nav_search(self.cx(), self.cy(), self.cz(), x, y, z, max_expand, true);
            self.relax_next = false;
            if thrifty { self.nav.mine = false; self.nav.tower_penalty = 3; self.nav.scaffold_penalty = 40; self.nav.effort = 1; }
            if ok { self.relaxed_routes += 1; }
        }
        self.boxed_expanded = self.nav.expanded;
        if !ok && self.nav.expanded <= (if rv.x_unstick { 3 } else { 1 }) && !(self.cx() == x && self.cy() == y && self.cz() == z) && self.unstick() {
            self.nav.start_h = self.start_h_now();
            ok = self.nav_search(self.cx(), self.cy(), self.cz(), x, y, z, max_expand, true);
        }
        ok
    }

    fn init_spares(&mut self) {
        if self.spares.is_some() { return; }
        let tb = t();
        let mut sp: FastMap<u16, i32> = FastMap::default();
        for i in 0..self.g.tgt.len() {
            let tg = self.g.tgt[i] as usize;
            if tg != 0 && tb.item_count[tg] > 0 { *sp.entry(tb.item[tg]).or_insert(0) += tb.item_count[tg] as i32; }
        }
        for v in sp.values_mut() { *v = 4.max(*v / 20) / 2; }
        self.spares = Some(sp);
    }

    /// How many more blocks of this kind the bot may break (and replace from the reserve in the chests).
    pub fn spare(&mut self, it: u16) -> i32 {
        self.init_spares();
        self.spares.as_ref().unwrap().get(&it).copied().unwrap_or(0)
    }

    pub fn spent(&mut self, state: u16) {
        self.init_spares();
        if let Some(v) = self.spares.as_mut().unwrap().get_mut(&t().item[state as usize]) { *v -= 1; }
    }

    pub fn unstick(&mut self) -> bool {
        let tb = t();
        if self.ticks - self.last_unstick < 200 { return false; }
        self.last_unstick = self.ticks;
        let (x0, y0, z0) = (self.cx(), self.cy(), self.cz());
        for dy in 0..=1 { for dz in -1..=1 { for dx in -1..=1 {
            let (qx, qy, qz) = (x0 + dx, y0 + dy, z0 + dz);
            if !self.g.inside(qx, qy, qz) || !self.body_in(qx, qy, qz) { continue; }
            let st = self.g.get(qx, qy, qz) as usize;
            if st == 0 || tb.walk_through[st] && tb.door_edges[st] == 0 { continue; }
            let opens = tb.hand_door[st] || (kind_of(st as u16) == Kind::TrapDoorBlock && !crate::is_blk!(st as u16, "iron_trapdoor"));
            if opens && self.toggle(qx, qy, qz) {
                self.settle();
                self.nav.blocked_edges.clear();
                self.unreachable.clear();
                self.unstuck += 1;
                return true;
            }
        } } }
        // redstone dust, a torch, a flower ... in the cell the bot stands in (or at head height): nothing can be put
        // there while it is, so no column, no way up - out it comes (and is put back later)
        if crate::rev::rev().x_unstick && self.c.on_ground() && self.count(self.scaffold_item) > 0 {
            for dy in 0..=1 {
                if !self.g.inside(x0, y0 + dy, z0) { continue; }
                let st = self.g.get(x0, y0 + dy, z0);
                let ci = self.g.idx(x0, y0 + dy, z0);
                if st == 0 || !tb.no_collision[st as usize] || self.scaffold.contains(ci as i32) || tb.door_edges[st as usize] != 0 { continue; }
                let bt = self.break_ticks(st);
                if bt < 0 || bt > 200 { continue; }
                let e = self.c.eye();
                if !self.find_break(x0, y0 + dy, z0, e[0], e[1], e[2]) { continue; }
                let cl = self.click.clone();
                if !self.break_block(x0, y0 + dy, z0, cl.ax, cl.ay, cl.az, bt + 40) { continue; }
                self.spent(st);
                if self.g.tgt[ci] != 0 { self.deferred.add(ci as i32); self.broken_out.insert(ci as i32); }
                self.settle();
                self.nav.blocked_edges.clear();
                self.unreachable.clear();
                self.unstuck += 1;
                if self.verbose { self.log(&format!("unstuck: took {} out of the bot's own cell at {}", tb.name(st), self.rel(x0, y0 + dy, z0))); }
                return true;
            }
        }
        let h0 = if self.g.inside(x0, y0, z0) { self.g.stand16(x0, y0, z0) } else { -1 };
        if h0 < 0 && self.c.on_ground() {
            for dy in 0..=1 {
                let st = self.g.get(x0, y0 + dy, z0);
                if st == 0 || tb.walk_through[st as usize] || !self.g.inside(x0, y0 + dy, z0) { continue; }
                let bt = self.break_ticks(st);
                if bt < 0 || bt > 1500 || tb.full_cube[st as usize] { continue; }
                let e = self.c.eye();
                if !self.find_break(x0, y0 + dy, z0, e[0], e[1], e[2]) { continue; }
                let cl = self.click.clone();
                if !self.break_block(x0, y0 + dy, z0, cl.ax, cl.ay, cl.az, bt + 40) { continue; }
                let ci = self.g.idx(x0, y0 + dy, z0);
                if !self.scaffold.contains(ci as i32) {
                    self.mined += 1; self.spent(st);
                    if self.g.tgt[ci] != 0 { self.deferred.add(ci as i32); self.broken_out.insert(ci as i32); }
                }
                self.settle();
                self.middle(x0, z0);
                self.nav.blocked_edges.clear();
                self.unreachable.clear();
                self.unstuck += 1;
                return true;
            }
        }
        let own = self.g.get(x0, y0, z0);
        if h0 > 0 && h0 < 16 && own != 0 && !self.scaffold.contains(self.g.idx(x0, y0, z0) as i32) && self.break_ticks(own) >= 0 && self.break_ticks(own) <= 1500 {
            if self.break_block(x0, y0, z0, x0 as f64 + 0.5, y0 as f64 + h0 as f64 / 16.0, z0 as f64 + 0.5, self.break_ticks(own) + 40) {
                self.mined += 1;
                self.spent(own);
                let ci = self.g.idx(x0, y0, z0);
                if self.g.tgt[ci] != 0 { self.deferred.add(ci as i32); self.broken_out.insert(ci as i32); }
                self.settle();
                self.nav.blocked_edges.clear();
                self.unreachable.clear();
                self.unstuck += 1;
                return true;
            }
        }
        if crate::rev::rev().a1 && self.boxed_expanded > 1 { return false; }   // (the rest is for a bot that cannot take a single step)
        for round in 0..2 {
            for d in 0..4 {
                self.c.look(yaw_to(DXH[d] as f64, DZH[d] as f64), 0.0);
                for n in 0..10 { self.c.keys(true, false, false, false, round == 1 || n > 3, false, false); self.tick(); }
                self.c.release_keys();
                self.settle();
                if (self.cx() != x0 || self.cy() != y0 || self.cz() != z0) && self.g.stand16(self.cx(), self.cy(), self.cz()) >= 0 {
                    self.unstuck += 1;
                    self.nav.blocked_edges.clear();
                    self.unreachable.clear();
                    return true;
                }
            }
        }
        self.nav.blocked_edges.clear();
        self.unreachable.clear();
        true
    }

    /// How long the block takes to break by hand, or -1 if the bot must not break it.
    pub fn mine_ticks(&mut self, x: i32, y: i32, z: i32) -> i32 {
        let tb = t();
        if !self.g.inside(x, y, z) || y < self.g.oy { return -1; }
        let w = self.g.get(x, y, z);
        if w == 0 { return 0; }
        if tb.gravity[self.g.get(x, y + 1, z) as usize] { return -1; }
        if crate::is_blk!(w, "ice") { return -1; }
        if !self.desperate && self.spare(tb.item[w as usize]) <= 0 { return -1; }
        for d in 0..6 {
            let n = self.g.get(x + DX[d], y + DY[d], z + DZ[d]);
            if n != 0 && d != 0 && !tb.scaffolding[n as usize] && !self.oracle.floats(n) { return -1; }
        }
        if z < self.g.oz - 2 && kind_of(w) == Kind::ChestBlock { return -1; }
        let tk = self.break_ticks(w);
        if tk < 0 || tk > (if self.desperate { 6000 } else { 1500 }) { -1 } else { tk }
    }

    pub fn route(&mut self, x: i32, y: i32, z: i32, max_expand: i32) -> bool {
        let here = if self.g.inside(self.cx(), self.cy(), self.cz()) { self.g.idx(self.cx(), self.cy(), self.cz()) as i32 } else { -1 };
        if here != self.unreachable_from || self.placed - self.unreachable_at > 30 { self.unreachable.clear(); self.unreachable_from = here; self.unreachable_at = self.placed; }
        let goal = self.g.idx_raw(x, y, z);
        if self.unreachable.contains(&goal) { return false; }
        if self.search(x, y, z, max_expand) { return true; }
        if self.shut_in >= 3 || self.force_mine {
            self.nav.mine = true;
            let mut ok = self.search(x, y, z, max_expand);
            if !ok { self.desperate = true; ok = self.search(x, y, z, max_expand); self.desperate = false; }
            self.nav.mine = false;
            if ok { return true; }
        }
        self.unreachable.insert(goal);
        false
    }

    pub fn go_to(&mut self, st: &Stance) -> bool {
        if self.watch.is_some() { self.wlog(&format!("goTo {} lean={} cap={} column={} moss={}", self.rel(st.x, st.y, st.z), st.lean, st.cap, st.column, st.moss)); }
        self.end_lean();
        let mut gy = if st.cap { st.y - 1 } else { st.y };
        if st.cap && self.cx() == st.x && self.cy() == st.y && self.cz() == st.z && self.c.on_ground() && self.scaffold.contains(self.g.idx(st.x, st.y - 1, st.z) as i32) && !self.g.is_tower(st.x, st.y - 1, st.z) { gy = st.y; }
        if !(self.cx() == st.x && self.cy() == gy && self.cz() == st.z && self.c.on_ground()) {
            self.sweep_here();
            if !self.rope.is_empty() {
                let top = self.rope[self.rope.len() - 1];
                let keep = !st.cap && top[0] == nav::PILLAR as i32 && st.x == top[4] && st.z == top[6] && st.y > top[5] && self.cx() == top[4] && self.cz() == top[6];
                if !keep { self.undo_rope(); }
            }
            self.towers_off = st.moss;
            self.nav.extra_effort = st.column * 400;
            let r = self.go_to_travel(st, gy);
            self.towers_off = false;
            self.nav.extra_effort = 0;
            if !r { return false; }
        }
        if st.cap && self.cy() == st.y - 1 {
            if self.count(self.scaffold_item) == 0 || self.g.get(st.x, st.y - 1, st.z) != 0 || !self.g.is_tower(st.x, st.y - 2, st.z) { return false; }
            if !self.pillar(st.x, st.y - 1, st.z) { return false; }
            self.rope.push([nav::PILLAR as i32, st.x, st.y - 1, st.z, st.x, st.y, st.z, self.g.idx(st.x, st.y - 1, st.z) as i32]);
        }
        if st.lean >= 0 {
            if self.want_held != 0 && !self.hold(self.want_held) { return false; }
            return self.lean(st.x, st.y, st.z, st.lean as usize);
        }
        true
    }

    fn go_to_travel(&mut self, st: &Stance, gy: i32) -> bool {
        if !self.route(st.x, gy, st.z, 20000) { return false; }
        if ((self.nav.scaffold_moves > 0 && self.count(self.scaffold_item) < self.nav.scaffold_moves + 4) || self.nav.tower_moves > self.count(self.tower_item)) && !self.stranded && self.restock_worthwhile() {
            let layer = if self.layer_now == i32::MAX { self.g.oy + self.g.sy } else { self.layer_now };
            self.restock(layer);
            if !self.route(st.x, gy, st.z, 20000) { return false; }
        }
        if !self.restocking {
            let mut one_way = false;
            let mut k = 1;
            while k < self.nav.length && !one_way {
                let m = self.nav.mv[k];
                one_way = m == nav::DROP || ((m == nav::WALK || m == nav::LADDER) && self.nav.path[k * 3 + 1] < self.nav.path[k * 3 - 2]);
                k += 1;
            }
            if one_way && !self.way_out(st.x, gy, st.z, self.accept_traps) { self.traps_avoided += 1; return false; }
        }
        self.travel()
    }

    pub fn restock_worthwhile(&self) -> bool {
        for st in &self.stock {
            if st.count > 0 && ((st.item == self.scaffold_item && self.nav.scaffold_moves > 0 && self.count(self.scaffold_item) < self.nav.scaffold_moves + 4) || (st.item == self.tower_item && self.nav.tower_moves > self.count(self.tower_item))) { return true; }
        }
        false
    }

    /// Sneaks to the edge of the block stood on, in the given direction, and stays there (sneak held).
    pub fn lean(&mut self, x: i32, y: i32, z: i32, dir: usize) -> bool {
        let tb = t();
        let (bx, bz) = (x + DXH[dir], z + DZH[dir]);
        if !self.g.passable(bx, y, bz) || !self.g.passable(bx, y + 1, bz) || tb.any_top[self.g.get(bx, y - 1, bz) as usize] > 6 || tb.any_top[self.g.get(bx, y - 2, bz) as usize] > 22 { return false; }
        if self.g.is_tower(x, y - 1, z) { return false; }
        if !self.centre(x, y, z) || self.on_tower() { return false; }
        let (mut last_x, mut last_z) = (f64::NAN, f64::NAN);
        for n in 0..40 {
            let pitch = self.c.pitch();
            self.c.look(yaw_to(DXH[dir] as f64, DZH[dir] as f64), pitch);
            self.c.keys(true, false, false, false, false, true, false);
            self.tick();
            if (self.c.x() - last_x).abs() + (self.c.z() - last_z).abs() < 1e-4 && n > 3 { break; }
            last_x = self.c.x(); last_z = self.c.z();
        }
        self.c.keys(false, false, false, false, false, true, false);
        self.tick();
        self.leaning = true;
        self.hold_sneak = true;
        self.c.on_ground() && self.cy() == y
    }

    pub fn end_lean(&mut self) {
        if !self.leaning { return; }
        self.leaning = false;
        self.hold_sneak = false;
        let (mut x, y, mut z) = (self.cx(), self.cy(), self.cz());
        if self.g.stand16(x, y, z) < 0 {
            for d in 0..4 {
                if self.g.stand16(x + DXH[d], y, z + DZH[d]) >= 0 && (self.c.x() - (x + DXH[d]) as f64 - 0.5).abs() < 0.85 && (self.c.z() - (z + DZH[d]) as f64 - 0.5).abs() < 0.85 { x += DXH[d]; z += DZH[d]; break; }
            }
        }
        for _ in 0..30 {
            let (dx, dz) = (x as f64 + 0.5 - self.c.x(), z as f64 + 0.5 - self.c.z());
            if dx * dx + dz * dz < 0.04 { break; }
            let pitch = self.c.pitch();
            self.c.look(yaw_to(dx, dz), pitch);
            self.c.keys(true, false, false, false, false, true, false);
            self.tick();
        }
        self.c.release_keys();
        self.tick();
    }

    /// Executes the route last found by nav.
    pub fn travel(&mut self) -> bool {
        self.t_step = -1;
        let ok = self.travel_impl();
        if !ok && self.t_step >= 1 && (self.t_step as usize) * 3 + 2 < self.t_path.len() {
            let ts = self.t_step as usize;
            let m = self.t_move[ts];
            let loose = (m == nav::BRIDGE || m == nav::PILLAR) && self.count(self.scaffold_item) > 0;
            if loose || m == nav::MINE || m == nav::CLEAR || m == nav::DIG {
                let (a, b) = (ts * 3 - 3, ts * 3);
                let tp = &self.t_path;
                if self.g.inside(tp[a], tp[a + 1], tp[a + 2]) && self.g.inside(tp[b], tp[b + 1], tp[b + 2]) {
                    let e = edge(self.g.idx(tp[a], tp[a + 1], tp[a + 2]) as i32, self.g.idx(tp[b], tp[b + 1], tp[b + 2]) as i32);
                    let old = *self.loose_fails.get(&e).unwrap_or(&0);
                    self.loose_fails.insert(e, old + 1);
                    if old >= 1 { self.nav.blocked_edges.insert(e); self.loose_blocked.insert(e); self.unreachable.clear(); }
                }
            }
        }
        ok
    }

    fn travel_impl(&mut self) -> bool {
        let tb = t();
        let n = self.nav.length;
        let path: Vec<i32> = self.nav.path[..n * 3].to_vec();
        let mv: Vec<u8> = self.nav.mv[..n].to_vec();
        self.t_path = path.clone();
        self.t_move = mv.clone();
        if self.watch.is_some() {
            let mut sb = String::from("route:");
            for k in 0..n { sb += &format!(" {}{}", if k == 0 { String::new() } else { format!("{}>", "WJDPBdcMCSTGL".as_bytes()[mv[k] as usize] as char) }, self.rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2])); }
            self.wlog(&sb);
        }
        let mut i = 1usize;
        while i < n {
            let m = mv[i];
            self.t_step = i as i32;
            if m == nav::PILLAR {
                let (x, y, z) = (path[i * 3], path[i * 3 + 1] - 1, path[i * 3 + 2]);
                let p_ok = self.pillar(x, y, z);
                if p_ok { self.rope.push([nav::PILLAR as i32, x, y, z, x, y + 1, z, self.g.idx(x, y, z) as i32]); }
                if !p_ok { return self.fail("pillar"); }
                i += 1;
            } else if m == nav::BRIDGE {
                let b_ok = self.bridge(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3], path[i * 3 + 2]);
                if b_ok { self.rope.push([nav::BRIDGE as i32, path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3], path[i * 3 + 1], path[i * 3 + 2], self.g.idx(path[i * 3], path[i * 3 + 1] - 1, path[i * 3 + 2]) as i32]); }
                if !b_ok { return self.fail("bridge"); }
                i += 1;
            } else if m == nav::CLEAR {
                let (x, y, z) = (path[i * 3], path[i * 3 + 1], path[i * 3 + 2]);
                let mut c_ok = true;
                let mut yy = y + 1;
                while yy >= y && c_ok {
                    if self.scaffold.contains(self.g.idx(x, yy, z) as i32) {
                        self.settle();
                        let e = self.c.eye();
                        c_ok = self.find_break(x, yy, z, e[0], e[1], e[2]) && { let cl = self.click.clone(); let bt = 80.max(self.break_ticks(self.g.get(x, yy, z))); self.break_block(x, yy, z, cl.ax, cl.ay, cl.az, bt) };
                    }
                    yy -= 1;
                }
                if !c_ok { return self.fail("clear scaffold"); }
                self.one = [x, y, z];
                if !self.follow_one() { return self.fail("walk after clearing"); }
                i += 1;
            } else if m == nav::MINE && path[i * 3] == path[i * 3 - 3] && path[i * 3 + 2] == path[i * 3 - 1] {
                // straight down: break the block underfoot and fall
                let (x, mut y, z) = (path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1]);
                // standing on something low in the own cell: that is what goes (on a post below: the post)
                if self.g.stand16(x, y, z) > 0 && !(crate::rev::rev().a1 && self.g.passable(x, y, z)) { y += 1; }
                let w = self.g.get(x, y - 1, z);
                if !self.centre(x, y - if self.g.stand16(x, y - 1, z) > 0 { 1 } else { 0 }, z) { return self.fail("centre before mining down"); }
                let on_top = self.g.get(x, y, z);
                if on_top != 0 && tb.no_collision[on_top as usize] { let bt = 40.max(self.break_ticks(on_top)); self.break_block(x, y, z, x as f64 + 0.5, y as f64 + 0.03, z as f64 + 0.5, bt); }
                let bt = 80.max(self.break_ticks(w) + 20);
                if !self.break_block(x, y - 1, z, x as f64 + 0.5, (y - 1) as f64 + 1.max(tb.any_top[w as usize] as i32) as f64 / 16.0, z as f64 + 0.5, bt) { return self.fail("mine down"); }
                self.mined += 1;
                self.spent(w);
                let ci = self.g.idx(x, y - 1, z);
                if self.g.tgt[ci] != 0 { self.deferred.add(ci as i32); self.broken_out.insert(ci as i32); }
                let mut n2 = 0;
                while n2 < 200 && !(self.c.on_ground() && self.cy() <= path[i * 3 + 1]) { self.tick(); n2 += 1; }
                self.settle();
                if self.cy() != path[i * 3 + 1] { return self.fail("fall after mining down"); }
                i += 1;
            } else if m == nav::MINE {
                let (x, y, z) = (path[i * 3], path[i * 3 + 1], path[i * 3 + 2]);
                let mut m_ok = true;
                let mut yy = y + 1;
                while yy >= y && m_ok {
                    let w = self.g.get(x, yy, z);
                    if !(w == 0 || tb.no_collision[w as usize]) {
                        self.settle();
                        let e = self.c.eye();
                        m_ok = self.find_break(x, yy, z, e[0], e[1], e[2]) && { let cl = self.click.clone(); let bt = 80.max(self.break_ticks(w) + 20); self.break_block(x, yy, z, cl.ax, cl.ay, cl.az, bt) };
                        if m_ok {
                            self.mined += 1;
                            self.spent(w);
                            let ci = self.g.idx(x, yy, z);
                            if self.g.tgt[ci] != 0 && !self.scaffold.contains(ci as i32) { self.deferred.add(ci as i32); self.broken_out.insert(ci as i32); }
                        }
                    }
                    yy -= 1;
                }
                if !m_ok { return self.fail("mine"); }
                self.one = [x, y, z];
                if !self.follow_one() { return self.fail("walk after mining"); }
                i += 1;
            } else if m == nav::CLIMB || m == nav::TOWER {
                let (x, z) = (path[i * 3], path[i * 3 + 2]);
                let mut j = i;
                while j < n && (mv[j] == nav::CLIMB || mv[j] == nav::TOWER) && path[j * 3] == x && path[j * 3 + 2] == z { j += 1; }
                let to_y = path[(j - 1) * 3 + 1];
                if self.cleaning { self.behind_x = x; self.behind_z = z; self.step_off = true; }
                let t_ok = self.tower(x, path[(i - 1) * 3 + 1], z, to_y);
                if !t_ok { return self.fail("tower"); }
                i = j;
            } else if m == nav::DESCEND {
                let (x, z) = (path[i * 3], path[i * 3 + 2]);
                let mut j = i;
                while j < n && mv[j] == nav::DESCEND && path[j * 3] == x && path[j * 3 + 2] == z { j += 1; }
                let to_y = path[(j - 1) * 3 + 1];
                let d_ok = self.centre(x, path[(i - 1) * 3 + 1], z) && self.middle(x, z) && self.descend(x, to_y, z);
                if !d_ok { return self.fail("sink"); }
                i = j;
            } else if m == nav::LADDER {
                let mut j = i;
                while j < n && mv[j] == nav::LADDER { j += 1; }
                let l_ok = self.ladder(&path, i, j);
                if !l_ok {
                    let k = 1.max((n as i32 - 1).min(self.failed_step)) as usize;
                    self.nav.blocked_edges.insert(edge(self.g.idx(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) as i32, self.g.idx(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) as i32));
                    self.unreachable.clear();
                    return self.fail("ladder");
                }
                self.ladders_used += 1;
                i = j;
            } else if m == nav::GATE {
                if !self.pass_gate(path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1], path[i * 3], path[i * 3 + 1], path[i * 3 + 2]) {
                    self.nav.blocked_edges.insert(edge(self.g.idx(path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1]) as i32, self.g.idx(path[i * 3], path[i * 3 + 1], path[i * 3 + 2]) as i32));
                    return self.fail("door");
                }
                i += 1;
            } else if m == nav::DIG {
                if !self.dig_down(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3 + 1]) { return self.fail("dig"); }
                i += 1;
            } else {
                let mut j = i;
                while j < n && mv[j] <= nav::DROP { j += 1; }
                let off = self.step_off && (path[i * 3] != self.behind_x || path[i * 3 + 2] != self.behind_z);
                if off { j = i + 1; }
                if !self.pending_close.is_empty() { j = i + 1; }
                if !self.follow(&path, i, j) {
                    let k = 1.max(self.failed_step) as usize;
                    if k < n && self.g.inside(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) && self.g.inside(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) {
                        self.nav.blocked_edges.insert(edge(self.g.idx(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) as i32, self.g.idx(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) as i32));
                    }
                    self.unreachable.clear();
                    self.step_off = false;
                    return self.fail("walk");
                }
                if off { self.step_off = false; let (bx, bz) = (self.behind_x, self.behind_z); self.take_down_behind(bx, bz); }
                self.close_pending();
                i = j;
            }
        }
        self.step_off = false;
        self.t_step = -1;
        true
    }

    /// Ladder moves path[from..n).
    fn ladder(&mut self, path: &[i32], from: usize, n: usize) -> bool {
        self.c.hold_attack(false);
        let mut opened: Vec<i32> = Vec::new();
        let mut ok = true;
        let mut i = from;
        while i < n && ok {
            self.failed_step = i as i32;
            let (wx, wy, wz) = (path[i * 3], path[i * 3 + 1], path[i * 3 + 2]);
            if self.g.rung(wx, wy, wz) == 3 {
                if !self.click_hatch(wx, wy, wz) { ok = false; break; }
                opened.push(self.g.idx(wx, wy, wz) as i32);
            }
            let feet = self.feet_of(wx, wy, wz);
            let hang = feet.is_nan();
            if hang && !self.g.hang(wx, wy, wz) { ok = false; break; }
            let ty = if hang { wy as f64 } else { feet };
            let mut budget = 80 + 20 * (wy - self.cy()).abs();
            loop {
                let (dx, dz) = (wx as f64 + 0.5 - self.c.x(), wz as f64 + 0.5 - self.c.z());
                let dist = (dx * dx + dz * dz).sqrt();
                let in_col = self.cx() == wx && self.cz() == wz;
                if if hang { in_col && dist < 0.2 && self.c.y() >= ty - 0.05 && self.c.y() < ty + 0.6 } else { in_col && self.c.on_ground() && (self.c.y() - feet).abs() < 0.02 } { break; }
                budget -= 1;
                if budget < 0 { ok = false; break; }
                let on_rung = self.g.rung(self.cx(), self.cy(), self.cz()) != 0;
                let jump = self.c.y() < ty + 0.02 && (!self.c.on_ground() || on_rung);
                if dist > 1e-3 { let pitch = self.c.pitch(); self.c.look(yaw_to(dx, dz), pitch); }
                self.c.keys(dist > 0.06, false, false, false, jump, false, false);
                self.tick();
            }
            if ok { self.close_hatches(&mut opened); }
            i += 1;
        }
        self.c.release_keys();
        if ok && self.g.stand16(path[n * 3 - 3], path[n * 3 - 2], path[n * 3 - 1]) >= 0 {
            self.one = [path[n * 3 - 3], path[n * 3 - 2], path[n * 3 - 1]];
            ok = self.follow_one();
            self.close_hatches(&mut opened);
        }
        ok
    }

    /// One click on a trapdoor from wherever the bot is; true if it changed.
    fn click_hatch(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.c.release_keys();
        if self.c.on_ground() && self.hspeed() > 0.003 { self.settle(); }
        let before = self.g.get(x, y, z);
        if before == 0 || !self.aim_at_block(x, y, z) { return false; }
        self.c.click_use();
        self.tick();
        if self.g.get(x, y, z) == before { self.tick(); }
        self.g.get(x, y, z) != before
    }

    fn close_hatches(&mut self, opened: &mut Vec<i32>) {
        let mut k = opened.len() as i32 - 1;
        while k >= 0 {
            let ku = k as usize;
            k -= 1;
            let cell = opened[ku] as usize;
            let (x, y, z) = (self.g.x_of(cell), self.g.y_of(cell), self.g.z_of(cell));
            if t().rung[self.g.cur[cell] as usize] != 2 { opened.remove(ku); continue; }
            let above = self.c.y() >= y as f64 + 0.99;
            let mut below = self.c.y() <= y as f64 - 0.99 && self.c.y() > y as f64 - 3.0;
            if !above && !below && self.c.y() < y as f64 && self.c.y() > y as f64 - 1.6 && !self.c.on_ground() {
                self.c.release_keys();
                let mut tk = 0;
                while tk < 12 && self.c.y() > y as f64 - 0.99 && !self.c.on_ground() { self.tick(); tk += 1; }
                below = self.c.y() <= y as f64 - 0.99;
            }
            if !above && !below { continue; }
            if above && !self.c.on_ground() { continue; }
            opened.remove(ku);
            if self.g.tgt[cell] != 0 && self.matches(self.g.cur[cell], self.g.tgt[cell]) { continue; }
            self.click_hatch(x, y, z);
        }
    }

    /// Walks from cell a to the neighbouring cell b through a door or gate.
    fn pass_gate(&mut self, ax: i32, ay: i32, az: i32, bx: i32, by: i32, bz: i32) -> bool {
        let tb = t();
        let d: usize = if bx > ax { 0 } else if bx < ax { 1 } else if bz > az { 2 } else { 3 };
        self.close_pending();
        if !self.centre(ax, ay, az) || !self.middle(ax, az) { return false; }
        let mut opened: Vec<usize> = Vec::new();
        for k in 0..4 {
            let (x, y, z) = (if k < 2 { ax } else { bx }, (if k < 2 { ay } else { by }) + (k & 1), if k < 2 { az } else { bz });
            let st = self.g.get(x, y, z) as usize;
            let blocks = (tb.door_edges[st] >> (if k < 2 { d } else { d ^ 1 }) & 1) != 0 || (k == 2 && tb.closed_gate[st]);
            if !blocks { continue; }
            if !tb.hand_door[st] || !self.toggle(x, y, z) {
                for &o in &opened.clone() { let (ox, oy, oz) = (self.g.x_of(o), self.g.y_of(o), self.g.z_of(o)); self.toggle(ox, oy, oz); }
                return false;
            }
            opened.push(self.g.idx(x, y, z));
        }
        self.one = [bx, by, bz];
        let ok = self.follow_one();
        if ok { self.middle(bx, bz); }
        for &cell in &opened {
            if self.g.tgt[cell] != 0 && self.matches(self.g.cur[cell], self.g.tgt[cell]) { continue; }
            let (cx, cy, cz) = (self.g.x_of(cell), self.g.y_of(cell), self.g.z_of(cell));
            let fills_cell = kind_of(self.g.cur[cell]) == Kind::FenceGateBlock;
            if fills_cell && (self.body_in(cx, cy, cz) || self.body_in(cx, cy - 1, cz)) { self.pending_close.push(cell as i32); continue; }
            if !self.toggle(cx, cy, cz) { self.pending_close.push(cell as i32); }
        }
        if ok { self.gates_passed += 1; }
        ok
    }

    /// Closes the gates left open behind as soon as the bot is out of them.
    pub fn close_pending(&mut self) {
        if self.pending_close.is_empty() { return; }
        let mut k = self.pending_close.len() as i32 - 1;
        while k >= 0 {
            let ku = k as usize;
            k -= 1;
            let cell = self.pending_close[ku] as usize;
            let (x, y, z) = (self.g.x_of(cell), self.g.y_of(cell), self.g.z_of(cell));
            if self.body_in(x, y, z) || self.body_in(x, y - 1, z) { continue; }
            self.pending_close.remove(ku);
            if self.g.tgt[cell] != 0 && self.g.cur[cell] != 0 && !self.matches(self.g.cur[cell], self.g.tgt[cell]) && t().hand_door[self.g.cur[cell] as usize] { self.toggle(x, y, z); }
        }
    }

    /// One click on a door or gate; true if it changed.
    pub fn toggle(&mut self, x: i32, y: i32, z: i32) -> bool {
        if !self.c.on_ground() || self.hspeed() > 0.003 { self.settle(); }
        let before = self.g.get(x, y, z);
        if before == 0 || !self.aim_at_block(x, y, z) { return false; }
        self.c.click_use();
        self.tick();
        if self.g.get(x, y, z) == before { self.tick(); }
        self.g.get(x, y, z) != before
    }

    /// During the final clean-up: having stepped off a scaffolding column, break as much of it as can be reached from here.
    fn take_down_behind(&mut self, x: i32, z: i32) {
        if !self.c.on_ground() { self.settle(); }
        let e = self.c.eye();
        let cy = self.cy();
        for y in cy - 5..=cy + 2 {
            if !self.g.inside(x, y, z) { continue; }
            let i = self.g.idx(x, y, z);
            if !self.scaffold.contains(i as i32) || !self.g.is_tower(x, y, z) { continue; }
            if !self.find_break(x, y, z, e[0], e[1], e[2]) { continue; }
            let mut top = y;
            while self.g.is_tower(x, top + 1, z) { top += 1; }
            let cl = self.click.clone();
            if self.break_block(x, y, z, cl.ax, cl.ay, cl.az, 40) {
                let mut n = 0;
                while n < 30 + 3 * (top - y) && self.g.is_tower(x, top, z) && top > y { self.tick(); n += 1; }
                if !self.c.on_ground() { self.settle(); }
            }
            return;
        }
    }

    pub fn centre(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.cx() == x && self.cz() == z && (self.c.x() - x as f64 - 0.5).abs() < 0.2 && (self.c.z() - z as f64 - 0.5).abs() < 0.2 && self.c.on_ground() { return true; }
        self.one = [x, y, z];
        self.follow_one()
    }

    /// Standing in column (x,z) at level y: makes sure a scaffolding column carries the bot up to level to_y, and climbs there.
    pub fn tower(&mut self, x: i32, y: i32, z: i32, to_y: i32) -> bool {
        let tb = t();
        if !self.centre(x, y, z) || !self.middle(x, z) { return false; }
        for _guard in 0..800 {
            if !self.c.on_ground() { self.settle(); }
            if self.cx() != x || self.cz() != z { return false; }
            if (self.c.y() - self.cy() as f64).abs() > 0.02 && !self.middle(x, z) { return false; }
            let f = self.cy();
            let mut top = if self.g.is_tower(x, f, z) { f } else { f - 1 };
            let any = self.g.is_tower(x, top, z);
            if any { while self.g.is_tower(x, top + 1, z) { top += 1; } }
            if any && top >= to_y - 1 { break; }
            let next = if any { top + 1 } else { f };
            let there = self.g.get(x, next, z);
            if there != 0 && !tb.replaceable[there as usize] { return self.fail("tower: in the way"); }
            if self.count(self.tower_item) == 0 || !self.hold(self.tower_item) { return false; }
            let (s_here, s_below) = (self.g.is_tower(x, f, z), self.g.is_tower(x, f - 1, z));
            let pitch: f32;
            let mut sneak = false;
            if self.g.is_tower(x, (self.c.y() + 1.62).floor() as i32, z) { pitch = -90.0; }
            else if s_here && !s_below { pitch = 90.0; sneak = true; }
            else if !s_here && !s_below { pitch = 90.0; sneak = tb.interactive[self.g.get(x, f - 1, z) as usize]; }
            else { if !self.descend(x, f - 1, z) { return self.fail("tower: could not get into the column"); } continue; }
            if sneak { self.c.keys(false, false, false, false, false, true, false); self.tick(); self.tick(); }
            let yaw = self.c.yaw();
            self.c.look(yaw, pitch);
            self.tick();
            let hit = self.c.pick(self.c.yaw(), pitch);
            let mut ok = false;
            if let Some(h) = hit {
                let pred = self.oracle.predict(&self.c.w, self.tower_item, h.pos, h.face, h.loc, h.inside, self.c.yaw(), pitch, sneak);
                let pa = self.oracle.placed_at;
                ok = pred != 0 && tb.scaffolding[pred as usize] && pa.x == x && pa.y == next && pa.z == z;
            }
            if ok { self.c.click_use(); self.tick(); }
            if sneak { self.c.release_keys(); self.tick(); }
            if ok && !self.g.is_tower(x, next, z) { self.tick(); }
            if !ok || !self.g.is_tower(x, next, z) { return self.fail("tower: no block"); }
            self.placed += 1; self.n_tower_blocks += 1;
            self.ev_placed(x, next, z);
            self.add_scaffold(x, next, z);
            self.tick(); self.tick();
        }
        if self.cy() == to_y && self.c.on_ground() { return true; }
        if self.cy() > to_y { self.descend(x, to_y, z) } else { self.climb(x, to_y, z) }
    }

    /// Standing in cell (x,y,z) on a full block: jump and put a loose block into that cell, ending on top of it.
    pub fn pillar(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.count(self.scaffold_item) == 0 { return false; }
        if !self.hold(self.scaffold_item) || !self.centre(x, y, z) { return false; }
        self.settle();
        let y0 = self.c.y();
        let yaw = self.c.yaw();
        self.c.look(yaw, 90.0);
        let sneak = t().interactive[self.g.get(x, y - 1, z) as usize];
        if sneak { self.c.keys(false, false, false, false, false, true, false); self.tick(); }
        self.c.keys(false, false, false, false, true, sneak, false);
        let mut ok = false;
        for n in 0..14 {
            // (y0 is y, or half a block more on a fence)
            if !ok && self.c.y() >= (if crate::rev::rev().a1 { y as f64 } else { y0 }) + 1.0 && self.g.get(x, y, z) == 0 { self.c.click_use(); }
            self.tick();
            if n == 0 { self.c.keys(false, false, false, false, false, sneak, false); }
            if self.g.get(x, y, z) != 0 { ok = true; }
            if self.c.on_ground() && n > 2 { break; }
        }
        self.c.release_keys();
        if ok { self.placed += 1; self.ev_placed(x, y, z); self.add_scaffold(x, y, z); self.n_pillar_moves += 1; } else { self.fail("pillar: no block appeared"); }
        self.settle();
        ok
    }

    /// From cell (px,py,pz), standing on a full block, put a loose block beside that block towards (x,z) and step onto it.
    pub fn bridge(&mut self, px: i32, py: i32, pz: i32, x: i32, z: i32) -> bool {
        if self.count(self.scaffold_item) == 0 { return false; }
        let dir: usize = if x > px { 0 } else if x < px { 1 } else if z > pz { 2 } else { 3 };
        if !self.hold(self.scaffold_item) || !self.lean(px, py, pz, dir) { self.fail("bridge: lean failed"); self.end_lean(); return false; }
        let e = self.c.eye();
        let mut ok = false;
        let mask = 1 << (if dir == 0 { 4 } else if dir == 1 { 5 } else if dir == 2 { 2 } else { 3 });
        if self.find_click(x, py - 1, z, self.scaffold_state, None, e[0], e[1], e[2], true, mask) {
            let cl = self.click.clone();
            self.c.look(cl.yaw, cl.pitch);
            self.c.click_use();
            self.c.keys(false, false, false, false, false, true, false);
            self.tick();
            ok = self.g.get(x, py - 1, z) != 0;
        }
        self.leaning = false;
        self.hold_sneak = false;
        self.c.release_keys();
        if !ok { self.tick(); return false; }
        self.placed += 1;
        self.ev_placed(x, py - 1, z);
        self.n_bridge_moves += 1;
        self.add_scaffold(x, py - 1, z);
        self.one = [x, py, z];
        self.follow_one()
    }

    /// Standing on a scaffold block in cell (x,y,z): break it (and any directly under it down to target_y) and sink.
    pub fn dig_down(&mut self, x: i32, y: i32, z: i32, target_y: i32) -> bool {
        self.n_dig_moves += 1;
        if !self.centre(x, y, z) { return false; }
        let mut yy = y - 1;
        while yy >= target_y {
            if self.g.get(x, yy, z) != 0 {
                if !self.scaffold.contains(self.g.idx(x, yy, z) as i32) { break; }
                let on_top = self.g.get(x, yy + 1, z);
                if on_top != 0 && t().no_collision[on_top as usize] { let bt = 40.max(self.break_ticks(on_top)); self.break_block(x, yy + 1, z, x as f64 + 0.5, yy as f64 + 1.03, z as f64 + 0.5, bt); }
                let bt = 80.max(self.break_ticks(self.g.get(x, yy, z)));
                if !self.break_block(x, yy, z, x as f64 + 0.5, yy as f64 + 1.0, z as f64 + 0.5, bt) { return false; }
            }
            yy -= 1;
        }
        let mut n = 0;
        while n < 200 && !(self.c.on_ground() && self.cy() <= target_y) { self.tick(); n += 1; }
        self.settle();
        self.cy() == target_y
    }

    /// Gets next to a walled-in cell by breaking a way to it through the build.
    pub fn tunnel_to(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.undo_rope();
        for dy in [0, -1] { for d in 0..4 {
            let (nx, ny, nz) = (x + DXH[d], y + dy, z + DZH[d]);
            if !self.g.inside(nx, ny, nz) || ny < self.g.oy { continue; }
            self.nav.mine = true;
            self.nav.effort = 20;
            let mut ok = self.search(nx, ny, nz, 60000);
            if !ok { self.desperate = true; ok = self.search(nx, ny, nz, 60000); self.desperate = false; }
            self.nav.mine = false;
            self.nav.effort = 1;
            if ok && self.travel() { return true; }
        } }
        false
    }
}
