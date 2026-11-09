// Frozen synthetic results exercise the complete presentation phases. Cargo supplies
// this build's replay executable, so no installed game data, Node runtime, or prior
// build is needed and the assertions cannot silently skip under the Rust test job.
//
// Synthetic scenario inputs are constructed with `v8math::sin` and `v8math::cos`
// so `x86_64` and `aarch64` produce the identical committed SHA-256 digests in
// `tests/fixtures/presentation/unit-goldens.json`.

use bfe_pose::display::DisplayRig;
use bfe_presentation::layout::{unit_command as c, unit_frame as f, unit_input as u, VERSION};
use bfe_presentation::unit_content::Input;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use three::Matrix4;

const NAMES: [&str; 23] = [
    "DummyRoot",
    "bone_pelvis",
    "bone_a_spine",
    "bone_b_spine",
    "bone_ribcage",
    "bone_neck",
    "bone_head",
    "bone_l_clavicle",
    "bone_l_upperarm",
    "bone_l_forearm",
    "bone_l_hand",
    "bone_r_clavicle",
    "bone_r_upperarm",
    "bone_r_forearm",
    "bone_r_hand",
    "hp_weapons",
    "bone_root",
    "bone_l_thigh",
    "bone_l_calf",
    "bone_l_foot",
    "bone_r_thigh",
    "bone_r_calf",
    "bone_r_foot",
];

const PARENTS: [i32; 23] = [
    -1, 0, 1, 2, 3, 4, 5, 4, 7, 8, 9, 4, 11, 12, 13, 14, 0, 1, 17, 18, 1, 20, 21,
];

struct SyntheticClip {
    name: String,
    frames: usize,
    fps: f64,
    root_motion: bool,
    tracks: Vec<i32>,
    data: Vec<f32>,
    duration: f64,
    speed: f64,
    idle: bool,
}

impl SyntheticClip {
    fn to_json(&self, idle_override: Option<bool>) -> Value {
        json!({
            "frames": self.frames,
            "fps": self.fps,
            "rootMotion": self.root_motion,
            "tracks": self.tracks,
            "data": self.data,
            "duration": self.duration,
            "speed": self.speed,
            "idle": idle_override.unwrap_or(self.idle),
        })
    }
}

struct Fixture {
    bones_json: Vec<Value>,
    clips: Vec<SyntheticClip>,
    ids: HashMap<String, usize>,
    descriptor: Value,
}

fn build_fixture() -> Fixture {
    let bones_json: Vec<Value> = NAMES
        .iter()
        .enumerate()
        .map(|(index, &name)| {
            let px = (index as f64) * 0.001;
            let py = 0.03 + (index as f64) * 0.002;
            let pz = -0.01;
            json!({
                "name": name,
                "parent": PARENTS[index],
                "pose": [px, py, pz, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            })
        })
        .collect();

    let mut clips: Vec<SyntheticClip> = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut add = |name: String| {
        let k = clips.len() + 1;
        let frames = 13usize;
        let fps = 30.0f64;
        let mut floats = Vec::with_capacity(NAMES.len() * frames * 7);
        for b in 0..NAMES.len() {
            for frame in 0..frames {
                let a = (k as f64) * 0.0007 + (b as f64) * 0.0003 + (frame as f64) * 0.003;
                floats.extend_from_slice(&[
                    0.0f32,
                    v8math::sin(a) as f32,
                    0.0f32,
                    v8math::cos(a) as f32,
                    (0.01 * (b as f64) + (frame as f64) * 0.002) as f32,
                    (0.02 * (b as f64)) as f32,
                    (0.003 * (k as f64) + (frame as f64) * 0.001) as f32,
                ]);
            }
        }
        let duration = ((frames - 1) as f64) / fps;
        let stride = (0.6f64).hypot(0.8f64);
        let speed = stride / duration;
        let idle = name.contains("_idle_");
        let idx = clips.len();
        ids.insert(name.clone(), idx);
        clips.push(SyntheticClip {
            name,
            frames,
            fps,
            root_motion: true,
            tracks: (0..NAMES.len() as i32).collect(),
            data: floats,
            duration,
            speed,
            idle,
        });
    };

    for posture in ["stand", "crouch", "prone"] {
        for gait in [
            "idle_emote",
            "walkforward",
            "runforward",
            "walkbackward",
            "runbackward",
            "turnleft",
            "turnright",
            "walkleft",
            "walkright",
        ] {
            add(format!("human_rifle_{posture}_{gait}"));
            if posture == "stand" {
                add(format!("human_rifle_{posture}_{gait}_alert"));
            }
            add(format!("human_pistol_{posture}_{gait}"))
        }
    }
    for action in [
        "jump",
        "fall",
        "landsoft",
        "landhard",
        "diveforward",
        "jetpack_hover",
        "stand_getdown_prone",
        "crouch_getdown_prone",
        "prone_getup_stand",
        "prone_getup_crouch",
        "stand_death_backward",
        "die",
    ] {
        add(format!("human_rifle_{action}"));
    }
    for way in ["front", "back"] {
        for action in ["flying", "bounce", "land", "rest"] {
            let suffix = if action == "flying" { "" } else { "soft" };
            add(format!("human_rifle_thrown_{action}{way}{suffix}"));
        }
    }
    for action in ["shoot", "reload", "charge"] {
        for posture in ["stand", "crouch", "prone"] {
            add(format!("human_rifle_{posture}_{action}"));
        }
    }
    add("pilot".to_string());

    let id = |name: &str| -> Option<usize> { ids.get(name).copied() };
    let loco_clip = |posture: &str, gait: &str| -> Option<&'static str> {
        match (posture, gait) {
            ("stand", "idle") => Some("human_rifle_stand_idle_emote"),
            ("stand", "walk") => Some("human_rifle_stand_walkforward"),
            ("stand", "run") => Some("human_rifle_stand_runforward"),
            ("stand", "walkBack") => Some("human_rifle_stand_walkbackward"),
            ("stand", "runBack") => Some("human_rifle_stand_runbackward"),
            ("stand", "turnLeft") => Some("human_rifle_stand_turnleft"),
            ("stand", "turnRight") => Some("human_rifle_stand_turnright"),
            ("crouch", "idle") => Some("human_rifle_crouch_idle_emote"),
            ("crouch", "walk") => Some("human_rifle_crouch_walkforward"),
            ("crouch", "walkBack") => Some("human_rifle_crouch_walkbackward"),
            ("crouch", "turnLeft") => Some("human_rifle_crouch_turnleft"),
            ("crouch", "turnRight") => Some("human_rifle_crouch_turnright"),
            ("prone", "idle") => Some("human_rifle_prone_idle_emote"),
            ("prone", "walk") => Some("human_rifle_prone_walkforward"),
            ("prone", "walkBack") => Some("human_rifle_prone_walkbackward"),
            ("prone", "walkLeft") => Some("human_rifle_prone_walkleft"),
            ("prone", "walkRight") => Some("human_rifle_prone_walkright"),
            ("prone", "turnLeft") => Some("human_rifle_prone_turnleft"),
            ("prone", "turnRight") => Some("human_rifle_prone_turnright"),
            _ => None,
        }
    };
    let postures = ["stand", "crouch", "prone"];
    let gaits = [
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
    let sets: Vec<Vec<Option<usize>>> = postures
        .iter()
        .map(|&p| {
            gaits
                .iter()
                .map(|&g| loco_clip(p, g).and_then(id))
                .collect()
        })
        .collect();
    let alert: Vec<Option<usize>> = gaits
        .iter()
        .map(|&g| {
            let base = loco_clip("stand", g)?;
            if matches!(g, "idle" | "run" | "runBack") {
                id(&format!("{base}_alert")).or_else(|| id(base))
            } else {
                id(base)
            }
        })
        .collect();
    let transitions: Vec<Vec<Option<usize>>> = postures
        .iter()
        .map(|&from| {
            postures
                .iter()
                .map(|&to| match (from, to) {
                    ("stand", "prone") => id("human_rifle_stand_getdown_prone"),
                    ("crouch", "prone") => id("human_rifle_crouch_getdown_prone"),
                    ("prone", "stand") => id("human_rifle_prone_getup_stand"),
                    ("prone", "crouch") => id("human_rifle_prone_getup_crouch"),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let thrown: Vec<Vec<Option<usize>>> = ["front", "back"]
        .iter()
        .map(|&way| {
            ["flying", "bounce", "land", "rest"]
                .iter()
                .map(|&action| {
                    let suffix = if action == "flying" { "" } else { "soft" };
                    id(&format!("human_rifle_thrown_{action}{way}{suffix}"))
                })
                .collect()
        })
        .collect();

    let rifle_upper: Vec<usize> = (0..clips.len()).collect();
    let pistol_upper: Vec<usize> = clips
        .iter()
        .enumerate()
        .map(|(i, clip)| {
            if let Some(suffix) = clip.name.strip_prefix("human_rifle_") {
                for p in ["stand", "crouch", "prone"] {
                    if let Some(rest) = suffix.strip_prefix(&format!("{p}_")) {
                        let action = rest.strip_suffix("_alert").unwrap_or(rest);
                        let gait = if action.starts_with("turn") {
                            "idle_emote"
                        } else {
                            action
                        };
                        if matches!(
                            gait,
                            "idle_emote"
                                | "walkforward"
                                | "walkbackward"
                                | "runforward"
                                | "runbackward"
                        ) {
                            if let Some(mapped) = id(&format!("human_pistol_{p}_{gait}")) {
                                return mapped;
                            }
                        }
                    }
                }
            }
            i
        })
        .collect();

    let descriptor = json!({
        "bones": bones_json,
        "clips": clips.iter().map(|c| c.to_json(None)).collect::<Vec<_>>(),
        "walker": Value::Null,
        "tables": {
            "sets": sets,
            "alert": alert,
            "transitions": transitions,
            "jump": id("human_rifle_jump"),
            "fall": id("human_rifle_fall"),
            "landSoft": id("human_rifle_landsoft"),
            "landHard": id("human_rifle_landhard"),
            "dive": id("human_rifle_diveforward"),
            "jet": id("human_rifle_jetpack_hover"),
            "thrown": thrown,
            "upper": [rifle_upper, pistol_upper],
        }
    });

    Fixture {
        bones_json,
        clips,
        ids,
        descriptor,
    }
}

fn scenarios() -> Vec<Value> {
    let mut frames = Vec::with_capacity(600);
    for i in 0..600usize {
        let phase = (i / 30) % 10;
        let posture = if phase == 2 {
            1
        } else if phase == 3 {
            2
        } else {
            0
        };
        let air = if phase == 4 {
            if i % 30 < 10 {
                1
            } else {
                2
            }
        } else if phase == 5 {
            4
        } else if phase == 7 && i % 30 < 20 {
            3
        } else {
            0
        };
        let dt = if i % 43 == 0 {
            0.0
        } else if i % 31 == 0 {
            0.07
        } else {
            1.0 / 60.0
        };
        let fi = i as f64;
        frames.push(json!({
            "dt": dt,
            "position": [fi * 0.001, 0.0, 0.0],
            "commands": [],
            "state": {
                "yaw": v8math::sin(fi * 0.021) * 3.0,
                "pitch": v8math::cos(fi * 0.037) * 0.7,
                "velocity": [
                    if phase % 3 == 0 { 0.0 } else { v8math::sin(fi * 0.09) * 6.0 },
                    if air != 0 { -2.0 } else { 0.0 },
                    if phase % 3 == 0 { 0.0 } else { v8math::cos(fi * 0.07) * 6.0 },
                ],
                "posture": posture,
                "air": air,
                "dive": if phase == 6 { if i % 2 != 0 { 1.0 } else { -1.0 } } else { 0.0 },
                "diveTime": 0.6,
            }
        }));
    }
    frames
}

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (dst, src) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *dst = dst.wrapping_add(src);
        }
    }
    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

fn format_js_json(rows: &[Value], fields: &[String]) -> String {
    let mut buf = String::new();
    let mut num_buf = [0u8; 32];
    buf.push('[');
    for (r_idx, row) in rows.iter().enumerate() {
        if r_idx > 0 {
            buf.push(',');
        }
        buf.push('[');
        for (f_idx, field) in fields.iter().enumerate() {
            if f_idx > 0 {
                buf.push(',');
            }
            let arr = row[field].as_array().expect("field array");
            buf.push('[');
            for (v_idx, val) in arr.iter().enumerate() {
                if v_idx > 0 {
                    buf.push(',');
                }
                let x = val.as_f64().expect("numeric element");
                if let Some(len) = jsrt::numfmt::json_number_into(x, &mut num_buf) {
                    buf.push_str(std::str::from_utf8(&num_buf[..len]).unwrap());
                } else {
                    buf.push_str(&jsrt::numfmt::json_number(x));
                }
            }
            buf.push(']');
        }
        buf.push(']');
    }
    buf.push(']');
    buf
}

fn run_replay(input: &Value, name: &str, goldens: &Value) {
    let exe = env!("CARGO_BIN_EXE_unit-replay");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn unit-replay");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(input).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait unit-replay");
    assert!(
        output.status.success(),
        "{name} unit-replay failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse unit-replay stdout");
    let g = &goldens[name];
    let expected_frames = g["frames"].as_u64().unwrap() as usize;
    assert_eq!(rows.len(), expected_frames, "{name} frame count");
    let fields: Vec<String> = g["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let json_text = format_js_json(&rows, &fields);
    let digest = sha256_hex(json_text.as_bytes());
    let expected_sha = g["sha256"].as_str().unwrap();
    assert_eq!(digest, expected_sha, "{name} synthetic output golden");
}

fn push_cmd(frames: &mut [Value], idx: usize, cmd: Value) {
    frames[idx]["commands"].as_array_mut().unwrap().push(cmd);
}

fn first_person_set_clip(set: &str, slot: &str) -> &'static str {
    match (set, slot) {
        ("rifle", "idle") => "_1st_rifle_idle",
        ("rifle", "run") => "_1st_rifle_run",
        ("rifle", "shoot") => "_1st_rifle_shoot",
        ("rifle", "shoot2") => "_1st_tool_idle",
        ("rifle", "reload") => "_1st_rifle_reload",
        ("rifle", "jump") => "_1st_rifle_jump",
        ("rifle", "flail") => "_1st_rifle_flail",
        ("rifle", "jump_land") => "_1st_rifle_jump_land",
        ("tool", "idle") => "_1st_tool_idle",
        ("tool", "run") => "_1st_tool_run",
        ("tool", "shoot") => "_1st_tool_shoot",
        ("tool", "shoot2") => "_1st_tool_idle",
        ("tool", "reload") => "_1st_tool_reload",
        ("tool", "jump") => "_1st_tool_jump",
        ("tool", "flail") => "_1st_tool_flail",
        ("tool", "jump_land") => "_1st_tool_jump_land",
        ("grenade", "idle") => "_1st_tool_idle",
        ("grenade", "run") => "_1st_tool_run",
        ("grenade", "shoot") => "_1st_grenade_shoot",
        ("grenade", "shoot2") => "_1st_grenade_shoot2",
        ("grenade", "reload") => "_1st_tool_reload",
        ("grenade", "jump") => "_1st_tool_jump",
        ("grenade", "flail") => "_1st_tool_flail",
        ("grenade", "jump_land") => "_1st_rifle_jump_land",
        ("bazooka", "idle") => "_1st_bazooka_idle",
        ("bazooka", "run") => "_1st_bazooka_run",
        ("bazooka", "shoot") => "_1st_bazooka_shoot",
        ("bazooka", "shoot2") => "_1st_bazooka_idle",
        ("bazooka", "reload") => "_1st_bazooka_reload",
        ("bazooka", "jump") => "_1st_bazooka_jump",
        ("bazooka", "flail") => "_1st_bazooka_flail",
        ("bazooka", "jump_land") => "_1st_bazooka_jump_land",
        ("droideka", "idle") => "_1st_droideka_idle",
        ("droideka", "run") => "_1st_droideka_walk",
        ("droideka", "shoot") => "_1st_droideka_shoot",
        ("droideka", "shoot2") => "_1st_droideka_shoot",
        ("droideka", "reload") => "_1st_droideka_reload",
        ("droideka", "jump") => "_1st_droideka_idle",
        ("droideka", "flail") => "_1st_droideka_idle",
        ("droideka", "jump_land") => "_1st_droideka_idle",
        _ => panic!("unknown first-person slot {set}.{slot}"),
    }
}

fn pack_field(out: &mut [f64], base: usize, offset: usize, val: &Value) {
    if let Some(arr) = val.as_array() {
        for (i, v) in arr.iter().enumerate() {
            out[base + offset + i] = v.as_f64().unwrap();
        }
    } else {
        out[base + offset] = val.as_f64().unwrap();
    }
}

fn frame_offset(key: &str) -> usize {
    match key {
        "version" => f::VERSION,
        "frame" => f::FRAME,
        "dt" => f::DT,
        "actors" => f::ACTORS,
        "commands" => f::COMMANDS,
        "hasView" => f::HAS_VIEW,
        "frustum" => f::FRUSTUM,
        "firstPerson" => f::FIRST_PERSON,
        _ => panic!("unknown frame field {key}"),
    }
}

fn unit_offset(key: &str) -> usize {
    match key {
        "actor" => u::ACTOR,
        "id" => u::ID,
        "generation" => u::GENERATION,
        "present" => u::PRESENT,
        "own" => u::OWN,
        "removed" => u::REMOVED,
        "alpha" => u::ALPHA,
        "position" => u::POSITION,
        "velocity" => u::VELOCITY,
        "yaw" => u::YAW,
        "pitch" => u::PITCH,
        "posture" => u::POSTURE,
        "air" => u::AIR,
        "dive" => u::DIVE,
        "diveTime" => u::DIVE_TIME,
        "dead" => u::DEAD,
        "previousPosition" => u::PREVIOUS_POSITION,
        "previousVelocity" => u::PREVIOUS_VELOCITY,
        "tickYaw" => u::TICK_YAW,
        "previousStep" => u::PREVIOUS_STEP,
        "groundStep" => u::GROUND_STEP,
        "network" => u::NETWORK,
        "networkEpoch" => u::NETWORK_EPOCH,
        "networkGeneration" => u::NETWORK_GENERATION,
        "networkTick" => u::NETWORK_TICK,
        "fireCounter" => u::FIRE_COUNTER,
        "deathPending" => u::DEATH_PENDING,
        "held" => u::HELD,
        "heldMatrix" => u::HELD_MATRIX,
        "heldClip" => u::HELD_CLIP,
        "heldTime" => u::HELD_TIME,
        "seat" => u::SEAT,
        "rider" => u::RIDER,
        "riderMatrix" => u::RIDER_MATRIX,
        "riderAnimal" => u::RIDER_ANIMAL,
        "riderVelocity" => u::RIDER_VELOCITY,
        "riderSpeed" => u::RIDER_SPEED,
        "riderYawRate" => u::RIDER_YAW_RATE,
        "riderAge" => u::RIDER_AGE,
        "riderHoverPhase" => u::RIDER_HOVER_PHASE,
        "riderQuaternion" => u::RIDER_QUATERNION,
        "rootQuaternion" => u::ROOT_QUATERNION,
        "deathClip" => u::DEATH_CLIP,
        "riderReset" => u::RIDER_RESET,
        "riderLean" => u::RIDER_LEAN,
        "authoritativePosition" => u::AUTHORITATIVE_POSITION,
        _ => panic!("unknown unit field {key}"),
    }
}

fn command_offset(key: &str) -> usize {
    match key {
        "sequence" => c::SEQUENCE,
        "actor" => c::ACTOR,
        "kind" => c::KIND,
        "clip" => c::CLIP,
        "action" => c::ACTION,
        "bank" => c::BANK,
        "yaw" => c::YAW,
        "pitch" => c::PITCH,
        "turn" => c::TURN,
        "time" => c::TIME,
        "hasTime" => c::HAS_TIME,
        _ => panic!("unknown command field {key}"),
    }
}

#[test]
fn display_goldens() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    let goldens: Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/fixtures/presentation/unit-goldens.json"))
            .expect("read unit-goldens.json"),
    )
    .expect("parse unit-goldens.json");

    let fx = build_fixture();

    // 1. Body phase synthetic animation goldens.
    {
        let mut frames = scenarios();
        let clip = |name: &str| fx.ids[name];
        push_cmd(&mut frames, 12, json!({ "kind": "fire" }));
        push_cmd(&mut frames, 35, json!({ "kind": "bank", "bank": 1 }));
        push_cmd(
            &mut frames,
            47,
            json!({ "kind": "weapon", "action": 1, "clip": clip("human_rifle_stand_shoot"), "shooting": true }),
        );
        push_cmd(&mut frames, 54, json!({ "kind": "endWeapon", "action": 1 }));
        push_cmd(&mut frames, 94, json!({ "kind": "holdAlert" }));
        push_cmd(
            &mut frames,
            211,
            json!({ "kind": "die", "clip": clip("human_rifle_stand_death_backward") }),
        );
        push_cmd(&mut frames, 243, json!({ "kind": "reset", "yaw": 0.4 }));
        push_cmd(&mut frames, 300, json!({ "kind": "bank", "bank": 0 }));
        push_cmd(
            &mut frames,
            330,
            json!({
                "kind": "gunner",
                "clip": clip("pilot"),
                "pitch": 0.7,
                "turn": -0.4,
                "time": Value::Null,
                "seat": [0.0, v8math::sin(0.2), 0.0, v8math::cos(0.2)],
            }),
        );
        push_cmd(
            &mut frames,
            350,
            json!({ "kind": "gunner", "clip": clip("pilot"), "pitch": 0.0, "turn": 0.0, "time": 0.13, "seat": Value::Null }),
        );
        push_cmd(
            &mut frames,
            370,
            json!({ "kind": "gunner", "clip": Value::Null, "pitch": 0.0, "turn": 0.0, "time": Value::Null, "seat": Value::Null }),
        );
        run_replay(
            &json!({ "content": fx.descriptor, "frames": frames }),
            "body",
            &goldens,
        );
    }

    // 2. First-person phase synthetic goldens (soldier and droideka).
    for forced in [false, true] {
        let mut fp_clips = Vec::with_capacity(46);
        let mut fp_ids = HashMap::new();
        let mut serial = 0usize;
        for bank in ["rifle", "tool", "bazooka", "grenade", "droideka"] {
            for slot in [
                "idle",
                "run",
                "walk",
                "shoot",
                "shoot2",
                "reload",
                "jump",
                "flail",
                "jump_land",
            ] {
                let name = format!("_1st_{bank}_{slot}");
                fp_ids.insert(name, fp_clips.len());
                fp_clips.push(fx.clips[serial].to_json(Some(false)));
                serial += 1;
            }
        }
        fp_ids.insert("_1st_handsdown".to_string(), fp_clips.len());
        fp_clips.push(fx.clips[serial].to_json(Some(false)));

        let slots = [
            "idle",
            "run",
            "shoot",
            "shoot2",
            "reload",
            "jump",
            "flail",
            "jump_land",
            "handsdown",
        ];
        let fp_sets: Vec<Vec<Option<usize>>> = ["rifle", "tool", "grenade", "bazooka"]
            .iter()
            .map(|&bank| {
                let set_name = if forced { "droideka" } else { bank };
                slots
                    .iter()
                    .map(|&slot| {
                        let clip_name = if slot == "handsdown" {
                            "_1st_handsdown"
                        } else {
                            first_person_set_clip(set_name, slot)
                        };
                        fp_ids.get(clip_name).copied()
                    })
                    .collect()
            })
            .collect();

        let mut offset = Matrix4::default();
        let mut rz = Matrix4::default();
        let mut ry = Matrix4::default();
        offset.make_translation(0.0, 0.13, -0.3645);
        rz.make_rotation_z(0.05);
        ry.make_rotation_y(3.0);
        offset.multiply(&rz);
        offset.multiply(&ry);

        let mut frames = scenarios();
        for (i, f_val) in frames.iter_mut().enumerate() {
            let shown = (i / 75) % 4;
            let obj = f_val.as_object_mut().unwrap();
            obj.insert("active".to_string(), json!(i < 550 && i != 340));
            obj.insert("zoomed".to_string(), json!(i >= 450));
            obj.insert("shown".to_string(), json!(shown));
            obj.insert("commands".to_string(), json!([]));
            let weapons: Vec<Value> = (0..4usize)
                .map(|set| {
                    json!({
                        "set": set,
                        "reloading": set == 1 && (90..105).contains(&i),
                        "handEmpty": set == 2 && (180..190).contains(&i),
                        "model": set,
                    })
                })
                .collect();
            obj.insert("weapons".to_string(), json!(weapons));
            let cmds = obj.get_mut("commands").unwrap().as_array_mut().unwrap();
            if i % 39 == 9 {
                cmds.push(json!({ "kind": "play", "event": 0, "weapon": shown }));
            }
            if i == 160 {
                cmds.push(json!({ "kind": "play", "event": 2, "weapon": shown }));
            }
            if i == 169 {
                cmds.push(json!({ "kind": "play", "event": 1, "weapon": shown }));
            }
            if i == 280 {
                cmds.push(json!({ "kind": "cue", "seconds": 0.4 }));
            }
        }

        let fp_descriptor = json!({
            "bones": fx.bones_json,
            "clips": fp_clips,
            "sets": fp_sets,
            "forcedSet": if forced { json!(0) } else { Value::Null },
            "offset": offset.elements,
        });
        run_replay(
            &json!({ "firstPerson": fp_descriptor, "frames": frames }),
            if forced {
                "view-droideka"
            } else {
                "view-soldier"
            },
            &goldens,
        );
    }

    // 3. Complete unit batch synthetic records.
    {
        let input_content: Input =
            serde_json::from_value(fx.descriptor.clone()).expect("deserialize Input");
        let loaded = input_content.load();
        let mut rig = DisplayRig::new(&loaded.skeleton);
        rig.tree.update_matrix_world(rig.root, true);
        let inverses: Vec<[f64; 16]> = rig
            .bones
            .iter()
            .map(|&id| {
                let mut inv = rig.tree.node(id).matrix_world;
                inv.invert();
                inv.elements
            })
            .collect();

        let batch_rows: Vec<Value> = serde_json::from_slice(
            &std::fs::read(root.join("tests/fixtures/presentation/unit-batches.json"))
                .expect("read unit-batches.json"),
        )
        .expect("parse unit-batches.json");

        let frames: Vec<Value> = batch_rows
            .iter()
            .map(|row| {
                let mut input = vec![0.0f64; f::STRIDE + u::STRIDE];
                for (k, v) in row["frame"].as_object().unwrap() {
                    pack_field(&mut input, 0, frame_offset(k), v);
                }
                input[f::VERSION] = VERSION as f64;
                for (k, v) in row["unit"].as_object().unwrap() {
                    pack_field(&mut input, f::STRIDE, unit_offset(k), v);
                }
                let cmd_rows = row["commands"].as_array().unwrap();
                let mut commands = vec![0.0f64; cmd_rows.len() * c::STRIDE];
                for (j, cmd_obj) in cmd_rows.iter().enumerate() {
                    for (k, v) in cmd_obj.as_object().unwrap() {
                        pack_field(&mut commands, j * c::STRIDE, command_offset(k), v);
                    }
                }
                json!({
                    "input": input,
                    "commands": commands,
                })
            })
            .collect();

        run_replay(
            &json!({
                "phase": true,
                "content": fx.descriptor,
                "inverses": inverses,
                "frames": frames,
            }),
            "batch",
            &goldens,
        );
    }
}
