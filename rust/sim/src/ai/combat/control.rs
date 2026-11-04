//! The control core's current requests and alerts as combat reads them.
//! Both packages keep their state in the same battle: feeding requests
//! must preserve combat's perception, which outlives a service call.

use super::world::{Ctl, Curves, Obj, Ref};
use crate::ai::control::world::{AlertSource, AlertState, CombatInput, CombatWrites, EntRef, UKey};
use crate::ai::nav::math::Vec3;
use three::Vector3;

/// The host's object identities. Unit, Character and post indices are
/// separate tables; combat compares their mapped identities with `===`.
pub trait Objects {
    fn unit(&self, key: UKey) -> Obj;
    fn character(&self, key: usize) -> Obj;
    fn post(&self, key: usize) -> Obj;
    fn unit_key(&self, obj: Obj) -> UKey;
    fn alert_source(&self, obj: Obj) -> AlertSource;

    fn source(&self, key: AlertSource) -> Obj {
        match key {
            AlertSource::Character(key) => self.character(key),
            AlertSource::Unit(key) => self.unit(key),
        }
    }

    fn entity_ref(&self, r: EntRef) -> Ref {
        Ref { obj: self.unit(r.unit), generation: r.generation }
    }

    fn control_ref(&self, r: Ref) -> EntRef {
        EntRef { unit: self.unit_key(r.obj), generation: r.generation }
    }
}

fn vec(v: Vec3) -> Vector3 { Vector3 { x: v.x, y: v.y, z: v.z } }
fn point(v: Vector3) -> Vec3 { Vec3 { x: v.x, y: v.y, z: v.z } }

impl Ctl {
    /// Apply only the fields owned by control. Perception is created at
    /// bind, and its heap, fire state and aim filters remain untouched.
    pub fn feed_control(&mut self, input: &CombatInput<'_>, ids: &impl Objects) {
        self.unit = input.unit.map(|u| ids.unit(u));
        self.generation = input.generation;
        self.character = Some(ids.character(input.character));
        self.objective = input.objective.map(|p| ids.post(p));
        self.target = input.target.map(|r| ids.entity_ref(r));
        self.fire_a = input.fire_a;
        self.fire_b = input.fire_b;
        self.roll = input.roll;
        self.range_scale = Some(input.range_scale);
        self.aim_state = input.aim_state;
        self.aim_point = vec(input.aim_point);
        self.aim_target = input.aim_target.map(|r| ids.entity_ref(r));
        self.stance = input.stance;
        self.nav_stance = input.nav_stance;
        self.facing = input.facing;
        self.wanted_mode = input.wanted_mode;
        self.dodge = input.dodge;
        self.alert_anim = input.alert_anim;
        self.shot.pending = input.shot_pending;
        self.shot.known = input.shot_known;
        self.shot.point = vec(input.shot_point);
        self.shot.source = input.shot_source.map(|s| ids.source(s));
        self.danger.pending = input.danger_pending;
        self.danger.point = vec(input.danger_point);
        self.acoustic.clear();
        self.acoustic.extend(input.acoustic.iter().map(|&r| ids.entity_ref(r)));
        self.difficulty = input.difficulty.map(|d| Curves { a: d.a, d: d.d, b: Some(d.b), s: Some(d.s) });
        self.jet.pending = input.jet_pending;
        self.jet.vector = vec(input.jet_vector);
    }

    /// The writes the core adopts after combat's target/aim/fire service.
    pub fn control_writes(&self, ids: &impl Objects) -> CombatWrites {
        CombatWrites {
            fire_a: self.fire_a,
            fire_b: self.fire_b,
            target: self.target.map(|r| ids.control_ref(r)),
            aim_state: self.aim_state,
            aim_point: point(self.aim_point),
            stance: self.stance,
            facing: self.facing,
            wanted_mode: self.wanted_mode,
            dodge: self.dodge,
            alert_anim: self.alert_anim,
        }
    }

    /// Event ingestion can alert controllers other than the caller.
    pub fn control_alerts(&self, controller: usize, ids: &impl Objects) -> AlertState {
        AlertState {
            controller,
            shot_pending: self.shot.pending,
            shot_known: self.shot.known,
            shot_point: point(self.shot.point),
            shot_source: self.shot.source.map(|s| ids.alert_source(s)),
            danger_pending: self.danger.pending,
            danger_point: point(self.danger.point),
            acoustic: self.acoustic.iter().map(|&r| ids.control_ref(r)).collect(),
            dodge: self.dodge,
            alert_anim: self.alert_anim,
        }
    }
}
