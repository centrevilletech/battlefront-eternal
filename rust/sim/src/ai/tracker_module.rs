//! Target selection uses live unit facts between callbacks. All state
//! borrows end before an import can reenter another simulation export.

use super::module::state;
use crate::{abi::sim, gen::{import_ai_tracker,AiTrackerArgs,AiTrackerOp as Op,AiTrackerResult}};
use three::Vector3;
use super::combat::math::f;

fn call(op: Op,id: u32,number: f64) -> AiTrackerResult {import_ai_tracker(&AiTrackerArgs {op,id,number})}

fn resolve(op: Op) -> u32 {
    let v=call(op,0,0.0).values;
    if v[1]!=0.0 && v[3]==v[5] && (v[3]==0.0 || v[3]==2.0 || v[2]==v[4]) {v[0] as u32} else {0}
}

#[no_mangle]
pub extern "C" fn ai_tracker_refresh(id: u32,target: u32,now: f64) {
    let point:[f64;3]=sim().calls.args().0[..3].try_into().unwrap();
    {
        let t=&mut state().trackers[id as usize].data;
        t.fresh=true;t.point=Vector3::new(point[0],point[1],point[2]);t.sample_time=f(now);
    }
    let cost=call(Op::Cost,target,0.0).number;
    let t=&mut state().trackers[id as usize].data;t.cost=cost;t.counting=false;
}

#[no_mangle]
pub extern "C" fn ai_tracker_transition(id: u32,op: u32,number: f64,now: f64) {
    let counting={
        let t=&mut state().trackers[id as usize].data;
        if op==0 {t.delay=f(number);} else {assert!(op==1);t.frozen=number!=0.0 && !number.is_nan();}
        t.counting
    };
    if counting {call(Op::Countdown,0,now);}
}

#[no_mangle]
pub extern "C" fn ai_tracker_select(id: u32) -> u32 {
    let initial=call(Op::Start,0,0.0).values;
    let unit=initial[0] as u32;
    let now=initial[1];
    let candidate=resolve(Op::Candidate);
    let before=resolve(Op::Target);
    let mut current=before;
    if current!=0 && unit!=0 && call(Op::Relation,current,0.0).number>=0.0 {
        call(Op::Reset,candidate,0.0);current=resolve(Op::Target);
    }
    if current==0 {
        call(Op::Reset,candidate,0.0);current=resolve(Op::Target);
    } else if current!=candidate {
        let cost=if candidate!=0 {call(Op::Cost,candidate,0.0).number} else {10000.0};
        let t=state().trackers[id as usize].data;
        if cost<t.cost {
            call(Op::Reset,candidate,0.0);current=resolve(Op::Target);
        } else if !t.counting {call(Op::Countdown,0,now);}
        else if f(t.deadline+0.0)<f(now) {
            call(Op::Reset,candidate,0.0);current=resolve(Op::Target);
        }
    }
    if current!=0 && current==candidate {call(Op::Refresh,current,now);}
    else {state().trackers[id as usize].data.fresh=false;}
    (before!=resolve(Op::Target)) as u32
}

#[no_mangle]
pub extern "C" fn ai_tracker_operation(id: u32,op: u32) -> u32 {
    match op {
        0=>{
            call(Op::Controller,0,0.0);
            let target=resolve(Op::Target);
            let memory=!state().trackers[id as usize].data.fresh && target!=0;
            call(Op::SetTarget,if memory {0} else {target},0.0);
            memory as u32
        }
        1=>{
            let target=resolve(Op::Target);
            if target==0 {0} else {call(Op::PointNear,target,0.0).number as u32}
        }
        2=>{
            let unit=call(Op::FireStart,0,0.0).number as u32;
            let target=resolve(Op::Target);
            let mut a=0.0;
            let mut b=0;
            if target!=0 && unit!=0 {
                if state().trackers[id as usize].data.fresh {b=1;}
                else if call(Op::WeaponLead,0,0.0).number!=0.0 && call(Op::Facing,0,0.0).number!=0.0 && call(Op::Near,0,0.0).number!=0.0 {
                    a=call(Op::ClearShot,target,0.0).number;
                }
            }
            call(Op::WriteFire,b,a);0
        }
        3=>{
            call(Op::Select,0,0.0);
            let target=resolve(Op::Target);
            // The controller is read after select, including when the
            // target is absent and the dispatch condition short-circuits.
            call(Op::Controller,0,0.0);
            if target!=0 && call(Op::UpdateReady,0,0.0).number!=0.0 {call(Op::Dispatch,target,0.0);}
            call(Op::Assign,0,0.0);call(Op::Fire,0,0.0);0
        }
        _=>panic!("invalid target operation"),
    }
}
