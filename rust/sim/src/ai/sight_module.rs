//! Candidate publication and cluster scans. Queries may reenter other
//! simulation exports; candidate and cache writes are visible beforehand.

use super::module::state;
use crate::{abi::sim, gen::{import_ai_sight,AiSightArgs,AiSightOp as Op,AiSightResult},store::word_u32};
use three::Vector3;
use super::combat::{math::{f, FLT_MAX}, perception::{cone_distance_points, matchup_priority, posture_radius, raw_cost_points, LOCAL_SENTINEL, OBJECTIVE_PRIORITY, SCAN_CAPACITY, TOWED_PRIORITY}};

fn call(op: Op,id: u32) -> AiSightResult {import_ai_sight(&AiSightArgs {op,id,..Default::default()})}

#[no_mangle]
pub extern "C" fn ai_sight_record(id: u32) {
    let words: [f64;7]=sim().calls.args().0[..7].try_into().unwrap();
    let target=word_u32(words[0]);
    {
        let p=&mut state().perceptions[id as usize];
        p.tag=2;p.unit=target;p.generation=words[1];p.generation_tag=word_u32(words[6]);p.serial+=1.0;
        p.point=Vector3::new(words[3],words[4],words[5]);
    }
    if words[2] != 0.0 {return;}
    let key=call(Op::Cluster,target).number as u64;
    let now=v8math::fround(call(Op::Time,0).number);
    if key==0 {return;}
    let scan_at=state().combat.shared.clusters.states[&key].scan_at;
    if !(v8math::fround(scan_at+5.0)<now) {return;}
    state().combat.shared.clusters.states.get_mut(&key).unwrap().scan_at=now;
    let found=call(Op::Query,target).ids;
    let mut count=1;
    for unit in found.into_iter().take(10) {
        if call(Op::Relation,unit).number>0.0 {return;}
        if call(Op::Kind,unit).number==0.0 {count+=1;}
    }
    if count>2 {state().combat.shared.clusters.states.get_mut(&key).unwrap().flagged_at=now;}
}

fn same_generation(a: f64, at: f64, b: f64, bt: f64) -> bool {
    at==bt && (at==0.0 || at==2.0 || a==b)
}

fn request_live(v: &[f64]) -> bool {
    v[1]!=0.0 && same_generation(v[2],v[3],v[4],v[5]) && v[6]!=0.0 && v[7]!=0.0 && same_generation(v[8],v[9],v[10],v[11])
}

fn candidate_live() -> bool {
    let v=call(Op::Candidate,0).values;
    v[0]!=0.0 && same_generation(v[1],v[2],v[3],v[4])
}

fn sight_test() {
    let target=call(Op::TargetPoint,0);
    let mut point=target.point;
    if target.flag {
        let eye=call(Op::TargetOrigin,0).point;
        for axis in 0..3 {point[axis]=v8math::fround(v8math::fround(eye[axis]+point[axis])*0.5);}
    }
    let from=call(Op::ShooterOrigin,0).point;
    let answer=import_ai_sight(&AiSightArgs {op:Op::Los,id:0,from,point});
    if answer.flag {
        // The foreign segment query receives the same point object the
        // source does. Preserve a write to that object before publication.
        import_ai_sight(&AiSightArgs {op:Op::Record,id:0,from:[0.0;3],point:answer.point});
    }
}

#[no_mangle]
pub extern "C" fn ai_sight_los(id: u32, op: u32, budget: f64) -> u32 {
    if op==0 {
        call(Op::ClearScratch,0);
        while call(Op::HasRequest,0).flag {
            let v=call(Op::Pop,0).values;
            if v[0]!=0.0 || !request_live(&v) || candidate_live() {continue;}
            call(Op::Keep,0);
        }
        call(Op::Swap,0);
        return 0;
    }
    assert!(op==1,"invalid sight queue command");
    let mut tests=0.0;
    let mut any=false;
    while tests<budget && call(Op::HasRequest,0).flag {
        let v=call(Op::Pop,0).values;
        if !request_live(&v) || candidate_live() {continue;}
        tests+=1.0;state().sight_queues[id as usize].tested+=1.0;any=true;
        sight_test();
    }
    any as u32
}

fn vector(p: [f64;3]) -> Vector3 {Vector3::new(p[0],p[1],p[2])}

fn scan_cone() -> f64 {
    let eye=vector(call(Op::ScanOrigin,0).point);
    let look=vector(call(Op::ScanForward,0).point);
    let at=vector(call(Op::ScanAim,0).point);
    cone_distance_points(&eye,&look,&at)
}

fn scan_range() -> f64 {
    let v=call(Op::ScanBounds,0).values;
    let multiplier=if v[0]==5.0 {2.0} else {1.0};
    let radius=if v[1]==0.0 && v[2]!=0.0 {
        posture_radius(if v[3]==1.0 {"crouch"} else if v[3]==2.0 {"prone"} else {"stand"})
    } else {f(v[4])};
    f(f(f(f(radius*20.0)+30.0))*multiplier)
}

fn scan_cost() -> f64 {
    let eye=vector(call(Op::ScanOrigin,0).point);
    let at=vector(call(Op::ScanAim,0).point);
    let v=call(Op::ScanPriority,0).values;
    let priority=if v[0]!=0.0 && v[0]==v[1] {OBJECTIVE_PRIORITY}
        else if v[2]!=0.0 && v[3]!=0.0 && v[4]>=0.0 && v[5]!=0.0 && v[5]==v[6] {TOWED_PRIORITY}
        else {matchup_priority(v[7],v[8])};
    raw_cost_points(&eye,&at,priority)
}

#[no_mangle]
pub extern "C" fn ai_sight_cost() -> f64 {scan_cost()}

#[no_mangle]
pub extern "C" fn ai_sight_scan(id: u32) -> f64 {
    if !call(Op::ScanReady,0).flag {return 0.0;}
    call(Op::ScanClear,0);
    state().scan_targets.clear();
    let shared=call(Op::ScanShared,0).number as u32;
    let special=shared!=0;
    if special {state().scan_targets.push(shared);} else {
        let mut radius=call(Op::ScanRange,0).number;
        if radius<=75.0 {radius=75.0;}
        if 350.0<=radius {radius=350.0;}
        let point=call(Op::ScanOrigin,0).point;
        let found=import_ai_sight(&AiSightArgs {op:Op::ScanQuery,id:0,from:[radius,0.0,0.0],point}).ids;
        state().scan_targets.extend(found.into_iter().take(SCAN_CAPACITY));
    }
    let mut index=0;
    let mut queued=0.0;
    // Reentrant scans share this buffer, just as the source's module array.
    loop {
        let Some(target)=state().scan_targets.get(index).copied() else {break;};
        index+=1;
        let v=call(Op::ScanTarget,target).values;
        if v[0]==0.0 || v[1]!=0.0 || v[2]!=0.0 || !(v[3]<FLT_MAX) {continue;}
        if !(call(Op::ScanRelation,0).number<0.0) {continue;}
        if call(Op::ScanCollision,0).flag {continue;}
        let l2=if special {-1.0} else {scan_cone()};
        if l2 == -2.0 {continue;}
        let range=scan_range();
        if !(l2<f(range*range)) {continue;}
        let dispatch=call(Op::ScanDispatch,0).values;
        if dispatch[0]==0.0 && dispatch[1]==0.0 {continue;}
        let mut cost=v8math::max(1.0,scan_cost());
        if dispatch[0]==0.0 && dispatch[1]!=0.0 {cost+=10000.0;}
        import_ai_sight(&AiSightArgs {op:Op::ScanInsert,id:0,from:[LOCAL_SENTINEL-cost,0.0,0.0],point:[0.0;3]});
        queued+=1.0;
    }
    state().perceptions[id as usize].scanned=queued;
    queued
}
