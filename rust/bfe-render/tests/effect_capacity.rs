// EffectsPass.draw_batch keeps source-plane strides when it clips an upload.
// Exercise hostile imported counts and the retained interleave staging without
// a browser, content files, or a second implementation of the draw kernel.
use bfe_render::effect_resources::{emitter_span, interleave, model_span, BatchSpan};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counter;
thread_local! {
    // libtest and other threads can allocate while this test runs. Count only
    // the thread executing the kernel, with allocation-free TLS initialization.
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}
fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        System.alloc(layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        System.realloc(pointer, layout, size)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
#[test]
fn bounded_prefix_preserves_planes_and_allocates_nothing() {
    let data = [
        10., 11., 12., 13., 20., 21., 22., 23., 24., 25., 26., 27., 99.,
    ];
    let span = BatchSpan::new(4, 0, 0, 3, 1, 2, data.len(), false, 1).unwrap();
    assert_eq!(span.count, 2);
    assert_eq!(span.source, 4);
    assert_eq!(span.values, 12);
    let mut out = Vec::with_capacity(6);
    interleave(&mut out, &data, 0, span.upload, span.source, &[1, 2], 3);
    assert_eq!(out, [10., 20., 21., 11., 22., 23.]);
    // Empty Snow has no instance bytes but still needs its semantic draw count.
    assert_eq!(
        BatchSpan::new(65537, 0, 0, 0, 1, 65536, 1, false, 1)
            .unwrap()
            .count,
        65536
    );
    assert_eq!(
        BatchSpan::new(usize::MAX, 0, 0, 3, 1, 2, 13, false, 1),
        None
    );
    assert_eq!(
        BatchSpan::new(4, 0, usize::MAX, 3, 0, 2, 13, false, 1),
        None
    );
    assert_eq!(BatchSpan::new(4, 0, 0, 3, 2, 2, 13, false, 1), None);
    assert_eq!(
        BatchSpan::new(1000, 0, 0, 0, 1, 5 * 2048, 1, true, 2048)
            .unwrap()
            .count,
        5 * 2048
    );
    for (m, v, w) in [
        (0, 0, 12),
        (usize::MAX, 0, 1),
        (0, usize::MAX, 1),
        (0, 0, usize::MAX),
    ] {
        assert_eq!(model_span(m, v, w, 16, 12), m == 0 && v == 0 && w == 12);
    }
    assert_eq!(emitter_span(usize::MAX, 0, 1024, 12288), Some(1024));
    assert_eq!(emitter_span(1024, usize::MAX, 1024, 12288), None);
    assert_eq!(emitter_span(1024, 1, 1024, 12288), None);
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for _ in 0..10000 {
        let s = BatchSpan::new(4, 0, 0, 3, 1, 2, data.len(), false, 1).unwrap();
        interleave(&mut out, &data, 0, s.upload, s.source, &[1, 2], 3);
        std::hint::black_box(&out);
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(allocations, 0);
}

#[test]
fn allocation_counter_excludes_other_threads_and_detects_local_growth() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let ready = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (worker_ready, worker_done) = (ready.clone(), done.clone());
    let worker = std::thread::spawn(move || {
        while !worker_ready.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        // A process-wide counter incorrectly attributes these three buffers to
        // the kernel under test. None was allocated by its measurement thread.
        for size in [17, 29, 43] {
            std::hint::black_box(vec![0u8; size]);
        }
        worker_done.store(true, Ordering::Release);
    });
    ALLOCATIONS.with(|count| count.set(Some(0)));
    ready.store(true, Ordering::Release);
    while !done.load(Ordering::Acquire) {
        std::hint::spin_loop();
    }
    let foreign = ALLOCATIONS.with(|count| count.get().unwrap());
    std::hint::black_box(vec![0u8; 17]);
    let local = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    worker.join().unwrap();
    assert_eq!(foreign, 0);
    assert_eq!(local, 1);
}
