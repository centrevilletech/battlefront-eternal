//! Acoustic geometry and protected-human scheduling. Controller notification
//! queues and the protection reference views remain explicit foreign writes.

use super::{module::state,combat::math::{dot32,f,length_sq32,normalize_or_x}};
use crate::gen::{import_ai_awareness,AiAwarenessArgs,AiAwarenessOp as Op,AiAwarenessResult};

fn call(op: Op,id: u32,number: f64) -> AiAwarenessResult {import_ai_awareness(&AiAwarenessArgs {op,id,number})}
fn clamp50(x: f64) -> f64 {if x<0.0 {0.0} else if 50.0<x {50.0} else {x}}

// perception.js noiseRadius/weaponNoise.
#[no_mangle]
pub extern "C" fn ai_noise_radius() -> f64 {
    let v=call(Op::Motion,0,0.0).values;
    let speed=if v[0]!=0.0 {f(v8math::sqrt(length_sq32(v[1],v[2],v[3])))} else {0.0};
    let kind=v[4];
    if kind==7.0 {return 0.0;}
    if kind==5.0 {return 1.0;}
    if kind!=0.0 {return clamp50(f(speed*2.0));}
    let noise=if call(Op::Weapon,0,0.0).flag && call(Op::Active,0,0.0).flag {f(call(Op::Mute,0,0.0).number)} else {0.0};
    let w=f(noise*0.5);let m=clamp50(f(speed*3.0));
    if w<=m {m} else {w}
}

// perception.js broadcastAcoustic. Shared query results retain the source's
// mutation behavior when an addAcoustic callback broadcasts again.
#[no_mangle]
pub extern "C" fn ai_broadcast_acoustic() -> f64 {
    if !call(Op::Alive,0,0.0).flag {return 0.0;}
    let raw=ai_noise_radius();let radius=if raw<=30.0 {raw} else {30.0};
    if !(radius>0.0) {return 0.0;}
    let found=call(Op::Query,0,radius).ids;
    state().acoustic_targets=found;
    let mut notified=0.0;let mut index=0;
    while index<state().acoustic_targets.len() && index<175 {
        let id=state().acoustic_targets[index];index+=1;
        let r=call(Op::Recipient,id,0.0);
        if !r.flag {continue;}
        let v=r.values;let dx=f(v[0]-v[1]);let dy=f(v[2]-v[3]);let dz=f(v[4]-v[5]);
        let n=normalize_or_x(dx,dy,dz);
        let vertical=dot32(0.0,1.0,0.0,n.x,n.y,n.z).abs();
        if f(0.87)<=vertical && f(12.25)<=length_sq32(dx,dy,dz) {continue;}
        call(Op::Notify,0,0.0);notified+=1.0;
    }
    notified
}

// weapons-ai.js rebuildProtectedHumans. Each living iterator is started at
// the same point as the source; fewer than nine humans enables the rebuild.
#[no_mangle]
pub extern "C" fn ai_rebuild_protected() {
    call(Op::LivingStart,0,0.0);
    let mut humans=Vec::new();
    loop {
        let u=call(Op::LivingNext,0,0.0);if !u.flag {break;}
        if u.number!=0.0 {humans.push(u.ids[0]);}
    }
    if humans.len()>=9 {return;}
    call(Op::LivingStart,0,1.0);
    loop {let u=call(Op::LivingNext,0,1.0);if !u.flag {break;}call(Op::Clear,u.ids[0],0.0);}
    for human in humans {
        let found=call(Op::ProtectQuery,human,0.0).ids;
        let count=found.len().min(20);state().protection_targets=found;
        for index in 0..count {
            let id=state().protection_targets[index];
            let recipient=call(Op::ProtectRecipient,id,0.0);
            if recipient.flag && recipient.number<8.0 {call(Op::ProtectAppend,human,0.0);}
        }
    }
}
