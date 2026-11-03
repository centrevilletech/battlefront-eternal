//! What a hull's tick reads of its class, from the battle's content
//! (rust/content): the hover's keys and springs (src/vehicle-hover.js
//! hoverClass), a walker's legs (`fx.legs`), each seat's aim rates for a bot
//! (aimCrew), and what its destruction reads (setHealth: `fx.dying.span`,
//! `fx.explosionOffset`, `explosion`). The hull's own fields (its speeds,
//! box, probes) are the store's (rust/sim/schema/vehicles.mjs).

use super::hull::{HoverClass, Legs, Spring};
use bfe_content::odf::{class_setting, num};
use bfe_content::value::Value;
use three::Vector3;

/// A hull class's keys for the tick.
#[derive(Clone, Debug, Default)]
pub struct HullClass {
    pub hover: HoverClass,
    pub legs: Option<Legs>,
    /// Each seat's (TurnRate, PitchRate) as aimCrew reads them, over the
    /// class's and, for the turn, the hull's own turn rate.
    pub seat_rates: Vec<(f64, f64)>,
    pub dying_span: f64,
    pub explosion_offset: Option<[f64; 3]>,
    pub has_explosion: bool,
    /// dyingTick's blast: `finalExplosion || explosion`, at `fx.dying.finalOffset || [0, 0, 0]`.
    pub has_final_explosion: bool,
    pub final_offset: [f64; 3],
}

fn number(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// JavaScript's truthiness of a content value.
fn truthy(v: Option<&Value>) -> bool {
    v.is_some_and(Value::truthy)
}

impl HullClass {
    /// The class `def` (the vehicle pack's), for a hull whose own turn rate
    /// is `turn_rate`.
    pub fn new(def: &Value, turn_rate: f64) -> HullClass {
        let n = |name: &str, fallback: f64| num(class_setting(Some(def), name), fallback);
        let springs = def.get("springs").and_then(Value::as_array).unwrap_or(&[]);
        let hover = HoverClass {
            altitude: n("SetAltitude", 1.0),
            gravity_scale: n("GravityScale", 1.0),
            lift_spring: n("LiftSpring", 5.0),
            lift_damp: n("LiftDamp", 2.0),
            traction: n("Traction", 10.0),
            level_spring: n("LevelSpring", 10.0),
            level_damp: n("LevelDamp", 3.0),
            velocity_spring: n("VelocitySpring", 0.0),
            velocity_damp: n("VelocityDamp", 0.0),
            omega_x_spring: n("OmegaXSpring", 0.0),
            omega_x_damp: n("OmegaXDamp", 0.0),
            omega_z_spring: n("OmegaZSpring", 0.0),
            omega_z_damp: n("OmegaZDamp", 0.0),
            springs: springs
                .iter()
                .take(super::hover::SPRINGS)
                .map(|s| {
                    let at = s.get("at").and_then(Value::as_array).unwrap_or(&[]);
                    let factors = s.get("factors").and_then(Value::as_array).unwrap_or(&[]);
                    let f = |i: usize| number(factors.get(i));
                    Spring {
                        at: Vector3::new(number(at.first()), number(at.get(1)), number(at.get(2))),
                        radius: number(s.get("radius")),
                        length: number(s.get("length")),
                        factors: [f(0), f(1), f(2), f(3), f(4), f(5)],
                    }
                })
                .collect(),
        };
        let fx = def.get("fx");
        let legs = fx.and_then(|f| f.get("legs")).filter(|l| l.truthy()).map(|l| Legs {
            ray_length: number(l.get("rayLength")),
            orient_roll: truthy(l.get("orientRoll")),
            width: number(l.get("width")),
            pairs: l
                .get("pairs")
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .map(|p| {
                    let at = |which: &str| {
                        let a = p.get(which).and_then(|f| f.get("at")).and_then(Value::as_array).unwrap_or(&[]);
                        [number(a.first()), number(a.get(1)), number(a.get(2))]
                    };
                    [at("left"), at("right")]
                })
                .collect(),
        });
        let settings = def.get("settings");
        let class_turn = num(settings.and_then(|s| s.get("TurnRate")), turn_rate);
        let class_pitch = num(settings.and_then(|s| s.get("PitchRate")), 1.0);
        let seat_rates = def
            .get("seats")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|seat| {
                let s = seat.get("settings");
                (num(s.and_then(|s| s.get("TurnRate")), class_turn), num(s.and_then(|s| s.get("PitchRate")), class_pitch))
            })
            .collect();
        let dying = fx.and_then(|f| f.get("dying")).and_then(|d| d.get("span"));
        let dying_span = match dying.and_then(Value::as_f64) {
            Some(x) if !(x == 0.0 || x.is_nan()) => x,
            _ => 0.0,
        };
        let explosion_offset = fx.and_then(|f| f.get("explosionOffset")).filter(|o| o.truthy()).map(|o| {
            let a = o.as_array().unwrap_or(&[]);
            [number(a.first()), number(a.get(1)), number(a.get(2))]
        });
        let final_offset = fx.and_then(|f| f.get("dying")).and_then(|d| d.get("finalOffset")).filter(|o| o.truthy()).map_or([0.0, 0.0, 0.0], |o| {
            let a = o.as_array().unwrap_or(&[]);
            [number(a.first()), number(a.get(1)), number(a.get(2))]
        });
        HullClass {
            hover,
            legs,
            seat_rates,
            dying_span,
            explosion_offset,
            has_explosion: truthy(def.get("explosion")),
            has_final_explosion: truthy(def.get("finalExplosion")) || truthy(def.get("explosion")),
            final_offset,
        }
    }
}
