//! setupRows and the roster program in createNativeAI.
//! Construction receipts carry ids and counts from earlier owners, while
//! the mission's authored call sequence stays in its original order.
use super::AiInterner;
use super::{error, optional_string, present, scalar, string};
use crate::gen::*;
use crate::world::admit::{AdmitError, Options};
use bfe_content::battle::Battle;
use bfe_content::files::Files;
use bfe_content::js::{js_max, to_number};
use bfe_content::mission::select_ai_mission;
use bfe_content::value::{Object, Value};
use std::rc::Rc;

/// The original JSON bytes, not JSON.stringify(parsed). Native navigation
/// and weapon policy parsers receive the host's exact file payloads.
#[derive(Clone, Debug, Default)]
pub struct AiAssets {
    pub navigation: Vec<u8>,
    pub navigation_bin: Vec<u8>,
    pub weapon_data: Vec<AiWeaponData>,
}

impl AiAssets {
    /// loadAIData/loadAIWeaponData use optional loads. An empty successful
    /// ai.json remains present; its native parser decides whether it is valid.
    pub fn read(battle: &Battle, files: &dyn Files) -> Result<Self, AdmitError> {
        let (navigation, navigation_bin) = if battle.family == bfe_content::GameFamily::Swbf2 {
            let navigation = encode_swbf2_nav(battle)?;
            let navigation_bin = if navigation.is_empty() {
                Vec::new()
            } else {
                battle
                    .swbf2_planning_bin()
                    .map(|b| b.to_vec())
                    .unwrap_or_default()
            };
            (navigation, navigation_bin)
        } else {
            let nav = format!("maps/{}/ai-nav.json", battle.map);
            let navigation = if battle.nav.is_some() {
                files
                    .read(&nav)
                    .ok_or_else(|| error("original AI navigation bytes unavailable"))?
                    .to_vec()
            } else {
                Vec::new()
            };
            let navigation_bin = battle
                .nav_bin
                .as_ref()
                .map(|b| b.to_vec())
                .unwrap_or_default();
            (navigation, navigation_bin)
        };
        let weapon_data = super::super::weapon_policies::source(battle, files, &[]).data;
        Ok(Self {
            navigation,
            navigation_bin,
            weapon_data,
        })
    }
}

fn encode_swbf2_nav(battle: &Battle) -> Result<Vec<u8>, AdmitError> {
    let Some(swbf2) = battle.swbf2.as_ref() else {
        return Ok(Vec::new());
    };
    let adm = &swbf2.map_admission;
    let map_data = &adm.map_data;
    let opt_field = |key: &str| map_data.get(key).filter(|v| !v.is_null());
    let plan = opt_field("plan");
    let groups = opt_field("planningGroups");
    let boundary_paths = opt_field("boundaryPaths");
    let mut out = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(br#"{"schema":2,"map":"#);
    let _ = serde_json::to_writer(&mut out, &battle.map);
    for (key, val) in [
        ("plan", plan),
        ("bounds", opt_field("bounds")),
        ("planningGroups", groups),
        ("boundaryPaths", boundary_paths),
    ] {
        if let Some(v) = val {
            out.push(b',');
            let _ = serde_json::to_writer(&mut out, key);
            out.push(b':');
            write_value_json(v, &mut out);
        }
    }
    write_navigation_records(map_data, &adm.active_layers, &mut out)?;
    out.extend_from_slice(br#","paths":["#);
    for (i, p) in adm.paths.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        write_value_json(p, &mut out);
    }
    out.extend_from_slice(br#"],"regions":["#);
    for (i, r) in adm.regions.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        write_value_json(r, &mut out);
    }
    out.extend_from_slice(b"]}");
    Ok(out)
}

fn write_navigation_records(map: &Value, active: &[String], out: &mut Vec<u8>) -> Result<(), AdmitError> {
    use crate::ai::nav::swbf2::records::RecordLimits;
    let mut limits = RecordLimits::default();
    for key in ["barriers", "hints"] {
        out.push(b',');
        serde_json::to_writer(&mut *out, key).unwrap();
        out.extend_from_slice(b":[");
        let layers = map.get("layers");
        let scopes = std::iter::once((None, map)).chain(active.iter().filter_map(|layer| {
            Some((Some(layer.as_str()), layers?.get(layer)?))
        }));
        let mut first = true;
        for (layer, scope) in scopes {
            let Some(value) = scope.get(key) else { continue };
            let rows = value.as_array().ok_or_else(|| error("navigation records must be an array"))?;
            limits.add(key, rows).map_err(error)?;
            for (index, row) in rows.iter().enumerate() {
                RecordLimits::check_index(row, index).map_err(error)?;
                if row.get("sourceLayer").is_some() {
                    return Err(error("navigation source layer is supplied by the containing package"));
                }
                if !first { out.push(b','); }
                first = false;
                if let Some(layer) = layer {
                    limits.add_layer_name(layer).map_err(error)?;
                    let mut fields = row.as_object().expect("validated navigation record").clone();
                    fields.set(Rc::from("sourceLayer"), Value::str(layer));
                    write_value_json(&Value::Object(Rc::new(fields)), out);
                } else {
                    write_value_json(row, out);
                }
            }
        }
        out.push(b']');
    }
    Ok(())
}

fn write_value_json(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if n.is_finite() {
                out.extend_from_slice(bfe_content::js::number_to_string(*n).as_bytes());
            } else {
                out.extend_from_slice(b"null");
            }
        }
        Value::String(s) => {
            let _ = serde_json::to_writer(&mut *out, s.as_ref());
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value_json(item, out);
            }
            out.push(b']');
        }
        Value::Object(obj) => {
            out.push(b'{');
            for (i, (k, val)) in obj.entries().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let _ = serde_json::to_writer(&mut *out, k);
                out.push(b':');
                write_value_json(val, out);
            }
            out.push(b'}');
        }
    }
}

/// createNativeAI creates B from the match seed without drawing it. A is
/// the existing shared generator; cold construction may already have used A.
pub fn streams(seed: u32, state_a: u32) -> AiStreamState {
    AiStreamState {
        a: state_a,
        b: seed ^ 0x9e3779b9,
        c: seed ^ 0x51ed270b,
    }
}

/// setupRows observes these values after F3 and the fleet finish their
/// cold bodies. The A state can have advanced during that construction.
#[derive(Clone, Debug)]
pub struct SetupFacts {
    pub stream_a: u32,
    pub stream_b: u32,
    pub state_a: u32,
    pub state_b: u32,
    pub next_unit_id: u32,
    pub hull_seats: Vec<u32>,
    pub buildings: u32,
    pub classes: u32,
    pub posts: u32,
    /// Navigation uses only static posts, even when content.posts also
    /// contains mobile command posts.
    pub navigation_posts: Vec<AiNavPost>,
    pub unit_scale: f64,
    pub tier: f64,
}

impl Default for SetupFacts {
    fn default() -> Self {
        Self {
            stream_a: 0,
            stream_b: 0,
            state_a: 0,
            state_b: 0,
            next_unit_id: 0,
            hull_seats: Vec::new(),
            buildings: 0,
            classes: 0,
            posts: 0,
            navigation_posts: Vec::new(),
            unit_scale: 1.0,
            tier: 2.0,
        }
    }
}

/// callsFromMapMission is used only when the selected AI script is absent.
/// Its object iteration order differs from sorting by a team's numeric id.
pub fn calls_from_map_mission(mission: Option<&Value>) -> Vec<Value> {
    let mut calls = Vec::new();
    let mut push = |name: &str, args: Vec<Value>| {
        calls.push(Value::Object(Rc::new(Object::from_written(vec![
            (Rc::from("name"), Value::str(name)),
            (Rc::from("args"), Value::Array(Rc::new(args))),
        ]))));
    };
    if let Some(teams) = mission
        .and_then(|m| m.get("teams"))
        .and_then(Value::as_object)
    {
        for (team, t) in teams.entries() {
            let n = Value::Number(to_number(Some(&Value::str(team))));
            if let Some(name) = t.get("name").filter(|v| v.truthy()) {
                push("SetTeamName", vec![n.clone(), name.clone()]);
            }
            for class in t.get("classes").and_then(Value::as_array).unwrap_or(&[]) {
                push(
                    "AddUnitClass",
                    vec![
                        n.clone(),
                        class.at(0).cloned().unwrap_or(Value::Null),
                        class.at(1).cloned().unwrap_or(Value::Null),
                    ],
                );
            }
            if let Some(hero) = t.get("hero").filter(|v| v.truthy()) {
                push("SetHeroClass", vec![n.clone(), hero.clone()]);
            }
            for (field, call) in [
                ("units", "SetUnitCount"),
                ("reinforcements", "SetReinforcementCount"),
            ] {
                if let Some(value) = present(t.get(field)) {
                    push(call, vec![n.clone(), value.clone()]);
                }
            }
        }
    }
    if let Some(delay) = mission
        .and_then(|m| m.get("spawnDelay"))
        .filter(|v| v.truthy())
    {
        push("SetSpawnDelay", delay.as_array().unwrap_or(&[]).to_vec());
    }
    if let Some(attacker) = present(mission.and_then(|m| m.get("attacker"))) {
        push("SetAttackingTeam", vec![attacker.clone()]);
    }
    calls
}

pub fn mission_calls(battle: &Battle) -> (Option<Value>, Vec<Value>) {
    let mission = select_ai_mission(battle.ai_mission.as_ref(), battle.world.get("mission"));
    let authored = mission
        .as_ref()
        .and_then(|m| m.get("mission"))
        .and_then(Value::as_str)
        .and_then(|name| {
            mission
                .as_ref()
                .and_then(|m| m.get("scripts"))
                .and_then(|s| s.get(name))
        })
        .and_then(|s| s.get("calls"))
        .filter(|v| !v.is_null());
    let calls = authored
        .and_then(Value::as_array)
        .map(<[Value]>::to_vec)
        .unwrap_or_else(|| calls_from_map_mission(battle.world.get("mission")));
    (mission, calls)
}

fn count(value: f64, name: &str) -> Result<u32, AdmitError> {
    if value.is_finite() && value >= 0.0 && value <= u32::MAX as f64 {
        Ok(jsrt::to_uint32(value))
    } else {
        Err(error(format!("AI {name} capacity is out of range")))
    }
}

pub fn setup_rows(
    strings: &mut dyn AiInterner,
    options: &Options,
    player_team: f64,
    calls: &[Value],
    facts: &SetupFacts,
    assets: &AiAssets,
) -> Result<AiWorldSetup, AdmitError> {
    let mut locals = 0.0;
    let mut total = 0.0;
    for call in calls
        .iter()
        .filter(|c| c.get("name").and_then(Value::as_str) == Some("SetUnitCount"))
    {
        let args = call.get("args");
        let scaled = js_max(
            0.0,
            (to_number(args.and_then(|a| a.at(1))) * facts.unit_scale).ceil(),
        );
        total += scaled;
        if to_number(args.and_then(|a| a.at(0))) > 2.0 {
            locals += scaled;
        }
    }
    let characters = js_max(65.0 + locals, total + 32.0);
    let hulls = facts.hull_seats.len() as f64;
    let buildings = facts.buildings as f64;
    // The AI's queries and identities cover every body the unit factory may
    // build (joins and class switches add bodies), plus the hull targets.
    let registry = js_max(
        js_max(256.0, facts.next_unit_id as f64 + characters * 2.0 + hulls + buildings + 64.0),
        facts.next_unit_id as f64 + crate::world::admit::rows::battle::stock::FACTORY_HEADROOM as f64 + js_max(8.0, hulls),
    );
    let seats: f64 = facts.hull_seats.iter().map(|n| *n as f64).sum();
    let capacity = AiCapacity {
        registry: count(registry, "registry")?,
        characters: count(characters, "characters")?,
        posts: facts.posts,
        hulls: count(js_max(8.0, hulls), "hulls")?,
        buildings: facts.buildings,
        controls: count(
            js_max(characters * 2.0, seats * 2.0 + buildings),
            "controls",
        )?,
        grenades: 4096,
        scopes: 16,
        cameras: 64,
        support_keys: count(
            js_max(16.0, facts.classes as f64 + hulls + buildings),
            "support keys",
        )?,
        repair_jobs: count(registry, "repair jobs")?,
    };
    // setupRows uses Number(value)||0, including replacing numeric -0 by
    // positive zero. The roster below retains the original scalar instead.
    let mission = calls
        .iter()
        .map(|call| {
            let args = call.get("args").and_then(Value::as_array).unwrap_or(&[]);
            AiMissionCall {
                name: strings.intern_str(&string(call.get("name"))),
                args: args
                    .iter()
                    .map(|v| {
                        let n = to_number(Some(v));
                        if n == 0.0 || n.is_nan() {
                            0.0
                        } else {
                            n
                        }
                    })
                    .collect(),
                strings: args
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|v| strings.intern_str(v))
                    .collect(),
            }
        })
        .collect();
    Ok(AiWorldSetup {
        settings: AiColdSettings {
            seed: options.seed,
            enhanced: options.enhanced,
            tier: facts.tier,
            profile_tier: facts.tier,
            player_team: Some(player_team),
            stream_a: facts.stream_a,
            stream_b: facts.stream_b,
            streams: Some(AiStreamState {
                a: facts.state_a,
                b: facts.state_b,
                c: options.seed ^ 0x51ed270b,
            }),
            player_skill: 4.0,
            enemy_skill: 14.0,
            sensor_range: 75.0,
            mission,
            capacity,
            capabilities: AiCapabilities {
                navigation: true,
                combat: true,
                combat_agent: true,
                population: true,
                vehicles: true,
                classes: true,
                random_b: true,
                mine_ground: true,
                vehicle_adapter: true,
            },
            ..Default::default()
        },
        navigation: AiNavSetup {
            json: assets.navigation.clone(),
            bin: assets.navigation_bin.clone(),
            posts: facts.navigation_posts.clone(),
            pools: AiNavPools {
                paths: 900.0,
                path_nodes: 7200.0,
                requests: 75.0,
                ..Default::default()
            },
            ..Default::default()
        },
    })
}

/// The roster's prepared names are side-table keys. heroNames instead
/// reads def.class, so it must not be inferred from those same keys.
pub struct PreparedClass<'a> {
    pub name: &'a str,
    pub definition: &'a Value,
    pub id: DefId,
}

pub fn roster_rows(
    strings: &mut dyn AiInterner,
    options: &Options,
    player_team: f64,
    unit_scale: f64,
    calls: &[Value],
    mission: Option<&Value>,
    prepared: &[PreparedClass<'_>],
    pads: Vec<F4PadSetup>,
) -> Result<AiRosterSetup, AdmitError> {
    roster_rows_with_pads(
        strings,
        options,
        player_team,
        unit_scale,
        calls,
        mission,
        prepared,
        |_| Ok(pads),
    )
}

/// A pad publisher that still needs to intern text runs at the generated
/// writer's pad field, after prepared names and before hero display names.
pub fn roster_rows_with_pads(
    strings: &mut dyn AiInterner,
    options: &Options,
    player_team: f64,
    unit_scale: f64,
    calls: &[Value],
    mission: Option<&Value>,
    prepared: &[PreparedClass<'_>],
    pads: impl FnOnce(&mut dyn AiInterner) -> Result<Vec<F4PadSetup>, AdmitError>,
) -> Result<AiRosterSetup, AdmitError> {
    let calls = calls
        .iter()
        .map(|call| {
            Ok(AiRosterCall {
                name: strings.intern_str(&string(call.get("name"))),
                args: call
                    .get("args")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .map(|v| scalar(strings, Some(v)))
                    .collect::<Result<_, _>>()?,
                faction: present(call.get("faction")).map(|v| to_number(Some(v))),
                mask: present(call.get("mask")).map(|v| to_number(Some(v))),
            })
        })
        .collect::<Result<_, AdmitError>>()?;
    let names = mission.and_then(|m| m.get("names"));
    let mut sides = Vec::new();
    for index in 0..5 {
        let side = names.and_then(|v| v.at(index).or_else(|| v.get(&index.to_string())));
        let mut list = |key| {
            side.and_then(|s| s.get(key))
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .map(|v| strings.intern_str(&string(Some(v))))
                .collect()
        };
        sides.push(AiRosterSideNames {
            first: list("first"),
            last: list("last"),
        });
    }
    let local = optional_string(strings, names.and_then(|n| n.get("local")));
    let prepared_names = prepared
        .iter()
        .map(|v| strings.intern_str(v.name))
        .collect();
    let pads = pads(strings)?;
    let hero_names = prepared
        .iter()
        .map(|v| F4HeroSetup {
            class_name: strings.intern_str(&string(v.definition.get("class"))),
            name: optional_string(strings, v.definition.get("displayName")),
        })
        .collect();
    Ok(AiRosterSetup {
        calls,
        names: sides,
        local,
        humans: vec![AiRosterHuman {
            team: player_team,
            id: None,
            name: None,
        }],
        unit_scale,
        heroes: options.heroes,
        demo: false,
        capacity: 200.0,
        prepared: prepared_names,
        pads,
        hero_names,
    })
}

#[cfg(test)]
mod navigation_records_tests {
    use super::*;
    use crate::ai::nav::swbf2::load_swbf2_nav;

    #[test]
    fn active_packages_keep_separate_source_indices_through_hint_expansion() {
        let frame = [1,0,0,0,1,0,0,0,1,-11,3,2];
        let hint = |index, kind| serde_json::json!({"sourceIndex":index,"type":kind,"xfrm":frame});
        let mut ring = hint(1, "7");
        ring["properties"] = serde_json::json!([["0dba4cb3", null, "3"]]);
        let panel = serde_json::json!({"sourceIndex":0,"size":[2,1,4],"flags":17,"enabled":true,"xfrm":frame});
        let world = serde_json::json!({
            "hints":[hint(0, "8")],
            "layers":{"east":{"hints":[hint(0, "unrecognized"),ring],"barriers":[panel]},
                      "store":{"hints":[hint(0, "4")]}, "west":{"hints":[hint(0, "2")]}}
        });
        let map = bfe_content::json::parse_bytes(world.to_string().as_bytes()).unwrap();
        let mut bytes = br#"{"schema":2"#.to_vec();
        write_navigation_records(&map, &["east".into(), "west".into()], &mut bytes).unwrap();
        bytes.push(b'}');
        let source = bfe_content::json::parse_bytes(&bytes).unwrap();
        let nav = load_swbf2_nav(&source, None, None).unwrap();
        assert_eq!(nav.barriers.list[0].flags, 17);
        let raw = source.get("hints").unwrap();
        let origins: Vec<_> = nav.hints.list.iter().map(|h| {
            let row = raw.at(h.source).unwrap();
            (row.get("sourceLayer").and_then(Value::as_str), row.get("sourceIndex").unwrap().as_f64().unwrap() as usize)
        }).collect();
        let rings = origins.iter().filter(|v| **v == (Some("east"), 1)).count();
        assert_eq!(origins.first(), Some(&(None, 0)));
        assert_eq!(rings, 4);
        assert_eq!(origins.last(), Some(&(Some("west"), 0)));
        assert_eq!(origins.len(), 6);
    }
}
