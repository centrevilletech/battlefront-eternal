//! A soldier's movement from the store (Soldier.tick): the
//! store's soldier ticked by the module's own movement (crate::movement),
//! with no exchange through JavaScript. It reads and writes what
//! copies in and out, the same way: the tick itself
//! is the kernel's (`movement::advance_profiled`), which the world's ticks already
//! run.

use super::unit::or_zero;
use crate::gen::{Air, MoveInput, Posture, Soldier};
use crate::movement::{MovementOwner, ProfileError};
use crate::movement::soldier::{swbf2::{BodyFacts, Step}, Mover, Air as MAir, Input, Posture as MPosture, V3};

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

/// The footing a store soldier shows to the per-life movement state.
pub(crate) fn body_facts(s: &Soldier) -> BodyFacts {
    BodyFacts { air: air(s.air), grounded: s.grounded, lock: s.lock, posture: posture(s.posture) }
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

/// Prepare the retained mover when UnitFactory builds its body.
pub fn prepare_in(movement: &mut crate::movement::MovementOwner, s: &mut Soldier, world: usize) {
    // His id in the module, with his class's numbers, on his first tick.
    if s.mover < 0.0 {
        let id = movement.new_mover(world, |m| load_class(m, s));
        s.mover = id as f64;
    }
}

pub(crate) fn load_class(m: &mut Mover, s: &Soldier) {
    m.max_speed = s.max_speed;
    m.max_strafe = s.max_strafe;
    m.max_jump = s.max_jump;
    m.max_turn = s.max_turn;
    m.max_pitch = s.max_pitch;
    m.accel = s.accel;
    m.pitch_limits = s.pitch_limits;
    m.jet.jump = s.jet.jump;
    m.jet.push = s.jet.push;
    m.jet.recharge = s.jet.recharge;
    m.jet.cost = s.jet.cost;
    m.jet.initial = s.jet.initial;
    m.jet.min_border = s.jet.min_border;
    m.jet.hover = s.jet.hover;
    m.threshold = s.collision_rule.threshold;
    m.scale = s.collision_rule.scale;
}

/// Soldier.tick(input, dt) for a store soldier, in collision world `world`;
/// `pass_through`: the ray slot of the body he passes through (a rider's
/// mount while he falls from it), -1 for none.
pub fn tick_soldier_in(movement: &mut MovementOwner, collision: &mut crate::collision::State, s: &mut Soldier, input: &MoveInput, dt: f64, world: usize, pass_through: i64, swbf2: Option<Step<'_>>) -> Result<bool, ProfileError> {
    movement.check_step(swbf2.is_some())?;
    let input = profile_input(input, swbf2.is_some());
    let swbf2 = match swbf2 {
        Some(step) if s.mover < 0.0 || dt == 0.0 => {
            let mut body = Mover::for_family(world, bfe_family::GameFamily::Swbf2);
            load_class(&mut body, s);
            load_body(&mut body, s, pass_through);
            if dt == 0.0 {
                let rolling = body.validate_swbf2(&input, dt, &step)?;
                return Ok(!rolling && step.state.tumble.is_none() && step.state.sprint_time.is_none() && body.lock == 0.0);
            }
            let (id, can_fire) = movement.advance_initial_swbf2(collision, body, &input, dt, step)?;
            s.mover = f64::from(id);
            store_body(s, movement.mover(id), dt);
            return Ok(can_fire);
        }
        other => other,
    };
    prepare_in(movement, s, world);
    let id = s.mover as u32;
    let m = movement.mover(id);
    load_body(m, s, pass_through);
    let can_fire = movement.advance_profiled(collision, id, &input, dt, swbf2)?;
    store_body(s, movement.mover(id), dt);
    Ok(can_fire)
}

pub(crate) fn load_body(m: &mut Mover, s: &Soldier, pass_through: i64) {
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
}

pub(crate) fn mover_input(input: &MoveInput) -> Input {
    // `input.turn || 0`, `input.pitch || 0`, and a jet direction that names
    // a direction at all.
    let jet_direction = input.jet_direction.filter(|d| d.iter().any(|x| *x != 0.0 && !x.is_nan())).map(V3::from);
    Input {
        forward: input.forward,
        right: input.right,
        turn: or_zero(input.turn),
        pitch: or_zero(input.pitch),
        jump: input.jump,
        crouch: input.crouch,
        prone: input.prone,
        jet_direction,
    }
}

/// Battlefront II checks the supplied axes before any look or physics arithmetic.
pub(crate) fn profile_input(input: &MoveInput, swbf2: bool) -> Input {
    let mut value = mover_input(input);
    if swbf2 {
        value.turn = input.turn;
        value.pitch = input.pitch;
        value.jet_direction = input.jet_direction.filter(|d| d.iter().any(|x| *x != 0.0)).map(V3::from);
    }
    value
}

pub(crate) fn store_body(s: &mut Soldier, m: &Mover, dt: f64) {
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
