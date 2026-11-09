//! The page publishes game facts; this phase owns sound state and decisions.
//! Rendering stays in the separate audio module, reached through timed numeric
//! commands. Collision is borrowed from the simulation's existing map owner.

pub mod layout;
pub mod engine;
pub mod acoustics;
pub mod layers;
pub mod phase;
pub mod player;
pub mod streams;
pub mod scene;
pub mod vehicles;
pub mod feet;
pub mod foot_limits;
pub mod director;
pub mod resources;
pub mod runtime;
pub mod diagnostic;
pub mod render { include!("../../audio/src/commands.rs"); }

pub type Point = [f64; 3];

/// CollisionWorld.cast supplies a nearest hit without allocating or copying
/// map geometry into the sound phase. Dynamic body flags are published with
/// the frame facts, before any room or source query can observe them.
pub trait Collision {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit>;
}

#[derive(Clone, Copy, Default)]
pub struct Hit {
    pub distance: f64,
    pub point: Point,
    pub terrain: bool,
}

pub fn length(p: Point) -> f64 { (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() }
pub fn sub(a: Point, b: Point) -> Point { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
pub fn dot(a: Point, b: Point) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
pub fn scale(a: Point, s: f64) -> Point { [a[0] * s, a[1] * s, a[2] * s] }
pub fn cross(a: Point, b: Point) -> Point { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
pub fn normalize(a: Point) -> Point { let n = length(a); scale(a, 1.0 / if n == 0.0 { 1.0 } else { n }) }

/// Native random state belongs to the battle sound phase. A reproducible
/// seed lets frozen scenes check the order of every gate and sample draw.
pub struct Random(pub u32);
impl Random {
    // Mirrors the JS PRNG .next() method across audio-control and policy tests.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 as f64 / 4294967296.0
    }
    pub fn spread(&mut self) -> f64 { 2.0 * (self.next() - 0.5) }
}

pub mod shell_music;
pub mod shell_music_layout;
