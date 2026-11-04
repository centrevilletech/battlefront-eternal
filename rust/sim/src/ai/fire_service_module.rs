//! Trigger decisions publish state before foreign queries and RNG draws.

use super::{module::state,combat::{math::f,weapons_ai::press_button}};
use crate::gen::{import_ai_fire,AiFireArgs,AiFireOp as Op,AiFireResult};

fn call(op: Op,number: f64,other: f64) -> AiFireResult {import_ai_fire(&AiFireArgs {op,number,other})}
fn flag(op: Op) -> bool {call(op,0.0,0.0).flag}

fn charge(id: usize,request: bool,now: f64) -> bool {
    if request {
        let gravity=call(Op::Gravity,0.0,0.0).number;
        if gravity==0.0 || f(state().fires[id].data.last_release+4.0)<=f(now) {
            if !state().fires[id].data.charging {
                state().fires[id].data.charging=true;
                let most=f(call(Op::Limit,0.0,0.0).number);
                state().fires[id].data.hold=most;
                if flag(Op::ChargeDraw) {
                    let hold=call(Op::DrawRange,f(most*0.25),most).number;
                    state().fires[id].data.hold=hold;
                }
            }
        }
    }
    if !state().fires[id].data.charging {return false;}
    let w=call(Op::ChargeHeld,0.0,0.0).values;
    if w[0]!=0.0 && state().fires[id].data.hold<=f(w[1]) {
        let fire=&mut state().fires[id].data;fire.last_release=now;fire.charging=false;return false;
    }
    true
}

fn semiauto(id: usize,mut request: bool,now: f64) -> bool {
    if flag(Op::Sniper) {
        {
            let fire=&mut state().fires[id].data;
            if fire.sniper_active && f(fire.sniper_deadline)<f(now) {fire.sniper_active=false;}
        }
        if !state().fires[id].data.sniper_active && request && flag(Op::Idle) {
            state().fires[id].data.sniper_active=true;
            let deadline=f(f(now)+call(Op::DrawRange,2.0,3.0).number);
            state().fires[id].data.sniper_deadline=deadline;
        } else {request=false;}
    }
    if !request {return false;}
    let count={let fire=&state().fires[id].data;fire.buttons[fire.channel as usize].count};
    if count>0.0 {return count==1.0;}
    if count == -1.0 {
        if flag(Op::Deployer) {call(Op::ClearRequests,0.0,0.0);}
        return false;
    }
    true
}

fn burst(id: usize,request: bool,now: f64) -> bool {
    if !request {state().fires[id].data.burst.state=0.0;return false;}
    let w=call(Op::Burst,0.0,0.0).values;
    let heat=f(w[0]);
    let ammo=if w[1]!=0.0 || w[2]==f64::INFINITY {2147483647.0} else {v8math::trunc(f(f(w[3]+w[2])*w[4]))};
    {
        let b=&mut state().fires[id].data.burst;
        if b.state==0.0 {b.heat_limit=0.0;b.last_end=-100.0;b.saved_ammo=0.0;b.count=0.0;b.state=1.0;}
    }
    if state().fires[id].data.burst.state==1.0 {
        let count=if w[5]!=0.0 {1.0} else {call(Op::DrawInt,w[6],w[7]).number};
        let b=&mut state().fires[id].data.burst;b.count=count;b.state=2.0;
    }
    let b=state().fires[id].data.burst;
    if b.state==2.0 && f(f(w[8])+f(b.last_end))<f(now) {
        let h=call(Op::BurstHeat,0.0,0.0).values;
        let shots=v8math::max(h[0],1.0);
        let salvo=f(f(shots)*f(h[1]));
        let mut needed=f(salvo*v8math::ceil(f(state().fires[id].data.burst.count/f(shots))));
        if needed>=1.0 {needed=f(0.99);}
        if needed<f(1.0-heat) {
            let b=&mut state().fires[id].data.burst;b.saved_ammo=ammo;b.state=3.0;
            b.heat_limit=f(f(1.0-if salvo>=1.0 {f(0.99)} else {salvo})-f(0.0001));
        }
    }
    let b=&mut state().fires[id].data.burst;
    if b.state==3.0 {
        if heat<b.heat_limit && b.saved_ammo-ammo<b.count {return true;}
        b.last_end=now;b.state=1.0;
    }
    false
}

#[no_mangle]
pub extern "C" fn ai_fire_consume(id: u32) -> u32 {
    let id=id as usize;
    let initial=call(Op::Start,0.0,0.0);
    if !initial.flag {return 0;}
    let now=initial.number;
    let weapon=flag(Op::Selected);
    let requests=call(Op::Requests,0.0,0.0).values;
    let mut request=requests[0]!=0.0;
    if weapon {
        if requests[0]==0.0 && requests[1]!=0.0 {
            let r=call(Op::Target,0.0,0.0).values;
            if r[0]!=0.0 && r[2]==r[4] && (r[2]==0.0 || r[2]==2.0 || r[1]==r[3]) {request=flag(Op::Validate);}
        }
        if flag(Op::Charge) {request=charge(id,request,now);}
        else if !flag(Op::Single) {
            if flag(Op::Cannon) {request=burst(id,request,now);}
        } else {request=semiauto(id,request,now);}
        if request && flag(Op::Protected) {request=false;}
        call(Op::Spread,0.0,0.0);
    }
    let (a,b)={
        let fire=&mut state().fires[id].data;let channel=fire.channel as usize;
        assert!(channel<2 && fire.channel==channel as f64,"invalid fire channel");
        press_button(&mut fire.buttons[channel],request);press_button(&mut fire.buttons[1-channel],false);
        (fire.buttons[0].count,fire.buttons[1].count)
    };
    call(Op::Buttons,a,b);request as u32
}
