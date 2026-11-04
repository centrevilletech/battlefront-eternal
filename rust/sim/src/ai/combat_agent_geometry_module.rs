//! Combat facing, look-around and evasion geometry. Shared scratch views
//! remain observable to transform and movement callbacks.
use super::combat::math::{angle,dot32,f,lookup_cos,lookup_sin,normalize_d3dx,normalize_xz};
use super::combat_agent_state_module::{ai_combat_agent_field,ai_combat_agent_kind,ai_combat_agent_write};
use crate::gen::{import_ai_combat_agent,AiCombatAgentArgs,AiCombatAgentOp as Op,AiCombatAgentResult};
fn call(op: Op,id: u32,number: f64) -> AiCombatAgentResult {import_ai_combat_agent(&AiCombatAgentArgs {op,id,number,..Default::default()})}
fn read(index: u32) -> [f64;3] {call(Op::PointRead,index,0.0).point}
fn write(index: u32,point: [f64;3]) {import_ai_combat_agent(&AiCombatAgentArgs {op:Op::PointWrite,id:index,point,..Default::default()});}
fn field(id: u32,index: u32) -> f64 {match ai_combat_agent_kind(id,index) {0=>ai_combat_agent_field(id,index),2=>0.0,3=>1.0,_=>call(Op::Field,index,0.0).number}}
fn number(id: u32,index: u32,value: f64) {ai_combat_agent_write(id,index,0,value);}
fn dot(a: [f64;3],b: [f64;3]) -> f64 {dot32(a[0],a[1],a[2],b[0],b[1],b[2])}

#[no_mangle]
pub extern "C" fn ai_combat_agent_geometry(id: u32,mode: u32) -> u32 {
    if mode==0 {
        if !call(Op::FreshTarget,0,0.0).flag {return 0;}
        call(Op::GeometryUnit,0,0.0);call(Op::TargetAim,0,0.0);call(Op::Origin,1,0.0);
        let a=read(0);let b=read(1);let n=normalize_d3dx(f(a[0]-b[0]),0.0,f(a[2]-b[2]));write(2,[n.x,n.y,n.z]);
        call(Op::Forward,1,0.0);let b=read(1);let n=read(2);let d=dot([b[0],0.0,b[2]],n);
        return (call(Op::InputNumber,1,0.0).number<d) as u32;
    }
    if mode==1 {
        call(Op::Random,0,0.0);
        if ai_combat_agent_kind(id,1)==0 && ai_combat_agent_field(id,1)==0.0 {
            let draw=call(Op::Range,0,0.0).number;number(id,2,f(draw*f(0.017453292)));
            let side=call(Op::Int,0,0.0);if side.flag && side.number==0.0 {number(id,2,f(field(id,2)*-1.0));}
        } else {number(id,2,f(field(id,2)*-2.0));}
        call(Op::GeometryUnit,0,0.0);call(Op::Forward,0,0.0);
        let c=v8math::cos(field(id,2));let s=v8math::sin(field(id,2));let a=read(0);
        let x=a[0]*c+a[2]*s;let z=-a[0]*s+a[2]*c;
        call(Op::CapturePoint,0,0.0);let p=call(Op::UnitPosition,0,0.0).point;
        import_ai_combat_agent(&AiCombatAgentArgs {op:Op::SetPoint,point:[f(p[0]+f(x*100.0)),f(p[1]+f(f(0.0001)*100.0)),f(p[2]+f(z*100.0))],..Default::default()});
        call(Op::LookCurrent,0,0.0);
        if ai_combat_agent_kind(id,1)==1 {call(Op::IncrementLooks,0,0.0);} else {number(id,1,field(id,1)+1.0);}
        return 0;
    }
    if mode==2 {
        call(Op::GeometryUnit,0,0.0);call(Op::Origin,0,0.0);let p=read(7);let a=read(0);
        let n=normalize_d3dx(f(p[0]-a[0]),0.0,f(p[2]-a[2]));write(2,[n.x,n.y,n.z]);
        call(Op::Forward,1,0.0);let b=read(1);let m=normalize_d3dx(b[0],0.0,b[2]);write(3,[m.x,m.y,m.z]);
        return (f(0.939)<dot(read(2),read(3))) as u32;
    }
    if mode==4 {
        call(Op::ConeStart,0,0.0);let p=read(6);let q=read(7);let n=normalize_xz(f(q[0]-p[0]),f(q[2]-p[2]));write(2,[n.x,n.y,n.z]);
        call(Op::Forward,0,0.0);let a=read(0);let m=normalize_xz(a[0],a[2]);write(3,[m.x,m.y,m.z]);
        return (lookup_cos(angle::cone())<dot(read(3),read(2))) as u32;
    }
    call(Op::EvadeStart,0,0.0);let v=call(Op::EvadeValues,0,0.0).values;let n=normalize_xz(f(v[0]-v[1]),f(v[2]-v[3]));write(2,[n.x,n.y,n.z]);
    let p=read(6);let n=read(2);write(0,[f(p[0]+f(n[0]*20.0)),f(p[1]+f(n[1]*20.0)),f(p[2]+f(n[2]*20.0))]);
    let draw=call(Op::Int,1,0.0);let mut dived=false;
    if draw.flag && draw.number==0.0 {
        let a=read(0);let p=read(6);let n=normalize_xz(f(a[0]-p[0]),f(a[2]-p[2]));write(2,[n.x,n.y,n.z]);
        call(Op::Forward,1,0.0);let b=read(1);let m=normalize_xz(b[0],b[2]);write(3,[m.x,m.y,m.z]);let n=read(2);let m=read(3);
        let cross=f(f(n[2]*m[0])-f(n[0]*m[2]));let s45=lookup_sin(angle::melee());
        if s45<cross {call(Op::Dodge,0,1.0);call(Op::Stop,0,0.0);dived=true;}
        else if cross< -s45 {call(Op::Dodge,0,-1.0);call(Op::Stop,0,0.0);dived=true;}
    }
    if !dived {call(Op::Move,0,0.0);}
    call(Op::Stance,0,0.0);call(Op::AlongPath,0,0.0);
    let t=call(Op::InputNumber,0,0.0);let at=f(if t.flag {t.number+2.0} else {call(Op::InputAdd,0,2.0).number});number(id,4,at);0
}

#[no_mangle]
pub extern "C" fn ai_combat_boarding() -> u32 {
    if !call(Op::BoardStart,0,0.0).flag {return 1;}
    call(Op::BoardQuery,0,0.0);let mut i=0;
    while (i as f64)<call(Op::BoardLength,0,0.0).number && i<30 {
        if call(Op::BoardValid,i,0.0).flag {return 0;}i+=1;
    }
    1
}
