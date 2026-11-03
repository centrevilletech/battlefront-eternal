//! Sync blocks: what crosses between JavaScript and Rust at a phase's
//! boundary, a kind's entities as rows of f64 in the module's memory
//! (docs/sim-store.md, "Sync blocks").
//!
//! ```text
//! [0] rows  [1] stride (f64 a row)  [2] where the heap starts  [3] heap length
//! [4] retired rows (the first rows: entities removed since the last export)
//! rows: slot, generation, then the fields the schema lists, in its order
//! heap: what a row's lists, sets and maps hold (a row has their start and
//! length; a map's entries are its key's string and its value's words)
//! ```
//!
//! After a phase in Rust, the generated export loop writes the rows of the
//! kinds it changed and JavaScript's generated loop copies them into the
//! mirror objects unported code reads. Before a phase in Rust, JavaScript's
//! generated import loop writes the fields of objects it still owns that the
//! phase reads, and Rust's copies them in. Every field is a number in a
//! row: a boolean 0 or 1, an enum its index, a reference the slot and
//! generation of what it names, a string or definition its index, a
//! Vector3 three numbers; a field that can be null or undefined takes a
//! word for its tag first (as in the capture format).

pub const HEADER: usize = 5;

#[derive(Default)]
pub struct Block {
    pub words: Vec<f64>,
    pub heap: Vec<f64>,
}

impl Block {
    /// Rows for `rows` entities of `stride` words; the heap empty.
    pub fn begin(&mut self, rows: usize, stride: usize) {
        self.words.clear();
        self.words.resize(HEADER + rows * stride, 0.0);
        self.words[0] = rows as f64;
        self.words[1] = stride as f64;
        self.heap.clear();
    }

    /// How many of the rows (the first) are retired entities'.
    pub fn set_retired(&mut self, n: usize) { self.words[4] = n as f64; }

    /// A row's words and the heap, to fill.
    #[inline]
    pub fn row(&mut self, i: usize) -> (&mut [f64], &mut Vec<f64>) {
        let stride = self.words[1] as usize;
        let at = HEADER + i * stride;
        (&mut self.words[at..at + stride], &mut self.heap)
    }

    /// Puts the heap after the rows; the block is ready to be read.
    pub fn finish(&mut self) {
        let rows = self.words[0] as usize;
        let stride = self.words[1] as usize;
        let start = HEADER + rows * stride;
        self.words.truncate(start);
        self.words[2] = start as f64;
        self.words[3] = self.heap.len() as f64;
        self.words.extend_from_slice(&self.heap);
    }

    /// Room for JavaScript to write `rows` rows of `stride` words and
    /// `heap` heap words; returns where it starts (the call can grow
    /// memory, so JavaScript makes its view after it).
    pub fn room(&mut self, rows: usize, stride: usize, heap: usize) -> *mut f64 {
        self.words.clear();
        self.words.resize(HEADER + rows * stride + heap, 0.0);
        self.words[0] = rows as f64;
        self.words[1] = stride as f64;
        self.words[2] = (HEADER + rows * stride) as f64;
        self.words[3] = heap as f64;
        self.words[4] = 0.0;
        self.words.as_mut_ptr()
    }

    pub fn rows(&self) -> usize { self.words[0] as usize }
    pub fn stride(&self) -> usize { self.words[1] as usize }

    /// A row JavaScript wrote, and the heap.
    #[inline]
    pub fn read(&self, i: usize) -> (&[f64], &[f64]) {
        let stride = self.words[1] as usize;
        let at = HEADER + i * stride;
        let heap = self.words[2] as usize;
        (&self.words[at..at + stride], &self.words[heap..heap + self.words[3] as usize])
    }

    pub fn ptr(&self) -> *const f64 { self.words.as_ptr() }
}
