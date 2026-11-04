//! Weapon range, speed and band policies. Opaque scalar coercions remain
//! explicit boundary operations; ordinary numeric arithmetic stays native.
use super::combat::math::f;
use crate::gen::{import_ai_range,AiRangeArgs,AiRangeOp as Op,AiRangeResult};
fn call(op: Op,id: u32,number: f64) -> AiRangeResult {import_ai_range(&AiRangeArgs {op,id,number})}
fn compare(value: AiRangeResult) -> bool {if value.flag {value.values[0]<=value.values[1]} else {call(Op::Compare,0,0.0).flag}}
fn publish(value: f64) -> AiRangeResult {call(Op::Publish,0,value)}
fn effective() -> AiRangeResult {
    if !call(Op::Start,0,0.0).flag {return publish(0.0);}
    let mut range=call(Op::Max,0,0.0);
    if call(Op::Implicit,0,0.0).flag {
        let kind=call(Op::Kind,0,0.0).number;
        if kind!=0.0 && kind!=1.0 && kind!=3.0 {
            let doubled=f(if range.flag {range.number+range.number} else {call(Op::Double,0,0.0).number});
            call(Op::Store,0,doubled);range.flag=true;range.number=doubled;
        }
    }
    let k=f(call(Op::Scale,0,0.0).number);let r=if range.flag {range.number} else {call(Op::Number,0,0.0).number};let scaled=f(k*r);
    if compare(call(Op::ScaledExtreme,0,scaled)) {publish(scaled)}
    else if compare(call(Op::ExtremeRange,0,0.0)) {call(Op::Publish,1,0.0)} else {call(Op::Publish,2,0.0)}
}
fn expanded() -> AiRangeResult {
    let range=effective();let speed=f(f(call(Op::Speed,0,0.0).number)*5.0);
    publish(f(if range.flag {range.number+speed} else {call(Op::Add,0,speed).number}))
}

#[no_mangle]
pub extern "C" fn ai_weapon_ranges(mode: u32) {
    if mode==0 {publish(f(call(Op::Speed,0,0.0).number));return;}
    if mode==1 {effective();return;}
    if mode==2 {expanded();return;}
    if mode==3 {
        let mut best=0.0;call(Op::Levels,0,0.0);
        while call(Op::Next,0,0.0).flag {let r=expanded().number;if best<=r {best=r;}}
        publish(best);return;
    }
    if mode==4 {if call(Op::Selected,0,0.0).flag {expanded();} else {publish(0.0);}return;}
    if !call(Op::Band,0,0.0).flag {publish(0.0);return;}
    if mode==5 {
        let min=call(Op::Minimum,0,0.0);let lo=f(if min.flag {min.number+10.0} else {call(Op::MinimumAdd,0,0.0).number});
        if compare(call(Op::OptimalCompare,0,lo)) {call(Op::OptimalResult,0,0.0);} else {publish(lo);}return;
    }
    call(Op::BandDef,0,0.0);let r=effective();let e=f(if r.flag {r.number-10.0} else {call(Op::Subtract,0,10.0).number});
    if compare(call(Op::HighCompare,0,e)) {call(Op::HighSelect,1,0.0);} else {call(Op::HighSelect,0,e);}
    let s=f(call(Op::Sight,0,0.0).number-10.0);
    if compare(call(Op::SightHigh,0,s)) {publish(s);} else {call(Op::HighResult,0,0.0);}
}
