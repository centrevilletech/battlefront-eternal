//! Device callbacks and paused input bursts must retain their admitted storage.
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};
use bfe_client::{audio::{MasterOutput, Status}, input::Input};
use winit::{event::MouseButton, keyboard::KeyCode};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| if track.get() { ALLOCS.with(|count| count.set(count.get() + 1)); });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout); }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| if track.get() { ALLOCS.with(|count| count.set(count.get() + 1)); });
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn device_and_input_paths_allocate_nothing_after_admission() {
    let mut audio = MasterOutput::new(48000, vec![0.1, -0.1]).unwrap();
    let status = Status::default();
    let mut output = [0.0f32; 513];
    let mut input = Input::default();
    // Include the first DSP quantum: setup must reserve its own scratch.
    ALLOCS.with(|count| count.set(0));
    TRACK.with(|track| track.set(true));
    for tick in 0..1000 {
        input.key(KeyCode::Space, true);
        input.key(KeyCode::Space, false);
        input.mouse(MouseButton::Left, true);
        input.mouse(MouseButton::Left, false);
        input.motion(2.0, -1.0);
        input.take(tick, 1);
        input.take_flight();
        input.take_view();
        audio.fill(&mut output, &status);
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}
