//! UnitBoardAgent, UnitUnBoardAgent and UnitFlyAgent use the same alternating
//! Fsm driver as the base Agent. Scalar state loans end before every callback,
//! preserving a running object if boarding or an interrupt replaces its owner.
use super::{agents::Behavior,arsenals::Mounted,weapons::Changes,vehicle_frame::Frame,vehicle_host::AgentHooks,vehicle_agent::VehicleAgent};
use crate::ai::{combat::world::Obj,vehicles::{agents::{AgentCx,FlyBehavior},service::VehicleServices}};
fn enter<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>>(frame:&mut Frame<'_,'_,C,M,H>,services:&mut impl VehicleServices,behavior:&Behavior,machine:&mut VehicleAgent,cx:&AgentCx) {
    match behavior {
        Behavior::Board(board)=>board.on_enter(frame,services,machine,cx),
        Behavior::Unboard(agent)=>agent.on_enter(frame,machine,cx),
        Behavior::Fly=>machine.clone().on_enter(frame,services,machine,cx),
        _=>unreachable!("native vehicle agent kind"),
    }
}
fn exit<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>>(frame:&mut Frame<'_,'_,C,M,H>,behavior:&Behavior,machine:&VehicleAgent,cx:&AgentCx) {
    if matches!(behavior,Behavior::Fly) {machine.on_exit(frame,machine,cx);}
}
fn work<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>>(frame:&mut Frame<'_,'_,C,M,H>,services:&mut impl VehicleServices,behavior:&Behavior,machine:&mut VehicleAgent,cx:&AgentCx,dt:f64) {
    match behavior {
        Behavior::Board(board)=>board.behave(frame,services,machine,cx,dt),
        Behavior::Unboard(agent)=>agent.behave(frame,services,machine,cx,dt),
        Behavior::Fly=>machine.clone().behave(frame,services,machine,cx,dt),
        _=>unreachable!("native vehicle agent kind"),
    }
}
/// Phase zero ticks the present state without draining transitions. Phase
/// one drains pending entries before ranking, then drains any new requests.
pub fn tick<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>>(frame:&mut Frame<'_,'_,C,M,H>,services:&mut impl VehicleServices,agent:Obj,dt:f64,phase:f64,combat_enabled:bool)->(f64,f64) {
    assert!(frame.rows.state.agents.enter(&frame.rows.state.entities,agent));
    let row=frame.rows.state.agents.get(&frame.rows.state.entities,agent).unwrap();
    let behavior=row.behavior.clone();let mut machine=row.vehicle.clone();
    let cx=AgentCx {agent,controller:row.controller,unit:machine.unit()};
    if phase==0. {
        if machine.state().state!=0. {work(frame,services,&behavior,&mut machine,&cx,dt);}
    }else{
        let mut rounds=0;
        loop {
            while machine.state().pending!=0. {
                rounds+=1;
                if rounds>crate::ai::control::fsm::DRAIN_LIMIT {machine.clear_pending();break;}
                exit(frame,&behavior,&machine,&cx);machine.transition();
                enter(frame,services,&behavior,&mut machine,&cx);
            }
            if rounds>crate::ai::control::fsm::DRAIN_LIMIT||machine.state().state==0. {break;}
            if combat_enabled {
                frame.combat(|scope,combat|{combat.rank_targets(scope,agent);});
            }
            if machine.state().pending==0. {break;}
        }
    }
    let state=machine.state();drop(machine);
    frame.rows.state.agents.leave(&mut frame.rows.state.entities,agent);frame.rows.state.reap_helpers();
    (state.state,state.pending)
}
/// Agent.exit terminates its own helper before the subclass onExit hook.
pub fn finish<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>>(frame:&mut Frame<'_,'_,C,M,H>,agent:Obj) {
    assert!(frame.rows.state.agents.enter(&frame.rows.state.entities,agent));
    let row=frame.rows.state.agents.get(&frame.rows.state.entities,agent).unwrap();
    let behavior=row.behavior.clone();let mut machine=row.vehicle.clone();let cx=AgentCx {agent,controller:row.controller,unit:machine.unit()};
    super::frame::Hooks::helper_terminate(frame.hooks,frame.rows,frame.random,agent);
    exit(frame,&behavior,&machine,&cx);machine.terminate();drop(machine);
    frame.rows.state.agents.leave(&mut frame.rows.state.entities,agent);frame.rows.state.reap_helpers();
}
