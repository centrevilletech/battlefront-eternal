//! A droid's pieces once it blows apart (src/debris.js). The flight, bounce
//! and rest rules are the shared port in bfe_sim::battle::debris; this owner
//! is its page: the pieces' models, the collision they bounce on, and the
//! trail and impact effects they play. Nothing here deals damage or reaches
//! the server; a piece is drawn until ten seconds after it rests.
use super::{effects::Spawner, models::Model, replay::Collision};
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_sim::{
    battle::debris::{self as flight, Chunk, Hit, Launch, Piece},
    store::StrId,
};
use three::{Matrix4, Quaternion, Vector3};
// Pieces in flight or at rest at once; a droid has about ten.
const PIECES: usize = 256;
/// One CHUNKSECTION of a class: its model, the bone it starts at (lower
/// case, as Debris.blowApart matches node names) and its flight numbers.
pub struct Part {
    model: usize,
    pub node: String,
    chunk: Chunk,
}
pub struct Debris {
    // One model per chunk geometry, shared by every class that names it.
    models: Vec<Model>,
    // Each class's parts, by the class's registry id.
    classes: Vec<Vec<Part>>,
    // The shared flight: every piece's position, turn, velocity and rest.
    flight: flight::Debris,
    // Each piece's id and model, in the pieces' own order.
    shown: Vec<(u32, usize)>,
    // Launches asked for since the last frame, with their models. They wait
    // for the frame's effect batch, which their trails start in.
    queued: Vec<(usize, Launch)>,
    // The last piece id given out; ids only tell pieces apart.
    serial: u32,
    collision: Collision,
    world: u32,
}
// The shared flight's page: Debris's scene, collision and effects.
struct Page<'a> {
    shown: &'a mut Vec<(u32, usize)>,
    model: usize,
    collision: &'a Collision,
    world: u32,
    effects: &'a mut Spawner,
    played: &'a mut u64,
    refused: &'a mut u64,
}
fn array(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
impl flight::World for Page<'_> {
    fn add(&mut self, id: u32, _: &Vector3, _: &Quaternion, _: Option<&Vector3>) {
        self.shown.push((id, self.model));
    }
    // Pieces rest and leave in any order; both lists keep theirs.
    fn remove(&mut self, piece: &Piece) {
        self.shown.retain(|(id, _)| *id != piece.id);
    }
    // Debris.bounce casts against the world's collision. A droid's pieces
    // pass through nothing (only a wrecked turret's are told what to ignore).
    fn cast(&mut self, start: &Vector3, ray: &Vector3, far: f64, _: Option<u32>) -> Option<Hit> {
        let ray = bfe_sim::collision::Ray { ox: start.x, oy: start.y, oz: start.z, dx: ray.x, dy: ray.y, dz: ray.z };
        self.collision
            .borrow_mut()
            .ray_hit(self.world, ray, far, -1)
            .map(|(_, h)| Hit { point: Vector3::new(h.px, h.py, h.pz), normal: Vector3::new(h.nx, h.ny, h.nz) })
    }
    // Only a hull's piece, carried by its middle, settles onto a side; a
    // droid's has no lift and never asks.
    fn low(&mut self, _: &Piece, _: &Quaternion) -> f64 {
        0.
    }
    // Effects.play for a trail (it has a velocity and is moved later), a
    // smoke or a bounce's impact. The effect group travels in the name's
    // place; the piece's id seeds its particles.
    fn play(&mut self, piece: &Piece, name: StrId, point: &Vector3, velocity: Option<&Vector3>, _: bool) -> Option<u32> {
        let handle = self.effects.start(name.0 as usize, array(point), velocity.map_or([0.; 3], array), piece.id);
        *if handle.is_some() { &mut *self.played } else { &mut *self.refused } += 1;
        handle
    }
    // Effect.move: a trail takes the piece's velocity, a smoke keeps its own.
    fn move_effect(&mut self, piece: &Piece, effect: u32, velocity: bool) {
        self.effects.sync(effect, array(&piece.position), velocity.then(|| array(&piece.velocity)));
    }
    fn stop_effect(&mut self, effect: u32) {
        self.effects.stop(effect);
    }
}
impl Debris {
    // Each class's chunks, by its registry id. A chunk whose model the
    // content lacks is left out, as Debris.blowApart skips it; an effect
    // the library lacks plays nothing.
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        effects: &[(String, usize)],
        collision: Collision,
        world: u32,
    ) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let effect = |chunk: &Value, key: &str| {
            let name = chunk.get(key).and_then(Value::as_str).filter(|name| !name.is_empty())?;
            effects.iter().find(|(n, _)| n == name).map(|(_, id)| StrId(*id as u32))
        };
        let number = |chunk: &Value, key: &str| chunk.get(key).and_then(Value::as_f64).unwrap_or(0.);
        let mut names: Vec<String> = Vec::new();
        let mut models = Vec::new();
        let mut classes = Vec::new();
        classes.resize_with(defs.registry.classes.len() + 1, Vec::new);
        for class in &defs.classes {
            let Some(id) = class.registry_id else {
                continue;
            };
            for chunk in class.record.get("chunks").and_then(Value::as_array).unwrap_or(&[]) {
                // The piece's model, made once however many classes use it.
                let name = chunk.get("ChunkGeometryName").and_then(Value::as_str).unwrap_or("");
                let model = match names.iter().position(|known| known == name) {
                    Some(model) => model,
                    None => {
                        let Some(model) = Model::new(content, bridge, textures, name, crate::shadows::COUNT)? else {
                            continue;
                        };
                        names.push(name.into());
                        models.push(model);
                        models.len() - 1
                    }
                };
                // ChunkOmega: its spin about each axis; ChunkPhysics STATIC
                // keeps a piece where it starts; ChunkTerrainCollisions is
                // how many bounces kick up ChunkTerrainEffect before it rests.
                let omega = chunk.get("ChunkOmega").and_then(Value::as_array);
                let spin = |axis: usize| omega.and_then(|o| o.get(axis)).and_then(Value::as_f64).unwrap_or(0.);
                classes[id as usize].push(Part {
                    model,
                    node: chunk.get("ChunkNodeName").and_then(Value::as_str).unwrap_or("").to_lowercase(),
                    chunk: Chunk {
                        speed: number(chunk, "ChunkSpeed"),
                        omega: Vector3::new(spin(0), spin(1), spin(2)),
                        still: chunk.get("ChunkPhysics").and_then(Value::as_str).is_some_and(|p| p.eq_ignore_ascii_case("STATIC")),
                        terrain_collisions: number(chunk, "ChunkTerrainCollisions"),
                        terrain_effect: effect(chunk, "ChunkTerrainEffect"),
                        trail: effect(chunk, "ChunkTrailEffect"),
                        smoke: effect(chunk, "ChunkSmokeEffect"),
                    },
                });
            }
        }
        Ok(Self {
            models,
            classes,
            flight: flight::Debris { pieces: Vec::with_capacity(PIECES), ..Default::default() },
            shown: Vec::with_capacity(PIECES),
            queued: Vec::with_capacity(PIECES),
            serial: 0,
            collision,
            world,
        })
    }
    pub fn parts(&self, class: usize) -> &[Part] {
        self.classes.get(class).map_or(&[], Vec::as_slice)
    }
    /// Debris.blowApart for one chunk of a class: it starts where its bone
    /// is (`matrix`, the bone's world matrix) and flies out from the body's
    /// origin. It joins the flight with the next frame's effect batch, which
    /// its trail needs.
    pub fn blow_apart(&mut self, class: usize, part: usize, matrix: Matrix4, origin: [f64; 3], velocity: [f64; 3], health: f64) -> bool {
        let Some(part) = self.classes.get(class).and_then(|parts| parts.get(part)) else {
            return false;
        };
        if self.flight.pieces.len() + self.queued.len() >= PIECES {
            return false;
        }
        self.serial = self.serial.wrapping_add(1);
        // A droid's piece is carried by its bone, not its middle: no bounds,
        // no floor, nothing to pass through.
        let nothing = Vector3::default();
        self.queued.push((
            part.model,
            Launch {
                id: self.serial,
                chunk: part.chunk.clone(),
                matrix,
                bounds: flight::Bounds { min: nothing, max: nothing },
                origin: Vector3::new(origin[0], origin[1], origin[2]),
                velocity: Vector3::new(velocity[0], velocity[1], velocity[2]),
                health,
                ignore: None,
                center: false,
                floor: f64::NEG_INFINITY,
            },
        ));
        true
    }
    /// Debris.update, after the launches asked for since the last frame.
    pub fn step(&mut self, dt: f64, effects: &mut Spawner, played: &mut u64, refused: &mut u64) {
        let mut page = Page { shown: &mut self.shown, model: 0, collision: &self.collision, world: self.world, effects, played, refused };
        for (model, launch) in self.queued.drain(..) {
            page.model = model;
            self.flight.launch(launch, &mut page);
        }
        self.flight.update(dt, &mut page);
    }
    // Another battle's pieces never cross into this one.
    pub fn clear(&mut self) {
        self.flight.pieces.clear();
        self.shown.clear();
        self.queued.clear();
    }
    pub fn record_budget(&self) -> usize {
        self.shown.iter().map(|(_, model)| self.models[*model].rows.len()).sum()
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        // Debris.blowApart sets every piece's scale to one, whatever its bone's.
        let unit = Vector3::new(1., 1., 1.);
        for (piece, (_, model)) in self.flight.pieces.iter().zip(&self.shown) {
            let mut matrix = Matrix4::default();
            matrix.compose(&piece.position, &piece.quaternion, &unit);
            (records, bones) = self.models[*model].append(bridge, &matrix.elements, records, bones);
        }
        (records, bones)
    }
}
