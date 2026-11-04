//! Controller phase ordering and the command loop. Goal objects and their
//! queue retain host identities while their state-machine transitions are
//! driven by the same native FSM used by agents and helpers.

use super::control::{controller::goal_state,fsm::DRAIN_LIMIT};
use crate::gen::{import_ai_controller_driver,AiControllerDriverArgs,AiControllerDriverOp as Op,AiControllerDriverResult};

fn call(op: Op,number: f64) -> AiControllerDriverResult {import_ai_controller_driver(&AiControllerDriverArgs {op,number})}

#[no_mangle]
pub extern "C" fn ai_controller_update() {
    let restricted=call(Op::Phase,0.0).flag;
    if restricted {call(Op::ClearRequests,0.0);}
    call(Op::DriveCommands,if restricted {1.0} else {0.0});
    if restricted {call(Op::ClearAlerts,0.0);}
    call(Op::LowTicketExit,0.0);
}

#[no_mangle]
pub extern "C" fn ai_controller_commands(restricted: u32) {
    call(Op::Commands,0.0);
    if restricted!=0 {
        if !call(Op::NeedsNext,0.0).flag {call(Op::Fsm,1.0);}
        return;
    }
    let mut rounds=0;
    call(Op::Published,0.0);
    loop {
        if call(Op::NeedsNext,0.0).flag {
            if call(Op::Count,0.0).flag {
                call(Op::Pop,0.0);call(Op::Unlatch,0.0);call(Op::TraceGoal,0.0);
                let next=goal_state(call(Op::GoalKind,0.0).number);
                if next!=0.0 {call(Op::Request,next);}
            } else {call(Op::Unlatch,0.0);}
        }
        call(Op::Fsm,0.0);
        rounds+=1;if rounds>DRAIN_LIMIT {call(Op::Runaway,0.0);return;}
        if !call(Op::NeedsNext,0.0).flag || call(Op::Published,1.0).flag {return;}
    }
}

#[no_mangle]
pub extern "C" fn ai_controller_goal_state() {
    let next=goal_state(call(Op::CurrentKind,0.0).number);
    if next!=0.0 {call(Op::Request,next);}
}

#[no_mangle]
pub extern "C" fn ai_controller_low_ticket_exit() {
    if call(Op::Boarded,0.0).flag && call(Op::LowTickets,0.0).flag && !call(Op::KeepsSeat,0.0).flag {call(Op::RequestExit,0.0);}
}
