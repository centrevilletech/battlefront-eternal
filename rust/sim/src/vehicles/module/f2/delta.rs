//! Sparse numeric publications over generated records. Each changed word is
//! sent once, in mask order, including signed zero and NaN payloads. Readers
//! retain their encoded row; variable heaps use the same generated block ABI.
use crate::gen::F2Delta;
use crate::store::block::Block;
pub const MASK_BITS: usize = 32;
#[derive(Default)]
pub struct Delta { pub value: F2Delta }
impl Delta {
    pub fn reserve(&mut self, words: usize) {
        self.value.mask.reserve(words.div_ceil(MASK_BITS)); self.value.values.reserve(words);
    }
    pub fn encode(&mut self, slot: u32, generation: u32, next: &Block, last: &Block) -> bool {
        self.value.slot = slot; self.value.generation = generation; self.value.length = next.words.len() as u32;
        self.value.mask.clear(); self.value.mask.resize(next.words.len().div_ceil(MASK_BITS), 0);
        self.value.values.clear();
        for (i, &word) in next.words.iter().enumerate() {
            if last.words.get(i).is_some_and(|old| old.to_bits() == word.to_bits()) { continue; }
            self.value.mask[i / MASK_BITS] |= 1 << (i % MASK_BITS); self.value.values.push(word);
        }
        !self.value.values.is_empty() || next.words.len() != last.words.len()
    }
}

/// A generated row with retained encoding and its last published bits.
pub struct Cache { pub next: Block, pub last: Block, pub delta: Delta, identity: Option<(u32,u32)> }
impl Cache {
    pub fn new(words: usize) -> Self {
        let mut delta = Delta::default(); delta.reserve(words);
        Self { next: Block { words: Vec::with_capacity(words), heap: Vec::with_capacity(words) },
            last: Block { words: Vec::with_capacity(words), heap: Vec::with_capacity(words) }, delta, identity: None }
    }
    pub fn invalidate(&mut self) { self.identity = None; }
    pub fn encode(&mut self, slot: u32, generation: u32, width: usize, write: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> bool {
        if self.identity != Some((slot,generation)) { self.last.words.clear(); self.identity = Some((slot,generation)); }
        self.next.begin(1,width); let (row,heap) = self.next.row(0); write(row,heap); self.next.finish();
        let changed = self.delta.encode(slot,generation,&self.next,&self.last);
        if changed { std::mem::swap(&mut self.next,&mut self.last); }
        changed
    }
}
