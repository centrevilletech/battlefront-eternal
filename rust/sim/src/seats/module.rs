//! The seat phase on the module, behind its switch (src/seats-sim.js; off
//! unless a world asks for it with ?seats=rust, or BFE_SEATS=rust under
//! Node): the hulls' input batch (Vehicles.latchCrews) runs on the leases,
//! hulls and riders' units as the store holds them, JavaScript writing them
//! in before the phase and syncing its objects from the store after it.
//!
//! What the seats ask of the battle meanwhile comes back as the imports of
//! rust/sim/schema/seats.mjs (the collision world's casts, an AI
//! controller's state, a bot's new seat control, a seat's weapons' spread
//! rule). None of them comes back into the module: the seats' Rust holds
//! the store across them. What the seats leave to the battle's JavaScript
//! (a body's combat flags and drawing as it takes or leaves a seat, the
//! battle's hooks) is the answer's effects, which JavaScript does in order
//! once the phase is over and its objects are synced: nothing in the phase
//! reads any of it back.

use super::hulls::{HullSide, Hulls};
use super::lease::{control_key, SeatControl, SeatIntent};
use super::{a3, v3, Attacker, Hit, Holder, Reason, SeatWorld, Seating};
use crate::abi::Sim;
use crate::gen::{
    Body, Post, SeatAiIntent, SeatEffect, SeatEffectKind, SeatHullFacts, SeatLease, SeatLeaseArgs, SeatPhaseAnswer, SeatPhaseArgs, SeatPolicyArgs, SeatRay, SeatReason, SeatRole, SeatSphere, SeatUnit, SeatWho,
    Store, Unit, UnitBody,
};
use crate::store::Ref;
use crate::units::shapes::{Bounds, HitShape};
use crate::units::world::{CastHit, GatherPolygon, UnitsWorld};
use three::Vector3;

#[cfg(target_arch = "wasm32")]
use crate::gen::{import_seat_cast, import_seat_controllable, import_seat_controller_state, import_seat_overlaps, import_seat_policy};

/// Natively there is no JavaScript to ask: the phase runs in the module
/// only (the replays serve the seats' boundary themselves).
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use crate::gen::{SeatFlag, SeatHit, SeatLeaseArgs, SeatMade, SeatNumber, SeatPolicyArgs, SeatRay, SeatSphere, SeatWho};
    pub fn import_seat_cast(_: &SeatRay) -> SeatHit {
        unreachable!("the seat phase's imports are the module's")
    }
    pub fn import_seat_overlaps(_: &SeatSphere) -> SeatFlag {
        unreachable!("the seat phase's imports are the module's")
    }
    pub fn import_seat_controller_state(_: &SeatWho) -> SeatNumber {
        unreachable!("the seat phase's imports are the module's")
    }
    pub fn import_seat_controllable(_: &SeatLeaseArgs) -> SeatMade {
        unreachable!("the seat phase's imports are the module's")
    }
    pub fn import_seat_policy(_: &SeatPolicyArgs) {
        unreachable!("the seat phase's imports are the module's")
    }
}
#[cfg(not(target_arch = "wasm32"))]
use native::*;

/// seats.js placeOnFoot's Soldier.spawn boundary, shared by the live owner.
#[cfg(target_arch = "wasm32")]
pub(crate) fn stand_up(unit: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
    match &mut unit.soldier {
        UnitBody::Soldier(s) => s.spawn(x, y, z, yaw, &mut Ground),
        UnitBody::Droideka(_) => unreachable!("a droideka never sits"),
    }
}

/// Vehicles.latchCrews on the store (the command seatLatch): every hull's
/// stale leases let go, every bot's intent frozen for this update, and the
/// exits asked for carried out.
pub fn latch(sim: &mut Sim, args: SeatPhaseArgs) -> SeatPhaseAnswer {
    let mut hulls = Hulls {
        time: args.time,
        ticks: args.ticks,
        hulls: args.hulls.iter().map(hull_side).collect(),
        controls: args.controls.iter().map(|c| (control_key(c.seat_unit), SeatControl { driven: c.driven, intent: seat_intent(&c.intent) })).collect(),
        ..Hulls::default()
    };
    let mut world = JsSeats::default();
    hulls.latch_crews(&mut sim.store, &mut world);
    SeatPhaseAnswer { ticks: hulls.ticks, latched: hulls.latched, effects: world.effects }
}

/// A hull's facts as its seats read them (attacker 1 the player, 2 a unit
/// by its id, 3 a hull by its place, else nobody).
fn hull_side(f: &SeatHullFacts) -> HullSide {
    HullSide {
        index: f.index as usize,
        vehicle: f.vehicle,
        last_attacker: match f.attacker as u32 {
            1 => Attacker::Player,
            2 => Attacker::Unit(f.attacker_id as u32),
            3 => Attacker::Hull(f.attacker_id as usize),
            _ => Attacker::Nobody,
        },
        hover: f.hover,
        animal: f.animal,
        turret_nodes: f.turret_nodes.clone(),
        weapons: f.weapons.iter().map(|&n| n as usize).collect(),
    }
}

fn seat_intent(i: &SeatAiIntent) -> SeatIntent {
    SeatIntent {
        forward: i.forward,
        right: i.right,
        turn: i.turn,
        pitch: i.pitch,
        jump: i.jump,
        crouch: i.crouch,
        prone: i.prone,
        r#use: i.r#use,
        exit: i.exit,
        buttons: i.buttons,
        reload: i.reload,
        next_primary: i.next_primary,
        previous_primary: i.previous_primary,
        next_secondary: i.next_secondary,
        zoom: i.zoom,
        aim: i.aim.map(|a| v3(&a)),
        brake: i.brake,
        up: i.up,
        down: i.down,
        trick: i.trick,
        immelmann: i.immelmann,
        boost: i.boost,
    }
}

fn reason(r: Reason) -> SeatReason {
    match r {
        Reason::Exit => SeatReason::Exit,
        Reason::Stale => SeatReason::Stale,
        Reason::Killed => SeatReason::Killed,
        Reason::Destroyed => SeatReason::Destroyed,
        Reason::Displaced => SeatReason::Displaced,
    }
}

/// A hull's place in the battle's list (the turret crews are not on the
/// module).
fn hull_index(holder: Holder) -> f64 {
    match holder {
        Holder::Hull(i) => i as f64,
        Holder::Turret(_) => unreachable!("the turret crews are not on the module"),
    }
}

/// The battle as the phase's imports give it, and the effects it leaves.
#[derive(Default)]
struct JsSeats {
    effects: Vec<SeatEffect>,
}

impl JsSeats {
    fn effect(&mut self, kind: SeatEffectKind, unit: Option<u32>, hull: f64, seat: usize, yaw: f64, at: [f64; 3], post: Option<Ref<Post>>, why: SeatReason) {
        let unit = unit.map_or(-1.0, |u| u as f64);
        self.effects.push(SeatEffect { kind, unit, hull, seat: seat as f64, yaw, at, post, reason: why });
    }

    fn body(&mut self, kind: SeatEffectKind, unit: u32, yaw: f64, at: [f64; 3]) {
        self.effect(kind, Some(unit), -1.0, 0, yaw, at, None, SeatReason::Exit);
    }
}

impl SeatWorld for JsSeats {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<Hit> {
        let h = import_seat_cast(&SeatRay { origin: a3(origin), direction: a3(direction), far, ignore });
        h.found.then(|| Hit { distance: h.distance, point: v3(&h.point), normal: v3(&h.normal), body: h.body })
    }

    fn overlaps_sphere(&mut self, center: &Vector3, radius: f64, ignore: Option<Ref<Body>>) -> bool {
        import_seat_overlaps(&SeatSphere { center: a3(center), radius, ignore }).value
    }

    fn raycast_units(&mut self, _origin: &Vector3, _direction: &Vector3, _far: f64, _exclude: Option<u32>) -> Option<f64> {
        unreachable!("the hulls' input batch looks for no unit along a ray")
    }

    fn relation(&mut self, _a: f64, _b: f64) -> f64 {
        unreachable!("the hulls' input batch boards no one")
    }

    fn stand_up(&mut self, unit: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
        match &mut unit.soldier {
            UnitBody::Soldier(s) => s.spawn(x, y, z, yaw, &mut Ground),
            UnitBody::Droideka(_) => unreachable!("a droideka never sits"),
        }
    }

    fn kill(&mut self, _st: &mut Store, _seating: Seating<'_>, _unit: u32, _attacker: Attacker) {
        unreachable!("the hulls' input batch kills no one")
    }

    fn rider_drop(&mut self, _holder: Holder, _index: usize, _unit: u32) -> Option<Vector3> {
        unreachable!("the hulls' input batch kills no one")
    }

    fn sync_rider(&mut self, _lease: Ref<SeatLease>, _unit: &mut Unit) {
        unreachable!("the hulls' input batch boards no one")
    }

    fn controller_state(&mut self, unit: Option<u32>) -> Option<f64> {
        import_seat_controller_state(&SeatWho { unit: unit.map_or(-1.0, |u| u as f64) }).value
    }

    fn controllable(&mut self, lease: Ref<SeatLease>, index: usize, role: SeatRole) -> Ref<SeatUnit> {
        import_seat_controllable(&SeatLeaseArgs { lease, index: index as f64, role }).seat_unit
    }

    fn boarded(&mut self, unit: u32, holder: Holder, index: usize) {
        self.effect(SeatEffectKind::Boarded, Some(unit), hull_index(holder), index, 0.0, [0.0; 3], None, SeatReason::Exit);
    }

    fn unboarded(&mut self, unit: Option<u32>, holder: Holder, index: usize, post: Option<Ref<Post>>, why: Reason) {
        self.effect(SeatEffectKind::Unboarded, unit, hull_index(holder), index, 0.0, [0.0; 3], post, reason(why));
    }

    fn moved(&mut self, unit: Option<u32>, holder: Holder, index: usize) {
        self.effect(SeatEffectKind::Moved, unit, hull_index(holder), index, 0.0, [0.0; 3], None, SeatReason::Exit);
    }

    fn seated(&mut self, unit: u32) {
        self.body(SeatEffectKind::Seated, unit, 0.0, [0.0; 3]);
    }

    fn unseated(&mut self, unit: u32, yaw: f64) {
        self.body(SeatEffectKind::Unseated, unit, yaw, [0.0; 3]);
    }

    fn placed(&mut self, unit: u32, yaw: f64) {
        self.body(SeatEffectKind::Placed, unit, yaw, [0.0; 3]);
    }

    fn dropped(&mut self, unit: u32, at: &Vector3) {
        self.body(SeatEffectKind::Dropped, unit, 0.0, a3(at));
    }

    fn seat_policy(&mut self, holder: Holder, index: usize, player: bool, random: bool) {
        import_seat_policy(&SeatPolicyArgs { hull: hull_index(holder), seat: index as f64, player, random });
    }
}

/// What Soldier.spawn asks of the units' world: the cast down under the
/// point it stands up at (the collision world's, through the seats' cast).
struct Ground;

impl UnitsWorld for Ground {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        crate::vehicles::module::collision_world().expect("the seat collision world")
            .cast(crate::collision::state(), origin, direction, far, None).expect("named collision bodies")
            .map(|h| CastHit { point: h.point, normal: h.normal, distance: h.distance, terrain: h.terrain })
    }
    fn gather(&mut self, _id: u32, _center: &Vector3, _radius: f64, _cache: usize, _out: &mut Vec<GatherPolygon>) {
        unreachable!("a spawn gathers nothing")
    }
    fn seat_exposed(&mut self, _id: u32, _unit: &Unit) -> bool {
        unreachable!("a spawn asks only for the ground")
    }
    fn rider_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        unreachable!("a spawn asks only for the ground")
    }
    fn held_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        unreachable!("a spawn asks only for the ground")
    }
    fn target_alive(&mut self, _id: u32) -> bool {
        unreachable!("a spawn asks only for the ground")
    }
    fn target_dead(&mut self, _id: u32) -> bool {
        unreachable!("a spawn asks only for the ground")
    }
    fn target_sphere(&mut self, _id: u32) -> Bounds {
        unreachable!("a spawn asks only for the ground")
    }
    fn move_soldier(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {
        unreachable!("a spawn asks only for the ground")
    }
    fn damage(&mut self, _id: u32, _unit: &mut Unit, _amount: f64) {
        unreachable!("a spawn asks only for the ground")
    }
    fn aim(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {
        unreachable!("a spawn asks only for the ground")
    }
    fn has_remote_step(&mut self, _id: u32, _unit: &Unit) -> bool {
        unreachable!("a spawn asks only for the ground")
    }
    fn remote_step(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _active: bool) {
        unreachable!("a spawn asks only for the ground")
    }
    fn combat_reset(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a spawn asks only for the ground")
    }
    fn combat_update(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _stick: f64) {
        unreachable!("a spawn asks only for the ground")
    }
    fn flush_muzzles(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a spawn asks only for the ground")
    }
    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a spawn asks only for the ground")
    }
    fn death_blast(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a spawn asks only for the ground")
    }
    fn blow_apart(&mut self, _id: u32, _unit: &mut Unit, _health: f64) {
        unreachable!("a spawn asks only for the ground")
    }
    fn let_go_of_character(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a spawn asks only for the ground")
    }
    fn release(&mut self, _id: u32, _unit: &Unit) {
        unreachable!("a spawn asks only for the ground")
    }
}
