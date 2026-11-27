//! A 2005 soldier class's movement, read from the matched prepared content:
//! its numbers (the class record's properties, its jetpack among them, the
//! frame count of its forward roll clip and the lengths of its two stand-up
//! clips) and the acceleration its body takes. This is the one rule for local
//! play, the authority and prediction: a networked battle's admission
//! registers the numbers on the server and builds its bodies by it; a client
//! reads both from its own copy of the same content, which the match's
//! content digest covers (the sides' `units.json` and every animation bank).

use super::{ControlSpeed, Jetpack, Tuning, DEFAULT_GETUP_BACK_SECONDS, DEFAULT_GETUP_FRONT_SECONDS};
use crate::movement::soldier::Jet;
use bfe_content::{content::Content, defs::{ClassDef, Defs}, value::Value};
use bfe_pose::character::stance;

/// The record's `[name, value]` properties, in inheritance order: a later
/// pair overrides an earlier one, so readers search from the end.
pub(crate) fn pairs(record: &Value) -> impl DoubleEndedIterator<Item = (&str, &str)> {
    record.get("properties").and_then(Value::as_array).unwrap_or_default().iter().filter_map(|v| {
        let [name, value] = v.as_array()? else { return None };
        Some((name.as_str()?, value.as_str()?))
    })
}

/// Whether a class carries a melee combo: a non-hover jet class with one
/// makes a Force double jump instead of flying.
pub(crate) fn has_melee_weapon(record: &Value) -> bool {
    bfe_pose::character::array(record, "weapons")
        .iter()
        .any(|weapon| {
            bfe_pose::character::array(weapon, "comboBindings")
                .iter()
                .any(|binding| binding.get("mode").and_then(Value::as_str) == Some("melee"))
        })
}

/// A class's jetpack: none unless its properties give a jump or a push.
/// Every jet number it names must be finite and nonnegative (an absent one
/// is zero); `JetEnergyDrain` is the energy one ignition costs; `JetType`
/// "hover" hovers.
pub fn jetpack(record: &Value) -> Result<Option<Jetpack>, String> {
    let get = |name: &str| pairs(record).rev().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, v)| v);
    let number = |name: &str| match get(name) {
        Some(value) => value.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.)
            .ok_or_else(|| format!("invalid infantry property {name}")),
        None => Ok(0.),
    };
    let jump = number("JetJump")?;
    let push = number("JetPush")?;
    if jump == 0. && push == 0. { return Ok(None); }
    let hover = get("JetType").is_some_and(|v| v.eq_ignore_ascii_case("hover"));
    Ok(Some(Jetpack {
        physics: Jet {
            jump, push,
            recharge: number("JetFuelRechargeRate")?,
            cost: number("JetFuelCost")?,
            initial: number("JetFuelInitialCost")?,
            min_border: number("JetFuelMinBorder")?,
            hover,
        },
        acceleration: number("JetAcceleration")?,
        energy_cost: number("JetEnergyDrain")?,
        force_jump: has_melee_weapon(record) && !hover,
    }))
}

/// A class's `ControlSpeed` row for one posture.
pub(crate) fn control_speed(record: &Value, mode: &str) -> Result<ControlSpeed, &'static str> {
    for (key, line) in pairs(record).rev() {
        if key.eq_ignore_ascii_case("ControlSpeed") {
            let mut words = line.split_ascii_whitespace();
            if words.next().is_some_and(|s| s.eq_ignore_ascii_case(mode)) {
                let mut next = || {
                    words
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .ok_or("invalid ControlSpeed")
                };
                return Ok(ControlSpeed {
                    forward: next()?,
                    strafe: next()?,
                    turn: next()?,
                });
            }
        }
    }
    Err("missing ControlSpeed")
}

/// The acceleration a class's body takes in place of the soldier default:
/// its last `Acceleration` property when that is finite and nonnegative.
pub fn body_acceleration(record: &Value) -> Option<f64> {
    pairs(record).rev().find(|(key, _)| key.eq_ignore_ascii_case("Acceleration"))
        .and_then(|(_, v)| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.)
}

/// The body a 2005 soldier class's units take: the soldier body of its
/// record, with its `Acceleration` property ([`body_acceleration`]).
pub fn soldier_body(record: &Value, ceiling: f64, getup: f64, dive: f64) -> crate::gen::Soldier {
    let mut body = crate::units::class::soldier_body(record, ceiling, getup, dive);
    if let Some(acceleration) = body_acceleration(record) {
        body.accel = acceleration;
    }
    body
}

/// A class's movement numbers from its record, the frames of its roll clip
/// and its two stand-up lengths.
pub fn tuning(
    record: &Value,
    roll_frames: u32,
    getup_front_seconds: f64,
    getup_back_seconds: f64,
) -> Result<Tuning, String> {
    let number = |key: &str| {
        pairs(record)
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.parse::<f64>().ok())
    };
    let need = |key: &str| number(key).ok_or_else(|| format!("missing infantry property {key}"));
    let speed = |mode: &str| control_speed(record, mode);
    let value = Tuning {
        stand: speed("stand")?,
        crouch: speed("crouch")?,
        jump: speed("jump")?,
        jet: speed("jet")?,
        sprint: speed("sprint")?,
        roll: speed("roll")?,
        tumble: speed("tumble")?,
        jetpack: jetpack(record)?,
        sprint_accelerate_time: need("SprintAccelerateTime")?,
        energy_capacity: need("EnergyBar")?,
        energy_floor: 0.,
        energy_restore: need("EnergyRestore")?,
        energy_restore_idle: need("EnergyRestoreIdle")?,
        energy_drain_sprint: need("EnergyDrainSprint")?,
        energy_min_sprint: need("EnergyMinSprint")?,
        energy_cost_jump: need("EnergyCostJump")?,
        energy_cost_jump_sprint: number("EnergyCostJumpSprint"),
        energy_cost_roll: need("EnergyCostRoll")?,
        jump_height: need("JumpHeight")?,
        jump_forward_speed_factor: need("JumpForwardSpeedFactor")?,
        jump_strafe_speed_factor: need("JumpStrafeSpeedFactor")?,
        roll_speed_factor: need("RollSpeedFactor")?,
        roll_frames,
        recover_from_tumble: number("RecoverFromTumble").is_some_and(|v| v != 0.),
        getup_front_seconds,
        getup_back_seconds,
    };
    value.validate().map_err(|e| e.to_string())?;
    Ok(value)
}

/// The frames of the forward roll a class plays: its movement slot's dive,
/// the same slot the display binds (`bfe_pose::character::stance`), from the
/// class's own clips (`side/name`) or else its body's. Zero when it has none.
pub fn roll_frames(content: &Content, side: &str, name: &str, body: Option<usize>) -> u32 {
    let body = body.and_then(|index| content.body_clips.get(index)).map(|(_, clips)| clips.as_ref());
    let clips = stance::movement_clips(content, &format!("{side}/{name}"), body);
    stance::RollSlot::movement(clips)
        .resolve("forward")
        .and_then(|roll| roll.layers[0])
        .map_or(0, |clip| u32::try_from(clip.frames).unwrap_or(u32::MAX))
}

/// The lengths, in seconds, of a class's face-down and face-up stand-up
/// clips (`stand_getupfront`, `stand_getupback`) from the same clips, or the
/// game's defaults where a class has none.
pub fn getup_seconds(content: &Content, side: &str, name: &str, body: Option<usize>) -> (f64, f64) {
    let body = body.and_then(|index| content.body_clips.get(index)).map(|(_, clips)| clips.as_ref());
    let clips = stance::movement_clips(content, &format!("{side}/{name}"), body);
    let length = |slot: &str, default: f64| {
        clips.and_then(|c| c.swbf2_slot(slot)).map(|c| c.duration).filter(|d| d.is_finite() && *d > 0.).unwrap_or(default)
    };
    (length("stand_getupfront", DEFAULT_GETUP_FRONT_SECONDS), length("stand_getupback", DEFAULT_GETUP_BACK_SECONDS))
}

/// A soldier class's movement numbers, or why it has none; None for a class
/// that is not a soldier.
pub fn of_class(content: &Content, defs: &Defs, class: &ClassDef) -> Option<Result<Tuning, String>> {
    if class.record.get("base").and_then(Value::as_str) != Some("soldier") {
        return None;
    }
    let (side, name) = (defs.names.text(class.side), defs.names.text(class.name));
    let (front, back) = getup_seconds(content, side, name, class.clips);
    Some(tuning(&class.record, roll_frames(content, side, name, class.clips), front, back))
}

/// Every soldier class of the battle, in the class list's order, with its
/// movement numbers or why it has none.
pub fn of_battle(content: &Content, defs: &Defs) -> Vec<(String, Result<Tuning, String>)> {
    defs.classes
        .iter()
        .filter_map(|class| Some((defs.names.text(class.name).to_string(), of_class(content, defs, class)?)))
        .collect()
}

/// Why the soldier class `name` has no movement numbers, as a clause for a
/// refusal; empty when the battle has no such soldier class.
pub fn refusal(content: &Content, defs: &Defs, name: &str) -> String {
    of_battle(content, defs).into_iter().find(|(class, _)| class == name)
        .and_then(|(_, tuning)| tuning.err()).map_or_else(String::new, |why| format!(": {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(properties: &[(&str, &str)]) -> Value {
        let rows: Vec<String> = properties.iter().map(|(k, v)| format!("[{k:?},{v:?}]")).collect();
        bfe_content::json::parse_bytes(format!(r#"{{"base":"soldier","properties":[{}]}}"#, rows.join(",")).as_bytes()).unwrap()
    }

    /// An invented class: every property the rule reads, once.
    fn complete() -> Vec<(&'static str, &'static str)> {
        vec![("ControlSpeed", "stand 1.0 0.8 1.1"), ("ControlSpeed", "crouch 0.5 0.4 0.9"), ("ControlSpeed", "jump 0.1 0.1 0.3"),
            ("ControlSpeed", "jet 0.6 0.6 0.6"), ("ControlSpeed", "sprint 1.7 0.4 0.5"), ("ControlSpeed", "roll 0.0 0.0 0.2"),
            ("ControlSpeed", "tumble 0.0 0.0 0.1"), ("SprintAccelerateTime", "0.4"), ("EnergyBar", "60"), ("EnergyRestore", "6"),
            ("EnergyRestoreIdle", "8"), ("EnergyDrainSprint", "18"), ("EnergyMinSprint", "12"), ("EnergyCostJump", "2"),
            ("EnergyCostRoll", "9"), ("JumpHeight", "1.5"), ("JumpForwardSpeedFactor", "1.1"), ("JumpStrafeSpeedFactor", "0.7"),
            ("RollSpeedFactor", "1.9")]
    }

    #[test]
    fn a_class_record_and_its_roll_clip_give_its_numbers() {
        let mut rows = complete();
        // A later row overrides an earlier one, as inheritance writes them.
        rows.push(("EnergyBar", "75"));
        rows.push(("controlspeed", "SPRINT 1.8 0.45 0.55"));
        let t = tuning(&record(&rows), 26, 1.2, 1.4).unwrap();
        assert_eq!((t.energy_capacity, t.sprint.forward, t.sprint.turn, t.roll_frames), (75., 1.8, 0.55, 26));
        assert_eq!((t.stand.strafe, t.energy_cost_jump_sprint, t.energy_floor), (0.8, None, 0.));
        assert_eq!((t.getup_front_seconds, t.getup_back_seconds, t.recover_from_tumble), (1.2, 1.4, false));
        rows.push(("EnergyCostJumpSprint", "3"));
        assert_eq!(tuning(&record(&rows), 26, 1.2, 1.4).unwrap().energy_cost_jump_sprint, Some(3.));
        // Each missing or unreadable property is named.
        let without = |key: &str| complete().into_iter().filter(|(k, _)| *k != key).collect::<Vec<_>>();
        assert_eq!(tuning(&record(&without("EnergyBar")), 26, 1.2, 1.4).unwrap_err(), "missing infantry property EnergyBar");
        assert_eq!(tuning(&record(&without("ControlSpeed")), 26, 1.2, 1.4).unwrap_err(), "missing ControlSpeed");
        let mut odd = complete();
        odd.push(("ControlSpeed", "roll 0.0 fast 0.2"));
        assert_eq!(tuning(&record(&odd), 26, 1.2, 1.4).unwrap_err(), "invalid ControlSpeed");
        let mut negative = complete();
        negative.push(("JumpHeight", "-1"));
        assert!(tuning(&record(&negative), 26, 1.2, 1.4).is_err());
    }

    // An invented jet class: an ignition energy cost or none, hover or not, its
    // own acceleration; a later row overrides an earlier one in any case; a
    // class with no jump and no push has no jetpack; an unreadable jet number
    // refuses.
    #[test]
    fn a_class_record_gives_its_jetpack() {
        let with = |extra: &[(&'static str, &'static str)]| {
            let mut rows = complete();
            rows.extend_from_slice(extra);
            tuning(&record(&rows), 26, 1.2, 1.4)
        };
        assert!(with(&[]).unwrap().jetpack.is_none());
        assert!(with(&[("JetType", "hover"), ("JetFuelCost", "0.2")]).unwrap().jetpack.is_none());
        let fuel = with(&[("JetJump", "4.75"), ("JetPush", "7.25"), ("JetType", "Hover"), ("JetFuelRechargeRate", "0.07"),
            ("JetFuelCost", "0.17"), ("JetFuelInitialCost", "0.31"), ("JetFuelMinBorder", "0.19"), ("JetAcceleration", "27")])
            .unwrap().jetpack.unwrap();
        assert_eq!((fuel.physics.jump, fuel.physics.push, fuel.physics.recharge, fuel.physics.cost, fuel.physics.initial,
            fuel.physics.min_border, fuel.physics.hover, fuel.acceleration), (4.75, 7.25, 0.07, 0.17, 0.31, 0.19, true, 27.));
        assert_eq!((fuel.energy_cost, fuel.force_jump), (0., false));
        let bare = with(&[("JetPush", "2"), ("JetType", "hover"), ("JetFuelCost", ".36")]).unwrap().jetpack.unwrap();
        assert_eq!((bare.physics.push, bare.physics.cost, bare.physics.hover, bare.energy_cost), (2., 0.36, true, 0.));
        let energy = with(&[("JetJump", "2"), ("jetjump", "9"), ("JetEnergyDrain", "13")]).unwrap().jetpack.unwrap();
        assert_eq!((energy.physics.jump, energy.physics.push, energy.physics.hover, energy.acceleration), (9., 0., false, 0.));
        assert_eq!(energy.energy_cost, 13.);
        assert_eq!(with(&[("JetPush", "3"), ("JetEnergyDrain", "0")]).unwrap().jetpack.unwrap().energy_cost, 0.);
        for (key, value) in [("JetJump", "NaN"), ("JetPush", "-1"), ("JetEnergyDrain", "fast"), ("JetFuelCost", "inf")] {
            let rows = [("JetJump", "4"), (key, value)];
            assert_eq!(with(&rows).unwrap_err(), format!("invalid infantry property {key}"), "{key} {value}");
        }
    }

    // The body takes the class's last readable Acceleration; the soldier
    // default stays where the class names none or an unreadable one.
    #[test]
    fn a_class_record_gives_its_body_acceleration() {
        let default = crate::units::class::soldier_body(&record(&complete()), 200., 0., 0.).accel;
        assert_eq!(soldier_body(&record(&complete()), 200., 0., 0.).accel, default);
        let mut rows = complete();
        rows.push(("Acceleration", "70"));
        rows.push(("acceleration", "90.0"));
        assert_eq!((body_acceleration(&record(&rows)), soldier_body(&record(&rows), 200., 0., 0.).accel), (Some(90.), 90.));
        rows.push(("Acceleration", "-5"));
        assert_eq!(body_acceleration(&record(&rows)), None);
        assert_eq!(soldier_body(&record(&rows), 200., 0., 0.).accel, default);
    }
}
