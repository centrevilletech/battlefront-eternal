//! Value records shared by native replay and the module command adapter.
use super::debris::{Bounds, Chunk, Debris, Launch, Piece, Settle, World};
use crate::gen::{DbChunk, DbInput, DbLaunch, DbOperation, DbPiece, DbSettle, DbState};
use three::{Matrix4, Quaternion, Vector3};

pub(super) fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
pub(super) fn a3(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
pub(super) fn q4(v: [f64; 4]) -> Quaternion { Quaternion::new(v[0], v[1], v[2], v[3]) }
pub(super) fn a4(v: Quaternion) -> [f64; 4] { [v.x, v.y, v.z, v.w] }
impl From<DbChunk> for Chunk {
    fn from(v: DbChunk) -> Self { Self { speed: v.speed, omega: v3(v.omega), still: v.still,
        terrain_collisions: v.terrain_collisions, terrain_effect: v.terrain_effect, trail: v.trail, smoke: v.smoke } }
}
impl From<&Chunk> for DbChunk {
    fn from(v: &Chunk) -> Self { Self { speed: v.speed, omega: a3(v.omega), still: v.still,
        terrain_collisions: v.terrain_collisions, terrain_effect: v.terrain_effect, trail: v.trail, smoke: v.smoke } }
}
impl From<DbPiece> for Piece {
    fn from(v: DbPiece) -> Self { Self { id: v.id as u32, chunk: v.chunk.into(), position: v3(v.position), previous: v3(v.previous),
        velocity: v3(v.velocity), omega: v3(v.omega), quaternion: q4(v.quaternion), hits: v.hits, lift: v.lift, thin: v.thin as usize,
        settle: v.settle.map(|s| Settle { from: q4(s.from), to: q4(s.to), y0: s.y0, y1: s.y1, t: s.t }), ignore: v.ignore.map(|n| n as u32),
        resting: v.resting, flight: v.flight, left: v.left, trail: v.trail.map(|n| n as u32), smoke: v.smoke.map(|n| n as u32), gone: v.gone } }
}
impl From<&Piece> for DbPiece {
    fn from(v: &Piece) -> Self { Self { id: v.id as f64, chunk: (&v.chunk).into(), position: a3(v.position), previous: a3(v.previous),
        velocity: a3(v.velocity), omega: a3(v.omega), quaternion: a4(v.quaternion), hits: v.hits, lift: v.lift, thin: v.thin as f64,
        settle: v.settle.as_ref().map(|s| DbSettle { from: a4(s.from), to: a4(s.to), y0: s.y0, y1: s.y1, t: s.t }), ignore: v.ignore.map(f64::from),
        resting: v.resting, flight: v.flight, left: v.left, trail: v.trail.map(f64::from), smoke: v.smoke.map(f64::from), gone: v.gone } }
}
impl From<DbState> for Debris {
    fn from(v: DbState) -> Self { Self { pieces: v.pieces.into_iter().map(Into::into).collect(), ray: v3(v.ray), start: v3(v.start), out: v3(v.out) } }
}
impl From<&Debris> for DbState {
    fn from(v: &Debris) -> Self { Self { pieces: v.pieces.iter().map(Into::into).collect(), ray: a3(v.ray), start: a3(v.start), out: a3(v.out) } }
}
impl From<DbLaunch> for Launch {
    fn from(v: DbLaunch) -> Self {
        let mut matrix = Matrix4::default(); matrix.from_array(&v.matrix, 0);
        Self { id: v.id as u32, chunk: v.chunk.into(), matrix, bounds: Bounds { min: v3(v.low), max: v3(v.high) }, origin: v3(v.origin),
            velocity: v3(v.velocity), health: v.health, ignore: v.ignore.map(|n| n as u32), center: v.center, floor: v.floor }
    }
}
pub fn run(input: DbInput, world: &mut impl World) -> DbState {
    let mut debris: Debris = input.state.into();
    match input.op {
        DbOperation::Launch => debris.launch(input.launch.expect("debris launch").into(), world),
        DbOperation::Update => debris.update(input.dt, world),
        DbOperation::Bounce => debris.bounce(input.id as u32, world),
        DbOperation::Rest => debris.rest(input.id as u32, input.floor, world),
        DbOperation::Clear => debris.clear(world),
    }
    (&debris).into()
}
