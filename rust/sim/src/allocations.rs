//! Allocation census for owned phases. Fixed instrumentation cannot allocate
//! from inside the allocator. Its encoded output is prepared separately, so
//! recording never borrows a simulation owner or its publication bank.
use crate::gen::{F3Allocation, F3AllocationStage};
use crate::store::block::Block;
use std::sync::atomic::{AtomicUsize, Ordering};

const TRACE_CAPACITY: usize = 128;

/// Allocation state with an explicit lifetime, independent of allocator dispatch.
/// Native callers can attribute an entry scope to their own diagnostics. The
/// WASM facade observes its one instance through narrow, nonallocating loans.
pub struct Diagnostics {
    totals: Totals,
    trace: Trace,
    output: Option<Block>,
    #[cfg(target_arch = "wasm32")]
    fleet: crate::vehicles::module::f2::allocation::Census,
}
struct Totals { calls: AtomicUsize, reallocs: AtomicUsize, bytes: AtomicUsize }
struct Trace {
    rows: [F3Allocation; TRACE_CAPACITY],
    count: usize,
    stage: F3AllocationStage,
    recording: bool,
}
impl Default for Diagnostics { fn default() -> Self { Self::new() } }
impl Diagnostics {
    pub const fn new() -> Self {
        Self { totals: Totals::new(), trace: Trace::new(), output: None,
            #[cfg(target_arch = "wasm32")]
            fleet: crate::vehicles::module::f2::allocation::Census::new() }
    }
    pub fn prepare(&mut self) { self.output = Some(prepared_output()); }
    pub fn begin(&mut self) { self.trace.begin(); }
    pub fn end(&mut self) { self.trace.recording = false; }
    pub fn stage(&mut self, stage: F3AllocationStage) -> F3AllocationStage { self.trace.stage(stage) }
    /// Explicit attribution uses the same fixed recorder as allocator dispatch.
    pub fn record(&mut self, bytes: usize, reallocation: bool) {
        self.totals.record(bytes, reallocation);
        self.trace.record(bytes, reallocation);
    }
    pub fn snapshot(&self) -> Counts { self.totals.snapshot() }
    pub fn publish(&mut self) -> *const f64 {
        encode(&self.trace, self.output.as_mut().expect("prepared allocation census"))
    }
}
impl Totals {
    const fn new() -> Self {
        Self { calls: AtomicUsize::new(0), reallocs: AtomicUsize::new(0), bytes: AtomicUsize::new(0) }
    }
    fn record(&self, bytes: usize, reallocation: bool) {
        let counter = if reallocation { &self.reallocs } else { &self.calls };
        counter.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    fn snapshot(&self) -> Counts {
        Counts { calls: self.calls.load(Ordering::Relaxed), reallocs: self.reallocs.load(Ordering::Relaxed), bytes: self.bytes.load(Ordering::Relaxed) }
    }
}
impl Trace {
    const fn new() -> Self {
        Self { rows: [const { F3Allocation { stage: F3AllocationStage::Other, bytes: 0, reallocation: false } }; TRACE_CAPACITY],
            count: 0, stage: F3AllocationStage::Other, recording: false }
    }
    fn record(&mut self, bytes: usize, reallocation: bool) {
        if !self.recording { return; }
        let n = self.count;
        if n < TRACE_CAPACITY {
            self.rows[n] = F3Allocation { stage: self.stage, bytes: bytes as u32, reallocation };
        }
        self.count = n.saturating_add(1);
    }
    fn begin(&mut self) { self.count = 0; self.stage = F3AllocationStage::Input; self.recording = true; }
    fn stage(&mut self, stage: F3AllocationStage) -> F3AllocationStage {
        std::mem::replace(&mut self.stage, stage)
    }
}
fn prepared_output() -> Block {
    let mut block = Block::default();
    block.words.reserve(crate::store::block::HEADER + TRACE_CAPACITY * F3Allocation::WORDS);
    block
}
fn encode(trace: &Trace, block: &mut Block) -> *const f64 {
    let n = trace.count.min(TRACE_CAPACITY);
    block.begin(n, F3Allocation::WORDS);
    for (i, allocation) in trace.rows[..n].iter().enumerate() {
        let (row, heap) = block.row(i);
        allocation.put(row, heap);
    }
    block.finish();
    block.ptr()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts { pub calls: usize, pub reallocs: usize, pub bytes: usize }
impl Counts {
    // Wrapping differences also work across the 32-bit WASM counter rollover.
    // Byte counts are requested sizes, including reallocations' new sizes.
    pub fn since(self, before: Self) -> Self {
        Self { calls: self.calls.wrapping_sub(before.calls), reallocs: self.reallocs.wrapping_sub(before.reallocs), bytes: self.bytes.wrapping_sub(before.bytes) }
    }
}

#[cfg(target_arch = "wasm32")]
mod facade {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::ptr::{addr_of, addr_of_mut};

    // The allocator object is stateless. Only fixed instrumentation is lent
    // here: output encoding may use its disjoint, already reserved block.
    static mut DIAGNOSTICS: Diagnostics = Diagnostics::new();
    struct Counted;
    fn record(bytes: usize, reallocation: bool) {
        crate::vehicles::module::f2::allocation::count();
        unsafe {
            (&*addr_of!(DIAGNOSTICS.totals)).record(bytes, reallocation);
            (&mut *addr_of_mut!(DIAGNOSTICS.trace)).record(bytes, reallocation);
        }
    }
    pub(crate) fn prepare() {
        // Reserving can itself call the allocator. Finish it before taking the
        // output loan, and never lend the enclosing Diagnostics across it.
        let output = prepared_output();
        unsafe { *addr_of_mut!(DIAGNOSTICS.output) = Some(output); }
    }
    // The allocation hook may run during an allocation by Fleet itself. Lend
    // only these fixed diagnostic cells, never the semantic Fleet root.
    pub(crate) fn fleet<R>(run: impl FnOnce(&mut crate::vehicles::module::f2::allocation::Census) -> R) -> R {
        unsafe { run(&mut *addr_of_mut!(DIAGNOSTICS.fleet)) }
    }
    pub(crate) fn begin() { unsafe { (&mut *addr_of_mut!(DIAGNOSTICS.trace)).begin(); } }
    pub(crate) fn end() { unsafe { (*addr_of_mut!(DIAGNOSTICS.trace)).recording = false; } }
    pub(crate) fn snapshot() -> Counts { unsafe { (&*addr_of!(DIAGNOSTICS.totals)).snapshot() } }
    pub(crate) struct Stage(F3AllocationStage);
    impl Drop for Stage {
        fn drop(&mut self) { unsafe { (*addr_of_mut!(DIAGNOSTICS.trace)).stage(self.0); } }
    }
    pub(crate) fn at(stage: F3AllocationStage) -> Stage {
        Stage(unsafe { (&mut *addr_of_mut!(DIAGNOSTICS.trace)).stage(stage) })
    }
    /// Read after the phase. Total counters include every request; this trace
    /// retains only the first records, without growing the publication bank.
    #[no_mangle]
    pub extern "C" fn f3_allocation_trace() -> *const f64 {
        unsafe { encode(&*addr_of!(DIAGNOSTICS.trace), (&mut *addr_of_mut!(DIAGNOSTICS.output)).as_mut().expect("prepared allocation census")) }
    }
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 { record(layout.size(), false); System.alloc(layout) }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { record(layout.size(), false); System.alloc_zeroed(layout) }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 { record(size, true); System.realloc(pointer, layout, size) }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { System.dealloc(pointer, layout); }
    }
    #[global_allocator]
    static ALLOCATOR: Counted = Counted;
}
#[cfg(target_arch = "wasm32")]
pub(crate) use facade::{at, begin, end, fleet, prepare, snapshot};
