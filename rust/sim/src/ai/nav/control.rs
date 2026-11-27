//! The movement request and one unit's navigation state.
//! The state's operations (attach, publish, consume, install, the trail,
//! the per-tick update) are the Nav service's methods in nav/mod.rs,
//! because they reach the planner and the path store the service owns;
//! this file is the state itself, field for field.

use super::executor::PathExecutor;
use super::math::Vec3;
use super::units::UnitRef;

/// MovementRequest: what an executor (or a stop) writes
/// for the unit's input step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementRequest {
    pub valid: bool,
    pub steering_target: Vec3,
    pub waypoint: Vec3,
    pub segment_direction: Vec3,
    pub mode: f64,
    pub lower: f64,
    pub upper: f64,
}

impl Default for MovementRequest {
    fn default() -> Self {
        MovementRequest {
            valid: false,
            steering_target: Vec3::default(),
            waypoint: Vec3::default(),
            segment_direction: Vec3::default(),
            mode: 0.0,
            lower: -1.0,
            upper: 1.0,
        }
    }
}

impl MovementRequest {
    pub fn write(&mut self, target: Vec3, waypoint: Vec3, direction: Vec3, mode: f64, lower: f64, upper: f64) {
        self.valid = true;
        self.steering_target = target;
        self.waypoint = waypoint;
        self.segment_direction = direction;
        self.mode = mode;
        self.lower = lower;
        self.upper = upper;
    }

    pub fn clear(&mut self) {
        self.valid = false;
    }
}

/// A pending jet request.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JetRequest {
    pub pending: bool,
    pub vector: Vec3,
    /// A 2005 bot's request to end its jet (the game's end-jet flag), consumed by the 2005
    /// input step while a jet sequence runs. Never set for a 2004 bot.
    pub end: bool,
    /// Where a 2005 bot's jet at a target steers while the body jets (the target, as the bot's
    /// follow order would); none for every other jet.
    pub steer: Option<Vec3>,
    /// A 2005 bot's melee leap (the game's leap flag): one jump along `vector`, no jet.
    pub leap: bool,
}

/// NavControl's fields. `unit` is the identity token of
/// units.rs for the unit object it is attached to, with the generation
/// it attached at (`unit?.generation ?? -1`). `published` is a path in
/// the service's store. `leader` is follow's unit reference.
#[derive(Clone, Debug)]
pub struct NavControl {
    pub unit: Option<u64>,
    pub generation: f64,
    pub serial: f64,
    pub disposed: bool,
    pub request: MovementRequest,
    pub executor: Option<PathExecutor>,
    pub published: Option<usize>,
    pub ready: bool,
    pub segments: u32,
    pub install_flags: [bool; 2],
    pub landing: bool,
    pub stance: f64,
    pub jet: JetRequest,
    pub jetting: bool,
    pub avoid_side: f64,
    pub stranded: bool,
    pub stranded_for: f64,
    pub stranded_at: Vec3,
    pub leader: Option<UnitRef>,
    pub trail: Option<usize>,
    pub trail_delay: f64,
}

impl NavControl {
    /// field initializers. (When a controller already
    /// holds a MovementRequest the JavaScript shares it; the service's
    /// control is the one request, so there is nothing to share.)
    pub fn new() -> Self {
        NavControl {
            unit: None,
            generation: -1.0,
            serial: 0.0,
            disposed: false,
            request: MovementRequest::default(),
            executor: None,
            published: None,
            ready: false,
            segments: 0,
            install_flags: [false, false],
            landing: false,
            stance: 0.0,
            jet: JetRequest::default(),
            jetting: false,
            avoid_side: 0.0,
            stranded: false,
            stranded_for: 0.0,
            stranded_at: Vec3::new(f64::INFINITY, 0.0, 0.0),
            leader: None,
            trail: None,
            trail_delay: 0.0,
        }
    }
}
