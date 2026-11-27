//! The process-wide live and peak byte counters follow the heap. This file holds one test so
//! nothing else in the process allocates in parallel.

use bfe_test_alloc::{live_bytes, peak_bytes, reset_peak, Tracking};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: Tracking = Tracking::new();

#[test]
fn live_and_peak_bytes_follow_the_heap() {
    reset_peak();
    let base = live_bytes();
    let held = black_box(vec![0u8; 50_000]);
    assert!(live_bytes().wrapping_sub(base) >= 50_000);
    drop(held);
    assert!(peak_bytes().wrapping_sub(base) >= 50_000);
    assert!(live_bytes().wrapping_sub(base) < 50_000);
}
