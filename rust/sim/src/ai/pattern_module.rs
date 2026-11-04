//! Attack-pattern lifecycle and transition policies. Action services release
//! native state before invoking movement, tracker or vehicle callbacks.
use super::combat::math::f;
use super::pattern_state_module::{ai_pattern_field,ai_pattern_kind,ai_pattern_write};
use crate::gen::{import_ai_pattern,AiPatternArgs,AiPatternOp as Op,AiPatternResult};

fn call(op: Op,id: u32,number: f64) -> AiPatternResult {import_ai_pattern(&AiPatternArgs {op,id,number,..Default::default()})}
fn write(id: u32,field: u32,value: f64) {ai_pattern_write(id,field,0,value);}
fn flag(id: u32,field: u32,value: bool) {ai_pattern_write(id,field,if value {3} else {2},0.0);}
fn read(id: u32,field: u32) -> f64 {
    match ai_pattern_kind(id,field) {0=>ai_pattern_field(id,field),2=>0.0,3=>1.0,_=>call(Op::Field,field,0.0).number}
}
fn request(next: f64) {call(Op::Request,0,next);}
fn int(current: bool,lo: f64,hi: f64) -> AiPatternResult {
    import_ai_pattern(&AiPatternArgs {op:Op::Int,id:current as u32,number:lo,other:hi})
}
fn sum(t: f64,draw: AiPatternResult) -> f64 {f(if draw.flag {t+draw.number} else {call(Op::Add,0,t).number})}
fn deadline(t: f64,current: bool,lo: f64,hi: f64) -> f64 {sum(t,int(current,lo,hi))}
fn truthy(id: u32,field: u32) -> bool {
    match ai_pattern_kind(id,field) {0=>{let n=ai_pattern_field(id,field);n!=0.0 && !n.is_nan()},2=>false,3=>true,_=>call(Op::FieldTruthy,field,0.0).flag}
}
fn plus(id: u32,field: u32,n: f64) -> f64 {
    if ai_pattern_kind(id,field)==0 {ai_pattern_field(id,field)+n} else {call(Op::Plus,field,n).number}
}
fn transition(kind: u32,from: f64) -> bool {call(Op::Transition,kind,from).flag}
fn usual(from: f64,lost: bool) -> bool {transition(0,from)||transition(1,from)||transition(2,from)||(lost&&transition(3,from))}
fn action(id: u32) {call(Op::Action,id,0.0);}

#[no_mangle]
pub extern "C" fn ai_pattern_run(id: u32,mode: u32) -> u32 {
    if mode==0 {
        let probability=f(call(Op::InitValue,0,0.0).number);write(id,0,probability);write(id,2,100.0);
        call(Op::Request,1,0.0);call(Op::Flush,0,0.0);return 0;
    }
    if mode==1 {call(Op::PriorCapture,0,0.0);write(id,2,100.0);call(Op::Request,2,0.0);return 0;}
    if mode==5 {call(Op::Cleanup,0,0.0);return 0;}
    if mode==2 {
        call(Op::Random,0,0.0);let t=f(call(Op::Time,0,0.0).number);
        write(id,1,-1.0);write(id,19,deadline(t,false,3.0,8.0));write(id,22,deadline(t,false,3.0,8.0));
        call(Op::Agent,0,0.0);let s=call(Op::State,0,0.0).number;
        if s==2.0 {call(Op::Stop,0,0.0);write(id,4,if call(Op::Roll,0,0.0).number%3.0==1.0 {1.0} else {0.0});}
        else if s==3.0 {action(20);flag(id,5,false);}
        else if s==4.0 {write(id,7,1.0);let draw=int(false,0.0,1.0);let side=if draw.flag {draw.number!=0.0 && !draw.number.is_nan()} else {call(Op::Add,1,0.0).flag};write(id,8,if side {2.0} else {0.0});write(id,9,0.0);write(id,10,0.0);action(21);}
        else if s==5.0 {write(id,13,0.0);call(Op::Stop,0,0.0);}
        else if s==6.0 {flag(id,3,false);write(id,15,-100.0);}
        else if s==8.0 {call(Op::Stop,0,0.0);write(id,16,-1.0);}
        else if s==9.0 {call(Op::CoverInit,0,0.0);}
        else if s==10.0 {action(22);}
        else if s==11.0 {call(Op::JetEnter,0,0.0);}
        else if s==12.0 {call(Op::Delay,16,t);call(Op::GotoTarget,0,0.0);flag(id,23,false);}
        else if call(Op::StateValue,0,0.0).number>=7.0 {call(Op::VehicleEnter,0,0.0);}
        return 0;
    }
    if mode==3 {
        call(Op::Agent,0,0.0);let t=f(call(Op::Time,0,0.0).number);call(Op::Stance,0,0.0);
        let s=call(Op::State,0,0.0).number;
        if s==7.0 {call(Op::Delay,0,t);call(Op::VehicleExit,0,0.0);}
        else if s==12.0 {call(Op::Delay,0,t);}
        else if s==9.0 {call(Op::CoverExit,0,0.0);}
        else if s==11.0 {call(Op::JetExit,0,0.0);}
        else if call(Op::StateValue,0,0.0).number>=13.0 {call(Op::VehicleExit,0,0.0);}
        return 0;
    }
    if mode==4 {
        flag(id,24,false);let s=call(Op::State,0,0.0).number;
        if [2.0,3.0,4.0,5.0,6.0,8.0,9.0,10.0,11.0,12.0].contains(&s) {action(s as u32);} else {call(Op::VehicleTick,0,0.0);}
        return 0;
    }
    if mode==6 {
        call(Op::Random,0,0.0);let t=f(call(Op::Time,0,0.0).number);
        if !(read(id,1)<t) {return 0;}
        write(id,1,deadline(t,false,16.0,25.0));
        let range=call(Op::Range,0,0.0);
        if !(if range.flag {range.number<range.other} else {call(Op::RangeCompare,0,0.0).flag}) {return 0;}
        if !call(Op::FindCover,0,0.0).flag {return 0;}
        call(Op::Reserve,0,t);call(Op::PriorFrom,0,0.0);request(9.0);return 1;
    }
    if mode==7 {
        if !call(Op::JetUnit,0,0.0).flag || !(call(Op::JetJump,0,0.0).number>0.0) {return 0;}
        let border=f(call(Op::JetBorder,0,0.0).number);return (border<call(Op::JetFuel,0,0.0).number) as u32;
    }
    if mode==8 {
        if !call(Op::JetReady,0,0.0).flag || call(Op::JetHints,0,0.0).flag {return 0;}
        let t=f(call(Op::Time,0,0.0).number);if !(read(id,22)<t) {return 0;}
        write(id,22,deadline(t,true,3.0,8.0));if !call(Op::HintFind,0,0.0).flag {return 0;}
        call(Op::PriorFrom,0,0.0);request(11.0);return 1;
    }
    if mode==9 {
        if !call(Op::JetReady,0,0.0).flag {return 0;}
        let t=f(call(Op::Time,0,0.0).number);if !(read(id,19)<t) {return 0;}
        let draw=int(true,6.0,12.0);call(Op::PriorFrom,0,0.0);write(id,19,sum(t,draw));request(10.0);return 1;
    }
    if mode==10 {
        let t=f(call(Op::Time,0,0.0).number);if !(f(t-call(Op::SampleTime,0,0.0).number)>2.0) {return 0;}
        call(Op::PriorFrom,0,0.0);request(12.0);return 1;
    }
    if mode==11 {
        if usual(2.0,false) {return 0;}
        if call(Op::StandingUnit,0,0.0).flag && call(Op::Speed,0,0.0).number<13.0 {call(Op::Sidestep,0,0.0);}
        call(Op::Stationary,0,0.0);call(Op::Face,1,0.0);return 0;
    }
    if mode==12 {
        if usual(3.0,true) {return 0;}
        let t=f(call(Op::Time,0,0.0).number);call(Op::Helper,0,0.0);
        if !truthy(id,5) {
            let arrived=call(Op::Arrived,0,0.0).flag;
            let pause=if arrived {true} else {let draw=int(true,1.0,6.0);draw.flag && draw.number==2.0};
            if pause {flag(id,5,true);write(id,6,f(t+f(1.9)));}
            call(Op::Face,0,0.0);return 0;
        }
        if t<=read(id,6) {call(Op::StopHelper,0,0.0);call(Op::Face,0,0.0);return 0;}
        flag(id,5,false);call(Op::Reposition,0,0.0);call(Op::Face,0,0.0);return 0;
    }
    if mode==13 {
        call(Op::Redirect,0,0.0);let deadline=f(call(Op::Redirect,1,0.0).number);write(id,9,deadline);
        call(Op::Flank,0,0.0);call(Op::Face,0,0.0);return 0;
    }
    if mode==14 {
        call(Op::CoverUpdate,0,0.0);
        if !call(Op::CoverDone,0,0.0).flag {call(Op::Face,2,0.0);} else {call(Op::Restore,0,0.0);}
        return 0;
    }
    if mode==15 {
        let after=f(plus(id,20,1.0));let t=f(call(Op::Time,0,0.0).number);
        if after<t && !call(Op::Flying,0,0.0).flag {flag(id,18,false);call(Op::Restore,0,0.0);}
        return 0;
    }
    if mode==16 {if call(Op::HintTick,0,0.0).flag {call(Op::Restore,0,0.0);}return 0;}
    if mode==17 {
        call(Op::Agent,0,0.0);
        if call(Op::FreshTarget,0,0.0).flag {call(Op::Restore,0,0.0);return 0;}
        if call(Op::AgentArrived,0,0.0).flag && !truthy(id,23) {flag(id,23,true);call(Op::Look,0,0.0);return 0;}
        call(Op::Face,0,0.0);return 0;
    }
    if transition(3,6.0)||transition(2,6.0)||transition(1,6.0) {return 0;}
    let t=f(call(Op::Time,0,0.0).number);
    if call(Op::MeleeStart,0,0.0).flag {
        if call(Op::DirectClear,0,0.0).flag {
            if call(Op::CanFollow,0,0.0).flag {call(Op::Follow,0,0.0);} else {call(Op::GotoMelee,1,0.0);}
            flag(id,3,false);
        } else {
            let mut wait=false;
            if truthy(id,3) {if call(Op::Arrived,0,0.0).flag {flag(id,3,false);} else {wait=true;}}
            if !wait && f(plus(id,15,2.0))<t {flag(id,3,true);write(id,15,t);call(Op::GotoMelee,0,0.0);}
        }
    }
    call(Op::Face,0,0.0);0
}
