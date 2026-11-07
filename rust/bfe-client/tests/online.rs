//! Device callbacks and paused input bursts must retain their admitted storage.
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};
use bfe_client::{input::Input,online::{receiver::Receiver,input::Sender,index::Index,clock::Clock}};
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
fn first_input_feedback_and_long_bursts_keep_the_admitted_storage(){
    let mut receiver=Receiver::new(serde_json::json!({}));
    receiver.welcomed=true;receiver.complete=true;receiver.corrections=1;
    receiver.own.generation=1.;receiver.own.class=1;receiver.own.alive=true;
    let mut sender=Sender::new(&[(2.,[-0.8,1.1]);2]);
    let mut input=Input::default();let mut index=Index::new(32);let mut clock=Clock::default();
    ALLOCS.with(|c|c.set(0));TRACK.with(|t|t.set(true));
    for tick in 1..10000{
        receiver.latest=tick as f64;receiver.corrected_sequence=(tick-1)as f64;
        receiver.message(true,br#"{"type":"input-ack","epoch":1,"generation":1,"ack":4,"serverTick":6}"#,tick as f64*1000./60.);
        receiver.message(true,br#"{"type":"entity-edge-results","epoch":1,"generation":1,"received":[1,2],"results":[{"id":1,"tick":5,"status":"consumed"}]}"#,0.);
        sender.feedback(&mut receiver);
        // A long paused click burst fills the retry bank before a send. Its
        // oldest edge expires without reserving or throwing on the frame.
        for _ in 0..40{input.mouse(MouseButton::Left,true);input.mouse(MouseButton::Left,false);}
        input.key(KeyCode::KeyW,true);input.motion(2.,-1.);
        let (command,look)=input.take(tick,1);sender.tick(&command,&look,&receiver,100.,tick as f64*1000./60.);sender.preview(&receiver);
        clock.arrive(tick as f64,tick as f64/60.,2.).unwrap();clock.advance(1./60.).unwrap();
        for id in 0..32{assert!(index.insert((id,tick),id as usize));}assert!(!index.insert((33,tick),33));
        for id in 0..32{assert_eq!(index.remove(&(id,tick)),Some(id as usize));}
    }
    TRACK.with(|t|t.set(false));assert_eq!(ALLOCS.with(Cell::get),0);assert!(sender.refused>0);
}
