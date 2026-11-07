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
    fleet: crate::vehicles::allocation::Census,
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
            fleet: crate::vehicles::allocation::Census::new() }
    }
    pub fn prepare(&mut self) { self.output = Some(prepared_output()); }
    pub fn begin(&mut self) { self.trace.begin(); }
    pub fn end(&mut self) { self.trace.recording = false; }
    pub fn stage(&mut self, stage: F3AllocationStage) -> F3AllocationStage { self.trace.stage(stage) }
    /// Explicit attribution uses the same fixed recorder as allocator dispatch.
    pub fn record(&mut self, bytes: usize, reallocation: bool) {
        self.totals.record(bytes, reallocation);
        self.trace.record(bytes, reallocation);
        self.fleet.record();
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

// Project only the field being observed. In particular, neither preparing
// the output nor publishing it may lend the entire cell to allocator hooks.
use std::ptr::{addr_of, addr_of_mut};
#[cfg(target_arch = "wasm32")]
mod storage {
    use super::*;
    static mut DIAGNOSTICS: Diagnostics = Diagnostics::new();
    pub(super) fn with<R>(run: impl FnOnce(*mut Diagnostics) -> R) -> R {
        run(addr_of_mut!(DIAGNOSTICS))
    }
    pub(super) fn record(bytes: usize, reallocation: bool) {
        with(|d| unsafe { record_in(d, bytes, reallocation) });
    }
}
#[cfg(not(target_arch = "wasm32"))]
mod storage {
    use super::*;
    use std::cell::UnsafeCell;
    thread_local! {
        static DIAGNOSTICS: UnsafeCell<Diagnostics> = const { UnsafeCell::new(Diagnostics::new()) };
    }
    pub(super) fn with<R>(run: impl FnOnce(*mut Diagnostics) -> R) -> R {
        DIAGNOSTICS.with(|d| run(d.get()))
    }
    pub(super) fn record(bytes: usize, reallocation: bool) {
        // A harness may allocate from another TLS destructor after this cell
        // has been destroyed. Shutdown cannot revive or panic its recorder.
        let _ = DIAGNOSTICS.try_with(|d| unsafe { record_in(d.get(), bytes, reallocation) });
    }
}
unsafe fn record_in(d: *mut Diagnostics, bytes: usize, reallocation: bool) {
    (&*addr_of!((*d).totals)).record(bytes, reallocation);
    (&mut *addr_of_mut!((*d).trace)).record(bytes, reallocation);
    (&mut *addr_of_mut!((*d).fleet)).record();
}
/// Allocator hook. Native totals stay zero unless a harness calls this from
/// its counting allocator; the library never installs a native allocator.
pub fn record(bytes: usize, reallocation: bool) { storage::record(bytes, reallocation); }
pub(crate) fn prepare() {
    let output = prepared_output();
    storage::with(|d| unsafe { *addr_of_mut!((*d).output) = Some(output); });
}
pub(crate) fn fleet<R>(run: impl FnOnce(&mut crate::vehicles::allocation::Census) -> R) -> R {
    storage::with(|d| unsafe { run(&mut *addr_of_mut!((*d).fleet)) })
}
pub(crate) fn begin() { storage::with(|d| unsafe { (&mut *addr_of_mut!((*d).trace)).begin(); }); }
pub(crate) fn end() { storage::with(|d| unsafe { (*addr_of_mut!((*d).trace)).recording = false; }); }
pub fn snapshot() -> Counts { storage::with(|d| unsafe { (&*addr_of!((*d).totals)).snapshot() }) }
pub(crate) struct Stage(F3AllocationStage);
impl Drop for Stage {
    fn drop(&mut self) { storage::with(|d| unsafe { (*addr_of_mut!((*d).trace)).stage(self.0); }); }
}
pub(crate) fn at(stage: F3AllocationStage) -> Stage {
    Stage(storage::with(|d| unsafe { (&mut *addr_of_mut!((*d).trace)).stage(stage) }))
}
/// Read after the phase. Total counters include every request; this trace
/// retains only the first records, without growing the publication bank.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn f3_allocation_trace() -> *const f64 {
    storage::with(|d| unsafe { encode(&*addr_of!((*d).trace), (&mut *addr_of_mut!((*d).output)).as_mut().expect("prepared allocation census")) })
}

#[cfg(target_arch = "wasm32")]
mod allocator {
    use super::record;
    use std::alloc::{GlobalAlloc, Layout, System};
    struct Counted;
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 { record(layout.size(), false); System.alloc(layout) }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { record(layout.size(), false); System.alloc_zeroed(layout) }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 { record(size, true); System.realloc(pointer, layout, size) }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { System.dealloc(pointer, layout); }
    }
    #[global_allocator]
    static ALLOCATOR: Counted = Counted;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::vehicles::allocation as fleet_census;

    #[test]
    fn ordinary_native_allocations_do_not_install_a_counter() {
        let before = snapshot();
        let bytes = std::hint::black_box(vec![17_u8; 4096]);
        assert_eq!(snapshot().since(before), Counts { calls: 0, reallocs: 0, bytes: 0 });
        std::hint::black_box(bytes);
    }

    #[test]
    fn scoped_stages_and_fleet_counts_share_one_bounded_trace() {
        prepare();
        begin();
        fleet_census::begin();
        let before = snapshot();
        {
            let _stage = at(F3AllocationStage::Other);
            let _group = fleet_census::enter(fleet_census::ORDNANCE);
            let _slot = fleet_census::nested(fleet_census::Stage::Fire);
            record(11, false);
        }
        // Restored scopes attribute this overflow to the outer phase. The
        // trace clips, while total and fleet counters retain every request.
        for _ in 0..TRACE_CAPACITY + 7 { record(23, true); }
        end();
        fleet_census::end();
        assert_eq!(snapshot().since(before), Counts { calls: 1, reallocs: TRACE_CAPACITY + 7, bytes: 11 + 23 * (TRACE_CAPACITY + 7) });
        assert_eq!(fleet_census::f2_allocations(), (TRACE_CAPACITY + 8) as u32);
        assert_eq!(fleet_census::f2_ordnance_allocations(), 1);
        assert_eq!(fleet_census::f2_core_allocations(), (TRACE_CAPACITY + 7) as u32);
        assert_eq!(fleet_census::f2_stage_allocations(fleet_census::Stage::Fire as usize), 1);
        storage::with(|d| unsafe {
            assert_eq!((*d).trace.rows[0].stage, F3AllocationStage::Other);
            assert_eq!((*d).trace.rows[1].stage, F3AllocationStage::Input);
        });
        let before = snapshot();
        f3_allocation_trace();
        assert_eq!(snapshot(), before);
        storage::with(|d| unsafe { assert_eq!((*d).output.as_ref().unwrap().rows(), TRACE_CAPACITY); });
    }
}
