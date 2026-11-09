//! The command vehicles' posts as createNativeAI makes them before its
//! first call (ai-world-load.js: `world.posts.attachVehicles(vehicleSystem,
//! mission.commandClasses)`, src/mobile-posts.js): one MobilePost for every
//! vehicle pad whose classes include a command vehicle (ODF base
//! commandwalker or commandhover), in pad order, each presented once
//! (`post.reported = state(post.present())`) over the hulls the fleet
//! constructed on its pad.
use super::super::{fleet, Call, Cx};
use super::{error, mission_calls};
use crate::gen::{MobileAi, MobileDesc, MobilePost, Reported};
use crate::world::admit::AdmitError;
use crate::world::owner::World;
use bfe_content::{js::to_number, Value};

/// isCommandClass: a command walker or a command hover.
pub fn command_class(def: Option<&Value>) -> bool {
    matches!(def.and_then(|d| d.get("base")).and_then(Value::as_str), Some("commandwalker" | "commandhover"))
}

/// Number.parseInt(value, 10): the value as a string, leading whitespace
/// and an optional sign, then decimal digits; NaN without any digit.
pub fn parse_int(value: Option<&Value>) -> f64 {
    let text = match value {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Number(n)) => bfe_content::js::number_to_string(*n),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".into(),
        _ => return f64::NAN,
    };
    let trimmed = text.trim_start();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let run: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if run.is_empty() {
        return f64::NAN;
    }
    let magnitude = run.parse::<f64>().unwrap_or(f64::NAN);
    if negative { -magnitude } else { magnitude }
}

/// classData(def, record): the mission's commandClasses record for the
/// class (weights, ValueBleed, spawn points), else the vehicle class's own
/// ValueBleed with the default weights and no spawn points.
fn class_data(def: Option<&Value>, record: Option<&Value>) -> MobileAi {
    let defaults = || vec![100.0; 6];
    let numbers = |v: Option<&Value>| v.and_then(Value::as_array).map(|a| a.iter().map(|x| to_number(Some(x))).collect::<Vec<f64>>());
    match record {
        Some(record) => {
            let values = record.get("values");
            let points: Vec<[f64; 4]> = record.get("spawnPoints").and_then(Value::as_array).unwrap_or(&[]).iter()
                .map(|p| std::array::from_fn(|k| p.at(k).filter(|v| !v.is_null()).map_or(0.0, |v| to_number(Some(v)))))
                .collect();
            MobileAi {
                atk: numbers(values.and_then(|v| v.get("atk"))).unwrap_or_else(defaults),
                def: numbers(values.and_then(|v| v.get("def"))).unwrap_or_else(defaults),
                value_bleed: record.get("valueBleed").filter(|v| !v.is_null()).map_or(100.0, |v| to_number(Some(v))),
                class_spawn_points: points.len() as f64,
                points,
            }
        }
        None => {
            let bleed = parse_int(def.and_then(|d| d.get("settings")).and_then(|s| s.get("ValueBleed")));
            MobileAi { atk: defaults(), def: defaults(), value_bleed: if bleed.is_finite() { bleed } else { 100.0 }, points: Vec::new(), class_spawn_points: 0.0 }
        }
    }
}

/// `new MobilePosts(vehicleSystem, commandClasses)` then each post's first
/// present(). Texts are interned through `interns` in the generated
/// writer's field order (name, label, then the desc's name and display
/// name), as the bridge interns them when it writes the import.
pub fn mobile_posts(world: &World, cx: &mut Cx, interns: &mut Call) -> Result<Vec<MobilePost>, AdmitError> {
    let battle = cx.battle;
    let Some(data) = battle.vehicle_def.as_ref() else { return Ok(Vec::new()); };
    let pads = data.get("spawns").and_then(Value::as_array).map(<[Value]>::to_vec).unwrap_or_default();
    let classes = data.get("classes");
    let (mission, _) = mission_calls(battle);
    let records = mission.as_ref().and_then(|m| m.get("commandClasses")).cloned();
    let plans = fleet::hulls(data, &battle.world)?;
    if plans.len() != cx.ids.hulls.len() {
        return Err(error("mobile posts need every hull's F3 binding"));
    }
    let mut out = Vec::new();
    for (index, pad) in pads.iter().enumerate() {
        let names: Vec<&str> = pad.get("classes").and_then(Value::as_object).map(|c| c.values().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !names.iter().any(|name| command_class(classes.and_then(|c| c.get(name)))) {
            continue;
        }
        // MobilePost.hull(): the last living command hull among the hulls
        // made for this pad (slot.variants, in construction order; the
        // pad's live vehicle is one of them). Prepared variants are not
        // slot variants.
        let mut found = None;
        for (plan, binding) in plans.iter().zip(&cx.ids.hulls) {
            if plan.pad != index || plan.prepared { continue; }
            let def = classes.and_then(|c| c.get(&plan.name));
            let Some(row) = world.sim.store.vehicle.get(binding.setup.hull) else { continue; };
            if row.alive && command_class(def) { found = Some((plan, binding.setup.hull, row, def)); }
        }
        let name = cx.intern(interns, &format!("transport-{index}"));
        let post = match found {
            None => {
                // No command hull lives: an inactive post at its defaults.
                let label = cx.intern(interns, "");
                let ai = class_data(None, None);
                MobilePost { index: index as f64, name, label, team: 0.0, active: false, mobile: true, vehicle: None, life: None,
                    yaw: 0.0, radius: 0.0, speed: 0.0, last_position: [0.0; 3],
                    desc: MobileDesc { name: label, display_name: label, value_bleed: 100.0, mobile: true },
                    ai: MobileAi { value_bleed: 100.0, ..ai },
                    reported: Reported { active: false, team: 0.0, vehicle: None, life: None } }
            }
            Some((plan, hull, row, def)) => {
                let record = records.as_ref().and_then(|r| r.get(&plan.name)).filter(|v| v.truthy());
                let ai = class_data(def, record);
                // `v.team | 0`; v.label is the class's label, else its name.
                let team = f64::from(jsrt::to_int32(row.team.unwrap_or(0.0)));
                let text = def.and_then(|d| d.get("label")).filter(|v| v.truthy()).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| plan.name.clone());
                let label = cx.intern(interns, &text);
                let class_name = cx.intern(interns, &plan.name);
                let life = row.target_generation;
                // Vector3.length: Math.sqrt(x*x + y*y + z*z), halved.
                let radius = (row.size[0] * row.size[0] + row.size[1] * row.size[1] + row.size[2] * row.size[2]).sqrt() / 2.0;
                // No seat is leased at admission, so the hull's own team
                // stands (v.controllingTeam).
                MobilePost { index: index as f64, name, label, team, active: true, mobile: true, vehicle: Some(hull), life: Some(life),
                    yaw: row.yaw, radius, speed: row.speed, last_position: [0.0; 3],
                    desc: MobileDesc { name: class_name, display_name: label, value_bleed: ai.value_bleed, mobile: true },
                    ai, reported: Reported { active: true, team, vehicle: Some(hull), life: Some(life) } }
            }
        };
        out.push(post);
    }
    Ok(out)
}
