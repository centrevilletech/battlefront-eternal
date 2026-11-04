//! Combat-agent initialization and acceptance policies on the shared AI.
use super::combat::math::{f,length_sq_xz32,normalize_xz};
use super::combat_agent_state_module::{ai_combat_agent_field,ai_combat_agent_kind,ai_combat_agent_write};
use crate::gen::{import_ai_combat_agent,AiCombatAgentArgs,AiCombatAgentOp as Op,AiCombatAgentResult};
fn call(op: Op,id: u32,number: f64) -> AiCombatAgentResult {import_ai_combat_agent(&AiCombatAgentArgs {op,id,number,..Default::default()})}
fn write(id: u32,field: u32,value: f64) {ai_combat_agent_write(id,field,0,value);}
fn flag(id: u32,field: u32,value: bool) {ai_combat_agent_write(id,field,if value {3} else {2},0.0);}
fn read(id: u32,field: u32) -> f64 {match ai_combat_agent_kind(id,field) {0=>ai_combat_agent_field(id,field),2=>0.0,3=>1.0,_=>call(Op::Field,field,0.0).number}}
fn truthy(id: u32,field: u32) -> bool {match ai_combat_agent_kind(id,field) {0=>{let v=ai_combat_agent_field(id,field);v!=0.0&&!v.is_nan()},2=>false,3=>true,_=>call(Op::FieldTruthy,field,0.0).flag}}
fn state() -> f64 {call(Op::State,0,0.0).number}
fn request(next: f64) {call(Op::Request,0,next);}
#[no_mangle]
pub extern "C" fn ai_combat_agent_run(id: u32,mode: u32) -> u32 {
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
    if mode==8 {
        if !call(Op::Shield,0,0.0).flag {return 0;}
        let max=f(f(call(Op::ShieldMax,0,0.0).number)*0.5);
        if !(max<call(Op::ShieldValue,0,0.0).number) {return 0;}
        call(Op::SwitchShield,0,0.0);call(Op::FireA,0,0.0);call(Op::FireB,0,0.0);return 1;
    }
    if mode==9 {
        let t=f(call(Op::EntryStart,0,0.0).number);let kind=call(Op::CapturedKind,0,0.0).number;
        if kind==5.0 && (state()==2.0||state()==3.0||state()==5.0||state()==6.0) {request(100.0);return 0;}
        if state()==3.0 && kind!=0.0 && kind!=3.0 {request(100.0);return 0;}
        call(Op::CapturedMode,0,1.0);let s=state();
        if s==1.0 {call(Op::Stop,0,0.0);}
        else if s==2.0 {
            call(Op::Stop,0,0.0);let v=call(Op::InvestigateStart,0,0.0).values;let dx=f(v[0]-v[1]);let dz=f(v[2]-v[3]);
            if length_sq_xz32(dx,dz)<100.0 {let n=normalize_xz(dx,dz);import_ai_combat_agent(&AiCombatAgentArgs {op:Op::InvestigateNormal,point:[n.x,n.y,n.z],..Default::default()});
                let v=call(Op::InvestigateValues,0,0.0).values;import_ai_combat_agent(&AiCombatAgentArgs {op:Op::InvestigatePoint,point:[f(v[0]+f(v[1]*15.0)),f(v[2]+f(v[3]*15.0)),f(v[4]+f(v[5]*15.0))],..Default::default()});}
            call(Op::Look,0,0.0);call(Op::Stance,0,0.0);flag(id,3,false);
        } else if s==3.0 {call(Op::Stop,0,0.0);write(id,1,0.0);call(Op::NextLook,0,0.0);flag(id,3,false);}
        else if s==4.0 {
            call(Op::Stop,0,0.0);if !call(Op::CaptureTarget,0,0.0).flag {request(3.0);return 0;}
            call(Op::Dispatch,0,0.0);call(Op::StorePattern,0,0.0);
            if call(Op::HasPattern,0,0.0).flag {call(Op::InitPattern,0,0.0);} else {request(100.0);}
        } else if s==5.0 {call(Op::Evade,0,t);}
        else if s==6.0 {call(Op::Stop,0,0.0);call(Op::Stance,0,2.0);call(Op::AlongPath,0,0.0);write(id,5,f(t+4.0));}
        return 0;
    }
    if !call(Op::BehaveStart,0,0.0).flag {return 0;}
    let t=f(call(Op::CapturedTime,0,0.0).number);call(Op::CapturedMode,0,1.0);let mut skip=false;flag(id,7,false);
    let changed=call(Op::Select,0,0.0).flag;call(Op::AssignWeapon,0,0.0);call(Op::Alert,0,0.0);
    let jetting=call(Op::JetState,0,0.0).flag && call(Op::Flying,0,0.0).flag;
    if !jetting && call(Op::Interrupt,0,0.0).flag {return 0;}
    let s=state();
    if s==2.0 {
        let mut look=true;
        if call(Op::Aligned,0,0.0).flag {
            if call(Op::Target,0,0.0).flag {request(4.0);look=false;}
            else if !truthy(id,3) {flag(id,3,true);write(id,0,f(t+f(0.75)));}
            else if read(id,0)<t {request(3.0);look=false;}
        }
        if look {call(Op::Look,0,0.0);}
    } else if s==3.0 {
        let mut turn=true;
        if call(Op::Target,0,0.0).flag {request(4.0);turn=false;}
        else if call(Op::Aligned,0,0.0).flag {
            if !truthy(id,3) {flag(id,3,true);write(id,0,f(t+f(0.1)));}
            else if read(id,0)<t {
                if read(id,1)<2.0 {call(Op::NextLook,0,0.0);flag(id,3,false);} else {request(100.0);}
                turn=false;
            }
        }
        if turn {call(Op::FastTurn,0,0.0);}
    } else if s==4.0 {
        if jetting {call(Op::PatternUpdate,0,0.0);}
        else {
            let mut investigate=true;
            if call(Op::CaptureTarget,0,0.0).flag {
                let limit=f(call(Op::SensorStart,0,0.0).number+10.0);let d=call(Op::SensorDistance,0,0.0).values;
                if f(limit*limit)<length_sq_xz32(f(d[0]-d[1]),f(d[2]-d[3])) {request(100.0);}
                call(Op::Dispatch,1,0.0);
                if call(Op::HasPattern,0,0.0).flag {
                    investigate=false;
                    if !changed && call(Op::SamePattern,0,0.0).flag {
                        call(Op::PatternUpdate,0,0.0);skip=call(Op::SkipFire,0,0.0).flag;
                        if call(Op::CapturedKind,0,0.0).number==3.0 && call(Op::RaiseShield,0,0.0).flag {skip=true;}
                    } else {request(4.0);}
                }
            }
            if investigate {request(3.0);}
        }
    } else if s==5.0 {if read(id,4)<t {request(3.0);}}
    else if s==6.0 {if read(id,5)<t {request(3.0);}}
    call(Op::HelperTick,0,0.0);if !skip {call(Op::FireControls,0,0.0);}0
}
