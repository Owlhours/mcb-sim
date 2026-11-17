//! fastutil's IntOpenHashSet / Int2IntOpenHashMap, reproduced closely enough that iteration order is the same:
//! the Java bot iterates these sets in places where the order decides what it does next.
#![allow(dead_code)]

#[inline]
fn mix(x: i32) -> i32 {
    let h = x.wrapping_mul(0x9E3779B9u32 as i32);
    h ^ ((h as u32) >> 16) as i32
}
fn array_size(expected: usize, f: f32) -> usize {
    let s = ((expected as f64) / f as f64).ceil() as u64;
    (s.next_power_of_two() as usize).max(2)
}
fn max_fill(n: usize, f: f32) -> usize { (((n as f32) * f).ceil() as usize).min(n - 1) }

#[derive(Clone)]
pub struct IntMap {
    key: Vec<i32>,
    value: Vec<i32>,
    n: usize,
    mask: usize,
    min_n: usize,
    max_fill: usize,
    size: usize,
    contains_null: bool,
    pub def: i32,
}

impl Default for IntMap { fn default() -> Self { Self::new() } }

impl IntMap {
    pub fn new() -> IntMap {
        let n = array_size(16, 0.75);
        IntMap { key: vec![0; n + 1], value: vec![0; n + 1], n, mask: n - 1, min_n: n, max_fill: max_fill(n, 0.75), size: 0, contains_null: false, def: 0 }
    }
    pub fn len(&self) -> usize { self.size }
    pub fn is_empty(&self) -> bool { self.size == 0 }
    fn find(&self, k: i32) -> Option<usize> {
        if k == 0 { return if self.contains_null { Some(self.n) } else { None }; }
        let mut pos = (mix(k) as usize) & self.mask;
        loop {
            let c = self.key[pos];
            if c == 0 { return None; }
            if c == k { return Some(pos); }
            pos = (pos + 1) & self.mask;
        }
    }
    pub fn contains(&self, k: i32) -> bool { self.find(k).is_some() }
    pub fn get(&self, k: i32) -> i32 { match self.find(k) { Some(p) => self.value[p], None => self.def } }
    fn insert_new(&mut self, k: i32, v: i32) {
        if k == 0 {
            self.contains_null = true;
            self.key[self.n] = 0;
            self.value[self.n] = v;
        } else {
            let mut pos = (mix(k) as usize) & self.mask;
            while self.key[pos] != 0 { pos = (pos + 1) & self.mask; }
            self.key[pos] = k;
            self.value[pos] = v;
        }
        let s = self.size;
        self.size += 1;
        if s >= self.max_fill { self.rehash(array_size(self.size + 1, 0.75)); }
    }
    /// put; returns the old value (or the default)
    pub fn put(&mut self, k: i32, v: i32) -> i32 {
        match self.find(k) {
            Some(p) => { let o = self.value[p]; self.value[p] = v; o }
            None => { self.insert_new(k, v); self.def }
        }
    }
    /// addTo; returns the old value (or the default)
    pub fn add_to(&mut self, k: i32, incr: i32) -> i32 {
        match self.find(k) {
            Some(p) => { let o = self.value[p]; self.value[p] = o + incr; o }
            None => { self.insert_new(k, self.def + incr); self.def }
        }
    }
    /// set-style add; true if the key was new
    pub fn add(&mut self, k: i32) -> bool {
        if self.find(k).is_some() { return false; }
        self.insert_new(k, 0);
        true
    }
    pub fn remove(&mut self, k: i32) -> bool {
        match self.find(k) {
            None => false,
            Some(p) => {
                self.size -= 1;
                if p == self.n { self.contains_null = false; self.key[self.n] = 0; } else { self.shift_keys(p); }
                if self.n > self.min_n && self.size < self.max_fill / 4 && self.n > 16 { self.rehash(self.n / 2); }
                true
            }
        }
    }
    fn shift_keys(&mut self, mut pos: usize) {
        loop {
            let last = pos;
            pos = (pos + 1) & self.mask;
            let curr;
            loop {
                let c = self.key[pos];
                if c == 0 { self.key[last] = 0; return; }
                let slot = (mix(c) as usize) & self.mask;
                if if last <= pos { last >= slot || slot > pos } else { last >= slot && slot > pos } { curr = c; break; }
                pos = (pos + 1) & self.mask;
            }
            self.key[last] = curr;
            self.value[last] = self.value[pos];
        }
    }
    fn rehash(&mut self, new_n: usize) {
        let mask = new_n - 1;
        let mut nk = vec![0i32; new_n + 1];
        let mut nv = vec![0i32; new_n + 1];
        let mut i = self.n;
        let real = if self.contains_null { self.size - 1 } else { self.size };
        for _ in 0..real {
            loop { i -= 1; if self.key[i] != 0 { break; } }
            let mut pos = (mix(self.key[i]) as usize) & mask;
            while nk[pos] != 0 { pos = (pos + 1) & mask; }
            nk[pos] = self.key[i];
            nv[pos] = self.value[i];
        }
        nv[new_n] = self.value[self.n];
        self.n = new_n;
        self.mask = mask;
        self.max_fill = max_fill(new_n, 0.75);
        self.key = nk;
        self.value = nv;
    }
    pub fn clear(&mut self) {
        if self.size == 0 { return; }
        self.size = 0;
        self.contains_null = false;
        for k in self.key.iter_mut() { *k = 0; }
    }
    /// Keys in fastutil's iteration order.
    pub fn keys(&self) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.size);
        if self.contains_null { out.push(0); }
        let mut pos = self.n;
        while pos > 0 {
            pos -= 1;
            if self.key[pos] != 0 { out.push(self.key[pos]); }
        }
        out
    }
}

/// IntOpenHashSet
pub type IntSet = IntMap;
