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
    Ok(FirstPersonInput {
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
pub fn walker(actor: &Actor, clips: &Clips, ball_radius: f64) -> Input {
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
pub fn copy_input(input: &Input) -> Input {
    use bfe_presentation::unit_content::WalkerInput;
    let t = &input.tables;
    Input {
        bones: copy_bones(&input.bones),
        clips: copy_clips(&input.clips),
        tables: TableInput {
            sets: t.sets,
            alert: t.alert,
            transitions: t.transitions,
            jump: t.jump,
            fall: t.fall,
            land_soft: t.land_soft,
            land_hard: t.land_hard,
            dive: t.dive,
            jet: t.jet,
            thrown: t.thrown,
            upper: t.upper.clone(),
        },
        walker: input.walker.as_ref().map(|w| WalkerInput {
            by_mode: w.by_mode,
            pivot: w.pivot,
            roll_center: w.roll_center,
        }),
    }
}
pub fn copy_first_person(
    input: &bfe_presentation::unit_content::FirstPersonInput,
) -> bfe_presentation::unit_content::FirstPersonInput {
    bfe_presentation::unit_content::FirstPersonInput {
        bones: copy_bones(&input.bones),
        clips: copy_clips(&input.clips),
        sets: input.sets.clone(),
        forced_set: input.forced_set,
        offset: input.offset,
    }
}
