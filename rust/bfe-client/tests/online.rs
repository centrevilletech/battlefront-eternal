//! Device callbacks and paused input bursts must retain their admitted storage.
use bfe_client::{
    input::Input,
    online::{clock::Clock, index::Index, input::Sender, receiver::Receiver},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use winit::{event::MouseButton, keyboard::KeyCode};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| {
                    count.set(count.get() + 1);
                });
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| {
                    count.set(count.get() + 1);
                });
            }
        });
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;





#[test]
#[ignore = "requires installed battle content"]
fn installed_dynamic_hull_shapes_include_polygon_identities_before_contact() {
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let content = bfe_content_native::Loader::new(root).load("nab2", "map=nab2&side=rep&ai=off&seed=1").unwrap();
    let replay = bfe_client::online::replay::Replay::new(&content).unwrap();
    let collision = replay.collision.borrow();
    let world = &collision.state.worlds[replay.world as usize];
    assert!(world.order.len() > 4);
    for &id in &world.order {
        let body = &collision.state.bodies[id as usize];
        let tree = &collision.state.trees[body.tree];
        if body.moving {
            assert_eq!(tree.polygons.len(), tree.index.len() / 3, "moving shape lost its polygon identities");
        }
    }
}

#[test]
fn server_rejection_text_and_heartbeat_bursts_stay_bounded() {
    let mut receiver = Receiver::new(serde_json::json!({}));
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for _ in 0..4096 {
        receiver.message(true, br#"{"type":"ping","time":123.5}"#, 0.);
    }
    for _ in 0..32 {
        assert_eq!(receiver.pong(), Some(&br#"{"type":"pong","time":123.5}"#[..]));
        receiver.pong_sent();
    }
    assert!(receiver.pong().is_none());
    receiver.message(true, br#"{"type":"ping","time":null}"#, 0.);
    assert_eq!(receiver.pong(), Some(&br#"{"type":"pong","time":null}"#[..]));
    receiver.pong_sent();
    receiver.message(true, br#"{"type":"gameplay-error","message":"This battle is full"}"#, 0.);
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(receiver.error().as_deref(), Some("This battle is full"));
    let text = "🚀".repeat(1000);
    let message = serde_json::to_vec(&serde_json::json!({"type":"version-error","message":text})).unwrap();
    let mut rejected = Receiver::new(serde_json::json!({}));
    rejected.message(true, &message, 0.);
    assert_eq!(rejected.error().unwrap().chars().count(), 240);
}

#[test]
fn escaped_server_diagnostic_text_survives_json_decoding() {
    let mut receiver = Receiver::new(serde_json::json!({}));
    receiver.message(true, br#"{"type":"gameplay-error","message":"Rejected: \"full\"\nTry again"}"#, 0.);
    assert_eq!(receiver.error().as_deref(), Some("Rejected: \"full\"\nTry again"));
}
