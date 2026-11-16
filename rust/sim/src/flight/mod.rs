//! Flight (and `Vehicle.fly`): a craft's tick at 1/60 s,
//! inside the hulls' phase, to the last bit as the JavaScript steps it.

pub mod ai;
pub mod config;
pub mod controls;
pub mod fly;
pub mod gear;
pub mod model;
pub mod pilotless;
pub mod pose;
pub mod state;
pub mod store;
pub mod targeting;

pub use config::{Config, Tuning};
pub use state::{Flight, LockState, Message, Trick};

use crate::vehicles::{BodyId, VehicleWorld};
use three::Vector3;

/// A craft's class of flight (flightClass): its
/// tuning's row.
pub use crate::gen::FlightClass;
/// A craft's flight state (null for a ground hull: `Option`).
pub use crate::gen::FlightState;

/// A landing zone of the battle's flight environment (a Box3 and the floor
/// a craft lands on in it, `floorY ?? min[1]`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zone {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub floor_y: f64,
}

/// The battle's flight environment (Vehicles.flightEnvironment: the
/// mission's ceiling, the map's bounds, and a space map's zones), and what
/// else flight reads of the battle's settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Environment {
    /// `env.ceiling` (None: undefined, then the mission's fly height).
    pub ceiling: Option<f64>,
    /// `system.maxFlyHeight`.
    pub max_fly_height: f64,
    /// `env.bounds`: min and max.
    pub bounds: Option<([f64; 3], [f64; 3])>,
    /// `Boolean(env.space)`.
    pub space: bool,
    /// `env.landingZones` (None: undefined).
    pub landing_zones: Option<std::rc::Rc<[Zone]>>,
    /// `system.flightAssists`: steering assists and the assist target
    /// (off on Hard).
    pub assists: bool,
}

impl Environment {
    /// F2's cold flight-environment reader is also used by private native
    /// sessions. Authored bounds and landing zones keep the same defaults.
    pub fn from_content(env:&bfe_content::Value,max_fly_height:f64)->Self{
        use bfe_content::Value;
        let nums3 = |v: Option<&Value>| -> [f64; 3] { [0, 1, 2].map(|k| v.and_then(|a| a.at(k)).and_then(Value::as_f64).unwrap_or(f64::NAN)) };
        let bounds = env.get("bounds").filter(|b| !matches!(b, Value::Null)).map(|b| (nums3(b.get("min")), nums3(b.get("max"))));
        let landing_zones = env.get("landingZones").filter(|z| !matches!(z, Value::Null)).map(|zones| {
            zones
                .as_array()
                .unwrap_or(&[])
                .iter()
                .map(|z| {
                    let (min, max) = (nums3(z.get("min")), nums3(z.get("max")));
                    let floor_y = match z.get("floorY") {
                        None | Some(Value::Null) => min[1],
                        Some(v) => v.as_f64().unwrap_or(f64::NAN),
                    };
                    Zone { min, max, floor_y }
                })
                .collect()
        });
        Self {
            ceiling: env.get("ceiling").and_then(Value::as_f64),
            max_fly_height,
            bounds,
            space: env.get("space").is_some_and(Value::truthy),
            landing_zones,
            assists: false,
        }
    }
}

/// A team as the registry holds one: a number, or nothing (null or
/// undefined, which the registry's relation treats alike).
pub type Team = Option<f64>;

/// One of the registry's entries (a unit, or a hull's, turret's or
/// building's target) as targeting reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Member {
    pub id: f64,
    /// `t.generation` (None: undefined; read `?? 0`).
    pub generation: Option<f64>,
    /// Whether `t.alive` is truthy.
    pub alive: bool,
    pub team: Team,
    /// Whether it is the craft's own entry (`t === v.aiTarget`).
    pub own: bool,
    /// A unit (its aim point is worked out when asked: `FlightWorld::aim_point`).
    pub unit: bool,
    /// The collision body of the hull behind a hull's entry (`t.vehicle.body`).
    pub body: Option<BodyId>,
    /// Any other entry's aim point (the middle of its box as it stands).
    pub point: Vector3,
}

/// What flight asks of the rest of the battle besides the hulls' calls.
pub trait FlightWorld: VehicleWorld {
    /// The battle's flight environment and settings.
    fn environment(&mut self) -> &Environment;
    /// The live tuning of a class (`FLIGHT_CLASSES[class]`).
    fn tuning(&mut self, class: FlightClass) -> Tuning;
    /// How many entries the registry holds now (`units.all().length`).
    fn member_count(&mut self) -> usize;
    /// The registry's entry `i` (in its order) as it is now.
    fn member(&mut self, i: usize) -> Member;
    /// Units.byId: the entry with this id.
    fn member_by_id(&mut self, id: f64) -> Option<usize> {
        let n = self.member_count();
        (0..n).find(|&i| sim_same(self.member(i).id, id))
    }
    /// Units.relation(a, b): 1 friend, 0 neutral, -1 enemy.
    fn relation(&mut self, a: Team, b: Team) -> f64;
    /// Entry `i`'s aim point (`t.aimPoint(point)`): a unit's hit shapes'
    /// middle, worked out on the first ask in a tick (so asked when the
    /// JavaScript asks it); any other entry's the middle of its box as it
    /// stands (`Member::point`).
    fn aim_point(&mut self, i: usize) -> Vector3;
    /// The entries whose aim points targeting is about to ask, one by one
    /// and in this order (a pass of bestTarget: every living enemy entry),
    /// for a world that answers them better together (the module asks
    /// JavaScript for all its units' at once). Nothing is asked of the
    /// battle between those asks, so answering them early answers them as
    /// asked; a world that serves each ask as it comes does nothing here.
    fn prefetch_aim_points(&mut self, _members: &[usize]) {}
    /// Whether a collision body is a hull's (`body.owner.system === v.system`).
    fn body_is_hull(&mut self, body: BodyId) -> bool;
    /// Vehicles.inDeathRegion (a pilotless craft asks its system directly).
    fn system_death_region(&mut self, position: &Vector3) -> bool;
}

/// `Map.get` by a number key: SameValueZero.
fn sim_same(a: f64, b: f64) -> bool {
    a == b || a.is_nan() && b.is_nan()
}
