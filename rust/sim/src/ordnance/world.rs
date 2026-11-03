//! What the rounds and items ask of the rest of the battle: the trait their
//! Rust reaches it through, method for method the boundary object of
//! src/ordnance-world.js (OrdnanceWorld and ItemsWorld), called at the same
//! points in the same order. In a battle the module's imports implement it
//! (JavaScript answers); in a replay the capture does (rust/sim-replay's
//! `Log`), checking every argument.
//!
//! References name what the store only refers to (a unit by its id, a hull,
//! a collision body); vectors cross as values, and where JavaScript keeps
//! the very vector it was handed (a unit's damage event keeps where the hit
//! came from) the argument says which vector it is ([`Point`]).

use super::defs::Field;
use crate::gen::{Beam, BeamSource, Body, Bolt, Grenade, HitPart, Impact, ItemOwner, OrdnanceOwner, Round, Unit, Vehicle};
use crate::store::{DefId, Nullable, Ref, RefSet, StrId, Vec3};

/// Which of the ordnance's hit records a cast fills (its `hit` or `behind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    Hit,
    Behind,
}

/// A cast's answer (src/collision.js cast, castMoving): how far along the
/// ray, where, the surface's normal facing back, whether the terrain was
/// hit, and the body hit (null for the terrain and the world's buildings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub distance: f64,
    pub point: Vec3,
    pub normal: Vec3,
    pub terrain: bool,
    pub body: Option<Ref<Body>>,
}

/// A unit raycast's answer (src/registry.js raycast).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitHit {
    pub unit: Ref<Unit>,
    pub distance: f64,
    pub point: Vec3,
    pub normal: Vec3,
    pub part: Nullable<HitPart>,
}

/// A hull whose riders a round may strike through it (riderMount): its
/// bounding radius as the hull gives it (NaN for none) and its body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiderMount {
    pub vehicle: Ref<Vehicle>,
    pub bound_radius: f64,
    pub body: Nullable<Ref<Body>>,
}

/// Whom a bolt is credited to on the wire (shooter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shooter {
    pub fire_id: f64,
    pub unit: f64,
    pub generation: f64,
}

/// A target of the items (watch): its side toward the item's team and,
/// unless that is 0, where it is, its size and its velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Watched {
    pub relation: f64,
    pub center: Vec3,
    pub radius: f64,
    pub velocity: Vec3,
}

/// An item's model (model): whether it has one, its size and its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemModel {
    pub mesh: bool,
    pub radius: f64,
    pub center: Option<Vec3>,
}

/// A round of any kind, for the capture's observers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shot {
    Bolt(Ref<Bolt>),
    Beam(Ref<Beam>),
    Grenade(Ref<Grenade>),
    Round(Ref<Round>),
}

/// What a damage call's definition is: the round's class, or a beam's
/// copy of it carrying this step's share of the Damage (Beam.damage).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefArg {
    Ordnance(DefId),
    Beam(Ref<Beam>),
}

/// Where a unit was hit, and which vector JavaScript is handed for it (a
/// damage event keeps it as the hit's source).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Point {
    /// A vector of its own (a bolt's impact point, made when it struck).
    Fresh(Vec3),
    /// The beam's end.
    BeamEnd(Ref<Beam>, Vec3),
    /// The unit raycast's record's point (the ordnance's `unitHit`).
    UnitHit(Vec3),
    /// The ordnance's `start`.
    Start(Vec3),
}

impl Point {
    pub fn value(&self) -> Vec3 {
        match *self {
            Point::Fresh(v) | Point::BeamEnd(_, v) | Point::UnitHit(v) | Point::Start(v) => v,
        }
    }
}

/// What the capture is told of a round's end (impact).
#[derive(Clone, Debug, PartialEq)]
pub enum ImpactInfo {
    /// `{ point, normal, surface }`.
    Plain { point: Vec3, normal: Vec3, surface: f64 },
    /// `{ ...impact, surface }`: a bolt's impact record, then its surface.
    Struck { impact: Impact, surface: f64 },
}

/// What a projectile record is (projectile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotKind {
    Round,
    Grenade,
    Beam,
}

impl ShotKind {
    pub fn name(self) -> &'static str {
        match self {
            ShotKind::Round => "round",
            ShotKind::Grenade => "grenade",
            ShotKind::Beam => "beam",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub trait World {
    // The rounds (OrdnanceWorld).

    /// collision.cast; `ignore_body` and `static_only` as passed (undefined: not passed).
    fn cast(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>, static_only: Option<bool>) -> Option<Hit>;
    /// collision.castMoving (None too when the collision world has none).
    fn cast_moving(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>) -> Option<Hit>;
    fn raycast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, mount: Option<Ref<Vehicle>>) -> Option<UnitHit>;
    fn rider_mount(&mut self, body: Nullable<Ref<Body>>) -> Option<RiderMount>;
    /// None when the battle's units cannot be queried.
    fn fat_query(&mut self, center: Vec3, radius: f64, owner: OrdnanceOwner, struck: &RefSet<Unit>, ignore_body: Option<Ref<Body>>) -> Option<Vec<Ref<Unit>>>;
    fn unit_sphere(&mut self, unit: Ref<Unit>) -> Option<(Vec3, f64)>;
    fn is_droid(&mut self, unit: Ref<Unit>) -> bool;
    /// Whether an owner or a beam's source is gone (`alive === false`).
    fn gone(&mut self, x: BeamSource) -> bool;
    fn beam_local_origin(&mut self, source: BeamSource, origin: Vec3) -> Option<Vec3>;
    fn beam_pose(&mut self, source: BeamSource, local_origin: Vec3, position: &mut Vec3, direction: &mut Vec3);
    fn shot_tick(&mut self, owner: OrdnanceOwner, channel: f64) -> Nullable<f64>;
    fn history_tick(&mut self) -> f64;
    fn shooter(&mut self, owner: OrdnanceOwner, channel: f64) -> Shooter;
    fn missile_target(&mut self, target_id: f64, target_generation: f64) -> Option<Vec3>;
    /// Turns `velocity`; returns the missile's target id now.
    fn navigate_missile(&mut self, position: Vec3, velocity: &mut Vec3, flight_class: Option<StrId>, target_id: f64, target_generation: f64, dt: f64) -> f64;
    fn water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3>;
    fn waver(&mut self) -> f64;
    fn spin(&mut self) -> f64;
    fn explosion(&mut self, def: &Field, position: Vec3, owner: OrdnanceOwner);
    fn hit(&mut self, body: Ref<Body>, def: DefArg, owner: OrdnanceOwner, direction: Vec3);
    fn hit_unit(&mut self, unit: Ref<Unit>, def: DefArg, point: Point, owner: OrdnanceOwner, part: Nullable<HitPart>, direction: Vec3);
    fn projectile(&mut self, shot: Shot, kind: ShotKind);
    fn launch(&mut self, bolt: Ref<Bolt>, tick: Nullable<f64>);
    fn impact(&mut self, shot: Shot, info: &ImpactInfo);
    fn grenade_contact(&mut self, grenade: Ref<Grenade>);
    fn bounce(&mut self, def: DefId, position: Vec3, speed: f64);

    // The items (ItemsWorld).

    fn item_cast(&mut self, origin: Vec3, direction: Vec3, far: f64) -> Option<Hit>;
    fn blocked(&mut self, position: Vec3, target: Vec3) -> bool;
    fn item_water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3>;
    fn item_model(&mut self, def: DefId) -> ItemModel;
    fn owner_key(&mut self, owner: ItemOwner) -> String;
    fn drop_roll(&mut self) -> f64;
    fn explode(&mut self, def: &Field, position: Vec3, owner: ItemOwner);
    fn watch(&mut self, index: usize, team: f64) -> Watched;
    fn standing(&mut self, index: usize) -> Vec3;
    fn give(&mut self, index: usize, def: DefId) -> bool;
    fn pickup(&mut self, def: DefId, position: Vec3);
}
