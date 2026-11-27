//! The counters count: every function reads a known number of allocations, so an allocator that
//! counts nothing (and so passes every zero-allocation test) fails here.

use bfe_test_alloc::{
    measure_all, measure_thread, measure_thread_of_size, measure_thread_peak, Counting,
};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: Counting = Counting::new();

#[test]
fn a_thread_counts_its_own_calls() {
    let (_, none) = measure_thread(|| black_box(1 + 1));
    assert_eq!(none, 0);
    let (kept, three) = measure_thread(|| {
        let a = black_box(Box::new(7u32));
        let b = black_box(Box::new(8u32));
        let c = black_box(Box::new(9u32));
        (a, b, c)
    });
    assert_eq!(three, 3);
    drop(kept);
}

#[test]
fn a_grown_vector_counts_its_reallocations() {
    let (v, calls) = measure_thread(|| {
        let mut v: Vec<u8> = Vec::new();
        v.push(1);
        v.reserve_exact(100);
        v
    });
    assert_eq!(calls, 2, "one allocation and one growth");
    drop(v);
}

#[test]
fn another_thread_is_not_counted_here() {
    let (_, here) = measure_thread_of_size(4097, || drop(black_box(vec![0u8; 4097])));
    assert_eq!(here, 1);
    let (_, there) = measure_thread_of_size(4097, || {
        std::thread::spawn(|| drop(black_box(vec![0u8; 4097]))).join().unwrap();
    });
    assert_eq!(there, 0, "the block belongs to the worker thread");
}

#[test]
fn a_size_filter_counts_only_that_size() {
    let (v, hits) = measure_thread_of_size(48, || {
        let a = black_box(vec![0u8; 48]);
        let b = black_box(vec![0u8; 49]);
        let c = black_box(vec![0u8; 48]);
        (a, b, c)
    });
    assert_eq!(hits, 2);
    drop(v);
}

#[test]
fn the_thread_peak_sees_what_was_live() {
    let ((), peak, net) = measure_thread_peak(|| {
        let big = black_box(vec![0u8; 10_000]);
        drop(big);
        let small = black_box(vec![0u8; 100]);
        drop(small);
    });
    assert!((10_000..11_000).contains(&peak), "peak {peak}");
    assert_eq!(net, 0);
}

#[test]
fn the_process_window_counts_every_thread() {
    let (_, totals) = measure_all(|| {
        std::thread::spawn(|| drop(black_box(vec![0u8; 256]))).join().unwrap();
    });
    assert!(totals.allocations >= 1, "{totals:?}");
    assert!(totals.bytes >= 256, "{totals:?}");
}
