//! The hulls (src/vehicles.js but flight, src/vehicle-hover.js): each hull's
//! tick on the ground, two 1/120 s substeps a world tick, as the JavaScript
//! steps it, to the last bit.
//!
//! - `hull`: a hull's state and its tick (`Vehicle.tick`): its crew's aim,
//!   its steering, its wheels, its collision body's frame, the death regions
//!   and deep water, and whether it sleeps;
//! - `hover`: a hover on its springs (`hoverPhysics`, `hoverContacts`,
//!   `hoverSprings`), the springs' contact search included;
//! - `walker`: a walker or riding animal on the ground (`walkGround`,
//!   `standOnLegs`, `alignGround`).
//!
//! A hull reaches the rest of the battle through [`VehicleWorld`] alone, the
//! calls src/vehicles-world.js makes for the JavaScript hull, in the order
//! the JavaScript makes them: the collision world's queries, the water, the
//! death regions, the battle's hooks, explosions and effects. The module
//! answers them from its own collision world; the replays
//! (tests/replay_vehicles.rs) from a capture, checking every argument.
//!
//! Transcendental functions come from `crate::rig::math` (V8's, from
//! rust/v8math); Math.min, max, hypot and sign from rust/v8math; three.js's
//! vectors and quaternions from rust/three, operation by operation.

pub mod hover;
pub mod hull;
pub mod walker;
#[cfg(feature = "replay")]
pub mod replay;

pub use hull::{Hull, Input, Lease, Seat, Wheel};

use three::{Matrix4, Quaternion, Vector3};

/// A collision body, by its serial (src/collision.js: its `offset` / 2^24).
pub type BodyId = u32;

/// What a cast met (src/collision.js makeHit).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
    pub terrain: bool,
    pub body: Option<BodyId>,
}

/// The terrain under a column (CollisionWorld.ground): its height and normal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ground {
    pub height: f64,
    pub normal: Vector3,
}

/// A polygon a gather met: its id (the collision world's, unique among the
/// gather's), whether it is a moving body's, and its triangles' corners.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    pub id: f64,
    pub moving: bool,
    pub triangles: Vec<[Vector3; 3]>,
}

/// What a hull asks of the rest of the battle (src/vehicles-world.js), each
/// call as the JavaScript makes it. `hull` names a hull by its place in the
/// battle's list (Vehicles.vehicles).
pub trait VehicleWorld {
    /// CollisionWorld.cast: the first hit along a ray within `far`, leaving
    /// out `ignore`.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit>;
    /// A hull's wall probes pushed out of the walls they meet
    /// (VehiclesWorld.pushWalls): `position` and `velocity` as they leave
    /// them; true when a push went deeper than 0.15 m.
    #[allow(clippy::too_many_arguments)]
    fn push_walls(&mut self, hull: usize, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool;
    /// CollisionWorld.ground: the terrain under (x, z) from `top` down.
    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground>;
    /// The polygons near spring `index` of a hover (CollisionWorld.gather
    /// through the spring's cache), into `out`.
    fn spring_polygons(&mut self, hull: usize, index: usize, center: &Vector3, radius: f64, out: &mut Vec<Polygon>);
    /// CollisionWorld.moveBody.
    fn move_body(&mut self, body: BodyId, matrix: &Matrix4);
    /// A body in or out of the collision's queries.
    fn enable_body(&mut self, body: BodyId, enabled: bool);
    /// Whether the map has water, and then whether (x, z) is over it.
    fn water_over(&mut self, x: f64, z: f64) -> bool;
    /// The water's level now (only asked after water_over said yes, or for a
    /// map with water).
    fn water_level(&mut self) -> f64;
    /// Whether the map has water at all (`system.water`).
    fn has_water(&mut self) -> bool;
    /// Whether a point is in one of the map's death regions.
    fn in_death_region(&mut self, position: &Vector3) -> bool;
    /// The battle's hook: a hull took `amount` (from nobody, in its own tick).
    fn damaged(&mut self, hull: usize, amount: f64);
    /// Vehicles.destroyedCrew: the bots aboard a destroyed hull let go and killed.
    fn destroyed_crew(&mut self, hull: usize);
    /// The battle's hook: a hull destroyed.
    fn destroyed(&mut self, hull: usize);
    /// An explosion (the class's `explosion`, its `finalExplosion`) at a point, owned by the hull.
    fn explosion(&mut self, hull: usize, final_explosion: bool, position: &Vector3);
    /// An effect at a point (recorded by a world for its page).
    fn effect(&mut self, name: &str, position: &Vector3);
}
