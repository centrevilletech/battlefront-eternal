//! A hull and its tick on the ground (src/vehicles.js `Vehicle`: tick,
//! aimCrew, steer, aimPlayerSeat, clampAim, holdYaw, syncBody, damage,
//! setHealth, the wheels and sleeping; src/vehicle-water.js
//! waterDamageStep).
//!
//! Field names are the JavaScript's, in snake case; what the hull holds for
//! the battle (its class's speeds, its box, its probes, its springs) sits
//! beside what it does (its pose, its rates, its seats' aim). A hull's
//! flight (`fly`) is the flight package's and not here.

use super::{hover, walker, BodyId, VehicleWorld};
use crate::gen::OrdnanceOwner;
use crate::rig::math;
use three::{clamp, Euler, EulerOrder, Matrix4, Quaternion, Vector3};
use v8math::{abs, sign};

pub(crate) const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
pub(crate) const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);
const UNIT_SCALE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
/// src/vehicle-hover.js HOVER_REST: a hull at rest on its springs keeps a
/// tick's gravity in its velocity, and sleeps below this speed (m/s).
const HOVER_REST: f64 = 0.15;
/// src/vehicle-water.js UPDATES_PER_SECOND.
const UPDATES_PER_SECOND: f64 = 60.0;

/// `const angle = (v) => Math.atan2(Math.sin(v), Math.cos(v))` (vehicles.js 35).
#[inline(always)]
pub fn angle(v: f64) -> f64 {
    let s = math::sin(v);
    let c = math::cos(v);
    math::atan2(s, c)
}

/// `const approach = (v, target, delta) => v + clamp(target - v, -delta, delta)` (vehicles.js 36).
#[inline(always)]
pub fn approach(v: f64, target: f64, delta: f64) -> f64 {
    v + clamp(target - v, -delta, delta)
}

/// JavaScript's truthiness of a number: false for 0, -0 and NaN.
#[inline(always)]
pub fn truthy(x: f64) -> bool {
    !(x == 0.0 || x.is_nan())
}

/// `x || 0` for a number: 0 (+0) for 0, -0 and NaN.
#[inline(always)]
pub fn or_zero(x: f64) -> f64 {
    if truthy(x) {
        x
    } else {
        0.0
    }
}

/// A seat's lease as the hull's tick reads it (src/seats.js SeatLease): a
/// person's (the player's own, `human`) or a unit's, a network player's
/// (`network_human`) or a bot's, and its frozen input's analogs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lease {
    pub human: bool,
    pub network_human: bool,
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub brake: bool,
    /// The unit on the seat (a tripped walker's credit).
    pub unit: Option<crate::store::Ref<crate::gen::Unit>>,
}

/// A seat: its aim's rates for a bot (aimCrew, from its section's TurnRate
/// and PitchRate over the class's) and limits (seatView), its aim, and who
/// holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seat {
    pub turn_rate: f64,
    pub pitch_rate: f64,
    pub pitch_limits: [f64; 2],
    pub yaw_limits: Option<[f64; 2]>,
    pub wrap_yaw: bool,
    pub aim_yaw: f64,
    pub pitch: f64,
    pub look_yaw: Option<f64>,
    pub look_pitch: Option<f64>,
    pub lease: Option<Lease>,
}

/// A wheel (the Hailfire's): its place across the hull, its radius, its turn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    pub x: f64,
    pub radius: f64,
    pub phase: f64,
    pub previous: f64,
    /// Its turn at the start of the world tick (captureTick).
    pub tick_from: Option<f64>,
}

/// A walker's water damage (src/vehicle-water.js waterDamageRule).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterRule {
    pub interval: f64,
    pub amount: f64,
}

/// A hover spring (src/vehicle-hover.js hoverClass: an AddSpringBody
/// sphere in the hull's frame, its travel and its six factors).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub at: Vector3,
    pub radius: f64,
    pub length: f64,
    pub factors: [f64; 6],
}

/// A hover class's keys (src/vehicle-hover.js hoverClass).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HoverClass {
    pub altitude: f64,
    pub gravity_scale: f64,
    pub lift_spring: f64,
    pub lift_damp: f64,
    pub traction: f64,
    pub level_spring: f64,
    pub level_damp: f64,
    pub velocity_spring: f64,
    pub velocity_damp: f64,
    pub omega_x_spring: f64,
    pub omega_x_damp: f64,
    pub omega_z_spring: f64,
    pub omega_z_damp: f64,
    pub springs: Vec<Spring>,
}

/// A walker's legs (the class's `fx.legs`, tools/zero_vehicle.py): each
/// pair's feet in the hull's frame, the rays' length, whether the pair
/// stands at its feet's mean (WalkerOrientRoll), the walker's width.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Legs {
    pub ray_length: f64,
    pub orient_roll: bool,
    pub width: f64,
    pub pairs: Vec<[[f64; 3]; 2]>,
}

/// What a hull's tick reads of its input (an idle stick, the player's
/// keys or a bot pilot's frozen SeatInput): `|| 0` and `Boolean()` are
/// applied where the tick reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub brake: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Hull {
    /// Its place in the battle's list (Vehicles.vehicles), and its handle in the store.
    pub index: usize,
    pub id: crate::store::Ref<crate::gen::Vehicle>,
    // What it is, for the battle.
    pub flying: bool,
    pub walking: bool,
    pub creature: bool,
    pub wheeled: bool,
    pub repulsor: bool,
    pub speed: f64,
    pub reverse_speed: f64,
    pub strafe_speed: f64,
    pub acceleration: f64,
    pub deceleration: f64,
    pub turn_rate: f64,
    pub hover: f64,
    pub bounds_min: Vector3,
    pub bounds_max: Vector3,
    pub center: Vector3,
    pub size: Vector3,
    pub bound_radius: f64,
    pub probe_radius: f64,
    pub probes: Vec<Vector3>,
    pub support_probes: Vec<Vector3>,
    pub max_health: f64,
    pub water_rule: Option<WaterRule>,
    pub water_depth: f64,
    pub hover_class: Option<HoverClass>,
    pub legs: Option<Legs>,
    pub body: BodyId,
    /// Its class's dying animation's length (`fx.dying.span || 0`).
    pub dying_span: f64,
    /// Its class's ExplosionOffset in its frame, and whether it has an explosion.
    pub explosion_offset: Option<[f64; 3]>,
    pub has_explosion: bool,
    // What it does.
    pub position: Vector3,
    pub previous: Vector3,
    pub velocity: Vector3,
    pub yaw: f64,
    pub previous_yaw: f64,
    pub tilt: f64,
    pub roll: f64,
    pub previous_tilt: f64,
    pub previous_roll: f64,
    pub yaw_rate: f64,
    pub tilt_rate: f64,
    pub roll_rate: f64,
    pub support: f64,
    pub forward_input: f64,
    pub right_input: f64,
    pub alive: bool,
    /// None: not set yet (a hull that never spawned).
    pub sleeping: Option<bool>,
    pub health: f64,
    pub dying: f64,
    pub water_clock: f64,
    pub quaternion: Quaternion,
    pub matrix: Matrix4,
    /// Its item target's centre (its box's centre in the world).
    pub item_center: Vector3,
    pub support_normal: Vector3,
    /// A walker's legs' sides (standOnLegs), once it has stood on them.
    pub leg_side: Option<Vec<f64>>,
    pub seat_index: usize,
    /// Whether the page's player drives it (VehicleDriver; never in a world).
    pub driver: bool,
    pub wheels: Vec<Wheel>,
    pub seats: Vec<Seat>,
    /// Its pose at the start of the world tick (captureTick).
    pub tick_from: Option<Vector3>,
    pub tick_from_yaw: Option<f64>,
    pub tick_from_tilt: Option<f64>,
    pub tick_from_roll: Option<f64>,
    /// Whether its rig shows (root.visible: a destroyed walker while it dies).
    pub visible: bool,
    /// Who last damaged it, for kill credit (`lastAttacker`).
    pub last_attacker: OrdnanceOwner,
    /// Its tow cable: the walker it holds (-1 none), the anchor in that
    /// walker's frame, the leg's name, the angle round it, the winding.
    pub tow_target: f64,
    pub tow_local: Vector3,
    pub tow_leg: String,
    pub tow_angle: f64,
    pub tow_winding: f64,
    /// What its dying and its final blast read of its class.
    pub has_final_explosion: bool,
    pub final_offset: [f64; 3],
    /// Its spawn pad (its place in the battle's list of pads), and its
    /// entry in the units registry: its number there and the life it is on.
    pub slot_index: usize,
    pub target_id: f64,
    pub target_generation: f64,
    /// Its pose (its rig, its seats' aim state): `pose.rs`.
    pub pose: Option<super::pose::HullPose>,
    /// Its flight (a craft's; every hull's flight state): crate::flight.
    pub flight: crate::flight::Flight,
}

impl Hull {
    /// A bot holds seat i (`crewed`).
    pub fn crewed(&self, i: usize) -> bool {
        matches!(self.seats.get(i).and_then(|s| s.lease), Some(lease) if !lease.human)
    }

    /// Anyone aboard (`occupied`): the player or a lease on any seat.
    pub fn occupied(&self) -> bool {
        self.driver || self.seats.iter().any(|s| s.lease.is_some())
    }

    /// A ground hull's frame as the physics reads it (src/vehicle-hover.js
    /// hullQuaternion): heading, then pitch and roll.
    pub fn hull_quaternion(&self, out: &mut Quaternion) {
        out.set_from_euler(&Euler::new(-self.tilt, self.yaw, self.roll, EulerOrder::YXZ));
    }

    /// vehicles.js 970: one substep. A flyer's tick is the flight
    /// package's (crate::flight::fly, `flyer_tick`), taken at 1/60 s.
    pub fn tick<W: VehicleWorld>(&mut self, w: &mut W, dt: f64, input: &Input) {
        self.previous = self.position;
        self.previous_yaw = self.yaw;
        self.previous_tilt = self.tilt;
        self.previous_roll = self.roll;
        for wheel in &mut self.wheels {
            wheel.previous = wheel.phase;
        }
        if !self.alive || self.sleeping == Some(true) && !self.occupied() {
            return;
        }
        self.aim_crew(dt);
        debug_assert!(!self.flying, "a flyer's tick is flight's");
        let old_yaw = self.yaw;
        self.steer(dt, input, self.strafe_speed == 0.0);
        let response = 1.0 - math::exp(-10.0 * dt);
        self.forward_input += (or_zero(input.forward) - self.forward_input) * response;
        self.right_input += (or_zero(input.right) - self.right_input) * response;
        let driving = (self.seat_index == 0 || self.crewed(0)) && !input.brake;
        if !self.walking {
            // A hover: its physics, then the collision pass (its wall
            // spheres, then its springs), then its springs' forces.
            let (fwd, side) = if driving { (self.forward_input, self.right_input) } else { (0.0, 0.0) };
            hover::hover_physics(self, w, dt, fwd, side, input.brake);
            w.enable_body(self.body, false);
            let mut q = Quaternion::default();
            self.hull_quaternion(&mut q);
            self.push_walls(w, old_yaw, &q, true);
            let compression = hover::hover_contacts(self, w);
            w.enable_body(self.body, self.alive);
            hover::hover_springs(self, dt, &compression);
        } else {
            walker::walk_ground(self, w, dt, input, driving, old_yaw);
        }
        if self.wheeled {
            let dx = self.position.x - self.previous.x;
            let dz = self.position.z - self.previous.z;
            let distance = dx * math::sin(self.yaw) + dz * math::cos(self.yaw);
            let turned = angle(self.yaw - self.previous_yaw);
            for wheel in &mut self.wheels {
                wheel.phase += (distance - turned * wheel.x) / wheel.radius;
            }
        }
        self.sync_body(w);
        if w.in_death_region(&self.position) || self.position.y < -300.0 {
            self.damage(w, f64::INFINITY, OrdnanceOwner::Null);
        }
        // Deep water wears a walker down (src/vehicle-water.js).
        if self.walking && self.alive {
            let hurt = self.water_damage_step(w, dt);
            if hurt > 0.0 {
                self.damage(w, hurt, OrdnanceOwner::Null);
            }
        }
        let rest = if self.walking { 0.0001 } else { HOVER_REST * HOVER_REST };
        self.sleeping = Some(
            !self.occupied()
                && self.velocity.length_sq() < rest
                && self.previous.distance_to_squared(&self.position) < 0.000001
                && abs(self.tilt - self.previous_tilt) + abs(self.roll - self.previous_roll) < 1e-5,
        );
    }

    /// src/vehicle-water.js waterDamageStep, for this hull's clock.
    fn water_damage_step<W: VehicleWorld>(&mut self, w: &mut W, dt: f64) -> f64 {
        let Some(rule) = self.water_rule else { return 0.0 };
        if !w.has_water() || !(dt > 0.0) {
            return 0.0;
        }
        let under = w.water_over(self.position.x, self.position.z) && w.water_level() >= self.position.y + self.water_depth;
        if !under {
            self.water_clock = rule.interval;
            return 0.0;
        }
        self.water_clock -= dt;
        if self.water_clock < 0.0 {
            rule.amount * UPDATES_PER_SECOND * dt
        } else {
            0.0
        }
    }

    /// vehicles.js aimCrew: bot seats turn their aim by their analogs.
    pub(crate) fn aim_crew(&mut self, dt: f64) {
        for i in 0..self.seats.len() {
            let seat = &mut self.seats[i];
            let Some(lease) = seat.lease else { continue };
            if lease.human || lease.network_human {
                continue;
            }
            if i > 0 {
                seat.aim_yaw -= lease.turn * seat.turn_rate * dt;
            }
            if self.flying && i == 0 {
                continue;
            }
            seat.pitch += lease.pitch * seat.pitch_rate * dt;
            self.clamp_aim(i);
        }
    }

    /// vehicles.js aimPlayerSeat: the player's look is his seat's aim.
    pub(crate) fn aim_player_seat(&mut self) {
        let i = self.seat_index;
        if let Some(look_yaw) = self.seats[i].look_yaw {
            self.seats[i].aim_yaw = look_yaw;
            // `this.pitch = this.lookPitch`. A look is set whole (seatLook,
            // restAim): a yaw without a pitch never happens; were it to, the
            // JavaScript's pitch would be null where this keeps 0.
            self.seats[i].pitch = self.seats[i].look_pitch.unwrap_or(0.0);
        }
        if self.driver {
            self.clamp_aim(i);
        }
    }

    /// vehicles.js steer: the hull follows its pilot's aim.
    fn steer(&mut self, dt: f64, input: &Input, keyboard_steers: bool) {
        self.aim_player_seat();
        let bot = self.crewed(0);
        if self.seat_index != 0 && !bot {
            self.yaw_rate *= math::exp(-10.0 * dt);
            return;
        }
        let keys = clamp(or_zero(input.turn) + if keyboard_steers { or_zero(input.right) } else { 0.0 }, -1.0, 1.0);
        let delta = -keys * self.turn_rate * dt;
        self.seats[0].aim_yaw += delta;
        let network = self.seats[0].lease.is_some_and(|l| l.network_human);
        let i = self.seat_index;
        if !bot || network {
            if let Some(look_yaw) = self.seats[i].look_yaw {
                self.seats[i].look_yaw = Some(look_yaw + delta);
            }
        }
        let error = angle(self.seats[0].aim_yaw - self.yaw);
        let wanted = clamp(error * 5.0, -self.turn_rate, self.turn_rate);
        self.yaw_rate += (wanted - self.yaw_rate) * (1.0 - math::exp(-9.0 * dt));
        let step = self.yaw_rate * dt;
        self.yaw += if sign(step) == sign(error) && abs(step) > abs(error) { error } else { step };
        // A turret's arc turns with the hull under it.
        if self.driver && self.seat_index != 0 {
            self.clamp_aim(self.seat_index);
        }
    }

    /// vehicles.js clampAim: a seat's aim held inside its limits.
    pub fn clamp_aim(&mut self, i: usize) {
        if self.flying && i == 0 {
            return;
        }
        let yaw = self.yaw;
        let seat = &mut self.seats[i];
        let (low, high) = (seat.pitch_limits[0], seat.pitch_limits[1]);
        seat.pitch = clamp(seat.pitch, low, high);
        if let Some(look_pitch) = seat.look_pitch {
            seat.look_pitch = Some(clamp(look_pitch, low, high));
        }
        let Some(limits) = seat.yaw_limits else { return };
        seat.aim_yaw = hold_yaw(yaw, limits, seat.wrap_yaw, seat.aim_yaw);
        if let Some(look_yaw) = seat.look_yaw {
            seat.look_yaw = Some(seat.aim_yaw + angle(hold_yaw(yaw, limits, seat.wrap_yaw, look_yaw) - seat.aim_yaw));
        }
    }

    /// vehicles.js pushWalls: the hull's wall probes, through the world.
    pub(crate) fn push_walls<W: VehicleWorld>(&mut self, w: &mut W, old_yaw: f64, q: &Quaternion, terrain: bool) {
        let own = if self.walking {
            let mut own = Quaternion::default();
            own.set_from_axis_angle(&UP, self.previous_yaw);
            Some(own)
        } else {
            None
        };
        let previous = self.previous;
        if w.push_walls(self.index, &self.probes, self.probe_radius, q, own.as_ref(), &previous, &mut self.position, &mut self.velocity, terrain) {
            self.yaw = old_yaw;
        }
    }

    /// vehicles.js syncBody: the collision body and the matrix gameplay
    /// reads, with the hull's pitch and roll; its quaternion stays its
    /// heading alone.
    pub fn sync_body<W: VehicleWorld>(&mut self, w: &mut W) {
        if !self.flying {
            self.quaternion.set_from_euler(&Euler::new(0.0, self.yaw, 0.0, EulerOrder::YXZ));
        }
        let mut q = Quaternion::default();
        let rotation = if self.flying {
            self.quaternion
        } else {
            self.hull_quaternion(&mut q);
            q
        };
        let position = self.position;
        self.matrix.compose(&position, &rotation, &UNIT_SCALE);
        w.move_body(self.body, &self.matrix);
        self.item_center.copy(&self.center).apply_matrix4(&self.matrix);
    }

    /// vehicles.js alignGround: tilt and roll toward a ground normal.
    pub fn align_ground(&mut self, normal: &Vector3, blend: f64) {
        let sin = math::sin(self.yaw);
        let cos = math::cos(self.yaw);
        let x = cos * normal.x - sin * normal.z;
        let z = sin * normal.x + cos * normal.z;
        self.tilt += (-math::atan2(z, normal.y) - self.tilt) * blend;
        self.roll += (-math::atan2(x, v8math::hypot(normal.y, z)) - self.roll) * blend;
    }

    /// vehicles.js damage: `attacker` (Null: nobody) is credited.
    pub fn damage<W: VehicleWorld>(&mut self, w: &mut W, amount: f64, attacker: OrdnanceOwner) {
        if !w.damage_allowed() { return; }
        if !self.alive || !(amount > 0.0) {
            return;
        }
        if attacker != OrdnanceOwner::Null {
            self.last_attacker = attacker;
        }
        w.current(self);
        w.damaged(self.index, amount, &attacker);
        w.reload(self);
        self.set_health(w, self.health - amount);
    }

    /// vehicles.js captureTick: the pose at the start of a world tick, for
    /// drawing between ticks (a flyer's quaternion is the flight package's).
    pub fn capture_tick(&mut self) {
        self.tick_from = Some(self.position);
        self.tick_from_yaw = Some(self.yaw);
        self.tick_from_tilt = Some(self.tilt);
        self.tick_from_roll = Some(self.roll);
        for wheel in &mut self.wheels {
            wheel.tick_from = Some(wheel.phase);
        }
    }

    /// vehicles.js dyingTick: a destroyed walker's death animation plays
    /// out; once it has, its final blast goes off and it is gone.
    pub fn dying_tick<W: VehicleWorld>(&mut self, w: &mut W, dt: f64) {
        if !(self.dying > 0.0) {
            return;
        }
        self.dying = v8math::max(0.0, self.dying - dt);
        if self.dying > 0.0 {
            return;
        }
        self.visible = false;
        if !self.has_final_explosion {
            return;
        }
        let o = self.final_offset;
        let mut at = Vector3::new(o[0], o[1], o[2]);
        apply_axis_angle(&mut at, &UP, self.yaw);
        at.add(&self.position);
        w.current(self);
        w.explosion(self.index, true, &at);
        w.reload(self);
    }

    /// vehicles.js setHealth: down to 0, the hull is destroyed.
    pub fn set_health<W: VehicleWorld>(&mut self, w: &mut W, value: f64) {
        if !self.alive {
            return;
        }
        self.health = clamp(value, 0.0, self.max_health);
        if self.health > 0.0 {
            return;
        }
        w.enable_body(self.body, false);
        self.alive = false;
        // A walker with a death animation plays it out before it is gone.
        self.dying = self.dying_span;
        self.visible = self.dying > 0.0;
        if self.flying {
            self.flight.state = Some(crate::flight::FlightState::Destroyed);
        }
        // Its pad's respawn clock starts.
        w.respawn_clock(self.index);
        w.current(self);
        w.destroyed_crew(self.index);
        w.reload(self);
        w.current(self);
        w.destroyed(self.index);
        w.reload(self);
        // Its own tow cable lets go with it (releaseTow 'destroyed': no sound).
        if self.tow_target >= 0.0 {
            self.tow_target = -1.0;
            self.tow_winding = 0.0;
            self.tow_leg.clear();
        }
        self.driver = false;
        w.release_crew(self.index);
        for seat in &mut self.seats {
            seat.lease = None;
        }
        self.velocity.set(0.0, 0.0, 0.0);
        let at = match self.explosion_offset {
            Some(o) => {
                let mut at = Vector3::new(o[0], o[1], o[2]);
                apply_axis_angle(&mut at, &UP, self.yaw);
                at.add(&self.position);
                at
            }
            None => self.position,
        };
        if self.has_explosion {
            w.current(self);
            w.explosion(self.index, false, &at);
            w.reload(self);
        } else if !self.creature {
            w.effect("explosion", &at);
        }
    }
}

/// vehicles.js holdYaw: a yaw held inside a seat's limited arc, measured
/// from the hull's heading (`arcYaw` folded in).
pub fn hold_yaw(hull_yaw: f64, limits: [f64; 2], wrap_yaw: bool, yaw: f64) -> f64 {
    let a = angle(yaw - hull_yaw);
    let relative = if wrap_yaw && a < 0.0 { a + 2.0 * std::f64::consts::PI } else { a };
    hull_yaw + clamp(relative, limits[0], limits[1])
}

/// three's Vector3.applyAxisAngle: applyQuaternion of setFromAxisAngle.
pub fn apply_axis_angle(v: &mut Vector3, axis: &Vector3, angle: f64) {
    let mut q = Quaternion::default();
    q.set_from_axis_angle(axis, angle);
    v.apply_quaternion(&q);
}
