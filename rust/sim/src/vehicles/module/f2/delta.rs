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
        self.encode_from(slot, generation, next, last, false)
    }
    // A synchronous consumer may borrow changed words in the retained cache.
    // The acknowledgement still owns their lifetime; only the extra values
    // vector and its second publication copy are omitted.
    fn encode_from(&mut self, slot: u32, generation: u32, next: &Block, last: &Block, borrowed: bool) -> bool {
        self.value.source = 0;
        let mut changed = next.words.len() != last.words.len();
        self.value.slot = slot; self.value.generation = generation; self.value.length = next.words.len() as u32;
        self.value.mask.clear(); self.value.mask.resize(next.words.len().div_ceil(MASK_BITS), 0);
        self.value.values.clear();
        // Most words retain their bits, including optional-record padding.
        // Bound the common span once rather than checking the old row's
        // length for every scalar. New heap words still publish in order.
        let common = next.words.len().min(last.words.len());
        for (chunk, words) in next.words[..common].chunks(MASK_BITS).enumerate() {
            let start = chunk * MASK_BITS;
            let bits = changed_bits(words, &last.words[start..start + words.len()]);
            self.value.mask[chunk] = bits;
            // Sparse values keep their original ascending word order. Compare
            // an entire mask word first so unchanged spans need no pushes or
            // repeated writes to the mask's retained vector.
            changed |= bits != 0;
            let mut pending = if borrowed { 0 } else { bits };
            while pending != 0 {
                let bit = pending.trailing_zeros() as usize;
                self.value.values.push(words[bit]);
                pending &= pending - 1;
            }
        }
        for (at, &word) in next.words[common..].iter().enumerate() {
            let i = common + at;
            self.value.mask[i / MASK_BITS] |= 1 << (i % MASK_BITS);
            if !borrowed { self.value.values.push(word); }
        }
        changed
    }
    // F2Delta is already a retained typed record. Its WASM source address is
    // bound only by the boundary wrapper, without another encoding or buffer.
    #[cfg(target_arch = "wasm32")]
    fn descriptor(&mut self, source: &Block, borrowed: bool) {
        self.value.source = if borrowed { source.ptr() as u32 } else { 0 };
    }
}

// Cache comparison is numeric storage work, not gameplay arithmetic. Integer
// lanes compare f64 bits so signed zero and NaN payloads remain distinct.
// Audio already requires SIMD in the browser; native replay uses the same
// bit decisions without introducing a second simulation path.
#[cfg(target_arch = "wasm32")]
fn changed_bits(next: &[f64], last: &[f64]) -> u32 {
    // Both slices have the same bounded length, at most one mask word.
    unsafe { changed_bits_simd(next, last) }
}
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn changed_bits_simd(next: &[f64], last: &[f64]) -> u32 {
    use core::arch::wasm32::*;
    let mut mask = 0;
    let mut i = 0;
    while i + 2 <= next.len() {
        let a = v128_load(next.as_ptr().add(i).cast());
        let b = v128_load(last.as_ptr().add(i).cast());
        mask |= (3 ^ i64x2_bitmask(i64x2_eq(a, b)) as u32) << i;
        i += 2;
    }
    if i < next.len() { mask |= ((next[i].to_bits() != last[i].to_bits()) as u32) << i; }
    mask
}
#[cfg(not(target_arch = "wasm32"))]
fn changed_bits(next: &[f64], last: &[f64]) -> u32 {
    let mut mask = 0;
    for (i, (a, b)) in next.iter().zip(last).enumerate() {
        mask |= ((a.to_bits() != b.to_bits()) as u32) << i;
    }
    mask
}

/// A generated row with retained encoding and its last published bits.
pub struct Cache { pub next: Block, pub last: Block, pub delta: Delta, identity: Option<(u32,u32)>, borrowed: bool }
impl Cache {
    pub fn new(words: usize) -> Self { Self::with_storage(words, false) }
    /// F3 consumes shared-memory rows synchronously before acknowledging them.
    /// F2 can retain its packed values for the same generated delta reader.
    pub fn borrowed(words: usize) -> Self { Self::with_storage(words, true) }
    fn with_storage(words: usize, borrowed: bool) -> Self {
        // A native descriptor has no address. Retain its changed values so
        // F3 and F4 readers can apply the same generated sparse rows.
        let borrowed = borrowed && cfg!(target_arch = "wasm32");
        let mut delta = Delta::default();
        delta.value.mask.reserve(words.div_ceil(MASK_BITS));
        if !borrowed { delta.value.values.reserve(words); }
        Self {
            next: Block { words: Vec::with_capacity(words), heap: Vec::with_capacity(words) },
            // Only encoded words become the consumed baseline. The writer's
            // one scratch heap stays with next across acknowledgements.
            last: Block { words: Vec::with_capacity(words), heap: Vec::new() },
            delta, identity: None, borrowed,
        }
    }
    pub fn borrowed_words(&self) -> usize {
        if !self.borrowed { return 0; }
        self.delta.value.mask.iter().map(|bits| bits.count_ones() as usize).sum()
    }
    pub fn invalidate(&mut self) { self.identity = None; }
    /// Prepare against the last consumed bits. A shared-memory reader may
    /// acknowledge several publications together after a native callback.
    pub fn prepare(&mut self, slot: u32, generation: u32, width: usize, write: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> bool {
        let changed = self.prepare_rows(slot, generation, width, write);
        #[cfg(target_arch = "wasm32")]
        self.delta.descriptor(&self.next, self.borrowed);
        changed
    }
    pub fn prepare_rows(&mut self, slot: u32, generation: u32, width: usize, write: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> bool {
        if self.identity != Some((slot,generation)) { self.last.words.clear(); self.identity = Some((slot,generation)); }
        self.next.begin(1,width); let (row,heap) = self.next.row(0); write(row,heap); self.next.finish();
        self.delta.encode_from(slot,generation,&self.next,&self.last,self.borrowed)
    }
    /// A movement-only observer changes fixed generated fields. Reuse the
    /// consumed heap and compare just those fields; a cold/rebound row instead
    /// needs the complete encoder. No pending combat delta may take this path.
    pub fn prepare_retained(&mut self, slot:u32, generation:u32, width:usize,
        ranges:&[std::ops::Range<usize>],write:impl FnOnce(&mut [f64],&mut Vec<f64>)) -> Option<bool> {
        let changed = self.prepare_retained_rows(slot, generation, width, ranges, write);
        #[cfg(target_arch = "wasm32")]
        if changed.is_some() { self.delta.descriptor(&self.next, self.borrowed); }
        changed
    }
    pub fn prepare_retained_rows(&mut self, slot:u32, generation:u32, width:usize,
        ranges:&[std::ops::Range<usize>],write:impl FnOnce(&mut [f64],&mut Vec<f64>)) -> Option<bool> {
        if self.identity!=Some((slot,generation)) || self.last.words.len()<crate::store::block::HEADER+width {return None;}
        // Acknowledgement swaps the two complete buffers. Their last mask
        // describes the difference in either direction; restore just those
        // words instead of copying the unchanged weapon heap for an AI intent.
        // A changed variable-list shape still needs the complete baseline.
        if self.next.words.len()!=self.last.words.len() {
            self.next.words.clear();self.next.words.extend_from_slice(&self.last.words);
        } else {
            for (chunk,&mask) in self.delta.value.mask.iter().enumerate() {
                let mut pending=mask;
                while pending!=0 {
                    let index=chunk*MASK_BITS+pending.trailing_zeros() as usize;
                    self.next.words[index]=self.last.words[index];
                    pending&=pending-1;
                }
            }
        }
        self.next.heap.clear();
        let base=crate::store::block::HEADER;
        write(&mut self.next.words[base..base+width],&mut self.next.heap);
        debug_assert!(self.next.heap.is_empty(),"retained fields have no heap spans");
        let value=&mut self.delta.value;
        value.slot=slot;value.generation=generation;value.length=self.next.words.len() as u32;
        value.source=0;
        let mut changed=false;
        value.mask.clear();value.mask.resize(self.next.words.len().div_ceil(MASK_BITS),0);value.values.clear();
        for range in ranges {
            for i in range.start..range.end {
                let word=self.next.words[i];
                if word.to_bits()==self.last.words[i].to_bits() {continue;}
                value.mask[i/MASK_BITS]|=1<<(i%MASK_BITS);
                changed=true;
                if !self.borrowed {value.values.push(word);}
            }
        }
        Some(changed)
    }
    pub fn commit(&mut self) { std::mem::swap(&mut self.next.words, &mut self.last.words); }
    pub fn encode(&mut self, slot:u32, generation:u32, width:usize, write:impl FnOnce(&mut [f64],&mut Vec<f64>)) -> bool {
        let changed=self.prepare(slot,generation,width,write);
        if changed {self.commit();}
        changed
    }
}
