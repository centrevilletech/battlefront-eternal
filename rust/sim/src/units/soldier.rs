//! A soldier's movement from the store (src/soldier.js Soldier.tick): the
//! store's soldier ticked by the module's own movement (crate::movement),
//! with no exchange through JavaScript. It reads and writes what
//! src/movement-kernel.js copies in and out, the same way: the tick itself
//! is the kernel's (`movement::advance`), which the world's ticks already
//! run (world-hash's `--movement wasm` is held to the JavaScript's).

use super::unit::or_zero;
use crate::gen::{Air, MoveInput, Posture, Soldier};
use crate::movement::soldier::{Air as MAir, Input, Posture as MPosture, V3};
use crate::movement::{advance, mover, new_mover};

fn posture(p: Posture) -> MPosture {
    match p {
        Posture::Stand => MPosture::Stand,
        Posture::Crouch => MPosture::Crouch,
        Posture::Prone => MPosture::Prone,
    }
}

fn posture_back(p: MPosture) -> Posture {
    match p {
        MPosture::Stand => Posture::Stand,
        MPosture::Crouch => Posture::Crouch,
        MPosture::Prone => Posture::Prone,
    }
}

fn air(a: Option<Air>) -> MAir {
    match a {
        None => MAir::None,
        Some(Air::Jump) => MAir::Jump,
        Some(Air::Fall) => MAir::Fall,
        Some(Air::Thrown) => MAir::Thrown,
        Some(Air::Jet) => MAir::Jet,
    }
}

fn air_back(a: MAir) -> Option<Air> {
    match a {
        MAir::None => None,
        MAir::Jump => Some(Air::Jump),
        MAir::Fall => Some(Air::Fall),
        MAir::Thrown => Some(Air::Thrown),
        MAir::Jet => Some(Air::Jet),
    }
}

/// Soldier.tick(input, dt) for a store soldier, in collision world `world`;
/// `pass_through`: the ray slot of the body he passes through (a rider's
/// mount while he falls from it), -1 for none.
pub fn tick_soldier(s: &mut Soldier, input: &MoveInput, dt: f64, world: usize, pass_through: i64) {
    // His id in the module, with his class's numbers, on his first tick.
    if s.mover < 0.0 {
        let c = s.clone();
        s.mover = new_mover(world, |m| {
            m.max_speed = c.max_speed;
            m.max_strafe = c.max_strafe;
            m.max_jump = c.max_jump;
            m.max_turn = c.max_turn;
            m.max_pitch = c.max_pitch;
            m.accel = c.accel;
            m.pitch_limits = c.pitch_limits;
            m.jet.jump = c.jet.jump;
            m.jet.push = c.jet.push;
            m.jet.recharge = c.jet.recharge;
            m.jet.cost = c.jet.cost;
            m.jet.initial = c.jet.initial;
            m.jet.min_border = c.jet.min_border;
            m.jet.hover = c.jet.hover;
            m.threshold = c.collision_rule.threshold;
            m.scale = c.collision_rule.scale;
        }) as f64;
    }
    let id = s.mover as u32;
    let m = mover(id);
    m.position = V3::from(s.position);
    m.settled = V3::from(s.settled);
    m.velocity = V3::from(s.velocity);
    m.normal = V3::from(s.normal);
    m.avoidance = V3::from(s.avoidance);
    m.aim_yaw = s.aim_yaw;
    m.aim_pitch = s.aim_pitch;
    m.state_time = s.state_time;
    m.lock = s.lock;
    m.fuel = s.fuel;
    m.dive = s.dive;
    m.posture = posture(s.posture);
    m.air = air(s.air);
    m.dive_from = posture(s.dive_from);
    m.grounded = s.grounded;
    m.ground_adjustment = s.ground_adjustment;
    m.max_fly_height = s.max_fly_height;
    m.getup_time = s.getup_time;
    m.dive_time = s.dive_time;
    m.pass_through = pass_through;
    // `input.turn || 0`, `input.pitch || 0`, and a jet direction that names
    // a direction at all.
    let jet_direction = input.jet_direction.filter(|d| d.iter().any(|x| *x != 0.0 && !x.is_nan())).map(V3::from);
    let inp = Input {
        forward: input.forward,
        right: input.right,
        turn: or_zero(input.turn),
        pitch: or_zero(input.pitch),
        jump: input.jump,
        crouch: input.crouch,
        prone: input.prone,
        jet_direction,
    };
    advance(id, &inp, dt);
    let m = mover(id);
    s.previous = s.position;
    s.position = m.position.arr();
    s.settled = m.settled.arr();
    s.velocity = m.velocity.arr();
    s.normal = m.normal.arr();
    s.avoidance = m.avoidance.arr();
    s.aim_yaw = m.aim_yaw;
    s.aim_pitch = m.aim_pitch;
    s.state_time = m.state_time;
    s.lock = m.lock;
    s.fuel = m.fuel;
    s.dive = m.dive;
    s.posture = posture_back(m.posture);
    s.air = air_back(m.air);
    s.dive_from = posture_back(m.dive_from);
    s.grounded = m.grounded;
    s.ground_adjustment = m.ground_adjustment;
    s.collision_damage = m.collision_damage;
    s.tick_length = dt;
    s.look_x = 0.0;
    s.look_y = 0.0;
}
