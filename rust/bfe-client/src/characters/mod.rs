//! Live Battlefront II characters, sampled from the battle's published unit records.
pub(crate) mod assets;
mod body;
mod cloth;
mod drape;
mod near;
mod network;
#[cfg(test)]
mod probe;
mod preview;
mod rider;
mod saber;
use crate::{online::receiver::Receiver, render_bridge::Bridge};
use assets::Part;
use bfe_content_native::{
    animation2::{
        player::{Arena, Player},
        state::{Action, State},
        Class,
    },
    loader::Blend,
};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_render::instances as draw;
use bfe_sim::wire::thrown::{Replay, Sample as ThrownSaberPose, Throw};
use three::{Matrix4, Quaternion, Vector3};

const THROWN_HISTORY: usize = 16;

#[derive(Clone, Copy, Default)]
struct ThrownSample {
    tick: u32,
    active: bool,
    launched_tick: u32,
    position: [f64; 3],
    prev_position: [f64; 3],
    /// Where the blade is heading while it returns: the release point at this tick.
    target: [f64; 3],
    returning: bool,
    spin_angle: f64,
    prev_spin_angle: f64,
}

struct ThrownTracker {
    samples: [ThrownSample; THROWN_HISTORY],
    newest: usize,
    count: usize,
    launched_tick: Option<u32>,
    base_rot: Matrix4,
    spin_axis: Vector3,
}

impl Default for ThrownTracker {
    fn default() -> Self {
        Self {
            samples: [ThrownSample::default(); THROWN_HISTORY],
            newest: 0,
            count: 0,
            launched_tick: None,
            base_rot: Matrix4::default(),
            spin_axis: Vector3::new(0., 1., 0.),
        }
    }
}

impl ThrownTracker {
    fn reset(&mut self) {
        self.count = 0;
        self.launched_tick = None;
    }

    fn publish(
        &mut self,
        tick: u32,
        thrown: Option<&ThrownSaberPose>,
        hand: Option<&Matrix4>,
        first_edge_socket: Option<&Matrix4>,
    ) {
        if self.count != 0 && self.samples[self.newest].tick >= tick {
            return;
        }
        let sample = if let Some(t) = thrown {
            if self.launched_tick != Some(t.launched_tick) {
                let mut base_rot = hand.copied().unwrap_or_default();
                base_rot.elements[12] = 0.;
                base_rot.elements[13] = 0.;
                base_rot.elements[14] = 0.;
                let mut blade_m = base_rot;
                if let Some(socket) = first_edge_socket {
                    blade_m.multiply(socket);
                }
                let mut blade_dir = Vector3::new(
                    blade_m.elements[8],
                    blade_m.elements[9],
                    blade_m.elements[10],
                );
                if blade_dir.length_sq() > 1e-12 {
                    blade_dir.normalize();
                } else {
                    blade_dir = Vector3::new(0., 1., 0.);
                }
                let mut fwd = Vector3::new(t.direction[0], t.direction[1], t.direction[2]);
                if fwd.length_sq() > 1e-12 {
                    fwd.normalize();
                } else {
                    fwd = Vector3::new(0., 0., 1.);
                }
                let mut axis = Vector3::default();
                axis.cross_vectors(&blade_dir, &fwd);
                if axis.length_sq() > 1e-12 {
                    axis.normalize();
                } else {
                    let ref_up = if fwd.y.abs() < 0.98 {
                        Vector3::new(0., 1., 0.)
                    } else {
                        Vector3::new(1., 0., 0.)
                    };
                    axis.cross_vectors(&ref_up, &fwd);
                    axis.normalize();
                }
                self.base_rot = base_rot;
                self.spin_axis = axis;
                self.launched_tick = Some(t.launched_tick);
            }
            ThrownSample {
                tick,
                active: true,
                launched_tick: t.launched_tick,
                position: t.position,
                prev_position: t.prev_position,
                target: t.target,
                returning: t.returning,
                spin_angle: t.spin_angle,
                prev_spin_angle: t.prev_spin_angle,
            }
        } else {
            ThrownSample { tick, ..ThrownSample::default() }
        };
        self.newest = (self.newest + 1) % THROWN_HISTORY;
        self.count = (self.count + 1).min(THROWN_HISTORY);
        self.samples[self.newest] = sample;
    }

    fn compose(&self, pos: [f64; 3], angle: f64) -> Matrix4 {
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&self.spin_axis, angle);
        let mut m = Matrix4::default();
        m.make_rotation_from_quaternion(&q);
        m.multiply(&self.base_rot);
        m.set_position(pos[0], pos[1], pos[2]);
        m
    }

    fn matrix_at(&self, shown: f64) -> Option<Matrix4> {
        if self.count == 0 {
            return None;
        }
        let newest = self.samples[self.newest];
        if newest.active && shown > f64::from(newest.tick) + 2.0 {
            return None;
        }
        let mut later = self.newest;
        let mut earlier = later;
        for _ in 0..self.count.saturating_sub(1) {
            let prev = (later + THROWN_HISTORY - 1) % THROWN_HISTORY;
            if shown >= f64::from(self.samples[prev].tick) {
                earlier = prev;
                break;
            }
            later = prev;
            earlier = prev;
        }
        let a = self.samples[earlier];
        let b = self.samples[later];
        let span = f64::from(b.tick) - f64::from(a.tick);
        let frac = if span > 0. {
            ((shown - f64::from(a.tick)) / span).clamp(0., 1.)
        } else {
            1.
        };
        match (a.active, b.active) {
            (true, true) if a.launched_tick == b.launched_tick => {
                let pos = std::array::from_fn(|i| a.position[i] + (b.position[i] - a.position[i]) * frac);
                let angle = a.spin_angle + (b.spin_angle - a.spin_angle) * frac;
                Some(self.compose(pos, angle))
            }
            (true, true) => {
                let pos = std::array::from_fn(|i| {
                    b.prev_position[i] + (b.position[i] - b.prev_position[i]) * frac
                });
                let angle = b.prev_spin_angle + (b.spin_angle - b.prev_spin_angle) * frac;
                Some(self.compose(pos, angle))
            }
            (false, true) if shown >= f64::from(b.tick) => {
                Some(self.compose(b.position, b.spin_angle))
            }
            // The tick after the last one flown: the game's blade ends where its last update left
            // it and the hilt is in the hand from the catch on. A returning blade is drawn
            // on its straight way to the release point until then, as an update at a higher
            // rate would show it; a blade that never turned back stays where it stopped.
            (true, false) if frac < 1. => {
                let pos = if a.returning {
                    std::array::from_fn(|i| a.position[i] + (a.target[i] - a.position[i]) * frac)
                } else {
                    a.position
                };
                let angle = a.spin_angle + (a.spin_angle - a.prev_spin_angle) * frac;
                Some(self.compose(pos, angle))
            }
            _ => None,
        }
    }
}

type Row = [f64; draw::REC_STRIDE];
struct Weapon {
    registry: usize,
    mounts: Vec<Mount>,
}
struct Mount {
    joint: usize,
    parts: Vec<Row>,
    edges: Vec<saber::Edge>,
}
struct Model {
    class: Class,
    registry: usize,
    bone_names: Vec<String>,
    choke_default: Option<usize>,
    bone_l_hand: Option<usize>,
    bone_ribcage: Option<usize>,
    head: Option<usize>,
    aim_bones: [Option<usize>; 5],
    /// Ends with one skinned rest-shape row per cloth piece.
    body: Vec<Row>,
    /// The cloth pieces and their rows for the moving surface.
    cloth: Vec<cloth::Piece>,
    cloth_rows: Vec<Row>,
    weapons: Vec<Weapon>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct WeaponPose {
    pub position: [f64; 3],
    pub forward: [f64; 3],
    pub head_y: Option<f64>,
    pub yaw: f64,
    pub pitch: f64,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct AimBones {
    pub quats: [Option<[f64; 4]>; 5],
}
struct Slot {
    actor: Player,
    id: Option<(u32, u32)>,
    model: usize,
    last_arc_model: Option<usize>,
    seen: bool,
    clocks: network::Clocks,
    edges: [saber::History; 4],
    thrown: ThrownTracker,
    /// The soldier's published throw and the replay of its flight, if the blade is out.
    throw: Throw,
    replay: Option<Replay>,
    seat: Option<Matrix4>,
    bind: Option<(Matrix4, Matrix4)>,
    weapon_pose: Option<WeaponPose>,
    aim_bones: Option<AimBones>,
    /// The cloth restarts from its rest shape (a new life, a teleport, another body).
    cloth_cut: bool,
    /// The body's clip changes counted up to the earlier publication of the pose its cloth last moved to.
    cloth_changes: u32,
}
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Drawn {
    pub bodies: usize,
    pub body_parts: usize,
    pub held_parts: usize,
    pub blades: usize,
    pub trail_triangles: usize,
    pub ribbons: usize,
}
impl Slot {
    /// A published throw replaces what this slot knew: a flight is replayed from its launch,
    /// and a new launch restarts the replay.
    fn hear_throw(&mut self, throw: Throw) {
        match (throw.launch, self.replay.as_ref()) {
            (Some(l), Some(old)) if old.launch().tick == l.tick => {}
            (Some(l), _) => self.replay = Some(Replay::new(l)),
            (None, _) => self.replay = None,
        }
        self.throw = throw;
    }

    /// The blade at `tick`, while it is out: the battle's flight rule stepped from the launch.
    fn throw_sample(&mut self, tick: f64, position: [f64; 3], yaw: f64) -> Option<ThrownSaberPose> {
        let sample = self.replay.as_mut()?.at(tick, position, yaw);
        if self.replay.as_ref().is_some_and(Replay::is_finished) {
            self.replay = None;
        }
        sample
    }

    /// Seconds since the throw animation began, while it still plays.
    fn throw_windup(&self, tick: u32) -> Option<f64> {
        let press = self.throw.press?;
        let elapsed = (f64::from(tick) - press) / 60.;
        (0. ..2.).contains(&elapsed).then_some(elapsed)
    }

    fn visible(
        &self,
        shown: f64,
        hidden: Option<(u32, u32)>,
        hidden_crew: &[(u32, u32)],
        camera: &bfe_render::core::Camera,
    ) -> Option<State> {
        let id = self.id?;
        if hidden == Some(id) || hidden_crew.contains(&id) {
            return None;
        }
        let state = self.actor.state_at(shown);
        if state.seat.is_some() {
            state.seat_clip?;
            self.seat?;
        }
        let mut p = self.feet(&state);
        p[1] += 1.;
        camera.frustum.sphere(&p, 2.7).then_some(state)
    }
    /// Whether the body's motion from the pose its cloth last moved to, to the pose drawn at `shown`, crosses one
    /// of its clip changes (a clip begun at its first frame, a new tumble phase): the only frames a pose jump may
    /// restart the cloth. A drawn pose lies between two publications, so a change's jump can take two frames.
    fn cloth_crossing(&mut self, shown: f64) -> bool {
        let (left, right) = self.actor.changes_at(shown);
        let crossing = right != self.cloth_changes;
        self.cloth_changes = left;
        crossing
    }
    /// Where the soldier stands: its seat's place when seated.
    fn feet(&self, state: &State) -> [f64; 3] {
        match (state.seat, self.seat) {
            (Some(_), Some(frame)) => [frame.elements[12], frame.elements[13], frame.elements[14]],
            _ => state.position,
        }
    }
}
pub(crate) struct Characters {
    pub drawn: Drawn,
    pub fallback_deaths: usize,
    models: Vec<Model>,
    poses: Arena,
    ribbons: saber::Pool,
    cloth: cloth::Pool,
    leases: crate::online::index::Index,
    vacant: Vec<usize>,
    classes: Vec<Option<usize>>,
    slots: Vec<Slot>,
    shown: f64,
    camera_at: usize,
    hidden_own: Option<(u32, u32)>,
    hidden_crew: Vec<(u32, u32)>,
    chairs: Vec<rider::ChairSeat>,
    /// Soldiers working a remote turret chair: the soldier, its seat's world
    /// frame and the chair's seat.
    chair_riders: Vec<((u32, u32), Matrix4, usize)>,
    light: crate::online::unit_light::UnitLight,
    /// The spawn launcher's class body.
    preview: preview::Preview,
    /// Record the own soldier's weapon and aim bones for the developer trace only.
    telemetry: bool,
    /// Whether the game's soldier near rule applies to this frame's view (not in first person).
    near_rule: bool,
    near_blends: near::Blends,
    /// Whether a pose that jumped at a clip change restarts the cloth (`BFE_DEV_SWBF2_CLOTH_NO_RESTART=1`
    /// turns it off, for comparing films; read once).
    cloth_restart: bool,
}
fn register(bridge: &mut Bridge, textures: &[u32], part: Part, skin: bool) -> Result<Row, String> {
    let (mut row, key) = material_row(bridge, textures, part.material, skin)?;
    row[draw::REC_GEOMETRY] = part.geometry.register(&mut bridge.r, &key)? as f64;
    Ok(row)
}
fn material_row(
    bridge: &mut Bridge,
    textures: &[u32],
    material: bfe_content_native::loader::Material,
    skin: bool,
) -> Result<(Row, String), String> {
    let key = crate::static_scene::actor_key_extended(
        &material,
        bridge.shadow_cascades(),
        skin,
        material.bump.is_some(),
        false,
    )?;
    let program = bridge.add_owned_program(&key)?;
    let mut row = [0.; draw::REC_STRIDE];
    row[draw::REC_KIND] = if skin { 1. } else { 2. };
    row[draw::REC_PROGRAM] = program as f64;
    row[draw::REC_TEXTURES..draw::REC_TEXTURES + 4].fill(-1.);
    row[draw::REC_TEXTURES] = material.texture.map_or(-1., |id| textures[id] as f64);
    row[draw::REC_GROUPS] = 1.;
    row[draw::REC_BIND..draw::REC_BIND + 16].copy_from_slice(&Matrix4::default().elements);
    row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16]
        .copy_from_slice(&Matrix4::default().elements);
    row[draw::REC_SPHERE + 1] = 0.8;
    row[draw::REC_SPHERE + 3] = 2.5;
    row[draw::REC_LOOK] = 1.;
    row[draw::REC_LOOK + 3] = 1.;
    row[draw::REC_RECEIVE_SHADOW] = 1.;
    row[draw::REC_FLAGS] = (8
        | if material.double_sided { 2 } else { 0 }
        | if material.cutout { 4 } else { 0 }) as f64;
    if let Some(id) = material.bump {
        row[draw::REC_BUMP] = (textures[id] + 1) as f64;
    }
    if let Some(scroll) = material.record["scroll"].as_array() {
        for (dst, value) in row[draw::REC_EXTRA..draw::REC_EXTRA + 2]
            .iter_mut()
            .zip(scroll)
        {
            *dst = value.as_f64().unwrap_or(0.);
        }
    }
    if let Some(s) = material.record["specular"]
        .as_array()
        .filter(|v| v.len() == 4)
    {
        for (d, v) in row[draw::REC_SPECULAR..draw::REC_SPECULAR + 4]
            .iter_mut()
            .zip(s)
        {
            *d = v.as_f64().unwrap_or(0.);
        }
    }
    if material.blend != Blend::Opaque {
        let dest = if material.blend == Blend::Additive {
            glow::ONE
        } else {
            glow::ONE_MINUS_SRC_ALPHA
        };
        let blend = [
            1,
            glow::FUNC_ADD,
            glow::FUNC_ADD,
            glow::SRC_ALPHA,
            dest,
            glow::ONE,
            dest,
        ];
        let passes = if material.double_sided {
            vec![program, 1, program, 0]
        } else {
            vec![program, 0]
        };
        row[draw::REC_KIND] = if skin {
            draw::KIND_BLENDED
        } else {
            draw::KIND_BLENDED_RIGID
        };
        row[draw::REC_BLEND] = bridge.inst_add_blend(&blend, 1, glow::LEQUAL, &passes) as f64;
    }
    Ok((row, key))
}
pub(crate) fn motion_id(id: u32) -> f64 {
    8589934592. + f64::from(id)
}
fn emit(
    bridge: &mut Bridge,
    records: &mut usize,
    template: &Row,
    world: &Matrix4,
    bones: Option<(usize, usize, &Matrix4)>,
    id: (u32, u32),
    part: usize,
) {
    if *records >= draw::MAX_RECORDS {
        return;
    }
    let mut row = *template;
    row[draw::REC_WORLD..draw::REC_WORLD + 16].copy_from_slice(&world.elements);
    if let Some((offset, count, inverse)) = bones {
        row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
        row[draw::REC_BONE_AT] = offset as f64;
        row[draw::REC_BONES] = count as f64;
    }
    row[draw::REC_MOTION_ID] = motion_id(id.0);
    row[draw::REC_MOTION_GENERATION] = id.1 as f64;
    row[draw::REC_MOTION_PART] = part as f64;
    let at = draw::IN_RECORDS + *records * draw::REC_STRIDE;
    bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
    *records += 1;
}
impl Characters {
    pub fn new(
        data: assets::Prepared,
        bridge: &mut Bridge,
        textures: &[u32],
    ) -> Result<Self, String> {
        let mut models = Vec::new();
        let mut ribbons = saber::Pool::default();
        let mut largest = 0;
        let joints = data.models.iter().map(|m| m.class.animation.rig.joints.len()).max();
        let mut preview = preview::Preview::new(joints.unwrap_or(1))?;
        let mut cloth_key = None;
        for m in data.models {
            largest = largest.max(m.class.animation.rig.joints.len());
            preview.admit(bridge, textures, &m.geometry, &m.materials, &m.parts, &m.gear)?;
            let mut body = body::register(bridge, textures, &m.geometry, m.materials)?;
            let mut cloth_rows = Vec::with_capacity(m.cloth.len());
            for p in m.parts {
                let moving = drape::material(p.material.texture, p.material.cutout, p.material.blend != Blend::Opaque);
                body.push(register(bridge, textures, p, true)?);
                let (row, key) = material_row(bridge, textures, moving, false)?;
                cloth_rows.push(row);
                cloth_key.get_or_insert(key);
            }
            let mut weapons = Vec::new();
            for w in m.gear {
                let mut mounts = Vec::new();
                for mount in w.mounts {
                    let mut parts = Vec::new();
                    let mut edges = Vec::new();
                    for p in mount.parts {
                        parts.push(register(bridge, textures, p, false)?);
                    }
                    for edge in mount.edges {
                        edges.push(saber::Edge::new(edge, bridge, textures, &mut ribbons)?);
                    }
                    mounts.push(Mount {
                        joint: mount.joint,
                        parts,
                        edges,
                    });
                }
                weapons.push(Weapon {
                    registry: w.registry,
                    mounts,
                });
            }
            let default_choke_crc = bfe_content::zero_hash::crc("human_choking");
            let choke_default = m
                .class
                .animation
                .clips
                .iter()
                .position(|c| c.crc == default_choke_crc);
            let bone_l_hand = m.class.animation.rig.joint("bone_l_hand");
            let bone_ribcage = m.class.animation.rig.joint("bone_ribcage");
            let head = m.class.animation.rig.joint("bone_head");
            let aim_bones = [
                "bone_a_spine",
                "bone_b_spine",
                "bone_ribcage",
                "bone_neck",
                "bone_head",
            ]
            .map(|n| m.class.animation.rig.joint(n));
            models.push(Model {
                class: m.class,
                registry: m.registry,
                bone_names: m.bone_names,
                choke_default,
                bone_l_hand,
                bone_ribcage,
                head,
                aim_bones,
                body,
                cloth: m.cloth,
                cloth_rows,
                weapons,
            });
        }
        let cloth = match &cloth_key {
            Some(key) => cloth::Pool::new(models.iter().map(|m| m.cloth.as_slice()), bridge, key)?,
            None => cloth::Pool::default(),
        };
        Self::assemble(models, largest, ribbons, cloth, preview, data.light, data.chairs)
    }
    /// The characters with their animation and cloth pieces but nothing registered for drawing: what the
    /// headless probes pose (`probe.rs`).
    #[cfg(test)]
    pub(crate) fn headless(data: assets::Prepared) -> Result<Self, String> {
        let largest = data.models.iter().map(|m| m.class.animation.rig.joints.len()).max().unwrap_or(1);
        let mut models = Vec::new();
        for m in data.models {
            let rig = &m.class.animation.rig;
            let (bone_l_hand, bone_ribcage, head) = (rig.joint("bone_l_hand"), rig.joint("bone_ribcage"), rig.joint("bone_head"));
            let aim_bones = ["bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head"].map(|n| rig.joint(n));
            let default_choke_crc = bfe_content::zero_hash::crc("human_choking");
            let choke_default = m.class.animation.clips.iter().position(|c| c.crc == default_choke_crc);
            let weapons = m.gear.iter().map(|w| Weapon { registry: w.registry, mounts: Vec::new() }).collect();
            models.push(Model {
                class: m.class,
                registry: m.registry,
                bone_names: m.bone_names,
                choke_default,
                bone_l_hand,
                bone_ribcage,
                head,
                aim_bones,
                body: Vec::new(),
                cloth: m.cloth,
                cloth_rows: Vec::new(),
                weapons,
            });
        }
        let preview = preview::Preview::new(largest)?;
        Self::assemble(models, largest, saber::Pool::default(), cloth::Pool::default(), preview, data.light, data.chairs)
    }
    fn assemble(
        models: Vec<Model>,
        largest: usize,
        ribbons: saber::Pool,
        cloth: cloth::Pool,
        preview: preview::Preview,
        light: crate::online::unit_light::UnitLight,
        chairs: Vec<rider::ChairSeat>,
    ) -> Result<Self, String> {
        let capacity = bfe_presentation::layout::unit_limits::ACTORS;
        let poses = Arena::new(capacity, largest)?;
        let mut classes = vec![None; models.iter().map(|m| m.registry).max().unwrap_or(0) + 1];
        for (i, m) in models.iter().enumerate() {
            if m.registry != 0 {
                classes[m.registry] = Some(i);
            }
        }
        let slots = (0..capacity)
            .map(|_| Slot {
                actor: Player::default(),
                id: None,
                model: 0,
                last_arc_model: None,
                seen: false,
                clocks: Default::default(),
                edges: [saber::History::default(); 4],
                thrown: ThrownTracker::default(),
                throw: Throw::IDLE,
                replay: None,
                seat: None,
                bind: None,
                weapon_pose: None,
                aim_bones: None,
                cloth_cut: false,
                cloth_changes: 0,
            })
            .collect();
        Ok(Self {
            drawn: Default::default(),
            fallback_deaths: 0,
            models,
            poses,
            ribbons,
            cloth,
            classes,
            leases: crate::online::index::Index::new(capacity),
            vacant: (0..capacity).rev().collect(),
            slots,
            light,
            preview,
            shown: 0.,
            hidden_own: None,
            telemetry: std::env::var_os("BFE_BENCH").is_some(),
            near_rule: true,
            near_blends: near::Blends::default(),
            cloth_restart: std::env::var_os("BFE_DEV_SWBF2_CLOTH_NO_RESTART").as_deref() != Some(std::ffi::OsStr::new("1")),
            hidden_crew: Vec::with_capacity(64),
            chair_riders: Vec::with_capacity(chairs.len()),
            chairs,
            camera_at: bfe_render::shader::FRAME_OFFSETS[bfe_render::shader::FRAME_MEMBERS
                .iter()
                .position(|(n, _)| *n == "cameraPosition")
                .unwrap()],
        })
    }
    /// Poses the class with registry id `class` on the spawn launcher's stage and
    /// submits it to the interface pass; returns the records submitted.
    #[allow(clippy::too_many_arguments)]
    pub fn preview(
        &mut self,
        bridge: &mut Bridge,
        class: usize,
        parent: &[f64; 16],
        dt: f64,
        view: &Matrix4,
        projection: &Matrix4,
        theme: usize,
        build: f64,
    ) -> usize {
        let Some(model) = self.classes.get(class).copied().flatten() else {
            return 0;
        };
        let class = &self.models[model].class;
        self.preview.draw(bridge, model, class, parent, dt, view, projection, theme, build)
    }
    pub fn set_hidden_crew(&mut self, crew: &[(u32, u32)]) {
        self.hidden_crew.clear();
        self.hidden_crew.extend_from_slice(crew);
    }
    /// The soldiers working an exposed remote turret chair this frame.
    pub(crate) fn set_chair_riders(&mut self, riders: &[crate::local::space::ChairRider]) {
        self.chair_riders.clear();
        for r in riders {
            let Some(seat) = self.chairs.iter().position(|c| c.instance == r.instance) else { continue };
            if self.chair_riders.len() == self.chair_riders.capacity() {
                break;
            }
            let mut frame = Matrix4::default();
            frame.elements = r.matrix;
            frame.multiply(&self.chairs[seat].local);
            self.chair_riders.push((r.unit, frame, seat));
        }
    }
    pub fn visible_budget(&self, bridge: &Bridge) -> usize {
        let camera = bridge.r.core.camera();
        let mut ribbons = 0;
        let ordinary = self
            .slots
            .iter()
            .filter_map(|s| {
                s.id?;
                let model = &self.models[s.model];
                let body_state = s.visible(self.shown, self.hidden_own, &self.hidden_crew, &camera);
                let thrown_visible = s.thrown.matrix_at(self.shown).is_some_and(|m| {
                    camera
                        .frustum
                        .sphere(&[m.elements[12], m.elements[13], m.elements[14]], 2.7)
                });
                if body_state.is_none() && !thrown_visible {
                    return None;
                }
                let state = body_state.unwrap_or_else(|| s.actor.state_at(self.shown));
                let mut records = if body_state.is_some() {
                    model.body.len()
                } else {
                    0
                };
                if let Some(weapon) = model
                    .weapons
                    .get(state.weapon)
                    .filter(|_| state.action != Action::Death && state.seat.is_none())
                {
                    let mut index = 0;
                    for (mount_idx, mount) in weapon.mounts.iter().enumerate() {
                        let draw_mount = if mount_idx == 0 {
                            body_state.is_some() || thrown_visible
                        } else {
                            body_state.is_some()
                        };
                        if draw_mount {
                            records += mount.parts.len();
                            for edge in &mount.edges {
                                if let Some(h) = s.edges.get(index) {
                                    let (blade, trail) = h.budget(edge, self.shown);
                                    records += blade;
                                    ribbons += trail;
                                }
                                index += 1;
                            }
                        } else {
                            index += mount.edges.len();
                        }
                    }
                }
                Some(records)
            })
            .sum::<usize>();
        (ordinary + ribbons.min(saber::MAX_RIBBONS)).min(draw::MAX_RECORDS)
    }
    pub fn max_bones(&self) -> usize {
        self.models
            .iter()
            .map(|m| m.class.animation.rig.mesh_joints)
            .max()
            .unwrap_or(1)
    }
    pub fn clear(&mut self) {
        self.leases.clear();
        self.vacant.clear();
        self.vacant.extend((0..self.slots.len()).rev());
        for slot in &mut self.slots {
            slot.id = None;
        }
        self.cloth.clear();
    }
    pub fn registry(&self, model: usize) -> usize {
        self.models[model].registry
    }
    pub fn named(&self, name: &str) -> Option<usize> {
        self.models
            .iter()
            .position(|m| m.class.name.eq_ignore_ascii_case(name))
    }
    fn lease(&mut self, id: (u32, u32), model: usize) -> Option<usize> {
        let i = if let Some(&i) = self.leases.get(&id) {
            i
        } else {
            let i = self.vacant.pop()?;
            if !self.leases.insert(id, i) {
                self.vacant.push(i);
                return None;
            }
            i
        };
        let s = &mut self.slots[i];
        if s.id != Some(id) || s.model != model {
            s.actor.reset();
            s.id = Some(id);
            s.model = model;
            s.clocks = Default::default();
            s.edges = [saber::History::default(); 4];
            s.thrown.reset();
            s.throw = Throw::IDLE;
            s.replay = None;
            s.seat = None;
            s.bind = None;
            s.weapon_pose = None;
            s.aim_bones = None;
            s.cloth_cut = true;
        }
        s.seen = true;
        Some(i)
    }
    pub fn publish(&mut self, id: (u32, u32), model: usize, state: State) {
        if let Some(i) = self.lease(id, model) {
            self.sample(i, state, None);
        }
    }
    fn sample(&mut self, i: usize, state: State, thrown: Option<&ThrownSaberPose>) {
        let slot = &mut self.slots[i];
        let model = &self.models[slot.model];
        let mut pose = self.poses.slot(i, model.class.animation.rig.joints.len());
        let before = slot.actor.fallback_deaths;
        if slot.actor.publish(&model.class, state, &mut pose) {
            slot.edges = [saber::History::default(); 4];
            slot.thrown.reset();
            slot.replay = None;
            slot.bind = None;
            slot.seat = None;
            slot.cloth_cut = true;
        }
        self.fallback_deaths += slot.actor.fallback_deaths - before;
        let thrown = thrown.filter(|_| state.action != Action::Death && state.seat.is_none());
        let weapon = model.weapons.get(state.weapon);
        let edge_weapon = weapon
            .filter(|w| w.mounts.iter().any(|m| !m.edges.is_empty()))
            .or_else(|| {
                thrown.and_then(|_| {
                    model
                        .weapons
                        .iter()
                        .find(|w| w.mounts.iter().any(|m| !m.edges.is_empty()))
                })
            });
        let has_edges = edge_weapon.is_some();
        if has_edges || thrown.is_some() {
            slot.actor
                .place_at(&model.class, state.tick as f64, false, &mut pose);
        }
        let primary_mount = edge_weapon
            .or(weapon)
            .and_then(|w| w.mounts.first());
        let mount0_hand = primary_mount.map(|m| &pose.world[m.joint]);
        let mount0_socket = primary_mount.and_then(|m| m.edges.first()).map(saber::Edge::socket);
        slot.thrown
            .publish(state.tick, thrown, mount0_hand, mount0_socket);
        if let Some(w) = edge_weapon {
            let thrown_m = slot.thrown.matrix_at(f64::from(state.tick));
            let mut index = 0;
            for (mount_idx, m) in w.mounts.iter().enumerate() {
                let flying = mount_idx == 0 && thrown_m.is_some();
                let xform = if flying {
                    thrown_m.as_ref().unwrap()
                } else {
                    &pose.world[m.joint]
                };
                let attack = slot.actor.trail || flying;
                for edge in &m.edges {
                    if let Some(h) = slot.edges.get_mut(index) {
                        let on = flying || state.blades_off & (1 << index) == 0;
                        h.publish(state.tick, xform, edge, on, attack);
                    }
                    index += 1;
                }
            }
        }
    }
    pub fn network(
        &mut self,
        receiver: &Receiver,
        replay: &crate::online::replay::Replay,
        resources: &crate::online::replicas::Replicas,
        eyes: bool,
    ) {
        self.near_rule = !resources.cameras.eye_view;
        self.present_records(receiver, Some(replay), Some(resources), eyes);
    }
    pub fn present_records(
        &mut self,
        receiver: &Receiver,
        replay: Option<&crate::online::replay::Replay>,
        resources: Option<&crate::online::replicas::Replicas>,
        eyes: bool,
    ) {
        let snapshot_tick = receiver.network.borrow().header
            [bfe_presentation::network_layout::net_header::ACK_TICK]
            as u32;
        self.hidden_own = eyes.then_some((receiver.own.id as u32, receiver.own.generation as u32));
        for s in &mut self.slots {
            s.seen = false;
        }
        for row in receiver
            .active()
            .filter(|row| row[r::KIND] as u32 == k::SOLDIER)
        {
            if let Some(&i) = self
                .leases
                .get(&(row[r::ID] as u32, row[r::GENERATION] as u32))
            {
                self.slots[i].seen = self
                    .classes
                    .get(row[r::CLASS_ID] as usize)
                    .is_some_and(Option::is_some);
            }
        }
        for (i, s) in self.slots.iter_mut().enumerate().filter(|(_, s)| !s.seen) {
            if let Some(id) = s.id.take() {
                self.leases.remove(&id);
                self.vacant.push(i);
            }
        }
        for row in receiver
            .active()
            .filter(|row| row[r::KIND] as u32 == k::SOLDIER)
        {
            let Some(model) = self
                .classes
                .get(row[r::CLASS_ID] as usize)
                .copied()
                .flatten()
            else {
                continue;
            };
            self.lease((row[r::ID] as u32, row[r::GENERATION] as u32), model);
        }
        for field in receiver.publications.iter() {
            if let Some((life, display)) = network::hero_display(field) {
                if let Some(&i) = self.leases.get(&life) {
                    let clocks = &mut self.slots[i].clocks;
                    clocks.melee = display.melee;
                    clocks.cast = display.cast;
                    clocks.choke = display.choked;
                }
            }
            if let Some((id, throw)) = network::throw(field) {
                if let Some(&i) = self.leases.get(&id) {
                    self.slots[i].hear_throw(throw);
                }
            }
            if let Some((victim, bearing)) = network::impact(field, receiver) {
                if let Some(&i) = self.leases.get(&victim) {
                    self.slots[i]
                        .clocks
                        .incoming(field.row[r::TICK] as u32, bearing);
                }
            }
            if let Some(e) = network::action_event(field) {
                if let Some(&i) = self.leases.get(&e.owner) {
                    let model = &self.models[self.slots[i].model];
                    let action = if e.action == Action::Fire && e.weapon_id != 0 {
                        if let Some(w_idx) = model
                            .weapons
                            .iter()
                            .position(|w| w.registry == e.weapon_id)
                        {
                            let tables = &model.class.tables;
                            if tables.offhand_weapon(w_idx) {
                                // InstantPlayFireAnim offhand guns already began at trigger press
                                // through PowerFrame; do not restart them on round release.
                                if tables.instant_anim(w_idx) {
                                    None
                                } else if tables.fire_anim(w_idx) == 1 {
                                    Some(Action::FireSecondary2)
                                } else {
                                    Some(Action::FireSecondary)
                                }
                            } else {
                                Some(Action::Fire)
                            }
                        } else if e.channel == 1 {
                            Some(Action::FireSecondary)
                        } else {
                            Some(Action::Fire)
                        }
                    } else {
                        Some(e.action)
                    };
                    if let Some(action) = action {
                        self.slots[i].clocks.event(action, e.tick);
                    }
                }
            }
        }
        for row in receiver
            .active()
            .filter(|row| row[r::KIND] as u32 == k::SOLDIER)
        {
            let id = (row[r::ID] as u32, row[r::GENERATION] as u32);
            let Some(&i) = self.leases.get(&id) else {
                continue;
            };
            let slot = &mut self.slots[i];
            let own =
                receiver.own.id == row[r::ID] && receiver.own.generation == row[r::GENERATION];
            let time = if own { receiver.own.tick } else { row[r::TICK] };
            let record_tick = time as u32;
            let Some(tick) = slot.clocks.sample_tick(row, record_tick, snapshot_tick) else {
                continue;
            };
            let model = &self.models[slot.model];
            let weapon = model
                .weapons
                .iter()
                .position(|w| w.registry == row[r::WEAPON] as usize)
                .unwrap_or(0);
            let local = receiver.infantry.filter(|s| s.id == id.0 && s.generation == row[r::GENERATION]);
            if let Some(local) = local {
                slot.clocks.note_jump_count(local.jump_count, tick, row[r::STATE] as u8);
            }
            let mut state = slot.clocks.state(row, tick, weapon, &model.class);
            // The player's own soldier never relaxes; the others do 2.5 s after their last shot.
            state.alert |= own;
            if local.is_none() {
                // A 2005 soldier's sprint and roll from his display field, the roll
                // from the tick it began, over his class's roll window.
                if let Some(display) = receiver.swbf2_display(id.0, id.1) {
                    let window = replay
                        .and_then(|replay| replay.swbf2_classes.get(row[r::CLASS_ID] as usize)?.as_ref())
                        .map_or(0., |tuning| tuning.roll_seconds());
                    network::movement(&mut state, display.sprint, display.roll.map(|(direction, start, _)| {
                        (direction, f64::from(tick.saturating_sub(start) + 1) / 60., window)
                    }));
                }
            }
            if let Some(local) = local {
                use bfe_sim::movement::soldier::swbf2::RollDirection;
                use bfe_content_native::animation2::state::Direction;
                state.sprint = local.sprint;
                state.jump_count = local.jump_count;
                state.roll = local.roll.map(|roll| match roll.direction {
                    RollDirection::Forward => Direction::Forward, RollDirection::Left => Direction::Left, RollDirection::Right => Direction::Right,
                });
                if let Some(roll) = local.roll {
                    state.phase_time = Some(roll.elapsed);
                    state.roll_seconds = Some(local.roll_seconds);
                }
                state.combo = local.combo.map(usize::from);
                // A combo row plays on its own clip clock; `elapsed` stays the clock of the
                // presenter's action (a hit flinch or a death) and of nothing else.
                if state.combo.is_some() { state.anim_time = local.anim_time; state.blades_off = local.blades_off; state.swing_turn = local.swing_turn; }
                if let Some((clip,time)) = local.saber.and_then(|s| s.reaction) {
                    state.contact_clip = Some(clip);
                    state.anim_time = time;
                }
            } else if let Some(pose) = slot.clocks.melee {
                network::published_combo(&mut state, &pose);
            }
            if let Some(contacts) = receiver.contacts.as_deref() {
                use bfe_sim::units::infantry::contact::Outcome;
                use bfe_sim::combat::combo::DeflectDirection as D;
                use bfe_content_native::animation2::state::{Action,Direction};
                if let Some(e) = contacts.since(0).filter(|e| e.target.slot == id.0 &&
                    e.target.generation == id.1 && e.tick <= tick && e.outcome == Outcome::Hit).last() {
                    if matches!(state.action,Action::Hit | Action::Death) {
                        state.impact = Some(match e.direction {
                            D::Left | D::FrontLeft => Direction::Left, D::Right | D::FrontRight => Direction::Right,
                            D::Back => Direction::Backward, D::Forward => Direction::Forward,
                        });
                    }
                }
            }
            let mut choked_active = false;
            // A push's tumble comes from the local battle's frame; the choke that holds a
            // soldier and the pose of a power it casts come from its published combat
            // display, the same for every soldier in Instant Action and online.
            let tumbling = receiver
                .powers
                .as_deref()
                .and_then(|p| p.unit(id.0, row[r::GENERATION]))
                .filter(|pose| pose.tumble_phase != 0 && state.seat.is_none());
            if let Some(pose) = tumbling {
                use bfe_content_native::animation2::state::{TumblePhase, TumblePose};
                let phase = match pose.tumble_phase {
                    1 => TumblePhase::Thrown,
                    2 => TumblePhase::Tumble,
                    3 => TumblePhase::Landed,
                    4 => TumblePhase::GetUp,
                    5 => TumblePhase::Recover,
                    6 => TumblePhase::RestDead,
                    _ => TumblePhase::None,
                };
                if phase != TumblePhase::None {
                    state.tumble = Some(TumblePose {
                        phase,
                        front: pose.tumble_flags & 1 != 0,
                        bounce: pose.tumble_flags & 2 != 0,
                        fly_dir: (pose.tumble_flags >> 2) & 3,
                        elapsed: f64::from(pose.tumble_elapsed),
                        angle: f64::from(pose.tumble_angle),
                        heading: f64::from(pose.tumble_heading),
                    });
                    state.combo = None;
                    state.contact_clip = None;
                    if state.action != Action::Death {
                        state.action = Action::None;
                    }
                }
            } else if let Some(anim) = slot.clocks.choke.filter(|_| state.action != Action::Death && state.seat.is_none()) {
                let default_crc = bfe_content::zero_hash::crc("human_choking");
                let crc = if anim != 0 { anim } else { default_crc };
                if let Some((_, elapsed)) = slot.clocks.choke_elapsed(Some(crc), tick) {
                    let clip = if crc == default_crc {
                        model.choke_default
                    } else {
                        model
                            .class
                            .animation
                            .clips
                            .iter()
                            .position(|c| c.crc == crc)
                            .or(model.choke_default)
                    };
                    if let Some(id) = clip {
                        state.choke_clip = Some(id);
                        state.choke_time = elapsed;
                        state.action = Action::None;
                        state.combo = None;
                        choked_active = true;
                    }
                }
            } else if let Some(cast) = slot.clocks.cast.filter(|_| state.action != Action::Death && state.seat.is_none()) {
                let action = network::cast_action(&cast);
                let elapsed = cast.elapsed(f64::from(state.tick));
                let duration = model.class.tables.action_duration(
                    &model.class.animation,
                    state.weapon,
                    state.crouch,
                    action,
                );
                if duration > 0. && (cast.holding || elapsed < duration) {
                    state.action = action;
                    state.elapsed = elapsed;
                }
            }
            if !choked_active {
                slot.clocks.choke_elapsed(None, tick);
            }
            if let Some((clip, time)) = state
                .seat
                .and_then(|id| rider::seat(resources?, id))
                .and_then(|seat| seat.pose(&model.class))
            {
                state.seat_clip = Some(clip);
                state.seat_time = time;
            } else if state.seat.is_none() && state.action != Action::Death {
                if let Some(clip) = self.chair_riders.iter().find(|r| r.0 == id)
                    .and_then(|r| rider::chair_clip(&model.class, &self.chairs[r.2].pose))
                {
                    let duration = model.class.animation.clips[clip].duration();
                    state.seat_clip = Some(clip);
                    state.seat_time = if duration > 0. { (f64::from(tick) / 60.).rem_euclid(duration) } else { 0. };
                }
            }
            let thrown = self.slots[i].throw_sample(f64::from(state.tick), state.position, state.yaw);
            if let Some(elapsed) = self.slots[i].throw_windup(state.tick) {
                let model = &self.models[self.slots[i].model];
                let duration = model.class.tables.action_duration(
                    &model.class.animation,
                    state.weapon,
                    state.crouch,
                    Action::Throw,
                );
                if state.action != Action::Death
                    && state.seat.is_none()
                    && state.tumble.is_none()
                    && state.choke_clip.is_none()
                    && elapsed < duration
                {
                    state.action = Action::Throw;
                    state.elapsed = elapsed;
                }
            }
            self.sample(i, state, thrown.as_ref());
        }
        self.shown = receiver.clock.shown;
        let chairs = &self.chair_riders;
        for slot in self.slots.iter_mut().filter(|s| s.id.is_some()) {
            let displayed = slot.actor.state_at(self.shown);
            slot.seat = displayed
                .seat
                .and_then(|id| rider::seat(resources?, id))
                .map(|s| s.source.frame)
                .or_else(|| {
                    displayed.seat.is_none().then_some(())?;
                    displayed.seat_clip?;
                    chairs.iter().find(|r| Some(r.0) == slot.id).map(|r| r.1)
                });
        }
    }
    pub fn at(&mut self, tick: f64) {
        self.shown = tick;
    }
    fn place_slot(&mut self, i: usize) {
        let s = &mut self.slots[i];
        let m = &self.models[s.model];
        let mut pose = self.poses.slot(i, m.class.animation.rig.joints.len());
        if let Some(frame) = s.seat {
            s.actor.place_on(&m.class, self.shown, frame, &mut pose);
        } else {
            s.actor.place_at(&m.class, self.shown, false, &mut pose);
        }
    }
    pub(crate) fn is_alive(&self, id: (u32, u32)) -> bool {
        let Some(&i) = self.leases.get(&id) else {
            return false;
        };
        let s = &self.slots[i];
        s.id == Some(id) && s.actor.state_at(self.shown).action != Action::Death
    }
    pub(crate) fn feet_world(&self, id: (u32, u32)) -> Option<[f64; 3]> {
        let &i = self.leases.get(&id)?;
        let s = &self.slots[i];
        if s.id != Some(id) {
            return None;
        }
        let state = s.actor.state_at(self.shown);
        if let Some(frame) = s.seat.filter(|_| state.seat.is_some()) {
            let e = &frame.elements;
            Some([e[12], e[13], e[14]])
        } else {
            Some(state.position)
        }
    }
    pub(crate) fn aim_angles(&self, id: (u32, u32)) -> Option<(f64, f64)> {
        let &i = self.leases.get(&id)?;
        let s = &self.slots[i];
        if s.id != Some(id) {
            return None;
        }
        let state = s.actor.state_at(self.shown);
        Some((state.yaw, state.pitch))
    }
    pub(crate) fn bone_world(&mut self, id: (u32, u32), bone_crc: u32) -> Option<[f64; 3]> {
        let &i = self.leases.get(&id)?;
        if self.slots[i].id != Some(id) {
            return None;
        }
        let m_idx = self.slots[i].model;
        let joint = self.models[m_idx]
            .class
            .animation
            .rig
            .joints
            .iter()
            .position(|j| j.crc == bone_crc)
            .or(self.models[m_idx].bone_ribcage)?;
        self.place_slot(i);
        let joints = self.models[m_idx].class.animation.rig.joints.len();
        let pose = self.poses.slot(i, joints);
        let e = &pose.world[joint].elements;
        Some([e[12], e[13], e[14]])
    }
    pub(crate) fn hand_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
        let &i = self.leases.get(&id)?;
        if self.slots[i].id != Some(id) {
            return None;
        }
        let m_idx = self.slots[i].model;
        let joint = self.models[m_idx]
            .bone_l_hand
            .or(self.models[m_idx].bone_ribcage);
        if let Some(joint) = joint {
            self.place_slot(i);
            let joints = self.models[m_idx].class.animation.rig.joints.len();
            let pose = self.poses.slot(i, joints);
            let e = &pose.world[joint].elements;
            Some([e[12], e[13], e[14]])
        } else {
            let mut p = self.feet_world(id)?;
            p[1] += 1.35;
            Some(p)
        }
    }
    pub(crate) fn chest_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
        let &i = self.leases.get(&id)?;
        if self.slots[i].id != Some(id) {
            return None;
        }
        let m_idx = self.slots[i].model;
        if let Some(joint) = self.models[m_idx].bone_ribcage {
            self.place_slot(i);
            let joints = self.models[m_idx].class.animation.rig.joints.len();
            let pose = self.poses.slot(i, joints);
            let e = &pose.world[joint].elements;
            Some([e[12], e[13], e[14]])
        } else {
            let mut p = self.feet_world(id)?;
            p[1] += 1.0;
            Some(p)
        }
    }
    pub(crate) fn list_for_arcs(
        &mut self,
        own: (u32, u32),
        cone_of: impl Fn(usize) -> Option<f64>,
        arcs: &mut crate::online::lightning::Arcs,
    ) {
        for i in 0..self.slots.len() {
            let Some(id) = self.slots[i].id else {
                continue;
            };
            let m_idx = self.slots[i].model;
            if self.slots[i].last_arc_model != Some(m_idx) {
                arcs.clear_actor(i);
                self.slots[i].last_arc_model = Some(m_idx);
            }
            let state = self.slots[i].actor.state_at(self.shown);
            let dead = state.action == Action::Death;
            let mounted = state.seat.is_some();
            let root = if let Some(frame) = self.slots[i].seat.filter(|_| mounted) {
                let e = &frame.elements;
                [e[12], e[13], e[14]]
            } else {
                state.position
            };
            let cone = cone_of(self.models[m_idx].registry);
            let hand = (!dead && !mounted && cone.is_some()).then(|| {
                let joint = self.models[m_idx]
                    .bone_l_hand
                    .or(self.models[m_idx].bone_ribcage);
                if let Some(joint) = joint {
                    self.place_slot(i);
                    let joints = self.models[m_idx].class.animation.rig.joints.len();
                    let pose = self.poses.slot(i, joints);
                    let e = &pose.world[joint].elements;
                    [e[12], e[13], e[14]]
                } else {
                    [root[0], root[1] + 1.35, root[2]]
                }
            });
            let facts = crate::online::lightning::Unit {
                generation: id.1 as f64,
                dead,
                mounted,
                walker: false,
                first_person: false,
                own: id == own,
                shown: self.hidden_own != Some(id),
                root,
                origin: hand,
                muzzle: hand,
                current_muzzle: hand,
                cone,
                color: None,
                charging: false,
                pressed: 0.,
                max_pressed: 1.,
                shield: None,
            };
            arcs.list(i, &facts);
        }
    }
    pub(crate) fn pose_for_arcs(&mut self, arcs: &mut crate::online::lightning::Arcs) {
        for i in 0..self.slots.len() {
            if self.slots[i].id.is_none() || !arcs.wants_pose(i) {
                continue;
            }
            self.place_slot(i);
            let m = &self.models[self.slots[i].model];
            let joints = m.class.animation.rig.joints.len();
            let pose = self.poses.slot(i, joints);
            arcs.pose(i, &m.bone_names, |bone| {
                pose.world
                    .get(bone)
                    .map_or([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.], |w| {
                        w.elements
                    })
            });
        }
    }
    pub fn own_clip(
        &self,
        own: (u32, u32),
    ) -> Option<bfe_content_native::animation2::player::ClipFrame<'_>> {
        let &i = self.leases.get(&own)?;
        let slot = &self.slots[i];
        let model = &self.models[slot.model];
        slot.actor.clip_at(&model.class, self.shown)
    }
    pub fn own_weapon_pose(&self, own: (u32, u32)) -> Option<WeaponPose> {
        let &i = self.leases.get(&own)?;
        self.slots[i].weapon_pose
    }
    pub fn own_aim_bones(&self, own: (u32, u32)) -> Option<AimBones> {
        let &i = self.leases.get(&own)?;
        self.slots[i].aim_bones
    }
    pub fn present_jets(
        &mut self,
        jets: &mut crate::online::jetpacks::Jetpacks,
        combat: &mut crate::online::combat::Combat,
        dt: f64,
    ) {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let Some(id) = slot.id else { continue };
            let model = &self.models[slot.model];
            let Some(chest) = model.bone_ribcage else { continue };
            let state = slot.actor.state_at(self.shown);
            let active = state.air == bfe_content_native::animation2::state::Air::Jet
                && state.action != Action::Death
                && state.seat.is_none();
            if !active && !jets.tracks(id) {
                continue;
            }
            let mut pose = self.poses.slot(i, model.class.animation.rig.joints.len());
            if let Some(frame) = slot.seat {
                slot.actor.place_on(&model.class, self.shown, frame, &mut pose);
            } else {
                slot.actor.place_at(&model.class, self.shown, false, &mut pose);
            }
            let (mut position, mut turn, mut scale) = (
                three::Vector3::default(),
                three::Quaternion::default(),
                three::Vector3::default(),
            );
            pose.world[chest].decompose(&mut position, &mut turn, &mut scale);
            let position = [position.x, position.y, position.z];
            let turn = [turn.x, turn.y, turn.z, turn.w];
            let class = model.registry;
            jets.update(id, class, active, position, turn, state.velocity, dt);
            if active && !jets.enhanced {
                if let Some(group) = jets.classic(class) {
                    combat.flame(id, group, position, state.velocity, turn);
                }
            }
        }
    }
    pub fn append(
        &mut self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        let mut drawn = Drawn::default();
        self.cloth.begin();
        let camera = bridge.r.core.camera();
        let at = self.camera_at;
        let eye = std::array::from_fn(|i| bridge.r.core.input[at + i]);
        let (forward, near) = near::view(&bridge.r.core.input[..16], &camera.view);
        for (i, s) in self.slots.iter_mut().enumerate() {
            let Some(id) = s.id else { continue };
            let m = &self.models[s.model];
            let body_visible = s.visible(self.shown, self.hidden_own, &self.hidden_crew, &camera);
            let fade = match &body_visible {
                Some(state) if self.near_rule => near::fade(near::torso(s.feet(state), state.crouch), eye, forward, near),
                _ => 0.,
            };
            let body_visible = body_visible.filter(|_| fade < 1.);
            let raw_thrown = s.thrown.matrix_at(self.shown);
            let hide_primary_in_hand = raw_thrown.is_some();
            let draw_thrown = raw_thrown.filter(|tm| {
                camera
                    .frustum
                    .sphere(&[tm.elements[12], tm.elements[13], tm.elements[14]], 2.7)
            });
            if body_visible.is_none() && draw_thrown.is_none() {
                continue;
            }
            let state = body_visible.unwrap_or_else(|| s.actor.state_at(self.shown));
            if body_visible.is_some() {
                let count = m.class.animation.rig.mesh_joints;
                if bones + count <= draw::MAX_BONES && records + m.body.len() <= draw::MAX_RECORDS {
                    let mut pose = self.poses.slot(i, m.class.animation.rig.joints.len());
                    let world = if let Some(frame) = s.seat {
                        s.actor.place_on(&m.class, self.shown, frame, &mut pose)
                    } else {
                        s.actor.place_at(&m.class, self.shown, false, &mut pose)
                    };
                    if self.telemetry {
                        s.aim_bones = Some(AimBones {
                            quats: m.aim_bones.map(|slot| {
                                slot.map(|j| {
                                    let (mut p, mut q, mut sc) = (
                                        three::Vector3::default(),
                                        three::Quaternion::default(),
                                        three::Vector3::default(),
                                    );
                                    pose.model_space[j].decompose(&mut p, &mut q, &mut sc);
                                    [q.x, q.y, q.z, q.w]
                                })
                            }),
                        });
                    }
                    let inverse = match s.bind {
                        Some((root, inv)) if root == world => inv,
                        _ => {
                            let mut inv = world;
                            inv.invert();
                            s.bind = Some((world, inv));
                            inv
                        }
                    };
                    bridge.r.core.bones[bones * 16..(bones + count) * 16]
                        .copy_from_slice(&pose.palette[..count * 16]);
                    let start = bones;
                    let first = records;
                    drawn.bodies += 1;
                    drawn.body_parts += m.body.len();
                    bones += count;
                    // Moving cloth replaces the skinned rest shape of the same pieces.
                    let moved = if m.cloth.is_empty() {
                        None
                    } else {
                        if self.cloth.tracing_frames() {
                            // The body the cloth hangs on: its tumble phase and clock, and the
                            // root bone's place and turn (the thrown somersault turns it).
                            let root = m.class.animation.rig.root.map(|r| {
                                let (mut p, mut q, mut sc) = (Vector3::default(), Quaternion::default(), Vector3::default());
                                pose.world[r].decompose(&mut p, &mut q, &mut sc);
                                [p.x, p.y, p.z, q.x, q.y, q.z, q.w]
                            });
                            eprintln!(
                                "cloth: body shown={:.3} slot={i} tumble={:?} action={:?} root={root:.3?}",
                                self.shown,
                                state.tumble.map(|t| (t.phase, t.front, (t.elapsed * 1000.).round() / 1000.)),
                                state.action,
                            );
                        }
                        let cut = std::mem::take(&mut s.cloth_cut);
                        let crossing = s.cloth_crossing(self.shown) && self.cloth_restart;
                        self.cloth.update(i, id, s.model, &m.cloth, &pose.world[..], self.shown, cut, crossing, bridge)
                    };
                    let skinned = m.body.len() - moved.map_or(0, |_| m.cloth.len());
                    for (part_i, r) in m.body[..skinned].iter().enumerate() {
                        emit(
                            bridge,
                            &mut records,
                            r,
                            &world,
                            Some((start, count, &inverse)),
                            id,
                            part_i,
                        );
                    }
                    if let Some(k) = moved {
                        for (c, template) in m.cloth_rows.iter().enumerate() {
                            let (geometry, frame, reach) = self.cloth.piece(k, c);
                            let mut row = *template;
                            row[draw::REC_GEOMETRY] = f64::from(geometry);
                            row[draw::REC_SPHERE..draw::REC_SPHERE + 3].fill(0.);
                            row[draw::REC_SPHERE + 3] = f64::from(reach);
                            emit(bridge, &mut records, &row, frame, None, id, skinned + c);
                        }
                    }
                    if fade > 0. {
                        self.near_blends.apply(bridge, first, records, fade);
                    }
                    s.weapon_pose = None;
                    if let Some(w) = m
                        .weapons
                        .get(state.weapon)
                        .filter(|_| state.action != Action::Death && state.seat.is_none())
                    {
                        let mut part = m.body.len();
                        let mut index = 0;
                        for (mount_idx, mount) in w.mounts.iter().enumerate() {
                            if mount_idx == 0 && hide_primary_in_hand {
                                part += mount.parts.len() + mount.edges.len() * saber::EDGE_PARTS;
                                index += mount.edges.len();
                                continue;
                            }
                            let hand = &pose.world[mount.joint];
                            if self.telemetry && s.weapon_pose.is_none() {
                                let e = &hand.elements;
                                let len = (e[8] * e[8] + e[9] * e[9] + e[10] * e[10]).sqrt();
                                if len > 1e-9 {
                                    s.weapon_pose = Some(WeaponPose {
                                        position: [e[12], e[13], e[14]],
                                        forward: [e[8] / len, e[9] / len, e[10] / len],
                                        head_y: m.head.map(|h| pose.world[h].elements[13]),
                                        yaw: state.yaw,
                                        pitch: state.pitch,
                                    });
                                }
                            }
                            for r in &mount.parts {
                                drawn.held_parts += usize::from(records < draw::MAX_RECORDS);
                                let at = records;
                                emit(bridge, &mut records, r, hand, None, id, part);
                                if fade > 0. {
                                    self.near_blends.apply(bridge, at, records, fade);
                                }
                                part += 1;
                            }
                            for edge in &mount.edges {
                                if let Some(h) = s.edges.get(index) {
                                    drawn.blades += usize::from(h.draw(
                                        edge,
                                        hand,
                                        self.shown,
                                        &eye,
                                        bridge,
                                        &mut records,
                                        id,
                                        part,
                                        &mut self.ribbons,
                                    ));
                                }
                                index += 1;
                                part += saber::EDGE_PARTS;
                            }
                        }
                    }
                    let light = self
                        .light
                        .at([world.elements[12], world.elements[13], world.elements[14]]);
                    let start = draw::IN_RECORDS + first * draw::REC_STRIDE;
                    let end = draw::IN_RECORDS + records * draw::REC_STRIDE;
                    let (rows, _) =
                        bridge.r.core.input[start..end].as_chunks_mut::<{ draw::REC_STRIDE }>();
                    for row in rows {
                        if row[draw::REC_FLAGS] as u32 & draw::NO_SHADOW == 0 {
                            row[draw::REC_LOOK..draw::REC_LOOK + 4].copy_from_slice(&light);
                        }
                    }
                }
            }
            if let Some(tm) = draw_thrown.filter(|_| state.action != Action::Death && state.seat.is_none()) {
                if let Some(mount) = m
                    .weapons
                    .get(state.weapon)
                    .filter(|w| !w.mounts.is_empty())
                    .or_else(|| {
                        m.weapons
                            .iter()
                            .find(|w| w.mounts.iter().any(|mt| !mt.edges.is_empty()))
                    })
                    .and_then(|w| w.mounts.first())
                {
                    let thrown_first = records;
                    let mut part = m.body.len();
                    for r in &mount.parts {
                        drawn.held_parts += usize::from(records < draw::MAX_RECORDS);
                        emit(bridge, &mut records, r, &tm, None, id, part);
                        part += 1;
                    }
                    let light = self
                        .light
                        .at([tm.elements[12], tm.elements[13], tm.elements[14]]);
                    let start = draw::IN_RECORDS + thrown_first * draw::REC_STRIDE;
                    let end = draw::IN_RECORDS + records * draw::REC_STRIDE;
                    let (rows, _) =
                        bridge.r.core.input[start..end].as_chunks_mut::<{ draw::REC_STRIDE }>();
                    for row in rows {
                        if row[draw::REC_FLAGS] as u32 & draw::NO_SHADOW == 0 {
                            row[draw::REC_LOOK..draw::REC_LOOK + 4].copy_from_slice(&light);
                        }
                    }
                    for (index, edge) in mount.edges.iter().enumerate() {
                        if let Some(h) = s.edges.get(index) {
                            drawn.blades += usize::from(h.draw(
                                edge,
                                &tm,
                                self.shown,
                                &eye,
                                bridge,
                                &mut records,
                                id,
                                part,
                                &mut self.ribbons,
                            ));
                        }
                        part += saber::EDGE_PARTS;
                    }
                }
            }
        }
        (drawn.ribbons, drawn.trail_triangles) = self.ribbons.emit(bridge, &mut records);
        self.drawn = drawn;
        (records, bones)
    }
}

#[cfg(test)]
mod thrown_tests {
    use super::*;

    #[test]
    fn a_thrown_blade_turns_its_tip_toward_the_way_it_flies() {
        // The game turns the blade by the right-hand rotation about normalize(blade x throw), so
        // its tip moves toward the throw direction: a blade held up spins forward over the top, a
        // blade held across to the left sweeps forward and then to the right.
        for blade in [[0., 1., 0.], [1., 0., 0.]] {
            let [bx, by, bz] = blade;
            let mut x = Vector3::new(0., 0., 1.);
            let b = Vector3::new(bx, by, bz);
            let mut y = Vector3::default();
            y.cross_vectors(&b, &x);
            x.cross_vectors(&y, &b);
            let mut hand = Matrix4::default();
            hand.elements = [x.x, x.y, x.z, 0., y.x, y.y, y.z, 0., bx, by, bz, 0., 0., 1.2, 0., 1.];
            let mut tracker = ThrownTracker::default();
            let pose = ThrownSaberPose {
                launched_tick: 11,
                position: [0.0, 1.2, 0.0],
                prev_position: [0.0, 1.2, 0.0],
                direction: [0.0, 0.0, 1.0],
                spin_angle: 0.0,
                prev_spin_angle: 0.0,
                returning: false,
                target: [0.0, 1.2, 0.0],
            };
            tracker.publish(11, Some(&pose), Some(&hand), None);
            let turned = tracker.compose([0., 1.2, 0.], 0.3);
            let tip = [turned.elements[8], turned.elements[9], turned.elements[10]];
            assert!((tip[2] - 0.3f64.sin()).abs() < 1e-9, "{blade:?}: the tip goes toward +z: {tip:?}");
            assert!((tip[0] * bx + tip[1] * by - 0.3f64.cos()).abs() < 1e-9, "{blade:?}: {tip:?}");
        }
    }

    #[test]
    fn thrown_tracker_interpolates_subtick_position_and_spin_plane() {
        let mut tracker = ThrownTracker::default();
        // Hand holds blade tilted in XY along (0.6, 0.8, 0.0), thrown forward along +Z.
        let mut hand = Matrix4::default();
        hand.elements = [
            0.8, -0.6, 0., 0., // X
            0., 0., -1., 0., // Y
            0.6, 0.8, 0., 0., // Z (blade axis in world space)
            0., 1.2, 0., 1., // hand origin
        ];
        tracker.publish(10, None, Some(&hand), None);
        assert!(tracker.matrix_at(10.0).is_none());

        let t1 = ThrownSaberPose {
            launched_tick: 11,
            position: [0.0, 1.2, 0.0],
            prev_position: [0.0, 1.2, 0.0],
            direction: [0.0, 0.0, 1.0],
            spin_angle: 0.0,
            prev_spin_angle: 0.0,
            returning: false,
            target: [0.0, 1.2, 0.0],
        };
        let t2 = ThrownSaberPose {
            position: [0.0, 1.2, 0.5],
            prev_position: [0.0, 1.2, 0.0],
            spin_angle: 0.4,
            prev_spin_angle: 0.0,
            ..t1
        };
        tracker.publish(11, Some(&t1), Some(&hand), None);
        tracker.publish(12, Some(&t2), Some(&hand), None);

        // Spin axis is normalize(blade_dir x throw_dir) = (0.6, 0.8, 0) x (0, 0, 1) = (0.8, -0.6, 0).
        assert!((tracker.spin_axis.x - 0.8).abs() < 1e-6);
        assert!((tracker.spin_axis.y + 0.6).abs() < 1e-6);
        assert!(tracker.spin_axis.z.abs() < 1e-6);
        // Zero spin preserves the release hand rotation intact without re-alignment.
        let m0 = tracker.compose([0., 1.2, 0.], 0.);
        for idx in 0..12 {
            assert!((m0.elements[idx] - hand.elements[idx]).abs() < 1e-6, "idx={idx}");
        }
        let b0 = Vector3::new(m0.elements[8], m0.elements[9], m0.elements[10]);
        assert!(b0.dot(&tracker.spin_axis).abs() < 1e-6);

        // Before launch tick 11.0 the hand holds the saber; at 11.0 it leaves from the release pose.
        assert!(tracker.matrix_at(10.5).is_none());
        let at_launch = tracker.matrix_at(11.0).expect("active at launch tick");
        assert!(at_launch.elements[14].abs() < 1e-6);

        // Sub-tick sampling at 144 Hz / 500 Hz produces distinct interpolated positions & orientations.
        let m_a = tracker.matrix_at(11.25).expect("subtick 11.25");
        let m_b = tracker.matrix_at(11.50).expect("subtick 11.50");
        let m_c = tracker.matrix_at(11.75).expect("subtick 11.75");
        assert!((m_a.elements[14] - 0.125).abs() < 1e-6);
        assert!((m_b.elements[14] - 0.250).abs() < 1e-6);
        assert!((m_c.elements[14] - 0.375).abs() < 1e-6);
        assert!((m_a.elements[8] - m_b.elements[8]).abs() > 1e-3);
        assert!((m_b.elements[8] - m_c.elements[8]).abs() > 1e-3);

        // Catch tick clears the thrown transform once reached, restoring the hilt to the hand.
        tracker.publish(13, None, Some(&hand), None);
        assert!(tracker.matrix_at(12.5).is_some());
        assert!(tracker.matrix_at(13.0).is_none());
    }

    #[test]
    fn thrown_tracker_expires_stale_entries_and_handles_rethrow_and_short_bounce() {
        let mut tracker = ThrownTracker::default();
        let hand = Matrix4::default();
        let t1 = ThrownSaberPose {
            launched_tick: 20,
            position: [0.0, 1.35, 1.5],
            prev_position: [0.0, 1.35, 1.0],
            direction: [0.0, 0.0, 1.0],
            spin_angle: 0.4,
            prev_spin_angle: 0.0,
            returning: false,
            target: [0.0, 1.35, 0.0],
        };
        tracker.publish(19, None, Some(&hand), None);
        tracker.publish(20, Some(&t1), Some(&hand), None);
        assert!(tracker.matrix_at(20.0).is_some());
        assert!(tracker.matrix_at(21.5).is_some());
        // If no further updates arrive while shown advances past the grace window, never freeze in air.
        assert!(tracker.matrix_at(22.1).is_none());

        // Short 1-tick wall bounce clears cleanly on the catch tick.
        tracker.publish(21, None, Some(&hand), None);
        assert!(tracker.matrix_at(20.5).is_some());
        assert!(tracker.matrix_at(21.0).is_none());

        // A second throw on a new direction re-initializes the spin plane cleanly.
        let t2 = ThrownSaberPose {
            launched_tick: 30,
            position: [2.0, 1.35, 0.0],
            prev_position: [1.5, 1.35, 0.0],
            direction: [1.0, 0.0, 0.0],
            ..t1
        };
        tracker.publish(30, Some(&t2), Some(&hand), None);
        assert!(tracker.spin_axis.x.abs() < 1e-6);
        let m2 = tracker.matrix_at(30.0).expect("second throw active");
        assert!((m2.elements[12] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn thrown_tracker_ends_the_blade_on_its_way_to_the_release_point_without_a_speed_spike() {
        let release = [10.0, 1.35, 30.0];
        let mut hand = Matrix4::default();
        hand.set_position(11.0, 1.5, 29.0);
        let dist_to_release = |m: &Matrix4| {
            let d = [m.elements[12] - release[0], m.elements[13] - release[1], m.elements[14] - release[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        };

        // A return at 0.833 m a tick toward the release point, caught on tick 105.
        let mut tracker = ThrownTracker::default();
        tracker.publish(99, None, Some(&hand), None);
        for (idx, z) in [34.20, 33.36, 32.53, 31.70, 30.86].iter().enumerate() {
            let tick = 100 + idx as u32;
            let pose = ThrownSaberPose {
                launched_tick: 100,
                position: [10.0, 1.35, *z],
                prev_position: [10.0, 1.35, *z + 0.833],
                direction: [0.0, 0.0, 1.0],
                spin_angle: 0.4 * f64::from(idx as u32 + 1),
                prev_spin_angle: 0.4 * f64::from(idx as u32),
                returning: true,
                target: release,
            };
            tracker.publish(tick, Some(&pose), Some(&hand), None);
        }
        let step: f64 = 60.0 / 144.0;
        let mut shown: f64 = 100.0;
        let mut published = 104u32;
        let (mut previous, mut biggest, mut last_distance) = (None::<f64>, 0.0f64, f64::MAX);
        let (mut flying, mut held) = (0usize, 0usize);
        while shown <= 150.0 {
            while published < (shown.floor() as u32 + 2).min(155) {
                published += 1;
                tracker.publish(published, None, Some(&hand), None);
            }
            match tracker.matrix_at(shown) {
                Some(m) => {
                    assert!(shown < 105.0, "shown={shown:.3}: the blade is gone from the catch tick on");
                    let d = dist_to_release(&m);
                    if let Some(p) = previous {
                        assert!(d <= p + 1e-9, "shown={shown:.3}: the blade moved away from the release point");
                        biggest = biggest.max((p - d) / step);
                    }
                    assert!(m.elements[14] >= 30.0 - 1e-9, "never past the release point");
                    previous = Some(d);
                    last_distance = d;
                    flying += 1;
                }
                None => {
                    assert!(shown >= 105.0, "shown={shown:.3}: the blade vanished before the catch tick");
                    held += 1;
                }
            }
            shown += step;
        }
        // The last tick moves at the speed of the ones before it (0.833 m a tick), not faster.
        assert!(biggest < 0.9, "no speed spike at the catch: {biggest:.3} m a tick");
        assert!(flying >= 12 && held >= 100);
        assert!(last_distance < 0.4, "the blade is within one frame of the release point when the hilt takes over: {last_distance:.3} m");

        // A blade that ended its outbound pass on a wall and was caught the next tick never turned
        // back: it stays where it stopped instead of moving on into the wall.
        let mut bounce = ThrownTracker::default();
        bounce.publish(19, None, Some(&hand), None);
        let stopped = ThrownSaberPose {
            launched_tick: 20,
            position: [10.0, 1.35, 30.6],
            prev_position: [10.0, 1.35, 30.0],
            direction: [0.0, 0.0, 1.0],
            spin_angle: 0.4,
            prev_spin_angle: 0.0,
            returning: false,
            target: release,
        };
        bounce.publish(20, Some(&stopped), Some(&hand), None);
        bounce.publish(21, None, Some(&hand), None);
        for frac in [0.25, 0.50, 0.75, 0.95] {
            let m = bounce.matrix_at(20.0 + frac).expect("drawn until the catch tick");
            assert!((m.elements[14] - 30.6).abs() < 1e-9, "stays at the wall: z={}", m.elements[14]);
        }
        assert!(bounce.matrix_at(21.0).is_none());
    }
}
