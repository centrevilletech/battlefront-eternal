//! Attack-pattern geometry uses the original shared vector views at service
//! boundaries, including vectors retained and changed by nested callbacks.
use super::combat::math::{f,length_sq_xz32,normalize_d3dx,normalize_xz};
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
