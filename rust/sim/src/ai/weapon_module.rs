//! Fire geometry executes in Rust while unit-owned transforms and hit
//! shapes remain foreign queries on the current battle bridge.

use super::combat::math::{angle,dot32,f,length_sq32,lookup_cos,normalize_or_x};
use super::combat::weapons_ai::sniper_yaw;
use super::{module::state,fire_module::ai_fire_ref};
use crate::gen::{import_ai_weapon,AiWeaponArgs,AiWeaponOp as Op,AiWeaponResult};

fn call(op: Op,index: u32) -> AiWeaponResult {import_ai_weapon(&AiWeaponArgs {op,index,number:0.0})}
fn perp(r: [f64;3],d: [f64;3]) -> f64 {
    let x=f(f(r[1]*f(d[2]))-f(r[2]*f(d[1])));
    let y=f(f(r[2]*f(d[0]))-f(r[0]*f(d[2])));
    let z=f(f(r[0]*f(d[1]))-f(r[1]*f(d[0])));
    length_sq32(x,y,z)
}

fn cone() -> bool {
    if !call(Op::ConeStart,0).flag {return false;}
    let origin=call(Op::Origin,0).point;
    let forward=call(Op::Forward,0).point;
    let point=call(Op::ConePoint,0).point;
    let r=[f(point[0]-origin[0]),f(point[1]-origin[1]),f(point[2]-origin[2])];
    let facts=call(Op::ConeFacts,0).values;
    if facts[0]!=0.0 && facts[1]==0.0 {return perp(r,forward)<0.0625;}
    let n=normalize_or_x(r[0],r[1],r[2]);
    let melee=facts[2]!=0.0;
    let limit=lookup_cos(if melee {angle::melee()} else {angle::fire()});
    if dot32(forward[0],forward[1],forward[2],n.x,n.y,n.z)<=limit {return false;}
    let q=call(Op::Aimer,0).point;
    let n=normalize_or_x(q[0],q[1],q[2]);
    if dot32(n.x,n.y,n.z,forward[0],forward[1],forward[2])<=limit && !melee {return false;}
    if length_sq32(r[0],r[1],r[2])<2500.0 {return true;}
    perp(r,forward)<400.0
}

fn facing() -> bool {
    if !cone() {return false;}
    let v=call(Op::Range,0).values;
    let mut range=effective_range(&v);
    let reach=if v[5]!=0.0 {v8math::max(20.0,v[6])} else {f64::INFINITY};
    range=v8math::min(range,reach);
    let origin=call(Op::Origin,0).point;
    let p=call(Op::RangePoint,0).point;
    length_sq32(f(p[0]-origin[0]),f(p[1]-origin[1]),f(p[2]-origin[2]))<f(range*range)
}

fn protected() -> bool {
    if !call(Op::ProtectionStart,0).flag {return false;}
    let origin=call(Op::Origin,0).point;
    let d=call(Op::Forward,0).point;
    let mut index=0;
    loop {
        let reference=call(Op::Reference,index);index+=1;
        if !reference.flag {break;}
        let r=reference.values;
        if r[0]==0.0 || r[2]!=r[4] || !(r[2]==0.0 || r[2]==2.0 || r[1]==r[3]) {continue;}
        let sphere=call(Op::Sphere,0);
        let p=sphere.point;
        let r=[f(p[0]-origin[0]),f(p[1]-origin[1]),f(p[2]-origin[2])];
        let x=f(f(f(d[1])*r[2])-f(f(d[2])*r[1]));
        let y=f(f(f(d[2])*r[0])-f(f(d[0])*r[2]));
        let z=f(f(f(d[0])*r[1])-f(f(d[1])*r[0]));
        let radius=f(sphere.values[0]);
        if length_sq32(x,y,z)<f(radius*radius) && dot32(d[0],d[1],d[2],r[0],r[1],r[2])>0.0 {return true;}
    }
    false
}

fn target(reference: &[f64]) -> bool {
    reference[0]!=0.0 && reference[2]==reference[4] && (reference[2]==0.0 || reference[2]==2.0 || reference[1]==reference[3])
}

fn validate() -> bool {
    if !call(Op::ValidationStart,0).flag {return false;}
    let r=call(Op::Target,0).values;
    if !target(&r) || r[5]==0.0 {return false;}
    call(Op::Lead,0);call(Op::TargetPoint,0);facing()
}

fn spread() {
    if !call(Op::SpreadStart,0).flag || !call(Op::Sniper,0).flag {return;}
    if !target(&call(Op::Target,0).values) {return;}
    let v=call(Op::SpreadValues,0).values;
    let d2=length_sq32(f(v[0]-v[3]),f(v[1]-v[4]),f(v[2]-v[5]));
    let value=f(f(v[6])*sniper_yaw(d2))*(180.0/std::f64::consts::PI);
    import_ai_weapon(&AiWeaponArgs {op:Op::SpreadWrite,index:0,number:value});
}

#[no_mangle]
pub extern "C" fn ai_weapon_guard(op: u32) -> u32 {
    (match op {0=>cone(),1=>facing(),2=>protected(),3=>validate(),4=>{spread();false},_=>panic!("invalid weapon guard")}) as u32
}

// weapons-ai.js effectiveRange, shared by geometry and inventory ranking.
fn effective_range(v: &[f64]) -> f64 {
    let mut range=v[2];
    if v[3]!=0.0 && v[0]!=0.0 && v[0]!=1.0 && v[0]!=3.0 {range=f(range+range);}
    let scaled=f(f(v[1])*range);
    if scaled<=v[4] {scaled} else if v[4]<=range {range} else {v[4]}
}

// weapons-ai.js selectWeapon. The inventory remains an external live array;
// each index is read after the preceding utility/geometry callback returns.
#[no_mangle]
pub extern "C" fn ai_weapon_select(switch_allowed: u32) {
    let start=call(Op::ChoiceStart,0);
    if !start.flag {return;}
    let now=start.values[0];
    call(Op::ChoiceOrigin,0);
    let v=call(Op::ChoiceAim,0).values;
    let d2=length_sq32(f(v[0]-v[3]),f(v[1]-v[4]),f(v[2]-v[5]));
    let owner_soldier=v[6]==0.0;
    let mut best=None;
    let mut best_score=-1.0;
    let mut best_big=false;
    let mut best_in_band=false;
    let mut index=0;
    loop {
        let item=call(Op::ChoiceWeapon,index);
        if !item.flag {break;}
        let i=index;index+=1;
        let w=item.values;
        if w[0]==0.0 {continue;}
        let ammo=if w[1]!=0.0 || w[2]!=0.0 {2147483647.0} else {v8math::trunc(f(f(w[3]+w[4])*w[5]))};
        if !(ammo>0.0) {continue;}
        let utility=call(Op::ChoiceUtility,0);
        if !utility.flag {continue;}
        let score=utility.values[0];
        let v=call(Op::ChoiceCapacity,0).values;
        let big=capacity(&v)>20.0;
        let better=best_score<score;
        let v=call(Op::ChoiceRange,0).values;
        let range=effective_range(&v);
        if d2<=f(v[5]*v[5]) || f(range*range)<=d2 {
            if best_in_band {continue;}
            let range=effective_range(&call(Op::ChoiceRange,0).values);
            let speed=f(call(Op::ChoiceSpeed,0).values[0]);
            let far=f(range+f(speed*5.0));
            if !(d2<f(far*far)) {continue;}
            if if best_big {big && better} else {big || better} {
                best_score=score;best=Some(i);best_big=big;
            }
            continue;
        }
        let mut take=!best_in_band || (if best_big {big && better} else {big || better});
        if owner_soldier && call(Op::ChoiceGrenade,0).flag {
            let cluster=call(Op::ChoiceCluster,0);
            let priority=if cluster.flag && f(now)<f(cluster.values[0]+5.0) {true} else {
                call(Op::ChoiceRoll,0).values[0]%10.0==v8math::trunc(v8math::trunc(now)/3.0)%10.0
            };
            if priority {take=true;}
        }
        if take {best_big=big;best_in_band=true;best_score=score;best=Some(i);}
    }
    let Some(best)=best else {return;};
    import_ai_weapon(&AiWeaponArgs {op:Op::ChoiceWrite,index:best,number:if best_in_band {1.0} else {0.0}});
    if switch_allowed!=0 && call(Op::ChoiceSelected,0).values[0]!=best as f64 {call(Op::ChoiceSwitch,best);}
}

// weapons-ai.js switchWeapon/selectInChannel. Reset state is visible to
// selection callbacks before they can reenter the AI.
#[no_mangle]
pub extern "C" fn ai_weapon_switch() -> u32 {
    let start=call(Op::SwitchStart,0);
    if !start.flag {return 0;}
    let id=start.values[0] as usize;
    {
        let fire=&mut state().fires[id].data;
        fire.sniper_active=false;fire.last_release=-1.0;fire.charging=false;fire.burst.state=0.0;
    }
    let channel=call(Op::SwitchChannel,0).values[0];
    state().fires[id].data.channel=channel;
    if call(Op::SwitchHasSelect,0).flag {return call(Op::SwitchSelect,0).flag as u32;}
    let index=call(Op::SwitchFallbackIndex,0).values[0];
    if index<0.0 {return 0;}
    if call(Op::SwitchCurrent,0).flag {return 1;}
    if call(Op::SwitchCurrentAgain,0).flag {return 0;}
    call(Op::SwitchDeselect,0);
    import_ai_weapon(&AiWeaponArgs {op:Op::SwitchWrite,index:0,number:index});
    1
}

// weapons-ai.js setTarget. The controller target stays a P15 write while
// the native fire target retains its independent generation and masks.
#[no_mangle]
pub extern "C" fn ai_weapon_target() {
    let start=call(Op::SetTargetStart,0);
    if !start.flag {return;}
    let id=start.values[0] as u32;
    if !call(Op::SetTargetSelected,0).flag {return;}
    let mut kept=start.values[1]!=0.0;
    if kept {
        call(Op::SetTargetAi,0);
        let relation=call(Op::SetTargetRelation,0).values[0];
        let mask=call(Op::SetTargetRelationMask,0).values[0];
        if v8math::to_uint32(mask)&(1u32<<(v8math::to_uint32(relation+1.0)&31))==0 {kept=false;}
        else {
            let v=call(Op::SetTargetCategory,0).values;
            if v8math::to_uint32(v[0])&(1u32<<(v8math::to_uint32(v[1])&31))==0 {kept=false;}
        }
    }
    if kept {
        let v=call(Op::SetTargetReference,0).values;
        ai_fire_ref(id,2,v[0] as u32,v[1],v[2] as u32);
    } else {ai_fire_ref(id,0,0,0.0,0);}
}

fn capacity(v: &[f64]) -> f64 {
    let clips=if v[0]>0.0 {v[0]} else {3.4028234663852886e38};
    let rounds=if v[1]!=0.0 {2147483647.0} else {v[2]};
    f(clips*rounds)
}

fn blast(index: u32,radius: f64) -> f64 {
    let b=call(Op::UtilityBlast,index);
    if !b.flag {return 0.0;}
    let v=b.values;
    let inner=f(v[0]);let outer=f(v[1]);
    let d=if radius>0.0 {f(2.0-f(radius*0.5))} else {2.0};
    let fall=if d<inner {1.0} else if d<outer {f(f(outer-d)/f(outer-inner))} else {0.0};
    let value=f(fall*f(v[2]));
    f(f(v[3])*value)
}

fn accept(score: f64) {
    import_ai_weapon(&AiWeaponArgs {op:Op::UtilityAccept,index:0,number:f(score)});
}

// weapons-ai.js weaponUtility: immutable content and unit facts are read
// through the bridge; damage math and acceptance are owned by this call.
#[no_mangle]
pub extern "C" fn ai_weapon_utility() {
    let start=call(Op::UtilityStart,0);
    if !start.flag {return;}
    let v=start.values;let owner_soldier=v[0]!=0.0;let target_kind=v[1];let ordnance=v[2]!=0.0;
    if !ordnance && v[3]!=0.0 && (target_kind==0.0 || target_kind==3.0) {accept(1000.0);return;}
    if owner_soldier && call(Op::UtilityHero,0).flag {accept(1000.0);return;}
    if call(Op::UtilityTrap,0).flag {return;}
    if owner_soldier && target_kind==0.0 {
        let v=call(Op::UtilityMask,0).values;
        if v8math::to_uint32(v[0])&(1u32<<(v8math::to_uint32(v[1])&31))==0 {return;}
    }
    if !ordnance {return;}
    let radius=call(Op::UtilityBound,0).values[0];
    let v=call(Op::UtilityDamage,0).values;
    let mut damage=f(f(v[0])*f(v[1]));
    damage=f(blast(0,radius)+damage);
    damage=f(blast(1,radius)+damage);
    let cap=capacity(&call(Op::UtilityCapacity,0).values);
    let value=f((if cap<=50.0 {cap} else {50.0})*damage);
    let health=f(call(Op::UtilityHealth,0).values[0]);
    if f(health*0.5)<value {accept(damage);return;}
    let owner=call(Op::UtilityOwner,0).values[0];
    if (owner==6.0 || owner==2.0 || owner==8.0 || owner==7.0) && damage>0.0 {accept(damage);return;}
    if target_kind!=6.0 && target_kind!=2.0 {
        if !(f(call(Op::UtilityHealth,0).values[0])<3.4028234663852886e38) || !(target_kind==9.0 || target_kind==8.0) {return;}
    }
    if !(damage>0.0) {return;}
    let v=call(Op::UtilityType,0).values;
    if v[0]!=0.0 {
        let velocity=call(Op::UtilityVelocity,0);
        if velocity.flag && length_sq32(velocity.point[0],velocity.point[1],velocity.point[2])<144.0 {accept(damage);}
        return;
    }
    if v[1]!=0.0 {accept(damage);}
}
