//! Classic debris launch, flight, bounce, settling, and lifetime
//! (src/debris.js). Model creation, named-bone lookup, bounds queries,
//! scene membership, and particle effects remain world callbacks.
//! Class numbers and effect-name handles are cooked by the content host.

use crate::store::StrId;
use three::{Matrix4, Quaternion, Vector3};

const GRAVITY: f64 = -9.8;
const RESTITUTION: f64 = 0.37;
const FRICTION: f64 = 1.5755;
const SETTLE: f64 = 0.35;
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);
const AXES: [Vector3; 3] = [Vector3::new(1.0, 0.0, 0.0), UP, Vector3::new(0.0, 0.0, 1.0)];

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub speed: f64,
    pub omega: Vector3,
    pub still: bool,
    pub terrain_collisions: f64,
    pub terrain_effect: Option<StrId>,
    /// A nonempty string; unlike smoke and impact, an empty trail is off.
    pub trail: Option<StrId>,
    pub smoke: Option<StrId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settle {
    pub from: Quaternion,
    pub to: Quaternion,
    pub y0: f64,
    pub y1: f64,
    pub t: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub id: u32,
    pub chunk: Chunk,
    pub position: Vector3,
    pub previous: Vector3,
    pub velocity: Vector3,
    pub omega: Vector3,
    pub quaternion: Quaternion,
    pub hits: f64,
    pub lift: f64,
    pub thin: usize,
    pub settle: Option<Settle>,
    pub ignore: Option<u32>,
    pub resting: bool,
    pub flight: f64,
    pub left: f64,
    pub trail: Option<u32>,
    pub smoke: Option<u32>,
    pub gone: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min: Vector3,
    pub max: Vector3,
}
impl Bounds {
    pub fn empty(&self) -> bool { self.max.x < self.min.x || self.max.y < self.min.y || self.max.z < self.min.z }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit { pub point: Vector3, pub normal: Vector3 }

/// Queries receive the complete native transform that their geometry
/// needs. Effects and scene calls receive only their actual arguments;
/// none supplies an afterstate for the piece's physics.
pub trait World {
    fn add(&mut self, id: u32, position: &Vector3, turn: &Quaternion, center_shift: Option<&Vector3>);
    fn publish(&mut self, _piece: &Piece) {}
    fn out(&mut self, _out: &Vector3) {}
    fn vectors(&mut self, _ray: &Vector3, _start: &Vector3) {}
    fn remove(&mut self, piece: &Piece);
    fn cast(&mut self, start: &Vector3, ray: &Vector3, far: f64, ignore: Option<u32>) -> Option<Hit>;
    fn low(&mut self, piece: &Piece, turned: &Quaternion) -> f64;
    fn play(&mut self, piece: &Piece, name: StrId, point: &Vector3, velocity: Option<&Vector3>, impact: bool) -> Option<u32>;
    fn move_effect(&mut self, piece: &Piece, effect: u32, velocity: bool);
    fn stop_effect(&mut self, effect: u32);
}

/// Inputs after the host has found the named model and bone. Bounds are
/// the newly made mesh's world bounds before it is recentered or placed.
pub struct Launch {
    pub id: u32,
    pub chunk: Chunk,
    pub matrix: Matrix4,
    pub bounds: Bounds,
    pub origin: Vector3,
    pub velocity: Vector3,
    pub health: f64,
    pub ignore: Option<u32>,
    pub center: bool,
    pub floor: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Debris {
    pub pieces: Vec<Piece>,
    pub ray: Vector3,
    pub start: Vector3,
    pub out: Vector3,
}

impl Debris {
    pub fn launch(&mut self, input: Launch, world: &mut impl World) {
        let factor = if input.health <= -50.0 { 1.0 } else { v8math::min(0.1 - (0.9 * input.health) / 50.0, 1.0) };
        let (mut lift, mut thin) = (0.0, 1);
        let (mut size, mut middle) = (Vector3::default(), Vector3::default());
        if input.center && !input.bounds.empty() {
            middle.add_vectors(&input.bounds.min, &input.bounds.max).multiply_scalar(0.5);
            size.sub_vectors(&input.bounds.max, &input.bounds.min);
            lift = v8math::min(v8math::min(size.x, size.y), size.z) / 2.0;
            thin = if size.x <= size.y && size.x <= size.z { 0 } else if size.y <= size.z { 1 } else { 2 };
        }
        let shift = (input.center && !input.bounds.empty()).then_some(middle);
        let (mut position, mut quaternion, mut scale) = (Vector3::default(), Quaternion::default(), Vector3::default());
        input.matrix.decompose(&mut position, &mut quaternion, &mut scale);
        if input.center {
            position.copy(&middle.apply_matrix4(&input.matrix));
            position.y = v8math::max(position.y, input.floor + size.y / 2.0);
        }
        world.add(input.id, &position, &quaternion, shift.as_ref());
        self.out.sub_vectors(&position, &input.origin);
        self.out.y += 0.25 * self.out.length();
        if self.out.length_sq() < 1e-8 { self.out.set(0.0, 1.0, 0.0); }
        let velocity = if input.chunk.still { Vector3::default() } else {
            self.out.normalize().multiply_scalar(input.chunk.speed).add(&input.velocity).multiply_scalar(factor);
            self.out
        };
        let mut piece = Piece { id: input.id, omega: input.chunk.omega, resting: input.chunk.still,
            left: if input.chunk.still { f64::INFINITY } else { 10.0 }, chunk: input.chunk,
            position, previous: position, velocity, quaternion, hits: 0.0, lift, thin, settle: None,
            ignore: input.ignore, flight: 0.0, trail: None, smoke: None, gone: None };
        world.out(&self.out);
        if let Some(name) = piece.chunk.trail { piece.trail = world.play(&piece, name, &piece.position, Some(&velocity), false); }
        if let Some(name) = piece.chunk.smoke { piece.smoke = world.play(&piece, name, &piece.position, None, false); }
        self.pieces.push(piece);
    }

    pub fn update(&mut self, dt: f64, world: &mut impl World) {
        for piece in &mut self.pieces {
            if piece.resting {
                piece.left -= dt;
                if let Some(s) = &mut piece.settle {
                    if s.t < SETTLE {
                        s.t = v8math::min(s.t + dt, SETTLE);
                        let w = s.t / SETTLE;
                        let ease = w * w * (3.0 - 2.0 * w);
                        piece.quaternion.slerp_quaternions(&s.from, &s.to, ease);
                        piece.position.y = s.y0 + (s.y1 - s.y0) * ease;
                        if let Some(effect) = piece.smoke { world.publish(piece); world.move_effect(piece, effect, false); }
                    }
                }
                world.publish(piece);
                continue;
            }
            piece.flight += dt;
            piece.velocity.y += GRAVITY * dt;
            piece.previous = piece.position;
            piece.position.add_scaled_vector(&piece.velocity, dt);
            for (axis, omega) in AXES.iter().zip([piece.omega.x, piece.omega.y, piece.omega.z]) {
                let mut q = Quaternion::default(); q.set_from_axis_angle(axis, omega * dt);
                piece.quaternion.multiply(&q);
            }
            bounce(&mut self.ray, &mut self.start, piece, world);
            world.publish(piece);
            if let Some(effect) = piece.trail { world.move_effect(piece, effect, true); }
            if let Some(effect) = piece.smoke { world.move_effect(piece, effect, false); }
            if piece.flight > 20.0 { rest(piece, f64::NEG_INFINITY, world); }
            world.publish(piece);
        }
        for piece in &mut self.pieces {
            if piece.left > 0.0 { continue; }
            world.remove(piece);
            if let Some(effect) = piece.trail { world.stop_effect(effect); }
            if let Some(effect) = piece.smoke { world.stop_effect(effect); }
            piece.gone = Some(true);
            world.publish(piece);
        }
        self.pieces.retain(|p| p.gone != Some(true));
    }

    pub fn bounce(&mut self, id: u32, world: &mut impl World) {
        let piece = self.pieces.iter_mut().find(|p| p.id == id).expect("debris piece");
        bounce(&mut self.ray, &mut self.start, piece, world);
    }
    pub fn rest(&mut self, id: u32, floor: f64, world: &mut impl World) {
        rest(self.pieces.iter_mut().find(|p| p.id == id).expect("debris piece"), floor, world);
    }
    pub fn clear(&mut self, world: &mut impl World) {
        for piece in &self.pieces {
            world.remove(piece);
            if let Some(effect) = piece.trail { world.stop_effect(effect); }
            if let Some(effect) = piece.smoke { world.stop_effect(effect); }
        }
        self.pieces.clear();
    }
}

fn bounce(ray: &mut Vector3, start: &mut Vector3, p: &mut Piece, world: &mut impl World) {
    ray.sub_vectors(&p.position, &p.previous);
    let length = ray.length(); if length < 1e-6 { return; }
    ray.divide_scalar(length);
    start.copy(&p.previous).add_scaled_vector(ray, -0.05);
    world.publish(p);
    world.vectors(ray, start);
    let Some(hit) = world.cast(start, ray, length + 0.1 + p.lift, p.ignore) else { return };
    let d = p.velocity.dot(&hit.normal); if d >= 0.0 { return; }
    p.position.copy(&hit.point).add_scaled_vector(&hit.normal, v8math::max(0.05, p.lift));
    world.publish(p);
    let most = p.chunk.terrain_collisions;
    if p.hits < most { if let Some(name) = p.chunk.terrain_effect { world.play(p, name, &hit.point, None, true); } }
    p.hits += 1.0;
    p.velocity.add_scaled_vector(&hit.normal, -d);
    let along = p.velocity.length();
    if along > 0.0 { p.velocity.multiply_scalar(v8math::max(along - v8math::min(FRICTION * -d, along), 0.0) / along); }
    p.velocity.add_scaled_vector(&hit.normal, RESTITUTION * -d);
    if p.hits >= most || p.velocity.length() < 1.0 { rest(p, hit.point.y, world); }
}

fn rest(p: &mut Piece, floor: f64, world: &mut impl World) {
    if p.lift > 0.0 && floor.is_finite() {
        let mut axis = AXES[p.thin]; axis.apply_quaternion(&p.quaternion);
        let mut target = Quaternion::default(); target.set_from_unit_vectors(&axis, if axis.y < 0.0 { &DOWN } else { &UP }).multiply(&p.quaternion);
        let low = world.low(p, &target);
        p.settle = Some(Settle { from: p.quaternion, to: target, y0: p.position.y, y1: p.position.y + floor - 0.15 - low, t: 0.0 });
    }
    p.resting = true; p.velocity.set(0.0, 0.0, 0.0);
    world.publish(p);
    if let Some(effect) = p.trail { world.stop_effect(effect); }
    p.trail = None;
}
