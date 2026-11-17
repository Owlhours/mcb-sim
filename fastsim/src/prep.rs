//! `prep`: packs a list of builds into one `.fsb` file in the layout the simulator runs on.
//!
//! Per build: A's normalised schematic (`/a/corpus/cache/xx/<hash>.mcb`), A's target normalisation (fluids, technical
//! blocks, waterlogging, floating gravity blocks dropped), and every remaining state reduced to a palette entry of
//! (class bits, item, stack size, items per block). Cells are stored column-major: `((z * sx) + x) * sy + y`.
use crate::mcb::Schem;
use std::collections::HashMap;
use std::io::Write;

// ---- state table flags (bit numbers as written by ref/mcsim-src/src/bin/dump_states.rs)
const F_AIR: u32 = 1 << 0;
const F_FLUID: u32 = 1 << 1;
const F_TECHNICAL: u32 = 1 << 2;
const F_GRAVITY: u32 = 1 << 3;
const F_REPLACEABLE: u32 = 1 << 4;
const F_NO_COLLISION: u32 = 1 << 5;
const F_SECONDARY: u32 = 1 << 9;
const F_LATE: u32 = 1 << 10;
const F_SCAFFOLDING: u32 = 1 << 11;
const F_WALK_THROUGH: u32 = 1 << 12;
const F_GAME_MASTER: u32 = 1 << 13;
const F_PLACEABLE: u32 = 1 << 14;

// ---- palette class bits (what the simulator knows about a target cell)
/// has a collision box: blocks movement, can be stood on
pub const C_SOLID: u8 = 1;
/// not judged (no collision box, or creative-only)
pub const C_EXEMPT: u8 = 2;
/// the bot has no way of placing it by hand
pub const C_UNPLACEABLE: u8 = 4;
/// second half of a two-cell block (door top, bed head): appears with its first half
pub const C_SECONDARY: u8 = 8;
/// falls without support
pub const C_GRAVITY: u8 = 16;
/// A's bot leaves these until the main pass is over
pub const C_LATE: u8 = 32;

pub struct States {
    pub n: usize,
    pub flags: Vec<u32>,
    pub dewater: Vec<u16>,
    pub item: Vec<u16>,
    pub item_count: Vec<u8>,
    pub stack: Vec<u8>,
}

pub fn load_states(path: &str) -> States {
    let txt = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut s = States { n: 0, flags: vec![], dewater: vec![], item: vec![], item_count: vec![], stack: vec![] };
    for l in txt.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        s.flags.push(f[2].parse().unwrap());
        s.dewater.push(f[3].parse().unwrap());
        s.item.push(f[6].parse().unwrap());
        s.item_count.push(f[7].parse().unwrap());
        s.stack.push(f[8].parse().unwrap());
    }
    s.n = s.flags.len();
    s
}

/// One build in memory, as `prep` writes it and `run` reads it.
pub struct Build {
    pub hash: String,
    pub sx: usize,
    pub sy: usize,
    pub sz: usize,
    pub blocks: u32,
    /// palette: entry 0 is "nothing"
    pub pal: Vec<Pal>,
    /// dense items: 0 = loose scaffold block, 1 = scaffolding (tower), 2.. = the build's items
    pub n_items: usize,
    /// index of the chest stack (A's site layout) that holds the first stack of each item
    pub item_chest: Vec<u16>,
    pub chest_stacks: u32,
    /// palette index per cell, column-major
    pub cells: Vec<u16>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pal {
    pub class: u8,
    pub stack: u8,
    pub icount: u8,
    pub item: u16,
}

pub fn pack(st: &States, hash: &str, s: &Schem) -> Build {
    let (sx, sy, sz) = (s.sx as usize, s.sy as usize, s.sz as usize);
    let layer = sx * sz;
    // A's Target::new (ids in y, z, x order)
    let mut ids = vec![0u16; s.ids.len()];
    let mut blocks = 0u32;
    for i in 0..ids.len() {
        let mut id = s.ids[i] as usize;
        if id >= st.n || id == 0 { continue; }
        let f = st.flags[id];
        if f & (F_AIR | F_FLUID | F_TECHNICAL) != 0 { continue; }
        id = st.dewater[id] as usize;
        if st.flags[id] & F_GRAVITY != 0 && i >= layer {
            let below = ids[i - layer] as usize;
            if below == 0 || st.flags[below] & F_REPLACEABLE != 0 { continue; }
        }
        ids[i] = id as u16;
        blocks += 1;
    }
    // items in A's order (state id ascending), and the chest stacks they start in
    let mut per_state: HashMap<u16, u64> = HashMap::new();
    for &c in &ids { if c != 0 { *per_state.entry(c).or_insert(0) += 1; } }
    let mut used: Vec<u16> = per_state.keys().cloned().collect();
    used.sort();
    let mut need: Vec<(u16, u64)> = Vec::new();
    for &id in &used {
        let (it, ic) = (st.item[id as usize], st.item_count[id as usize]);
        if it == 0 || ic == 0 { continue; }
        let n = per_state[&id] * ic as u64;
        if let Some(e) = need.iter_mut().find(|e| e.0 == it) { e.1 += n; } else { need.push((it, n)); }
    }
    let scaffold_count = (64u64 * 54 * 20).min(64 * 27 + blocks as u64 * 3 + 64 * sy as u64 * 2);
    let tower_count = (64u64 * 54 * 20).min(64 * 27 + blocks as u64 * 3 + 64 * sy as u64 * 8);
    let mut dense: HashMap<u16, u16> = HashMap::new();
    let mut item_chest: Vec<u16> = vec![0, 0];
    let mut stacks = 0u64;
    stacks += scaffold_count.div_ceil(64);
    item_chest[1] = (stacks / 135) as u16;
    stacks += tower_count.div_ceil(64);
    for &(it, n) in &need {
        // (stack size from any state of that item)
        let stack = used.iter().find(|&&id| st.item[id as usize] == it).map(|&id| st.stack[id as usize]).unwrap_or(64).max(1) as u64;
        dense.insert(it, item_chest.len() as u16);
        item_chest.push((stacks / 135) as u16);
        stacks += (n + 4.max(n / 20)).div_ceil(stack);
    }
    let chest_stacks = stacks.div_ceil(135).max(1) as u32;
    // palette
    let mut pal: Vec<Pal> = vec![Pal::default()];
    let mut pal_of: HashMap<Pal, u16> = HashMap::new();
    let mut state_pal: HashMap<u16, u16> = HashMap::new();
    for &id in &used {
        let f = st.flags[id as usize];
        let mut class = 0u8;
        let gm = f & F_GAME_MASTER != 0;
        if f & F_NO_COLLISION == 0 && f & F_WALK_THROUGH == 0 { class |= C_SOLID; }
        if f & F_NO_COLLISION != 0 || gm { class |= C_EXEMPT; }
        if f & F_SECONDARY != 0 { class |= C_SECONDARY; }
        else if f & F_PLACEABLE == 0 || gm || f & F_SCAFFOLDING != 0 { class |= C_UNPLACEABLE; }
        if f & F_GRAVITY != 0 { class |= C_GRAVITY; }
        if f & F_LATE != 0 { class |= C_LATE; }
        let item = if class & (C_UNPLACEABLE | C_SECONDARY) != 0 { 0 } else { dense[&st.item[id as usize]] };
        let p = Pal { class, stack: st.stack[id as usize].max(1), icount: st.item_count[id as usize], item };
        let k = *pal_of.entry(p).or_insert_with(|| { pal.push(p); (pal.len() - 1) as u16 });
        state_pal.insert(id, k);
    }
    let mut cells = vec![0u16; sx * sy * sz];
    for y in 0..sy { for z in 0..sz { for x in 0..sx {
        let id = ids[(y * sz + z) * sx + x];
        if id != 0 { cells[(z * sx + x) * sy + y] = state_pal[&id]; }
    } } }
    Build { hash: hash.to_string(), sx, sy, sz, blocks, pal, n_items: item_chest.len(), item_chest, chest_stacks, cells }
}

fn w16(o: &mut Vec<u8>, v: u16) { o.extend_from_slice(&v.to_le_bytes()); }
fn w32(o: &mut Vec<u8>, v: u32) { o.extend_from_slice(&v.to_le_bytes()); }

pub fn write_build(o: &mut Vec<u8>, b: &Build) {
    let mut h = [b' '; 20];
    h[..b.hash.len().min(20)].copy_from_slice(&b.hash.as_bytes()[..b.hash.len().min(20)]);
    o.extend_from_slice(&h);
    w16(o, b.sx as u16); w16(o, b.sy as u16); w16(o, b.sz as u16); w16(o, b.pal.len() as u16);
    w32(o, b.blocks); w32(o, b.chest_stacks); w16(o, b.n_items as u16); w16(o, 0);
    for p in &b.pal { o.push(p.class); o.push(p.stack); o.push(p.icount); o.push(0); w16(o, p.item); w16(o, 0); }
    for &c in &b.item_chest { w16(o, c); }
    if b.pal.len() <= 256 { for &c in &b.cells { o.push(c as u8); } } else { for &c in &b.cells { w16(o, c); } }
}

pub struct Reader<'a> { pub d: &'a [u8], pub p: usize }
impl<'a> Reader<'a> {
    fn r16(&mut self) -> u16 { let v = u16::from_le_bytes([self.d[self.p], self.d[self.p + 1]]); self.p += 2; v }
    fn r32(&mut self) -> u32 { let v = u32::from_le_bytes([self.d[self.p], self.d[self.p + 1], self.d[self.p + 2], self.d[self.p + 3]]); self.p += 4; v }
    pub fn header(&mut self) -> usize { assert_eq!(&self.d[0..4], b"FSB1", "not a packed build file"); self.p = 4; self.r32() as usize }
    pub fn build(&mut self) -> Build {
        let hash = String::from_utf8_lossy(&self.d[self.p..self.p + 20]).trim().to_string();
        self.p += 20;
        let (sx, sy, sz, np) = (self.r16() as usize, self.r16() as usize, self.r16() as usize, self.r16() as usize);
        let (blocks, chest_stacks, n_items, _) = (self.r32(), self.r32(), self.r16() as usize, self.r16());
        let mut pal = Vec::with_capacity(np);
        for _ in 0..np { let (class, stack, icount) = (self.d[self.p], self.d[self.p + 1], self.d[self.p + 2]); self.p += 4; let item = self.r16(); self.p += 2; pal.push(Pal { class, stack, icount, item }); }
        let mut item_chest = Vec::with_capacity(n_items);
        for _ in 0..n_items { item_chest.push(self.r16()); }
        let n = sx * sy * sz;
        let mut cells = Vec::with_capacity(n);
        if np <= 256 { cells.extend(self.d[self.p..self.p + n].iter().map(|&c| c as u16)); self.p += n; } else { for _ in 0..n { cells.push(self.r16()); } }
        Build { hash, sx, sy, sz, blocks, pal, n_items, item_chest, chest_stacks, cells }
    }
}

/// `prep <list> <out.fsb>`: the list has a hash per line (anything after the first blank is ignored).
pub fn prep(list: &str, out: &str, corpus: &str, states: &str) {
    let st = load_states(states);
    let txt = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{list}: {e}"));
    let mut o: Vec<u8> = Vec::new();
    o.extend_from_slice(b"FSB1");
    let hashes: Vec<&str> = txt.lines().filter_map(|l| l.split_whitespace().next()).collect();
    let mut n = 0u32;
    w32(&mut o, 0);
    for h in hashes {
        let s = match Schem::load(&Schem::cache_file(corpus, h)) { Ok(s) => s, Err(e) => { eprintln!("{h}: {e}"); continue; } };
        let b = pack(&st, h, &s);
        write_build(&mut o, &b);
        n += 1;
    }
    o[4..8].copy_from_slice(&n.to_le_bytes());
    std::fs::File::create(out).and_then(|mut f| f.write_all(&o)).unwrap_or_else(|e| panic!("{out}: {e}"));
    eprintln!("{out}: {n} builds, {} bytes", o.len());
}
