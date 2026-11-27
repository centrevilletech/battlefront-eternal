//! Prepare remote.js class defaults once while content loads. Numeric handles
//! for explosions, ordnance and weapons are resolved by the owning world.
use super::*;
use crate::combat::defs::Js;
fn vector(def: &Js, field: &str, default: [f64; 3]) -> [f64; 3] {
    let value = def.get(field);
    if value.truthy() {
        [value.at(0).num(), value.at(1).num(), value.at(2).num()]
    } else {
        default
    }
}
impl DroidDef {
    pub fn read(def: &Js, loadout: Vec<DefId>, explosion: Option<u32>, muzzle: [f64; 3]) -> Self {
        let pitch = def.get("PitchLimits");
        let threshold = def.get("CollisionThreshold");
        let scale = def.get("CollisionScale");
        let center = vector(
            def,
            "TrackCenter",
            vector(def, "EyePointOffset", [0.0, 0.5, 0.0]),
        );
        let guided = (def.get("GuidedMissile").num() > 0.0).then(|| {
            let min_speed = max(def.get("MinSpeed").nn(0.0), 0.0);
            let max_speed = max(def.get("MaxSpeed").nn(min_speed), min_speed);
            let boost_speed = max(def.get("BoostSpeed").nn(max_speed), max_speed);
            let accel = max(def.get("Acceleraton").nn(0.0), 0.0);
            let boost_accel = max(def.get("BoostAcceleration").nn(accel), 0.0);
            let direct_damage = max(def.get("GuidedMissileDamage").nn(0.0), 0.0);
            let boost_damage = max(def.get("GuidedMissileDamageBoost").nn(direct_damage), direct_damage);
            let boost_threshold = clamp(
                def.get("GuidedMissileBoostExplosionSpeedThreshold")
                    .nn(crate::space::flight::PROVISIONAL_WEAPON_DEFAULTS.guided_boost_threshold),
                0.0,
                1.0,
            );
            GuidedMissileDef {
                min_speed,
                max_speed,
                boost_speed,
                accel,
                boost_accel,
                energy_bar: max(def.get("EnergyBar").nn(0.0), 0.0),
                energy_restore: def.get("EnergyAutoRestore").nn(0.0),
                boost_drain: max(def.get("EnergyBoostDrain").nn(0.0), 0.0),
                pitch_rate: max(def.get("PitchRate").nn(0.0), 0.0),
                turn_rate: max(def.get("TurnRate").nn(0.0), 0.0),
                direct_damage,
                boost_damage,
                boost_threshold,
                explosion_critical: None,
            }
        });
        Self {
            health: def.get("MaxHealth").nn(40.0),
            add_health: def.get("AddHealth").nn(0.0),
            altitude: def.get("SetAltitude").nn(1.75),
            gravity: def.get("GravityScale").nn(1.0),
            spring: def.get("LiftSpring").nn(5.0),
            damp: def.get("LiftDamp").nn(2.0),
            acceleration: def.get("Acceleraton").nn(70.0),
            speed: def.get("MaxSpeed").nn(9.0),
            strafe: def.get("MaxStrafeSpeed").nn(7.0),
            turn: def.get("MaxTurnSpeed").nn(2.0),
            pitch: if pitch.truthy() {
                [pitch.at(0).num(), pitch.at(1).num()]
            } else {
                [-50.0, 50.0]
            },
            threshold: if threshold.truthy() {
                [threshold.at(0).num(), threshold.at(1).num()]
            } else {
                [7.5, 2.5]
            },
            collision_scale: if matches!(scale, Js::Arr(_)) {
                scale.at(0).num()
            } else {
                scale.nn(10.0)
            },
            eye: vector(def, "EyePointOffset", [0.0, 0.5, 0.0]),
            center,
            offset: vector(def, "TrackOffset", [0.0, -0.25, 1.5]),
            tilt: def.get("TiltValue").nn(5.0),
            muzzle,
            third_fov: def.get("ThirdPersonFOV").nn(48.0) * (std::f64::consts::PI / 180.0),
            first_fov: def.get("FirstPersonFOV").nn(48.0) * (std::f64::consts::PI / 180.0),
            explosion,
            loadout,
            guided,
        }
    }
}
impl StrikeDef {
    pub fn read(def: &Js, ordnance: u32) -> Self {
        Self {
            ordnance,
            elevation: def.get("AimElevation").nn(90.0),
            azimuth: def.get("AimAzimuth").nn(0.0),
            count: def.get("SalvoCount").or(1.0),
            delay: def.get("SalvoDelay").or(0.0),
            scatter: def.get("ScatterDistance").or(0.0),
            distance: def.get("AimDistance").nn(100.0),
        }
    }
}
impl WeaponLevel {
    pub fn read(def: &Js, ordnance: Option<u32>, strike: Option<u32>) -> Self {
        Self {
            ordnance,
            strike,
            pressed_time: def.get("MaxPressedTime").or(3.0),
            zoom_min: def.get("ZoomMin").or(1.0),
            zoom_max: def.get("ZoomMax").or(1.0),
        }
    }
}
