//! Controller/agent state values and the generic transition driver. Host
//! callbacks may reenter any machine; no state borrow spans a callback.

use super::{module::state,control::fsm::Fsm};

#[derive(Default)]
pub(super) struct LiveFsm {
    data: Fsm,
    // Zero is a number; one is an opaque host value. Only numeric zero
    // stops a transition, as in the source's strict comparisons.
    kinds: [u32;2],
}

#[no_mangle]
pub extern "C" fn ai_fsm_new() -> u32 {
    let s=state();s.fsms.push(LiveFsm::default());(s.fsms.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_fsm_field(id: u32,field: u32) -> f64 {
    let m=&state().fsms[id as usize];if field==0 {m.data.state} else {m.data.pending}
}

#[no_mangle]
pub extern "C" fn ai_fsm_kind(id: u32,field: u32) -> u32 {state().fsms[id as usize].kinds[field as usize]}

#[no_mangle]
pub extern "C" fn ai_fsm_write(id: u32,field: u32,kind: u32,value: f64) {
    let m=&mut state().fsms[id as usize];m.kinds[field as usize]=kind;
    if field==0 {m.data.state=value;} else {m.data.pending=value;}
}

#[cfg(target_arch="wasm32")]
mod drive {
    use super::*;
    use crate::gen::{import_ai_fsm,AiFsmArgs,AiFsmOp as Op};
    use crate::ai::control::fsm::{DONE,DRAIN_LIMIT};
    fn call(op: Op) {import_ai_fsm(&AiFsmArgs {op});}
    fn zero(id: usize,field: usize) -> bool {
        let m=&state().fsms[id];m.kinds[field]==0 && (if field==0 {m.data.state} else {m.data.pending})==0.0
    }
    fn transition(id: usize) {
        call(Op::Exit);
        {
            let m=&mut state().fsms[id];m.data.state=m.data.pending;m.kinds[0]=m.kinds[1];m.data.pending=0.0;m.kinds[1]=0;
        }
        call(Op::Enter);
    }
    // controller.js Fsm.update/flush/terminate. Tick's original argument
    // stays in its host frame; the driver does not coerce or compute it.
    #[no_mangle]
    pub extern "C" fn ai_fsm_drive(id: u32,mode: u32,alternate: u32) {
        let id=id as usize;
        if mode==2 {
            call(Op::Exit);let m=&mut state().fsms[id];m.data.state=DONE;m.kinds[0]=0;return;
        }
        let mut rounds=0;
        if mode==1 {
            while !zero(id,1) {
                rounds+=1;if rounds>DRAIN_LIMIT {call(Op::Runaway);return;}
                transition(id);
            }
            return;
        }
        if alternate!=0 {if !zero(id,0) {call(Op::Tick);}return;}
        let mut next=!zero(id,1);
        loop {
            while next {
                rounds+=1;if rounds>DRAIN_LIMIT {call(Op::Runaway);return;}
                transition(id);next=!zero(id,1);
            }
            if zero(id,0) {return;}
            call(Op::Tick);next=!zero(id,1);if !next {return;}
        }
    }
}
