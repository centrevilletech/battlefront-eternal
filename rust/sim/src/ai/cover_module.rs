//! Cover geometry and bounded randomized hint iteration. Reservations are
//! observed through the hint service while helpers migrate to native state.

use super::combat::math::{angle,dot32,f,length_sq_xz32,lookup_cos,normalize_xz,FLT_MAX};
use super::module::state;
use crate::gen::{import_ai_cover,AiCoverArgs,AiCoverOp as Op,AiCoverResult};

fn call(op: Op,id: u32,number: f64) -> AiCoverResult {import_ai_cover(&AiCoverArgs {op,id,number})}

fn range() -> f64 {
    if !call(Op::RangeWeapon,0,0.0).flag {return 40.0;}
    let values=call(Op::RangeValues,0,0.0);
    let range=if values.flag {super::weapon_module::effective_range(&values.values)} else {0.0};
    if range<=40.0 {range} else {40.0}
}

#[no_mangle]
pub extern "C" fn ai_cover_score() -> f64 {
    let v=call(Op::ScoreStart,0,0.0).values;
    let dx=f(v[0]-v[3]);let dz=f(v[2]-v[5]);
    let ex=f(v[6]-v[0]);let ey=f(v[7]-v[1]);let ez=f(v[8]-v[2]);
    if !(dot32(dx,0.0,dz,ex,ey,ez)<0.0) {return FLT_MAX;}
    let n=normalize_xz(f(v[0]-v[6]),f(v[2]-v[8]));
    state().cover_normal=[n.x,n.y,n.z];
    let forward=call(Op::Forward,0,0.0).values;
    let n=state().cover_normal;
    if !(lookup_cos(angle::face_wide())<dot32(forward[0],forward[1],forward[2],n[0],n[1],n[2])) {return FLT_MAX;}
    let r=range();let h2=length_sq_xz32(ex,ez);
    if !(h2<f(r*r)) {return FLT_MAX;}
    let low=call(Op::Lower,0,0.0);
    let l=if low.flag {let lo=f(low.values[0]+10.0);if low.values[1]<=lo {low.values[1]} else {lo}} else {0.0};
    if !(f(l*l)<h2) {return FLT_MAX;}
    let last=call(Op::Finish,0,0.0).values;
    f(f(length_sq_xz32(f(last[0]-last[1]),f(last[2]-last[3]))*2.25)+h2)
}

#[no_mangle]
pub extern "C" fn ai_find_cover() -> u32 {
    call(Op::FindStart,0,0.0);
    let half=v8math::trunc(range());
    let mut list=call(Op::Query,0,half).ids;
    call(Op::Random,0,0.0);
    let t=f(call(Op::Time,0,0.0).number);
    let mut best=0;let mut score=FLT_MAX;
    for i in (0..list.len()).rev() {
        let draw=call(Op::Draw,0,i as f64).number;
        let valid=draw>=0.0 && draw.fract()==0.0 && draw<(list.len() as f64);
        let k=draw as usize;let hint=if valid {list[k]} else {0};
        if call(Op::Available,hint,t).flag {
            let next=call(Op::Score,hint,0.0).number;
            if next<score {best=hint;score=next;}
        }
        if valid {list[k]=list[i];}
    }
    best
}
