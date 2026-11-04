//! Candidate publication and cluster scans. Queries may reenter other
//! simulation exports; candidate and cache writes are visible beforehand.

use super::module::state;
use crate::{abi::sim, gen::{import_ai_sight,AiSightArgs,AiSightOp as Op,AiSightResult},store::word_u32};
use three::Vector3;

fn call(op: Op,id: u32) -> AiSightResult {import_ai_sight(&AiSightArgs {op,id})}

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
