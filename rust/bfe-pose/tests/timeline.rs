//! Complete snapshot-phase fixtures use sparse ids, partial packets, crossing
//! generations and a full ring, including steady-state allocation checks.
use bfe_pose::{timeline::Timeline,timeline_layout::{timeline_frame as f,timeline_record as r,timeline_output as o,timeline_limits as lim,VERSION}};
use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self,l:Layout)->*mut u8 {if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|v|v.set(v.get()+1));}System.alloc(l)}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|v|v.set(v.get()+1));}System.realloc(p,l,n)}
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
fn frame(t:&mut Timeline,tick:f64) {t.input[f::VERSION]=VERSION as f64;t.input[f::TICK]=tick;t.input[f::HZ]=60.0;t.input[f::EXTRAPOLATE]=6.0;t.input[f::SNAPSHOTS]=64.0;}
fn sample(t:&mut Timeline,row:usize,id:f64,x:f64,generation:f64) {
    let at=f::STRIDE+row*r::STRIDE;let input=&mut t.input[at..at+r::STRIDE];input.fill(0.0);
    input[r::ID]=id;input[r::GENERATION]=generation;input[r::KIND]=1.0;input[r::HEALTH]=100.0;
    input[r::POSITION]=x;input[r::VELOCITY]=60.0;input[r::QUATERNION+3]=1.0;
}
#[test]
fn hull_samples_pair_by_id_and_never_cross_a_discontinuity() {
    let mut t=Timeline::new();frame(&mut t,10.0);t.input[f::COUNT]=2.0;
    sample(&mut t,0,999.0,10.0,1.0);sample(&mut t,1,7.0,100.0,1.0);t.push();
    frame(&mut t,20.0);sample(&mut t,0,7.0,200.0,1.0);sample(&mut t,1,999.0,20.0,1.0);t.push();
    frame(&mut t,15.0);t.present();
    assert_eq!(t.output[o::ID],999.0);assert_eq!(t.output[o::POSITION],15.0);
    assert_eq!(t.output[o::STRIDE+o::ID],7.0);assert_eq!(t.output[o::STRIDE+o::POSITION],150.0);
    // A later part replaces only its own identity at the same tick.
    frame(&mut t,20.0);t.input[f::COUNT]=1.0;sample(&mut t,0,999.0,500.0,2.0);t.push();
    frame(&mut t,15.0);t.present();assert_eq!(t.output[o::POSITION],15.0);assert_eq!(t.output[o::GENERATION],1.0);
    frame(&mut t,20.0);t.present();assert_eq!(t.output[o::POSITION],500.0);assert_eq!(t.output[o::GENERATION],2.0);
    frame(&mut t,30.0);t.present();assert_eq!(t.output[o::POSITION],506.0,"coast stops after six ticks");
    t.remove(999.0);t.present();assert_eq!(t.output[o::VALID],0.0);
}
#[test]
fn full_snapshot_bursts_and_paused_samples_allocate_nothing() {
    let mut t=Timeline::new();frame(&mut t,0.0);t.input[f::COUNT]=f64::INFINITY;
    for i in 0..lim::ENTITIES {sample(&mut t,i,(i*11+3) as f64,i as f64,1.0);}
    TRACK.with(|v|v.set(true));
    for tick in 0..80 {t.input[f::TICK]=tick as f64;t.push();t.present();}
    // More identities than any map has: a full store refuses them whole.
    t.input[f::TICK]=80.0;t.input[f::COUNT]=1.0;t.input[f::STRIDE+r::ID]=1e12;t.push();
    t.input[f::TICK]=79.5;for _ in 0..30 {t.present();}
    TRACK.with(|v|v.set(false));
    assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(t.refused,1);
    assert_eq!(t.output[o::ID],3.0);assert_eq!(t.output[o::VALID],1.0);
    assert_eq!(t.output[(lim::ENTITIES-1)*o::STRIDE+o::VALID],1.0);
    // Thousands of later births reuse departed slots without allocating or
    // exposing the departed entity's earlier interpolation brackets.
    TRACK.with(|v|v.set(true));
    let mut old=3.0;
    for birth in 0..2048 {
        t.remove(old);let id=1e9+birth as f64;
        frame(&mut t,100.0+birth as f64);sample(&mut t,0,id,42.0,1.0);t.push();t.present();
        assert_eq!(t.input[f::STRIDE+r::SLOT],0.0);
        assert_eq!(t.output[o::ID],id);assert_eq!(t.output[o::POSITION],42.0);
        old=id;
    }
    TRACK.with(|v|v.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
}
