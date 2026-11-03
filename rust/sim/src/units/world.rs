//! What the units ask of the rest of the battle: the collision world, the
//! seats and props a rider's or a held body's shapes come from, the
//! registry's other members (hulls', turrets' and buildings' targets), and
//! across a unit's tick its combat, its AI aimer, a network player's recon
//! droid and the Character that plays it.
//!
//! Each method is a call the JavaScript makes, at the point it makes it:
//! the replays serve them from captures (rust/sim/tests/replay_units.rs),
//! and the module will answer them from the subsystems that own what they
//! ask about (in Rust as each is ported, through imports from JavaScript
//! until then). A call that changes the unit (a combat update that kills
//! him, an aimer that turns him) takes it mutably. `id` is the unit's.

use super::shapes::{Bounds, HitShape};
use crate::gen::Unit;
use crate::rig::three::Vector3;

/// What CollisionWorld.cast answers: where along the ray, what it met.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CastHit {
    pub point: Vector3,
    pub normal: Vector3,
    pub distance: f64,
    pub terrain: bool,
}

/// A polygon a sphere gather met (CollisionWorld.gather): its triangles and
/// whether its body moves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GatherPolygon {
    pub triangles: Vec<[Vector3; 3]>,
    pub moving: bool,
}

/// The units' boundary.
pub trait UnitsWorld {
    // ---- The collision world (src/collision.js).

    /// cast(origin, direction, far, hit): the first hit within `far`.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit>;
    /// gather(center, radius, false, cache): the polygons near a sphere,
    /// through the body's gather cache `cache` (a droideka's base 0, its
    /// ball 1).
    fn gather(&mut self, id: u32, center: &Vector3, radius: f64, cache: usize, out: &mut Vec<GatherPolygon>);

    // ---- Hit shapes on a seat or in a creature's hold.

    /// unit.seat.exposed (SeatLease, src/seats.js): whether rounds meet him
    /// on his seat.
    fn seat_exposed(&mut self, id: u32, unit: &Unit) -> bool;
    /// unit.seat.riderShapes(unit, shapes): his body as the seat poses it;
    /// the count written.
    fn rider_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize;
    /// unit.held.prop.heldShapes(unit, shapes): his body as a creature holds
    /// it; the count written.
    fn held_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize;

    // ---- The registry's other members (src/seats.js VehicleTarget and
    // TurretTarget, src/objectives.js ObjectiveTarget), by registry id.

    /// `alive` (its hull's, turret's or building's).
    fn target_alive(&mut self, id: u32) -> bool;
    /// `dead`.
    fn target_dead(&mut self, id: u32) -> bool;
    /// `sphere`: its hull's, turret's or building's bounding sphere now.
    fn target_sphere(&mut self, id: u32) -> Bounds;

    // ---- Across a unit's tick (src/units.js Unit.tick).

    /// soldier.tick(input, h): a soldier's movement (src/soldier.js, the
    /// module's `movement`), from the unit's `input`.
    fn move_soldier(&mut self, id: u32, unit: &mut Unit, h: f64);
    /// combat.damage(amount) (takeImpacts): what may kill him.
    fn damage(&mut self, id: u32, unit: &mut Unit, amount: f64);
    /// onMoved(h): a bot's aimer (ai.aim), between moving and firing.
    fn aim(&mut self, id: u32, unit: &mut Unit, h: f64);
    /// Whether a network player's recon droid steps with him (`remoteStep`).
    fn has_remote_step(&mut self, id: u32, unit: &Unit) -> bool;
    /// remoteStep(h, intent or null, controls); `active` when it flies (the
    /// intent is passed).
    fn remote_step(&mut self, id: u32, unit: &mut Unit, h: f64, active: bool);
    /// combat.reset() (Unit.spawn): his weapons and health for a new life.
    fn combat_reset(&mut self, id: u32, unit: &mut Unit);
    /// combat.update(h, controls, stick): his weapons.
    fn combat_update(&mut self, id: u32, unit: &mut Unit, h: f64, stick: f64);
    /// combat.flushMuzzles().
    fn flush_muzzles(&mut self, id: u32, unit: &mut Unit);
    /// combat.clearCharge() (undraw).
    fn clear_charge(&mut self, id: u32, unit: &mut Unit);
    /// A droideka's blast during its death clip (onDeathBlast: the game's
    /// deathBlast hook).
    fn death_blast(&mut self, id: u32, unit: &mut Unit);
    /// A droideka gone up at the end of its death clip (onExploded:
    /// combat.onBlowApart(0), the game's onBlowApart hook).
    fn blow_apart(&mut self, id: u32, unit: &mut Unit, health: f64);
    /// remove(): `if (character.unit === this) character.unit = null` for
    /// the Character he played (the battle's).
    fn let_go_of_character(&mut self, id: u32, unit: &mut Unit);
    /// remove()'s onRemoved: the factory takes the body back into its
    /// class's pool (src/units.js UnitFactory.release).
    fn release(&mut self, id: u32, unit: &Unit);
}
