//! A counting global allocator for tests and examples.
//!
//! Installing a global allocator means implementing the `GlobalAlloc` trait by
//! hand; this crate holds the one implementation that tests share, so no test
//! file needs its own. A test installs it with
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();
//! ```
//!
//! and reads the counts through the functions below. Every method forwards to
//! the system allocator unchanged and only observes sizes. [`Tracking`] also
//! keeps the process-wide live and peak bytes ([`live_bytes`], [`peak_bytes`]);
//! [`Counting`] leaves that out so a timing test pays only for its own counts.
//!
//! Counts are `u32` and wrap: read a difference, never an absolute value
//! across a long run. Byte totals are exact while fewer than 4 GiB are live
//! or allocated in one measured window, which every user of this crate keeps
//! far below.
//!
//! Three families, because tests need them:
//! - per thread, always on: [`thread_allocations`], [`measure_thread`],
//!   [`measure_thread_of_size`], [`measure_thread_peak`]. Tests that run in
//!   parallel do not count each other.
//! - all threads, switched on for a window: [`measure_all`].
//! - all threads, live and peak bytes, with [`Tracking`] only: [`live_bytes`],
//!   [`peak_bytes`], [`reset_peak`].

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The allocator type to install with `#[global_allocator]`; `TRACK_LIVE` adds the
/// process-wide live and peak bytes.
pub struct Allocator<const TRACK_LIVE: bool>;

/// Counts allocation calls per thread and for a window over all threads.
pub type Counting = Allocator<false>;

/// [`Counting`] plus the process-wide live and peak bytes.
pub type Tracking = Allocator<true>;

impl<const TRACK_LIVE: bool> Allocator<TRACK_LIVE> {
    /// The value to put in the `#[global_allocator]` static.
    pub const fn new() -> Self {
        Self
    }
}

impl<const TRACK_LIVE: bool> Default for Allocator<TRACK_LIVE> {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// Allocation calls on this thread (`alloc`, `alloc_zeroed`, `realloc`).
    static TOTAL: Cell<u32> = const { Cell::new(0) };
    /// Size to watch, or 0 for none, and the calls of exactly that size.
    static WATCH: Cell<u32> = const { Cell::new(0) };
    static WATCHED: Cell<u32> = const { Cell::new(0) };
    /// Peak tracking for this thread: switch, live bytes since the start, peak.
    static PEAK_ON: Cell<bool> = const { Cell::new(false) };
    static PEAK_LIVE: Cell<i32> = const { Cell::new(0) };
    static PEAK_MAX: Cell<i32> = const { Cell::new(0) };
}

static ALL_ON: AtomicBool = AtomicBool::new(false);
static ALL_CALLS: AtomicU32 = AtomicU32::new(0);
static ALL_BYTES: AtomicU32 = AtomicU32::new(0);
static LIVE: AtomicU32 = AtomicU32::new(0);
static PEAK: AtomicU32 = AtomicU32::new(0);

/// A size as the `u32` the counters use; larger sizes saturate.
fn small(size: usize) -> u32 {
    u32::try_from(size).unwrap_or(u32::MAX)
}

fn note_call(size: usize) {
    let size = small(size);
    let _ = TOTAL.try_with(|t| t.set(t.get().wrapping_add(1)));
    let _ = WATCH.try_with(|w| {
        if w.get() != 0 && w.get() == size {
            let _ = WATCHED.try_with(|n| n.set(n.get().wrapping_add(1)));
        }
    });
    if ALL_ON.load(Ordering::Relaxed) {
        ALL_CALLS.fetch_add(1, Ordering::Relaxed);
        ALL_BYTES.fetch_add(size, Ordering::Relaxed);
    }
}

fn grew<const TRACK_LIVE: bool>(bytes: usize) {
    if !TRACK_LIVE {
        return;
    }
    let bytes = small(bytes);
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed).wrapping_add(bytes);
    PEAK.fetch_max(now, Ordering::Relaxed);
}

fn shrank<const TRACK_LIVE: bool>(bytes: usize) {
    if TRACK_LIVE {
        LIVE.fetch_sub(small(bytes), Ordering::Relaxed);
    }
}

fn note_thread_live(delta: i32) {
    let _ = PEAK_ON.try_with(|on| {
        if on.get() {
            let now = PEAK_LIVE.with(|l| {
                l.set(l.get().saturating_add(delta));
                l.get()
            });
            PEAK_MAX.with(|p| p.set(p.get().max(now)));
        }
    });
}

fn signed(size: usize) -> i32 {
    i32::try_from(size).unwrap_or(i32::MAX)
}

// SAFETY: every method forwards to `System` with the arguments it received and
// returns its result; the counters only read the sizes.
unsafe impl<const TRACK_LIVE: bool> GlobalAlloc for Allocator<TRACK_LIVE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_call(layout.size());
        note_thread_live(signed(layout.size()));
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew::<TRACK_LIVE>(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_call(layout.size());
        note_thread_live(signed(layout.size()));
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew::<TRACK_LIVE>(layout.size());
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_call(new_size);
        // The old and the new block may coexist while the contents move.
        note_thread_live(signed(new_size));
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        note_thread_live(signed(layout.size()).saturating_neg());
        if !p.is_null() {
            shrank::<TRACK_LIVE>(layout.size());
            grew::<TRACK_LIVE>(new_size);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_thread_live(signed(layout.size()).saturating_neg());
        unsafe { System.dealloc(ptr, layout) };
        shrank::<TRACK_LIVE>(layout.size());
    }
}

/// Allocation calls made on this thread so far (wraps; read a difference).
pub fn thread_allocations() -> u32 {
    TOTAL.try_with(Cell::get).unwrap_or(0)
}

/// Run `f` and return its result and the allocation calls it made on this thread.
pub fn measure_thread<R>(f: impl FnOnce() -> R) -> (R, u32) {
    let before = thread_allocations();
    let out = f();
    (out, thread_allocations().wrapping_sub(before))
}

/// Like [`measure_thread`] but counting only calls whose requested size is exactly `size`
/// (`size` must not be 0).
pub fn measure_thread_of_size<R>(size: u32, f: impl FnOnce() -> R) -> (R, u32) {
    assert!(size != 0, "a size to watch is not 0");
    WATCHED.with(|n| n.set(0));
    WATCH.with(|w| w.set(size));
    let out = f();
    WATCH.with(|w| w.set(0));
    (out, WATCHED.with(Cell::get))
}

/// Run `f` and return its result, the peak of live bytes it reached on this thread above the
/// start (never below 0) and the net change of live bytes. Allocations made by `f` and freed by
/// another thread count as live here.
pub fn measure_thread_peak<R>(f: impl FnOnce() -> R) -> (R, u32, i32) {
    PEAK_LIVE.with(|l| l.set(0));
    PEAK_MAX.with(|p| p.set(0));
    PEAK_ON.with(|on| on.set(true));
    let out = f();
    PEAK_ON.with(|on| on.set(false));
    let peak = u32::try_from(PEAK_MAX.with(Cell::get).max(0)).unwrap_or(0);
    (out, peak, PEAK_LIVE.with(Cell::get))
}

/// What [`measure_all`] counted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Totals {
    /// Calls to `alloc`, `alloc_zeroed` and `realloc` on every thread.
    pub allocations: u32,
    /// Bytes those calls asked for (the new size on a `realloc`).
    pub bytes: u32,
}

/// Switch counting on for every thread of the process and start from zero. Two windows must not
/// overlap: tests that use this serialize themselves.
pub fn start_all() {
    ALL_CALLS.store(0, Ordering::SeqCst);
    ALL_BYTES.store(0, Ordering::SeqCst);
    ALL_ON.store(true, Ordering::SeqCst);
}

/// Switch the window off and return what it counted.
pub fn finish_all() -> Totals {
    ALL_ON.store(false, Ordering::SeqCst);
    Totals {
        allocations: ALL_CALLS.load(Ordering::SeqCst),
        bytes: ALL_BYTES.load(Ordering::SeqCst),
    }
}

/// Run `f` inside a window over every thread and return its result and what was counted.
pub fn measure_all<R>(f: impl FnOnce() -> R) -> (R, Totals) {
    start_all();
    let out = f();
    (out, finish_all())
}

/// Bytes live in the process now (wraps below the start of the process's own allocations: read a
/// difference taken after [`reset_peak`] or two readings).
pub fn live_bytes() -> u32 {
    LIVE.load(Ordering::Relaxed)
}

/// The most [`live_bytes`] reached since the last [`reset_peak`].
pub fn peak_bytes() -> u32 {
    PEAK.load(Ordering::Relaxed)
}

/// Start the peak again from the bytes live now.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}
