//! Acoustic geometry and protected-human scheduling. Controller notification
//! queues and the protection reference views remain explicit foreign writes.

use super::{module::state,combat::math::{dot32,f,length_sq32,normalize_or_x}};
use crate::gen::{import_ai_awareness,AiAwarenessArgs,AiAwarenessOp as Op,AiAwarenessResult};

fn call(op: Op,id: u32,number: f64) -> AiAwarenessResult {import_ai_awareness(&AiAwarenessArgs {op,id,number,other:0.0})}
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

// perception.js notifyDanger: the difficulty and RNG callbacks run before
// reading the projectile's timing fields, exactly as the source does.
#[no_mangle]
pub extern "C" fn ai_notify_danger(enemy_skill: f64) -> f64 {
    let row=call(Op::DangerRow,0,0.0);
    let (lo,hi)=if row.flag {(row.values[0],row.values[1])} else if enemy_skill<17.0 {
        if enemy_skill<14.0 {(f(1.1),f(2.1))} else {(f(0.6),f(1.3))}
    } else {(0.0,f(0.8))};
    let delay=import_ai_awareness(&AiAwarenessArgs {op:Op::DangerDraw,id:0,number:lo,other:hi}).number;
    let now=f(call(Op::DangerTime,0,0.0).number);
    if !(f(call(Op::DangerFirst,0,0.0).number+delay)<now) || !(f(call(Op::DangerLast,0,0.0).number+1.0)<now) {return 0.0;}
    let found=call(Op::DangerQuery,0,0.0).ids;state().danger_targets=found;
    let mut warned=false;let mut notified=0.0;let mut index=0;
    while index<state().danger_targets.len() && index<175 {
        let id=state().danger_targets[index];index+=1;
        let r=call(Op::DangerRecipient,id,0.0);
        if !r.flag {continue;}
        if !warned && r.number==0.0 {warned=true;call(Op::DangerWarn,0,0.0);}
        if call(Op::DangerHasNotify,0,0.0).flag {call(Op::DangerNotify,0,0.0);notified+=1.0;}
    }
    call(Op::DangerWrite,0,now);
    notified
}

fn add_alert(id: u32) {
    if id!=0 && call(Op::AlertCanNotify,id,0.0).flag && !state().alerted.contains(&id) {state().alerted.push(id);}
}

// perception.js alertControllers: primary seat/body, then every eligible
// crew member, with shared ordered deduplication during nested notifications.
fn alert_controllers() {
    state().alerted.clear();
    let v=call(Op::AlertSeat,0,0.0).values;
    let id=if v[0]!=0.0 && v[1]==0.0 && v[2]!=0.0 {call(Op::AlertSeatController,0,0.0).number} else {call(Op::AlertOwn,0,0.0).number};
    add_alert(id as u32);
    if call(Op::AlertBody,0,0.0).flag {
        let leases=call(Op::AlertLeases,0,0.0).ids;
        for lease in leases {
            if lease==0 {continue;}
            let v=call(Op::AlertLease,lease,0.0).values;
            if v[0]!=0.0 || v[1]!=0.0 || v[2]!=0.0 {continue;}
            let id=call(Op::AlertCrew,lease,0.0).number;add_alert(id as u32);
        }
    }
}

// perception.js notifyTargetedShot/notifyDamage. Character/unit source and
// point objects retain their host identities through each notification.
#[no_mangle]
pub extern "C" fn ai_notify_alert(kind: u32) -> u32 {
    if kind==0 {
        if !call(Op::ShotStart,0,0.0).flag || !call(Op::ShotSource,0,0.0).flag {return 0;}
        if !(call(Op::ShotRelation,0,0.0).number<0.0) || call(Op::ShotHuman,0,0.0).flag {return 0;}
    } else {
        if !call(Op::DamageStart,0,0.0).flag || !(call(Op::DamageRelation,0,0.0).number<0.0) {return 0;}
        let v=call(Op::DamagePacket,0,0.0).values;
        let amount=f(v[0]);let generic=f(v[1]);let typed=f(v[2]);
        if !(f(amount*generic)>1.0 || f(amount*typed)>1.0) {return 0;}
    }
    alert_controllers();
    let mut notified=false;let mut index=0;
    while index<state().alerted.len() {
        let id=state().alerted[index];index+=1;call(Op::AlertNotify,id,0.0);notified=true;
    }
    notified as u32
}
