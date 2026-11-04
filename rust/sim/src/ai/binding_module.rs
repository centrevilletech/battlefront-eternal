//! Binding lifecycle, aim sequencing and journal notification decisions.
//! Every import completes before another operation borrows live AI state.
use crate::gen::{import_ai_binding,AiBindingArgs,AiBindingOp as Op,AiBindingResult};
fn call(op: Op,id: u32) -> AiBindingResult {import_ai_binding(&AiBindingArgs {op,id,number:0.0})}
fn curves() {
    if call(Op::Difficulty,0).flag && call(Op::CurveA,0).number.is_finite() {call(Op::OwnCurves,0);return;}
    if call(Op::AiCurves,0).flag && call(Op::AiCurvesCall,0).flag {return;}
    if call(Op::DifficultyCurves,0).flag && call(Op::DifficultyCurvesCall,0).flag {return;}
    call(Op::NullCurves,0);
}
fn resolve(id: u32) {
    if call(Op::ResolveUnit,id).flag && call(Op::ResolveAlive,id).flag && call(Op::ResolveGeneration,id).flag {
        call(Op::ResolveResult,id);
    } else {call(Op::ReferenceNull,id);}
}
fn event_unit(id: u32) {
    if !call(Op::Reference,id).flag {call(Op::ReferenceNull,id);return;}
    if call(Op::ReferenceUnit,id).flag {resolve(id);return;}
    if !(call(Op::ReferenceHasAlive,id).flag && call(Op::ReferenceAlive,id).flag) {call(Op::ReferenceNull,id);}
}
fn event() {
    if call(Op::EventKind,0).flag {
        call(Op::EventSource,0);event_unit(0);call(Op::ShotController,0);
        call(Op::EventTarget,1);event_unit(1);
        if !call(Op::Reference,1).flag && call(Op::ShotFallback,0).flag {call(Op::ShotReference,1);resolve(1);}
        if call(Op::Pair,0).flag {call(Op::Shot,0);}
    } else if call(Op::EventKind,1).flag {
        call(Op::EventTarget,0);event_unit(0);call(Op::EventAttacker,1);event_unit(1);
        if !call(Op::Reference,1).flag {call(Op::AttackerFallback,1);}
        if call(Op::Pair,0).flag {call(Op::Damage,0);}
    }
}
fn eligible() -> bool {
    let alive=call(Op::EligibleAlive,0);
    if !alive.flag || alive.number!=0.0 || !call(Op::EligibleTargetable,0).flag {return false;}
    let max=call(Op::EligibleHealth,0);
    if !(if max.flag {max.number<3.4028234663852886e38} else {call(Op::EligibleHealthCompare,0).flag}) {return false;}
    let relation=call(Op::EligibleRelation,0);
    if relation.flag {relation.number<0.0} else {call(Op::EligibleRelationCompare,0).flag}
}
#[no_mangle]
pub extern "C" fn ai_combat_binding(mode: u32) {
    match mode {
        0 => {call(Op::Create,0);call(Op::Reset,0);}
        1 => {if call(Op::Arsenal,0).flag {call(Op::Weapons,0);while call(Op::NextWeapon,0).flag {call(Op::WeaponRandom,0);}}}
        2 => {if call(Op::HasPerception,0).flag {call(Op::ClearCandidate,0);}}
        3 => {let unit=call(Op::Unit,0).flag;let weapon=call(Op::Weapon,0).flag;
            if unit && weapon {call(Op::Lead,0);call(Op::Result,1);} else {call(Op::Result,0);}}
        4 => {call(Op::Fire,0);if !call(Op::Hooked,0).flag && call(Op::HasIntent,0).flag {call(Op::Update,0);call(Op::IntentWrite,0);}}
        5 => {call(Op::Hook,0);call(Op::Update,0);if call(Op::Intent,0).flag {call(Op::IntentWrite,0);}}
        6 => {if call(Op::AimStart,0).flag {call(Op::Forward,0);curves();call(Op::Aimer,0);call(Op::Weapon,0);call(Op::AimerUpdate,0);}else{call(Op::Result,2);}}
        7 => {if call(Op::Reader,0).flag {call(Op::Read,0);}}
        8 => {
            if !call(Op::DangerState,0).flag {call(Op::DangerNew,0);}
            call(Op::DangerBind,0);let pair=call(Op::DangerContact,0);
            let earlier=if pair.flag {pair.values[0]<pair.values[1]}else{call(Op::DangerCompare,0).flag};
            if earlier {call(Op::DangerWrite,0);}call(Op::DangerNotify,0);
        }
        9 => event(),
        10 => {let valid=eligible();call(Op::Result,u32::from(valid));}
        _ => panic!("invalid combat binding command"),
    }
}
