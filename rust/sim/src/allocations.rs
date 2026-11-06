//! Allocation census for owned phases. Counting the module allocator includes
//! hidden Vec growth in solvers and serializers, rather than estimating it from
//! source patterns. Construction and other phases stay outside each snapshot.
use crate::gen::{F3Allocation, F3AllocationStage};
use crate::store::block::Block;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr::addr_of_mut;
use std::sync::atomic::{AtomicUsize, Ordering};

static CALLS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Counted;
const TRACE_CAPACITY: usize = 128;
static mut TRACE: [F3Allocation; TRACE_CAPACITY] = [const {
    F3Allocation {
        stage: F3AllocationStage::Other,
        bytes: 0,
        reallocation: false,
    }
}; TRACE_CAPACITY];
static mut TRACE_COUNT: usize = 0;
static mut STAGE: F3AllocationStage = F3AllocationStage::Other;
static mut RECORDING: bool = false;
static mut BLOCK: Option<Block> = None;

// Fixed records are safe even from inside the allocator: recording itself
// cannot call Vec growth, format a message or request another allocation.
fn record(bytes: usize, reallocation: bool) {
    crate::vehicles::module::f2::allocation::count();
    unsafe {
        if *addr_of_mut!(RECORDING) {
            let n = *addr_of_mut!(TRACE_COUNT);
            if n < TRACE_CAPACITY {
                (*addr_of_mut!(TRACE))[n] = F3Allocation {
                    stage: *addr_of_mut!(STAGE),
                    bytes: bytes as u32,
                    reallocation,
                };
            }
            *addr_of_mut!(TRACE_COUNT) = n + 1;
        }
    }
}

pub(crate) fn prepare() {
    let mut block = Block::default();
    block
        .words
        .reserve(crate::store::block::HEADER + TRACE_CAPACITY * F3Allocation::WORDS);
    unsafe {
        *addr_of_mut!(BLOCK) = Some(block);
    }
}
pub(crate) fn begin() {
    unsafe {
        *addr_of_mut!(TRACE_COUNT) = 0;
        *addr_of_mut!(STAGE) = F3AllocationStage::Input;
        *addr_of_mut!(RECORDING) = true;
    }
}
pub(crate) fn end() {
    unsafe {
        *addr_of_mut!(RECORDING) = false;
    }
}

pub(crate) struct Stage(F3AllocationStage);
impl Drop for Stage {
    fn drop(&mut self) {
        unsafe {
            *addr_of_mut!(STAGE) = self.0;
        }
    }
}
pub(crate) fn at(stage: F3AllocationStage) -> Stage {
    unsafe {
        let prior = *addr_of_mut!(STAGE);
        *addr_of_mut!(STAGE) = stage;
        Stage(prior)
    }
}

/// A debug read after the phase. Total counts include every allocation; this
/// bounded trace exposes the first records without adding work to publications.
#[no_mangle]
pub extern "C" fn f3_allocation_trace() -> *const f64 {
    unsafe {
        let n = (*addr_of_mut!(TRACE_COUNT)).min(TRACE_CAPACITY);
        let block = (&mut *addr_of_mut!(BLOCK))
            .as_mut()
            .expect("prepared allocation census");
        block.begin(n, F3Allocation::WORDS);
        for i in 0..n {
            let (row, heap) = block.row(i);
            (*addr_of_mut!(TRACE))[i].put(row, heap);
        }
        block.finish();
        block.ptr()
    }
}

// A module has one thread. Relaxed counters add no synchronization contract;
// their only purpose is to observe allocator traffic between phase boundaries.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        record(layout.size(), false);
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        record(layout.size(), false);
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
        record(size, true);
        System.realloc(pointer, layout, size)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

#[derive(Clone, Copy)]
pub(crate) struct Counts {
    pub calls: usize,
    pub reallocs: usize,
    pub bytes: usize,
}
pub(crate) fn snapshot() -> Counts {
    Counts {
        calls: CALLS.load(Ordering::Relaxed),
        reallocs: REALLOCS.load(Ordering::Relaxed),
        bytes: BYTES.load(Ordering::Relaxed),
    }
}
impl Counts {
    // Wrapping differences also work across the 32-bit WASM counter rollover.
    // Byte counts are requested sizes, including reallocations' new sizes.
    pub fn since(self, before: Self) -> Self {
        Self {
            calls: self.calls.wrapping_sub(before.calls),
            reallocs: self.reallocs.wrapping_sub(before.reallocs),
            bytes: self.bytes.wrapping_sub(before.bytes),
        }
    }
}
