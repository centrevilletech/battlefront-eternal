//! The hulls (src/vehicles.js but flight, src/vehicle-hover.js): each hull's
//! tick on the ground, two 1/120 s substeps a world tick, as the JavaScript
//! steps it, to the last bit.
//!
//! - `hull`: a hull's state and its tick (`Vehicle.tick`): its crew's aim,
//!   its steering, its wheels, its collision body's frame, the death regions
//!   and deep water, and whether it sleeps;
//! - `hover`: a hover on its springs (`hoverPhysics`, `hoverContacts`,
//!   `hoverSprings`), the springs' contact search included;
//! - `class` and `store`: what a hull's tick reads of its class (the
//!   content's) and of the store (rust/sim/schema/vehicles.mjs), and what
//!   it gives back;
//! - `walker`: a walker or riding animal on the ground (`walkGround`,
//!   `standOnLegs`, `alignGround`);
//! - `pose`: a hull's pose at the tick (`aimTick`, `render`): its root,
//!   its seats' aim targets, turrets and aimers, its barrels' kick.
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

pub mod build;
pub mod class;
pub mod damage;
pub mod firing;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "module/f2/firing.rs"]
mod firing_phase;
pub mod hover;
pub mod hull;
#[path = "module/f2/seats.rs"]
pub mod f2_seats;
#[path = "module/f2/delta.rs"]
pub mod f2_delta;
#[path = "module/f2/publication.rs"]
pub mod f2_publication;
pub mod native;
pub mod pose;
pub mod posing;
pub mod spawn;
pub mod store;
pub mod tethers;
pub mod walker;
#[cfg(target_arch = "wasm32")]
pub mod module;
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

/// hoverContacts reads the gather until its next query. Native worlds lend
/// that prepared storage directly; replay hosts retain their decoded rows.
/// Neither representation changes polygon or triangle visitation order.
pub enum SpringPolygons<'a> {
    Recorded(&'a [Polygon]),
    Native(&'a [crate::collision::spheres::Polygon]),
}

impl SpringPolygons<'_> {
    pub fn len(&self) -> usize {
        match self { Self::Recorded(rows) => rows.len(), Self::Native(rows) => rows.len() }
    }
    pub fn moving(&self, index: usize) -> bool {
        match self { Self::Recorded(rows) => rows[index].moving, Self::Native(rows) => rows[index].moving }
    }
    pub fn triangles(&self, index: usize) -> usize {
        match self { Self::Recorded(rows) => rows[index].triangles.len(), Self::Native(rows) => rows[index].tris.len() }
    }
    pub fn triangle(&self, index: usize, triangle: usize) -> [Vector3; 3] {
        match self {
            Self::Recorded(rows) => rows[index].triangles[triangle],
            Self::Native(rows) => {
                let t = &rows[index].tris[triangle];
                [Vector3::new(t.a[0], t.a[1], t.a[2]),
                 Vector3::new(t.b[0], t.b[1], t.b[2]),
                 Vector3::new(t.c[0], t.c[1], t.c[2])]
            }
        }
    }
}

/// What a hull asks of the rest of the battle (src/vehicles-world.js), each
/// call as the JavaScript makes it. `hull` names a hull by its place in the
/// battle's list (Vehicles.vehicles).
pub trait VehicleWorld {
    /// Private replay hulls override damage with a no-op. Battle worlds
    /// retain the normal damage path unless they explicitly opt out.
    fn damage_allowed(&self) -> bool { true }
    /// A private clone may share its leg-side array with a JavaScript
    /// source. Publish a native write, or refresh after a world callback.
    /// Worlds that own the whole hull need no exchange.
    fn leg_side(&mut self, _hull: usize, _sides: &mut Vec<f64>, _publish: bool) {}
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
    fn spring_polygons<'a>(&'a mut self, hull: usize, index: usize, center: &Vector3, radius: f64, out: &'a mut Vec<Polygon>) -> SpringPolygons<'a>;
    /// CollisionWorld.moveBody.
    fn move_body(&mut self, body: BodyId, matrix: &Matrix4);
    /// A body in or out of the collision's queries.
    fn enable_body(&mut self, body: BodyId, enabled: bool);
    /// Whether a body takes part in the collision's queries.
    fn body_enabled(&mut self, body: BodyId) -> bool;
    /// Whether the map has water, and then whether (x, z) is over it.
    fn water_over(&mut self, x: f64, z: f64) -> bool;
    /// The water's level now (only asked after water_over said yes, or for a
    /// map with water).
    fn water_level(&mut self) -> f64;
    /// Whether the map has water at all (`system.water`).
    fn has_water(&mut self) -> bool;
    /// Whether a point is in one of the map's death regions.
    fn in_death_region(&mut self, position: &Vector3) -> bool;
    /// The battle's hook: a hull took `amount`, credited to `attacker`.
    fn damaged(&mut self, hull: usize, amount: f64, attacker: &crate::gen::OrdnanceOwner);
    /// Vehicles.destroyedCrew: the bots aboard a destroyed hull let go and killed.
    fn destroyed_crew(&mut self, hull: usize);
    /// The battle's hook: a hull destroyed.
    fn destroyed(&mut self, hull: usize);
    /// An explosion (the class's `explosion`, its `finalExplosion`) at a point, owned by the hull.
    fn explosion(&mut self, hull: usize, final_explosion: bool, position: &Vector3);
    /// An effect at a point (recorded by a world for its page).
    fn effect(&mut self, name: &str, position: &Vector3);
    /// Units.raycast: the distance to the first unit along a ray within `far`.
    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64>;
    /// The clock barrels kick back on (a world's tick clock).
    fn clock(&mut self) -> f64;
    /// A tow cable let go (snapped, tripped its walker, released), which the
    /// presentation capture hears: emitSound("towRelease", hull, def, reason).
    fn tow_released(&mut self, hull: usize, reason: &str);
    /// A spawn's new life (Vehicle.spawn): the hull's registry entry starts
    /// a new generation and its seats get new weapons, which are the
    /// registry's and the combat package's (JavaScript's until they move);
    /// a world whose spawns are JavaScript's has nothing to do.
    fn new_life(&mut self, _hull: usize) {}
    /// A destroyed hull's pad starts its respawn clock (`slot.remaining =
    /// slot.desc.respawnTime`): the pads are the battle's while the hull
    /// phase runs on the module with JavaScript's Vehicles.
    fn respawn_clock(&mut self, _hull: usize) {}
    /// A destroyed hull lets its crew's leases go (each lease released, its
    /// unit's seat cleared), which are the seats package's.
    fn release_crew(&mut self, _hull: usize) {}
    /// The hull before a hook that hands it to the battle (damaged,
    /// destroyedCrew, destroyed, explosion), and after it: a world whose
    /// hulls are JavaScript's while the phase runs makes the battle see this
    /// hull as it is now, and reads back what the hook did to it.
    fn current(&mut self, _hull: &Hull) {}
    fn reload(&mut self, _hull: &mut Hull) {}
}
