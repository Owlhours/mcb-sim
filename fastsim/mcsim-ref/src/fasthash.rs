//! A cheap hasher for the small integer keys used all over (std's SipHash is several times slower).
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub struct Fx(u64);
impl Hasher for Fx {
    #[inline]
    fn finish(&self) -> u64 { self.0 }
    #[inline]
    fn write(&mut self, bytes: &[u8]) { for &b in bytes { self.add(b as u64); } }
    #[inline]
    fn write_u8(&mut self, v: u8) { self.add(v as u64) }
    #[inline]
    fn write_u16(&mut self, v: u16) { self.add(v as u64) }
    #[inline]
    fn write_u32(&mut self, v: u32) { self.add(v as u64) }
    #[inline]
    fn write_i32(&mut self, v: i32) { self.add(v as u32 as u64) }
    #[inline]
    fn write_u64(&mut self, v: u64) { self.add(v) }
    #[inline]
    fn write_i64(&mut self, v: i64) { self.add(v as u64) }
    #[inline]
    fn write_usize(&mut self, v: usize) { self.add(v as u64) }
}
impl Fx {
    #[inline]
    fn add(&mut self, v: u64) {
        let h = (self.0.rotate_left(5) ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 29);
    }
}
pub type FastSet<K> = HashSet<K, BuildHasherDefault<Fx>>;
pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// A `FastSet` with a small table of counters in front: `contains` asks the table first (one load) and probes the
/// set only when some member shares the key's slot. Same answers, same iteration order as the set alone; for sets that
/// are asked very often and mostly answer "no" (the route search's blocked moves and stop cells).
pub trait FiltKey: Copy + Eq + std::hash::Hash { fn slot(self) -> usize; }
const FILT_BITS: u32 = 11;
impl FiltKey for i32 { #[inline(always)] fn slot(self) -> usize { ((self as u32).wrapping_mul(0x9E37_79B1) >> (32 - FILT_BITS)) as usize } }
/// (a move `from << 32 | to`: the slot is that of the cell moved into, so that it can be asked before `from` is known)
impl FiltKey for i64 { #[inline(always)] fn slot(self) -> usize { (self as i32).slot() } }
pub struct FiltSet<K: FiltKey> { set: FastSet<K>, cnt: Box<[u32; 1 << FILT_BITS]> }
impl<K: FiltKey> Default for FiltSet<K> { fn default() -> Self { FiltSet { set: FastSet::default(), cnt: Box::new([0; 1 << FILT_BITS]) } } }
impl<K: FiltKey> FiltSet<K> {
    #[inline]
    pub fn insert(&mut self, k: K) -> bool { let new = self.set.insert(k); if new { self.cnt[k.slot()] += 1; } new }
    #[inline]
    pub fn remove(&mut self, k: &K) -> bool { let was = self.set.remove(k); if was { self.cnt[k.slot()] -= 1; } was }
    pub fn clear(&mut self) { if !self.set.is_empty() { self.cnt.fill(0); } self.set.clear(); }
    #[inline(always)]
    pub fn is_empty(&self) -> bool { self.set.is_empty() }
    #[inline]
    pub fn len(&self) -> usize { self.set.len() }
    #[inline]
    pub fn iter(&self) -> std::collections::hash_set::Iter<'_, K> { self.set.iter() }
    /// False: no member has this slot (so none has a key with this slot).
    #[inline(always)]
    pub fn maybe(&self, slot: usize) -> bool { self.cnt[slot & ((1 << FILT_BITS) - 1)] != 0 }
    #[inline(always)]
    pub fn contains(&self, k: &K) -> bool { self.maybe(k.slot()) && self.set.contains(k) }
    /// The set alone, without the table in front (the reference form of the route search asks this way).
    #[inline]
    pub fn contains_plain(&self, k: &K) -> bool { self.set.contains(k) }
}
