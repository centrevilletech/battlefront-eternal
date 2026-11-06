//! Calls across the boundary that are not a kind's rows (docs/sim-store.md,
//! "Commands, events and imports"): a command (JavaScript writing into
//! state Rust owns, applied at once so writes keep their order), an event
//! (Rust telling JavaScript something, drained in order at the end of the
//! phase) and an import (Rust asking JavaScript, while the state asked about
//! is still JavaScript's). Each one's arguments, result or payload is a
//! record of a schema, carried as that record's words, as a row of a block
//! carries a kind's fields.

use super::block::{Block, HEADER};
use super::word_u32;

/// The block a command's arguments come in and its result goes out in
/// (and an import's, the other way): one record's words, then its heap.
#[derive(Default)]
pub struct Calls {
    pub block: Block,
}

impl Calls {
    /// The arguments JavaScript wrote: the record's words and the heap.
    pub fn args(&self) -> (&[f64], &[f64]) { self.block.read(0) }

    /// Writes a result record of `words` words.
    pub fn result(&mut self, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
        self.block.begin(1, words);
        let (row, heap) = self.block.row(0);
        put(row, heap);
        self.block.finish();
    }

    /// Room for JavaScript to write `words` words of a record and `heap`
    /// heap words (its pointer).
    pub fn room(&mut self, words: usize, heap: usize) -> *mut f64 { self.block.room(1, words, heap) }
}

/// The events of the phase so far, in the order they were emitted: each
/// its code, its length (the words after these two), its record's words
/// and then its heap (whose offsets count from the heap's start).
pub struct Events {
    words: Vec<f64>,
    heap: Vec<f64>,
    row: Vec<f64>,
}

impl Default for Events {
    fn default() -> Self { Events { words: vec![0.0], heap: Vec::new(), row: Vec::new() } }
}

impl Events {
    /// Reserve a phase's event working set during construction. Payloads keep
    /// their generated layouts and append directly; publication need not rebuild
    /// the events already delivered at an earlier synchronous observation point.
    pub fn reserve(&mut self, words: usize, record: usize, heap: usize) {
        self.words.reserve(words.saturating_sub(self.words.len()));
        self.row.reserve(record.saturating_sub(self.row.len()));
        self.heap.reserve(heap.saturating_sub(self.heap.len()));
    }

    /// Emits an event of `code` whose record is `words` words long.
    pub fn emit(&mut self, code: u32, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
        self.row.clear();
        self.row.resize(words, 0.0);
        self.heap.clear();
        put(&mut self.row, &mut self.heap);
        self.words.push(code as f64);
        self.words.push((words + self.heap.len()) as f64);
        self.words.extend_from_slice(&self.row);
        self.words.extend_from_slice(&self.heap);
        self.words[0] = (self.words.len() - 1) as f64;
    }

    /// The events' words, after their total length (what JavaScript drains).
    pub fn ptr(&self) -> *const f64 { self.words.as_ptr() }

    pub fn snapshot(&self) -> Vec<f64> { self.words.clone() }

    pub fn len(&self) -> usize { self.words.len() - 1 }
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Forgets the events (JavaScript has handled them).
    pub fn clear(&mut self) {
        self.words.truncate(1);
        self.words[0] = 0.0;
    }
}

/// The block an import's arguments go out in and its result comes back in:
/// a fixed stretch of the module's memory (`EXCHANGE` words), laid out as a
/// one-row sync block, so JavaScript finds it where it found it the first
/// time and reads the arguments and writes the result there itself, with
/// no call for room (an import is a call each way and nothing more). Not
/// part of `Sim`: an import is called from code that holds the store, and
/// this is reached only for a moment before and after the call, never
/// across it. A handler that calls back into the module (and so, perhaps,
/// makes another import) has read its arguments first and writes its
/// result after.
pub const EXCHANGE: usize = 1 << 13;
/// An owned exchange with the same layout and capacity as the WASM facade.
/// Native contexts copy records out before dispatch, so no borrow of this
/// allocation survives a callback (including same-context reentry).
pub struct ImportExchange {
    words: [f64; EXCHANGE],
    heap: Vec<f64>,
}

impl Default for ImportExchange {
    fn default() -> Self { Self::new() }
}

impl ImportExchange {
    pub const fn new() -> Self { Self { words: [0.0; EXCHANGE], heap: Vec::new() } }

    pub fn out(&mut self, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
        self.heap.clear();
        put(&mut self.words[HEADER..HEADER + words], &mut self.heap);
        let start = HEADER + words;
        let n = self.heap.len();
        assert!(start + n <= EXCHANGE, "an import's arguments outgrow the exchange block");
        self.words[start..start + n].copy_from_slice(&self.heap);
        self.words[0] = 1.0;
        self.words[1] = words as f64;
        self.words[2] = start as f64;
        self.words[3] = n as f64;
        self.words[4] = 0.0;
    }

    pub fn read<T>(&self, take: impl FnOnce(&[f64], &[f64]) -> T) -> T {
        let words = word_u32(self.words[1]) as usize;
        let start = word_u32(self.words[2]) as usize;
        let n = word_u32(self.words[3]) as usize;
        take(&self.words[HEADER..HEADER + words], &self.words[start..start + n])
    }

    pub fn ptr(&self) -> *const f64 { self.words.as_ptr() }

    pub fn room(&mut self, words: usize, heap: usize) -> *mut f64 {
        assert!(HEADER + words + heap <= EXCHANGE, "an import's result outgrows the exchange block");
        self.words[0] = 1.0;
        self.words[1] = words as f64;
        self.words[2] = (HEADER + words) as f64;
        self.words[3] = heap as f64;
        self.words[4] = 0.0;
        self.words.as_mut_ptr()
    }
}

static mut EXCHANGE_BLOCK: ImportExchange = ImportExchange::new();

fn exchange() -> &'static mut ImportExchange {
    // WASM facade: one instance, one thread; no borrow spans an import.
    unsafe { &mut *std::ptr::addr_of_mut!(EXCHANGE_BLOCK) }
}

pub fn import_out(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) { exchange().out(words, put); }
pub fn import_in<T>(take: impl FnOnce(&[f64], &[f64]) -> T) -> T { exchange().read(take) }
pub fn import_args_ptr() -> *const f64 { exchange().ptr() }
pub fn import_room(words: usize, heap: usize) -> *mut f64 { exchange().room(words, heap) }

const _: () = assert!(HEADER == 5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_their_order_and_their_heap() {
        let mut e = Events::default();
        e.emit(3, 2, |row, heap| { row[0] = 1.5; row[1] = heap.len() as f64; heap.extend_from_slice(&[7.0, 8.0]); });
        e.emit(1, 1, |row, _| row[0] = -0.0);
        let w = unsafe { std::slice::from_raw_parts(e.ptr(), e.len() + 1) };
        assert_eq!(w[0], 9.0);
        assert_eq!(&w[1..7], &[3.0, 4.0, 1.5, 0.0, 7.0, 8.0]);
        assert_eq!((w[7], w[8]), (1.0, 1.0));
        assert_eq!(w[9].to_bits(), (-0.0f64).to_bits());
        e.clear();
        assert!(e.is_empty());
    }
}
