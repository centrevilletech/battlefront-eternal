//! A unit (src/units.js Unit): one body of one class on the field, its
//! life, the intent its controller writes, the controls that latch it, the
//! hit shapes made from its body, and its tick. The unit is the store's
//! (`crate::gen::Unit`, rust/sim/schema/units.mjs); its id is its slot.

use super::damage::take_impacts;
use super::droideka::{self, DroidekaParts};
use super::shapes::{bound_shapes, soldier_shapes, v3, Bounds, Half, HitPosture, HitShape, ShapePart, ShapeType, DROIDEKA_BALL, DROIDEKA_ORD_AT, DROIDEKA_ORD_HALF, DROIDEKA_ORD_RADIUS};
use super::world::UnitsWorld;
use crate::gen::{Controls, Posture, Unit, UnitBody};
use three::Vector3;

/// A body lies 4 s where it fell, then is gone (units.js DEATH_LINGER).
pub const DEATH_LINGER: f64 = 4.0;
/// The most shapes a rider (or a held body) has (src/rider-pose.js).
pub const RIDER_SHAPES: usize = crate::rig::rider::RIDER_SHAPES;

/// `x || 0` for a number: 0, -0 and NaN become 0.
#[inline(always)]
pub fn or_zero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        0.0
    } else {
        x
    }
}

/// Whether a number is truthy (not 0, -0 or NaN).
#[inline(always)]
pub fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

impl Controls {
    /// IntentControls.latch() (units.js:138): a channel's count above zero
    /// is held; the first held count after a release is a press.
    pub fn latch(&mut self, buttons: &[f64; 2]) {
        for c in 0..2 {
            self.pressed[c] = buttons[c] > 0.0 && !(self.was[c] > 0.0);
            self.was[c] = buttons[c];
        }
    }
}

impl Unit {
    /// `hitPosture` (units.js:304): standing in the air, crouched in a dive,
    /// lying down prone or dead. (A droideka's is its posture's.)
    pub fn hit_posture(&self) -> HitPosture {
        let UnitBody::Soldier(s) = &self.soldier else { return HitPosture::Stand };
        if self.dead {
            return HitPosture::Prone;
        }
        if truthy(s.dive) {
            return HitPosture::Crouch;
        }
        if s.air.is_some() {
            return HitPosture::Stand;
        }
        match s.posture {
            Posture::Stand => HitPosture::Stand,
            Posture::Crouch => HitPosture::Crouch,
            Posture::Prone => HitPosture::Prone,
        }
    }

    /// Whether its hit shapes are a rider's (or a held body's) set
    /// (`hitSet === riderHits`): a rider's set has 6 or 10 shapes, its own 1
    /// or 2, and updateSpheres sets the count with the set.
    pub fn rider_set(&self) -> bool {
        self.shape_count > 2.0 && self.rider_hits.is_some()
    }

    /// The set `hitShapes()` answers from.
    pub fn hit_set(&self) -> &[HitShape] {
        if self.rider_set() {
            self.rider_hits.as_deref().unwrap_or(&self.shapes)
        } else {
            &self.shapes
        }
    }

    pub fn position(&self) -> Vector3 {
        match &self.soldier {
            UnitBody::Soldier(s) => v3(s.position),
            UnitBody::Droideka(d) => v3(d.position),
        }
    }

    pub fn velocity(&self) -> Vector3 {
        match &self.soldier {
            UnitBody::Soldier(s) => v3(s.velocity),
            UnitBody::Droideka(d) => v3(d.velocity),
        }
    }

    pub fn aim_yaw(&self) -> f64 {
        match &self.soldier {
            UnitBody::Soldier(s) => s.aim_yaw,
            UnitBody::Droideka(d) => d.aim_yaw,
        }
    }

    pub fn aim_pitch(&self) -> f64 {
        match &self.soldier {
            UnitBody::Soldier(s) => s.aim_pitch,
            UnitBody::Droideka(d) => d.aim_pitch,
        }
    }

    /// Whether its body is a droideka folded, rolled or unfolding.
    pub fn rolled(&self) -> bool {
        matches!(&self.soldier, UnitBody::Droideka(d) if d.mode >= 11.0)
    }

    /// `sphere` (units.js:268): the broadphase sphere about the hit shapes,
    /// for this tick's pose (made on first asking in a tick).
    pub fn sphere(&mut self, id: u32, world: &mut dyn UnitsWorld) -> Bounds {
        if self.spheres_tick != self.tick_count {
            self.update_spheres(id, world);
        }
        self.bounds.clone()
    }

    /// `hitShapes()` (units.js:296): the hit shapes, for this tick's pose.
    pub fn hit_shapes(&mut self, id: u32, world: &mut dyn UnitsWorld) -> &[HitShape] {
        if self.spheres_tick != self.tick_count {
            self.update_spheres(id, world);
        }
        let count = self.shape_count as usize;
        &self.hit_set()[..count]
    }

    /// updateSpheres() (units.js:312).
    pub fn update_spheres(&mut self, id: u32, world: &mut dyn UnitsWorld) {
        // A rider shown in the open (SeatLease exposed): rounds meet his posed
        // body on the seat (src/rider-pose.js). Held by a creature
        // (src/props.js), his body as it hangs from its node.
        let exposed = self.seat.is_some() && world.seat_exposed(id, self);
        let held_frame = matches!(&self.held, crate::store::Nullable::Value(h) if !matches!(h.frame, crate::store::Nullable::Null | crate::store::Nullable::Undefined));
        if exposed || held_frame {
            let mut hits = self.rider_hits.take().unwrap_or_else(|| vec![rider_blank(); RIDER_SHAPES]);
            let count = if exposed { world.rider_shapes(id, self, &mut hits) } else { world.held_shapes(id, self, &mut hits) };
            self.rider_hits = Some(hits);
            if count != 0 {
                self.shape_count = count as f64;
                let hits = self.rider_hits.as_ref().unwrap();
                bound_shapes(hits, count, &mut self.bounds);
                self.spheres_tick = self.tick_count;
                return;
            }
        }
        let posture = self.hit_posture();
        match &self.soldier {
            UnitBody::Droideka(s) => {
                // The ball: p_rollsphere alone. Deployed, p_ord too, the
                // upright cylinder that takes in the head and the guns.
                let p = s.position;
                let rolled = s.mode >= 11.0;
                let r = if rolled { s.ball_radius } else { DROIDEKA_BALL };
                let ball = &mut self.shapes[0];
                ball.r#type = ShapeType::Sphere;
                ball.part = ShapePart::Body;
                ball.center = [p[0], p[1] + r, p[2]];
                ball.radius = r;
                ball.yaw = s.aim_yaw;
                self.shape_count = 1.0;
                if !rolled {
                    let ord = &mut self.shapes[1];
                    let [_, y, z] = DROIDEKA_ORD_AT;
                    ord.r#type = ShapeType::Cylinder;
                    ord.part = ShapePart::Body;
                    ord.center = [p[0] + v8math::sin(s.aim_yaw) * z, p[1] + y, p[2] + v8math::cos(s.aim_yaw) * z];
                    ord.radius = DROIDEKA_ORD_RADIUS;
                    ord.half = Half::F64(DROIDEKA_ORD_HALF);
                    ord.yaw = s.aim_yaw;
                    self.shape_count = 2.0;
                }
                let count = self.shape_count as usize;
                bound_shapes(&self.shapes, count, &mut self.bounds);
            }
            UnitBody::Soldier(s) => {
                let p = s.position;
                let sin = v8math::sin(s.aim_yaw);
                let cos = v8math::cos(s.aim_yaw);
                let parts = soldier_shapes(posture, self.root_scale);
                for (i, part) in parts.iter().enumerate() {
                    let shape = &mut self.shapes[i];
                    let [x, y, z] = part.at;
                    // The soldier's left is (cos yaw, 0, -sin yaw), forward (sin yaw, 0, cos yaw).
                    shape.center = [p[0] + cos * x + sin * z, p[1] + y, p[2] - sin * x + cos * z];
                    shape.r#type = part.kind;
                    shape.part = part.part;
                    shape.radius = or_zero(part.radius);
                    shape.half = if part.kind == ShapeType::Box {
                        // `half` becomes halfSize itself.
                        shape.half_size = part.half_box;
                        Half::Vec3(shape.half_size)
                    } else {
                        Half::F64(or_zero(part.half))
                    };
                    shape.yaw = s.aim_yaw;
                }
                self.shape_count = parts.len() as f64;
                bound_shapes(&self.shapes, parts.len(), &mut self.bounds);
            }
        }
        self.spheres_tick = self.tick_count;
    }

    /// tick(h) (units.js:515): the intent through the body's own movement,
    /// then its weapons. `droideka`: a droideka's class, clips and gun rig.
    pub fn tick(&mut self, id: u32, h: f64, world: &mut dyn UnitsWorld, droideka: Option<&mut DroidekaParts>) {
        if self.removed {
            return;
        }
        self.tick_count += 1.0;
        // The occupied control consumes its intent and runs the vehicle
        // physics: no edges consumed, the hidden soldier not stepped twice.
        if self.seat.is_some() && self.alive {
            return;
        }
        // A creature holds him: it moves him, and nothing he does counts
        // until it lets go; the dead body it carries waits too.
        if matches!(self.held, crate::store::Nullable::Value(_)) {
            return;
        }
        let remote_active = self.remotes_active == Some(true);
        let intent = &mut self.intent;
        let input = &mut self.input;
        if self.alive && !remote_active {
            input.forward = intent.forward;
            input.right = intent.right;
            input.turn = intent.turn;
            input.pitch = intent.pitch;
            input.jump = intent.jump;
            input.crouch = intent.crouch;
            input.prone = intent.prone;
            input.r#use = intent.r#use;
            input.jet_direction = intent.jet_direction;
        } else {
            input.forward = 0.0;
            input.right = 0.0;
            input.turn = 0.0;
            input.pitch = 0.0;
            input.jump = false;
            input.crouch = false;
            input.prone = false;
            input.r#use = false;
        }
        intent.jump = false;
        intent.crouch = false;
        intent.prone = false;
        if !remote_active {
            intent.r#use = false;
        }
        // (presentation.beforeStep: a page's, and a world's previousYaw
        // nothing reads.)
        let mut parts = droideka;
        match (&self.soldier, parts.as_deref_mut()) {
            (UnitBody::Droideka(_), Some(p)) => droideka::tick(id, self, h, world, p),
            _ => world.move_soldier(id, self, h),
        }
        // What hitting the ground or a wall too hard did to him, or a fall
        // past saving (src/collision-damage.js).
        take_impacts(id, self, world);
        // The aimer runs between moving and firing (a bot's Classic Aimer).
        if self.alive {
            world.aim(id, self, h);
        }
        self.aim_override = if self.alive { opt_nullable(self.intent.aim) } else { crate::store::Nullable::Null };
        let buttons = self.intent.buttons;
        self.controls.latch(&buttons);
        if world.has_remote_step(id, self) {
            world.remote_step(id, self, h, remote_active);
        }
        if let (UnitBody::Droideka(_), Some(p)) = (&self.soldier, parts.as_deref_mut()) {
            droideka::sample_muzzles(self, h, p);
        }
        let stick = v8math::max(self.input.forward.abs(), self.input.right.abs());
        world.combat_update(id, self, h, stick);
        world.flush_muzzles(id, self);
        if self.dead {
            self.dead_time += h;
            if self.dead_time >= DEATH_LINGER {
                self.remove(id, world);
            }
        }
    }

    /// spawn(x, y, z, yaw) (units.js:375): into the world at (x, y, z)
    /// facing yaw, fresh, a life later.
    pub fn spawn(&mut self, id: u32, x: f64, y: f64, z: f64, yaw: f64, world: &mut dyn UnitsWorld) {
        self.seat = None;
        self.let_go();
        self.targetable = Some(true);
        self.mounted = Some(false);
        self.generation += 1.0;
        match &mut self.soldier {
            UnitBody::Soldier(s) => {
                s.dead = Some(false);
                s.spawn(x, y, z, yaw, world);
            }
            UnitBody::Droideka(d) => {
                d.dead = false;
                d.spawn_place(x, y, z, yaw, world);
                // enter(0): mode 0 from its start (no clip read: it does not
                // play in reverse).
                let was = d.mode;
                d.mode = 0.0;
                d.reverse = false;
                d.time = 0.0;
                if was == 12.0 {
                    d.velocity = [0.0; 3];
                    d.legs = 0.0;
                }
                d.previous = d.position;
                d.settled = d.position;
                // (The drawn animator's reset is the page's.) The hidden gun
                // rig's animator: facing the new yaw, its mode unknown.
                let roll = d.roll;
                if let Some(a) = &mut d.muzzle_animator {
                    a.facing = yaw;
                    a.mode = -1.0;
                    a.went_up_rolled = false;
                    a.transition = 0.0;
                    a.roll_origin = roll;
                }
            }
        }
        world.combat_reset(id, self);
        // (presentation.reset, the drawn animator's reset and the root's
        // place are the page's.)
        self.alive = true;
        self.dead = false;
        self.dead_time = 0.0;
        self.removed = false;
        self.intent = blank_intent();
        self.controls.was = [0.0, 0.0];
        self.tick_count += 1.0;
        self.update_spheres(id, world);
    }

    /// remove() (units.js:425): off the field (after a death, or between
    /// lives); its Character lets go of it and the body goes back to the
    /// pool for anyone's next spawn.
    pub fn remove(&mut self, id: u32, world: &mut dyn UnitsWorld) {
        self.alive = false;
        self.removed = true;
        self.let_go();
        world.let_go_of_character(id, self);
        self.character = None;
        // undraw(): the body, a droideka's shield and a jet flame are the
        // page's; a weapon's charge glow is combat's.
        world.clear_charge(id, self);
        world.release(id, self);
    }

    /// letGo() (units.js:437): out of a creature's hold.
    pub fn let_go(&mut self) {
        if !matches!(self.held, crate::store::Nullable::Value(_)) {
            return;
        }
        self.held = crate::store::Nullable::Null;
        self.mounted = Some(false);
        // (The animator's gunner clip and the root's tilt are the page's.)
    }

    /// origin(out) (units.js:279) on foot: the aim pivot, else the eyes'
    /// height over his feet by posture. (A seated unit's is his seat's.)
    pub fn origin_on_foot(&self) -> Vector3 {
        let pivot = v3(self.pivot);
        if pivot.length_sq() > 0.0 {
            return pivot;
        }
        let p = self.position();
        let rise = match &self.soldier {
            UnitBody::Soldier(s) => match s.posture {
                Posture::Prone => 0.6,
                Posture::Crouch => 1.2,
                Posture::Stand => 1.7,
            },
            // A droideka's posture: "crouch" folded, rolled or unfolding.
            UnitBody::Droideka(d) => {
                if d.mode >= 11.0 {
                    1.2
                } else {
                    1.7
                }
            }
        };
        Vector3 { x: p.x, y: p.y + rise, z: p.z }
    }

    /// forward(out) (units.js:287) on foot: the aim direction, unit length.
    pub fn forward_on_foot(&self) -> Vector3 {
        let (yaw, pitch) = (self.aim_yaw(), self.aim_pitch());
        Vector3 { x: v8math::sin(yaw) * v8math::cos(pitch), y: v8math::sin(pitch), z: v8math::cos(yaw) * v8math::cos(pitch) }
    }
}

/// blankIntent() (units.js:114).
pub fn blank_intent() -> crate::gen::Intent {
    crate::gen::Intent {
        forward: 0.0,
        right: 0.0,
        turn: 0.0,
        pitch: 0.0,
        jump: false,
        crouch: false,
        prone: false,
        r#use: false,
        buttons: [0.0, 0.0],
        reload: false,
        next_primary: false,
        previous_primary: false,
        next_secondary: false,
        zoom: false,
        aim: None,
        jet_direction: None,
        exit: false,
        brake: None,
        up: None,
        down: None,
    }
}

/// A rider's shape as units.js makes riderHits' (a sphere at the origin).
fn rider_blank() -> HitShape {
    HitShape { r#type: ShapeType::Sphere, part: ShapePart::Body, center: [0.0; 3], radius: 0.0, half: Half::F64(0.0), yaw: 0.0, half_size: [0.0; 3] }
}

/// The intent's `aim` (a vector or null) as combat's aimOverride.
fn opt_nullable(v: Option<[f64; 3]>) -> crate::store::Nullable<[f64; 3]> {
    match v {
        Some(v) => crate::store::Nullable::Value(v),
        None => crate::store::Nullable::Null,
    }
}
