//! A native harness opts into counting; the simulation library supplies only
//! fixed thread-local diagnostics, so independent battles never mix counts.
use bfe_sim::allocations::{record, snapshot, Counts};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::{Arc, Barrier};

thread_local! { static ENABLED: Cell<bool> = const { Cell::new(false) }; }
struct Counted;
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ENABLED.with(|e| if e.get() { record(layout.size(), false); });
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ENABLED.with(|e| if e.get() { record(layout.size(), false); });
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ENABLED.with(|e| if e.get() { record(size, true); });
        System.realloc(p, layout, size)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { System.dealloc(p, layout); }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn measured(bytes: usize) -> Counts {
    let before = snapshot();
    ENABLED.with(|e| e.set(true));
    unsafe {
        let layout = Layout::from_size_align(bytes, 8).unwrap();
        let p = std::alloc::alloc(layout);
        assert!(!p.is_null());
        let p = std::alloc::realloc(p, layout, bytes * 2);
        assert!(!p.is_null());
        std::alloc::dealloc(p, Layout::from_size_align(bytes * 2, 8).unwrap());
        let p = std::alloc::alloc_zeroed(layout);
        assert!(!p.is_null());
        std::alloc::dealloc(p, layout);
    }
    ENABLED.with(|e| e.set(false));
    snapshot().since(before)
}

#[test]
fn counting_allocator_is_nonrecursive_and_thread_local() {
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = [32, 128].into_iter().map(|bytes| {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            // A const TLS initializer must not allocate back into this hook.
            ENABLED.with(|e| e.set(true));
            let before = snapshot();
            ENABLED.with(|e| e.set(false));
            assert_eq!(before, Counts { calls: 0, reallocs: 0, bytes: 0 });
            barrier.wait();
            let counts = measured(bytes);
            barrier.wait();
            assert_eq!(snapshot(), counts);
            assert_eq!(counts, Counts { calls: 2, reallocs: 1, bytes: bytes * 4 });
        })
    }).collect();
    let before = snapshot();
    barrier.wait();
    let own = measured(64);
    barrier.wait();
    assert_eq!(own, Counts { calls: 2, reallocs: 1, bytes: 256 });
    assert_eq!(snapshot().since(before), own);
    for thread in threads { thread.join().unwrap(); }
}
