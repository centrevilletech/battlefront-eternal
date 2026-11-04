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
    let t=f(call(Op::Time,0,0.0).number);if !(f(t-call(Op::SampleTime,0,0.0).number)>2.0) {return 0;}
    call(Op::PriorFrom,0,0.0);request(12.0);1
}
