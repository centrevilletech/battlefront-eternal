//! Admission and playout share one reserved bank even while drawing is paused.
use bfe_presentation::ordnance::network::{Combat, feedback};
use bfe_presentation::ordnance_layout::{combat_input as i, combat_result as r,
    combat_output as o, combat_output_kind as k, ordnance_limits as limits,
    feedback_input as fi, feedback_output as fo};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Count;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self,l:Layout)->*mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {ALLOCS.with(|n|n.set(n.get()+1));}
        System.alloc(l)
    }
    unsafe fn dealloc(&self,p:*mut u8,l:Layout) {System.dealloc(p,l);}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {ALLOCS.with(|n|n.set(n.get()+1));}
        System.realloc(p,l,n)
    }
}
#[global_allocator]
static ALLOCATOR: Count=Count;

fn event(c:&mut Combat,id:u32,schema:u32,tick:f64) {
    let a=&mut c.input;a.fill(0.0);
    a[i::ID]=id as f64;a[i::GENERATION]=1.0;a[i::SCHEMA]=schema as f64;
    a[i::TICK]=tick;a[i::EXPIRY_TICK]=tick+120.0;a[i::EXECUTION_TICK]=tick;
    a[i::LAUNCH_TICK]=tick;a[i::FIRE_ID]=id as f64;
    a[i::SPEED]=60.0;a[i::LIFE]=1.0;a[i::MERGE]=8.0;a[i::DIRECTION+2]=1.0;
}


#[test]
fn delayed_explosion_survives_expiry_until_playout_reaches_it() {
    let mut c=Combat::new(8);event(&mut c,3,5,10.0);c.input[i::EXPIRY_TICK]=28.0;
    assert!(c.receive(4.0,10.0));c.resolve(false);c.present(9.0);
    event(&mut c,4,8,40.0);assert!(c.receive(4.0,40.0));c.resolve(false);
    c.present(40.0);c.settle(40.0);assert_eq!(c.output[o::ID],3.0);
    assert_eq!(c.output[o::KIND],k::EXPLODE as f64);
    c.settle(41.0);assert_eq!(c.result[r::ROWS],0.0);
    // Earlier sessions cannot roll the clock back or repopulate their events.
    event(&mut c,9,1,41.0);assert!(!c.receive(3.0,41.0));
}

#[test]
fn feedback_qualifies_generations_and_preserves_damage_view_convention() {
    let mut a=[0.0;fi::STRIDE];let mut out=[0.0;fo::STRIDE];
    a[fi::SCHEMA]=3.0;a[fi::TARGET_PRESENT]=1.0;a[fi::TARGET_OWN]=1.0;
    a[fi::TARGET_GENERATION]=2.0;a[fi::TARGET_ACTUAL_GENERATION]=2.0;
    a[fi::HAS_SOURCE]=1.0;a[fi::SOURCE]=1.0;a[fi::YAW]=0.0;
    feedback(&a,&mut out);assert_eq!(out[fo::DAMAGE],1.0);
    assert_eq!(out[fo::ANGLE],-std::f64::consts::FRAC_PI_2);
    a[fi::TARGET_ACTUAL_GENERATION]=3.0;feedback(&a,&mut out);
    assert_eq!(out[fo::DAMAGE],0.0);assert_eq!(out[fo::NOTE_ATTACKER],0.0);
    a[fi::SCHEMA]=4.0;a[fi::TARGET_ACTUAL_GENERATION]=2.0;
    a[fi::OWN_TEAM]=1.0;a[fi::ATTACKER_TEAM]=3.0;a[fi::LOCALS]=1.0;a[fi::RELATION]=1.0;
    feedback(&a,&mut out);assert_eq!(out[fo::DEATH],1.0);assert_eq!(out[fo::RELATION],3.0);
}

#[test]
fn raw_frame_bounds_and_resource_keys_stay_native() {
    use bfe_presentation::{ordnance::Ordnance,ordnance_layout::{ordnance_frame as f,ordnance_host as h,ordnance_record as record,ordnance_kind as kind}};
    let mut c=Ordnance::new();c.host_begin(false,false);
    for i in 0..limits::RECORDS*3{assert_eq!(c.host_record(),if i<limits::RECORDS{i as i32}else{-1});}
    c.host_begin(false,false);let at=f::STRIDE+c.host_record()as usize*h::STRIDE;
    c.host.input[at..at+h::STRIDE].fill(f64::NAN);
    c.host.input[at+h::KIND]=kind::BOLT as f64;c.host.input[at+h::STYLE]=0.0;c.host.input[at+h::SOURCE]=0.0;
    c.host.input[at+h::SIM_SLOT]=7.0;c.host.input[at+h::SIM_GENERATION]=3.0;c.host.input[f::RECORDS]=1.0;
    TRACK.with(|t|t.set(true));c.prepare_host();TRACK.with(|t|t.set(false));
    assert_eq!(c.input[f::STRIDE+record::KEY],7.0+4.0*(1u64<<20)as f64);
    assert_eq!(ALLOCS.with(Cell::get),0);
    for _ in 0..limits::NET_SLOTS{assert!(c.host.allocate()>=0);}assert_eq!(c.host.allocate(),-1);
    c.host.release(usize::MAX);c.host.release(1);c.host.release(1);assert_eq!(c.host.allocate(),1);assert_eq!(c.host.allocate(),-1);
}


#[test]
fn combat_fact_transport_preserves_signed_zero_and_optional_defaults() {
    let mut c=Combat::new(2);event(&mut c,1,1,10.0);
    c.input[i::DIRECTION] = -0.0;c.input[i::SURFACE]=f64::NAN;c.input[i::EXECUTION_TICK]=f64::NAN;
    assert!(c.receive_facts(1.0,10.0,"{}",true).unwrap());assert!(c.resolve(false));
    assert!(c.output[o::DIRECTION].is_sign_negative());
    assert_eq!(c.output[o::EXECUTION_TICK],10.0);
}
