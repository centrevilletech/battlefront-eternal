//! Controller phase ordering and the command loop. Goal objects and their
//! queue retain host identities while their state-machine transitions are
//! driven by the same native FSM used by agents and helpers.

use super::control::{controller::goal_state,fsm::{DONE,DRAIN_LIMIT}};
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

#[no_mangle]
pub extern "C" fn ai_controller_agent(mode: u32) {
    match mode {
        0=>{let factory=call(Op::Factory,0.0).flag;call(Op::CreateAgent,if factory {1.0} else {0.0});call(Op::TraceEnter,0.0);}
        1=>{if call(Op::HasAgent,0.0).flag {call(Op::ExitAgent,0.0);call(Op::DisposeAgent,0.0);}}
        2=>{if call(Op::HasAgent,0.0).flag {let done=call(Op::TickAgent,0.0).flag;call(Op::CurrentDone,if done {1.0} else {0.0});}}
        _=>{call(Op::CaptureAgent,0.0);call(Op::ReleaseAgent,0.0);}
    }
}

#[no_mangle]
pub extern "C" fn ai_agent_tick() -> u32 {
    let alternate=call(Op::AgentPhase,0.0).flag;
    call(Op::AgentUpdate,if alternate {1.0} else {0.0});
    (call(Op::AgentState,0.0).number==DONE) as u32
}

#[no_mangle]
pub extern "C" fn ai_agent_on_tick() {
    if call(Op::ControllerPhase,0.0).number!=0.0 {call(Op::RankTargets,0.0);} else {call(Op::Behave,0.0);}
}

#[no_mangle]
pub extern "C" fn ai_agent_exit() {
    if call(Op::HasHelper,0.0).flag {call(Op::StopHelper,0.0);}
    call(Op::TerminateAgent,0.0);
}

#[no_mangle]
pub extern "C" fn ai_agent_aim() {
    call(Op::AimStart,0.0);call(Op::Combat,0.0);
    if call(Op::CurrentTarget,0.0).flag {call(Op::AimTarget,0.0);return;}
    if call(Op::MemoryPoint,0.0).flag {call(Op::AimLook,0.0);}
    else if call(Op::HasLook,0.0).flag {call(Op::AimLook,1.0);}
    else {call(Op::AimAlong,0.0);}
}

#[no_mangle]
pub extern "C" fn ai_agent_can_engage() -> u32 {
    (!call(Op::EngageHook,0.0).flag || call(Op::Engage,0.0).flag) as u32
}

#[no_mangle]
pub extern "C" fn ai_agent_board_policy() -> u32 {
    (if call(Op::BoardHook,0.0).flag {call(Op::Board,0.0).flag} else {call(Op::NominalSpeed,0.0).number>0.0}) as u32
}
