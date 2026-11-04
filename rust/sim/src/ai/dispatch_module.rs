//! Engagement policy with callback-visible writes kept in source order.
//! The selected weapon and vehicle-policy result retain host identities.

use crate::gen::{import_ai_dispatch,AiDispatchArgs,AiDispatchOp as Op,AiDispatchResult};

fn call(op: Op,number: f64) -> AiDispatchResult {import_ai_dispatch(&AiDispatchArgs {op,number})}
fn pattern(value: f64) {call(Op::Pattern,value);}
fn probability(value: f64) {call(Op::Probability,value);}
fn select() {call(Op::Select,0.0);}
fn integer(value: f64) -> bool {value.is_finite() && value.fract()==0.0}

fn role() -> f64 {
    call(Op::RoleStart,0.0);
    let ai=call(Op::RoleAI,0.0).number;if integer(ai) {return call(Op::RoleAI,0.0).number;}
    let plain=call(Op::RolePlain,0.0).number;if integer(plain) {return call(Op::RolePlain,0.0).number;}
    let id=call(Op::RoleName,0.0).text;
    let name=String::from_utf16_lossy(crate::abi::sim().strings.units(id));
    match name.as_str() {
        "scout"=>0.0,"anti-armor"=>1.0,"assault"=>2.0,"support"|"repair"|"special"=>3.0,"pilot"=>4.0,"trooper"=>5.0,
        "constructor"|"__proto__"=>f64::NAN,_=>6.0,
    }
}

fn foot(role: f64) {
    if call(Op::OwnerHero,0.0).flag && call(Op::TargetHero,0.0).flag {return;}
    select();
    if call(Op::TargetHero,0.0).flag || !call(Op::Aux,0.0).flag {return;}
    if role==0.0 && call(Op::Action,0.0).number==1.0 {pattern(if call(Op::InBand,0.0).flag {2.0} else {0.0});return;}
    if role==2.0 && call(Op::Action,0.0).number==4.0 {probability(0.0);pattern(6.0);return;}
    ai_range_policy();
}

fn vehicle() {
    probability(0.75);select();
    if !call(Op::Aux,0.0).flag {pattern(0.0);return;}
    if call(Op::NumericAction,0.0).number<=3.0 {
        let odd=(call(Op::Roll,0.0).number%2.0).abs()==1.0;
        if !odd {pattern(5.0);} else if call(Op::InBand,0.0).flag {pattern(2.0);}
    } else if call(Op::Action,0.0).number==4.0 || call(Op::Action,0.0).number==6.0 {pattern(0.0);}
}

#[no_mangle]
pub extern "C" fn ai_dispatch() {
    if !call(Op::Init,0.0).flag {return;}
    let kinds=call(Op::Kinds,0.0).values;let s=kinds[0];let t=kinds[1];
    if s==0.0 || s==1.0 {
        let role=if s==1.0 {5.0} else {role()};
        if t==0.0 || t==1.0 || t==3.0 {foot(role);}
        else if t==2.0 || t==4.0 || t==6.0 || t==8.0 || t==9.0 {vehicle();}
        else if t==5.0 {select();}
    } else if s==3.0 || s==7.0 {
        select();if t!=5.0 && call(Op::Aux,0.0).flag && call(Op::InBand,0.0).flag {pattern(2.0);}
    } else if call(Op::VehicleHook,0.0).flag {call(Op::VehicleDispatch,0.0);} else {select();}
}

#[no_mangle]
pub extern "C" fn ai_range_policy() {
    let start=call(Op::RangeStart,0.0);let in_band=start.flag;
    if start.number==2.0 {if in_band {pattern(2.0);}return;}
    probability(0.5);
    let hard=call(Op::Difficulty,0.0).flag;
    let i=((call(Op::Roll,0.0).number%6.0)+6.0)%6.0;
    let still=if in_band {2.0} else {3.0};
    let p=if call(Op::Skill,0.0).number<17.0 {
        if call(Op::Skill,0.0).number<14.0 {if i<=2.0 {still} else if i==3.0 {3.0} else if i==4.0 {4.0} else {5.0}}
        else if i<=1.0 {still} else if i<=3.0 {3.0} else if i==4.0 {4.0} else {5.0}
    } else if hard {if i<=4.0 {4.0} else {5.0}}
    else if i==0.0 {still} else if i==1.0 {3.0} else if i<=4.0 {4.0} else {5.0};
    pattern(p);
}
