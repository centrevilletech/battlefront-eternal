//! Local space observations use the same mission bindings as replicated combat.
use super::{
    rounds::{Rounds, ShotAdmission},
    Burst, EffectBody, RoundClass, Shot,
};
use crate::local::space::{Aim, Frame, History, Hull};
use crate::online::{
    combat::Combat,
    turrets::{TurretMuzzle, Turrets},
};
use audio_control::swbf2::zero_hash;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_sim::space::{
    combat::EmittingBody,
    observations::{Contact, Round},
};
use std::collections::BTreeMap;
use three::{Quaternion, Vector3};

struct Weapon {
    linear: Option<RoundClass>,
    tracer: Option<RoundClass>,
    flash: Option<usize>,
    trails: Vec<usize>,
    /// The class hash of a round drawn as a mounted body.
    mount: Option<u32>,
    impact: [Option<usize>; 3],
    explosion: Option<usize>,
}

impl Weapon {
    /// What a hit plays: the struck surface's effect and the round's explosion. Most flight
    /// guns name an explosion that harms nothing, only to be seen.
    fn strike(&self, hit: &Contact) -> [Option<usize>; 2] {
        let surface = if hit.shield { 2 } else { usize::from(hit.rigid) };
        [self.impact[surface], self.explosion]
    }
}

struct Trail {
    serial: u64,
    group: usize,
    handle: u32,
    seen: bool,
}

enum AnchorTarget {
    Fighter {
        hull: u32,
        generation: u32,
        local_origin: [f64; 3],
        local_dir: [f64; 3],
    },
    Turret {
        building: u32,
        barrel: Option<u32>,
        launch_yaw: f64,
        launch_pitch: f64,
        launch_origin: [f64; 3],
        launch_dir: [f64; 3],
        local_dir: [f64; 3],
        bound: bool,
    },
}

struct BeamAnchor {
    serial: [u32; 2],
    target: AnchorTarget,
}

#[derive(Default)]
struct Counts {
    bolts: u32,
    tracers: u32,
    rockets: u32,
    impacts: u32,
    blasts: u32,
    deaths: u32,
}

pub(crate) struct SpaceEffects {
    weapons: BTreeMap<u32, Weapon>,
    destruction: BTreeMap<u32, usize>,
    hull_lives: Vec<Option<u32>>,
    rocket_lives: Vec<(u64, u32)>,
    last_hull_tick: Option<u32>,
    trails: Vec<Trail>,
    beam_anchors: Vec<BeamAnchor>,
    last: Option<u32>,
    counts: Counts,
    trace: Option<std::io::BufWriter<std::fs::File>>,
}

/// A finished round clips a live beam or fast bolt only when it struck something.
/// A missed instant beam retires on launch without advancing past its origin.
fn finish_position(round: &Round) -> Option<[f64; 3]> {
    round.contact.map(|_| round.position)
}

const IDENTITY_BASIS: [[f64; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize_or_z(v: [f64; 3]) -> [f64; 3] {
    let mag = dot3(v, v).sqrt();
    if mag.is_finite() && mag > 1e-9 {
        [v[0] / mag, v[1] / mag, v[2] / mag]
    } else {
        [0., 0., 1.]
    }
}

fn short_arc_lerp(previous: f64, current: f64, alpha: f64) -> f64 {
    let delta = current - previous;
    previous + alpha * delta.sin().atan2(delta.cos())
}

fn apply_basis(v: [f64; 3], basis: [[f64; 3]; 3]) -> [f64; 3] {
    std::array::from_fn(|k| basis[0][k] * v[0] + basis[1][k] * v[1] + basis[2][k] * v[2])
}

fn project_into_basis(v: [f64; 3], basis: [[f64; 3]; 3]) -> [f64; 3] {
    [dot3(v, basis[0]), dot3(v, basis[1]), dot3(v, basis[2])]
}

/// Orthonormal world basis of a space turret aimed at `(yaw, pitch)`: `root * R_y(yaw) * R_x(-pitch)`.
fn turret_aim_basis(root: [[f64; 3]; 3], yaw: f64, pitch: f64) -> [[f64; 3]; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let lx = [cy, 0., -sy];
    let ly = [-sy * sp, cp, -cy * sp];
    let lz = [sy * cp, sp, cy * cp];
    [
        apply_basis(lx, root),
        apply_basis(ly, root),
        apply_basis(lz, root),
    ]
}

fn beam_anchor(frame: &Frame, round: &Round, serial: [u32; 2]) -> Option<BeamAnchor> {
    let mag = dot3(round.initial_velocity, round.initial_velocity).sqrt();
    if !mag.is_finite() || mag <= 0. || !round.origin.iter().all(|v| v.is_finite()) {
        return None;
    }
    let launch_dir = [
        round.initial_velocity[0] / mag,
        round.initial_velocity[1] / mag,
        round.initial_velocity[2] / mag,
    ];
    match round.emitter? {
        EmittingBody::Fighter { index, generation } => {
            let hull = frame
                .hulls
                .iter()
                .find(|h| h.id == index && h.generation == generation)?;
            let [qx, qy, qz, qw] = hull.current.rotation;
            let mut inv = Quaternion::new(qx, qy, qz, qw);
            inv.conjugate();
            let mut local_origin = Vector3::new(
                round.origin[0] - hull.current.position[0],
                round.origin[1] - hull.current.position[1],
                round.origin[2] - hull.current.position[2],
            );
            local_origin.apply_quaternion(&inv);
            let mut local_dir = Vector3::new(launch_dir[0], launch_dir[1], launch_dir[2]);
            local_dir.apply_quaternion(&inv);
            local_dir.normalize();
            Some(BeamAnchor {
                serial,
                target: AnchorTarget::Fighter {
                    hull: index as u32,
                    generation,
                    local_origin: [local_origin.x, local_origin.y, local_origin.z],
                    local_dir: [local_dir.x, local_dir.y, local_dir.z],
                },
            })
        }
        EmittingBody::Building { index, .. } => {
            let [launch_yaw, launch_pitch] = frame
                .turrets
                .binary_search_by_key(&index, |t| t.building)
                .ok()
                .map_or([0., 0.], |i| frame.turrets[i].current);
            Some(BeamAnchor {
                serial,
                target: AnchorTarget::Turret {
                    building: index as u32,
                    barrel: round.barrel.map(|b| b as u32),
                    launch_yaw,
                    launch_pitch,
                    launch_origin: round.origin,
                    launch_dir,
                    local_dir: [0., 0., 1.],
                    bound: false,
                },
            })
        }
    }
}

pub(crate) trait SpaceShotSink {
    fn observe_shot(&mut self, shot: Shot) -> bool;
    fn has_beam(&self, serial: [u32; 2]) -> bool;
    fn finish_live(&mut self, serial: [u32; 2], tick: f64, at: Option<[f64; 3]>);
    fn move_beam(&mut self, serial: [u32; 2], position: [f64; 3], direction: [f64; 3]) -> bool;
    fn observe_burst(&mut self, burst: Burst);
}

impl SpaceShotSink for Combat {
    fn observe_shot(&mut self, shot: Shot) -> bool {
        Combat::observe_shot(self, shot)
    }
    fn has_beam(&self, serial: [u32; 2]) -> bool {
        self.mission_effects
            .as_ref()
            .is_some_and(|fx| fx.rounds.has_beam(serial))
    }
    fn finish_live(&mut self, serial: [u32; 2], tick: f64, at: Option<[f64; 3]>) {
        if let Some(fx) = &mut self.mission_effects {
            fx.rounds.finish_live_words(serial, tick, at);
        }
    }
    fn move_beam(&mut self, serial: [u32; 2], position: [f64; 3], direction: [f64; 3]) -> bool {
        self.mission_effects
            .as_mut()
            .is_some_and(|fx| fx.rounds.move_beam(serial, position, direction))
    }
    fn observe_burst(&mut self, burst: Burst) {
        if let Some(fx) = &mut self.mission_effects {
            fx.observe(burst);
        }
    }
}

struct RoundsSink {
    rounds: Rounds,
    next_serial: u32,
    refused: u32,
}

impl SpaceShotSink for RoundsSink {
    fn observe_shot(&mut self, shot: Shot) -> bool {
        match self.rounds.observe_shot(&mut self.next_serial, shot) {
            ShotAdmission::Accepted { .. } => true,
            ShotAdmission::Ignored => false,
            ShotAdmission::Refused => {
                self.refused = self.refused.saturating_add(1);
                false
            }
        }
    }
    fn has_beam(&self, serial: [u32; 2]) -> bool {
        self.rounds.has_beam(serial)
    }
    fn finish_live(&mut self, serial: [u32; 2], tick: f64, at: Option<[f64; 3]>) {
        self.rounds.finish_live_words(serial, tick, at);
    }
    fn move_beam(&mut self, serial: [u32; 2], position: [f64; 3], direction: [f64; 3]) -> bool {
        self.rounds.move_beam(serial, position, direction)
    }
    fn observe_burst(&mut self, _burst: Burst) {}
}

fn anchor_pose(hull: &Hull, alpha: f64, local_origin: [f64; 3], local_dir: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let [px, py, pz, pw] = hull.previous.rotation;
    let [cx, cy, cz, cw] = hull.current.rotation;
    let mut q = Quaternion::new(px, py, pz, pw);
    q.slerp(&Quaternion::new(cx, cy, cz, cw), alpha);
    let mut offset = Vector3::new(local_origin[0], local_origin[1], local_origin[2]);
    offset.apply_quaternion(&q);
    let mut dir = Vector3::new(local_dir[0], local_dir[1], local_dir[2]);
    dir.apply_quaternion(&q);
    dir.normalize();
    let pos: [f64; 3] = std::array::from_fn(|i| {
        hull.previous.position[i] + alpha * (hull.current.position[i] - hull.previous.position[i])
    });
    (
        [pos[0] + offset.x, pos[1] + offset.y, pos[2] + offset.z],
        [dir.x, dir.y, dir.z],
    )
}

fn first<'a>(fields: &'a Value, key: &str) -> Option<&'a str> {
    fields.get(key)?.at(0)?.as_str()
}

fn rotation(velocity: [f64; 3]) -> [f64; 4] {
    let [x, y, z] = velocity;
    let mut dir = Vector3::new(x, y, z);
    dir.normalize();
    let mut q = Quaternion::default();
    q.set_from_unit_vectors(&Vector3::new(0., 0., 1.), &dir);
    [q.x, q.y, q.z, q.w]
}

impl SpaceEffects {
    pub(crate) fn new(
        content: &BattleContent,
        combat: &mut Combat,
    ) -> Result<Option<Self>, String> {
        let Some(fx) = &mut combat.mission_effects else {
            return Ok(None);
        };
        let pack = content.visual_effects();
        let Some(links) = pack.get("links").and_then(Value::as_object) else {
            return Ok(None);
        };
        let explosion = |fields: &Value| {
            let class =
                first(fields, "ExplosionDestruct").or_else(|| first(fields, "ExplosionName"))?;
            fx.binding(class, "Effect", 0)
        };
        let mut weapons = BTreeMap::new();
        let mut destruction = BTreeMap::new();
        for (key, fields) in links.entries() {
            if let Some(name) = key.strip_prefix("entc/") {
                if let Some(group) = explosion(fields) {
                    destruction.insert(zero_hash(name), group);
                }
            }
            let Some(name) = key.strip_prefix("wpnc/") else {
                continue;
            };
            let Some(round) = first(fields, "OrdnanceName") else {
                continue;
            };
            let mut trails = Vec::new();
            let bindings = pack.get("bindings").and_then(|v| v.get(round));
            for property in ["OrdnanceEffect", "TrailEffect"] {
                let count = bindings
                    .and_then(|v| v.get(property))
                    .and_then(Value::as_array)
                    .map_or(0, |v| v.len());
                trails.extend((0..count).filter_map(|i| fx.binding(round, property, i)));
            }
            let mount = round.strip_prefix("entc/").and_then(|class| {
                pack.get("mounts")?.get(class)?.get("maxHealth")?.as_f64()?;
                fx.mount_class(class)?;
                Some(bfe_content::zero_hash::fnv1a(class))
            });
            let impact = fx.binding(round, "ImpactEffect", 0);
            weapons.insert(
                zero_hash(name),
                Weapon {
                    linear: fx.round(key, 0),
                    tracer: fx.secondary_round(key, 0),
                    flash: fx.binding(key, "MuzzleFlashEffect", 0),
                    trails,
                    mount,
                    impact: [
                        impact,
                        fx.binding(round, "ImpactEffectRigid", 0).or(impact),
                        fx.binding(round, "ImpactEffectShield", 0).or(impact),
                    ],
                    explosion: links.get(round).and_then(explosion),
                },
            );
        }
        let limit = bfe_sim::space::combat::MAX_SPACE_PROJECTILES;
        fx.rounds.reserve_live(limit);
        let trace = std::env::var_os("BFE_DEV_SWBF2_FX_TRACE")
            .map(std::fs::File::create)
            .transpose()
            .map_err(|e| e.to_string())?
            .map(std::io::BufWriter::new);
        Ok(Some(Self {
            weapons,
            destruction,
            hull_lives: Vec::new(),
            rocket_lives: Vec::with_capacity(limit),
            last_hull_tick: None,
            trails: Vec::with_capacity(limit * 4),
            beam_anchors: Vec::with_capacity(limit),
            last: None,
            counts: Counts::default(),
            trace,
        }))
    }

    pub(crate) fn reserve_fleet(&mut self, slots: usize) {
        self.hull_lives.resize(slots, None);
    }

    fn fleet_step(&mut self, frame: &Frame, fx: &mut super::MissionEffects) {
        // The publisher visits live hulls in slot order. Missing slots have retired.
        let mut hulls = frame.hulls.iter().peekable();
        for (slot, previous) in self.hull_lives.iter_mut().enumerate() {
            let hull = hulls.next_if(|h| h.id == slot);
            let generation = hull.map(|h| h.generation);
            if *previous != generation {
                if let Some(old) = *previous {
                    fx.retire_mount(slot as u64, old);
                }
                *previous = generation;
            }
            let Some(h) = hull.filter(|h| h.max_health > 0.) else { continue };
            let pose = h.previous.matrix(&h.current, 1.);
            let mut rotation = Quaternion::default();
            rotation.set_from_rotation_matrix(&pose);
            let body = EffectBody {
                owner: slot as u64,
                generation: h.generation,
                class: h.class,
                tick: frame.tick,
                position: h.current.position,
                rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
                velocity: h.velocity,
            };
            fx.ship_state(body, h.thrusting, (h.health / h.max_health).clamp(0., 1.));
        }
        // A mounted round is one fixed-step body per life, at its published end point.
        self.rocket_lives.retain(|&(id, generation)| {
            let live = frame.rounds.iter().any(|(slot, round)| {
                (1 << 32) | *slot as u64 == id && round.serial as u32 == generation && round.active
            });
            if !live {
                fx.retire_mount(id, generation);
            }
            live
        });
        for (slot, r) in &frame.rounds {
            let Some(class) = self.weapons.get(&r.weapon).and_then(|w| w.mount).filter(|_| r.active) else {
                continue;
            };
            let id = ((1 << 32) | *slot as u64, r.serial as u32);
            if !self.rocket_lives.contains(&id) {
                self.rocket_lives.push(id);
            }
            let body = EffectBody {
                owner: id.0,
                generation: id.1,
                class,
                tick: frame.tick,
                position: r.position,
                rotation: rotation(r.velocity),
                velocity: r.velocity,
            };
            fx.ship_state(body, true, 1.);
        }
    }

    fn track_beam(
        &mut self,
        frame: &Frame,
        round: &Round,
        beam_serial: [u32; 2],
        sink: &impl SpaceShotSink,
    ) {
        if self.beam_anchors.len() < self.beam_anchors.capacity() && sink.has_beam(beam_serial) {
            if let Some(anchor) = beam_anchor(frame, round, beam_serial) {
                self.beam_anchors.push(anchor);
            }
        }
    }

    fn events(&mut self, frame: &Frame, sink: &mut impl SpaceShotSink) {
        // An online frame stands for every tick since the one before it.
        let win_start = frame.prev_tick.unwrap_or_else(|| frame.tick.saturating_sub(1));
        for (_, r) in &frame.rounds {
            let Some(w) = self.weapons.get(&r.weapon) else {
                continue;
            };
            let serial = (1 << 62) | r.serial;
            let tracer_serial = serial | (1 << 61);
            let serial_words = [serial as u32, (serial >> 32) as u32];
            let tracer_words = [tracer_serial as u32, (tracer_serial >> 32) as u32];
            let launch = r.born.saturating_sub(1) as f64;
            let (linear, tracer, flash, has_rocket, impact_hit) = (
                w.linear,
                w.tracer,
                w.flash,
                !w.trails.is_empty() || w.mount.is_some(),
                r.contact.map(|hit| (hit, w.strike(&hit))),
            );
            if r.born > win_start && r.born <= frame.tick {
                if let Some(round) = linear {
                    if sink.observe_shot(Shot {
                        round,
                        serial,
                        tick: launch,
                        position: r.origin,
                        direction: r.initial_velocity,
                        contact: None,
                    }) {
                        self.counts.bolts = self.counts.bolts.saturating_add(1);
                        self.track_beam(frame, r, serial_words, sink);
                    }
                } else {
                    if has_rocket {
                        self.counts.rockets = self.counts.rockets.saturating_add(1);
                    }
                    if let Some(group) = flash {
                        sink.observe_burst(Burst {
                            serial,
                            group,
                            tick: launch,
                            position: r.origin,
                            normal: None,
                        });
                    }
                }
                if r.tracer {
                    if let Some(tracer) = tracer {
                        if sink.observe_shot(Shot {
                            round: tracer,
                            serial: tracer_serial,
                            tick: launch,
                            position: r.origin,
                            direction: r.initial_velocity,
                            contact: None,
                        }) {
                            self.counts.tracers = self.counts.tracers.saturating_add(1);
                            self.track_beam(frame, r, tracer_words, sink);
                        }
                    }
                }
            }
            let hit = finish_position(r);
            let hit_tick = r.tick.min(frame.tick) as f64;
            if !r.active && linear.is_some() {
                sink.finish_live(serial_words, hit_tick, hit);
            }
            if !r.active && r.tracer && tracer.is_some() {
                sink.finish_live(tracer_words, frame.tick as f64, hit);
            }
            if let Some((hit, [impact, explosion])) = impact_hit {
                let burst = |group| Burst {
                    serial,
                    group,
                    tick: hit_tick,
                    position: r.position,
                    normal: hit.normal,
                };
                if let Some(group) = impact {
                    sink.observe_burst(burst(group));
                    self.counts.impacts = self.counts.impacts.saturating_add(1);
                }
                if let Some(group) = explosion {
                    sink.observe_burst(burst(group));
                    self.counts.blasts = self.counts.blasts.saturating_add(1);
                }
            }
        }
        for (i, death) in frame.deaths.iter().enumerate() {
            if let Some(&group) = self.destruction.get(&death.class) {
                sink.observe_burst(Burst {
                    serial: (1 << 63) | (u64::from(frame.tick) << 16) | i as u64,
                    group,
                    tick: frame.tick as f64,
                    position: death.position,
                    normal: None,
                });
                self.counts.deaths = self.counts.deaths.saturating_add(1);
            }
        }
    }

    pub(crate) fn repose_beams(
        &mut self,
        history: &History,
        shown: f64,
        turrets: Option<&Turrets>,
        sink: &mut impl SpaceShotSink,
    ) {
        let Some((frame, alpha)) = history.sample(shown) else {
            return;
        };
        let lookup = |building: usize, barrel: Option<usize>| {
            turrets.and_then(|rig| rig.muzzle_pose(building, barrel))
        };
        self.repose_beams_on(frame, alpha, &lookup, sink);
    }

    fn repose_beams_on(
        &mut self,
        frame: &Frame,
        alpha: f64,
        turret_muzzle: &dyn Fn(usize, Option<usize>) -> Option<TurretMuzzle>,
        sink: &mut impl SpaceShotSink,
    ) {
        self.beam_anchors.retain_mut(|anchor| match &mut anchor.target {
            AnchorTarget::Fighter {
                hull,
                generation,
                local_origin,
                local_dir,
            } => {
                let Some(found) = frame
                    .hulls
                    .iter()
                    .find(|h| h.id as u32 == *hull && h.generation == *generation)
                else {
                    return false;
                };
                let (position, direction) = anchor_pose(found, alpha, *local_origin, *local_dir);
                sink.move_beam(anchor.serial, position, direction)
            }
            AnchorTarget::Turret {
                building,
                barrel,
                launch_yaw,
                launch_pitch,
                launch_origin,
                launch_dir,
                local_dir,
                bound,
            } => {
                let bidx = *building as usize;
                let barrel_idx = barrel.map(|b| b as usize);
                if let Some(muzzle) = turret_muzzle(bidx, barrel_idx) {
                    if !*bound {
                        let launch_basis =
                            turret_aim_basis(muzzle.root_basis, *launch_yaw, *launch_pitch);
                        *local_dir = normalize_or_z(project_into_basis(*launch_dir, launch_basis));
                        *bound = true;
                    }
                    let direction = apply_basis(*local_dir, muzzle.basis);
                    sink.move_beam(anchor.serial, muzzle.origin, direction)
                } else if let Ok(i) = frame.turrets.binary_search_by_key(&bidx, |t| t.building) {
                    let aim = &frame.turrets[i];
                    let yaw = short_arc_lerp(aim.previous[0], aim.current[0], alpha);
                    let pitch = short_arc_lerp(aim.previous[1], aim.current[1], alpha);
                    let launch_basis =
                        turret_aim_basis(IDENTITY_BASIS, *launch_yaw, *launch_pitch);
                    let dir_local = normalize_or_z(project_into_basis(*launch_dir, launch_basis));
                    let cur_basis = turret_aim_basis(IDENTITY_BASIS, yaw, pitch);
                    let direction = apply_basis(dir_local, cur_basis);
                    sink.move_beam(anchor.serial, *launch_origin, direction)
                } else {
                    false
                }
            }
        });
    }

    pub(crate) fn frame(&mut self, history: &History, shown: f64, combat: &mut Combat) {
        let Some((frame, alpha)) = history.sample(shown) else {
            return;
        };
        let fx = combat.mission_effects.as_mut().unwrap();
        for fixed in history.after(self.last_hull_tick, u32::MAX) {
            self.fleet_step(fixed, fx);
            self.last_hull_tick = Some(fixed.tick);
        }
        let previous = self.last;
        for new in history.after(previous, frame.tick) {
            self.events(new, combat);
            self.last = Some(new.tick);
        }
        for trail in &mut self.trails {
            trail.seen = false;
        }
        for (_, r) in &frame.rounds {
            if !r.active && alpha >= 1. {
                continue;
            }
            let Some(w) = self.weapons.get(&r.weapon) else {
                continue;
            };
            if w.trails.is_empty() {
                continue;
            }
            let position =
                std::array::from_fn(|i| r.previous[i] + alpha * (r.position[i] - r.previous[i]));
            let turn = rotation(r.velocity);
            for &group in &w.trails {
                if let Some(t) = self.trails.iter_mut().find(|t| {
                    t.serial == r.serial && t.group == group && combat.effects.playing(t.handle)
                }) {
                    t.seen = true;
                    combat
                        .effects
                        .sync(t.handle, position, Some(turn), Some(r.velocity));
                } else if self.trails.len() < self.trails.capacity() {
                    if let Some(handle) = combat.effects.start_turned(
                        group,
                        position,
                        r.velocity,
                        turn,
                        r.serial as u32,
                    ) {
                        self.trails.push(Trail {
                            serial: r.serial,
                            group,
                            handle,
                            seen: true,
                        });
                    }
                }
            }
        }
        self.trails.retain(|t| {
            if !t.seen {
                combat.effects.stop(t.handle);
            }
            t.seen
        });
        if self.last != previous {
            use std::io::Write;
            if let Some(out) = &mut self.trace {
                let fx = combat.mission_effects.as_ref().unwrap();
                let c = &self.counts;
                let [thrust, contrails, hull_smoke, system_smoke] = fx.mounts.active_emitters();
                let _ = writeln!(
                    out,
                    "{},{},{},{},{},{},{},{},{},{},{},{thrust},{contrails},{hull_smoke},{system_smoke},{}",
                    frame.tick,
                    c.bolts,
                    c.rockets,
                    c.impacts,
                    c.blasts,
                    c.deaths,
                    self.trails.len(),
                    self.rocket_lives.len() + self.hull_lives.iter().flatten().count(),
                    fx.bursts,
                    fx.refused,
                    combat.total_blasters,
                    c.tracers
                );
            }
        }
    }
    fn harness_for_beam(weapon_hash: u32, range: f64, life: f64, capacity: usize) -> (Self, RoundsSink) {
        let (rounds, round_class) = Rounds::for_beam_harness(range, life, capacity);
        let mut weapons = BTreeMap::new();
        weapons.insert(
            weapon_hash,
            Weapon {
                linear: Some(round_class),
                tracer: None,
                flash: None,
                trails: Vec::new(),
                mount: None,
                impact: [None; 3],
                explosion: None,
            },
        );
        (
            Self {
                weapons,
                destruction: BTreeMap::new(),
                hull_lives: Vec::new(),
                rocket_lives: Vec::new(),
                last_hull_tick: None,
                trails: Vec::new(),
                beam_anchors: Vec::with_capacity(capacity),
                last: None,
                counts: Counts::default(),
                trace: None,
            },
            RoundsSink {
                rounds,
                next_serial: 0,
                refused: 0,
            },
        )
    }
}

#[doc(hidden)]
pub struct SpaceBeamHarness {
    effects: SpaceEffects,
    sink: RoundsSink,
    frame: Frame,
    weapon_hash: u32,
    next_round_id: u32,
}

#[doc(hidden)]
impl SpaceBeamHarness {
    pub fn new(capacity: usize) -> Self {
        let weapon_hash = 0x51a7_0001;
        let (effects, sink) = SpaceEffects::harness_for_beam(weapon_hash, 180., 0.3, capacity);
        let mut frame = Frame::default();
        frame.hulls = Vec::with_capacity(2);
        frame.turrets = Vec::with_capacity(1);
        frame.rounds = Vec::with_capacity(2);
        Self {
            effects,
            sink,
            frame,
            weapon_hash,
            next_round_id: 1,
        }
    }

    pub fn step(&mut self, step: usize) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
        let tick_u32 = 10u32.saturating_add(step as u32);
        let tick = tick_u32.into();
        let angle = (step as f64) * 0.05;
        let prev_angle = angle - 0.05;
        let mut q_prev = Quaternion::default();
        q_prev.set_from_axis_angle(&Vector3::new(0., 1., 0.), prev_angle);
        let mut q_cur = Quaternion::default();
        q_cur.set_from_axis_angle(&Vector3::new(0., 1., 0.), angle);

        self.frame.tick = tick;
        self.frame.hulls.clear();
        let mut hull = Hull {
            id: 2,
            generation: 5,
            team: 1,
            ..Default::default()
        };
        hull.previous.position = [10. + (step as f64 - 1.) * 2., 3., 5.];
        hull.previous.rotation = [q_prev.x, q_prev.y, q_prev.z, q_prev.w];
        hull.current.position = [10. + (step as f64) * 2., 3., 5.];
        hull.current.rotation = [q_cur.x, q_cur.y, q_cur.z, q_cur.w];
        self.frame.hulls.push(hull);

        self.frame.turrets.clear();
        self.frame.turrets.push(Aim {
            building: 9,
            previous: [prev_angle, 0.1],
            current: [angle, 0.1],
            class: 0,
            weapon: 0,
            position: [0.; 3],
            yaw_rate: 0.,
            pitch_rate: 0.,
            alive: true,
            held: false,
            fired: false,
            firing: false,
            spinning: false,
            overheated: false,
            heat: 0.,
            heat_threshold: 0.,
        });

        self.frame.rounds.clear();
        if step.is_multiple_of(15) {
            let f_serial = self.next_round_id.into();
            self.next_round_id = self.next_round_id.wrapping_add(1).max(1);
            let mut offset = Vector3::new(0., 0., 4.);
            offset.apply_quaternion(&q_cur);
            let mut f_dir = Vector3::new(0., 0., 1.);
            f_dir.apply_quaternion(&q_cur);
            self.frame.rounds.push((
                0,
                Round {
                    serial: f_serial,
                    weapon: self.weapon_hash,
                    emitter: Some(EmittingBody::Fighter {
                        index: 2,
                        generation: 5,
                    }),
                    born: tick,
                    tick,
                    origin: [
                        hull.current.position[0] + offset.x,
                        hull.current.position[1] + offset.y,
                        hull.current.position[2] + offset.z,
                    ],
                    initial_velocity: [f_dir.x * 1200., f_dir.y * 1200., f_dir.z * 1200.],
                    position: [
                        hull.current.position[0] + offset.x,
                        hull.current.position[1] + offset.y,
                        hull.current.position[2] + offset.z,
                    ],
                    active: false,
                    contact: None,
                    ..Default::default()
                },
            ));
            let t_serial = self.next_round_id.into();
            self.next_round_id = self.next_round_id.wrapping_add(1).max(1);
            let t_basis = turret_aim_basis(IDENTITY_BASIS, angle, 0.1);
            let t_origin = [
                100. + t_basis[2][0] * 6.,
                20. + t_basis[2][1] * 6.,
                -40. + t_basis[2][2] * 6.,
            ];
            self.frame.rounds.push((
                1,
                Round {
                    serial: t_serial,
                    weapon: self.weapon_hash,
                    barrel: Some(step % 2),
                    emitter: Some(EmittingBody::Building {
                        index: 9,
                        life: 1.,
                    }),
                    born: tick,
                    tick,
                    origin: t_origin,
                    initial_velocity: [
                        t_basis[2][0] * 1200.,
                        t_basis[2][1] * 1200.,
                        t_basis[2][2] * 1200.,
                    ],
                    position: t_origin,
                    active: false,
                    contact: None,
                    ..Default::default()
                },
            ));
        }

        self.effects.events(&self.frame, &mut self.sink);
        let shown_yaw = short_arc_lerp(prev_angle, angle, 0.5);
        let muzzle_basis = turret_aim_basis(IDENTITY_BASIS, shown_yaw, 0.1);
        let muzzle_lookup = |building: usize, barrel: Option<usize>| -> Option<TurretMuzzle> {
            (building == 9).then(|| {
                let side = if barrel == Some(1) { -1.2 } else { 1.2 };
                let offset = apply_basis([side, 0., 6.], muzzle_basis);
                TurretMuzzle {
                    origin: [100. + offset[0], 20. + offset[1], -40. + offset[2]],
                    basis: muzzle_basis,
                    root_basis: IDENTITY_BASIS,
                }
            })
        };
        self.effects
            .repose_beams_on(&self.frame, 0.5, &muzzle_lookup, &mut self.sink);
        let draws = self.sink.rounds.take_beams(f64::from(tick_u32));
        let first = draws
            .first()
            .map_or(([0.; 3], [0.; 3]), |b| (b.position, b.end));
        let last = draws
            .last()
            .map_or(([0.; 3], [0.; 3]), |b| (b.position, b.end));
        self.sink.rounds.return_beams(draws);
        (first.0, first.1, last.0, last.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_plays_its_surface_and_its_explosion() {
        let mut weapon = Weapon {
            linear: None,
            tracer: None,
            flash: None,
            trails: Vec::new(),
            mount: None,
            impact: [Some(3), Some(4), None],
            explosion: Some(9),
        };
        let hit = |shield, rigid| Contact { shield, rigid, ..Contact::default() };
        assert_eq!(weapon.strike(&hit(false, false)), [Some(3), Some(9)]);
        assert_eq!(weapon.strike(&hit(false, true)), [Some(4), Some(9)]);
        // A shield without an effect of its own still shows the explosion.
        assert_eq!(weapon.strike(&hit(true, true)), [None, Some(9)]);
        weapon.explosion = None;
        assert_eq!(weapon.strike(&hit(false, true)), [Some(4), None]);
    }

    #[test]
    fn events_preserves_authored_range_on_missed_instant_beam_and_clips_on_contact() {
        let weapon = 0x1020_3040;
        let (mut effects, mut sink) = SpaceEffects::harness_for_beam(weapon, 175., 0.25, 16);
        let mut frame = Frame::default();
        frame.tick = 40;
        // Missed instant beam retires on launch without advancing past origin.
        frame.rounds.push((
            0,
            Round {
                serial: 11,
                weapon,
                born: 40,
                tick: 40,
                origin: [12., 4., -8.],
                initial_velocity: [0., 0., 1200.],
                position: [12., 4., -8.],
                active: false,
                contact: None,
                ..Default::default()
            },
        ));
        // Hit instant beam clips to its recorded impact point.
        frame.rounds.push((
            1,
            Round {
                serial: 12,
                weapon,
                born: 40,
                tick: 40,
                origin: [20., 4., -8.],
                initial_velocity: [0., 0., 1200.],
                position: [20., 4., 52.],
                active: false,
                contact: Some(Contact {
                    rigid: true,
                    ..Contact::default()
                }),
                ..Default::default()
            },
        ));
        effects.events(&frame, &mut sink);
        let draws = sink.rounds.take_beams(40.);
        assert_eq!(draws.len(), 2);
        assert_eq!(draws[0].position, [12., 4., -8.]);
        assert_eq!(draws[0].end, [12., 4., 167.]);
        assert_eq!(draws[1].position, [20., 4., -8.]);
        assert_eq!(draws[1].end, [20., 4., 52.]);
        sink.rounds.return_beams(draws);
    }

    #[test]
    fn events_anchors_fighter_beam_to_its_own_emitter_even_with_a_closer_friendly_hull() {
        let weapon = 0x2233_4455;
        let (mut effects, mut sink) = SpaceEffects::harness_for_beam(weapon, 150., 0.25, 16);
        let mut nearby = Hull {
            id: 1,
            generation: 2,
            team: 2,
            ..Default::default()
        };
        nearby.previous.position = [10., 2., 7.];
        nearby.previous.rotation = [0., 0., 0., 1.];
        nearby.current = nearby.previous;

        let mut owner = Hull {
            id: 3,
            generation: 7,
            team: 2,
            ..Default::default()
        };
        owner.previous.position = [10., 2., 5.];
        owner.previous.rotation = [0., 0., 0., 1.];
        owner.current = owner.previous;

        let mut frame = Frame::default();
        frame.tick = 20;
        frame.hulls = vec![nearby, owner];
        // Muzzle is 2 m from `nearby` but 4 m from `owner`, owned explicitly by Fighter 3.
        frame.rounds.push((
            0,
            Round {
                serial: 21,
                weapon,
                team: 2,
                emitter: Some(EmittingBody::Fighter {
                    index: 3,
                    generation: 7,
                }),
                born: 20,
                tick: 20,
                origin: [10., 2., 9.],
                initial_velocity: [0., 0., 1200.],
                position: [10., 2., 9.],
                active: false,
                contact: None,
                ..Default::default()
            },
        ));
        effects.events(&frame, &mut sink);

        // Next frame: move `nearby` opposite while `owner` translates and yaws 90 deg (+Z -> +X).
        let mut turned = Quaternion::default();
        turned.set_from_axis_angle(&Vector3::new(0., 1., 0.), std::f64::consts::FRAC_PI_2);
        frame.tick = 21;
        frame.rounds.clear();
        frame.hulls[0].previous = frame.hulls[0].current;
        frame.hulls[0].current.position = [-50., 2., -50.];
        frame.hulls[1].previous = frame.hulls[1].current;
        frame.hulls[1].current.position = [30., 2., 25.];
        frame.hulls[1].current.rotation = [turned.x, turned.y, turned.z, turned.w];

        let no_turrets = |_: usize, _: Option<usize>| None;
        effects.repose_beams_on(&frame, 1.0, &no_turrets, &mut sink);
        let draws = sink.rounds.take_beams(21.);
        assert_eq!(draws.len(), 1);
        assert!((draws[0].position[0] - 34.).abs() < 1e-6);
        assert!((draws[0].position[1] - 2.).abs() < 1e-6);
        assert!((draws[0].position[2] - 25.).abs() < 1e-6);
        assert!((draws[0].end[0] - 184.).abs() < 1e-6);
        assert!((draws[0].direction[0] - 1.).abs() < 1e-6);
        sink.rounds.return_beams(draws);
    }

    #[test]
    fn events_anchors_turret_and_chair_beams_to_posed_muzzle_while_yawing_and_pitching() {
        let weapon = 0x3344_5566;
        let (mut effects, mut sink) = SpaceEffects::harness_for_beam(weapon, 200., 0.5, 16);
        let mut frame = Frame::default();
        frame.tick = 50;
        frame.turrets.push(Aim {
            building: 12,
            previous: [0., 0.],
            current: [0., 0.],
            class: 0,
            weapon: 0,
            position: [0.; 3],
            yaw_rate: 0.,
            pitch_rate: 0.,
            alive: true,
            held: false,
            fired: false,
            firing: false,
            spinning: false,
            overheated: false,
            heat: 0.,
            heat_threshold: 0.,
        });
        // Include slight horizontal weapon spread at launch along (+0.05, 0, ~1).
        let spread_dir = normalize_or_z([0.05, 0., 1.]);
        frame.rounds.push((
            0,
            Round {
                serial: 31,
                weapon,
                barrel: Some(1),
                emitter: Some(EmittingBody::Building {
                    index: 12,
                    life: 1.,
                }),
                born: 50,
                tick: 50,
                origin: [100., 26., -32.],
                initial_velocity: spread_dir.map(|v| v * 1200.),
                position: [100., 26., -32.],
                active: false,
                contact: None,
                ..Default::default()
            },
        ));
        effects.events(&frame, &mut sink);

        // Next frame: the chair yaws 90 deg (+Z -> +X) and pitches up 0.25 rad while moving on a carrier.
        let yaw = std::f64::consts::FRAC_PI_2;
        let pitch = 0.25;
        frame.tick = 51;
        frame.rounds.clear();
        frame.turrets[0].previous = [0., 0.];
        frame.turrets[0].current = [yaw, pitch];
        let posed_basis = turret_aim_basis(IDENTITY_BASIS, yaw, pitch);
        let barrel_offset = apply_basis([-1.5, 0., 8.], posed_basis);
        let expected_origin = [105. + barrel_offset[0], 26. + barrel_offset[1], -40. + barrel_offset[2]];
        let expected_dir = apply_basis(spread_dir, posed_basis);
        let rig_muzzle = |building: usize, barrel: Option<usize>| -> Option<TurretMuzzle> {
            (building == 12 && barrel == Some(1)).then_some(TurretMuzzle {
                origin: expected_origin,
                basis: posed_basis,
                root_basis: IDENTITY_BASIS,
            })
        };
        effects.repose_beams_on(&frame, 1.0, &rig_muzzle, &mut sink);
        let draws = sink.rounds.take_beams(51.);
        assert_eq!(draws.len(), 1);
        for k in 0..3 {
            assert!((draws[0].position[k] - expected_origin[k]).abs() < 1e-6);
            assert!((draws[0].direction[k] - expected_dir[k]).abs() < 1e-6);
            assert!((draws[0].end[k] - (expected_origin[k] + expected_dir[k] * 200.)).abs() < 1e-5);
        }
        sink.rounds.return_beams(draws);

        // Without a posed rig, the Aim fallback still sweeps beam direction with turret yaw/pitch.
        let no_rig = |_: usize, _: Option<usize>| None;
        effects.repose_beams_on(&frame, 1.0, &no_rig, &mut sink);
        let draws = sink.rounds.take_beams(52.);
        assert_eq!(draws.len(), 1);
        for k in 0..3 {
            assert!((draws[0].direction[k] - expected_dir[k]).abs() < 1e-6);
        }
        sink.rounds.return_beams(draws);
    }

    #[test]
    fn repose_beams_reads_the_history_at_the_shown_time_and_follows_the_owning_hull() {
        let weapon = 0x4455_6677;
        let (mut effects, mut sink) = SpaceEffects::harness_for_beam(weapon, 120., 0.25, 8);
        let mut history = History::new(crate::local::space::Counts::default());
        let mut shooter = Hull { id: 7, generation: 3, team: 1, ..Default::default() };
        shooter.previous.position = [0., 5., 100.];
        shooter.previous.rotation = [0., 0., 0., 1.];
        shooter.current = shooter.previous;
        // Tick 30: the firing craft, at rest, fires one beam four metres ahead of its centre.
        let mut launch = Frame::default();
        launch.tick = 30;
        launch.hulls = vec![shooter];
        launch.rounds.push((0, Round {
            serial: 41,
            weapon,
            team: 1,
            emitter: Some(EmittingBody::Fighter { index: 7, generation: 3 }),
            born: 30,
            tick: 30,
            origin: [0., 5., 104.],
            initial_velocity: [0., 0., 1200.],
            position: [0., 5., 104.],
            active: false,
            contact: None,
            ..Default::default()
        }));
        effects.events(&launch, &mut sink);
        history.publish(&mut launch);
        // Tick 31: the craft has flown 6 m along x and yawed a quarter turn.
        let mut turned = Quaternion::default();
        turned.set_from_axis_angle(&Vector3::new(0., 1., 0.), std::f64::consts::FRAC_PI_2);
        let mut hull = shooter;
        hull.previous = shooter.current;
        hull.current.position = [6., 5., 100.];
        hull.current.rotation = [turned.x, turned.y, turned.z, turned.w];
        let mut next = Frame::default();
        next.tick = 31;
        next.hulls = vec![hull];
        history.publish(&mut next);
        // Halfway between the two ticks: half the flight and an eighth of a turn.
        effects.repose_beams(&history, 30.5, None, &mut sink);
        let draws = sink.rounds.take_beams(31.);
        assert_eq!(draws.len(), 1);
        let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
        let expected = [3. + 4. * s, 5., 100. + 4. * c];
        for (axis, (got, want)) in draws[0].position.iter().zip(expected).enumerate() {
            assert!((got - want).abs() < 1e-9, "axis {axis}: {:?}", draws[0].position);
        }
        assert!((draws[0].direction[0] - s).abs() < 1e-9 && (draws[0].direction[2] - c).abs() < 1e-9);
        sink.rounds.return_beams(draws);
        // A shown time before any published tick re-poses nothing and keeps the beam as it is.
        let empty = History::new(crate::local::space::Counts::default());
        effects.repose_beams(&empty, 30.5, None, &mut sink);
        assert_eq!(sink.rounds.take_beams(31.).len(), 1);
    }
}
