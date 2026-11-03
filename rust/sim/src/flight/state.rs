//! What flight keeps of a hull (rust/sim/schema/flight.mjs): the numbers
//! every hull carries for it (its flight state, null on the ground) and
//! what initFlight gives a craft as it spawns. Field names are the
//! JavaScript's in snake case, less their `flight` prefix where they have
//! one (`flightClock` is `clock`).

use super::config::Config;
use super::{FlightClass, FlightState};
use crate::pose::Clip;
use three::{Quaternion, Vector3};

/// A trick in progress (`v.trick`: '' for none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trick {
    #[default]
    None,
    Barrel,
    Loop,
    Immelmann,
}

impl Trick {
    pub const TEXTS: [&'static str; 4] = ["", "barrel", "loop", "immelmann"];
    pub fn text(self) -> &'static str {
        Self::TEXTS[self as usize]
    }
    pub fn from_text(s: &str) -> Option<Trick> {
        Some([Trick::None, Trick::Barrel, Trick::Loop, Trick::Immelmann][Self::TEXTS.iter().position(|&t| t == s)?])
    }
    /// Whether `v.trick` is truthy (a trick is on).
    pub fn on(self) -> bool {
        self != Trick::None
    }
}

/// The lock's state (`v.flightLockState`: '' for none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LockState {
    #[default]
    None,
    Locking,
    Locked,
}

impl LockState {
    pub const TEXTS: [&'static str; 3] = ["", "Locking", "Locked"];
    pub fn text(self) -> &'static str {
        Self::TEXTS[self as usize]
    }
    pub fn from_text(s: &str) -> Option<LockState> {
        Some([LockState::None, LockState::Locking, LockState::Locked][Self::TEXTS.iter().position(|&t| t == s)?])
    }
}

/// A message a craft shows (its warning, its landing prompt and reason),
/// the texts the network's flight records know (src/runtime/
/// network-entity-flight.js `messages`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Message {
    #[default]
    None,
    TakeoffBlocked,
    LandingBlocked,
    TooFast,
    NoLandingZone,
    PressSpace,
    Ceiling,
    Leaving,
}

impl Message {
    pub const TEXTS: [&'static str; 8] =
        ["", "Takeoff path blocked", "Landing path blocked", "Too fast", "No landing zone", "Press SPACE to land", "Flight ceiling", "Leaving battlefield"];
    pub fn text(self) -> &'static str {
        Self::TEXTS[self as usize]
    }
    pub fn from_text(s: &str) -> Option<Message> {
        const ALL: [Message; 8] = [
            Message::None,
            Message::TakeoffBlocked,
            Message::LandingBlocked,
            Message::TooFast,
            Message::NoLandingZone,
            Message::PressSpace,
            Message::Ceiling,
            Message::Leaving,
        ];
        Some(ALL[Self::TEXTS.iter().position(|&t| t == s)?])
    }
    /// Whether the text is not empty (`v.landingMessage` truthy).
    pub fn shown(self) -> bool {
        self != Message::None
    }
}

/// A takeoff clip (a craft's gear, doors and foils), its length in seconds,
/// and the bone its constant lift is on (DummyRoot, -1 for none).
#[derive(Clone)]
pub struct GearClip {
    pub clip: std::rc::Rc<Clip>,
    pub duration: f64,
    pub root: i32,
}

impl std::fmt::Debug for GearClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GearClip({} frames, {} s, root {})", self.clip.frames, self.duration, self.root)
    }
}

/// What updateFlightTarget leaves in every weapon of every seat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lock {
    pub target_id: f64,
    pub generation: f64,
    pub elapsed: f64,
    pub complete: bool,
}

/// A hull's flight.
#[derive(Clone, Debug, Default)]
pub struct Flight {
    // Every hull's.
    pub state: Option<FlightState>,
    pub grounded: bool,
    pub altitude: f64,
    pub throttle: f64,
    pub airspeed: f64,
    pub state_time: f64,
    pub floor_y: f64,
    pub bank: f64,
    pub gear_foot: Option<f64>,
    pub radius: f64,
    pub team: Option<f64>,
    // A craft's, from initFlight.
    pub class: FlightClass,
    /// Its class definition's own keys (flightConfig less the tuning).
    pub config: Config,
    pub min_speed: f64,
    pub mid_speed: f64,
    pub boost_speed: f64,
    pub power: f64,
    pub stick_x: f64,
    pub stick_y: f64,
    pub roll_rate: f64,
    pub roll_lock: bool,
    pub boosting: bool,
    pub trick: Trick,
    pub trick_time: f64,
    pub trick_cooldown: f64,
    pub trick_pending: f64,
    pub trick_direction: f64,
    pub gear: f64,
    pub landing_message: Message,
    pub landing_message_time: f64,
    pub trick_right: Vector3,
    pub clock: f64,
    pub boost_exhausted: bool,
    pub landing_reason: Message,
    pub warning: Message,
    pub terrain_warning: bool,
    pub assist_id: f64,
    pub lock_id: f64,
    pub lock_generation: f64,
    pub lock_timer: f64,
    pub lost_los: f64,
    pub lock_state: LockState,
    pub start: Vector3,
    pub end: Vector3,
    pub start_q: Quaternion,
    pub trick_loop_q: Quaternion,
    pub trick_end_q: Quaternion,
    pub previous_q: Quaternion,
    pub tick_q: Quaternion,
    /// The keys held at its last step (None before its first).
    pub action_held: Option<bool>,
    pub trick_held: Option<bool>,
    pub immelmann_held: Option<bool>,
    /// The frame drawFlyer last drew it by.
    pub frame_q: Option<Quaternion>,
    /// Its takeoff clip, if its class has one (`v.clips.get('takeoff')`).
    pub gear_clip: Option<GearClip>,
    /// What its last targeting left in its weapons (None: not this tick).
    pub lock: Option<Lock>,
}

impl Flight {
    /// initFlight (src/flight/model.js), for a craft of `class` whose
    /// definition's keys are `config`, its frame `quaternion`: the hull's
    /// speeds from the configuration (the caller sets the hull's turn rate
    /// and speed to `config.turn_rate` and `config.max_speed`), then a
    /// craft landed with full power, stick centred, no trick, gear down, no
    /// target.
    pub fn init(&mut self, class: FlightClass, config: Config, quaternion: &Quaternion) {
        self.class = class;
        self.config = config;
        self.min_speed = config.min_speed;
        self.mid_speed = config.cruise_speed;
        self.boost_speed = config.boost_speed;
        self.state = Some(FlightState::Landed);
        self.power = 100.0;
        self.stick_x = 0.0;
        self.stick_y = 0.0;
        self.roll_rate = 0.0;
        self.bank = 0.0;
        self.roll_lock = false;
        self.boosting = false;
        self.trick = Trick::None;
        self.trick_time = 0.0;
        self.trick_cooldown = 0.0;
        self.trick_pending = -1.0;
        self.trick_direction = 1.0;
        self.gear = 1.0;
        self.landing_message = Message::None;
        self.landing_message_time = 0.0;
        self.trick_right = Vector3::default();
        self.clock = 0.0;
        self.boost_exhausted = false;
        self.landing_reason = Message::None;
        self.warning = Message::None;
        self.terrain_warning = false;
        self.assist_id = -1.0;
        self.lock_id = -1.0;
        self.lock_generation = -1.0;
        self.lock_timer = 0.0;
        self.lost_los = 0.0;
        self.lock_state = LockState::None;
        self.start = Vector3::default();
        self.end = Vector3::default();
        self.start_q = Quaternion::default();
        self.trick_loop_q = Quaternion::default();
        self.trick_end_q = Quaternion::default();
        self.previous_q = *quaternion;
        self.tick_q = *quaternion;
    }
}
