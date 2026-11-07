//! Native receive, baseline rotation and display share one allocation domain.
//! The counter starts immediately after setup, including the first packet and
//! first sample; a warm-up must not hide a lazy native reservation.
use bfe_presentation::{
    network::{input,outbound::Outbound, Bindings, Output, Session, Words},
    network_layout::{net_header as h,net_op as op},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
const BOUNDS: [f64; 6] = [-1024., 2048., -1024., 2048., -400., 700.];
fn header(bytes: &mut [u8], magic: u32, tick: u32) {
    bytes.fill(0);
    bytes[..4].copy_from_slice(&magic.to_le_bytes());
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..10].copy_from_slice(&tick.to_le_bytes());
    bytes[10] = 2;
    bytes[15] = 1;
}
fn entity(bytes: &mut [u8; 60], tick: u32) {
    header(bytes, 0x31534e42, tick);
    bytes[20..22].copy_from_slice(&1u16.to_le_bytes());
    bytes[22] = 0;
    bytes[23] = 1;
    // All six groups of one new soldier. Its unseated sentinel is deliberately
    // nonzero, exposing accidental default-zero seat links in the direct path.
    bytes[26] = 253;
    bytes[27] = 0;
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&7u32.to_le_bytes());
    bytes[38] = 1;
    bytes[39] = 129;
    bytes[40..46].copy_from_slice(&[0, 128, 0, 128, 0, 128]);
    bytes[46] = 0;
    bytes[54] = 100;
    bytes[59] = 0;
}
#[test]
fn receive_and_display_reserve_every_store_before_first_use() {
    let shared=std::rc::Rc::new(std::cell::RefCell::new(Session::new()));
    shared.borrow_mut().reserve_receive();shared.borrow_mut().reset(1.);let mut links=Bindings::default();links.bind(0,7.,1.,0.,&shared);
    let mut session=shared.borrow_mut();
    let mut body = [0u8; 64];
    let mut own = [0u8; 21];
    let mut outbound = Outbound::default();
    let mut output = [0.; 8];
    // Packet construction is outside the measured path. The length below
    // includes the packed soldier status, seat sentinel and firing counter.
    let mut packet = [0u8; 60];
    entity(&mut packet, 1);
    body[..60].copy_from_slice(&packet);
    body[60] = 255;
    body[61] = 255;
    body[62] = 0;
    body[63] = 3;
    // Correct status follows health at 54: posture, state, weapon, seat, index.
    body[54] = 100;
    body[55] = 0;
    body[56] = 0;
    body[57] = 0;
    body[58] = 0;
    body[59] = 0;
    header(&mut own, 0x314f4e42, 1);
    // Empty command batches still encode the counted registry validity span.
    // ACK bytes share the same writer, with no lazy reservation on first use.
    let mut packet_input=[0.;136];packet_input[0]=1.;packet_input[1]=1.;
    let mut encoded=[0;1100];
    TRACK.with(|n| n.set(true));
    for tick in 1..2000u32 {
        body[6..10].copy_from_slice(&tick.to_le_bytes());
        own[6..10].copy_from_slice(&tick.to_le_bytes());
        session.receive(&body, &BOUNDS).unwrap();
        session.receive(&own, &BOUNDS).unwrap();
        session.present(tick as f64 + 0.25, -1., -1.);
        assert_eq!(input::encode(op::INPUT,&mut Words::new(&packet_input),&mut encoded).unwrap(),16);
        assert_eq!(input::encode(op::ACK,&mut Words::new(&[1.,tick as f64]),&mut encoded).unwrap(),10);
        outbound
            .run(
                op::OUTBOUND_COMMAND,
                &mut Words::new(&[1., 1., tick as f64, tick as f64, 0., 1., 0.125, -0.25]),
                &mut Output {
                    values: &mut output,
                    at: 0,
                },
            )
            .unwrap();
    }
    TRACK.with(|n| n.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(session.header[h::ACK_TICK], 1999.);
    assert_eq!(unsafe{*links.states_ptr()},1.);
}

// More unfinished ticks than the multipart store holds emulate a long page
// load. Old assemblies are evicted; the next complete snapshot still works.
#[test]
fn stalled_receive_burst_evicts_assemblies_without_growing() {
    let mut session = Session::new();
    session.reserve_receive();
    let mut bytes = [0u8; 24];
    for tick in 1..5000u32 {
        header(&mut bytes, 0x31534e42, tick);
        bytes[15] = 2;
        bytes[20] = 1;
        session.receive(&bytes, &BOUNDS).unwrap();
    }
    let mut own = [0u8; 21];
    header(&mut own, 0x314f4e42, 6000);
    session.receive(&own, &BOUNDS).unwrap();
    header(&mut bytes, 0x31534e42, 6000);
    bytes[20] = 1;
    session.receive(&bytes, &BOUNDS).unwrap();
    assert_eq!(session.header[h::ACK_TICK], 6000.);
}

// An artificial peer can retain far more fields than any mission publishes.
// Drive the payload bank beyond capacity through valid multipart datagrams;
// a refused part remains unacknowledged and later keyframes still complete.
#[test]
fn excessive_field_versions_degrade_without_allocation_or_frame_failure(){
 let mut session=Session::new();session.reserve_receive();let mut bytes=[0u8;1008];let mut missed=false;
 TRACK.with(|n|n.set(true));
 for tick in 1..=36u32{for part in 0..64u16{
  header(&mut bytes,0x31534e42,tick);bytes[15]=0;bytes[20..22].copy_from_slice(&part.to_le_bytes());bytes[22..24].copy_from_slice(&64u16.to_le_bytes());
  bytes[24..26].copy_from_slice(&981u16.to_le_bytes());bytes[26]=70;
  for i in 0..70u32{let at=27+i as usize*14;let id=part as u32*70+i;bytes[at..at+4].copy_from_slice(&id.to_le_bytes());bytes[at+4]=1;bytes[at+6]=1;bytes[at+11]=1;bytes[at+13]=tick as u8;}
  session.receive(&bytes,&BOUNDS).unwrap();missed|=session.header[h::MISSING_BASELINES]>0.;session.present(tick as f64,-1.,-1.);
 }}
 TRACK.with(|n|n.set(false));assert!(missed);assert_eq!(ALLOCS.with(Cell::get),0);
 let mut entities=[0u8;24];header(&mut entities,0x31534e42,10000);entities[20]=1;session.receive(&entities,&BOUNDS).unwrap();let mut own=[0u8;21];header(&mut own,0x314f4e42,10000);session.receive(&own,&BOUNDS).unwrap();assert_eq!(session.header[h::ACK_TICK],10000.);
}
