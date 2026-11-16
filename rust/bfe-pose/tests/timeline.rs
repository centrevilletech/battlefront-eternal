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
    push(&mut t,33.0,64.0,&[(5.0,1.0,1.0,6.0,0.0),(6.0,3.3,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut t,29.0,5.0)[..3],[1.0,0.0,0.0],"he stands until the last snapshot without him");
    assert_eq!(shown(&mut t,30.0,5.0)[..3],[1.0,0.0,0.0]);
    let half=shown(&mut t,31.5,5.0);assert!((half[1]-0.5).abs()<1e-9 && (half[2]-0.5).abs()<1e-9,"{half:?}");
    assert_eq!(shown(&mut t,33.0,5.0)[..3],[1.0,1.0,1.0]);
    // A walking soldier is drawn evenly between his poses, as before.
    let walking=shown(&mut t,1.5,6.0);assert!((walking[1]-0.15).abs()<1e-9,"{walking:?}");
}
#[test]
fn a_resting_soldier_keeps_his_place_after_his_last_pose_leaves_the_ring() {
    let mut t=Timeline::new();push(&mut t,0.0,4.0,&[(5.0,7.0,2.0,0.0,0.0),(6.0,0.0,0.0,6.0,0.0)]);
    for n in 1..=8 {push(&mut t,n as f64*3.0,4.0,&[(6.0,n as f64*0.3,0.0,6.0,0.0)]);}
    assert_eq!(shown(&mut t,22.0,5.0),[1.0,7.0,2.0,0.0],"the pose from before the ring still places him");
    // His first step is ahead of the clock: he is not drawn there early.
    push(&mut t,27.0,4.0,&[(5.0,8.0,2.0,6.0,0.0),(6.0,2.7,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut t,23.0,5.0)[..3],[1.0,7.0,2.0]);
    let half=shown(&mut t,25.5,5.0);assert!((half[1]-7.5).abs()<1e-9,"{half:?}");
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
    push(&mut early,16.0,64.0,&[(5.0,10.6,0.0,6.0,0.0)]);
    assert_eq!(shown(&mut early,8.0,5.0),[1.0,10.0,0.0,13.0]);
    let after=shown(&mut early,14.5,5.0);assert!((after[1]-10.3).abs()<1e-9,"{after:?}");
    // The bit lingers on later records of a soldier who has not moved since:
    // his turn is still drawn evenly.
    push(&mut t,16.0,64.0,&[(5.0,10.0,1.0,0.0,1.0)]);
    let half=shown(&mut t,14.5,5.0);assert!((half[2]-0.5).abs()<1e-9,"{half:?}");
}
