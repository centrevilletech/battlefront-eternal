//! animator.js::SoldierAnimation and presentation/unit-content.js::unitContent.
//! Resolve authored names during admission. The pose kernel receives only clip
//! indices, bone transforms and immutable samples on its frame path.
use bfe_content::{actor::Actor, clips::Clips, Value};
use bfe_presentation::unit_content::{Bone, ClipInput, Input, TableInput};

const POSTURES: [&str; 3] = ["stand", "crouch", "prone"];
const GAITS: [&str; 9] = [
    "idle",
    "walk",
    "run",
    "walkBack",
    "runBack",
    "turnLeft",
    "turnRight",
    "walkLeft",
    "walkRight",
];
const NAMES: [&str; 9] = [
    "idle_emote",
    "walkforward",
    "runforward",
    "walkbackward",
    "runbackward",
    "turnleft",
    "turnright",
    "walkleft",
    "walkright",
];

pub fn soldier(actor: &Actor, clips: &Clips, banks: Option<&Value>) -> Input {
    let id = |name: &str| clips.list.iter().position(|c| c.name == name);
    let sets = std::array::from_fn(|posture| {
        std::array::from_fn(|gait| {
            if posture == 1 && matches!(GAITS[gait], "run" | "runBack" | "walkLeft" | "walkRight") {
                return None;
            }
            if posture == 2 && matches!(GAITS[gait], "run" | "runBack") {
                return None;
            }
            if posture == 0 && matches!(GAITS[gait], "walkLeft" | "walkRight") {
                return None;
            }
            id(&format!(
                "human_rifle_{}_{}",
                POSTURES[posture], NAMES[gait]
            ))
        })
    });
    let alert = std::array::from_fn(|gait| {
        let base = format!("human_rifle_stand_{}", NAMES[gait]);
        if gait >= 7 {
            None
        } else if matches!(GAITS[gait], "idle" | "run" | "runBack") {
            id(&format!("{base}_alert")).or_else(|| id(&base))
        } else {
            sets[0][gait]
        }
    });
    soldier_from_tables(actor, clips, banks, sets, alert)
}

/// Computes ordered clip-name prefixes for a 2005 unit from its definition and
/// assigned animation banks (ordered lowest-to-highest precedence).
///
/// Saber/melee heroes search their own `{bank}_sabre` overrides first, then
/// `human_sabre`, before falling back to `human_rifle` for postures without a
/// saber clip. Non-saber soldiers search their own `{bank}_rifle` first, then
/// `human_rifle`.
pub fn swbf2_slot_prefixes(unit_def: Option<&Value>, anim_banks: &[String]) -> Vec<String> {
    let is_saber = anim_banks
        .iter()
        .any(|b| b.to_ascii_lowercase().ends_with("_sabre"))
        || unit_def
            .and_then(|d| d.get("weapons"))
            .and_then(Value::as_array)
            .is_some_and(|ws| {
                ws.iter().any(|w| {
                    matches!(
                        w.get("base").and_then(Value::as_str),
                        Some("melee" | "meleethrow")
                    ) || w
                        .get("saberHilts")
                        .and_then(Value::as_array)
                        .is_some_and(|h| !h.is_empty())
                        || w.get("comboBindings")
                            .and_then(Value::as_array)
                            .is_some_and(|c| !c.is_empty())
                })
            });
    let mut prefixes: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !p.is_empty() && !prefixes.contains(&p) {
            prefixes.push(p);
        }
    };
    for bank in anim_banks.iter().rev() {
        let stem = bfe_content::clips::bank_stem(bank);
        if stem == "human" || stem.starts_with("human_") && !stem.ends_with("_sabre") {
            continue;
        }
        if stem.ends_with("_sabre") || stem.ends_with("_rifle") {
            push(stem);
        } else if is_saber {
            push(format!("{stem}_sabre"));
            push(stem.clone());
            push(format!("{stem}_rifle"));
        } else {
            push(format!("{stem}_rifle"));
            push(stem.clone());
            push(format!("{stem}_sabre"));
        }
    }
    if is_saber {
        push("human_sabre".to_string());
    }
    push("human_rifle".to_string());
    prefixes
}

/// 2005 soldier clip-table binder: resolves every locomotion, alert,
/// transition, air, dive, jetpack, and thrown slot across `preferred_prefixes`
/// (trying plain and `_full` forms), keeping the 2004 [`soldier`] table
/// untouched.
pub fn soldier_swbf2(
    actor: &Actor,
    clips: &Clips,
    banks: Option<&Value>,
    preferred_prefixes: &[String],
) -> Input {
    let fallback_prefixes;
    let prefixes: &[String] = if preferred_prefixes.is_empty() {
        fallback_prefixes = vec!["human_rifle".to_string()];
        &fallback_prefixes
    } else {
        preferred_prefixes
    };
    let id = |name: &str| clips.list.iter().position(|c| c.name == name);
    let slot = |suffix: &str| -> Option<usize> {
        for prefix in prefixes {
            if let Some(i) = id(&format!("{prefix}_{suffix}")) {
                return Some(i);
            }
            if let Some(i) = id(&format!("{prefix}_{suffix}_full")) {
                return Some(i);
            }
        }
        None
    };
    let sets = std::array::from_fn(|posture| {
        std::array::from_fn(|gait| {
            if posture == 1 && matches!(GAITS[gait], "run" | "runBack" | "walkLeft" | "walkRight") {
                return None;
            }
            if posture == 2 && matches!(GAITS[gait], "run" | "runBack") {
                return None;
            }
            if posture == 0 && matches!(GAITS[gait], "walkLeft" | "walkRight") {
                return None;
            }
            slot(&format!("{}_{}", POSTURES[posture], NAMES[gait]))
        })
    });
    let alert = std::array::from_fn(|gait| {
        if gait >= 7 {
            None
        } else if matches!(GAITS[gait], "idle" | "run" | "runBack") {
            slot(&format!("stand_{}_alert", NAMES[gait])).or(sets[0][gait])
        } else {
            sets[0][gait]
        }
    });
    let mut input = soldier_from_tables(actor, clips, banks, sets, alert);
    input.tables.transitions = std::array::from_fn(|from| {
        std::array::from_fn(|to| match (from, to) {
            (0, 2) | (1, 2) => slot(&format!("{}_getdown_prone", POSTURES[from])),
            (2, 0) | (2, 1) => slot(&format!("prone_getup_{}", POSTURES[to])),
            _ => None,
        })
    });
    input.tables.jump = slot("jump");
    input.tables.fall = slot("fall");
    input.tables.land_soft = slot("landsoft");
    input.tables.land_hard = slot("landhard");
    input.tables.dive = slot("diveforward");
    input.tables.jet = slot("jetpack_hover");
    input.tables.thrown = std::array::from_fn(|way| {
        std::array::from_fn(|action| {
            let way = ["front", "back"][way];
            let action = ["flying", "bounce", "land", "rest"][action];
            slot(&format!(
                "thrown_{action}{way}{}",
                if action == "flying" { "" } else { "soft" }
            ))
        })
    });
    input
}

fn soldier_from_tables(
    actor: &Actor,
    clips: &Clips,
    banks: Option<&Value>,
    sets: [[Option<usize>; 9]; 3],
    alert: [Option<usize>; 9],
) -> Input {
    let id = |name: &str| clips.list.iter().position(|c| c.name == name);
    let transitions = std::array::from_fn(|from| {
        std::array::from_fn(|to| match (from, to) {
            (0, 2) | (1, 2) => id(&format!("human_rifle_{}_getdown_prone", POSTURES[from])),
            (2, 0) | (2, 1) => id(&format!("human_rifle_prone_getup_{}", POSTURES[to])),
            _ => None,
        })
    });
    let banks = banks
        .and_then(|v| v.get("banks"))
        .and_then(Value::as_object);
    let mut upper = vec![(0..clips.list.len()).collect::<Vec<_>>()];
    if let Some(banks) = banks {
        for (_name, bank) in banks.entries().filter(|(name, _)| *name != "rifle") {
            upper.push(
                clips
                    .list
                    .iter()
                    .enumerate()
                    .map(|(index, clip)| {
                        let Some(suffix) = clip.name.strip_prefix("human_rifle_") else {
                            return index;
                        };
                        // findUpper first honors postureless aliases, then alert/loco.
                        // Missing banks deliberately retain the authored rifle sample.
                        let mut named = bank
                            .get("postureless")
                            .and_then(|b| b.get(suffix))
                            .and_then(Value::as_str);
                        if named.is_none() {
                            if let Some((posture, action)) =
                                suffix.split_once('_').filter(|(p, _)| POSTURES.contains(p))
                            {
                                let (action, alert) = action
                                    .strip_suffix("_alert")
                                    .map_or((action, false), |a| (a, true));
                                let gait = if action.starts_with("turn") {
                                    "idle_emote"
                                } else {
                                    action
                                };
                                if alert {
                                    named = bank
                                        .get("alert")
                                        .and_then(|b| b.get(gait))
                                        .and_then(Value::as_str);
                                }
                                named = named.or_else(|| {
                                    bank.get("loco")
                                        .and_then(|b| b.get(posture))
                                        .and_then(|b| b.get(gait))
                                        .and_then(Value::as_str)
                                });
                            }
                        }
                        named.and_then(id).unwrap_or(index)
                    })
                    .collect(),
            );
        }
    }
    Input {
        bones: actor
            .bones
            .iter()
            .map(|b| Bone {
                name: b.name.clone(),
                parent: b.parent as i32,
                pose: [
                    b.position.x,
                    b.position.y,
                    b.position.z,
                    b.quaternion.x,
                    b.quaternion.y,
                    b.quaternion.z,
                    b.quaternion.w,
                    b.scale.x,
                    b.scale.y,
                    b.scale.z,
                ],
            })
            .collect(),
        clips: clips
            .list
            .iter()
            .map(|c| ClipInput {
                frames: c.frames_f64() as usize,
                fps: c.fps,
                root_motion: c.root_motion,
                tracks: c.tracks.iter().map(|&t| t as i32).collect(),
                data: c.data().to_vec(),
                duration: c.duration,
                speed: c.speed,
                idle: c.name.contains("_idle_"),
            })
            .collect(),
        tables: TableInput {
            sets,
            alert,
            transitions,
            jump: id("human_rifle_jump"),
            fall: id("human_rifle_fall"),
            land_soft: id("human_rifle_landsoft"),
            land_hard: id("human_rifle_landhard"),
            dive: id("human_rifle_diveforward"),
            jet: id("human_rifle_jetpack_hover"),
            thrown: std::array::from_fn(|way| {
                std::array::from_fn(|action| {
                    let way = ["front", "back"][way];
                    let action = ["flying", "bounce", "land", "rest"][action];
                    id(&format!(
                        "human_rifle_thrown_{action}{way}{}",
                        if action == "flying" { "" } else { "soft" }
                    ))
                })
            }),
            upper,
        },
        walker: None,
    }
}

/// Inspectable admission descriptor for loader fixtures and capture checks.
/// f32 samples expand to f64 as JavaScript's Array.from(Float32Array) does.
pub fn descriptor(input: &Input) -> serde_json::Value {
    let t = &input.tables;
    serde_json::json!({"bones":input.bones.iter().map(|b|serde_json::json!({"name":b.name,"parent":b.parent,"pose":b.pose})).collect::<Vec<_>>(),
        "clips":input.clips.iter().map(|c|serde_json::json!({"frames":c.frames,"fps":c.fps,"rootMotion":c.root_motion,"tracks":c.tracks,"data":c.data.iter().map(|v|*v as f64).collect::<Vec<_>>(),"duration":c.duration,"speed":c.speed,"idle":c.idle})).collect::<Vec<_>>(),
        "walker":input.walker.as_ref().map(|w|serde_json::json!({"byMode":w.by_mode,"pivot":w.pivot,"rollCenter":w.roll_center})),
        "tables":{"sets":t.sets,"alert":t.alert,"transitions":t.transitions,"jump":t.jump,"fall":t.fall,"landSoft":t.land_soft,"landHard":t.land_hard,"dive":t.dive,"jet":t.jet,"thrown":t.thrown,"upper":t.upper}})
}

/// firstperson.js::SETS and firstPersonContent. The offset uses the shared
/// three.js arithmetic, including its authored multiplication order.
pub fn first_person(
    def: &Value,
    clips: &Clips,
    walker: bool,
) -> Result<bfe_presentation::unit_content::FirstPersonInput, String> {
    use bfe_presentation::unit_content::FirstPersonInput;
    let bones = def
        .get("bones")
        .and_then(Value::as_array)
        .ok_or("first-person rig has no bones")?
        .iter()
        .map(|b| {
            let local = b
                .get("local")
                .and_then(Value::as_array)
                .ok_or("first-person bone has no local matrix")?;
            if local.len() != 16 {
                return Err("first-person bone matrix has invalid length".into());
            }
            let matrix = local
                .iter()
                .map(|v| v.as_f64().ok_or("bone matrix is not numeric"))
                .collect::<Result<Vec<_>, _>>()?;
            let (p, q, s) = bfe_content::three::Matrix4::from_array(&matrix).decompose();
            Ok(Bone {
                name: b.get("name").and_then(Value::as_str).unwrap_or("").into(),
                parent: b.get("parent").and_then(Value::as_f64).unwrap_or(-1.0) as i32,
                pose: [p.x, p.y, p.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z],
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let names = if walker {
        [[
            "_1st_droideka_idle",
            "_1st_droideka_walk",
            "_1st_droideka_shoot",
            "_1st_droideka_shoot",
            "_1st_droideka_reload",
            "_1st_droideka_idle",
            "_1st_droideka_idle",
            "_1st_droideka_idle",
            "_1st_handsdown",
        ]; 4]
    } else {
        [
            [
                "_1st_rifle_idle",
                "_1st_rifle_run",
                "_1st_rifle_shoot",
                "_1st_tool_idle",
                "_1st_rifle_reload",
                "_1st_rifle_jump",
                "_1st_rifle_flail",
                "_1st_rifle_jump_land",
                "_1st_handsdown",
            ],
            [
                "_1st_tool_idle",
                "_1st_tool_run",
                "_1st_tool_shoot",
                "_1st_tool_idle",
                "_1st_tool_reload",
                "_1st_tool_jump",
                "_1st_tool_flail",
                "_1st_tool_jump_land",
                "_1st_handsdown",
            ],
            [
                "_1st_tool_idle",
                "_1st_tool_run",
                "_1st_grenade_shoot",
                "_1st_grenade_shoot2",
                "_1st_tool_reload",
                "_1st_tool_jump",
                "_1st_tool_flail",
                "_1st_rifle_jump_land",
                "_1st_handsdown",
            ],
            [
                "_1st_bazooka_idle",
                "_1st_bazooka_run",
                "_1st_bazooka_shoot",
                "_1st_bazooka_idle",
                "_1st_bazooka_reload",
                "_1st_bazooka_jump",
                "_1st_bazooka_flail",
                "_1st_bazooka_jump_land",
                "_1st_handsdown",
            ],
        ]
    };
    let mut offset = three::Matrix4::default();
    let mut rotation = three::Matrix4::default();
    if walker {
        offset.make_translation(0.01, 0.35, 0.0);
        rotation.make_rotation_y(3.2);
        offset.multiply(&rotation);
    } else {
        offset.make_translation(0.0, 0.13, -0.3645);
        rotation.make_rotation_z(0.05);
        offset.multiply(&rotation);
        rotation.make_rotation_y(3.0);
        offset.multiply(&rotation);
    }
    // A droideka's guns hang on its arms: the bones its model's parts hang
    // on, the left one (+X at bind) first, as its world guns are ordered.
    let guns = if walker { first_person_guns(def, &bones) } else { Vec::new() };
    Ok(FirstPersonInput {
        guns,
        bones,
        clips: clips
            .list
            .iter()
            .map(|c| ClipInput {
                frames: c.frames_f64() as usize,
                fps: c.fps,
                root_motion: c.root_motion,
                tracks: c.tracks.iter().map(|t| *t as i32).collect(),
                data: c.data().to_vec(),
                duration: c.duration,
                speed: c.speed,
                idle: false,
            })
            .collect(),
        sets: names
            .into_iter()
            .map(|s| s.map(|name| clips.list.iter().position(|c| c.name == name)))
            .collect(),
        forced_set: walker.then_some(0),
        offset: offset.elements,
    })
}
pub fn first_person_descriptor(
    input: &bfe_presentation::unit_content::FirstPersonInput,
) -> serde_json::Value {
    serde_json::json!({"bones":input.bones.iter().map(|b|serde_json::json!({"name":b.name,"parent":b.parent,"pose":b.pose})).collect::<Vec<_>>(),"clips":input.clips.iter().map(|c|serde_json::json!({"frames":c.frames,"fps":c.fps,"rootMotion":c.root_motion,"tracks":c.tracks,"data":c.data.iter().map(|v|*v as f64).collect::<Vec<_>>(),"duration":c.duration,"speed":c.speed,"idle":c.idle})).collect::<Vec<_>>(),"sets":input.sets,"forcedSet":input.forced_set,"offset":input.offset})
}

/// droideka.js::DroidekaAnimator and unitContent. Reuse the native skinned
/// roll-axle derivation; a bind-pose bounding box is not its folded wheel axis.
pub fn walker(actor: &Actor, clips: &Clips, ball_radius: f64, blasters: Vec<usize>, aimer: [f64; 6], tips: Vec<Vec<[f64; 3]>>) -> Input {
    use bfe_pose::{
        rig::{
            droideka::{roll_axle, SkinnedBody},
            skeleton::{BoneDesc, Skeleton},
        },
        Clip,
    };
    let skeleton = Skeleton::new(
        &actor
            .bones
            .iter()
            .map(|b| BoneDesc {
                name: b.name.clone(),
                parent: b.parent as i32,
                local: b.local,
            })
            .collect::<Vec<_>>(),
        false,
    );
    let inverses = actor
        .bones
        .iter()
        .map(|b| three::Matrix4 {
            elements: b.inverse.elements,
        })
        .collect::<Vec<_>>();
    let body = SkinnedBody {
        positions: &actor.positions,
        skin_index: &actor.skin_index,
        bind_matrix: three::Matrix4 {
            elements: actor.bind_matrix.elements,
        },
        bone_inverses: &inverses,
    };
    let roll = clips.get("roll").map(|c| Clip {
        frames: c.frames_f64() as usize,
        fps: c.fps,
        root_motion: c.root_motion,
        tracks: c.tracks.iter().map(|v| *v as i32).collect(),
        data: c.data().to_vec(),
    });
    let (pivot, center) = roll_axle(&skeleton, &body, roll.as_ref(), ball_radius);
    let modes = [
        Some("idle"),
        Some("turnl"),
        Some("turnr"),
        Some("death01"),
        None,
        Some("idle_to_leftfoot"),
        None,
        Some("walk_leftfoot_rightfoot"),
        Some("walk_rightfoot_leftfoot"),
        Some("leftfoot_to_idle"),
        Some("rightfoot_to_idle"),
        Some("fold"),
        Some("roll"),
        Some("unfold"),
    ];
    let mut input = soldier(actor, clips, None);
    input.tables = TableInput {
        sets: [[None; 9]; 3],
        alert: [None; 9],
        transitions: [[None; 3]; 3],
        jump: None,
        fall: None,
        land_soft: None,
        land_hard: None,
        dive: None,
        jet: None,
        thrown: [[None; 4]; 2],
        upper: vec![(0..clips.list.len()).collect()],
    };
    input.walker = Some(bfe_presentation::unit_content::WalkerInput {
        by_mode: modes
            .map(|name| name.and_then(|name| clips.list.iter().position(|c| c.name == name))),
        pivot: [pivot.x, pivot.y, pivot.z],
        roll_center: [center.x, center.y, center.z],
        blasters,
        aimer,
        tips,
    });
    input
}

// UnitFactory may register the same body/clip template for several texture
// overrides. Resource identity does not consume immutable animation content.
// Copy only during admission so each retained owner can load its own state.
fn copy_bones(bones: &[Bone]) -> Vec<Bone> {
    bones
        .iter()
        .map(|b| Bone {
            name: b.name.clone(),
            parent: b.parent,
            pose: b.pose,
        })
        .collect()
}
fn copy_clips(clips: &[ClipInput]) -> Vec<ClipInput> {
    clips
        .iter()
        .map(|c| ClipInput {
            frames: c.frames,
            fps: c.fps,
            root_motion: c.root_motion,
            tracks: c.tracks.clone(),
            data: c.data.clone(),
            duration: c.duration,
            speed: c.speed,
            idle: c.idle,
        })
        .collect()
}
/// The bones a first person model's parts hang on, left (+X at bind) first.
fn first_person_guns(def: &Value, bones: &[bfe_presentation::unit_content::Bone]) -> Vec<usize> {
    // A bone's place and turn at bind, from its parents' (root first).
    fn world(i: usize, bones: &[bfe_presentation::unit_content::Bone], depth: usize) -> (three::Vector3, three::Quaternion) {
        let b = &bones[i];
        let place = three::Vector3::new(b.pose[0], b.pose[1], b.pose[2]);
        let turn = three::Quaternion::new(b.pose[3], b.pose[4], b.pose[5], b.pose[6]);
        let Some(parent) = usize::try_from(b.parent).ok().filter(|&p| p < bones.len() && depth < bones.len()) else {
            return (place, turn);
        };
        let (mut at, up) = world(parent, bones, depth + 1);
        let mut offset = place;
        offset.apply_quaternion(&up);
        at.add(&offset);
        let mut q = up;
        q.multiply(&turn);
        (at, q)
    }
    let mut found: Vec<(usize, f64)> = Vec::new();
    for part in def.get("parts").and_then(Value::as_array).unwrap_or(&[]) {
        let Some(name) = part.get("bone").and_then(Value::as_str) else { continue };
        let Some(i) = bones.iter().position(|b| b.name.eq_ignore_ascii_case(name)) else { continue };
        if found.iter().all(|(b, _)| *b != i) {
            found.push((i, world(i, bones, 0).0.x));
        }
    }
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    found.into_iter().map(|(i, _)| i).collect()
}

pub fn copy_first_person(
    input: &bfe_presentation::unit_content::FirstPersonInput,
) -> bfe_presentation::unit_content::FirstPersonInput {
    bfe_presentation::unit_content::FirstPersonInput {
        guns: input.guns.clone(),
        bones: copy_bones(&input.bones),
        clips: copy_clips(&input.clips),
        sets: input.sets.clone(),
        forced_set: input.forced_set,
        offset: input.offset,
    }
}

#[cfg(test)]
mod swbf2_slot_tests {
    use super::*;
    use bfe_content::{
        actor::build_actor,
        clips::load_clips_by_crc,
        json::parse,
        zero_hash,
    };
    use std::collections::HashMap;
    use std::rc::Rc;

    #[test]
    fn swbf2_soldier_table_resolves_custom_bank_and_prefers_sabre_over_rifle() {
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let actor_json = parse(
            r#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":1,"indexOffset":0,"indexCount":1,"texture":""}]}"#,
        )
        .unwrap();
        let actor = build_actor(&actor_json, &[0.0f32; 9]).unwrap();
        let bones = vec!["DummyRoot".to_string()];

        // 1. Standalone non-human melee bank (no common human_rifle clips at all).
        let quad_meta = parse(&format!(
            r#"{{
                "fps": 30,
                "banks": {{
                    "quad": {{
                        "clips": [
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 1, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 1, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}}
                        ]
                    }}
                }}
            }}"#,
            zero_hash::crc_hex("quad_sabre_stand_idle_emote_full"),
            zero_hash::crc_hex("quad_sabre_stand_walkforward"),
            zero_hash::crc_hex("quad_sabre_stand_runforward_full"),
            zero_hash::crc_hex("quad_sabre_crouch_idle_emote_full"),
            zero_hash::crc_hex("quad_sabre_jump_full"),
            zero_hash::crc_hex("quad_sabre_diveforward_full"),
            zero_hash::crc_hex("quad_sabre_thrown_flyingfront_full"),
        ))
        .unwrap();
        let floats: Rc<[f32]> = Rc::from(vec![0.0f32; 14]);
        let (quad_clips, _) =
            load_clips_by_crc(&quad_meta, &floats, &["quad".into()], &bones, &HashMap::new()).unwrap();
        let quad_def = parse(r#"{"weapons":[{"base":"melee"}]}"#).unwrap();
        let quad_prefixes = swbf2_slot_prefixes(Some(&quad_def), &["quad".into()]);
        let quad_in = soldier_swbf2(&actor, &quad_clips, None, &quad_prefixes);
        assert!(quad_in.tables.sets[0][0].is_some()); // stand idle
        assert!(quad_in.tables.sets[0][1].is_some()); // stand walk
        assert!(quad_in.tables.sets[0][2].is_some()); // stand run
        assert!(quad_in.tables.sets[1][0].is_some()); // crouch idle
        assert!(quad_in.tables.alert[0].is_some());
        assert!(quad_in.tables.jump.is_some());
        assert!(quad_in.tables.dive.is_some());
        assert!(quad_in.tables.thrown[0][0].is_some());

        // 2. Saber hero with both human_rifle and human_sabre + hero override prefers hero -> sabre -> rifle.
        let hero_meta = parse(&format!(
            r#"{{
                "fps": 30,
                "banks": {{
                    "human_0": {{
                        "clips": [
                            {{"crc": "{}", "flags": 1, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}}
                        ]
                    }},
                    "human_sabre": {{
                        "clips": [
                            {{"crc": "{}", "flags": 1, "frames": 4, "offset": 0, "tracks": ["{root_crc}"]}}
                        ]
                    }},
                    "duelist": {{
                        "clips": [
                            {{"crc": "{}", "flags": 0, "frames": 6, "offset": 0, "tracks": ["{root_crc}"]}}
                        ]
                    }}
                }}
            }}"#,
            zero_hash::crc_hex("human_rifle_stand_runforward"),
            zero_hash::crc_hex("human_rifle_jump"),
            zero_hash::crc_hex("human_sabre_stand_runforward_full"),
            zero_hash::crc_hex("duelist_sabre_jump_full"),
        ))
        .unwrap();
        let hero_floats: Rc<[f32]> = Rc::from(vec![0.0f32; 42]);
        let banks = vec!["human_0".into(), "human_sabre".into(), "duelist".into()];
        let (hero_clips, _) =
            load_clips_by_crc(&hero_meta, &hero_floats, &banks, &bones, &HashMap::new()).unwrap();
        let hero_prefixes = swbf2_slot_prefixes(Some(&quad_def), &banks);
        let hero_in = soldier_swbf2(&actor, &hero_clips, None, &hero_prefixes);
        let run_idx = hero_in.tables.sets[0][2].expect("stand run");
        assert_eq!(hero_clips.list[run_idx].name, "human_sabre_stand_runforward_full");
        let jump_idx = hero_in.tables.jump.expect("jump");
        assert_eq!(hero_clips.list[jump_idx].name, "duelist_sabre_jump_full");
    }
}
