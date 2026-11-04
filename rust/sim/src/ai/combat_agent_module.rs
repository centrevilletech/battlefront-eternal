//! Combat-agent initialization and acceptance policies on the shared AI.
use super::combat::math::{f,length_sq_xz32};
use crate::gen::{import_ai_combat_agent,AiCombatAgentArgs,AiCombatAgentOp as Op,AiCombatAgentResult};
fn call(op: Op,id: u32,number: f64) -> AiCombatAgentResult {import_ai_combat_agent(&AiCombatAgentArgs {op,id,number})}
fn request(next: f64) {call(Op::Request,0,next);}
#[no_mangle]
pub extern "C" fn ai_combat_agent_run(_id: u32,mode: u32) -> u32 {
    if mode==0 {
        let goal=call(Op::GoalMode,0,0.0).number;
        if goal==0.0 {request(2.0);call(Op::GoalPoint,0,0.0);}
        else if goal==1.0 {request(4.0);call(Op::GoalTarget,0,0.0);}
        else if goal==2.0 {request(5.0);call(Op::GoalPoint,1,0.0);}
        else if goal==3.0 {request(6.0);}
        return 0;
    }
    if mode==1 {call(Op::Cleanup,0,0.0);call(Op::Base,0,0.0);return 0;}
    if mode==2 {call(Op::Mode,0,2.0);if call(Op::State,0,0.0).number==4.0 {call(Op::PatternExit,0,0.0);}return 0;}
    if mode==3 {return (call(Op::Unit,0,0.0).number!=5.0 && call(Op::State,0,0.0).number!=5.0) as u32;}
    if mode==4 {
        let unit=call(Op::Unit,0,0.0);if [2.0,6.0,5.0].contains(&unit.number) {return 0;}
        if unit.flag && call(Op::Speed,0,0.0).number==0.0 && call(Op::Mount,0,0.0).flag {return 1;}
        if !call(Op::Known,0,0.0).flag || call(Op::State,0,0.0).number==3.0 {return 1;}
        if call(Op::State,0,0.0).number==4.0 {return (!call(Op::Cone,0,0.0).flag) as u32;}
        return 0;
    }
    if mode==5 {
        let unit=call(Op::Unit,0,0.0);
        if !call(Op::Known,0,0.0).flag || [2.0,6.0,5.0].contains(&unit.number) {return 0;}
        call(Op::SourcePoint,0,0.0);
        if call(Op::Speed,0,0.0).number==0.0 && call(Op::Mount,0,0.0).flag {return 1;}
        if call(Op::State,0,0.0).number==2.0 || call(Op::Cone,0,0.0).flag || !call(Op::Target,0,0.0).flag {return 0;}
        let a=call(Op::DistanceStart,0,0.0).values;let a=length_sq_xz32(f(a[0]-a[1]),f(a[2]-a[3]));
        let b=call(Op::DistanceTarget,0,0.0).values;let b=length_sq_xz32(f(b[0]-b[1]),f(b[2]-b[3]));
        if !(a<f(b*f(0.43560004))) {return 0;}
        call(Op::Engage,0,0.0);return 2;
    }
    if mode==6 {return (call(Op::MountReach,0,0.0).flag && !call(Op::MountCall,0,0.0).flag) as u32;}
    if mode==7 {if !call(Op::Boarding,0,0.0).flag {return 0;}call(Op::Base,0,0.0);return 2;}
    if !call(Op::Shield,0,0.0).flag {return 0;}
    let max=f(f(call(Op::ShieldMax,0,0.0).number)*0.5);
    if !(max<call(Op::ShieldValue,0,0.0).number) {return 0;}
    call(Op::SwitchShield,0,0.0);call(Op::FireA,0,0.0);call(Op::FireB,0,0.0);1
}
