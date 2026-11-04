//! Persistent navigation controls on the battle's AI instance. The
//! caller supplies only the unit fields read by the operation; paths,
//! executors, movement requests and follow trails remain here.

use super::{
    module::state,
    nav::{
        goto::{GotoWorld, Resolved},
        math::Vec3,
        units::{ClassDef, NavUnit, SoldierView, UnitRef},
    },
    random::ClassicRandom,
};
use crate::{abi::sim, store::word_u32};

fn id(value: f64) -> Option<usize> {
    (value >= 0.0).then_some(value as usize)
}
fn boolean(value: f64) -> bool {
    value != 0.0
}
fn vector(out: &mut Vec<f64>, v: Vec3) {
    out.extend([v.x, v.y, v.z]);
}
fn coordinate(v: &mut Vec3, component: u32, value: f64) {
    match component {
        0 => v.x = value,
        1 => v.y = value,
        2 => v.z = value,
        _ => unreachable!(),
    }
}

#[no_mangle]
pub extern "C" fn ai_nav_control_read(control: u32) {
    let s = state();
    let c = &s.nav.controls[control as usize];
    let out = &mut s.out;
    out.clear();
    out.extend([
        c.unit.map_or(0.0, |v| v as f64),
        c.generation,
        c.serial,
        c.disposed as u8 as f64,
        c.published.map_or(-1.0, |v| v as f64),
        c.ready as u8 as f64,
        c.segments as f64,
        c.install_flags[0] as u8 as f64,
        c.install_flags[1] as u8 as f64,
        c.landing as u8 as f64,
        c.stance,
        c.jet.pending as u8 as f64,
        c.jetting as u8 as f64,
        c.avoid_side,
        c.stranded as u8 as f64,
        c.stranded_for,
        c.leader.map_or(0.0, |r| r.unit as f64),
        c.leader.and_then(|r| r.generation).is_some() as u8 as f64,
        c.leader.and_then(|r| r.generation).unwrap_or(0.0),
        c.trail.map_or(-1.0, |v| v as f64),
        c.trail_delay,
        c.request.valid as u8 as f64,
        c.request.mode,
        c.request.lower,
        c.request.upper,
    ]);
    for v in [
        c.jet.vector,
        c.stranded_at,
        c.request.steering_target,
        c.request.waypoint,
        c.request.segment_direction,
    ] {
        vector(out, v);
    }
    if let Some(e) = &c.executor {
        out.extend([
            e.path as f64,
            e.index as f64,
            e.mode,
            e.side,
            e.complete as u8 as f64,
            e.status,
            e.stationary,
            e.best,
            e.no_progress,
            e.hold as u8 as f64,
        ]);
        for v in [
            e.prev,
            e.waypoint,
            e.next,
            e.fin,
            e.direction,
            e.anchor,
            e.target,
        ] {
            vector(out, v);
        }
    } else {
        out.push(-1.0);
    }
}

/// Scalar writes made by callers still using controller facades. Vector
/// components use the same field numbering as the reusable read block.
#[no_mangle]
pub extern "C" fn ai_nav_control_write(control: u32, field: u32, value: f64) {
    let c = &mut state().nav.controls[control as usize];
    match field {
        0 => c.unit = (value != 0.0).then_some(value as u64),
        1 => c.generation = value,
        2 => c.serial = value,
        3 => c.disposed = boolean(value),
        4 => c.published = id(value),
        5 => c.ready = boolean(value),
        6 => c.segments = word_u32(value),
        7..=8 => c.install_flags[(field - 7) as usize] = boolean(value),
        9 => c.landing = boolean(value),
        10 => c.stance = value,
        11 => c.jet.pending = boolean(value),
        12 => c.jetting = boolean(value),
        13 => c.avoid_side = value,
        14 => c.stranded = boolean(value),
        15 => c.stranded_for = value,
        19 => c.trail = id(value),
        20 => c.trail_delay = value,
        21 => c.request.valid = boolean(value),
        22 => c.request.mode = value,
        23 => c.request.lower = value,
        24 => c.request.upper = value,
        25..=27 => coordinate(&mut c.jet.vector, field - 25, value),
        28..=30 => coordinate(&mut c.stranded_at, field - 28, value),
        31..=33 => coordinate(&mut c.request.steering_target, field - 31, value),
        34..=36 => coordinate(&mut c.request.waypoint, field - 34, value),
        37..=39 => coordinate(&mut c.request.segment_direction, field - 37, value),
        44 => c.executor.as_mut().expect("path executor").complete = boolean(value),
        49 => c.executor.as_mut().expect("path executor").hold = boolean(value),
        _ => panic!("invalid navigation control field"),
    }
}

struct Input<'a> {
    words: &'a [f64],
    at: usize,
}
impl Input<'_> {
    fn number(&mut self) -> f64 {
        let v = self.words[self.at];
        self.at += 1;
        v
    }
    fn optional(&mut self) -> Option<f64> {
        let present = self.number() != 0.0;
        let v = self.number();
        present.then_some(v)
    }
    fn vector(&mut self) -> Vec3 {
        Vec3::new(self.number(), self.number(), self.number())
    }
    fn unit(&mut self) -> Option<NavUnit> {
        if self.number() == 0.0 {
            return None;
        }
        let id = self.number() as u64;
        let kind = self.number();
        let position = self.vector();
        let mut u = NavUnit::new(id, kind, position);
        u.velocity = self.vector();
        u.generation = self.optional();
        if self.number() != 0.0 {
            u.class_def = Some(ClassDef {
                max_speed: self.optional(),
                forward_speed: self.optional(),
                turn_rate: self.optional(),
                ..Default::default()
            });
        }
        if self.number() != 0.0 {
            let aim_yaw = self.optional();
            let avoidance = if self.number() != 0.0 {
                Some(self.vector())
            } else {
                None
            };
            u.soldier = Some(SoldierView {
                aim_yaw,
                avoidance,
                ..Default::default()
            });
        }
        if self.number() != 0.0 {
            u.forward = Some(self.vector());
        }
        Some(u)
    }
}

struct World {
    random: ClassicRandom,
    leader: Option<NavUnit>,
    far: f64,
}
impl GotoWorld for World {
    fn random_b(&mut self) -> &mut ClassicRandom {
        &mut self.random
    }
    fn leader(&mut self, _: UnitRef) -> Option<NavUnit> {
        self.leader.clone()
    }
    fn follow_far(&mut self) -> f64 {
        self.far
    }
    fn time(&self) -> f64 {
        unreachable!("navigation control does not read time")
    }
    fn random_a(&mut self) -> &mut ClassicRandom {
        unreachable!("navigation control uses stream B")
    }
    fn resolve(&mut self, _: UnitRef) -> Option<Resolved> {
        unreachable!("navigation control reads its leader")
    }
    fn remembered(&mut self, _: u64) -> Option<Vec3> {
        unreachable!("navigation control has no remembered target")
    }
    fn clear_above(&mut self, _: Vec3, _: &NavUnit) -> bool {
        unreachable!("navigation control does not test jet clearance")
    }
}

/// No imports occur while this operation owns navigation. A command's
/// result includes the source RNG state, the soldier contact write and
/// any planner publications caused by segment completion.
#[no_mangle]
pub extern "C" fn ai_nav_control_command(op: u32, control: u32, a: f64, b: f64, c: f64) {
    let s = sim();
    let (words, _) = s.calls.args();
    let mut input = Input { words, at: 0 };
    let mut unit = input.unit();
    let leader = input.unit();
    let mut world = World {
        random: ClassicRandom::from_state(word_u32(input.number())),
        leader,
        far: input.number(),
    };
    let s = state();
    let n = &mut s.nav;
    let owner = control as usize;
    let result = match op {
        0 => {
            n.control_attach(owner, unit.as_ref());
            0.0
        }
        1 => {
            n.control_reset(owner);
            0.0
        }
        2 => {
            n.control_strand(owner, boolean(a), unit.as_ref());
            0.0
        }
        3 => {
            n.control_publish(owner, id(a), boolean(b));
            0.0
        }
        4 => {
            if !n.controls[owner].ready {
                0.0
            } else if n.controls[owner].published.is_none() {
                1.0
            } else {
                let (ready, success) =
                    n.control_consume(owner, a, unit.as_mut().expect("consume with a unit"));
                ready as u8 as f64 + 2.0 * success as u8 as f64
            }
        }
        5 => {
            n.control_install(
                owner,
                a as usize,
                boolean(b),
                boolean(c),
                unit.as_mut().expect("install with a unit"),
            );
            0.0
        }
        6 => {
            n.control_append(
                owner,
                a as usize,
                unit.as_mut().expect("append with a unit"),
            );
            0.0
        }
        7 => {
            n.control_stop(owner, unit.as_ref());
            0.0
        }
        8 => {
            if let Some(unit) = unit.as_mut() {
                n.control_update(owner, a, unit, &mut world);
            } else if n.controls[owner].executor.is_none() && n.controls[owner].stranded {
                n.controls[owner].stranded_for =
                    super::nav::math::f32(n.controls[owner].stranded_for + a);
            }
            0.0
        }
        9 => {
            n.control_drop_executor(owner);
            0.0
        }
        10 => {
            n.control_set_leader(
                owner,
                (a != 0.0).then_some(UnitRef {
                    unit: a as u64,
                    generation: boolean(b).then_some(c),
                }),
            );
            0.0
        }
        11 => {
            let leader = world.leader.clone().expect("trail sample leader");
            n.control_sample_trail(
                owner,
                a,
                &leader,
                &mut world,
                unit.as_mut().expect("trail sample unit"),
            );
            0.0
        }
        12 => {
            n.control_record_trail(owner, boolean(a), unit.as_mut().expect("trail with a unit"));
            0.0
        }
        13 => {
            n.control_consume_segment(owner);
            0.0
        }
        14 => {
            n.control_dispose(owner);
            0.0
        }
        15 => n.control_status(owner, unit.as_ref()),
        _ => panic!("invalid navigation control command"),
    };
    s.out.clear();
    s.out.extend([result, world.random.state as f64]);
    if let Some(avoidance) = unit.and_then(|u| u.soldier).and_then(|s| s.avoidance) {
        s.out.push(1.0);
        vector(&mut s.out, avoidance);
    } else {
        s.out.extend([0.0, 0.0, 0.0, 0.0]);
    }
    for (owner, path, ready) in n.publications.as_mut().unwrap().drain(..) {
        s.out.extend([
            owner as f64,
            path.map_or(-1.0, |p| p as f64),
            ready as u8 as f64,
        ]);
    }
}
