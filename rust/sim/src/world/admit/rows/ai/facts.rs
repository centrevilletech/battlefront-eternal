//! Authored immutable facts, porting the five *Facts functions in
//! ai-world-content.js. Missing and null remain distinct scalar variants.
use super::AiInterner;
use super::{error, numeric, present, string};
use crate::gen::*;
use crate::world::admit::AdmitError;
use bfe_content::value::Value;

/// The schema accepts primitives only. Silently stringifying an object
/// here would change both authored presence and native policy selection.
pub fn scalar(
    strings: &mut dyn AiInterner,
    value: Option<&Value>,
) -> Result<AiClassScalar, AdmitError> {
    Ok(match value {
        None => AiClassScalar::Undefined,
        Some(Value::Null) => AiClassScalar::Null,
        Some(Value::Bool(v)) => AiClassScalar::Bool(*v),
        Some(Value::Number(v)) => AiClassScalar::F64(*v),
        Some(Value::String(v)) => AiClassScalar::Str(strings.intern_str(v)),
        Some(_) => return Err(error("AI class scalar must be a primitive")),
    })
}

pub(crate) fn optional_string(
    strings: &mut dyn AiInterner,
    value: Option<&Value>,
) -> Option<StrId> {
    present(value).map(|v| strings.intern_str(&string(Some(v))))
}

pub(crate) fn authored_string(
    strings: &mut dyn AiInterner,
    value: Option<&Value>,
) -> Result<Option<StrId>, AdmitError> {
    present(value)
        .map(|v| {
            v.as_str()
                .map(|v| strings.intern_str(v))
                .ok_or_else(|| error("AI string field must be text"))
        })
        .transpose()
}

/// classFacts(identity, name, source): a hull's policy may read its raw
/// definition while each seat reads a different, cooked movement view.
pub fn class_facts(
    strings: &mut dyn AiInterner,
    def: DefId,
    identity: &Value,
    name: Option<&str>,
    source: &Value,
) -> Result<AiUnitClass, AdmitError> {
    let name = name
        .or_else(|| identity.get("class").and_then(Value::as_str))
        .ok_or_else(|| error("AI class has no string name"))?;
    let label = present(identity.get("label"))
        .or_else(|| identity.get("settings").and_then(|s| s.get("Label")));
    let mut row = AiUnitClass {
        def,
        name: strings.intern_str(name),
        label: authored_string(strings, label)?,
        ..Default::default()
    };
    // Intern in putAiUnitClass's field order. The object was built in a
    // different order, but the bridge does not intern until it writes it.
    row.no_enter_vehicles = scalar(strings, source.get("NoEnterVehicles"))?;
    row.role = scalar(strings, source.get("role"))?;
    row.ai_role = scalar(strings, source.get("ai").and_then(|ai| ai.get("role")))?;
    macro_rules! fields { ($($field:ident => $key:literal),* $(,)?) => { $(row.$field = scalar(strings, source.get($key))?;)* }; }
    fields! {
        unit_type => "UnitType", size_type => "AISizeType", health_type => "HealthType", base => "base",
        max_speed => "MaxSpeed", max_strafe_speed => "MaxStrafeSpeed", strafe_speed => "StrafeSpeed",
        max_turn_speed => "MaxTurnSpeed", max_pitch_speed => "MaxPitchSpeed", forward_speed => "ForwardSpeed",
        turn_rate => "TurnRate", pitch_rate => "PitchRate", moving_turn_only => "MovingTurnOnly", min_speed => "MinSpeed",
        max_yaw_speed => "MaxYawSpeed", vehicle_type => "VehicleType", landing_time => "LandingTime", landing_speed => "LandingSpeed",
        landed_height => "LandedHeight", max_health => "MaxHealth", jet_jump => "JetJump", jet_fuel_min_border => "JetFuelMinBorder",
        jet_fuel_recharge_rate => "JetFuelRechargeRate",
    }
    if let Some(bounds) = source.get("bounds").filter(|b| b.truthy()) {
        let low: [f64; 3] = numeric(bounds.at(0))?;
        let high: [f64; 3] = numeric(bounds.at(1))?;
        row.bounds = Some([low[0], low[1], low[2], high[0], high[1], high[2]]);
    }
    Ok(row)
}

const HEALTH_TYPES: [&str; 7] = [
    "Person",
    "Animal",
    "Droid",
    "Vehicle",
    "Building",
    "BuildingRebuild",
    "BuildingBuild",
];

/// itemFacts keeps authored service quantities, rather than F1's cooked
/// pickup gains. A present empty service table hides all named fields.
pub fn item_facts(
    strings: &mut dyn AiInterner,
    def: DefId,
    source: &Value,
) -> Result<AiItemDefinition, AdmitError> {
    let mut row = AiItemDefinition {
        def,
        base: authored_string(strings, source.get("base"))?,
        class_name: authored_string(strings, source.get("class"))?,
        ..Default::default()
    };
    let mut service = |name| -> Result<Option<Vec<AiClassScalar>>, AdmitError> {
        source
            .get("service")
            .and_then(|s| s.get(name))
            .filter(|v| v.truthy())
            .map(|v| {
                (0..7)
                    .map(|i| scalar(strings, v.at(i).or_else(|| v.get(&i.to_string()))))
                    .collect()
            })
            .transpose()
    };
    row.service_health = service("health")?;
    row.service_ammo = service("ammo")?;
    row.health = HEALTH_TYPES
        .iter()
        .map(|name| scalar(strings, source.get(&format!("{name}Health"))))
        .collect::<Result<_, _>>()?;
    row.ammo = HEALTH_TYPES
        .iter()
        .map(|name| scalar(strings, source.get(&format!("{name}Ammo"))))
        .collect::<Result<_, _>>()?;
    row.bare = HEALTH_TYPES
        .iter()
        .map(|name| scalar(strings, source.get(name)))
        .collect::<Result<_, _>>()?;
    Ok(row)
}

/// postFacts consumes attachPostAI's immutable record. The native post's
/// changing owner, activity and target life never enter this content row.
pub fn post_facts(
    strings: &mut dyn AiInterner,
    post: SpawnPost,
    name: Option<&Value>,
    desc: &Value,
    ai: &Value,
) -> Result<AiPostDefinition, AdmitError> {
    let truth = |v: Option<&Value>| v.is_some_and(Value::truthy);
    let path = |key| ai.get(key).and_then(|v| v.get("present"));
    let capture = ai
        .get("capture")
        .filter(|v| v.truthy())
        .map(|v| {
            Ok(AiCapture {
                center: numeric(v.get("center"))?,
                half: numeric(v.get("half"))?,
            })
        })
        .transpose()?;
    let raw_capture = desc
        .get("capture")
        .filter(|v| v.truthy())
        .map(|v| {
            Ok(AiRawCapture {
                center: present(v.get("center"))
                    .map(|v| numeric(Some(v)))
                    .transpose()?,
                matrix: present(v.get("matrix"))
                    .map(|v| {
                        v.as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|v| bfe_content::js::to_number(Some(v)))
                                    .collect()
                            })
                            .ok_or_else(|| error("AI capture matrix must be an array"))
                    })
                    .transpose()?,
                size: present(v.get("size"))
                    .map(|v| numeric(Some(v)))
                    .transpose()?,
                half: present(v.get("half"))
                    .map(|v| numeric(Some(v)))
                    .transpose()?,
            })
        })
        .transpose()?;
    Ok(AiPostDefinition {
        policy: AiPostPolicy {
            post,
            attack: numeric(ai.get("atk"))?,
            defend: numeric(ai.get("def"))?,
            bans: bfe_content::js::to_number(ai.get("bans")),
            spawn: truth(path("spawnPath")),
            ally: truth(path("allyPath")),
            class_spawn_points: bfe_content::js::to_number(ai.get("classSpawnPoints")),
        },
        name: authored_string(strings, name)?,
        order: present(ai.get("order")).map(|v| bfe_content::js::to_number(Some(v))),
        value_bleed: bfe_content::js::to_number(ai.get("valueBleed")),
        spawn_present: present(path("spawnPath")).map(Value::truthy),
        ally_present: present(path("allyPath")).map(Value::truthy),
        ally_count: bfe_content::js::to_number(ai.get("allyCount")),
        capture,
        radius: bfe_content::js::to_number(ai.get("radius")),
        yaw: bfe_content::js::to_number(ai.get("yaw")),
        raw_capture,
        spawn_path: truth(desc.get("spawnPath")),
        spawn_points: desc
            .get("spawnPoints")
            .and_then(Value::as_array)
            .map_or(0.0, |a| a.len() as f64),
        ally_path: truth(desc.get("allyPath")),
    })
}

/// propFacts keeps the registry target's own class separately from the
/// building description. A command post can have no AI registry member.
pub struct PropFacts<'a> {
    pub desc: &'a Value,
    pub def: DefId,
    pub target: RepairTarget,
    pub owner: F4Handle,
    pub member: Option<Member>,
    pub target_name: Option<&'a str>,
    pub target_class: Option<&'a Value>,
    pub team: f64,
    pub armed: bool,
    pub post: Option<SpawnPost>,
}

pub fn prop_facts(
    strings: &mut dyn AiInterner,
    input: &PropFacts<'_>,
) -> Result<AiPropDefinition, AdmitError> {
    let desc = input.desc;
    let empty = Value::Null;
    let name = match present(desc.get("class")) {
        None => "undefined",
        Some(v) => v
            .as_str()
            .ok_or_else(|| error("AI prop class name must be text"))?,
    };
    let policy = class_facts(
        strings,
        input.def,
        desc,
        Some(name),
        present(desc.get("settings")).unwrap_or(&empty),
    )?;
    let target_policy = input
        .target_class
        .map(|v| class_facts(strings, input.def, desc, input.target_name, v))
        .transpose()?;
    Ok(AiPropDefinition {
        target: input.target,
        owner: input.owner.clone(),
        member: input.member,
        policy,
        target_policy,
        team: input.team,
        armed: input.armed,
        command_post: desc.get("base").and_then(Value::as_str) == Some("commandpost"),
        post: input.post,
        yaw: optional_string(
            strings,
            desc.get("settings").and_then(|s| s.get("YawLimits")),
        ),
        pitch: optional_string(
            strings,
            desc.get("settings").and_then(|s| s.get("PitchLimits")),
        ),
    })
}
