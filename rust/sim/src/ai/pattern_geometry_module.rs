//! Attack-pattern geometry uses the original shared vector views at service
//! boundaries, including vectors retained and changed by nested callbacks.
use super::combat::math::{angle,f,length_sq32,length_sq_xz32,lookup_cos,normalize_d3dx,normalize_xz};
use super::pattern_state_module::{ai_pattern_field,ai_pattern_kind,ai_pattern_write};
use crate::gen::{import_ai_pattern,AiPatternArgs,AiPatternOp as Op,AiPatternResult};
fn call(op: Op,id: u32,number: f64) -> AiPatternResult {import_ai_pattern(&AiPatternArgs {op,id,number,..Default::default()})}
fn read(id: u32) -> [f64;3] {call(Op::PointRead,id,0.0).point}
fn write(id: u32,point: [f64;3]) {import_ai_pattern(&AiPatternArgs {op:Op::PointWrite,id,point,..Default::default()});}
fn draw(lo: f64,hi: f64) -> f64 {
    let lo=f(lo);let hi=f(hi);
    if lo==hi {lo} else {import_ai_pattern(&AiPatternArgs {op:Op::RangeDraw,number:lo,other:hi,..Default::default()}).number}
}
fn number(id: u32,field: u32,value: f64) {ai_pattern_write(id,field,0,value);}
fn equal(id: u32,field: u32,value: f64) -> bool {ai_pattern_kind(id,field)==0 && ai_pattern_field(id,field)==value}
fn flag(id: u32,field: u32,value: bool) {ai_pattern_write(id,field,if value {3} else {2},0.0);}
fn field(id: u32,index: u32) -> f64 {
    match ai_pattern_kind(id,index) {0=>ai_pattern_field(id,index),2=>0.0,3=>1.0,_=>call(Op::Field,index,0.0).number}
}
fn transition(kind: u32,from: f64) -> bool {call(Op::Transition,kind,from).flag}
fn usual(from: f64,lost: bool) -> bool {transition(0,from)||transition(1,from)||transition(2,from)||(lost&&transition(3,from))}
fn current_range(lo: f64,hi: f64) -> AiPatternResult {import_ai_pattern(&AiPatternArgs {op:Op::GeometryRange,number:lo,other:hi,..Default::default()})}
fn cross(v: [f64;3]) -> [f64;3] {[v[1]*0.0-v[2],v[2]*0.0-v[0]*0.0,v[0]-v[1]*0.0]}

#[no_mangle]
pub extern "C" fn ai_pattern_geometry(id: u32,mode: u32) -> f64 {
    if mode==0 {
        call(Op::Random,0,0.0);
        let z=f(f(call(Op::Uniform,0,0.0).number-0.5)*2.0);call(Op::Uniform,1,0.0);
        let x=f(f(call(Op::Uniform,0,0.0).number-0.5)*2.0);
        let n=normalize_d3dx(f(x+0.0001),0.0,z);write(3,[n.x,n.y,n.z]);
        let mut radius=0.0;
        if call(Op::RepositionWeapon,0,0.0).flag {
            let eff=call(Op::RepositionRange,0,0.0).number;let v=call(Op::RepositionBands,0,0.0).values;
            radius=draw(f(v[0]-f(f(v[0]-v[1])*0.2)),f(f(f(eff-v[0])*0.2)+v[0]));
        }
        call(Op::TargetPoint,0,0.0);let t=read(5);let n=read(3);
        write(0,[f(t[0]+f(n[0]*radius)),f(t[1]+f(n[1]*radius)),f(t[2]+f(n[2]*radius))]);
        call(Op::MovePoint,0,0.0);return 0.0;
    }
    if mode==3 {
        if usual(4.0,true) {return 0.0;}
        let t=f(call(Op::Time,0,0.0).number);let was=call(Op::WasFacing,0,0.0).flag;
        if !call(Op::FacingTarget,0,lookup_cos(angle::face_tight())).flag {
            if was {number(id,10,0.0);}
            if field(id,10)<t {let band=call(Op::StepToBand,0,0.0).flag;if !band {number(id,10,f(t+3.0));}call(Op::Stepped,0,0.0);}
            call(Op::Face,0,0.0);return 0.0;
        }
        if !was {number(id,9,0.0);}
        let v=call(Op::BandStart,0,0.0).values;let d2=length_sq_xz32(f(v[0]-v[1]),f(v[2]-v[3]));
        if field(id,9)<t {
            let lo=call(Op::LowerBand,0,0.0).number;
            if d2<f(lo*lo) {call(Op::RedirectAction,3,t);return 0.0;}
            let hi=call(Op::UpperBand,0,0.0).number;
            if f(hi*hi)<d2 {call(Op::RedirectAction,1,t);return 0.0;}
        }
        let flank=if !was {true} else {let draw=import_ai_pattern(&AiPatternArgs {op:Op::Int,id:1,number:1.0,other:2.0,..Default::default()});draw.flag && draw.number==1.0};
        if flank {call(Op::Flank,0,0.0);}call(Op::Face,0,0.0);return 0.0;
    }
    if mode==4 {
        if usual(5.0,true) {return 0.0;}
        let t=f(call(Op::Time,0,0.0).number);
        if field(id,13)<t {
            flag(id,14,false);call(Op::GeometryStart,0,0.0);let p=read(4);let target=read(5);
            let n=normalize_d3dx(f(p[0]-target[0]),0.0,f(p[2]-target[2]));write(3,[n.x,n.y,n.z]);
            if !call(Op::FacingTarget,1,lookup_cos(angle::face_wide())).flag {
                if call(Op::StepToBand,0,0.0).flag {call(Op::Face,0,0.0);return 0.0;}
            } else {
                flag(id,14,true);let n=read(3);let flat=[n[0],0.0,n[2]];write(1,flat);write(2,cross(flat));call(Op::SideClone,0,0.0);
                call(Op::Forward,0,0.0);let b=read(1);let n=read(3);let turn=f(f(b[2]*n[0])-f(b[0]*n[2]));
                let k=f(call(Op::LateralRange,0,0.0).number*if turn>0.0 {-1.0} else {1.0});
                call(Op::CaptureMove,0,0.0);let target=read(5);let side=read(6);
                write(0,[f(target[0]+f(side[0]*k)),f(target[1]+f(side[1]*k)),f(target[2]+f(side[2]*k))]);call(Op::MovePoint,1,0.0);
            }
            number(id,13,f(t+4.0));
        }
        call(Op::Face,0,0.0);return 0.0;
    }
    if mode==5 {
        if usual(8.0,false) {return 0.0;}
        let t=f(call(Op::Time,0,0.0).number);
        if field(id,16)<t {
            let draw=current_range(3.0,6.0);let at=f(if draw.flag {t+draw.number} else {call(Op::Add,0,t).number});number(id,16,at);
            call(Op::GeometryStart,0,0.0);let p=read(4);let target=read(5);
            let n=normalize_xz(f(p[0]-target[0]),f(p[2]-target[2]));write(3,[n.x,n.y,n.z]);
            let angle=current_range(f(-0.7853981852531433),f(0.7853981852531433));
            let c=v8math::cos(if angle.flag {angle.number} else {call(Op::AngleValue,0,0.0).number});
            let s=v8math::sin(if angle.flag {angle.number} else {call(Op::AngleValue,0,0.0).number});
            call(Op::CaptureMove,0,0.0);let p=read(4);let n=read(3);
            write(0,[p[0]+100.0*(n[0]*c+n[2]*s),p[1],p[2]+100.0*(-n[0]*s+n[2]*c)]);call(Op::MovePoint,1,0.0);
        }
        call(Op::Face,0,0.0);return 0.0;
    }
    if mode==6 {
        call(Op::DirectStart,0,0.0);
        for _ in 0..10 {
            let z=f(f(f(call(Op::Uniform,0,0.0).number-0.5)*2.0)*40.0);call(Op::Uniform,1,0.0);
            let x=f(f(f(call(Op::Uniform,0,0.0).number-0.5)*2.0)*40.0);
            if length_sq32(x,0.0,z)<400.0 {continue;}
            let p=call(Op::DirectPosition,0,0.0).point;write(0,[f(p[0]+x),p[1],f(p[2]+z)]);
            if !call(Op::PointClear,0,0.0).flag {continue;}
            let p=read(0);write(1,[p[0]+0.0,p[1]+40.0,p[2]+0.0]);
            if !call(Op::Los,0,0.0).flag || call(Op::JetObstructed,0,0.0).flag {continue;}
            call(Op::JetMove,0,0.0);call(Op::JetRequest,0,0.0);let t=f(call(Op::DirectTime,0,0.0).number);number(id,20,t);flag(id,18,true);return 0.0;
        }
        number(id,20,-10.0);call(Op::Restore,0,0.0);return 0.0;
    }
    call(Op::GeometryStart,0,0.0);let t=read(5);let p=read(4);
    if mode==1 {
        let n=normalize_xz(f(t[0]-p[0]),f(t[2]-p[2]));write(3,[n.x,n.y,n.z]);
        number(id,8,if !equal(id,8,0.0) {0.0} else {2.0});
        let n=read(3);write(1,n);write(2,cross(n));
        let across=if equal(id,8,2.0) {30.0} else {-30.0};let along=if equal(id,7,1.0) {30.0} else {-30.0};
        let c=read(2);let n=read(3);write(0,[f(f(c[0]*across)+f(n[0]*along)),f(f(c[1]*across)+f(n[1]*along)),f(f(c[2]*across)+f(n[2]*along))]);
        let p=read(4);let a=read(0);write(0,[f(p[0]+a[0]),f(p[1]+a[1]),f(p[2]+a[2])]);call(Op::MovePoint,0,0.0);
        let draw=import_ai_pattern(&AiPatternArgs {op:Op::Int,id:1,number:1.0,other:5.0,..Default::default()});
        if draw.flag && draw.number==2.0 {call(Op::Dodge,0,if equal(id,8,0.0) {-1.0} else {1.0});}
        return 0.0;
    }
    let dx=f(p[0]-t[0]);let dz=f(p[2]-t[2]);let n=normalize_d3dx(dx,0.0,dz);write(3,[n.x,n.y,n.z]);
    let d2=length_sq_xz32(dx,dz);let lo=call(Op::LowerBand,0,0.0).number;
    let away=d2<f(lo*lo);
    let toward=if away {false} else {let hi=call(Op::UpperBand,0,0.0).number;f(hi*hi)<d2};
    if away||toward {
        call(Op::CaptureMove,0,0.0);let p=read(4);let n=read(3);
        let a=[f(n[0]*15.0),f(n[1]*15.0),f(n[2]*15.0)];
        write(0,if away {[f(p[0]+a[0]),f(p[1]+a[1]),f(p[2]+a[2])]} else {[f(p[0]-a[0]),f(p[1]-a[1]),f(p[2]-a[2])]});
        call(Op::MovePoint,1,0.0);return 0.0;
    }
    call(Op::GeometryStop,0,0.0);1.0
}
