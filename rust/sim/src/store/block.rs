//! Sync blocks: what crosses between JavaScript and Rust at a phase's
//! boundary, a kind's entities as rows of f64 in the module's memory
//! (docs/sim-store.md, "Sync blocks").

pub const HEADER: usize = crate::gen::BlockHeader::WORDS;

#[derive(Default)]
pub struct Block {
    pub words: Vec<f64>,
    pub heap: Vec<f64>,
}

impl Block {
    pub const fn new() -> Self { Self { words: Vec::new(), heap: Vec::new() } }

    /// Rows for `rows` entities of `stride` words; the heap empty.
    pub fn begin(&mut self, rows: usize, stride: usize) {
        self.words.clear();
        self.words.resize(HEADER + rows * stride, 0.0);
        self.words[crate::gen::BlockHeader::ROWS] = rows as f64;
        self.words[crate::gen::BlockHeader::STRIDE] = stride as f64;
        self.heap.clear();
    }

    /// How many of the rows (the first) are retired entities'.
    pub fn set_retired(&mut self, n: usize) { self.words[crate::gen::BlockHeader::RETIRED] = n as f64; }

    /// A row's words and the heap, to fill.
    #[inline]
    pub fn row(&mut self, i: usize) -> (&mut [f64], &mut Vec<f64>) {
        let stride = self.words[crate::gen::BlockHeader::STRIDE] as usize;
        let at = HEADER + i * stride;
        (&mut self.words[at..at + stride], &mut self.heap)
    }

    /// Puts the heap after the rows; the block is ready to be read.
    pub fn finish(&mut self) {
        let rows = self.words[crate::gen::BlockHeader::ROWS] as usize;
        let stride = self.words[crate::gen::BlockHeader::STRIDE] as usize;
        let start = HEADER + rows * stride;
        self.words.truncate(start);
        self.words[crate::gen::BlockHeader::HEAP] = start as f64;
        self.words[crate::gen::BlockHeader::HEAP_WORDS] = self.heap.len() as f64;
        self.words.extend_from_slice(&self.heap);
    }

    /// Room for JavaScript to write `rows` rows of `stride` words and
    /// `heap` heap words; returns where it starts (the call can grow
    /// memory, so JavaScript makes its view after it).
    pub fn room(&mut self, rows: usize, stride: usize, heap: usize) -> *mut f64 {
        self.words.clear();
        self.words.resize(HEADER + rows * stride + heap, 0.0);
        self.words[crate::gen::BlockHeader::ROWS] = rows as f64;
        self.words[crate::gen::BlockHeader::STRIDE] = stride as f64;
        self.words[crate::gen::BlockHeader::HEAP] = (HEADER + rows * stride) as f64;
        self.words[crate::gen::BlockHeader::HEAP_WORDS] = heap as f64;
        self.words[crate::gen::BlockHeader::RETIRED] = 0.0;
        self.words.as_mut_ptr()
    }

    pub fn rows(&self) -> usize { self.words[crate::gen::BlockHeader::ROWS] as usize }
    pub fn stride(&self) -> usize { self.words[crate::gen::BlockHeader::STRIDE] as usize }

    /// A row JavaScript wrote, and the heap.
    #[inline]
    pub fn read(&self, i: usize) -> (&[f64], &[f64]) {
        let stride = self.words[crate::gen::BlockHeader::STRIDE] as usize;
        let at = HEADER + i * stride;
        let heap = self.words[crate::gen::BlockHeader::HEAP] as usize;
        (&self.words[at..at + stride], &self.words[heap..heap + self.words[crate::gen::BlockHeader::HEAP_WORDS] as usize])
    }

    pub fn ptr(&self) -> *const f64 { self.words.as_ptr() }
}
