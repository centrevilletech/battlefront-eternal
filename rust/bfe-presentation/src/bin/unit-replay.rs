//! Native display-pose oracle. Descriptors and records are supplied privately
//! by the caller; synthetic fixtures require no installed game content.
use bfe_pose::display::{Animator, DisplayRig, Gunning, SoldierState, NONE};
use bfe_presentation::unit_content::Input;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Read};
use three::{Quaternion, Vector3};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    yaw: f64,
    pitch: f64,
    velocity: [f64; 3],
    posture: usize,
    air: u32,
    dive: f64,
    dive_time: f64,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Command {
    Reset {
        yaw: f64,
    },
    Die {
        clip: Option<usize>,
    },
    Bank {
        bank: usize,
    },
    Fire,
    HoldAlert,
    Weapon {
        action: u32,
        clip: Option<usize>,
        shooting: bool,
    },
    EndWeapon {
        action: Option<u32>,
    },
    Gunner {
        clip: Option<usize>,
        pitch: f64,
        turn: f64,
        time: Option<f64>,
        seat: Option<[f64; 4]>,
    },
}
#[derive(Deserialize)]
struct Frame {
    dt: f64,
    state: State,
    commands: Vec<Command>,
    position: [f64; 3],
}
#[derive(Deserialize)]
struct Replay {
    content: Input,
    frames: Vec<Frame>,
}
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    if input.contains("\"phase\"") {
        unit_phase(&input);
        return;
    }
    if input.contains("\"firstPerson\"") {
        first_person(&input);
        return;
    }
    let replay: Replay = serde_json::from_str(&input).unwrap();
    let c = replay.content.load();
    let mut rig = DisplayRig::new(&c.skeleton);
    let mut animator = Animator::new(&rig, c.clips.len());
    let mut out: Vec<Value> = Vec::new();
    for f in replay.frames {
        for cmd in f.commands {
            match cmd {
                Command::Reset { yaw } => animator.reset(yaw),
                Command::Die { clip } => animator.die(clip.unwrap_or(NONE)),
                Command::Bank { bank } => animator.set_bank(bank),
                Command::Fire => animator.fire(),
                Command::HoldAlert => animator.hold_alert(),
                Command::Weapon {
                    action,
                    clip,
                    shooting,
                } => animator.weapon_action(action, clip.unwrap_or(NONE), shooting),
                Command::EndWeapon { action } => animator.end_weapon_action(action),
                Command::Gunner {
                    clip,
                    pitch,
                    turn,
                    time,
                    seat,
                } => animator.gunner(clip.map(|clip| Gunning {
                    clip,
                    pitch,
                    turn,
                    time,
                    seat: seat.map(|q| Quaternion::new(q[0], q[1], q[2], q[3])),
                })),
            }
        }
        let s = f.state;
        let state = SoldierState {
            yaw: s.yaw,
            pitch: s.pitch,
            velocity: Vector3::new(s.velocity[0], s.velocity[1], s.velocity[2]),
            posture: s.posture,
            air: s.air,
            dive: s.dive,
            dive_time: s.dive_time,
        };
        rig.tree.node_mut(rig.root).position =
            Vector3::new(f.position[0], f.position[1], f.position[2]);
        animator.update(f.dt, &state, &mut rig, &c.tables, &c.clips);
        rig.tree.update_matrix_world(rig.root, true);
        let mut pose = Vec::new();
        for &id in &rig.bones {
            let b = rig.tree.node(id);
            pose.extend_from_slice(&[
                b.quaternion.x,
                b.quaternion.y,
                b.quaternion.z,
                b.quaternion.w,
                b.position.x,
                b.position.y,
                b.position.z,
            ]);
        }
        let mut world = Vec::new();
        for &id in &rig.bones {
            world.extend_from_slice(&rig.tree.node(id).matrix_world.elements);
        }
        let root = rig.tree.node(rig.root);
        out.push(json!({"pose":pose,"world":world,"root":[root.quaternion.x,root.quaternion.y,root.quaternion.z,root.quaternion.w],
            "state":[animator.time,animator.facing,animator.legs,animator.action_time,animator.gait as f64,animator.posture as f64,animator.alert,animator.aim_yaw,animator.aim_pitch]}));
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum FirstCommand {
    Play { event: u32, weapon: usize },
    Cue { seconds: f64 },
}
#[derive(Deserialize)]
struct FirstFrame {
    dt: f64,
    state: State,
    active: bool,
    zoomed: bool,
    shown: Option<usize>,
    commands: Vec<FirstCommand>,
    weapons: Vec<WeaponInput>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeaponInput {
    set: usize,
    reloading: bool,
    hand_empty: bool,
    model: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirstReplay {
    first_person: bfe_presentation::unit_content::FirstPersonInput,
    frames: Vec<FirstFrame>,
}
fn first_person(input: &str) {
    use bfe_pose::first_person::{FirstPerson, Weapon};
    let replay: FirstReplay = serde_json::from_str(input).unwrap();
    let (mut rig, t, clips) = replay.first_person.load();
    let mut first = FirstPerson::new(&rig);
    let mut out = Vec::new();
    for f in replay.frames {
        let weapons: Vec<_> = f
            .weapons
            .into_iter()
            .map(|w| Weapon {
                set: w.set,
                reloading: w.reloading,
                hand_empty: w.hand_empty,
                model: w.model,
            })
            .collect();
        for c in f.commands {
            match c {
                FirstCommand::Play { event, weapon } => {
                    first.play(event, weapon, &rig, &t, &clips, &weapons)
                }
                FirstCommand::Cue { seconds } => first.cue(seconds, &rig, &t, &weapons),
            }
        }
        let s = f.state;
        let s = SoldierState {
            yaw: s.yaw,
            pitch: s.pitch,
            velocity: Vector3::new(s.velocity[0], s.velocity[1], s.velocity[2]),
            posture: s.posture,
            air: s.air,
            dive: s.dive,
            dive_time: s.dive_time,
        };
        first.update(
            f.dt,
            f.active,
            f.zoomed,
            &s,
            f.shown.unwrap_or(NONE),
            &mut rig,
            &t,
            &clips,
            &weapons,
        );
        if first.was_active {
            rig.place_camera(&three::Matrix4::default());
        }
        let mut pose = Vec::new();
        let mut world = Vec::new();
        for &id in &rig.bones {
            let b = rig.tree.node(id);
            pose.extend_from_slice(&[
                b.quaternion.x,
                b.quaternion.y,
                b.quaternion.z,
                b.quaternion.w,
                b.position.x,
                b.position.y,
                b.position.z,
            ]);
            world.extend_from_slice(&b.matrix_world.elements);
        }
        out.push(json!({"pose":pose,"world":world,"state":[if first.slot==NONE{-1.0}else{first.slot as f64},if first.clip==NONE{-1.0}else{first.clip as f64},first.time,first.blend,if first.holding==NONE{-1.0}else{first.holding as f64},if first.visible{1.0}else{0.0},if first.was_active{1.0}else{0.0}]}));
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}

#[derive(Deserialize)]
struct UnitPhaseFrame {
    input: Vec<f64>,
    commands: Vec<f64>,
}
#[derive(Deserialize)]
struct UnitPhaseReplay {
    content: Input,
    inverses: Vec<[f64; 16]>,
    frames: Vec<UnitPhaseFrame>,
}
fn unit_phase(input: &str) {
    use bfe_presentation::{
        layout::{unit_node as n, unit_output as o},
        units::Units,
    };
    let replay: UnitPhaseReplay = serde_json::from_str(input).unwrap();
    let bones = replay.inverses.len();
    let mut units = Units::new();
    let content = units.load(replay.content);
    units.actor(content, &replay.inverses);
    let mut frames = Vec::new();
    for frame in replay.frames {
        units.input[..frame.input.len()].copy_from_slice(&frame.input);
        units.commands[..frame.commands.len()].copy_from_slice(&frame.commands);
        units.frame();
        // The batch's remote soldier is drawn: the scene update places it.
        units.places[0] = 0;
        units.place_actors(1);
        let mut pose = Vec::new();
        let mut world = Vec::new();
        for bone in 0..bones {
            let at = (bone + 2) * n::STRIDE;
            pose.extend_from_slice(&units.nodes[at + n::QUATERNION..at + n::QUATERNION + 4]);
            pose.extend_from_slice(&units.nodes[at + n::POSITION..at + n::POSITION + 3]);
            world.extend_from_slice(&units.nodes[at + n::WORLD..at + n::WORLD + 16]);
        }
        let mut root = Vec::new();
        root.extend_from_slice(&units.nodes[n::QUATERNION..n::QUATERNION + 4]);
        frames.push(json!({"pose":pose,"world":world,"root":root,"state":&units.output[o::ANIMATOR..o::ANIMATOR+9],"presentation":&units.output[o::POSITION..o::DEATH_PENDING+1],"skin":&units.skin[..bones*16],"status":units.header[1]}));
    }
    println!("{}", serde_json::to_string(&frames).unwrap());
}
