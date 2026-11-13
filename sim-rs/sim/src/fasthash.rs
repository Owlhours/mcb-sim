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
