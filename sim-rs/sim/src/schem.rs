//! Normalised schematics (`.mcb`, written by the Java version's `Schem.save`) and the zlib inflate they need.

pub struct Schem {
    pub sx: i32,
    pub sy: i32,
    pub sz: i32,
    pub ids: Vec<u16>,
}

struct Bits<'a> { d: &'a [u8], pos: usize, bit: u32, nbits: u32 }
impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) { while self.nbits < n { let b = if self.pos < self.d.len() { self.d[self.pos] } else { 0 }; self.pos += 1; self.bit |= (b as u32) << self.nbits; self.nbits += 8; } }
    fn get(&mut self, n: u32) -> u32 { if n == 0 { return 0; } self.need(n); let v = self.bit & ((1u32 << n) - 1); self.bit >>= n; self.nbits -= n; v }
}
struct Huff { counts: [u16; 16], symbols: Vec<u16> }
impl Huff {
    fn new(lengths: &[u8]) -> Huff {
        let mut counts = [0u16; 16];
        for &l in lengths { counts[l as usize] += 1; }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 { offs[i] = offs[i - 1] + counts[i - 1]; }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() { if l != 0 { symbols[offs[l as usize] as usize] = s as u16; offs[l as usize] += 1; } }
        Huff { counts, symbols }
    }
    fn decode(&self, b: &mut Bits) -> u32 {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1) as i32;
            let count = self.counts[len] as i32;
            if code - count < first { return self.symbols[(index + (code - first)) as usize] as u32; }
            index += count; first += count; first <<= 1; code <<= 1;
        }
        0
    }
}

/// Inflates a zlib stream (RFC 1950/1951).
pub fn inflate(data: &[u8]) -> Vec<u8> {
    const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 8);
    let mut b = Bits { d: data, pos: 2, bit: 0, nbits: 0 };
    loop {
        let last = b.get(1);
        let ty = b.get(2);
        if ty == 0 {
            b.bit = 0; b.nbits = 0;
            let len = data[b.pos] as usize | (data[b.pos + 1] as usize) << 8;
            b.pos += 4;
            out.extend_from_slice(&data[b.pos..b.pos + len]);
            b.pos += len;
        } else {
            let (lit, dist);
            if ty == 1 {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() { *v = if i < 144 { 8 } else if i < 256 { 9 } else if i < 280 { 7 } else { 8 }; }
                lit = Huff::new(&l);
                dist = Huff::new(&[5u8; 30]);
            } else {
                let nlen = b.get(5) as usize + 257;
                let ndist = b.get(5) as usize + 1;
                let ncode = b.get(4) as usize + 4;
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for i in 0..ncode { cl[ORDER[i]] = b.get(3) as u8; }
                let ch = Huff::new(&cl);
                let mut lens = vec![0u8; nlen + ndist];
                let mut i = 0;
                while i < nlen + ndist {
                    let sym = ch.decode(&mut b);
                    if sym < 16 { lens[i] = sym as u8; i += 1; }
                    else {
                        let (v, rep) = match sym { 16 => (lens[i - 1], 3 + b.get(2)), 17 => (0, 3 + b.get(3)), _ => (0, 11 + b.get(7)) };
                        for _ in 0..rep { lens[i] = v; i += 1; }
                    }
                }
                lit = Huff::new(&lens[..nlen]);
                dist = Huff::new(&lens[nlen..]);
            }
            loop {
                let sym = lit.decode(&mut b);
                if sym < 256 { out.push(sym as u8); }
                else if sym == 256 { break; }
                else {
                    let s = (sym - 257) as usize;
                    let len = LBASE[s] as usize + b.get(LEXT[s] as u32) as usize;
                    let ds = dist.decode(&mut b) as usize;
                    let d = DBASE[ds] as usize + b.get(DEXT[ds] as u32) as usize;
                    let start = out.len() - d;
                    for k in 0..len { let v = out[start + k]; out.push(v); }
                }
            }
        }
        if last == 1 { break; }
    }
    out
}

impl Schem {
    pub fn load(path: &str) -> Result<Schem, String> {
        let raw = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let d = inflate(&raw);
        let rd = |p: usize| -> i32 { i32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) };
        if rd(0) != 0x4d434231 { return Err("not a normalised schematic".into()); }
        let (sx, sy, sz) = (rd(4), rd(8), rd(12));
        let ulen = u16::from_be_bytes([d[28], d[29]]) as usize;
        let mut p = 30 + ulen;
        let total = sx as usize * sy as usize * sz as usize;
        let mut ids = Vec::with_capacity(total);
        while ids.len() < total {
            let mut run = 0usize;
            let mut sh = 0;
            loop { let b = d[p]; p += 1; run |= ((b & 127) as usize) << sh; sh += 7; if b < 128 { break; } }
            let id = u16::from_be_bytes([d[p], d[p + 1]]);
            p += 2;
            for _ in 0..run { ids.push(id); }
        }
        ids.truncate(total);
        Ok(Schem { sx, sy, sz, ids })
    }
    pub fn cache_file(corpus: &str, hash: &str) -> String { format!("{corpus}/cache/{}/{hash}.mcb", &hash[..2]) }
}
