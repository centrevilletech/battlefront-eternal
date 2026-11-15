//! The Enhanced destruction effects (destruction.js) for droids, over the
//! shared Destruction phase of bfe-environment, which decides the births,
//! the pieces' lives and their order. This host gives it what only the
//! client has (a dying droid's bones, where its chunks were, the camera,
//! collision) as commands, and applies what it answers: lights, bodies to
//! hide, pieces to draw, and its hot and smoke particles and arcs.
//!
//! A droid blown apart goes up at once: a flash, a fireball turning to
//! smoke, sparks, flakes, a dust ring, a scorch, arcs, and its pieces flying
//! from where its body parts were. One killed on its feet shorts out first,
//! arcs crawling between its joints, then blows apart the same way.
//! destructionEffects Classic leaves all of it to debris.rs and the stock
//! effects.
//!
//! The pieces draw with their material's heated twin and are given its
//! values (hot where they tore, cooling, sooted, crumbling away at the end
//! of their life); on screen the glow does not show yet. The droid's
//! blaster is thrown clear with its pieces, and a droid that shorts out is
//! heard bursting (one blown apart is heard through the world's explosion).
//!
//! Not drawn here: wrecks of power-up droids.
use super::{models::Model, replay::Collision};
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_environment::{
    fx::Fx,
    layout::{
        dst, dst_burst as burst, dst_chunk as chunk, dst_chunk_at as chunk_at, dst_command as cmd, dst_command_kind as kind, dst_event as ev,
        dst_event_kind as ek, dst_frame as frame, dst_matrix_at as matrix_at, dst_piece as piece, dst_result as res, dst_result_at as at,
        dst_slot_state as slot_state, dst_slot_static as slot_static, dst_station as station, dst_station_state as station_state,
        result as fxr,
    },
    solver::Hit,
    Destruction as Phase,
};
use bfe_render::{effect_layout as layout, effects as render};
use three::{Matrix4, Quaternion, Vector3};

const HOT: usize = 1536;
const SMOKE: usize = 1536;
const ARCS: usize = 48;
// The lights' colours, by the phase's names for them.
const COLORS: [[f64; 3]; 3] = [[0.5, 0.72, 1.0], [1.0, 0.62, 0.3], [1.0, 0.42, 0.12]];
// m/s, drifting the smoke (destruction-fx.js WIND).
const WIND: [f32; 3] = [0.55, 0., 0.32];
// pieceShape: the 26 directions its extreme points are found along.
fn directions() -> Vec<Vector3> {
    let mut list = Vec::new();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if x != 0 || y != 0 || z != 0 {
                    let mut v = Vector3::new(x as f64, y as f64, z as f64);
                    v.normalize();
                    list.push(v);
                }
            }
        }
    }
    list
}

/// A class's chunks: its piece shape, the bone it hangs from (lower case)
/// and its speed.
struct Class {
    chunks: Vec<(usize, String, f64)>,
    explosion: bool,
    // Its ExplosionName's combat definition (one-based, 0 none), for the
    // blast's sound.
    sound: usize,
}

/// The explosion a class dies in, by its class name (the definitions
/// resolve ExplosionName into the class's explosion record).
pub fn explosion_of(record: &Value) -> Option<&str> {
    record.get("explosion").and_then(|e| e.get("class")).and_then(Value::as_str).or_else(|| record.get("ExplosionName").and_then(Value::as_str))
}

/// The combat definitions (one-based) that are the explosion `name`: the
/// registry names each by where it sits, so match its own class.
pub fn explosion_ids<'a>(defs: &'a bfe_content::defs::Defs, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    defs.registry.combat.iter().enumerate().filter(move |(_, (_, def))| def.get("class").and_then(Value::as_str) == Some(name)).map(|(i, _)| i + 1)
}

/// A piece's shape, registered once with the phase, and its heated model.
fn register(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], cascades: usize, phase: &mut Phase, names: &mut Vec<String>, models: &mut Vec<Model>, name: &str) -> Result<Option<usize>, String> {
    if let Some(h) = names.iter().position(|n| n == name) {
        return Ok(Some(h));
    }
    if names.len() >= dst::SHAPES {
        return Ok(None);
    }
    let (Some(shape), Some(model)) = (shape_of(content, name), Model::new(content, bridge, textures, name, cascades)?) else { return Ok(None) };
    // Drawn heated: glowing where it tore, sooted, crumbling.
    let model = match model.heated(bridge) {
        Some(heated) => heated,
        None => {
            eprintln!("destruction: {name} has no heated program; drawn plain");
            model
        }
    };
    let handle = names.len();
    let (center, half, radius, inertia, points) = shape;
    phase.shape_points[..points.len()].copy_from_slice(&points);
    phase.register_shape(handle, center, half, radius, inertia, points.len() / 3);
    names.push(name.to_string());
    models.push(model);
    Ok(Some(handle))
}

/// A droid the phase tracks (shorting out, or arcs hanging from its bones).
#[derive(Clone, Copy)]
struct Tracked {
    unit: (u32, u32),
    class: usize,
    active: bool,
}

/// What a droid's body is now, as the host reads it each frame.
pub struct Body<'a> {
    pub unit: (u32, u32),
    pub class: usize,
    pub walker: bool,
    pub dead: bool,
    pub visible: bool,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    /// Bone names (lower case), each bone's world matrix, `stride` values
    /// apart from `at`, and each bone's parent (-1 none).
    pub names: &'a [String],
    pub parents: &'a [i32],
    pub nodes: &'a [f64],
    pub stride: usize,
    pub at: usize,
    /// The weapon in his hand: its model's name and world matrix.
    pub weapon: Option<(&'a str, [f64; 16])>,
}

pub struct Destructions {
    pub enhanced: bool,
    phase: Box<Phase>,
    classes: Vec<Option<Class>>,
    models: Vec<Model>,
    slots: [Option<Tracked>; dst::SLOTS],
    commands: usize,
    chunks: usize,
    matrices: usize,
    collision: Collision,
    world: u32,
    hot: u32,
    smoke: u32,
    arcs: u32,
    at: usize,
    arc_vertices: usize,
    arc_indices: usize,
    /// Pieces to draw: shape, world matrix, and the heated material's
    /// values (heat, soot, where it tore and how far the heat spreads,
    /// dissolve).
    pieces: Vec<(usize, [f64; 16], [f64; 7])>,
    /// Each registered shape's model name.
    names: Vec<String>,
    /// Units the phase hid this frame (gone for the rest of their life).
    pub hidden: Vec<(u32, u32)>,
    /// Lights asked for this frame: flash (true) or held, where, colour,
    /// radius, amount or duration.
    pub lights: Vec<(bool, [f64; 3], [f64; 3], f64, f64)>,
    /// Droid deaths drawn the Enhanced way so far.
    pub deaths: u64,
    /// Blasts to be heard this frame: the explosion's combat definition and
    /// where (a droid bursting after its short-out; one blown apart is heard
    /// through the world's own explosion).
    pub sounds: Vec<(usize, [f64; 3])>,
    /// The hottest piece drawn since the bench last asked (counts).
    pub peak_heat: f64,
    // The power-up droids watched for wrecking, by building id.
    stations: Vec<usize>,
}

fn shape_of(content: &BattleContent, name: &str) -> Option<([f64; 3], [f64; 3], f64, [f64; 3], Vec<f64>)> {
    let prefix = format!("{name}/");
    let mut points = Vec::new();
    for mesh in content.models.iter().filter(|m| m.name.starts_with(&prefix)) {
        for v in mesh.geometry.buffers[0].chunks_exact(32) {
            let p: [f64; 3] = std::array::from_fn(|axis| f32::from_le_bytes(v[axis * 4..axis * 4 + 4].try_into().unwrap()) as f64);
            points.push(Vector3::new(p[0], p[1], p[2]));
        }
    }
    if points.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in &points {
        lo = Vector3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vector3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let center = Vector3::new((lo.x + hi.x) / 2., (lo.y + hi.y) / 2., (lo.z + hi.z) / 2.);
    let half = Vector3::new(((hi.x - lo.x) / 2.).max(0.02), ((hi.y - lo.y) / 2.).max(0.02), ((hi.z - lo.z) / 2.).max(0.02));
    let mut extreme: Vec<Vector3> = Vec::new();
    for d in directions() {
        let best = points.iter().max_by(|a, b| {
            let da = (a.x - center.x) * d.x + (a.y - center.y) * d.y + (a.z - center.z) * d.z;
            let db = (b.x - center.x) * d.x + (b.y - center.y) * d.y + (b.z - center.z) * d.z;
            da.total_cmp(&db)
        });
        if let Some(best) = best {
            let point = Vector3::new(best.x - center.x, best.y - center.y, best.z - center.z);
            if !extreme.iter().any(|q| q.distance_to_squared(&point) < 0.0004) {
                extreme.push(point);
            }
        }
    }
    let radius = extreme.iter().map(|p| p.length()).fold(0.05, f64::max);
    let (x2, y2, z2) = (half.x * half.x, half.y * half.y, half.z * half.z);
    let inertia = [((y2 + z2) / 3.).max(0.002), ((x2 + z2) / 3.).max(0.002), ((x2 + y2) / 3.).max(0.002)];
    let flat = extreme.iter().take(dst::SHAPE_POINTS).flat_map(|p| [p.x, p.y, p.z]).collect();
    Some(([center.x, center.y, center.z], [half.x, half.y, half.z], radius, inertia, flat))
}

impl Destructions {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], collision: Collision, world: u32, at: usize) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |t| t.subsec_nanos());
        let mut phase = Box::new(Phase::new(Fx::new(HOT, SMOKE, ARCS), seed));
        let mut names: Vec<String> = Vec::new();
        let mut models = Vec::new();
        let mut classes = Vec::new();
        classes.resize_with(defs.registry.classes.len() + 1, || None);
        let cascades = bridge.shadow_cascades();
        for class in &defs.classes {
            let Some(id) = class.registry_id else { continue };
            let Some(list) = class.record.get("chunks").and_then(Value::as_array).filter(|l| !l.is_empty()) else { continue };
            let mut chunks = Vec::new();
            for c in list {
                let Some(name) = c.get("ChunkGeometryName").and_then(Value::as_str) else { continue };
                let Some(handle) = register(content, bridge, textures, cascades, &mut phase, &mut names, &mut models, name)? else { continue };
                let node = c.get("ChunkNodeName").and_then(Value::as_str).unwrap_or("").to_lowercase();
                chunks.push((handle, node, c.get("ChunkSpeed").and_then(Value::as_f64).unwrap_or(7.)));
            }
            if !chunks.is_empty() {
                // destruction.js prepare: its weapons' shapes too, to be thrown clear.
                for w in class.record.get("weapons").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(name) = w.get("GeometryName").and_then(Value::as_str) {
                        register(content, bridge, textures, cascades, &mut phase, &mut names, &mut models, name)?;
                    }
                }
                let sound = explosion_of(&class.record).and_then(|name| explosion_ids(&defs, name).next()).unwrap_or(0);
                classes[id as usize] = Some(Class { chunks, explosion: sound > 0, sound });
            }
        }
        let shadows = cascades > 0;
        let hot = bridge.effect_batch(layout::HOT as u32, 0, shadows, -1, HOT as u32, -1, &[])?;
        let smoke = bridge.effect_batch(layout::SMOKE as u32, 0, shadows, -1, SMOKE as u32, -1, &[])?;
        let arc_vertices = ARCS * bfe_environment::fx::POINTS_PER_ARC * 2;
        let arc_indices = phase.fx.arcs.indices.len();
        let arcs = bridge.effect_mesh(layout::DESTRUCTION_ARCS as u32, shadows, -1, arc_vertices as u32, arc_indices as u32, &[])?;
        let host = Self {
            enhanced: true,
            phase,
            classes,
            models,
            slots: [None; dst::SLOTS],
            commands: 0,
            chunks: 0,
            matrices: 0,
            collision,
            world,
            hot,
            smoke,
            arcs,
            at,
            arc_vertices,
            arc_indices,
            pieces: Vec::with_capacity(dst::PIECES),
            names,
            sounds: Vec::new(),
            peak_heat: 0.,
            stations: Vec::new(),
            hidden: Vec::new(),
            lights: Vec::new(),
            deaths: 0,
        };
        bridge.fx_reserve(host.end() as u32);
        Ok(host)
    }

    pub fn end(&self) -> usize {
        self.at + HOT * 16 + 1 + SMOKE * 20 + 4 + self.arc_vertices * 10 + self.arc_indices
    }

    /// Whether a class comes apart in pieces the Enhanced way.
    pub fn chunked(&self, class: usize) -> bool {
        self.enhanced && self.classes.get(class).is_some_and(Option::is_some)
    }

    fn cast(collision: &Collision, world: u32) -> impl FnMut(&Vector3, &Vector3, f64) -> Option<Hit> + '_ {
        move |o: &Vector3, d: &Vector3, far: f64| {
            let ray = bfe_sim::collision::Ray { ox: o.x, oy: o.y, oz: o.z, dx: d.x, dy: d.y, dz: d.z };
            collision.borrow_mut().ray_hit(world, ray, far, -1).map(|(_, h)| Hit { point: Vector3::new(h.px, h.py, h.pz), normal: Vector3::new(h.nx, h.ny, h.nz) })
        }
    }

    // The queued commands run now, with no time passing.
    fn flush(&mut self) {
        self.phase.frame[frame::ENHANCED] = self.enhanced as u8 as f64;
        self.phase.frame[frame::COMMANDS] = self.commands as f64;
        {
            let mut cast = Self::cast(&self.collision, self.world);
            self.phase.apply(&mut cast);
        }
        self.commands = 0;
        self.chunks = 0;
        self.matrices = 0;
        self.read_result();
    }

    fn command(&mut self, kind: f64) -> usize {
        if self.commands >= dst::COMMANDS {
            self.flush();
        }
        let i = self.commands;
        self.commands += 1;
        let c = &mut self.phase.commands[i * cmd::STRIDE..(i + 1) * cmd::STRIDE];
        c.fill(0.);
        c[cmd::KIND] = kind;
        i * cmd::STRIDE
    }

    // Each chunk's frame in the world (its bone's), written from chunk frame
    // `first`; how many.
    fn write_chunks(&mut self, body: &Body, first: usize) -> usize {
        let Some(class) = self.classes.get(body.class).and_then(Option::as_ref) else { return 0 };
        let mut n = 0;
        for (shape, node, speed) in class.chunks.iter().take(dst::UNIT_CHUNKS) {
            let Some(bone) = body.names.iter().rposition(|b| b == node) else { continue };
            let Some(m) = body.nodes.get(body.at + bone * body.stride..body.at + bone * body.stride + 16) else { continue };
            let o = (first + n) * chunk::STRIDE;
            let c = &mut self.phase.chunks[o..o + chunk::STRIDE];
            c[chunk::SHAPE] = *shape as f64;
            c[chunk::SPEED] = *speed;
            c[chunk::MATRIX..chunk::MATRIX + 16].copy_from_slice(m);
            n += 1;
        }
        n
    }

    /// game.js onBlowApart: Enhanced draws it (true), the body is hidden at
    /// once. False leaves it to the Classic pieces.
    pub fn blow_apart(&mut self, body: &Body) -> bool {
        if !self.chunked(body.class) {
            return false;
        }
        let mut slot = dst::SLOTS;
        for (i, s) in self.slots.iter_mut().enumerate() {
            if let Some(t) = s.as_mut().filter(|t| t.unit == body.unit && t.active) {
                t.active = false;
                slot = i;
            }
        }
        if self.chunks + dst::UNIT_CHUNKS > chunk_at::BURSTS || self.matrices + 1 > matrix_at::WEAPONS || self.commands >= dst::COMMANDS {
            self.flush();
        }
        let first = self.chunks;
        let count = self.write_chunks(body, first);
        self.chunks += count;
        let explosion = self.classes[body.class].as_ref().is_some_and(|c| c.explosion);
        // The droid's blaster, thrown clear: its matrix beside the commands'.
        let matrix_start = self.matrices;
        let weapon = self.weapon(body).map_or(-1., |(handle, matrix)| {
            let at = (matrix_at::COMMANDS + self.matrices) * 16;
            self.phase.matrices[at..at + 16].copy_from_slice(&matrix);
            self.matrices += 1;
            handle as f64
        });
        let c = self.command(kind::BLOW_APART);
        let c = &mut self.phase.commands[c..c + cmd::STRIDE];
        c[cmd::SLOT] = slot as f64;
        c[cmd::WALKER] = body.walker as u8 as f64;
        c[cmd::AT..cmd::AT + 3].copy_from_slice(&body.position);
        c[cmd::VELOCITY..cmd::VELOCITY + 3].copy_from_slice(&body.velocity);
        c[cmd::FRAME_START] = first as f64;
        c[cmd::FRAME_COUNT] = count as f64;
        c[cmd::MATRIX_START] = matrix_start as f64;
        c[cmd::WEAPON] = weapon;
        c[cmd::EXPLOSION] = explosion as u8 as f64;
        c[cmd::SOUND] = 0.;
        self.deaths += 1;
        true
    }

    // The shape of the weapon in a body's hand, and its world matrix.
    fn weapon(&self, body: &Body) -> Option<(usize, [f64; 16])> {
        let (name, matrix) = body.weapon?;
        Some((self.names.iter().position(|n| n == name)?, matrix))
    }

    // destruction.js skeleton: the bones arcs can crawl over (not the
    // hardpoints or dummyroot) and each one's nearest such ancestor.
    fn skeleton(body: &Body) -> (Vec<usize>, Vec<usize>) {
        let bones: Vec<usize> = (0..body.names.len()).filter(|&i| !body.names[i].starts_with("hp_") && !body.names[i].starts_with("dummyroot")).collect();
        let mut joints = Vec::new();
        for &i in &bones {
            let mut parent = body.parents.get(i).copied().unwrap_or(-1);
            while parent >= 0 && !bones.contains(&(parent as usize)) {
                parent = body.parents.get(parent as usize).copied().unwrap_or(-1);
            }
            if parent >= 0 {
                joints.push(i);
                joints.push(parent as usize);
            }
        }
        (bones, joints)
    }

    /// game.js onDeath: a chunked droid still standing shorts out, then
    /// blows apart. With every slot taken it blows apart at once.
    pub fn died(&mut self, body: &Body) {
        if !self.chunked(body.class) || !body.visible || self.slots.iter().flatten().any(|t| t.unit == body.unit && t.active) {
            return;
        }
        if self.commands >= dst::COMMANDS {
            self.flush();
        }
        let Some(slot) = self.slots.iter().position(Option::is_none) else {
            self.blow_apart(body);
            self.hidden.push(body.unit);
            return;
        };
        let (bones, joints) = Self::skeleton(body);
        let base = slot * slot_static::STRIDE;
        let s = &mut self.phase.slot_static;
        let mut n = 0;
        for &b in bones.iter().filter(|&&b| b < dst::BONES).take(dst::BONES) {
            s[base + slot_static::BONES + n] = b as f64;
            n += 1;
        }
        s[base + slot_static::BONE_COUNT] = n as f64;
        let mut j = 0;
        for pair in joints.chunks_exact(2) {
            if j + 1 >= dst::JOINT_NUMBERS || pair[0] >= dst::BONES || pair[1] >= dst::BONES {
                continue;
            }
            s[base + slot_static::JOINTS + j] = pair[0] as f64;
            s[base + slot_static::JOINTS + j + 1] = pair[1] as f64;
            j += 2;
        }
        s[base + slot_static::JOINT_COUNT] = j as f64;
        self.slots[slot] = Some(Tracked { unit: body.unit, class: body.class, active: true });
        let c = self.command(kind::DIED);
        self.phase.commands[c + cmd::SLOT] = slot as f64;
        self.phase.commands[c + cmd::WALKER] = body.walker as u8 as f64;
        self.deaths += 1;
    }

    /// A droideka's blast during its death clip: true when Enhanced drew it.
    pub fn death_blast(&mut self, body: &Body) -> bool {
        if !self.enhanced {
            return false;
        }
        let (bones, _) = Self::skeleton(body);
        let count = bones.len().min(matrix_at::WEAPONS);
        if self.matrices + count > matrix_at::WEAPONS {
            self.flush();
        }
        let first = self.matrices;
        for (k, &b) in bones.iter().take(count).enumerate() {
            let m = &body.nodes[body.at + b * body.stride..body.at + b * body.stride + 16];
            let mut t = [0.; 16];
            t[0] = 1.;
            t[5] = 1.;
            t[10] = 1.;
            t[15] = 1.;
            t[12..15].copy_from_slice(&m[12..15]);
            self.phase.matrices[(first + k) * 16..(first + k + 1) * 16].copy_from_slice(&t);
        }
        self.matrices += count;
        let c = self.command(kind::DEATH_BLAST);
        self.phase.commands[c + cmd::MATRIX_START] = first as f64;
        self.phase.commands[c + cmd::MATRIX_COUNT] = count as f64;
        true
    }

    /// Destruction.blast: the Enhanced blast at a point, scale 1, fully
    /// electric, inheriting no motion.
    pub fn blast(&mut self, at: [f64; 3]) {
        let c = self.command(kind::BLAST);
        let c = &mut self.phase.commands[c..c + cmd::STRIDE];
        c[cmd::AT..cmd::AT + 3].copy_from_slice(&at);
        c[cmd::SCALE] = 1.;
        c[cmd::ELECTRIC] = 1.;
    }

    /// The droids the phase tracks, for the frame's bodies.
    pub fn tracked(&self) -> impl Iterator<Item = (usize, (u32, u32))> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.map(|t| (i, t.unit)))
    }

    /// The frame (Destruction.update). `body(unit)` gives a tracked droid's
    /// body as drawn now, None once it is gone.
    pub fn tracked_bodies(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.slots.iter().flatten().filter(|slot| slot.active).map(|slot| slot.unit)
    }

    pub fn frame<'a>(&mut self, dt: f64, eye: [f64; 3], body: &mut dyn FnMut((u32, u32)) -> Option<Body<'a>>) {
        let dt = dt.clamp(0., 0.1);
        self.hidden.clear();
        self.lights.clear();
        let f = &mut self.phase.frame;
        f[frame::DT] = dt;
        f[frame::ENHANCED] = self.enhanced as u8 as f64;
        f[frame::HAS_EYE] = 1.;
        f[frame::EYE..frame::EYE + 3].copy_from_slice(&eye);
        f[frame::COMMANDS] = self.commands as f64;
        f[frame::STATIONS] = self.stations.len() as f64;
        let tracked: Vec<(usize, (u32, u32), bool)> = self.slots.iter().enumerate().filter_map(|(i, s)| s.map(|t| (i, t.unit, t.active))).collect();
        let mut bodies: Vec<Option<Body<'a>>> = Vec::with_capacity(tracked.len());
        for &(i, unit, active) in &tracked {
            let b = body(unit);
            let at = i * slot_state::STRIDE;
            let valid = active && b.as_ref().is_some_and(|b| b.dead && b.visible);
            self.phase.slot_state[at + slot_state::VALID] = valid as u8 as f64;
            if let Some(b) = &b {
                self.phase.slot_state[at + slot_state::POSITION..at + slot_state::POSITION + 3].copy_from_slice(&b.position);
                let n = b.names.len().min(dst::BONES);
                for k in 0..n {
                    let m = &b.nodes[b.at + k * b.stride..b.at + k * b.stride + 16];
                    self.phase.slot_matrices[(i * dst::BONES + k) * 16..(i * dst::BONES + k + 1) * 16].copy_from_slice(m);
                }
            }
            bodies.push(b);
        }
        self.phase.prepare();
        let bursts = self.phase.result[res::BURSTS] as usize;
        for k in 0..bursts {
            let slot = self.phase.result[at::BURSTS + k] as usize;
            let Some(index) = tracked.iter().position(|t| t.0 == slot) else { continue };
            let Some(b) = bodies[index].as_ref() else { continue };
            let o = slot * burst::STRIDE;
            let first = chunk_at::BURSTS + slot * dst::UNIT_CHUNKS;
            let count = self.write_chunks(b, first);
            let explosion = self.classes.get(b.class).and_then(Option::as_ref).is_some_and(|c| c.explosion);
            // Its blaster's matrix: the slot's own, beside the commands'.
            let weapon = self.weapon(b).map_or(-1., |(handle, matrix)| {
                let at = (matrix_at::WEAPONS + slot) * 16;
                self.phase.matrices[at..at + 16].copy_from_slice(&matrix);
                handle as f64
            });
            let r = &mut self.phase.burst_blocks[o..o + burst::STRIDE];
            r[burst::VELOCITY..burst::VELOCITY + 3].copy_from_slice(&b.velocity);
            r[burst::START] = first as f64;
            r[burst::COUNT] = count as f64;
            r[burst::WEAPON] = weapon;
            r[burst::EXPLOSION] = explosion as u8 as f64;
        }
        {
            let mut cast = Self::cast(&self.collision, self.world);
            self.phase.update(&mut cast);
        }
        self.commands = 0;
        self.chunks = 0;
        self.matrices = 0;
        self.read_result();
    }

    // What the phase left: events, pieces, and which slots it still holds.
    fn read_result(&mut self) {
        let r = &self.phase.result;
        for k in 0..r[res::EVENTS] as usize {
            let e = &r[at::EVENTS + k * ev::STRIDE..at::EVENTS + (k + 1) * ev::STRIDE];
            let position = [e[ev::X], e[ev::Y], e[ev::Z]];
            if e[ev::KIND] == ek::FLASH || e[ev::KIND] == ek::ADD {
                let color = COLORS[(e[ev::A] as usize).min(2)];
                self.lights.push((e[ev::KIND] == ek::FLASH, position, color, e[ev::P], e[ev::Q]));
            } else if e[ev::KIND] == ek::SOUND {
                let class = self.slots.get(e[ev::A] as usize).copied().flatten().map(|t| t.class);
                if let Some(c) = class.and_then(|c| self.classes.get(c)).and_then(Option::as_ref).filter(|c| c.sound > 0) {
                    self.sounds.push((c.sound, position));
                }
            } else if e[ev::KIND] == ek::HIDE {
                if let Some(t) = self.slots.get(e[ev::A] as usize).copied().flatten() {
                    self.hidden.push(t.unit);
                }
            }
        }
        self.pieces.clear();
        for k in 0..(r[res::PIECES] as usize).min(dst::PIECES) {
            let p = &r[at::PIECES + k * piece::STRIDE..at::PIECES + (k + 1) * piece::STRIDE];
            let shape = p[piece::SHAPE] as usize;
            if shape >= self.models.len() || p[piece::DISSOLVE] >= 1. {
                continue;
            }
            let mut m = Matrix4::default();
            m.compose(
                &Vector3::new(p[piece::POSITION], p[piece::POSITION + 1], p[piece::POSITION + 2]),
                &Quaternion::new(p[piece::QUATERNION], p[piece::QUATERNION + 1], p[piece::QUATERNION + 2], p[piece::QUATERNION + 3]),
                &Vector3::new(1., 1., 1.),
            );
            self.peak_heat = self.peak_heat.max(p[piece::HEAT]);
            let look = [p[piece::HEAT], p[piece::SOOT], p[piece::TEAR], p[piece::TEAR + 1], p[piece::TEAR + 2], p[piece::TEAR + 3], p[piece::DISSOLVE]];
            self.pieces.push((shape, m.elements, look));
        }
        for i in 0..dst::SLOTS {
            if let Some(t) = self.slots[i].as_mut() {
                t.active = r[at::ACTIVE + i] > 0.;
                if !t.active && r[at::REFS + i] == 0. {
                    self.slots[i] = None;
                }
            }
        }
    }

    /// DestructionFx: the hot and smoke pools and the arcs as draws.
    pub fn draw(&self, bridge: &mut Bridge) {
        let fx = &self.phase.fx;
        let r = &self.phase.fx_result;
        let time = r[fxr::TIME] as f32;
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        let data = &mut bridge.r.effects.data;
        let mut at = self.at;
        let mut rows: Vec<[f64; 6]> = Vec::new();
        let mut orders: Vec<(f64, f64)> = Vec::new();
        let hot = fx.hot.count.min(HOT);
        if hot > 0 {
            let start = at;
            for plane in &fx.hot.attributes {
                data[at..at + hot * 4].copy_from_slice(&plane[..hot * 4]);
                at += hot * 4;
            }
            data[at] = time;
            rows.push([render::KIND_BATCH as f64, self.hot as f64, hot as f64, 0., start as f64, HOT as f64]);
            orders.push((11., self.hot as f64));
        }
        at = self.at + HOT * 16 + 1;
        let smoke = fx.smoke.count.min(SMOKE);
        if smoke > 0 {
            let start = at;
            for plane in &fx.smoke.attributes {
                data[at..at + smoke * 4].copy_from_slice(&plane[..smoke * 4]);
                at += smoke * 4;
            }
            data[at] = time;
            data[at + 1..at + 4].copy_from_slice(&WIND);
            rows.push([render::KIND_BATCH as f64, self.smoke as f64, smoke as f64, 0., start as f64, SMOKE as f64]);
            orders.push((9., self.smoke as f64));
        }
        at = self.at + HOT * 16 + 1 + SMOKE * 20 + 4;
        let (vertices, indices) = (fx.arcs.vertex.min(self.arc_vertices), fx.arcs.index.min(self.arc_indices));
        if fx.arcs.live > 0 && vertices > 0 && indices > 0 {
            let start = at;
            data[at..at + vertices * 3].copy_from_slice(&fx.arcs.positions[..vertices * 3]);
            at += vertices * 3;
            data[at..at + vertices * 3].copy_from_slice(&fx.arcs.tangents[..vertices * 3]);
            at += vertices * 3;
            data[at..at + vertices * 4].copy_from_slice(&fx.arcs.meta[..vertices * 4]);
            at += vertices * 4;
            for (slot, index) in data[at..at + indices].iter_mut().zip(&fx.arcs.indices) {
                *slot = f32::from_bits(*index as u32);
            }
            rows.push([render::KIND_MESH as f64, self.arcs as f64, vertices as f64, indices as f64, start as f64, 0.]);
            orders.push((12., self.arcs as f64));
        }
        for (values, (order, id)) in rows.into_iter().zip(orders) {
            if draws == render::MAX_DRAWS {
                break;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&values);
            row[render::DRAW_KEY + 1] = order;
            row[render::DRAW_KEY + 5] = id;
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }

    /// destruction.js watch: the power-up droids (Buildings::station_frames)
    /// and, each frame, whether each is wrecked and how far repaired. A new
    /// set (a new battle) is watched afresh.
    pub fn stations(&mut self, frames: impl Iterator<Item = super::buildings::StationFrame>) {
        let frames: Vec<_> = frames.take(dst::STATIONS).collect();
        let fresh = frames.len() != self.stations.len() || frames.iter().zip(&self.stations).any(|(f, id)| f.id != *id);
        for (i, f) in frames.iter().enumerate() {
            let s = &mut self.phase.station_state[i * station_state::STRIDE..(i + 1) * station_state::STRIDE];
            s[station_state::DESTROYED] = f.destroyed as u8 as f64;
            s[station_state::HEALTH] = f.health;
            s[station_state::MAX_HEALTH] = f.max_health;
            if fresh {
                let c = &mut self.phase.station_config[i * station::STRIDE..(i + 1) * station::STRIDE];
                c[station::MIN..station::MIN + 3].copy_from_slice(&f.bounds[0]);
                c[station::MAX..station::MAX + 3].copy_from_slice(&f.bounds[1]);
                c[station::CENTER..station::CENTER + 3].copy_from_slice(&f.center);
            }
        }
        if fresh {
            self.stations = frames.iter().map(|f| f.id).collect();
            self.phase.watch(frames.len());
        }
    }

    /// Classic now: everything gone at once.
    pub fn clear(&mut self) {
        self.command(kind::CLEAR);
        self.flush();
    }

    pub fn record_budget(&self) -> usize {
        self.pieces.iter().map(|(shape, _, _)| self.models[*shape].rows.len()).sum()
    }

    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        use bfe_render::instances as draw;
        for (shape, matrix, look) in &self.pieces {
            let first = records;
            (records, bones) = self.models[*shape].append(bridge, matrix, records, bones);
            // shader.rs EXTRAS: heat, heatBase, heatScale, soot; heatTear; dissolve.
            for r in first..records {
                let e = draw::IN_RECORDS + r * draw::REC_STRIDE + draw::REC_EXTRA;
                let row = &mut bridge.r.core.input[e..e + 13];
                row[4..8].copy_from_slice(&[look[0], 0., 1., look[1]]);
                row[8..12].copy_from_slice(&look[2..6]);
                row[12] = look[6];
            }
        }
        (records, bones)
    }

    /// For the bench's counts: pieces, hot and smoke particles, arcs.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (self.pieces.len(), self.phase.fx.hot.count, self.phase.fx.smoke.count, self.phase.fx.arcs.live)
    }
}
