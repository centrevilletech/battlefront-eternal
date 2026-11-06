//! Count only the measured phase's test thread. Parallel corpus/oracle tests
//! cannot add requests to its warmed allocation result.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
pub struct Counted;
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ENABLED.with(|e| { if e.get() { COUNT.with(|n| n.set(n.get() + 1)); } }); System.alloc(layout)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { System.dealloc(p, layout); }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ENABLED.with(|e| { if e.get() { COUNT.with(|n| n.set(n.get() + 1)); } }); System.realloc(p, layout, size)
    }
}
pub fn start() { COUNT.with(|n| n.set(0)); ENABLED.with(|e| e.set(true)); }
pub fn stop() -> usize { ENABLED.with(|e| e.set(false)); COUNT.with(Cell::get) }
