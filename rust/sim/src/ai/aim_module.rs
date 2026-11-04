//! Classic and Enhanced aim filters owned by the battle's AI state.
//! Their vector facades write through, including the Enhanced service's
//! update of the Classic fire-cone direction.

use super::{
    combat::{
        aim::{ballistic_lead, projectile_values, Aimer},
        enhanced::{EnhancedAimer, Profile},
        world::{Curves, Draws},
    },
    module::state,
    random::ClassicRandom,
};
use crate::{
    abi::sim,
    combat::defs::{Js, JsObj},
    store::{word_u32, StrId},
};
use std::rc::Rc;
use three::Vector3;

#[no_mangle]
pub extern "C" fn ai_lead_relative() {
    let w:[f64;10]=sim().calls.args().0[..10].try_into().expect("relative lead input");
    let h=v8math::fround(w[9]);
    let s=state();s.out.clear();
    for i in 0..3 {s.out.push(v8math::fround(w[i]+v8math::fround((w[i+3]-w[i+6])*h)));}
}

#[no_mangle]
pub extern "C" fn ai_ballistic_weapon(alias: u32) -> u32 {
    let w:[f64;16]=sim().calls.args().0[..16].try_into().expect("weapon lead input");
    let mut p=Vector3::new(w[0],w[1],w[2]);
    let mut v=Vector3::new(w[3],w[4],w[5]);
    let mut origin=Vector3::new(w[6],w[7],w[8]);
    let initial=Vector3::new(v8math::fround(p.x),v8math::fround(p.y),v8math::fround(p.z));
    // The source writes out before reading its operands. A caller may
    // deliberately use one of those vector objects as its output.
    if alias&1!=0 {p=initial;}
    if alias&2!=0 {v=initial;}
    if alias&4!=0 {origin=initial;}
    let base=(w[10]!=0.0).then(||text(word_u32(w[11])));
    let (speed,gravity)=if w[9]!=0.0 {projectile_values(base.as_deref(),(w[12]!=0.0).then_some(w[13]),(w[14]!=0.0).then_some(w[15]))} else {(0.0,0.0)};
    let (point,valid)=ballistic_lead(&p,&v,&origin,speed,gravity);
    let s=state();s.out.clear();s.out.extend([point.x,point.y,point.z]);valid as u32
}

fn vector(v: &mut Vector3, field: u32, value: f64) {
    match field {
        0 => v.x = value,
        1 => v.y = value,
        2 => v.z = value,
        _ => unreachable!(),
    }
}
fn text(id: u32) -> String {
    String::from_utf16_lossy(sim().strings.units(StrId(id)))
}

#[no_mangle]
pub extern "C" fn ai_aimer_new() -> u32 {
    let s = state();
    s.aimers.push(Aimer::default());
    (s.aimers.len() - 1) as u32
}
#[no_mangle]
pub extern "C" fn ai_aimer_read(id: u32) {
    let s = state();
    let a = &s.aimers[id as usize];
    s.out.clear();
    s.out.extend([
        a.direction.x,
        a.direction.y,
        a.direction.z,
        a.velocity.x,
        a.velocity.y,
        a.velocity.z,
        a.started as u8 as f64,
        a.curves.a,
        a.curves.d,
        a.curves.b,
        a.curves.s,
    ]);
}
#[no_mangle]
pub extern "C" fn ai_aimer_write(id: u32, field: u32, value: f64) {
    let a = &mut state().aimers[id as usize];
    match field {
        0..=2 => vector(&mut a.direction, field, value),
        3..=5 => vector(&mut a.velocity, field - 3, value),
        6 => a.started = value != 0.0,
        7 => a.curves.a = value,
        8 => a.curves.d = value,
        9 => a.curves.b = value,
        10 => a.curves.s = value,
        _ => panic!("invalid aim filter field"),
    }
}
#[no_mangle]
pub extern "C" fn ai_aimer_update(id: u32, op: u32) {
    let w: [f64; 14] = sim().calls.args().0[..14].try_into().expect("aim input");
    let desired = Vector3 {
        x: w[0],
        y: w[1],
        z: w[2],
    };
    let dt = w[3];
    let hero = w[4] != 0.0;
    let curves = (w[5] != 0.0).then_some(Curves {
        a: w[6],
        d: w[7],
        b: (w[8] != 0.0).then_some(w[9]),
        s: (w[10] != 0.0).then_some(w[11]),
    });
    let weapon = if w[12] != 0.0 {
        Some(JsObj {
            fields: vec![(Rc::from("base"), Js::Str(Rc::from(text(word_u32(w[13])))))],
            def: None,
        })
    } else {
        None
    };
    let s = state();
    let a = &mut s.aimers[id as usize];
    let scales = if op == 0 {
        a.reset(&desired);
        None
    } else {
        a.update(&desired, dt, curves.as_ref(), weapon.as_ref(), hero)
            .1
    };
    s.out.clear();
    match scales {
        Some(v) => s.out.extend([1.0, v.spread_scale, v.sniper_scale]),
        None => s.out.push(0.0),
    }
}

struct Random(ClassicRandom);
impl Draws for Random {
    fn uniform_a(&mut self) -> f64 {
        unreachable!("Enhanced aim uses stream C")
    }
    fn uniform_b(&mut self) -> f64 {
        unreachable!("Enhanced aim uses stream C")
    }
    fn uniform_c(&mut self) -> f64 {
        self.0.uniform()
    }
}
#[no_mangle]
pub extern "C" fn ai_enhanced_aimer_new(
    reaction: f64,
    floor: f64,
    convergence: f64,
    sigma25: f64,
    slew: f64,
    skill: f64,
) -> u32 {
    let s = state();
    s.enhanced_aimers.push(EnhancedAimer::new(
        Profile {
            reaction,
            reaction_floor: floor,
            convergence,
            sigma25,
            slew,
        },
        skill,
    ));
    (s.enhanced_aimers.len() - 1) as u32
}
#[no_mangle]
pub extern "C" fn ai_enhanced_aimer_read(id: u32) {
    let s = state();
    let a = &s.enhanced_aimers[id as usize];
    let key = |value: &Option<String>| {
        value.as_ref().map_or(0.0, |v| {
            sim()
                .strings
                .intern(&v.encode_utf16().collect::<Vec<_>>())
                .0 as f64
                + 1.0
        })
    };
    s.out.clear();
    s.out.extend([
        a.direction.x,
        a.direction.y,
        a.direction.z,
        a.started as u8 as f64,
        a.error[0],
        a.error[1],
        a.bias[0],
        a.bias[1],
        key(&a.key),
        key(&a.last_key),
        a.lost_at,
        a.acquired_at,
        a.ready_at,
    ]);
}
#[no_mangle]
pub extern "C" fn ai_enhanced_aimer_write(id: u32, field: u32, value: f64) {
    let a = &mut state().enhanced_aimers[id as usize];
    match field {
        0..=2 => vector(&mut a.direction, field, value),
        3 => a.started = value != 0.0,
        4..=5 => a.error[(field - 4) as usize] = value,
        6..=7 => a.bias[(field - 6) as usize] = value,
        8 => a.key = (value != 0.0).then(|| text(word_u32(value) - 1)),
        9 => a.last_key = (value != 0.0).then(|| text(word_u32(value) - 1)),
        10 => a.lost_at = value,
        11 => a.acquired_at = value,
        12 => a.ready_at = value,
        _ => panic!("invalid Enhanced aim field"),
    }
}
#[no_mangle]
pub extern "C" fn ai_enhanced_aimer_command(id: u32, op: u32, seed: u32) -> f64 {
    let w: [f64; 9] = sim().calls.args().0[..9]
        .try_into()
        .expect("Enhanced aim input");
    let mut random = Random(ClassicRandom::from_state(seed));
    let s = state();
    let a = &mut s.enhanced_aimers[id as usize];
    let result = match op {
        0 => {
            a.reset(&Vector3 {
                x: w[0],
                y: w[1],
                z: w[2],
            });
            0.0
        }
        1 => {
            a.observe_key(
                &mut random,
                (w[0] != 0.0).then(|| text(word_u32(w[0]) - 1)),
                w[1],
            );
            0.0
        }
        2 => a.reacting(w[0]) as u8 as f64,
        3 => a.sigma(w[0], w[1], w[2] != 0.0),
        4 => {
            a.update(
                &mut random,
                &Vector3 {
                    x: w[0],
                    y: w[1],
                    z: w[2],
                },
                w[3],
                w[4],
                w[5],
                w[6] != 0.0,
                w[7],
                w[8] != 0.0,
            );
            0.0
        }
        5 => EnhancedAimer::gaussian(&mut random),
        _ => panic!("invalid Enhanced aim command"),
    };
    s.out.clear();
    s.out.push(random.0.state as f64);
    result
}
