//! Unit.present and SoldierPresentation.update as a batched frame owner.
//! Inputs are snapshots and ordered scalar commands. Poses, interpolation
//! clocks, world matrices, inverse binds, and skin records stay in Rust.
use crate::layout::{
    unit_command as c, unit_command_kind as k, unit_frame as f, unit_header as h, unit_input as i,
    unit_node as n, unit_output as o, unit_weapon as w, VERSION,
};
use crate::unit_content::{Content, FirstPersonInput, Input};
use bfe_pose::display::{Action, Animator, DisplayRig, Gunning, SoldierState, NONE};
use bfe_pose::first_person::{FirstPerson, Rig as FirstRig, Tables as FirstTables, Weapon};
use bfe_pose::{js_max as max, js_min as min};
use serde::Deserialize;
use three::{Matrix4, NodeId, Quaternion, TransformNode, TransformTree, Vector3};

use crate::layout::unit_limits::{
    ACTORS as MAX_ACTORS, COMMANDS as MAX_COMMANDS, NODES as MAX_NODES, WEAPONS as MAX_WEAPONS,
};

#[derive(Deserialize)]
pub struct Attachment {
    pub parent: u32,
    pub pose: [f64; 10],
}

struct Actor {
    content: usize,
    rig: DisplayRig,
    animator: Animator,
    walker: Option<bfe_pose::rig::droideka::DroidekaAnimator>,
    pelvis: Option<usize>,
    chest: Option<usize>,
    generation: f64,
    pose_clock: f64,
    step_offset: f64,
    last_step: f64,
    network: bool,
    fire_epoch: f64,
    fire_generation: f64,
    fire_tick: f64,
    fire_counter: f64,
    rider_lean: f64,
    node_start: usize,
    skin_start: usize,
    inverses: Vec<Matrix4>,
    /// Whether the bones' local matrices must be composed at placement.
    compose: bool,
    /// Posed but not yet placed: its world matrices, skin and node records
    /// wait for `place_actors` or `refresh_actor`.
    stale: bool,
}
struct View {
    rig: FirstRig,
    animator: FirstPerson,
    tables: FirstTables,
    clips: Vec<bfe_pose::display::DisplayClip>,
    node_start: usize,
}

/// Fixed exchange storage. Resource registration may allocate; frame() never
/// grows a collection, including when all actors change pose in one frame.
pub struct Units {
    pub input: Vec<f64>,
    pub commands: Vec<f64>,
    pub output: Vec<f64>,
    pub nodes: Vec<f64>,
    pub changed: Vec<u32>,
    /// Actor ids for `place_actors`, written by the host.
    pub places: Vec<u32>,
    pub skin: Vec<f32>,
    pub header: [f64; h::STRIDE],
    pub weapon_input: Vec<f64>,
    contents: Vec<Content>,
    actors: Vec<Actor>,
    views: Vec<View>,
    weapons: [Weapon; MAX_WEAPONS],
    weapon_revisions: [f64; MAX_WEAPONS],
    node_count: usize,
    skin_count: usize,
}
impl Default for Units {
    fn default() -> Self {
        Self::new()
    }
}
impl Units {
    pub fn new() -> Self {
        Self::with_capacity(MAX_ACTORS, MAX_NODES, MAX_COMMANDS, MAX_WEAPONS)
    }
    /// A menu owns a small instance of the same kernel; it does not reserve
    /// a battle's exchange buffers for its single class preview.
    pub fn preview() -> Self {
        Self::with_capacity(1, 1024, 64, MAX_WEAPONS)
    }
    fn with_capacity(actors: usize, nodes: usize, commands: usize, weapons: usize) -> Self {
        Self {
            input: vec![0.0; f::STRIDE + actors * i::STRIDE],
            commands: vec![0.0; commands * c::STRIDE],
            output: vec![0.0; actors * o::STRIDE],
            nodes: vec![0.0; nodes * n::STRIDE],
            changed: vec![0; nodes],
            places: vec![0; actors],
            skin: vec![0.0; nodes * 16],
            header: [0.0; h::STRIDE],
            weapon_input: vec![0.0; weapons * w::STRIDE],
            contents: Vec::new(),
            actors: Vec::with_capacity(actors),
            views: Vec::new(),
            weapons: [Weapon::default(); MAX_WEAPONS],
            weapon_revisions: [-1.0; MAX_WEAPONS],
            node_count: 0,
            skin_count: 0,
        }
    }
    pub fn load(&mut self, input: Input) -> usize {
        let id = self.contents.len();
        self.contents.push(input.load());
        id
    }
    pub fn actor(&mut self, content: usize, inverses: &[[f64; 16]]) -> usize {
        assert!(
            self.actors.len() < self.output.len() / o::STRIDE,
            "unit capacity"
        );
        let data = &self.contents[content];
        let rig = DisplayRig::new(&data.skeleton);
        let animator = Animator::new(&rig, data.clips.len());
        let id = self.actors.len();
        let node_start = self.node_count;
        self.node_count += rig.tree.nodes.len();
        self.skin_count += rig.bones.len();
        assert!(
            self.node_count <= self.nodes.len() / n::STRIDE
                && self.skin_count <= self.skin.len() / 16,
            "unit node capacity"
        );
        assert_eq!(inverses.len(), rig.bones.len(), "bone inverse count");
        let pelvis = data.skeleton.by_name("bone_pelvis");
        let chest = data.skeleton.by_name("bone_ribcage");
        let walker = data.walker.as_ref().map(|w| {
            let mut rig = bfe_pose::rig::droideka::DroidekaRig::new(
                &data.skeleton,
                &[],
                Vector3::new(w.roll_center[0], w.roll_center[1], w.roll_center[2]),
            );
            rig.animator.pivot = Vector3::new(w.pivot[0], w.pivot[1], w.pivot[2]);
            rig.animator
        });
        let inverses: Vec<_> = inverses
            .iter()
            .map(|a| {
                let mut m = Matrix4::default();
                m.from_array(a, 0);
                m
            })
            .collect();
        self.actors.push(Actor {
            content,
            rig,
            animator,
            walker,
            pelvis,
            chest,
            generation: -1.0,
            pose_clock: 0.0,
            step_offset: 0.0,
            last_step: 0.0,
            network: false,
            fire_epoch: -1.0,
            fire_generation: -1.0,
            fire_tick: -1.0,
            fire_counter: 0.0,
            rider_lean: 0.0,
            node_start,
            skin_start: self.skin_count - inverses.len(),
            inverses,
            compose: true,
            stale: false,
        });
        id
    }
    /// Attachments are load-time rigid nodes: guns, muzzle markers, shadow
    /// proxies, and their children. Only the newest actor can grow its range.
    pub fn attach(&mut self, actor: usize, attachments: &[Attachment]) {
        assert_eq!(
            actor + 1,
            self.actors.len(),
            "register complete actor before next actor"
        );
        let a = &mut self.actors[actor];
        assert_eq!(
            a.node_start + a.rig.tree.nodes.len(),
            self.node_count,
            "actor node range"
        );
        for d in attachments {
            let node = a.rig.tree.add_node(attachment_node(d.pose));
            a.rig.tree.add_child(d.parent, node);
            self.node_count += 1;
        }
        assert!(
            self.node_count <= self.nodes.len() / n::STRIDE,
            "unit node capacity"
        );
    }
    pub fn view(&mut self, input: FirstPersonInput) -> usize {
        let (rig, tables, clips) = input.load();
        let animator = FirstPerson::new(&rig);
        let node_start = self.node_count;
        self.node_count += rig.tree.nodes.len();
        assert!(
            self.node_count <= self.nodes.len() / n::STRIDE,
            "view node capacity"
        );
        let id = self.views.len();
        self.views.push(View {
            rig,
            tables,
            clips,
            animator,
            node_start,
        });
        id
    }
    pub fn view_attach(&mut self, view: usize, attachments: &[Attachment]) {
        let a = &mut self.views[view];
        assert_eq!(
            a.node_start + a.rig.tree.nodes.len(),
            self.node_count,
            "view node range"
        );
        for d in attachments {
            let id = a.rig.tree.add_node(attachment_node(d.pose));
            a.rig.tree.add_child(d.parent, id);
            self.node_count += 1;
        }
        assert!(
            self.node_count <= self.nodes.len() / n::STRIDE,
            "view node capacity"
        );
    }
    pub fn actor_node_start(&self, id: usize) -> usize {
        self.actors[id].node_start
    }
    pub fn actor_skin_start(&self, id: usize) -> usize {
        self.actors[id].skin_start
    }
    pub fn view_node_start(&self, id: usize) -> usize {
        self.views[id].node_start
    }

    /// One presented frame, in original command order. Invalid counts fail
    /// explicitly and retain the previous outputs rather than running JS.
    pub fn frame(&mut self) {
        self.pose_frame();
        self.attachment_frame();
    }

    /// Unit poses and their persistent clocks. The web host consumes this
    /// batch before its camera phase; a native replay already has both sets
    /// of inputs and calls frame() to execute the same two phases together.
    pub fn pose_frame(&mut self) {
        self.header.fill(0.0);
        self.header[h::VERSION] = VERSION as f64;
        // A mismatched host is a build error. Counts beyond the stores are
        // clamped: a frame never fails because a queue filled.
        if self.input[f::VERSION] != VERSION as f64 {
            self.header[h::STATUS] = 1.0;
            return;
        }
        let actors = self.actor_rows();
        let commands = (self.input[f::COMMANDS] as usize).min(self.commands.len() / c::STRIDE);
        for (at, weapon) in self.weapons.iter_mut().enumerate() {
            let p = at * w::STRIDE;
            if p >= self.weapon_input.len() {
                break;
            }
            let revision = self.weapon_input[p + w::REVISION];
            if self.weapon_revisions[at] == revision {
                continue;
            }
            self.weapon_revisions[at] = revision;
            weapon.set = self.weapon_input[p + w::SET] as usize;
            weapon.reloading = self.weapon_input[p + w::RELOADING] != 0.0;
            weapon.hand_empty = self.weapon_input[p + w::HAND_EMPTY] != 0.0;
            weapon.model = index(self.weapon_input[p + w::MODEL]);
        }
        let view_id = index(self.input[f::FIRST_PERSON]);
        for at in 0..commands {
            let cmd = &self.commands[at * c::STRIDE..][..c::STRIDE];
            let kind = cmd[c::KIND] as u32;
            if kind == k::FIRST_PERSON_PLAY || kind == k::FIRST_PERSON_CUE {
                if let Some(v) = self.views.get_mut(view_id) {
                    if kind == k::FIRST_PERSON_PLAY {
                        v.animator.play(
                            cmd[c::ACTION] as u32,
                            index(cmd[c::WEAPON]),
                            &v.rig,
                            &v.tables,
                            &v.clips,
                            &self.weapons,
                        );
                    } else {
                        v.animator
                            .cue(cmd[c::SECONDS], &v.rig, &v.tables, &self.weapons);
                    }
                }
                continue;
            }
            // A command for an actor this kernel never registered is dropped.
            let Some(a) = self.actors.get_mut(cmd[c::ACTOR] as usize) else {
                continue;
            };
            match kind {
                k::RESET => a.animator.reset(cmd[c::YAW]),
                k::DIE => a.animator.die(index(cmd[c::CLIP])),
                k::BANK => a.animator.set_bank(cmd[c::BANK] as usize),
                k::FIRE => a.animator.fire(),
                k::HOLD_ALERT => a.animator.hold_alert(),
                k::WEAPON => a.animator.weapon_action(
                    cmd[c::ACTION] as u32,
                    index(cmd[c::CLIP]),
                    cmd[c::SHOOTING] != 0.0,
                ),
                k::END_WEAPON => a.animator.end_weapon_action(if cmd[c::ACTION] < 0.0 {
                    None
                } else {
                    Some(cmd[c::ACTION] as u32)
                }),
                k::GUNNER => a.animator.gunner(if cmd[c::CLIP] < 0.0 {
                    None
                } else {
                    Some(Gunning {
                        clip: cmd[c::CLIP] as usize,
                        pitch: cmd[c::PITCH],
                        turn: cmd[c::TURN],
                        time: if cmd[c::HAS_TIME] != 0.0 {
                            Some(cmd[c::TIME])
                        } else {
                            None
                        },
                        seat: if cmd[c::HAS_SEAT] != 0.0 {
                            Some(quaternion(cmd, c::SEAT_QUATERNION))
                        } else {
                            None
                        },
                    })
                }),
                k::FIDGET => a.animator.start(Action::Fidget, index(cmd[c::CLIP])),
                k::END_FIDGET => {
                    if a.animator.action == Action::Fidget {
                        a.animator.action = Action::Loco;
                    }
                }
                k::PRESENTATION_RESET => {
                    a.step_offset = 0.0;
                    a.last_step = cmd[c::TIME];
                    a.network = false;
                    a.fire_epoch = -1.0;
                    a.fire_generation = -1.0;
                    a.fire_tick = -1.0;
                }
                _ => {}
            }
        }
        let dt = self.input[f::DT];
        let mut own_state = SoldierState::default();
        let mut changed = 0;
        for row in 0..actors {
            let start = f::STRIDE + row * i::STRIDE;
            let input = &self.input[start..start + i::STRIDE];
            let id = input[i::ACTOR] as usize;
            let Some(a) = self.actors.get_mut(id) else {
                continue;
            };
            if input[i::PRESENT] == 0.0 || input[i::REMOVED] != 0.0 {
                continue;
            }
            let out = &mut self.output[id * o::STRIDE..][..o::STRIDE];
            let previous: [f64; o::STRIDE] = (&*out).try_into().unwrap();
            out[o::CHANGED] = 0.0;
            a.rig.set_parent(if input[i::HAS_POSE_PARENT] != 0.0 {
                Some(matrix(input, i::POSE_PARENT))
            } else {
                None
            });
            let content = &self.contents[a.content];
            if a.generation != input[i::GENERATION] {
                a.generation = input[i::GENERATION];
                a.pose_clock = 0.0;
                a.rider_lean = 0.0;
                a.step_offset = 0.0;
                a.last_step = input[i::PREVIOUS_STEP];
                a.network = false;
                a.fire_tick = -1.0;
            }
            out[o::GONE] = 0.0;
            out[o::GONE_BIRTH] = 0.0;
            if input[i::GONE] != 0.0 && input[i::GONE_GENERATION] != input[i::GENERATION] {
                out[o::GONE] = -2.0;
            } else {
                let birth = a.walker.is_some()
                    && input[i::DEAD] != 0.0
                    && input[i::WALKER_MODE] == 4.0
                    && input[i::GONE] == 0.0;
                if input[i::GONE] != 0.0 || birth {
                    let time = if birth { 0.0 } else { input[i::GONE_TIME] } + max(dt, 0.0);
                    out[o::GONE_TIME] = time;
                    out[o::GONE_BIRTH] = if birth { 1.0 } else { 0.0 };
                    out[o::GONE] = if a.walker.is_none() || time >= 0.5 {
                        -1.0
                    } else {
                        max(time / 0.5, 1e-3)
                    };
                    if out[o::GONE] == -1.0 {
                        mark_changed(&previous, out);
                        continue;
                    }
                }
            }
            if input[i::OWN] != 0.0 {
                write_vector(out, o::POSITION, vector(input, i::POSITION));
                write_vector(out, o::VELOCITY, vector(input, i::VELOCITY));
                out[o::YAW] = input[i::YAW];
                out[o::PITCH] = input[i::PITCH];
                out[o::POSTURE] = input[i::POSTURE];
                out[o::AIR] = input[i::AIR];
                out[o::DIVE] = input[i::DIVE];
                out[o::DEAD] = input[i::DEAD];
                out[o::NETWORK_FIRED] = 0.0;
                out[o::DEATH_PENDING] = input[i::DEATH_PENDING];
            } else {
                interpolate(a, input, dt, out);
            }
            let mut walker = [
                input[i::WALKER_ROLL],
                input[i::WALKER_LEAN],
                input[i::WALKER_LEGS],
                input[i::WALKER_TIME],
            ];
            if a.walker.is_some() && input[i::NETWORK] == 0.0 && input[i::OWN] == 0.0 {
                let alpha = max(0.0, min(1.0, input[i::ALPHA]));
                for key in 0..4 {
                    let previous = input[i::PREVIOUS_WALKER_POSE + key];
                    let current = walker[key];
                    let same = input[i::PREVIOUS_WALKER_MODE] == input[i::WALKER_MODE]
                        && previous.is_finite()
                        && current.is_finite();
                    let restart = key == 3
                        && if input[i::WALKER_REVERSE] != 0.0 {
                            current > previous
                        } else {
                            current < previous
                        };
                    if same && !restart {
                        walker[key] = (1.0 - alpha) * previous + alpha * current;
                    }
                }
            }
            out[o::WALKER_POSE..o::WALKER_POSE + 4].copy_from_slice(&walker);
            let mut state = SoldierState {
                yaw: out[o::YAW] + input[i::YAW_OFFSET],
                pitch: out[o::PITCH] + input[i::PITCH_OFFSET],
                velocity: vector(out, o::VELOCITY),
                posture: out[o::POSTURE] as usize,
                air: out[o::AIR] as u32,
                dive: out[o::DIVE],
                dive_time: input[i::DIVE_TIME],
            };
            out[o::YAW] = state.yaw;
            out[o::PITCH] = state.pitch;
            let held = input[i::HELD] != 0.0;
            if held {
                let m = matrix(input, i::HELD_MATRIX);
                a.rig
                    .tree
                    .node_mut(a.rig.root)
                    .position
                    .set_from_matrix_position(&m);
                let p = a.rig.tree.node(a.rig.root).position;
                write_vector(out, o::POSITION, p);
                a.animator.gunner(if input[i::HELD_CLIP] < 0.0 {
                    None
                } else {
                    Some(Gunning {
                        clip: input[i::HELD_CLIP] as usize,
                        pitch: 0.0,
                        turn: 0.0,
                        time: Some(input[i::HELD_TIME]),
                        seat: None,
                    })
                });
            }
            if input[i::OWN] == 0.0
                && out[o::NETWORK_FIRED] != 0.0
                && out[o::DEAD] == 0.0
                && input[i::SEAT] == 0.0
            {
                a.animator.hold_alert();
            }
            if input[i::OWN] == 0.0 && out[o::DEATH_PENDING] != 0.0 && out[o::DEAD] != 0.0 {
                out[o::DEATH_PENDING] = 0.0;
                a.animator.die(index(input[i::DEATH_CLIP]));
            }
            let mut position = vector(out, o::POSITION);
            position.add(&vector(input, i::CORRECTION));
            a.rig.tree.node_mut(a.rig.root).position = position;
            // External owners sometimes change the root at a seat/release.
            // Recopying an unchanged quaternion would spuriously synchronize
            // its Euler (including signed zero), unlike the original frame.
            let q = quaternion(input, i::ROOT_QUATERNION);
            let root = a.rig.tree.node_mut(a.rig.root);
            if root.quaternion.x != q.x
                || root.quaternion.y != q.y
                || root.quaternion.z != q.z
                || root.quaternion.w != q.w
            {
                root.copy_quaternion(&q);
            }
            // Distant animation uses accumulated visual dt. Interpolation and
            // birth/death edge recovery still run on every presented frame.
            a.pose_clock += dt;
            let every =
                if input[i::OWN] != 0.0 || a.walker.is_some() || self.input[f::HAS_VIEW] == 0.0 {
                    1
                } else {
                    lod(&self.input, input, position)
                };
            let d2 = vector(input, i::AUTHORITATIVE_POSITION)
                .distance_to_squared(&vector(&self.input, f::CAMERA_POSITION));
            out[o::SHADOW_NEAR] =
                if out[o::GONE] <= 0.0 && (self.input[f::HAS_VIEW] == 0.0 || d2 < 70.0 * 70.0) {
                    1.0
                } else {
                    0.0
                };
            let posed = every == 1 || (self.input[f::FRAME] + input[i::ID]) % every as f64 == 0.0;
            if posed {
                if let (Some(anim), Some(w)) = (&mut a.walker, &content.walker) {
                    let clips = bfe_pose::rig::droideka::DroidekaClips {
                        by_mode: w.by_mode.map(|id| id.map(|i| &content.clips[i].sample)),
                    };
                    let d = bfe_pose::rig::droideka::DroidekaState {
                        mode: input[i::WALKER_MODE] as i32,
                        time: walker[3],
                        dead: input[i::DEAD] != 0.0,
                        legs: walker[2],
                        aim_pitch: state.pitch,
                        aim_yaw: state.yaw,
                        roll: walker[0],
                        lean: walker[1],
                        speed: input[i::WALKER_SPEED],
                        position,
                    };
                    bfe_pose::rig::droideka::update_display(
                        anim,
                        &mut a.rig.tree,
                        &a.rig.bones,
                        a.rig.root,
                        a.pose_clock,
                        &d,
                        &clips,
                    );
                } else {
                    a.animator.update(
                        a.pose_clock,
                        &state,
                        &mut a.rig,
                        &content.tables,
                        &content.clips,
                    );
                }
                a.pose_clock = 0.0;
            }
            if held {
                let m = matrix(input, i::HELD_MATRIX);
                let mut p = Vector3::default();
                let mut q = Quaternion::default();
                let mut scale = Vector3::default();
                m.decompose(&mut p, &mut q, &mut scale);
                let root = a.rig.tree.node_mut(a.rig.root);
                root.position = p;
                root.copy_quaternion(&q);
            }
            if input[i::RIDER] != 0.0 {
                place_rider(a, input, dt);
            }
            // A new pose, or a root placed from a held/rider matrix, needs
            // its bones composed again. Unit.present's LOD skip otherwise
            // retains every bone's local pose and its local matrices.
            if posed || held || input[i::RIDER] != 0.0 {
                a.compose = true;
            }
            out[o::POSE_CLOCK] = a.pose_clock;
            out[o::RIDER_LEAN] = a.rider_lean;
            out[o::GENERATION] = a.generation;
            out[o::NODE_START] = a.node_start as f64;
            out[o::NODE_COUNT] = a.rig.tree.nodes.len() as f64;
            out[o::SKIN_START] = a.skin_start as f64;
            out[o::SKIN_COUNT] = a.rig.bones.len() as f64;
            out[o::ACTION] = a.animator.action as u32 as f64;
            let anim = &a.animator;
            out[o::ANIMATOR..o::ANIMATOR + 9].copy_from_slice(&[
                anim.time,
                anim.facing,
                anim.legs,
                anim.action_time,
                anim.gait as f64,
                anim.posture as f64,
                anim.alert,
                anim.aim_yaw,
                anim.aim_pitch,
            ]);
            // Graphics.updateVisible places a soldier's bones only when it is
            // drawn: it skips hidden roots (pooled soldiers, hidden gunners)
            // and, through cullOffView, soldiers out of the camera's view. A
            // remote actor is therefore placed after the camera, by
            // place_actors for the drawn ones or by refresh_actor when a
            // matrix is asked of it. The own body feeds the camera: placed now.
            if input[i::OWN] != 0.0 {
                changed = place(
                    a,
                    id,
                    out,
                    &mut self.skin,
                    &mut self.nodes,
                    &mut self.changed,
                    changed,
                );
            } else {
                a.stale = true;
                out[o::STALE] = 1.0;
                out[o::SKIN_CHANGED] = 0.0;
                publish_root(&mut a.rig.tree, a.rig.root, a.node_start, &mut self.nodes);
            }
            mark_changed(&previous, out);
            if input[i::OWN] != 0.0 {
                state.yaw = out[o::YAW];
                state.pitch = out[o::PITCH];
                own_state = state;
            }
        }
        if let Some(v) = self.views.get_mut(view_id) {
            v.animator.update(
                dt,
                self.input[f::FIRST_PERSON_ACTIVE] != 0.0,
                self.input[f::FIRST_PERSON_ZOOMED] != 0.0,
                &own_state,
                index(self.input[f::FIRST_PERSON_SHOWN]),
                &mut v.rig,
                &v.tables,
                &v.clips,
                &self.weapons,
            );
            let a = &v.animator;
            self.header[h::FIRST_PERSON_HOLDING] = number(a.holding);
            self.header[h::FIRST_PERSON_MODEL] = number(a.model);
            self.header[h::FIRST_PERSON_VISIBLE] = if a.visible { 1.0 } else { 0.0 };
            self.header[h::FIRST_PERSON_STATE..h::FIRST_PERSON_STATE + 7].copy_from_slice(&[
                number(a.slot),
                number(a.clip),
                a.time,
                a.blend,
                number(a.holding),
                if a.visible { 1.0 } else { 0.0 },
                if a.was_active { 1.0 } else { 0.0 },
            ]);
        }
        self.header[h::CHANGED] = changed as f64;
    }
    /// First-person world matrices and attachments. The input camera is the
    /// current pre-shake camera, supplied after it consumed the body output.
    /// No animation clock or event is advanced in this phase.
    pub fn attachment_frame(&mut self) {
        if self.header[h::STATUS] != 0.0 {
            return;
        }
        // The interface may place a preview after its pose phase. Refresh
        // only that parent-dependent world chain; clocks remain untouched.
        let mut changed = 0;
        for row in 0..self.actor_rows() {
            let input = &self.input[f::STRIDE + row * i::STRIDE..][..i::STRIDE];
            if input[i::PRESENT] == 0.0 || input[i::REMOVED] != 0.0 {
                continue;
            }
            let id = input[i::ACTOR] as usize;
            let Some(a) = self.actors.get_mut(id) else {
                continue;
            };
            let parent = if input[i::HAS_ATTACHMENT_PARENT] != 0.0 {
                Some(matrix(input, i::ATTACHMENT_PARENT))
            } else {
                None
            };
            let out = &mut self.output[id * o::STRIDE..][..o::STRIDE];
            out[o::CHANGED] = 0.0;
            // A deferred actor keeps the new parent for its placement.
            if !a.rig.set_parent(parent) || a.stale {
                continue;
            }
            a.rig.tree.update_matrix_world(a.rig.root, true);
            publish_skin(a, out, &mut self.skin);
            out[o::CHANGED] = 1.0;
            changed = publish_nodes(
                &mut a.rig.tree,
                a.node_start,
                id,
                &mut self.nodes,
                &mut self.changed,
                changed,
            );
        }
        self.header[h::CHANGED] = changed as f64;
        let view_id = index(self.input[f::FIRST_PERSON]);
        if let Some(v) = self.views.get_mut(view_id) {
            if v.animator.was_active {
                v.rig
                    .place_camera(&matrix(&self.input, f::FIRST_PERSON_CAMERA));
            }
            self.header[h::CHANGED] = publish_nodes(
                &mut v.rig.tree,
                v.node_start,
                MAX_ACTORS + view_id,
                &mut self.nodes,
                &mut self.changed,
                self.header[h::CHANGED] as usize,
            ) as f64;
        }
    }
    /// Places the deferred actors listed in `places`: the soldiers the
    /// scene update will draw this frame. Their clocks and poses do not
    /// advance; an actor placed since its phase is passed over.
    pub fn place_actors(&mut self, count: usize) {
        self.header[h::CHANGED] = 0.0;
        let mut changed = 0;
        for k in 0..count.min(self.places.len()) {
            let id = self.places[k] as usize;
            let Some(a) = self.actors.get_mut(id) else {
                continue;
            };
            if a.stale {
                let out = &mut self.output[id * o::STRIDE..][..o::STRIDE];
                changed = place(
                    a,
                    id,
                    out,
                    &mut self.skin,
                    &mut self.nodes,
                    &mut self.changed,
                    changed,
                );
            }
        }
        self.header[h::CHANGED] = changed as f64;
    }
    /// One deferred actor asked for a matrix before the scene update.
    pub fn refresh_actor(&mut self, id: usize) {
        if self.places.is_empty() {
            return;
        }
        self.places[0] = id.min(u32::MAX as usize) as u32;
        self.place_actors(1);
    }
    /// The batch's actor rows, clamped to the input and output stores.
    fn actor_rows(&self) -> usize {
        (self.input[f::ACTORS] as usize)
            .min((self.input.len() - f::STRIDE) / i::STRIDE)
            .min(self.output.len() / o::STRIDE)
    }
}
// A deferred actor's root local record. Hosts read a soldier's root place
// every frame (culling, sound, labels); its matrices only once it is drawn.
fn publish_root(tree: &mut TransformTree, root: NodeId, start: usize, nodes: &mut [f64]) {
    let node = tree.node_mut(root);
    let row = &mut nodes[(start + root as usize) * n::STRIDE..][..n::STRIDE];
    let (p, q, s) = (node.position, node.quaternion, node.scale);
    let same = [p.x, p.y, p.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z]
        .iter()
        .zip(&row[n::POSITION..n::SCALE + 3])
        .all(|(a, b)| a.to_bits() == b.to_bits());
    if same && row[n::LOCAL_VERSION] != 0.0 {
        return;
    }
    row[n::LOCAL_VERSION] += 1.0;
    row[n::EULER_SYNC] = if node.euler_sync { 1.0 } else { 0.0 };
    if node.euler_sync {
        let rotation = node.rotation();
        row[n::EULER..n::EULER + 3].copy_from_slice(&[rotation.x, rotation.y, rotation.z]);
    }
    row[n::POSITION..n::SCALE + 3]
        .copy_from_slice(&[p.x, p.y, p.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z]);
}
// Graphics.updateVisible over one actor's tree: world matrices from the pose
// and parent of its last phase, then its skin and node records.
fn place(
    a: &mut Actor,
    id: usize,
    out: &mut [f64],
    skin: &mut [f32],
    nodes: &mut [f64],
    changed: &mut [u32],
    count: usize,
) -> usize {
    // Without a new pose the local matrices already hold the bones' pose;
    // only their world chain follows the root.
    let retained = !a.compose;
    if retained {
        for &bone in &a.rig.bones {
            a.rig.tree.node_mut(bone).matrix_auto_update = false;
        }
    }
    a.rig.tree.update_matrix_world(a.rig.root, true);
    if retained {
        for &bone in &a.rig.bones {
            a.rig.tree.node_mut(bone).matrix_auto_update = true;
        }
    }
    a.compose = false;
    a.stale = false;
    out[o::STALE] = 0.0;
    publish_skin(a, out, skin);
    publish_nodes(&mut a.rig.tree, a.node_start, id, nodes, changed, count)
}
// buildActor's attached skin binding and Skeleton.update. Preserve f32
// rounding per lane; only changed matrices dirty the host's bone texture.
fn publish_skin(a: &Actor, out: &mut [f64], skin: &mut [f32]) {
    let root = a.rig.tree.node(a.rig.root).matrix_world;
    out[o::ROOT_WORLD..o::ROOT_WORLD + 16].copy_from_slice(&root.elements);
    let mut inverse = a.rig.tree.node(a.rig.body).matrix_world;
    inverse.invert();
    out[o::BIND_INVERSE..o::BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
    let mut skin_changed = false;
    for (bone, &node) in a.rig.bones.iter().enumerate() {
        let mut m = Matrix4::default();
        m.multiply_matrices(&a.rig.tree.node(node).matrix_world, &a.inverses[bone]);
        let at = (a.skin_start + bone) * 16;
        for k in 0..16 {
            let next = m.elements[k] as f32;
            // One changed lane already dirties the whole skeleton. Later
            // lanes keep their exact f32 values without repeating the test.
            if !skin_changed && skin[at + k].to_bits() != next.to_bits() {
                skin_changed = true;
            }
            skin[at + k] = next;
        }
    }
    out[o::SKIN_CHANGED] = if skin_changed { 1.0 } else { 0.0 };
}

// Only a touched mirror is copied by the web host. The change marker itself
// is transient, so a previously changed row can become unchanged at dt=0.
fn mark_changed(previous: &[f64], out: &mut [f64]) {
    for i in 0..o::CHANGED {
        if previous[i].to_bits() != out[i].to_bits() {
            out[o::CHANGED] = 1.0;
            return;
        }
    }
}

fn index(x: f64) -> usize {
    if x < 0.0 {
        NONE
    } else {
        x as usize
    }
}
fn number(x: usize) -> f64 {
    if x == NONE {
        -1.0
    } else {
        x as f64
    }
}
fn vector(a: &[f64], o: usize) -> Vector3 {
    Vector3::new(a[o], a[o + 1], a[o + 2])
}
fn quaternion(a: &[f64], o: usize) -> Quaternion {
    Quaternion::new(a[o], a[o + 1], a[o + 2], a[o + 3])
}
fn matrix(a: &[f64], o: usize) -> Matrix4 {
    let mut m = Matrix4::default();
    m.from_array(a, o);
    m
}
fn write_vector(a: &mut [f64], o: usize, p: Vector3) {
    a[o] = p.x;
    a[o + 1] = p.y;
    a[o + 2] = p.z;
}
fn node(p: [f64; 10]) -> TransformNode {
    let mut n = TransformNode::placed(
        Vector3::new(p[0], p[1], p[2]),
        Vector3::new(p[7], p[8], p[9]),
    );
    n.set_quaternion(p[3], p[4], p[5], p[6]);
    n
}
// Rigid gun/model attachments retain their authored local pose. Their
// world matrices still update with the animated parent, as Object3D does.
fn attachment_node(p: [f64; 10]) -> TransformNode {
    let mut n = node(p);
    n.update_matrix();
    n.matrix_auto_update = false;
    n
}

/// SoldierPresentation.update. Network samples have already selected their
/// historical timestamp, so world alpha must never interpolate them again.
fn interpolate(a: &mut Actor, input: &[f64], dt: f64, out: &mut [f64]) {
    out[o::NETWORK_FIRED] = 0.0;
    out[o::DEATH_PENDING] = input[i::DEATH_PENDING];
    if input[i::NETWORK] != 0.0 {
        write_vector(out, o::POSITION, vector(input, i::POSITION));
        write_vector(out, o::VELOCITY, vector(input, i::VELOCITY));
        out[o::YAW] = input[i::YAW];
        out[o::PITCH] = input[i::PITCH];
        let epoch = input[i::NETWORK_EPOCH];
        let generation = input[i::NETWORK_GENERATION];
        let tick = input[i::NETWORK_TICK];
        let counter = input[i::FIRE_COUNTER];
        if tick.is_finite() && tick >= 0.0 {
            if !a.network || epoch != a.fire_epoch || generation != a.fire_generation {
                a.fire_epoch = epoch;
                a.fire_generation = generation;
                a.fire_tick = tick;
                a.fire_counter = counter;
            } else if tick > a.fire_tick {
                let shots = counter - a.fire_counter;
                out[o::NETWORK_FIRED] = if (1.0..=3.0).contains(&shots) {
                    1.0
                } else {
                    0.0
                };
                a.fire_tick = tick;
                a.fire_counter = max(a.fire_counter, counter);
            }
        }
        a.network = true;
        a.step_offset = 0.0;
    } else {
        let alpha = max(0.0, min(1.0, input[i::ALPHA]));
        let mut p = vector(input, i::POSITION);
        let mut v = vector(input, i::VELOCITY);
        if !a.network {
            p.lerp_vectors(
                &vector(input, i::PREVIOUS_POSITION),
                &vector(input, i::POSITION),
                alpha,
            );
            v.lerp_vectors(
                &vector(input, i::PREVIOUS_VELOCITY),
                &vector(input, i::VELOCITY),
                alpha,
            );
        }
        a.network = false;
        a.fire_tick = -1.0;
        out[o::YAW] = input[i::YAW] - input[i::TICK_YAW] * (1.0 - alpha);
        out[o::PITCH] = input[i::PITCH];
        let step = (1.0 - alpha) * input[i::PREVIOUS_STEP] + alpha * input[i::GROUND_STEP];
        a.step_offset = max(-0.45, min(0.45, a.step_offset + step - a.last_step));
        a.last_step = step;
        a.step_offset *= v8math::exp(-max(dt, 0.0) / 0.075);
        p.y -= a.step_offset;
        write_vector(out, o::POSITION, p);
        write_vector(out, o::VELOCITY, v);
    }
    out[o::POSTURE] = input[i::POSTURE];
    out[o::AIR] = input[i::AIR];
    out[o::DIVE] = input[i::DIVE];
    out[o::DEAD] = input[i::DEAD];
}

/// Unit.present's frustum sphere, centered 0.9 m over the drawn feet.
fn lod(frame: &[f64], input: &[f64], position: Vector3) -> u32 {
    for plane in 0..6 {
        let p = f::FRUSTUM + plane * 4;
        let distance = frame[p] * position.x
            + frame[p + 1] * (position.y + 0.9)
            + frame[p + 2] * position.z
            + frame[p + 3];
        if distance < -1.5 {
            return 6;
        }
    }
    if vector(frame, f::CAMERA_POSITION)
        .distance_to_squared(&vector(input, i::AUTHORITATIVE_POSITION))
        > 120.0 * 120.0
    {
        2
    } else {
        1
    }
}

/// seatRider from src/vehicles.js. The current hull's rider matrix is an
/// input; body and pelvis matrices are resolved here after animation.
fn place_rider(a: &mut Actor, input: &[f64], dt: f64) {
    let seat = matrix(input, i::RIDER_MATRIX);
    if input[i::RIDER_RESET] != 0.0 {
        a.rider_lean = input[i::RIDER_LEAN];
    }
    if input[i::RIDER_ANIMAL] == 0.0 {
        let mut p = Vector3::default();
        let mut q = Quaternion::default();
        let mut s = Vector3::default();
        seat.decompose(&mut p, &mut q, &mut s);
        let root = a.rig.tree.node_mut(a.rig.root);
        root.position = p;
        root.copy_quaternion(&q);
        return;
    }
    // Bone handles are resolved at registration, including optional saddle
    // chest/pelvis controls; no name lookup belongs on this frame path.
    let Some(pelvis) = a.pelvis.map(|i| a.rig.bones[i]) else {
        return;
    };
    a.rig
        .tree
        .node_mut(a.rig.root)
        .copy_quaternion(&quaternion(input, i::RIDER_QUATERNION));
    let amount = min(
        1.0,
        vector(input, i::RIDER_VELOCITY).length() / max(1.0, input[i::RIDER_SPEED]),
    );
    a.rider_lean +=
        (-input[i::RIDER_YAW_RATE] * amount * 0.08 - a.rider_lean) * (1.0 - v8math::exp(-6.0 * dt));
    let breath = v8math::sin(input[i::RIDER_AGE] * 2.2 + input[i::RIDER_HOVER_PHASE]);
    if let Some(chest) = a.chest {
        let mut q = Quaternion::default();
        q.set_from_axis_angle(
            &Vector3::new(1.0, 0.0, 0.0),
            breath * 0.008 + amount * 0.018,
        );
        let bone = a.rig.tree.node_mut(a.rig.bones[chest]);
        bone.with_quaternion(|b| {
            b.multiply(&q);
        });
        q.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), a.rider_lean);
        bone.with_quaternion(|b| {
            b.multiply(&q);
        });
    }
    a.rig.tree.update_matrix_world(a.rig.root, true);
    let mut offset = Vector3::default();
    a.rig.tree.get_world_position(pelvis, &mut offset);
    offset.sub(&a.rig.tree.node(a.rig.root).position);
    let mut position = Vector3::default();
    position.set_from_matrix_position(&seat).sub(&offset);
    a.rig.tree.node_mut(a.rig.root).position = position;
}

/// Publish canonical local/Euler and world records for the host's views,
/// without a second scene-graph update. Local versions distinguish parent
/// movement from changed local transforms. Each phase owns one bounded
/// changed-node list. Matrix and scalar readers share the same published
/// storage until the next phase.
fn publish_nodes(
    tree: &mut TransformTree,
    start: usize,
    actor: usize,
    out: &mut [f64],
    changed: &mut [u32],
    mut count: usize,
) -> usize {
    for (index, node) in tree.nodes.iter_mut().enumerate() {
        let at = (start + index) * n::STRIDE;
        let row = &mut out[at..at + n::STRIDE];
        // Most moving bones differ in the first lane. Stop each comparison
        // there; the publication below still copies the complete matrix.
        let world_changed =
            (0..16).any(|k| row[n::WORLD + k].to_bits() != node.matrix_world.elements[k].to_bits());
        let mut local_changed =
            (0..16).any(|k| row[n::MATRIX + k].to_bits() != node.matrix.elements[k].to_bits());
        let q = node.quaternion;
        if row[n::QUATERNION].to_bits() != q.x.to_bits()
            || row[n::QUATERNION + 1].to_bits() != q.y.to_bits()
            || row[n::QUATERNION + 2].to_bits() != q.z.to_bits()
            || row[n::QUATERNION + 3].to_bits() != q.w.to_bits()
        {
            local_changed = true;
        }
        if !world_changed && !local_changed {
            continue;
        }
        if local_changed {
            row[n::LOCAL_VERSION] += 1.0;
        }
        row[n::EULER_SYNC] = if node.euler_sync { 1.0 } else { 0.0 };
        if node.euler_sync {
            let rotation = node.rotation();
            row[n::EULER..n::EULER + 3].copy_from_slice(&[rotation.x, rotation.y, rotation.z]);
        }
        row[n::ACTOR] = actor as f64;
        row[n::INDEX] = index as f64;
        if local_changed {
            write_vector(row, n::POSITION, node.position);
            row[n::QUATERNION..n::QUATERNION + 4].copy_from_slice(&[
                node.quaternion.x,
                node.quaternion.y,
                node.quaternion.z,
                node.quaternion.w,
            ]);
            write_vector(row, n::SCALE, node.scale);
            row[n::MATRIX..n::MATRIX + 16].copy_from_slice(&node.matrix.elements);
        }
        row[n::WORLD..n::WORLD + 16].copy_from_slice(&node.matrix_world.elements);
        changed[count] = (start + index) as u32;
        count += 1;
    }
    count
}
