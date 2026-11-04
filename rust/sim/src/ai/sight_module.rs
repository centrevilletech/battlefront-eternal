//! Candidate publication and cluster scans. Queries may reenter other
//! simulation exports; candidate and cache writes are visible beforehand.

use super::module::state;
use crate::{abi::sim, gen::{import_ai_sight,AiSightArgs,AiSightOp as Op,AiSightResult},store::word_u32};
use three::Vector3;

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
