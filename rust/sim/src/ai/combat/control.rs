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
    #[inline(always)]
    pub fn feed_control(&mut self, input: &CombatInput<'_>, ids: &impl Objects) {
        self.character=Some(ids.character(input.source.character));
        self.feed_requests(input,ids);
    }
    /// Native controllers bind Character identity once. Repeated requests
    /// keep that qualified object rather than resolving the roster again.
    pub fn feed_requests(&mut self,input:&CombatInput<'_>,ids:&impl Objects) {
        let c=input.source;
        self.controller_state=c.fsm.state;
        self.unit = c.unit.map(|u| ids.unit(u));
        self.generation = c.generation;
        self.objective = input.objective.map(|p| ids.post(p));
        self.target = c.target.map(|r| ids.entity_ref(r));
        self.fire_a = c.fire_a;
        self.fire_b = c.fire_b;
        self.roll = c.roll;
        self.range_scale = Some(c.range_scale);
        self.aim_state = c.aim_state;
        self.aim_point = vec(c.aim_point);
        self.aim_target = c.aim_target.map(|r| ids.entity_ref(r));
        self.stance = c.stance;
        self.nav_stance = input.nav_stance;
        self.facing = c.facing;
        self.wanted_mode = c.wanted_mode;
        self.dodge = c.dodge;
        self.alert_anim = c.alert_anim;
        self.shot.pending = c.shot_pending;
        self.shot.known = c.shot_known;
        self.shot.point = vec(c.shot_point);
        self.shot.source = c.shot_source.map(|s| ids.source(s));
        self.danger.pending = c.danger_pending;
        self.danger.point = vec(c.danger_point);
        self.acoustic.clear();
        self.acoustic.extend(c.acoustic.iter().map(|&r| ids.entity_ref(r)));
        self.difficulty = c.difficulty.map(|d| Curves { a: d.a, d: d.d, b: Some(d.b), s: Some(d.s) });
        self.jet.pending = c.nav_control.as_ref().map_or(c.jet_pending,|n|n.jet_pending);
        self.jet.vector = vec(c.nav_control.as_ref().map_or(c.jet_vector,|n|n.jet_vector));
    }

    /// selectedWeapon, ranges and firingCone inspect only the current owner
    /// and range scale. Their pure query cannot consume alerts or requests.
    pub fn feed_query(&mut self,c:&crate::ai::control::controller::Controller,ids:&impl Objects) {
        self.unit=c.unit.map(|u|ids.unit(u));self.generation=c.generation;
        self.range_scale=Some(c.range_scale);
    }
    /// aimPointFor additionally observes the desired target and point. It
    /// writes no controller fields; tracker and aimer storage stays retained.
    pub fn feed_aim_query(&mut self,c:&crate::ai::control::controller::Controller,ids:&impl Objects) {
        self.feed_query(c,ids);self.aim_state=c.aim_state;self.aim_point=vec(c.aim_point);
        self.aim_target=c.aim_target.map(|r|ids.entity_ref(r));self.target=c.target.map(|r|ids.entity_ref(r));
    }

    /// Post-movement aim reads current difficulty and target, while Character
    /// identity and its Enhanced skill remain bound for the controller's life.
    pub fn feed_aimer(&mut self,c:&crate::ai::control::controller::Controller,ids:&impl Objects) {
        self.feed_aim_query(c,ids);
        self.difficulty=c.difficulty.map(|d|Curves {a:d.a,d:d.d,b:Some(d.b),s:Some(d.s)});
    }

    /// consumeFire reads current targeting and fire requests, and returns
    /// only those requests. Alert queues and helper policy are not consumed.
    pub fn feed_fire(&mut self,c:&crate::ai::control::controller::Controller,ids:&impl Objects) {
        self.feed_aimer(c,ids);self.fire_a=c.fire_a;self.fire_b=c.fire_b;
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
