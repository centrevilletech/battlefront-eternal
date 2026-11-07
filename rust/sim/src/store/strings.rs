//! The instance's strings: what a `str` field holds is an index here, the
//! same on both sides (src/sim/runtime.js `StringTable` interns through
//! `sim_str_intern` and keeps a copy of each).

use super::StrId;

/// Strings as their UTF-16 units, so any JavaScript string comes back the
/// same, found by an open-addressed table of FNV-1a hashes (no HashMap:
/// its hasher and its code are more than this needs).
#[derive(Clone, Default)]
pub struct Strings {
    pub list: Vec<Vec<u16>>,
    /// Index + 1 of the string in each bucket (0 empty); a power of two long.
    table: Vec<u32>,
}

fn hash_units(units: &[u16]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &u in units {
        h ^= u as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl Strings {
    pub const fn new() -> Self { Self { list: Vec::new(), table: Vec::new() } }

    /// The string's index, made the first time it is asked for.
    pub fn intern(&mut self, units: &[u16]) -> StrId {
        if self.table.len() < (self.list.len() + 1) * 2 {
            self.rehash();
        }
        let mask = self.table.len() - 1;
        let mut b = hash_units(units) as usize & mask;
        loop {
            match self.table[b] {
                0 => break,
                i if self.list[i as usize - 1] == units => return StrId(i - 1),
                _ => b = (b + 1) & mask,
            }
        }
        let i = self.list.len() as u32;
        self.list.push(units.to_vec());
        self.table[b] = i + 1;
        StrId(i)
    }

    pub fn intern_str(&mut self, s: &str) -> StrId {
        // Repeated scalar writes normally name a string interned at load.
        // Probe the existing table over UTF-16 directly, without a temporary.
        if !self.table.is_empty() {
            let mut hash = 0x811c_9dc5u32;
            for unit in s.encode_utf16() { hash ^= unit as u32; hash = hash.wrapping_mul(0x0100_0193); }
            let mask = self.table.len() - 1; let mut bucket = hash as usize & mask;
            loop {
                match self.table[bucket] {
                    0 => break,
                    id if self.list[id as usize - 1].iter().copied().eq(s.encode_utf16()) => return StrId(id - 1),
                    _ => bucket = (bucket + 1) & mask,
                }
            }
        }
        self.intern(&s.encode_utf16().collect::<Vec<_>>())
    }

    pub fn units(&self, id: StrId) -> &[u16] { &self.list[id.0 as usize] }

    fn rehash(&mut self) {
        let size = ((self.list.len() + 1) * 4).next_power_of_two().max(16);
        self.table = vec![0; size];
        let mask = size - 1;
        for (i, units) in self.list.iter().enumerate() {
            let mut b = hash_units(units) as usize & mask;
            while self.table[b] != 0 {
                b = (b + 1) & mask;
            }
            self.table[b] = i as u32 + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_gives_one_index_a_string_in_first_asked_order() {
        let mut s = Strings::default();
        let names: Vec<String> = (0..200).map(|i| format!("name{i}")).collect();
        for (i, n) in names.iter().enumerate() {
            assert_eq!(s.intern_str(n), StrId(i as u32));
        }
        for (i, n) in names.iter().enumerate() {
            assert_eq!(s.intern_str(n), StrId(i as u32));
        }
        // A lone surrogate is kept as it is.
        let odd = s.intern(&[0x41, 0xd800]);
        assert_eq!(s.units(odd), &[0x41, 0xd800]);
    }
}
