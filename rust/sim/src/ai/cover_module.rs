//! Cover geometry and bounded randomized hint iteration. Reservations are
//! observed through the hint service while helpers migrate to native state.

use super::combat::math::{angle,dot32,f,length_sq_xz32,lookup_cos,normalize_xz,FLT_MAX};
use super::module::state;
use super::cover_state_module::{ai_cover_field,ai_cover_kind,ai_cover_write};
use crate::gen::{import_ai_cover,AiCoverArgs,AiCoverOp as Op,AiCoverResult};

fn call(op: Op,id: u32,number: f64) -> AiCoverResult {import_ai_cover(&AiCoverArgs {op,id,number,..Default::default()})}

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

fn write(id: u32,field: u32,value: f64) {ai_cover_write(id,field,0,value);}
fn read(id: u32,field: u32) -> f64 {
    if ai_cover_kind(id,field)==0 {ai_cover_field(id,field)} else {call(Op::HelperField,field,0.0).number}
}
fn request(next: f64) {call(Op::HelperRequest,0,next);}
fn renew() {call(Op::HelperRenew,0,0.0);}
fn deadline(t: f64,lo: f64,hi: f64) -> f64 {
    let draw=import_ai_cover(&AiCoverArgs {op:Op::HelperInt,number:lo,other:hi,..Default::default()});
    f(if draw.flag {t+draw.number} else {call(Op::HelperAdd,0,t).number})
}
fn mask_draw(kind: u32) -> Option<bool> {
    call(Op::HelperMaskStart,kind,0.0);
    let limit=if kind==2 {3} else {7};
    if v8math::to_int32(call(Op::HelperMask,0,0.0).number)&limit==0 {return None;}
    loop {
        let draw=call(Op::HelperMaskDraw,0,if kind==2 {1.0} else {2.0});
        let bit=1i32.wrapping_shl(v8math::to_int32(draw.number) as u32);
        if v8math::to_int32(call(Op::HelperMask,0,0.0).number)&bit!=0 {return Some(draw.flag);}
    }
}
fn posture(id: u32,kind: u32) {
    if mask_draw(kind).is_some() {call(Op::HelperPostureDraw,0,0.0);} else {write(id,1,0.0);}
}
fn replace_hint() -> bool {
    if !call(Op::HelperFind,0,0.0).flag {return false;}
    call(Op::HelperHint,2,0.0);renew();request(2.0);true
}

#[no_mangle]
pub extern "C" fn ai_cover_helper(id: u32,mode: u32) {
    if mode==0 {
        let t=f(call(Op::HelperTime,0,0.0).number);
        call(Op::HelperHint,1,0.0);call(Op::HelperReserveInit,0,t);
        write(id,1,0.0);write(id,3,f(t+5.0));write(id,4,deadline(t,16.0,25.0));
        request(2.0);call(Op::HelperFlush,0,0.0);return;
    }
    if mode==1 {
        let t=f(call(Op::HelperRenewPrepare,0,0.0).number);call(Op::HelperReserveHeld,0,t);return;
    }
    if mode==5 {call(Op::HelperRelease,0,0.0);call(Op::HelperHint,0,0.0);return;}
    if mode==4 && call(Op::HelperState,0,0.0).number==100.0 {return;}
    let t=f(call(Op::HelperCapture,0,0.0).number);
    if mode==2 {
        let s=call(Op::HelperState,0,0.0).number;
        if s==2.0 {
            renew();call(Op::HelperGoto,0,0.0);write(id,1,0.0);call(Op::HelperFreeze,1,t);
        } else if s==3.0 {
            write(id,2,deadline(t,2.0,4.0));posture(id,0);call(Op::HelperFreeze,1,t);
        } else if s==4.0 {write(id,2,deadline(t,2.0,6.0));posture(id,1);}
        else if s==5.0 {
            write(id,2,deadline(t,4.0,7.0));
            let scale=if mask_draw(2).unwrap_or(false) {3.0} else {-3.0};
            let forward=call(Op::HelperSideForward,0,0.0).values;
            let across=[(forward[1]*0.0-forward[2])*scale,(forward[2]*0.0-forward[0]*0.0)*scale,(forward[0]-forward[1]*0.0)*scale];
            let p=call(Op::HelperSidePosition,0,0.0).values;
            import_ai_cover(&AiCoverArgs {op:Op::HelperGotoPoint,point:[across[0]+p[0],across[1]+p[1],across[2]+p[2]],..Default::default()});
            posture(id,1);call(Op::HelperDelay,4,t);
        }
        return;
    }
    if mode==3 {
        write(id,1,0.0);
        if call(Op::HelperStateValue,0,0.0).number>1.0 && call(Op::HelperStateValue,0,0.0).number<4.0 {call(Op::HelperFreeze,0,t);}
        else if call(Op::HelperState,0,0.0).number==5.0 {call(Op::HelperDelay,0,t);}
        return;
    }
    if read(id,3)<t {
        write(id,3,f(t+5.0));
        if call(Op::HelperScore,0,0.0).number==FLT_MAX {
            call(Op::HelperRelease,0,0.0);
            if !replace_hint() {call(Op::HelperHint,0,0.0);request(100.0);}
            return;
        }
    }
    if read(id,4)<t {
        write(id,4,deadline(t,16.0,25.0));
        if replace_hint() {return;}
    }
    let s=call(Op::HelperState,0,0.0).number;
    if s==2.0 {if !call(Op::HelperArrived,0,0.0).flag {renew();} else {request(3.0);}}
    else if s==3.0 {renew();if read(id,2)<t {request(if call(Op::HelperSideMask,0,0.0).number>0.0 {5.0} else {4.0});}}
    else if s==4.0 {renew();if read(id,2)<t {request(3.0);}}
    else if s==5.0 {renew();if read(id,2)<t {request(2.0);}}
}
