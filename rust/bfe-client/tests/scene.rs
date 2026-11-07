use bfe_client::{
    host::{Host, STEP},
    scene::MapCamera,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use winit::keyboard::KeyCode;
thread_local! {static COUNT:Cell<Option<usize>>=const{Cell::new(None)};}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.realloc(p, l, n)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
#[test]
fn admitted_camera_consumes_generated_input_and_clears_debt_on_focus_loss() {
    let mut host = Host::new(MapCamera::new([0.0, 10.0, 0.0], [1280, 720]).unwrap());
    host.input.key(KeyCode::KeyW, true);
    host.input.motion(100.0, -20.0);
    let before = host.world.position;
    host.frame(STEP).unwrap();
    assert_ne!(host.world.position, before);
    let moved = host.world.position;
    // A focus transition discards held motion and time debt, as it must before
    // this camera is replaced by an authoritative native battle owner.
    host.pause();
    host.frame(STEP).unwrap();
    assert_eq!(host.world.position, moved);
    host.input.motion(f64::INFINITY, f64::NAN);
    host.input.motion(1e100, -1e100);
    host.frame(STEP * 10000.0).unwrap();
    assert!(host.world.position.iter().all(|v| v.is_finite()));
    assert!(MapCamera::new([f64::NAN; 3], [1280, 720]).is_err());
    assert!(MapCamera::new([0.0; 3], [0, 720]).is_err());
}

#[test]
fn camera_steps_publish_reused_frame_storage_without_allocating() {
    let mut host = Host::new(MapCamera::new([0.0, 10.0, 0.0], [1280, 720]).unwrap());
    host.input.key(KeyCode::KeyW, true);
    COUNT.with(|c| c.set(Some(0)));
    for _ in 0..2000 {
        host.input.motion(2.0, -1.0);
        host.frame(STEP).unwrap();
    }
    let allocations = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(allocations, 0);
}
