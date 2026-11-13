//! Per block-state tables (the Java version's `StateInfo`, plus what the game model needs), loaded from
//! `reference/mc/states.tsv` and the files written by `tools/gen_tables.py`.
use crate::kinds::{Kind, KIND_NAMES};
use crate::props::*;
use crate::shapes::VoxShape;
use std::collections::HashMap;
use std::sync::OnceLock;

pub type S = u16;

pub struct PropDef {
    pub id: u8,
    pub vals: Vec<u8>,
    pub stride: u16,
}

pub struct BlockInfo {
    pub name: String,
    pub kind: Kind,
    pub first: u16,
    pub n: u16,
    pub default: u16,
    /// 0 none, 1 XZ, 2 XYZ (random per-position offset of the model and shapes)
    pub offset: u8,
    pub push: u8,
    pub item: u16,
    pub props: Vec<PropDef>,
    prop_slot: [u8; N_PROPS],
}

pub const PUSH_NORMAL: u8 = 0;
pub const PUSH_DESTROY: u8 = 1;
pub const PUSH_BLOCK: u8 = 2;
pub const PUSH_IGNORE: u8 = 3;
pub const PUSH_ONLY: u8 = 4;

pub struct ItemInfo {
    pub name: String,
    pub stack: u8,
    /// blocks whose item this is (block indices); the first is the primary one
    pub blocks: Vec<u16>,
}

pub struct Tables {
    pub n: usize,
    pub block: Vec<u16>,
    pub air: Vec<bool>,
    pub fluid: Vec<bool>,
    pub gravity: Vec<bool>,
    pub replaceable: Vec<bool>,
    pub full_cube: Vec<bool>,
    pub no_collision: Vec<bool>,
    pub scaffolding: Vec<bool>,
    pub walk_through: Vec<bool>,
    pub sturdy_top: Vec<bool>,
    pub door_edges: Vec<u8>,
    pub hand_door: Vec<bool>,
    pub closed_gate: Vec<bool>,
    pub rung: Vec<u8>,
    pub rung_facing: Vec<u8>,
    pub leaves: Vec<bool>,
    pub climbable: Vec<bool>,
    pub interactive: Vec<bool>,
    pub secondary: Vec<bool>,
    pub late: Vec<bool>,
    pub technical: Vec<bool>,
    pub dewater: Vec<u16>,
    pub canon: Vec<u16>,
    pub loose: Vec<u16>,
    pub item: Vec<u16>,
    pub item_count: Vec<u8>,
    pub col_top: Vec<i8>,
    pub col_bottom: Vec<i8>,
    pub any_top: Vec<i8>,
    pub any_bottom: Vec<i8>,
    pub friction: Vec<f32>,
    pub speed_factor: Vec<f32>,
    pub jump_factor: Vec<f32>,
    pub destroy_time: Vec<f32>,
    pub requires_tool: Vec<bool>,
    pub light: Vec<u8>,
    /// collision / outline shape per state (index into `shapes`), as the game gives them at BlockPos.ZERO
    pub col_shape: Vec<u32>,
    pub out_shape: Vec<u32>,
    pub shapes: Vec<VoxShape>,
    /// outline boxes as the bot's `Ray` keeps them (floats)
    pub out_boxes_f32: Vec<Box<[f32]>>,
    pub out_full: Vec<bool>,
    pub blocks: Vec<BlockInfo>,
    pub block_by_name: HashMap<String, u16>,
    pub items: Vec<ItemInfo>,
    pub item_by_name: HashMap<String, u16>,
    pub block_tags: HashMap<String, Vec<bool>>,
    pub empty_shape: u32,
    pub full_shape: u32,
    /// from reference/java-extra/states-extra.tsv (the game's own answers); empty if that file is absent
    pub suffocating: Vec<bool>,
    pub conductor: Vec<bool>,
    pub solid: Vec<bool>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

#[inline(always)]
pub fn t() -> &'static Tables {
    TABLES.get().expect("tables not loaded")
}

pub fn init(ref_dir: &str, data_dir: &str) {
    if TABLES.get().is_some() {
        return;
    }
    let tb = load(ref_dir, data_dir);
    let _ = TABLES.set(tb);
}

fn vcode(v: &str) -> u8 {
    if let Ok(n) = v.parse::<u8>() {
        return n;
    }
    32 + VALUE_NAMES.iter().position(|x| *x == v).unwrap_or_else(|| panic!("unknown value {v}")) as u8
}

pub fn value_name(code: u8) -> String {
    if code < 32 { code.to_string() } else { VALUE_NAMES[(code - 32) as usize].to_string() }
}

fn parse_boxes(s: &str) -> Vec<[f64; 6]> {
    let mut out = Vec::new();
    if s.is_empty() {
        return out;
    }
    for b in s.split(';') {
        let v: Vec<f64> = b.split(',').map(|x| x.parse().unwrap()).collect();
        out.push([v[0], v[1], v[2], v[3], v[4], v[5]]);
    }
    out
}

fn load(ref_dir: &str, data_dir: &str) -> Tables {
    // ---- blocks
    let mut blocks = Vec::new();
    let mut block_by_name = HashMap::new();
    let kinds: HashMap<&str, Kind> = KIND_NAMES.iter().cloned().collect();
    let mut item_names: Vec<String> = Vec::new();
    let btxt = std::fs::read_to_string(format!("{data_dir}/blocks.tsv")).expect("blocks.tsv");
    let mut block_item_name = Vec::new();
    for l in btxt.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        let mut props = Vec::new();
        let mut prop_slot = [0u8; N_PROPS];
        if !f[9].is_empty() {
            for p in f[9].split(';') {
                let (k, vals) = p.split_once('=').unwrap();
                let id = PROP_NAMES.iter().position(|x| *x == k).unwrap() as u8;
                props.push(PropDef { id, vals: vals.split('|').map(vcode).collect(), stride: 1 });
            }
            let mut stride = 1u16;
            for i in (0..props.len()).rev() {
                props[i].stride = stride;
                stride *= props[i].vals.len() as u16;
                prop_slot[props[i].id as usize] = i as u8 + 1;
            }
        }
        let offset = match f[6] { "XZ" => 1, "XYZ" => 2, _ => 0 };
        let push = match f[7] { "DESTROY" => PUSH_DESTROY, "BLOCK" => PUSH_BLOCK, "IGNORE" => PUSH_IGNORE, "PUSH_ONLY" => PUSH_ONLY, _ => PUSH_NORMAL };
        block_by_name.insert(f[1].to_string(), blocks.len() as u16);
        block_item_name.push(f[8].to_string());
        blocks.push(BlockInfo {
            name: f[1].to_string(),
            kind: *kinds.get(f[2]).unwrap(),
            first: f[3].parse().unwrap(),
            n: f[4].parse().unwrap(),
            default: f[5].parse().unwrap(),
            offset,
            push,
            item: 0,
            props,
            prop_slot,
        });
    }
    // ---- items
    let mut items = vec![ItemInfo { name: "air".into(), stack: 64, blocks: vec![] }];
    let mut item_by_name: HashMap<String, u16> = HashMap::new();
    item_by_name.insert("air".into(), 0);
    let itxt = std::fs::read_to_string(format!("{data_dir}/items.tsv")).expect("items.tsv");
    for l in itxt.lines() {
        let (n, st) = l.split_once('\t').unwrap();
        item_by_name.insert(n.to_string(), items.len() as u16);
        item_names.push(n.to_string());
        items.push(ItemInfo { name: n.to_string(), stack: st.parse().unwrap(), blocks: vec![] });
    }
    for (bi, b) in blocks.iter_mut().enumerate() {
        let it = *item_by_name.get(&block_item_name[bi]).unwrap_or(&0);
        b.item = it;
        if it != 0 {
            // primary first: the block with the item's own name, else in registry order
            if b.name == items[it as usize].name { items[it as usize].blocks.insert(0, bi as u16); } else { items[it as usize].blocks.push(bi as u16); }
        }
    }
    // ---- tags
    let mut block_tags = HashMap::new();
    let ttxt = std::fs::read_to_string(format!("{data_dir}/tags.tsv")).expect("tags.tsv");
    for l in ttxt.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f[0] != "block" { continue; }
        let mut v = vec![false; blocks.len()];
        if f.len() > 2 {
            for n in f[2].split(',') {
                if let Some(&i) = block_by_name.get(n) { v[i as usize] = true; }
            }
        }
        block_tags.insert(f[1].to_string(), v);
    }
    // ---- states
    let stxt = std::fs::read_to_string(format!("{ref_dir}/mc/states.tsv")).expect("states.tsv");
    let mut lines = stxt.lines();
    let hdr: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let col = |n: &str| hdr.iter().position(|x| *x == n).unwrap_or_else(|| panic!("no column {n}"));
    let (c_air, c_fluid, c_gravity, c_repl, c_full, c_nocol, c_scaf, c_walk, c_sturdy, c_dooredges, c_hand, c_gate) = (
        col("air"), col("fluid"), col("gravity"), col("replaceable"), col("fullCube"), col("noCollision"), col("scaffolding"), col("walkThrough"),
        col("sturdyTop"), col("doorEdges"), col("handDoor"), col("closedGate"));
    let (c_rung, c_rungf, c_leaves, c_climb, c_inter, c_sec, c_late, c_tech, c_dew, c_canon, c_loose, c_item, c_icount) = (
        col("rung"), col("rungFacing"), col("leaves"), col("climbable"), col("interactive"), col("secondary"), col("late"), col("technical"),
        col("dewater"), col("canon"), col("loose"), col("item"), col("itemCount"));
    let (c_ct, c_cb, c_at, c_ab, c_fr, c_sf, c_jf, c_dt, c_rt, c_light, c_colb, c_outb) = (
        col("colTop"), col("colBottom"), col("anyTop"), col("anyBottom"), col("friction"), col("speedFactor"), col("jumpFactor"), col("destroyTime"),
        col("requiresTool"), col("light"), col("collisionBoxes"), col("outlineBoxes"));
    let mut tb = Tables {
        n: 0, block: vec![], air: vec![], fluid: vec![], gravity: vec![], replaceable: vec![], full_cube: vec![], no_collision: vec![], scaffolding: vec![],
        walk_through: vec![], sturdy_top: vec![], door_edges: vec![], hand_door: vec![], closed_gate: vec![], rung: vec![], rung_facing: vec![], leaves: vec![],
        climbable: vec![], interactive: vec![], secondary: vec![], late: vec![], technical: vec![], dewater: vec![], canon: vec![], loose: vec![], item: vec![],
        item_count: vec![], col_top: vec![], col_bottom: vec![], any_top: vec![], any_bottom: vec![], friction: vec![], speed_factor: vec![], jump_factor: vec![],
        destroy_time: vec![], requires_tool: vec![], light: vec![], col_shape: vec![], out_shape: vec![], shapes: vec![], out_boxes_f32: vec![], out_full: vec![],
        blocks: vec![], block_by_name: HashMap::new(), items: vec![], item_by_name: HashMap::new(), block_tags: HashMap::new(), empty_shape: 0, full_shape: 0, suffocating: vec![], conductor: vec![], solid: vec![],
    };
    let mut shape_ids: HashMap<String, u32> = HashMap::new();
    let mut intern = |s: &str, shapes: &mut Vec<VoxShape>| -> u32 {
        if let Some(&i) = shape_ids.get(s) { return i; }
        let i = shapes.len() as u32;
        shapes.push(VoxShape::from_boxes(&parse_boxes(s)));
        shape_ids.insert(s.to_string(), i);
        i
    };
    tb.empty_shape = intern("", &mut tb.shapes);
    tb.full_shape = intern("0.0,0.0,0.0,1.0,1.0,1.0", &mut tb.shapes);
    let mut bi = 0usize;
    for l in lines {
        let f: Vec<&str> = l.split('\t').collect();
        let id: usize = f[0].parse().unwrap();
        assert_eq!(id, tb.block.len());
        while !(id >= blocks[bi].first as usize && id < blocks[bi].first as usize + blocks[bi].n as usize) { bi += 1; }
        assert_eq!(blocks[bi].name, f[1]);
        let b = |c: usize| f[c] == "1";
        tb.block.push(bi as u16);
        tb.air.push(b(c_air)); tb.fluid.push(b(c_fluid)); tb.gravity.push(b(c_gravity)); tb.replaceable.push(b(c_repl)); tb.full_cube.push(b(c_full));
        tb.no_collision.push(b(c_nocol)); tb.scaffolding.push(b(c_scaf)); tb.walk_through.push(b(c_walk)); tb.sturdy_top.push(b(c_sturdy));
        tb.door_edges.push(f[c_dooredges].parse().unwrap()); tb.hand_door.push(b(c_hand)); tb.closed_gate.push(b(c_gate));
        tb.rung.push(f[c_rung].parse().unwrap()); tb.rung_facing.push(f[c_rungf].parse().unwrap()); tb.leaves.push(b(c_leaves)); tb.climbable.push(b(c_climb));
        tb.interactive.push(b(c_inter)); tb.secondary.push(b(c_sec)); tb.late.push(b(c_late)); tb.technical.push(b(c_tech));
        tb.dewater.push(f[c_dew].parse().unwrap()); tb.canon.push(f[c_canon].parse().unwrap()); tb.loose.push(f[c_loose].parse().unwrap());
        tb.item.push(*item_by_name.get(f[c_item]).unwrap_or(&0));
        tb.item_count.push(f[c_icount].parse().unwrap());
        tb.col_top.push(f[c_ct].parse().unwrap()); tb.col_bottom.push(f[c_cb].parse().unwrap()); tb.any_top.push(f[c_at].parse().unwrap()); tb.any_bottom.push(f[c_ab].parse().unwrap());
        tb.friction.push(f[c_fr].parse().unwrap()); tb.speed_factor.push(f[c_sf].parse().unwrap()); tb.jump_factor.push(f[c_jf].parse().unwrap());
        tb.destroy_time.push(f[c_dt].parse().unwrap()); tb.requires_tool.push(b(c_rt)); tb.light.push(f[c_light].parse().unwrap());
        let colb = if c_colb < f.len() { f[c_colb] } else { "" };
        let outb = if c_outb < f.len() { f[c_outb] } else { "" };
        tb.col_shape.push(intern(colb, &mut tb.shapes));
        tb.out_shape.push(intern(outb, &mut tb.shapes));
        let ob = parse_boxes(outb);
        let mut fl = Vec::with_capacity(ob.len() * 6);
        for bx in &ob { for v in bx { fl.push(*v as f32); } }
        tb.out_full.push(ob.len() == 1 && fl[0] == 0.0 && fl[1] == 0.0 && fl[2] == 0.0 && fl[3] == 1.0 && fl[4] == 1.0 && fl[5] == 1.0);
        tb.out_boxes_f32.push(fl.into_boxed_slice());
    }
    tb.n = tb.block.len();
    if let Ok(ex) = std::fs::read_to_string(format!("{ref_dir}/java-extra/states-extra.tsv")) {
        let mut lines = ex.lines();
        let hdr: Vec<&str> = lines.next().unwrap().split('\t').collect();
        let col = |n: &str| hdr.iter().position(|x| *x == n).unwrap();
        let (c_cond, c_sol, c_suf) = (col("isRedstoneConductor"), col("isSolid"), col("isSuffocating"));
        for l in lines {
            let f: Vec<&str> = l.split('\t').collect();
            tb.conductor.push(f[c_cond] == "1");
            tb.solid.push(f[c_sol] == "1");
            tb.suffocating.push(f[c_suf] == "1");
        }
        if tb.suffocating.len() != tb.n { tb.suffocating.clear(); tb.conductor.clear(); tb.solid.clear(); }
    }
    tb.blocks = blocks;
    tb.block_by_name = block_by_name;
    tb.items = items;
    tb.item_by_name = item_by_name;
    tb.block_tags = block_tags;
    tb
}

impl Tables {
    #[inline]
    pub fn binfo(&self, s: S) -> &BlockInfo { &self.blocks[self.block[s as usize] as usize] }
    #[inline]
    pub fn kind(&self, s: S) -> Kind { self.binfo(s).kind }
    pub fn name(&self, s: S) -> String {
        let b = self.binfo(s);
        if b.props.is_empty() { return b.name.clone(); }
        let mut out = format!("{}[", b.name);
        for (i, p) in b.props.iter().enumerate() {
            if i > 0 { out.push(','); }
            out.push_str(PROP_NAMES[p.id as usize]);
            out.push('=');
            out.push_str(&value_name(get(s, p.id).unwrap()));
        }
        out.push(']');
        out
    }
    pub fn bid(&self, name: &str) -> u16 { *self.block_by_name.get(name).unwrap_or_else(|| panic!("no block {name}")) }
    pub fn default_of(&self, name: &str) -> S { self.blocks[self.bid(name) as usize].default }
    pub fn tag(&self, name: &str) -> &Vec<bool> { self.block_tags.get(name).unwrap_or_else(|| panic!("no tag {name}")) }
}

/// Value code of a property of a state, or None if the block has no such property.
#[inline]
pub fn get(s: S, p: u8) -> Option<u8> {
    let tb = t();
    let b = tb.binfo(s);
    let slot = b.prop_slot[p as usize];
    if slot == 0 { return None; }
    let pd = &b.props[slot as usize - 1];
    Some(pd.vals[((s - b.first) / pd.stride) as usize % pd.vals.len()])
}
#[inline]
pub fn has(s: S, p: u8) -> bool { t().binfo(s).prop_slot[p as usize] != 0 }
/// Value code; panics if the block lacks the property (as `getValue` throws in the game).
#[inline]
pub fn val(s: S, p: u8) -> u8 { get(s, p).unwrap_or_else(|| panic!("{} has no property {}", t().name(s), PROP_NAMES[p as usize])) }
#[inline]
pub fn is_true(s: S, p: u8) -> bool { get(s, p) == Some(V_TRUE) }
/// The same state with one property changed; panics on a value the property does not have.
pub fn with(s: S, p: u8, v: u8) -> S {
    let tb = t();
    let b = tb.binfo(s);
    let slot = b.prop_slot[p as usize];
    if slot == 0 { panic!("{} has no property {}", b.name, PROP_NAMES[p as usize]); }
    let pd = &b.props[slot as usize - 1];
    let old = ((s - b.first) / pd.stride) as usize % pd.vals.len();
    let new = pd.vals.iter().position(|x| *x == v).unwrap_or_else(|| panic!("{}: {} cannot be {}", b.name, PROP_NAMES[p as usize], value_name(v)));
    (s as i32 + (new as i32 - old as i32) * pd.stride as i32) as S
}
/// `with` that leaves the state alone if the block lacks the property or the value.
pub fn try_with(s: S, p: u8, v: u8) -> S {
    let b = t().binfo(s);
    let slot = b.prop_slot[p as usize];
    if slot == 0 { return s; }
    if !b.props[slot as usize - 1].vals.contains(&v) { return s; }
    with(s, p, v)
}
#[inline]
pub fn with_bool(s: S, p: u8, v: bool) -> S { with(s, p, if v { V_TRUE } else { V_FALSE }) }
#[inline]
pub fn block_of(s: S) -> u16 { t().block[s as usize] }
#[inline]
pub fn kind_of(s: S) -> Kind { t().kind(s) }
#[inline]
pub fn same_block(a: S, b: S) -> bool { block_of(a) == block_of(b) }

/// Tag membership with a per-call-site cache: `in_tag!(state, "fences")`.
#[macro_export]
macro_rules! in_tag {
    ($s:expr, $name:literal) => {{
        static CACHE: std::sync::OnceLock<&'static Vec<bool>> = std::sync::OnceLock::new();
        let v = CACHE.get_or_init(|| $crate::tables::t().tag($name));
        v[$crate::tables::block_of($s) as usize]
    }};
}
/// Block index by name with a per-call-site cache: `blk!("scaffolding")`.
#[macro_export]
macro_rules! blk {
    ($name:literal) => {{
        static CACHE: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
        *CACHE.get_or_init(|| $crate::tables::t().bid($name))
    }};
}
/// Is the state of the named block: `is_blk!(state, "ladder")`.
#[macro_export]
macro_rules! is_blk {
    ($s:expr, $name:literal) => { $crate::tables::block_of($s) == $crate::blk!($name) };
}
