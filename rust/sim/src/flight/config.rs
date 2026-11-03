//! src/flight/config.js and the flight half of src/vehicle-motion.js: the
//! port's tuning per class of craft (`FLIGHT_CLASSES`: shared live by every
//! craft of the class, retuned by a command), a craft's class from its name,
//! its own speeds and handling (its class definition's keys, read as the
//! 2004 game reads them), and the stick's speed target and its inverse.
//!
//! JavaScript keeps a craft's configuration as one object per class
//! definition whose prototype is the class's live tuning
//! (`Object.create(FLIGHT_CLASSES[kind])`), so a retune reaches every craft
//! of the class at once; the two sets of keys never overlap. Here the
//! definition's own keys are a [`Config`] and the class's a [`Tuning`] the
//! world holds (`FlightWorld::tuning`), one per battle: the JavaScript's
//! table is the process's, kept from one battle to the next (quirks.md).

use super::FlightClass;
use bfe_content::odf::{class_setting, num, setting};
use bfe_content::value::Value;
use v8math::{max, min};

/// A class's tuning (src/flight/config.js `common` and its roll rate), in
/// the JavaScript object's key order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub boost_acceleration: f64,
    pub boost_turn: f64,
    pub stick_return: f64,
    pub response_curve: f64,
    pub landing_height: f64,
    pub max_slope: f64,
    pub power_drain: f64,
    pub power_recharge: f64,
    pub trick_cost: f64,
    pub trick_cooldown: f64,
    pub barrel_displacement: f64,
    pub missile_turn_rate: f64,
    pub missile_seeker: f64,
    pub missile_speed: f64,
    pub missile_lifetime: f64,
    pub missile_navigation: f64,
    pub missile_roll_break_rate: f64,
    pub missile_evade_range: f64,
    pub barrel_time: f64,
    pub loop_time: f64,
    pub immelmann_time: f64,
    pub trick_window: f64,
    pub terrain_lookahead: f64,
    pub recovery_sharpness: f64,
    pub lethal_impact_speed: f64,
    pub lethal_impact_cosine: f64,
    pub scrape_damage_limit: f64,
    pub impact_damage_speed: f64,
    pub assist_cone: f64,
    pub assist_strength: f64,
    pub sticky_scale: f64,
    pub camera_blend_time: f64,
    pub lock_range: f64,
    pub lock_cone: f64,
    pub retain_cone: f64,
    pub lock_dwell: f64,
    pub los_grace: f64,
    pub roll_rate: f64,
}

/// The tuning's keys as the JavaScript names them, in its order.
pub const TUNING_KEYS: [&str; 38] = [
    "boostAcceleration", "boostTurn", "stickReturn", "responseCurve", "landingHeight", "maxSlope", "powerDrain", "powerRecharge",
    "trickCost", "trickCooldown", "barrelDisplacement", "missileTurnRate", "missileSeeker", "missileSpeed", "missileLifetime",
    "missileNavigation", "missileRollBreakRate", "missileEvadeRange", "barrelTime", "loopTime", "immelmannTime", "trickWindow",
    "terrainLookahead", "recoverySharpness", "lethalImpactSpeed", "lethalImpactCosine", "scrapeDamageLimit", "impactDamageSpeed",
    "assistCone", "assistStrength", "stickyScale", "cameraBlendTime", "lockRange", "lockCone", "retainCone", "lockDwell", "losGrace",
    "rollRate",
];

impl Tuning {
    /// FLIGHT_CLASSES[class] as src/flight/config.js declares it.
    pub const fn of(class: FlightClass) -> Tuning {
        Tuning {
            boost_acceleration: 2.0,
            boost_turn: 0.62,
            stick_return: 3.5,
            response_curve: 1.35,
            landing_height: 18.0,
            max_slope: 0.9375,
            power_drain: 25.0,
            power_recharge: 12.0,
            trick_cost: 30.0,
            trick_cooldown: 1.0,
            barrel_displacement: 12.0,
            missile_turn_rate: 1.1,
            missile_seeker: 15.0,
            missile_speed: 180.0,
            missile_lifetime: 8.0,
            missile_navigation: 3.5,
            missile_roll_break_rate: 8.0,
            missile_evade_range: 300.0,
            barrel_time: 0.6,
            loop_time: 1.8,
            immelmann_time: 1.5,
            trick_window: 0.3,
            terrain_lookahead: 1.5,
            recovery_sharpness: 1.8,
            lethal_impact_speed: 60.0,
            lethal_impact_cosine: 0.8,
            scrape_damage_limit: 0.45,
            impact_damage_speed: 120.0,
            assist_cone: 3.0,
            assist_strength: 0.45,
            sticky_scale: 0.8,
            camera_blend_time: 0.25,
            lock_range: 1600.0,
            lock_cone: 15.0,
            retain_cone: 25.0,
            lock_dwell: 1.0,
            los_grace: 0.3,
            roll_rate: match class {
                FlightClass::Fighter => 2.8,
                FlightClass::Scout => 3.4,
                FlightClass::Bomber => 1.8,
                FlightClass::Transport => 1.3,
            },
        }
    }

    fn slot(&mut self, key: &str) -> Option<&mut f64> {
        Some(match key {
            "boostAcceleration" => &mut self.boost_acceleration,
            "boostTurn" => &mut self.boost_turn,
            "stickReturn" => &mut self.stick_return,
            "responseCurve" => &mut self.response_curve,
            "landingHeight" => &mut self.landing_height,
            "maxSlope" => &mut self.max_slope,
            "powerDrain" => &mut self.power_drain,
            "powerRecharge" => &mut self.power_recharge,
            "trickCost" => &mut self.trick_cost,
            "trickCooldown" => &mut self.trick_cooldown,
            "barrelDisplacement" => &mut self.barrel_displacement,
            "missileTurnRate" => &mut self.missile_turn_rate,
            "missileSeeker" => &mut self.missile_seeker,
            "missileSpeed" => &mut self.missile_speed,
            "missileLifetime" => &mut self.missile_lifetime,
            "missileNavigation" => &mut self.missile_navigation,
            "missileRollBreakRate" => &mut self.missile_roll_break_rate,
            "missileEvadeRange" => &mut self.missile_evade_range,
            "barrelTime" => &mut self.barrel_time,
            "loopTime" => &mut self.loop_time,
            "immelmannTime" => &mut self.immelmann_time,
            "trickWindow" => &mut self.trick_window,
            "terrainLookahead" => &mut self.terrain_lookahead,
            "recoverySharpness" => &mut self.recovery_sharpness,
            "lethalImpactSpeed" => &mut self.lethal_impact_speed,
            "lethalImpactCosine" => &mut self.lethal_impact_cosine,
            "scrapeDamageLimit" => &mut self.scrape_damage_limit,
            "impactDamageSpeed" => &mut self.impact_damage_speed,
            "assistCone" => &mut self.assist_cone,
            "assistStrength" => &mut self.assist_strength,
            "stickyScale" => &mut self.sticky_scale,
            "cameraBlendTime" => &mut self.camera_blend_time,
            "lockRange" => &mut self.lock_range,
            "lockCone" => &mut self.lock_cone,
            "retainCone" => &mut self.retain_cone,
            "lockDwell" => &mut self.lock_dwell,
            "losGrace" => &mut self.los_grace,
            "rollRate" => &mut self.roll_rate,
            _ => return None,
        })
    }

    /// A key's value, by the JavaScript's name.
    pub fn get(&self, key: &str) -> Option<f64> {
        let mut copy = *self;
        copy.slot(key).map(|x| *x)
    }

    /// src/game.js step, `command.flightTuning`: a key of the class's own,
    /// set to a finite number above 0; anything else is let be. Returns
    /// whether it was set. (The JavaScript's `key in FLIGHT_CLASSES[name]`
    /// also takes keys every object inherits, which no flight code reads:
    /// left out here, quirks.md.)
    pub fn retune(&mut self, key: &str, value: f64) -> bool {
        if !(value.is_finite() && value > 0.0) {
            return false;
        }
        match self.slot(key) {
            Some(slot) => {
                *slot = value;
                true
            }
            None => false,
        }
    }
}

/// The battle's four class tunings (FLIGHT_CLASSES), as the battle's world
/// holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tunings(pub [Tuning; 4]);

impl Default for Tunings {
    fn default() -> Self {
        Tunings([
            Tuning::of(FlightClass::Fighter),
            Tuning::of(FlightClass::Scout),
            Tuning::of(FlightClass::Bomber),
            Tuning::of(FlightClass::Transport),
        ])
    }
}

impl Tunings {
    pub fn of(&self, class: FlightClass) -> Tuning {
        self.0[class as usize]
    }

    /// `command.flightTuning`'s entry for a class by its JavaScript name.
    pub fn retune(&mut self, class: &str, key: &str, value: f64) -> bool {
        let Some(i) = FlightClass::NAMES.iter().position(|&n| n == class) else { return false };
        self.0[i].retune(key, value)
    }
}

/// src/flight/config.js flightClass: a craft's class from its class name.
pub fn flight_class(name: &str) -> FlightClass {
    let n = name.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if any(&["gunship", "transport", "maf", "laat"]) {
        FlightClass::Transport
    } else if any(&["bomber", "ywing", "bwing"]) {
        FlightClass::Bomber
    } else if any(&["awing", "interceptor", "geofighter", "tiefighter", "snowspeeder"]) {
        FlightClass::Scout
    } else {
        FlightClass::Fighter
    }
}

/// A class definition's own flight keys (src/vehicle-motion.js
/// flightSpeeds and flightHandling, and the boost speed), in the order
/// flightConfig assigns them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Config {
    pub min_speed: f64,
    pub cruise_speed: f64,
    pub max_speed: f64,
    pub acceleration: f64,
    pub turn_rate: f64,
    pub turn_filter: f64,
    pub pitch_rate: f64,
    pub pitch_filter: f64,
    pub bank_angle: f64,
    pub bank_filter: f64,
    pub level_filter: f64,
    pub takeoff_height: f64,
    pub takeoff_time: f64,
    pub takeoff_speed: f64,
    pub landed_height: f64,
    pub landing_time: f64,
    pub landing_speed: f64,
    pub boost_speed: f64,
}

impl Config {
    /// flightConfig(def, kind)'s own keys: flightSpeeds(def),
    /// flightHandling(def), then `{ boostSpeed: speeds.maxSpeed }`.
    pub fn of(def: &Value) -> Config {
        // vehicle-motion.js number(def, name, fallback, read): parseFloat
        // of the key as `read` finds it, when finite.
        let class = |name: &str, fallback: f64| num(class_setting(Some(def), name), fallback);
        let body = |name: &str, fallback: f64| num(setting(def.get("settings"), name), fallback);
        let max_speed = class("MaxSpeed", 30.0);
        Config {
            min_speed: class("MinSpeed", 10.0),
            cruise_speed: class("MidSpeed", 20.0),
            max_speed,
            acceleration: class("Acceleraton", 5.0),
            turn_rate: max(body("TurnRate", 2.0), 1.5),
            turn_filter: body("TurnFilter", 10.0),
            pitch_rate: body("PitchRate", 1.0),
            pitch_filter: body("PitchFilter", 10.0),
            bank_angle: class("BankAngle", 0.5),
            bank_filter: class("BankFilter", 2.0),
            level_filter: class("LevelFilter", 3.0),
            takeoff_height: class("TakeoffHeight", 5.0),
            takeoff_time: class("TakeoffTime", 5.0),
            takeoff_speed: class("TakeoffSpeed", 3.0),
            landed_height: class("LandedHeight", 0.0),
            landing_time: class("LandingTime", 5.0),
            landing_speed: class("LandingSpeed", 3.0),
            boost_speed: max_speed,
        }
    }
}

/// throttleSpeed(c, throttle): the stick's speed target: MidSpeed
/// released, MaxSpeed forward, MinSpeed back.
pub fn throttle_speed(c: &Config, throttle: f64) -> f64 {
    if throttle >= 0.0 {
        c.cruise_speed + (c.max_speed - c.cruise_speed) * throttle
    } else {
        c.cruise_speed + (c.cruise_speed - c.min_speed) * throttle
    }
}

/// throttleFor(c, speed): the stick for a speed.
pub fn throttle_for(c: &Config, speed: f64) -> f64 {
    let span = if speed >= c.cruise_speed { c.max_speed - c.cruise_speed } else { c.cruise_speed - c.min_speed };
    if span > 0.0 {
        max(-1.0, min(1.0, (speed - c.cruise_speed) / span))
    } else {
        0.0
    }
}
