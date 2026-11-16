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
    // The newest record's life is the one bodies are bound to: it is shown at
    // its first pose at once, held until the clock reaches it.
    frame(&mut t,15.0);t.present();assert_eq!(t.output[o::POSITION],500.0);assert_eq!(t.output[o::GENERATION],2.0);
    assert_eq!(t.output[o::EXTRAPOLATED_TICKS],0.0);
    frame(&mut t,20.0);t.present();assert_eq!(t.output[o::POSITION],500.0);assert_eq!(t.output[o::GENERATION],2.0);
    frame(&mut t,30.0);t.present();assert_eq!(t.output[o::POSITION],506.0,"coast stops after six ticks");
    t.remove(999.0);t.present();assert_eq!(t.output[o::VALID],0.0);
}

#[test]
fn sparse_identity_churn_reuses_slots_without_breaking_probe_clusters() {
    let mut t=Timeline::new();
    for life in 0..8 {
        frame(&mut t,life as f64);t.input[f::COUNT]=lim::ENTITIES as f64;
        for i in 0..lim::ENTITIES {
            let at=f::STRIDE+i*r::STRIDE;t.input[at+r::ID]=(life*100000+i*977)as f64;
            t.input[at+r::KIND]=0.;
        }
        t.push();
        for i in 0..lim::ENTITIES {assert!(t.slot((life*100000+i*977)as f64).is_some());}
        for i in (0..lim::ENTITIES).step_by(2) {t.remove((life*100000+i*977)as f64);}
        for i in (1..lim::ENTITIES).step_by(2) {assert!(t.slot((life*100000+i*977)as f64).is_some());t.remove((life*100000+i*977)as f64);}
    }
    assert_eq!(t.refused,0);
}

fn soldier(t:&mut Timeline,row:usize,id:f64,x:f64,yaw:f64,speed:f64,disc:f64) {
    let at=f::STRIDE+row*r::STRIDE;let input=&mut t.input[at..at+r::STRIDE];input.fill(0.0);
    input[r::ID]=id;input[r::GENERATION]=1.0;input[r::KIND]=0.0;input[r::HEALTH]=100.0;
    input[r::POSITION]=x;input[r::VELOCITY]=speed;input[r::YAW]=yaw;input[r::DISC]=disc;
}
fn push(t:&mut Timeline,tick:f64,snapshots:f64,rows:&[(f64,f64,f64,f64,f64)]) {
    frame(t,tick);t.input[f::SNAPSHOTS]=snapshots;t.input[f::COUNT]=rows.len() as f64;
    for (row,&(id,x,yaw,speed,disc)) in rows.iter().enumerate() {soldier(t,row,id,x,yaw,speed,disc);}
    t.push();
}
fn shown(t:&mut Timeline,tick:f64,id:f64)->[f64;4] {
    frame(t,tick);t.present();let at=t.slot(id).unwrap()*o::STRIDE;
    [t.output[at+o::VALID],t.output[at+o::POSITION],t.output[at+o::YAW],t.output[at+o::FROM_TICK]]
}
#[test]
fn a_rested_soldier_starts_to_move_at_the_last_snapshot_that_left_him_out() {
    // Soldier 5 stops at tick 0 and sends nothing while soldier 6 walks; at
    // tick 33 he has taken a step and turned.
    let mut t=Timeline::new();push(&mut t,0.0,64.0,&[(5.0,0.0,0.0,0.0,0.0),(6.0,0.0,0.0,6.0,0.0)]);
    for n in 1..=10 {push(&mut t,n as f64*3.0,64.0,&[(6.0,n as f64*0.3,0.0,6.0,0.0)]);}
    push(&mut t,33.0,64.0,&[(5.0,0.25,1.0,6.0,0.0),(6.0,3.3,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut t,29.0,5.0)[..3],[1.0,0.0,0.0],"he stands until the last snapshot without him");
    assert_eq!(shown(&mut t,30.0,5.0)[..3],[1.0,0.0,0.0]);
    let half=shown(&mut t,31.5,5.0);assert!((half[1]-0.125).abs()<1e-9 && (half[2]-0.5).abs()<1e-9,"{half:?}");
    assert_eq!(shown(&mut t,33.0,5.0)[..3],[1.0,0.25,1.0]);
    // A walking soldier is drawn evenly between his poses, as before.
    let walking=shown(&mut t,1.5,6.0);assert!((walking[1]-0.15).abs()<1e-9,"{walking:?}");
}
#[test]
fn a_resting_soldier_keeps_his_place_after_his_last_pose_leaves_the_ring() {
    let mut t=Timeline::new();push(&mut t,0.0,4.0,&[(5.0,7.0,2.0,0.0,0.0),(6.0,0.0,0.0,6.0,0.0)]);
    for n in 1..=8 {push(&mut t,n as f64*3.0,4.0,&[(6.0,n as f64*0.3,0.0,6.0,0.0)]);}
    assert_eq!(shown(&mut t,22.0,5.0),[1.0,7.0,2.0,0.0],"the pose from before the ring still places him");
    // His first step is ahead of the clock: he is not drawn there early.
    push(&mut t,27.0,4.0,&[(5.0,7.25,2.0,6.0,0.0),(6.0,2.7,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut t,23.0,5.0)[..3],[1.0,7.0,2.0]);
    let half=shown(&mut t,25.5,5.0);assert!((half[1]-7.125).abs()<1e-9,"{half:?}");
    t.remove(5.0);frame(&mut t,25.5);t.present();assert!(t.slot(5.0).is_none());
}
#[test]
fn a_seat_change_or_teleport_ahead_of_the_clock_is_shown_at_once() {
    let mut t=Timeline::new();push(&mut t,10.0,64.0,&[(5.0,0.0,0.0,5.0,0.0)]);
    push(&mut t,13.0,64.0,&[(5.0,10.0,0.0,0.0,1.0)]);
    assert_eq!(shown(&mut t,11.0,5.0),[1.0,10.0,0.0,13.0],"not left at the place he has gone from");
    // Also when an ordinary pose still lies between the clock and the exit.
    let mut early=Timeline::new();push(&mut early,7.0,64.0,&[(5.0,0.0,0.0,5.0,0.0)]);
    push(&mut early,10.0,64.0,&[(5.0,0.3,0.0,5.0,0.0)]);push(&mut early,13.0,64.0,&[(5.0,10.0,0.0,0.0,1.0)]);
    push(&mut early,16.0,64.0,&[(5.0,10.3,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut early,8.0,5.0),[1.0,10.0,0.0,13.0]);
    let after=shown(&mut early,14.5,5.0);assert!((after[1]-10.15).abs()<1e-9,"{after:?}");
    // The bit lingers on later records of a soldier who has not moved since:
    // his turn is still drawn evenly.
    push(&mut t,16.0,64.0,&[(5.0,10.0,1.0,0.0,1.0)]);
    let half=shown(&mut t,14.5,5.0);assert!((half[2]-0.5).abs()<1e-9,"{half:?}");
}

// A soldier standing on a spawn point (unit 5) is pushed 0.57 m aside in one
// tick when another spawns on him, while the world publishes every tick.
fn shoved(disc:f64,health:f64,kind:f64,step:f64)->Timeline {
    let mut t=Timeline::new();push(&mut t,0.0,64.0,&[(5.0,7.0,0.0,0.0,0.0),(6.0,0.0,0.0,6.0,0.0)]);
    let at=f::STRIDE;t.input[at+r::HEALTH]=health;t.input[at+r::KIND]=kind;t.input[at+r::QUATERNION+3]=1.0;t.push();
    for tick in 1..=40 {
        if tick==16 {push(&mut t,16.0,64.0,&[(5.0,7.0+step,0.0,0.0,disc),(6.0,1.6,0.0,6.0,0.0)]);t.input[at+r::HEALTH]=health;t.input[at+r::KIND]=kind;t.input[at+r::QUATERNION+3]=1.0;t.push();}
        else {push(&mut t,tick as f64,64.0,&[(6.0,tick as f64*0.1,0.0,6.0,0.0)]);}
    }
    t
}
#[test]
fn a_shove_the_units_speed_does_not_explain_is_drawn_as_a_slide() {
    let mut t=shoved(0.0,100.0,0.0,0.57);
    assert_eq!(shown(&mut t,15.0,5.0)[1],7.0);
    // In the tick of the shove the body has barely moved; it is half way after
    // 67 ms and there, at rest, after 133 ms. It never steps more than 0.11 m
    // in a tick (the world moved it 0.57 m in one).
    let first=shown(&mut t,16.0,5.0)[1];assert!((first-7.0-0.57*0.04296875).abs()<1e-9,"{first}");
    let half=shown(&mut t,19.0,5.0)[1];assert!((half-7.285).abs()<1e-9,"{half}");
    assert_eq!(shown(&mut t,23.0,5.0)[1],7.57);assert_eq!(shown(&mut t,30.0,5.0)[1],7.57);
    let mut last=7.0;
    for n in 0..=96 {let x=shown(&mut t,15.0+n as f64/12.0,5.0)[1];assert!(x>=last && x-last<0.11/12.0+1e-9 && x<=7.57,"{n}: {last} -> {x}");last=x;}
    // The walking soldier beside him is drawn exactly between his poses.
    let walking=shown(&mut t,16.5,6.0)[1];assert!((walking-1.65).abs()<1e-9,"{walking}");
}
#[test]
fn teleports_long_steps_the_dead_and_hulls_are_not_slid() {
    // Marked by the server (a teleport, a seat change, a vehicle exit).
    let mut marked=shoved(1.0,100.0,0.0,1.0);
    assert_eq!(shown(&mut marked,15.5,5.0)[1],8.0,"shown at once while it is ahead of the clock");
    assert_eq!(shown(&mut marked,16.5,5.0)[1],8.0);
    // 1.5 m or more is not a shove.
    let mut long=shoved(0.0,100.0,0.0,1.5);
    assert_eq!(shown(&mut long,15.5,5.0)[1],7.75);assert_eq!(shown(&mut long,16.0,5.0)[1],8.5);
    let mut dead=shoved(0.0,0.0,0.0,0.57);assert_eq!(shown(&mut dead,16.0,5.0)[1],7.57);
    let mut hull=shoved(0.0,100.0,1.0,0.57);assert_eq!(shown(&mut hull,16.0,5.0)[1],7.57);
}
#[test]
fn only_the_part_of_a_step_beyond_the_speed_is_held_back() {
    // A running soldier lifted 0.35 m onto a ledge in one tick: his run is
    // drawn as sampled, the lift is eased. 20 Hz snapshots, a slower shove.
    let mut t=Timeline::new();
    for tick in 0..=30 {
        frame(&mut t,tick as f64);t.input[f::SNAPSHOTS]=64.0;t.input[f::COUNT]=1.0;soldier(&mut t,0,5.0,tick as f64*0.1,0.0,6.0,0.0);
        t.input[f::STRIDE+r::POSITION+1]=if tick>=10 {0.35} else {0.0};t.push();
    }
    let at=|t:&mut Timeline,tick:f64|{frame(t,tick);t.present();let row=t.slot(5.0).unwrap()*o::STRIDE;[t.output[row+o::POSITION],t.output[row+o::POSITION+1]]};
    let before=at(&mut t,9.0);assert!((before[0]-0.9).abs()<1e-9 && before[1]==0.0);
    let mid=at(&mut t,13.0);assert!((mid[0]-1.3).abs()<1e-9 && (mid[1]-0.175).abs()<1e-9,"{mid:?}");
    let after=at(&mut t,17.0);assert!((after[0]-1.7).abs()<1e-9 && (after[1]-0.35).abs()<1e-9,"{after:?}");
    // 20 Hz: a standing soldier shoved 0.4 m between two snapshots.
    let mut online=Timeline::new();push(&mut online,0.0,64.0,&[(5.0,2.0,0.0,0.0,0.0)]);push(&mut online,3.0,64.0,&[(5.0,2.4,0.0,0.0,0.0)]);
    for n in 2..=6 {push(&mut online,n as f64*3.0,64.0,&[(6.0,0.0,0.0,0.0,0.0)]);}
    assert_eq!(shown(&mut online,0.0,5.0)[1],2.0);
    let x=shown(&mut online,4.0,5.0)[1];assert!((x-2.2).abs()<1e-9,"{x}");
    assert_eq!(shown(&mut online,8.0,5.0)[1],2.4);
}
#[test]
fn a_unit_carried_faster_than_its_speed_is_drawn_where_it_is_sampled() {
    // 0.3 m a tick at a recorded 5 m/s (a soldier pushed along by a vehicle):
    // every step is beyond his speed. Only the first one slides.
    let mut t=Timeline::new();
    for tick in 0..=40 {push(&mut t,tick as f64,64.0,&[(5.0,tick as f64*0.3,0.0,5.0,0.0)]);}
    let early=shown(&mut t,1.0,5.0)[1];assert!(early<0.3 && early>0.08,"{early}");
    for tick in [9.5,20.25,33.0] {let x=shown(&mut t,tick,5.0)[1];assert!((x-tick*0.3).abs()<1e-9,"{tick}: {x}");}
}
