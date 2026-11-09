//! The Enhanced destruction effects (src/destruction.js): how droids die and
//! how wrecked power-up droids smoulder. Everything here is presentation: the
//! simulation decides whether a droid blows apart or plays a death clip; this
//! only reads what it is told and draws.
//!
//! A droid dies one of two ways on screen: blown apart at once (a blast where
//! its pieces are, its pieces flying from where its body parts were), or, killed
//! on its feet, it shorts out first (arcs crawl between its joints and sparks
//! spit from them while it starts to fall), then blows apart the same way. A
//! droideka shorts out for its whole death clip. A power-up droid destroyed
//! goes up in a blast of its own and then smoulders as a wreck.
//!
//! The page tells the phase what happened as commands, with the numbers the
//! phase cannot get itself (a body's bone matrices, where its chunks were);
//! the phase decides the births, the pieces' lives and the order of it all,
//! and answers with events (a light, a sound, a unit to hide, a piece to
//! show) and records.

use crate::fx::{Anchor, Fx, Smoke};
use crate::layout::{
    dst, dst_burst as burst_at, dst_chunk as dc, dst_chunk_at as ca, dst_command as cmd, dst_command_kind as kind, dst_event as ev, dst_event_kind as ek, dst_frame as df,
    dst_matrix_at as ma, dst_piece as dp, dst_result as dr, dst_result_at as ra, dst_site as ds, dst_slot_state as sl, dst_slot_static as sst, dst_station as dstn, dst_station_state as dss,
};
use crate::random::Random;
use crate::solver::{Cast, Piece, Shape, Solver};
use jsrt::conv::{js_max, js_min};
use three::{Matrix4, Quaternion, Vector3};

const SHORT_OUT: f64 = 0.42; // s a droid killed on its feet crackles before it blows
const CHARGE_TIME: f64 = 0.9; // s for a piece's leftover charge to fall by e
const SITE_LIFE: f64 = 180.0; // s a wreck smoulders unrepaired
const SITE_REACH: f64 = 160.0; // m: wrecks farther from the camera emit nothing
const NO_FLOOR: f64 = -1e4; // the height of "no floor": nothing skips off it
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);
// The styles of the hot and smoke pools (destruction-fx.js).
const SPARK: f64 = 0.0;
const FIRE: f64 = 1.0;
const FLASH: f64 = 2.0;
const GLOW: f64 = 3.0;
const EMBER: f64 = 4.0;
const SMOKE: f64 = 0.0;
const CHIP: f64 = 1.0;
const SCORCH: f64 = 2.0;
// Light colours, as the page names them.
pub const ELECTRIC: f64 = 0.0;
pub const FIRELIGHT: f64 = 1.0;
pub const EMBERLIGHT: f64 = 2.0;
// A piece's life (destruction-debris.js).
const LINGER: f64 = 22.0; // s at rest before a piece crumbles away
const CRUMBLE: f64 = 1.1; // s to crumble
const MAX_FLIGHT: f64 = 14.0; // s; a piece still moving then crumbles anyway
const COOL: f64 = 2.6; // s for the heat to fall by e
const LIGHT_EVERY: f64 = 0.25; // s between light samples while moving
const LOST: f64 = 30.0; // m below where it started: fallen out of the world
const SHADOW_REACH: f64 = 45.0; // m from the eye past which a piece casts no shadow

/// A droid shorting out (a deaths entry).
struct Death {
    slot: usize,
    t: f64,
    length: f64,
    arcs: f64,
    sparks: f64,
    flashes: f64,
    smoke: f64,
    walker: bool,
}

/// A smouldering wreck.
struct Site {
    station: usize,
    t: f64,
    lo: Vector3,
    hi: Vector3,
    centre: Vector3,
    top: Vector3,
    hot: [Vector3; 3],
    size: Vector3,
    smoke: f64,
    flame: f64,
    ember: f64,
    crackle: f64,
    heat: f64,
    soot: f64,
}

struct Station {
    lo: Vector3,
    hi: Vector3,
    center: Vector3,
    destroyed: bool,
    health: f64,
    max_health: f64,
}

/// A piece on the field: its body and its look (a pieces entry).
struct Live {
    uid: u32,
    shape: usize,
    age: f64,
    rest: f64,
    heat: f64,
    crumble: f64,
    light_clock: f64,
    shadows: bool,
    charge: f64,
    trail: f64,
    shed: f64,
    crackle: f64,
    wisp: f64,
    dissolve: f64,
    soot: f64,
    tear: [f64; 4],
    /// Where the page should sample the map's light, asked for this call.
    sample: Option<Vector3>,
    object_position: Vector3,
    matrix: [f64; 16],
    moved: bool,
}

pub struct Destruction {
    pub fx: Fx,
    pub random: Random,
    shapes: Vec<Shape>,
    known: Vec<bool>,
    solver: Solver,
    pieces: Vec<Live>,
    bodies: Vec<Piece>,
    uid: u32,
    deaths: Vec<Death>,
    sites: Vec<Site>,
    stations: Vec<Station>,
    blasts: u32,
    enhanced: bool,
    eye: Vector3,
    has_eye: bool,
    events: usize,
    overflow: u32,
    pub commands: Vec<f64>,
    pub chunks: Vec<f64>,
    pub matrices: Vec<f64>,
    pub frame: Vec<f64>,
    pub station_config: Vec<f64>,
    pub station_state: Vec<f64>,
    /// Per slot of a droid the page tracks: its bones and joints (static, written
    /// when it dies), then, every frame, whether it still stands in the way of its
    /// death, where it is, and its bones' world matrices.
    pub slot_static: Vec<f64>,
    pub slot_state: Vec<f64>,
    pub slot_matrices: Vec<f64>,
    /// For the slots about to burst: velocity (3), first chunk frame, chunk
    /// frames, held weapon's shape (-1 for none), its matrix, explosion (0/1).
    pub burst_blocks: Vec<f64>,
    /// The extreme points of the shape being registered, three numbers each.
    pub shape_points: Vec<f64>,
    pub result: Vec<f64>,
    pub fx_result: Vec<f64>,
}

const CAPACITY: usize = dst::PIECES;
const SITE_RECORD: usize = ds::STRIDE;

fn matrix_of(elements: &[f64]) -> Matrix4 {
    let mut m = Matrix4::default();
    m.elements.copy_from_slice(&elements[..16]);
    m
}

impl Destruction {
    // Everything is allocated here, at the sizes the content needs (the layout
    // lists them: the most any map has of models, bones, droids and wrecks is
    // below every table), so no frame allocates and the page's views of the
    // module's memory, taken once, never go stale.
    pub fn new(fx: Fx, seed: u32) -> Destruction {
        let empty = || Shape { center: Vector3::default(), half: Vector3::default(), points: Vec::with_capacity(dst::SHAPE_POINTS), radius: 0.0, inverse_inertia: Vector3::default() };
        Destruction {
            fx,
            random: Random::new(seed),
            shapes: (0..dst::SHAPES).map(|_| empty()).collect(),
            known: vec![false; dst::SHAPES],
            solver: Solver::new(),
            pieces: Vec::with_capacity(CAPACITY + 1),
            bodies: Vec::with_capacity(CAPACITY + 1),
            uid: 0,
            deaths: Vec::with_capacity(dst::SLOTS),
            sites: Vec::with_capacity(dst::SITES),
            stations: Vec::with_capacity(dst::STATIONS),
            blasts: 0,
            enhanced: false,
            eye: Vector3::default(),
            has_eye: false,
            events: 0,
            overflow: 0,
            commands: vec![0.0; dst::COMMANDS * cmd::STRIDE],
            chunks: vec![0.0; ca::LEN * dc::STRIDE],
            matrices: vec![0.0; ma::LEN * 16],
            frame: vec![0.0; df::STRIDE],
            station_config: vec![0.0; dst::STATIONS * dstn::STRIDE],
            station_state: vec![0.0; dst::STATIONS * dss::STRIDE],
            slot_static: vec![0.0; dst::SLOTS * sst::STRIDE],
            slot_state: vec![0.0; dst::SLOTS * sl::STRIDE],
            slot_matrices: vec![0.0; dst::SLOTS * dst::BONES * 16],
            burst_blocks: vec![0.0; dst::SLOTS * burst_at::STRIDE],
            shape_points: vec![0.0; dst::SHAPE_POINTS * 3],
            result: vec![0.0; ra::LEN],
            fx_result: vec![0.0; crate::layout::result::STRIDE],
        }
    }

    /// A model's shape as the page worked it out from its geometry (pieceShape);
    /// its extreme points are in `shape_points`.
    pub fn register_shape(&mut self, handle: usize, center: [f64; 3], half: [f64; 3], radius: f64, inertia: [f64; 3], n: usize) {
        // A handle past the table is a model the page has no room for: it throws
        // no pieces (the page treats its shape as absent).
        if handle >= dst::SHAPES {
            return;
        }
        let n = n.min(dst::SHAPE_POINTS);
        let s = &mut self.shapes[handle];
        s.center = Vector3::new(center[0], center[1], center[2]);
        s.half = Vector3::new(half[0], half[1], half[2]);
        s.points.clear();
        for k in 0..n {
            s.points.push(Vector3::new(self.shape_points[k * 3], self.shape_points[k * 3 + 1], self.shape_points[k * 3 + 2]));
        }
        s.radius = radius;
        s.inverse_inertia = Vector3::new(inertia[0], inertia[1], inertia[2]);
        self.known[handle] = true;
    }

    /// The stations to watch, from `station_config` (watch).
    pub fn watch(&mut self, count: usize) {
        let count = count.min(dst::STATIONS);
        self.stations.clear();
        for i in 0..count {
            let c = &self.station_config[i * dstn::STRIDE..(i + 1) * dstn::STRIDE];
            self.stations.push(Station {
                lo: Vector3::new(c[dstn::MIN], c[dstn::MIN + 1], c[dstn::MIN + 2]),
                hi: Vector3::new(c[dstn::MAX], c[dstn::MAX + 1], c[dstn::MAX + 2]),
                center: Vector3::new(c[dstn::CENTER], c[dstn::CENTER + 1], c[dstn::CENTER + 2]),
                destroyed: false,
                health: 0.0,
                max_health: 1.0,
            });
        }
        // The states they start in are the first frame's.
        for i in 0..count {
            self.stations[i].destroyed = self.station_state[i * dss::STRIDE + dss::DESTROYED] != 0.0;
        }
    }

    // An event for the page to apply after the call: a light, a sound, a body to
    // hide, a wreck to glow or to cool.
    fn event(&mut self, kind: f64, a: f64, b: f64, at: &Vector3, rest: [f64; 6]) {
        // The block holds what one call can make (256 commands, every slot
        // and wreck at most a few events a frame); past it an event is
        // counted as lost, never written out of bounds.
        if self.events >= dst::EVENTS {
            self.overflow += 1;
            return;
        }
        let r = &mut self.result[ra::EVENTS + self.events * ev::STRIDE..ra::EVENTS + (self.events + 1) * ev::STRIDE];
        r.copy_from_slice(&[kind, a, b, at.x, at.y, at.z, rest[0], rest[1], rest[2], rest[3], rest[4], rest[5]]);
        self.events += 1;
    }

    // A light is asked for as an event, never made here: the lights belong to the
    // page.
    fn light(&mut self, kind: f64, at: &Vector3, color: f64, radius: f64, amount: f64) {
        self.event(kind, color, 0.0, at, [radius, amount, 0.0, 0.0, 0.0, 0.0]);
    }

    /// The floor under a point (for sparks and chips to skip off): its height
    /// and how level it is (normal.y). With none within 6 m there is nothing to
    /// skip off or glow on: NO_FLOOR, and flat 0 (floor).
    // Where the floor is under a blast or a droid, for sparks to skip off and
    // glows to lie on: a cast straight down from a little above the point, six
    // metres at most.
    fn floor(at: &Vector3, cast: Cast) -> (f64, f64) {
        let mut from = *at;
        from.add_scaled_vector(&UP, 0.3);
        match cast(&from, &DOWN, 6.0) {
            Some(hit) => (hit.point.y, hit.normal.y),
            None => (NO_FLOOR, 0.0),
        }
    }

    /// The blast: a flash and its light on the floor, a compact fireball
    /// turning to smoke, sparks, flakes of plating, a ring of dust thrown out
    /// along the ground, a scorch mark, arcs, and the light (blast).
    pub fn blast(&mut self, at: &Vector3, scale: f64, inherited: Option<&Vector3>, electric: f64, cast: Cast) {
        self.blasts += 1;
        let (floor, flat) = Self::floor(at, cast);
        let height = at.y - floor;
        let still = Vector3::default();
        let rng = &mut self.random;
        let fx = &mut self.fx;
        // The flash, in two pops: a short blue-white one at once and a longer one a
        // moment later (the delay), so the blast seems to kindle rather than appear.
        // The third (below) is the glow the blast throws on a floor close under it.
        fx.hot_particle(rng, FLASH, at, &still, 0.07, 0.22 * scale, 0.75 * scale, 0.0, 1.0, 1.6, electric, 0.0);
        fx.hot_particle(rng, FLASH, at, &still, 0.14, 0.2 * scale, 0.6 * scale, 0.0, 1.0, 1.3, 0.0, 0.015);
        if flat > 0.0 && height < 2.5 {
            fx.hot_particle(rng, GLOW, at, &still, 0.38, 1.1 * scale, 2.1 * scale, 0.0, 1.0, 0.85 * (1.0 - height / 2.5), floor, 0.0);
        }
        // A compact fireball: eleven fire particles thrown out and mostly up, each
        // living about a third of a second and rising (negative pull) as it thins.
        // They take a little of the droid's own velocity so a running droid's blast
        // trails behind it.
        for _ in 0..11 {
            let mut p = *at;
            let d = rng.direction(1.0, 0.0);
            let l = rng.rand(0.0, 0.18) * scale;
            p.add(&scaled(d, l));
            let mut v = rng.direction(0.8, 0.3);
            v.multiply_scalar(rng.rand(1.5, 3.6) * scale);
            if let Some(i) = inherited {
                v.add_scaled_vector(i, 0.4);
            }
            let life = rng.rand(0.28, 0.5);
            let s0 = rng.rand(0.1, 0.15) * scale;
            let s1 = rng.rand(0.3, 0.46) * scale;
            let delay = rng.rand(0.0, 0.04);
            fx.hot_particle(rng, FIRE, &p, &v, life, s0, s1, -0.4, 5.0, 1.0, -1e4, delay);
        }
        // Sparks: about 48 a unit of scale, fast and long-lived, that skip off the
        // floor (the floor's height rides in the particle's `extra`; none when there
        // is no floor under the blast).
        let sparks = jsrt::conv::js_round(48.0 * scale) as usize;
        for _ in 0..sparks {
            let mut v = rng.direction(0.75, 0.3);
            v.multiply_scalar(rng.rand(4.0, 14.0));
            let life = rng.rand(0.4, 1.2);
            let s0 = rng.rand(0.014, 0.024);
            let intensity = rng.rand(0.8, 1.4);
            fx.hot_particle(rng, SPARK, at, &v, life, s0, 0.01, 1.0, 0.8, intensity, floor, 0.0);
        }
        // Flakes of dark plating: smoke-pool particles that spin as they fly and
        // also skip off the floor.
        let chips = jsrt::conv::js_round(22.0 * scale) as usize;
        for _ in 0..chips {
            let mut v = rng.direction(0.8, 0.45);
            v.multiply_scalar(rng.rand(2.5, 7.0));
            let life = rng.rand(1.0, 1.8);
            let s0 = rng.rand(0.02, 0.05);
            let spin = rng.rand(-12.0, 12.0);
            fx.smoke_particle(rng, CHIP, at, &v, life, s0, 0.0, 1.0, 0.35, 1.0, Smoke { spin, floor, ..Smoke::default() });
        }
        // The oily smoke the fireball turns into: nine slow, big, dark puffs, lit
        // from below by the fire while young (heat 1) and drifting with the wind.
        for _ in 0..9 {
            let mut p = *at;
            let d = rng.direction(1.0, 0.0);
            let l = rng.rand(0.0, 0.25) * scale;
            p.add(&scaled(d, l));
            let mut v = rng.direction(0.6, 0.45);
            v.multiply_scalar(rng.rand(0.6, 1.6) * scale);
            v.add_scaled_vector(&UP, rng.rand(0.3, 0.9));
            let life = rng.rand(2.0, 3.4);
            let s0 = rng.rand(0.22, 0.3) * scale;
            let s1 = rng.rand(0.62, 0.95) * scale;
            let tone = rng.rand(0.08, 0.18);
            let spin = rng.rand(-0.4, 0.4);
            let delay = rng.rand(0.04, 0.14);
            fx.smoke_particle(rng, SMOKE, &p, &v, life, s0, s1, -0.06, 1.8, 0.72, Smoke { tone, heat: 1.0, wind: 0.8, spin, delay, ..Smoke::default() });
        }
        // Close to a level floor the shock throws a ring of dust out along it, and a
        // scorch mark is left where it lay (only on a nearly flat floor).
        if height < 1.6 && flat > 0.6 {
            // The shock throws a ring of dust out along the ground.
            let mut p = Vector3::default();
            for i in 0..26 {
                let angle = (i as f64 / 26.0 + rng.rand(-0.015, 0.015)) * std::f64::consts::PI * 2.0;
                p.set(at.x + v8math::cos(angle) * 0.3 * scale, floor + 0.1, at.z + v8math::sin(angle) * 0.3 * scale);
                let mut v = Vector3::default();
                v.set(v8math::cos(angle), 0.06, v8math::sin(angle));
                v.multiply_scalar(rng.rand(3.5, 6.0) * scale);
                let life = rng.rand(1.0, 1.6);
                let s1 = rng.rand(0.6, 0.9) * scale;
                let spin = rng.rand(-0.5, 0.5);
                fx.smoke_particle(rng, SMOKE, &p, &v, life, 0.2 * scale, s1, 0.0, 4.5, 0.26, Smoke { tone: 0.62, wind: 0.4, spin, ..Smoke::default() });
            }
            if flat > 0.85 {
                p.set(at.x, floor, at.z);
                let s0 = rng.rand(0.75, 0.95) * scale;
                fx.smoke_particle(rng, SCORCH, &p, &still, 26.0, s0, 0.0, 0.0, 1.0, 0.7, Smoke { floor, ..Smoke::default() });
            }
        }
        if electric > 0.0 {
            for _ in 0..6 {
                let mut p = rng.direction(0.7, 0.25);
                p.multiply_scalar(rng.rand(0.5, 1.2) * scale);
                p.add(at);
                let life = rng.rand(0.06, 0.22);
                fx.arc(rng, Anchor::WORLD, at, Anchor::WORLD, &p, life, 0.012, 1.0);
            }
        }
        self.light(ek::FLASH, at, FIRELIGHT, 7.0 * scale, 0.4);
        if electric > 0.0 {
            self.light(ek::FLASH, at, ELECTRIC, 5.0 * scale, 0.12);
        }
    }

    /// A small pop: the droideka's blasts while it dies, a spark burst (pop).
    // A droideka's death clip pops every 0.4 s: a smaller flash, a few embers,
    // two dozen sparks and a little smoke and arcs (the stock effect it replaces
    // played an explosion there).
    fn pop(&mut self, at: &Vector3, scale: f64, cast: Cast) {
        let (floor, _) = Self::floor(at, cast);
        let rng = &mut self.random;
        let fx = &mut self.fx;
        let mut v = Vector3::default();
        fx.hot_particle(rng, FLASH, at, &v, 0.08, 0.15 * scale, 0.45 * scale, 0.0, 1.0, 1.5, 1.0, 0.0);
        for _ in 0..4 {
            v = rng.direction(0.7, 0.5);
            v.multiply_scalar(rng.rand(0.8, 2.0));
            let life = rng.rand(0.24, 0.4);
            fx.hot_particle(rng, FIRE, at, &v, life, 0.08 * scale, 0.26 * scale, -0.6, 4.0, 1.0, -1e4, 0.0);
        }
        for _ in 0..24 {
            v = rng.direction(0.7, 0.35);
            v.multiply_scalar(rng.rand(3.0, 9.0));
            let life = rng.rand(0.3, 0.8);
            fx.hot_particle(rng, SPARK, at, &v, life, 0.016, 0.009, 1.0, 1.0, 1.2, floor, 0.0);
        }
        for _ in 0..2 {
            v = rng.direction(0.3, 0.8);
            v.multiply_scalar(0.6);
            let life = rng.rand(1.6, 2.4);
            fx.smoke_particle(rng, SMOKE, at, &v, life, 0.15 * scale, 0.6 * scale, -0.1, 1.4, 0.6, Smoke { tone: 0.12, heat: 0.8, delay: 0.05, ..Smoke::default() });
        }
        for _ in 0..3 {
            let mut p = rng.direction(0.8, 0.2);
            p.multiply_scalar(rng.rand(0.35, 0.8));
            p.add(at);
            let life = rng.rand(0.07, 0.16);
            fx.arc(rng, Anchor::WORLD, at, Anchor::WORLD, &p, life, 0.016, 1.3);
        }
        self.light(ek::FLASH, at, ELECTRIC, 4.5 * scale, 0.14);
    }

    /// One piece out of a blast at `centre` (throw). The page gave its shape
    /// (-1 for none) and the frame of its model in the world.
    #[allow(clippy::too_many_arguments)]
    fn throw(&mut self, shape: i64, matrix: &Matrix4, centre: &Vector3, inherited: &Vector3, speed: f64, heat: f64, cast: Cast) {
        let Some(shape) = self.shape_of(shape) else { return };
        let s_center = self.shapes[shape].center;
        let mut com = s_center;
        com.apply_matrix4(matrix);
        let mut out = Vector3::default();
        out.sub_vectors(&com, centre);
        let d = out.length();
        if d < 0.05 {
            out = self.random.direction(0.5, 0.3);
        } else {
            out.divide_scalar(d);
        }
        out.y += 0.35;
        let mut jitter = self.random.direction(1.0, 0.0);
        jitter.multiply_scalar(0.35);
        out.add(&jitter).normalize();
        let mut velocity = out;
        velocity.multiply_scalar(speed * self.random.rand(0.4, 0.7));
        velocity.add_scaled_vector(inherited, 0.5);
        velocity.add_scaled_vector(&UP, self.random.rand(0.4, 1.4));
        let mut spin = self.random.direction(1.0, 0.0);
        spin.multiply_scalar(self.random.rand(4.0, 11.0));
        // It tore away on the side that faced the blast.
        // It tore away on the side that faced the blast: the extreme point of its
        // shape nearest the blast, in the model's own frame, is where its heat is
        // concentrated.
        let mut inverse = *matrix;
        inverse.invert();
        let mut local = *centre;
        local.apply_matrix4(&inverse);
        let mut tear: Option<Vector3> = None;
        let mut nearest = f64::INFINITY;
        for p in self.shapes[shape].points.iter() {
            let mut q = Vector3::default();
            q.add_vectors(p, &s_center);
            let d2 = q.distance_to_squared(&local);
            if d2 < nearest {
                nearest = d2;
                tear = Some(*p);
            }
        }
        let mut tear_point = tear.unwrap_or(self.shapes[shape].points[0]);
        tear_point.add(&s_center);
        // A little less hot than the blast's own: a droid's pieces are hot, its
        // blaster (heat 0.25) is warm. This draw is made before the piece is, as the
        // page's code evaluated its arguments.
        let heat_now = heat * self.random.rand(0.85, 1.0);
        let soot = if heat > 0.5 { 1.0 } else { 0.3 };
        if !self.spawn(shape, matrix, &velocity, &spin, Some(tear_point), heat_now, soot, Some(centre), cast) {
            return;
        }
        let piece = self.pieces.last_mut().expect("spawned");
        let charge = if heat > 0.5 { self.random.rand(0.7, 1.0) } else { 0.2 };
        piece.charge = charge;
        piece.trail = 0.0;
        piece.shed = 0.0;
        piece.crackle = self.random.rand(0.02, 0.1);
        piece.wisp = 0.0;
    }

    /// A piece of `shape`, its model frame at `matrix` (world), moving at
    /// `velocity` and turning at `spin` (spawn). `from`: a point in the open (the
    /// blast) it must start on the same side of every surface as.
    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, shape: usize, matrix: &Matrix4, velocity: &Vector3, spin: &Vector3, tear: Option<Vector3>, heat: f64, soot: f64, from: Option<&Vector3>, cast: Cast) -> bool {
        // A frame or motion that isn't finite would only spread to its neighbours.
        if !matrix.elements.iter().all(|v| v.is_finite()) || !finite(velocity) || !finite(spin) {
            return false;
        }
        // Over the budget of pieces on the field, the oldest gives way (it is let go
        // at once, not crumbled) so a long battle never grows the list.
        if self.pieces.len() >= CAPACITY {
            self.retire(0);
        }
        let mut position = Vector3::default();
        let mut quaternion = Quaternion::default();
        let mut scale = Vector3::default();
        matrix.decompose(&mut position, &mut quaternion, &mut scale);
        let (s_center, s_radius) = (self.shapes[shape].center, self.shapes[shape].radius);
        let mut centre = s_center;
        centre.apply_quaternion(&quaternion).add(&position);
        // It starts on the blast's side of every surface: a centre through the
        // ground (a foot in a slope) or a wall is brought back out of it.
        // It starts on the blast's side of every surface: a centre that a foot in a
        // slope, or a wall, put through the ground is brought back out along the
        // ray from the blast to it, 3 cm clear.
        if let Some(from) = from {
            let mut ray = Vector3::default();
            ray.sub_vectors(&centre, from);
            let length = ray.length();
            if length > 0.01 {
                ray.divide_scalar(length);
                if let Some(hit) = cast(from, &ray, length) {
                    centre = hit.point;
                    centre.add_scaled_vector(&ray, -0.03);
                }
            }
        }
        let mut body = Piece::new(&self.shapes, shape, &centre, &quaternion, velocity, spin);
        body.lowest = centre.y - LOST;
        let (tx, ty, tz, tw) = match tear {
            Some(t) => (t.x, t.y, t.z, js_max(s_radius * 0.45, 0.08)),
            None => (s_center.x, s_center.y, s_center.z, s_radius * 0.5),
        };
        self.uid += 1;
        let uid = self.uid;
        // The page learns of the piece from its record (its model, how hot,
        // where it tore away), not from an event that could be lost.
        let mut live = Live {
            uid, shape, age: 0.0, rest: 0.0, heat, crumble: 0.0, light_clock: 0.0, shadows: true, charge: f64::NAN, trail: 0.0, shed: 0.0, crackle: 0.0, wisp: 0.0, dissolve: 0.0,
            soot, tear: [tx, ty, tz, tw], sample: None, object_position: Vector3::default(), matrix: [0.0; 16], moved: true,
        };
        Self::place(&self.shapes, &mut live, &body);
        live.sample = Some(body.position);
        self.pieces.push(live);
        self.bodies.push(body);
        true
    }

    /// Gone at once (over budget, or done): back to the idle models (retire).
    fn retire(&mut self, i: usize) {
        self.pieces.remove(i);
        self.bodies.remove(i);
    }

    /// Its model's place from its body (place): the model's origin is where
    /// the body's centre of mass would put it.
    // The page's model sits at its own origin, not at the body's centre of mass:
    // so its place is the body's, less the centre turned by the body's rotation.
    // The matrix is kept for the arcs that hang from the piece.
    fn place(shapes: &[Shape], live: &mut Live, body: &Piece) {
        let mut p = shapes[live.shape].center;
        p.apply_quaternion(&body.quaternion).negate().add(&body.position);
        live.object_position = p;
        let mut m = Matrix4::default();
        m.compose(&p, &body.quaternion, &Vector3::new(1.0, 1.0, 1.0));
        live.matrix = m.elements;
        live.moved = true;
    }

    /// A droid blowing apart: the blast, and its pieces flying (burst). The
    /// frames of its chunks are `chunks[start..start + count]`.
    #[allow(clippy::too_many_arguments)]
    // A droid blowing apart. The blast is at the mean of its chunks' centres; each
    // chunk is thrown out from it, and the droid's blaster too, more gently.
    fn burst(&mut self, slot: usize, walker: bool, position: &Vector3, velocity: &Vector3, start: usize, count: usize, weapon: i64, weapon_matrix: usize, sound: bool, explosion: bool, hide: bool, cast: Cast) {
        // The frames the page wrote are cut to the block (and the weapon's matrix
        // to the matrices), whatever count it gave.
        let start = start.min(ca::LEN);
        let count = count.min(ca::LEN - start);
        // (A chunk's shape or a weapon's matrix that is not in range is none.)
        let weapon = if weapon_matrix < ma::LEN { weapon } else { -1 };
        let mut centre = Vector3::default();
        let mut counted = 0;
        for k in start..start + count {
            let c = &self.chunks[k * dc::STRIDE..(k + 1) * dc::STRIDE];
            let Some(shape) = self.shape_of(c[dc::SHAPE] as i64) else { continue };
            let mut p = self.shapes[shape].center;
            p.apply_matrix4(&matrix_of(&c[dc::MATRIX..]));
            centre.add(&p);
            counted += 1;
        }
        if counted > 0 {
            centre.divide_scalar(counted as f64);
        } else {
            centre = *position;
            centre.add_scaled_vector(&UP, if walker { 0.8 } else { 0.9 });
        }
        let mut spread = 0.0;
        for k in start..start + count {
            let c = &self.chunks[k * dc::STRIDE..(k + 1) * dc::STRIDE];
            let mut p = Vector3::default();
            p.set_from_matrix_position(&matrix_of(&c[dc::MATRIX..]));
            spread = js_max(spread, p.distance_to(&centre));
        }
        // How big a blast: a droideka's is fixed; a droid's grows with how far apart
        // its chunks were when it blew (a body that was spread wide is a bigger
        // blast), within limits.
        let scale = if walker { 1.45 } else { three::clamp(0.75 + spread * 0.45, 0.9, 1.25) };
        self.blast(&centre, scale, Some(velocity), 1.0, cast);
        for k in start..start + count {
            let (shape, speed, m) = {
                let c = &self.chunks[k * dc::STRIDE..(k + 1) * dc::STRIDE];
                (c[dc::SHAPE] as i64, c[dc::SPEED], matrix_of(&c[dc::MATRIX..]))
            };
            self.throw(shape, &m, &centre, velocity, speed, 1.0, cast);
        }
        // The droid's blaster, thrown clear.
        if weapon >= 0 {
            let m = matrix_of(&self.matrices[weapon_matrix * 16..]);
            self.throw(weapon, &m, &centre, velocity, 5.0, 0.25, cast);
        }
        // Its pieces take its place, for the rest of its life; the page hides
        // the body and lets the unit go.
        // A droid that burst after its short-out is hidden by the page, on this event;
        // one blown apart by command was hidden when the command was made.
        if hide {
            self.event(ek::HIDE, slot as f64, 0.0, &Vector3::default(), [0.0; 6]);
        }
        if sound && explosion {
            self.event(ek::SOUND, slot as f64, 0.0, &centre, [0.0; 6]);
        }
    }

    // A bone's place, from the matrices the page gave this frame.
    fn bone_position(&self, slot: usize, bone: usize) -> Vector3 {
        let at = (slot * dst::BONES + bone.min(dst::BONES - 1)) * 16;
        let mut p = Vector3::default();
        p.set_from_matrix_position(&matrix_of(&self.slot_matrices[at..at + 16]));
        p
    }

    fn bone_matrix(&self, slot: usize, bone: usize) -> [f64; 16] {
        let at = (slot * dst::BONES + bone.min(dst::BONES - 1)) * 16;
        let mut m = [0.0; 16];
        m.copy_from_slice(&self.slot_matrices[at..at + 16]);
        m
    }

    /// One of a body's own carrying bones, at random (someBone).
    // One of the droid's own carrying bones, at random (the draw is made even for
    // a body with none, as before, but then there is nothing to pick).
    fn some_bone(&mut self, slot: usize) -> usize {
        let base = slot * sst::STRIDE;
        let count = (self.slot_static[base + sst::BONE_COUNT] as usize).min(dst::BONES);
        let pick = (self.random.next() * count as f64).floor() as usize;
        (self.slot_static[base + sst::BONES + pick.min(dst::BONES - 1)] as usize).min(dst::BONES - 1)
    }

    /// A droid shorting out: arcs along its limbs, sparks off its joints, a
    /// flickering blue light, until it blows. False once it is done
    /// (shortOut).
    // A droid killed on its feet crackles for 0.42 s (a droideka for its whole
    // death clip, 6 s) before it blows. Each effect has a rate a second, kept as
    // an accumulator of fractions so the births follow time, not frames.
    fn short_out(&mut self, death_index: usize, dt: f64, cast: Cast) -> bool {
        let slot = self.deaths[death_index].slot;
        self.deaths[death_index].t += dt;
        let s = slot * sl::STRIDE;
        let valid = self.slot_state[s + sl::VALID] != 0.0;
        let position = Vector3::new(self.slot_state[s + sl::POSITION], self.slot_state[s + sl::POSITION + 1], self.slot_state[s + sl::POSITION + 2]);
        if !valid {
            return false;
        }
        // Its time is up: it blows apart with its explosion's sound, and its pieces
        // take its place.
        let (walker, t, length) = (self.deaths[death_index].walker, self.deaths[death_index].t, self.deaths[death_index].length);
        if t >= length {
            let (velocity, start, count, weapon, wm, explosion) = self.burst_data(slot);
            self.burst(slot, walker, &position, &velocity, start, count, weapon, wm, true, explosion, true, cast);
            return false;
        }
        // The bones and joints the page listed, cut to the stores and to whole
        // pairs, whatever counts it gave.
        let base = slot * sst::STRIDE;
        let bone_count = (self.slot_static[base + sst::BONE_COUNT] as usize).min(dst::BONES);
        // A body whose bones were not listed has nothing to crawl over.
        if bone_count == 0 {
            return true;
        }
        let joints_at = base + sst::JOINTS;
        let joints = (self.slot_static[base + sst::JOINT_COUNT] as usize).min(dst::JOINT_NUMBERS) & !1;
        let mut centre = Vector3::default();
        for k in 0..bone_count {
            let bone = self.slot_static[base + sst::BONES + k] as usize;
            let b = self.bone_position(slot, bone);
            centre.add(&b);
        }
        centre.divide_scalar(bone_count as f64);
        let big = if walker { 1.3 } else { 1.0 };
        // Arcs: mostly along a limb between two joints, some leaping off it.
        // Arcs: mostly along a limb between two joints (a joint is a bone and its
        // nearest carrying ancestor), some leaping off a bone into the air.
        self.deaths[death_index].arcs += dt * 42.0 * big;
        while self.deaths[death_index].arcs >= 1.0 {
            self.deaths[death_index].arcs -= 1.0;
            if joints > 0 && self.random.next() < 0.7 {
                let k = 2 * ((self.random.next() * (joints as f64 / 2.0)).floor() as usize);
                let child = (self.slot_static[joints_at + k] as usize).min(dst::BONES - 1);
                let parent = (self.slot_static[joints_at + k + 1] as usize).min(dst::BONES - 1);
                let mut a = self.random.direction(1.0, 0.0);
                a.multiply_scalar(0.04);
                let mut b = self.random.direction(1.0, 0.0);
                b.multiply_scalar(0.04);
                let life = self.random.rand(0.07, 0.16);
                let from = Anchor::bone(slot as u32, child as u32, self.bone_matrix(slot, child));
                let to = Anchor::bone(slot as u32, parent as u32, self.bone_matrix(slot, parent));
                self.fx.arc(&mut self.random, from, &a, to, &b, life, 0.02, 1.5);
            } else {
                let bone = self.some_bone(slot);
                let mut b = self.bone_position(slot, bone);
                let mut v = self.random.direction(1.0, 0.0);
                v.multiply_scalar(self.random.rand(0.25, 0.6));
                b.add(&v);
                let life = self.random.rand(0.05, 0.12);
                let from = Anchor::bone(slot as u32, bone as u32, self.bone_matrix(slot, bone));
                self.fx.arc(&mut self.random, from, &Vector3::default(), Anchor::WORLD, &b, life, 0.016, 1.3);
            }
        }
        // Sparks spit from the bones, skipping off the floor below the droid.
        self.deaths[death_index].sparks += dt * 70.0 * big;
        let mut above = position;
        above.add_scaled_vector(&UP, 0.5);
        let (floor, _) = Self::floor(&above, cast);
        while self.deaths[death_index].sparks >= 1.0 {
            self.deaths[death_index].sparks -= 1.0;
            let bone = self.some_bone(slot);
            let a = self.bone_position(slot, bone);
            let mut v = self.random.direction(0.8, 0.3);
            v.multiply_scalar(self.random.rand(2.0, 6.5));
            let life = self.random.rand(0.2, 0.55);
            self.fx.hot_particle(&mut self.random, SPARK, &a, &v, life, 0.018, 0.01, 1.0, 1.2, 1.3, floor, 0.0);
        }
        // Blue-white flashes where it shorts, and a flickering glow about it.
        // Blue-white flashes where it shorts, and now and then a bigger glow at its
        // centre (a chance a frame that scales with time).
        self.deaths[death_index].flashes += dt * 18.0;
        while self.deaths[death_index].flashes >= 1.0 {
            self.deaths[death_index].flashes -= 1.0;
            let bone = self.some_bone(slot);
            let a = self.bone_position(slot, bone);
            self.fx.hot_particle(&mut self.random, FLASH, &a, &Vector3::default(), 0.09, 0.12, 0.32 * big, 0.0, 1.0, 1.4, 1.0, 0.0);
        }
        if self.random.next() < dt * 22.0 {
            let intensity = self.random.rand(0.25, 0.5);
            self.fx.hot_particle(&mut self.random, FLASH, &centre, &Vector3::default(), 0.06, 0.6 * big, 0.8 * big, 0.0, 1.0, intensity, 1.0, 0.0);
        }
        // Thin smoke rises from its bones while it burns.
        self.deaths[death_index].smoke += dt * if walker { 6.0 } else { 3.0 };
        while self.deaths[death_index].smoke >= 1.0 {
            self.deaths[death_index].smoke -= 1.0;
            let bone = self.some_bone(slot);
            let a = self.bone_position(slot, bone);
            let v = Vector3::new(self.random.rand(-0.2, 0.2), self.random.rand(0.4, 0.9), self.random.rand(-0.2, 0.2));
            let life = self.random.rand(1.4, 2.2);
            self.fx.smoke_particle(&mut self.random, SMOKE, &a, &v, life, 0.08, 0.36 * big, -0.08, 1.2, 0.5, Smoke { tone: 0.1, heat: 0.2, ..Smoke::default() });
        }
        // A flickering blue light about it.
        let strength = 0.6 + 0.8 * self.random.next();
        self.light(ek::ADD, &centre, ELECTRIC, 6.0 * big, strength);
        true
    }

    /// What a bursting droid brings: its velocity, its chunk frames, its held
    /// weapon's shape and matrix, whether it has an explosion. For a death in
    /// progress the page left them in the slot's burst block.
    fn burst_data(&self, slot: usize) -> (Vector3, usize, usize, i64, usize, bool) {
        let b = &self.burst_blocks[slot * burst_at::STRIDE..(slot + 1) * burst_at::STRIDE];
        // The slot's own weapon matrix is the one beside the commands'.
        (Vector3::new(b[burst_at::VELOCITY], b[burst_at::VELOCITY + 1], b[burst_at::VELOCITY + 2]), b[burst_at::START] as usize, b[burst_at::COUNT] as usize, b[burst_at::WEAPON] as i64, ma::WEAPONS + slot, b[burst_at::EXPLOSION] != 0.0)
    }

    /// A shape handle the page gave, if it names a shape the page has
    /// registered.
    fn shape_of(&self, handle: i64) -> Option<usize> {
        let h = usize::try_from(handle).ok()?;
        (h < dst::SHAPES && self.known[h]).then_some(h)
    }
}

fn scaled(mut v: Vector3, s: f64) -> Vector3 {
    v.multiply_scalar(s);
    v
}

fn finite(v: &Vector3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

impl Destruction {
    /// What the pieces give off: smoke and sparks as they fly, puffs of dust
    /// where they land, arcs while charged, wisps of smoke while hot
    /// (emitPieces).
    // Smoke trails, sparks, dust puffs and arcs from the pieces. Trails and
    // wisps give way when many pieces are out at once (the room left in the
    // smoke and hot pools), keeping a reserve for blasts.
    fn emit_pieces(&mut self, dt: f64) {
        // Trails and wisps give way when many pieces are out at once.
        let room = Fx::room(&self.fx.smoke);
        let spark_room = Fx::room(&self.fx.hot);
        for i in 0..self.pieces.len() {
            if self.pieces[i].charge.is_nan() {
                continue;
            }
            let (heat, rest) = (self.pieces[i].heat, self.pieces[i].rest);
            let at = self.bodies[i].position;
            let velocity = self.bodies[i].velocity;
            let ground = if self.bodies[i].ground.found { self.bodies[i].ground.point.y } else { NO_FLOOR };
            // A piece in flight leaves a trail and sheds sparks; one lying hot smoulders
            // in wisps for a few seconds; one that has just landed hard puffs dust and
            // sparks where it hit. A cold piece gives off nothing.
            let flying = !self.bodies[i].sleeping && velocity.length_sq() > 1.5;
            if flying && heat > 0.12 {
                self.pieces[i].trail += dt * 60.0 * heat * room;
                while self.pieces[i].trail >= 1.0 {
                    self.pieces[i].trail -= 1.0;
                    // Laid back along the path it flew this frame, so the trail is unbroken.
                    let mut a = at;
                    a.add_scaled_vector(&velocity, -dt * self.random.next());
                    let mut v = velocity;
                    v.multiply_scalar(-0.08);
                    let mut d = self.random.direction(1.0, 0.0);
                    d.multiply_scalar(0.12);
                    v.add(&d);
                    let life = self.random.rand(0.6, 1.0);
                    let s1 = self.random.rand(0.18, 0.3);
                    let spin = self.random.rand(-0.6, 0.6);
                    self.fx.smoke_particle(&mut self.random, SMOKE, &a, &v, life, 0.03 + 0.02 * heat, s1, -0.06, 2.4, 0.3 * heat, Smoke { tone: 0.22, heat: 0.5 * heat, wind: 0.6, spin, ..Smoke::default() });
                }
                self.pieces[i].shed += dt * 14.0 * heat * spark_room;
                while self.pieces[i].shed >= 1.0 {
                    self.pieces[i].shed -= 1.0;
                    let mut v = velocity;
                    v.multiply_scalar(0.3);
                    let mut d = self.random.direction(1.0, 0.0);
                    d.multiply_scalar(self.random.rand(1.0, 3.0));
                    v.add(&d);
                    let style = if self.random.next() < 0.6 { SPARK } else { EMBER };
                    let life = self.random.rand(0.25, 0.6);
                    self.fx.hot_particle(&mut self.random, style, &at, &v, life, 0.014, 0.01, 0.8, 1.4, 0.9, ground, 0.0);
                }
            } else if heat > 0.06 && rest < 12.0 {
                self.pieces[i].wisp += dt * 3.2 * js_min(heat * 2.5, 1.0) * room;
                while self.pieces[i].wisp >= 1.0 {
                    self.pieces[i].wisp -= 1.0;
                    let mut a = at;
                    let mut d = self.random.direction(1.0, 0.0);
                    d.multiply_scalar(self.shapes[self.pieces[i].shape].radius * 0.4);
                    a.add(&d);
                    let v = Vector3::new(self.random.rand(-0.1, 0.1), self.random.rand(0.35, 0.7), self.random.rand(-0.1, 0.1));
                    let life = self.random.rand(2.2, 3.4);
                    let s1 = self.random.rand(0.22, 0.36);
                    let spin = self.random.rand(-0.3, 0.3);
                    self.fx.smoke_particle(&mut self.random, SMOKE, &a, &v, life, 0.04, s1, -0.05, 1.0, 0.24 * js_min(heat * 3.0, 1.0), Smoke { tone: 0.22, heat: 0.4 * heat, wind: 1.0, spin, ..Smoke::default() });
                    if self.random.next() < 0.35 {
                        let e = Vector3::new(self.random.rand(-0.2, 0.2), self.random.rand(0.3, 0.8), self.random.rand(-0.2, 0.2));
                        let life = self.random.rand(0.8, 1.6);
                        self.fx.hot_particle(&mut self.random, EMBER, &a, &e, life, 0.018, 0.012, -0.03, 1.0, 0.9, -1e4, 0.0);
                    }
                }
            }
            // A landing: how hard it was scales the dust and the sparks. (The point is
            // the offset the body's contact left, kept as it always was.)
            let impact = self.bodies[i].impact;
            if impact > 2.2 {
                let hard = js_min(impact / 8.0, 1.0);
                let p = self.bodies[i].impact_point;
                for _ in 0..(2 + jsrt::conv::js_round(hard * 3.0) as usize) {
                    let mut v = self.random.direction(0.4, 0.6);
                    v.multiply_scalar(self.random.rand(0.4, 1.2));
                    let life = self.random.rand(0.7, 1.2);
                    let s1 = self.random.rand(0.22, 0.38) * (0.6 + hard);
                    self.fx.smoke_particle(&mut self.random, SMOKE, &p, &v, life, 0.06, s1, 0.05, 3.0, 0.35 * hard + 0.1, Smoke { tone: 0.62, wind: 0.5, ..Smoke::default() });
                }
                if heat > 0.2 {
                    for _ in 0..(jsrt::conv::js_round(4.0 + 8.0 * hard) as usize) {
                        let mut v = self.random.direction(0.5, 0.55);
                        v.multiply_scalar(self.random.rand(1.5, 5.0) * hard + 1.0);
                        let life = self.random.rand(0.2, 0.5);
                        self.fx.hot_particle(&mut self.random, SPARK, &p, &v, life, 0.013, 0.008, 1.0, 1.5, 1.0, p.y - 0.01, 0.0);
                    }
                }
            }
            self.bodies[i].impact = 0.0;
            // What charge it kept when it tore away decays; while it is charged it
            // crackles with small arcs between its own points, now and then with sparks.
            self.pieces[i].charge *= v8math::exp(-dt / CHARGE_TIME);
            if self.pieces[i].charge > 0.08 {
                self.pieces[i].crackle -= dt;
                if self.pieces[i].crackle <= 0.0 {
                    let charge = self.pieces[i].charge;
                    self.pieces[i].crackle = self.random.rand(0.04, 0.2) / charge;
                    let shape = self.pieces[i].shape;
                    let centre = self.shapes[shape].center;
                    let n = self.shapes[shape].points.len() as f64;
                    let mut a = self.shapes[shape].points[(self.random.next() * n).floor() as usize];
                    a.add(&centre);
                    let mut b = self.shapes[shape].points[(self.random.next() * n).floor() as usize];
                    b.add(&centre);
                    let life = self.random.rand(0.05, 0.12);
                    let uid = self.pieces[i].uid;
                    let matrix = self.pieces[i].matrix;
                    self.fx.arc(&mut self.random, Anchor::piece(uid, matrix), &a, Anchor::piece(uid, matrix), &b, life, 0.008, 0.85 * js_min(charge * 1.5, 1.0));
                    if self.random.next() < 0.5 {
                        a.apply_matrix4(&matrix_of(&matrix));
                        for _ in 0..4 {
                            let mut v = self.random.direction(0.8, 0.3);
                            v.multiply_scalar(self.random.rand(1.0, 3.5));
                            let life = self.random.rand(0.15, 0.4);
                            self.fx.hot_particle(&mut self.random, SPARK, &a, &v, life, 0.012, 0.008, 1.0, 1.5, 1.0, ground, 0.0);
                        }
                    }
                }
            }
        }
    }

    /// The pieces' lives (DebrisPieces.update): the solver steps them, then
    /// each cools, rests, crumbles and is let go when it should be.
    // The pieces' lives: the solver steps their bodies, then each cools, rests,
    // crumbles and is let go when it should be. The scan runs from the end so
    // removing one never skips another.
    fn update_pieces(&mut self, dt: f64, cast: Cast) {
        if dt <= 0.0 || self.pieces.is_empty() {
            return;
        }
        self.solver.step(&self.shapes, &mut self.bodies, dt, cast);
        let mut i = self.pieces.len();
        while i > 0 {
            i -= 1;
            let sleeping = self.bodies[i].sleeping;
            self.pieces[i].age += dt;
            self.pieces[i].heat *= v8math::exp(-dt / COOL);
            if !sleeping {
                Self::place(&self.shapes, &mut self.pieces[i], &self.bodies[i]);
                self.pieces[i].light_clock -= dt;
                if self.pieces[i].light_clock <= 0.0 {
                    self.pieces[i].light_clock = LIGHT_EVERY;
                    self.pieces[i].sample = Some(self.bodies[i].position);
                }
            } else if self.pieces[i].rest == 0.0 {
                self.pieces[i].sample = Some(self.bodies[i].position);
            }
            if sleeping {
                self.pieces[i].rest += dt;
            }
            let flight = self.bodies[i].flight;
            if self.pieces[i].rest >= LINGER || self.pieces[i].age >= MAX_FLIGHT + LINGER || (!sleeping && flight >= MAX_FLIGHT) {
                self.pieces[i].crumble += dt / CRUMBLE;
            }
            if self.pieces[i].crumble > 0.0 {
                self.pieces[i].dissolve = js_min(self.pieces[i].crumble, 1.0) * 1.02;
            }
            // No shadow once it starts to crumble, or far off.
            // No shadow once it starts to crumble, or far from the eye: a piece's shadow
            // is one of the dearer things on a slow PC.
            let near = !self.has_eye || self.eye.distance_to_squared(&self.bodies[i].position) < SHADOW_REACH * SHADOW_REACH;
            self.pieces[i].shadows = self.pieces[i].crumble <= 0.0 && near;
            let body = &self.bodies[i];
            if self.pieces[i].crumble >= 1.0 || body.position.y.partial_cmp(&body.lowest) == Some(std::cmp::Ordering::Less) || body.position.y.is_nan() || body.lowest.is_nan() || !finite(&body.position) {
                self.retire(i);
            }
        }
    }

    /// Power-up droids wrecked since the last frame (watchStations).
    // Power-up droids wrecked since the last frame: only the change into
    // destroyed starts a wreck (and only when Enhanced is on).
    fn watch_stations(&mut self, cast: Cast) {
        let count = self.stations.len();
        for i in 0..count {
            let s = &self.station_state[i * dss::STRIDE..(i + 1) * dss::STRIDE];
            let (now, health, max_health) = (s[dss::DESTROYED] != 0.0, s[dss::HEALTH], s[dss::MAX_HEALTH]);
            // A wreck's repair is read from the station each frame: a repair weapon
            // rebuilding it smothers its smoke and flames as it goes.
            self.stations[i].health = health;
            self.stations[i].max_health = max_health;
            let was = self.stations[i].destroyed;
            if was == now {
                continue;
            }
            self.stations[i].destroyed = now;
            if now && self.enhanced {
                self.wreck(i, cast);
            }
        }
    }

    // A wrecked power-up droid goes up in a blast of its own, then smoulders.
    // Three hot points on it (drawn now, in the old order) are where its flames
    // and embers start.
    fn wreck(&mut self, i: usize, cast: Cast) {
        let (lo, hi, blast_at) = (self.stations[i].lo, self.stations[i].hi, self.stations[i].center);
        let mut centre = Vector3::default();
        centre.add_vectors(&lo, &hi).multiply_scalar(0.5);
        let mut size = Vector3::default();
        size.sub_vectors(&hi, &lo);
        let top = Vector3::new(centre.x, hi.y - size.y * 0.15, centre.z);
        let mut hot = [Vector3::default(); 3];
        for h in hot.iter_mut() {
            let x = self.random.rand(lo.x, hi.x) * 0.6 + centre.x * 0.4;
            let y = lo.y + size.y * self.random.rand(0.35, 0.8);
            let z = self.random.rand(lo.z, hi.z) * 0.6 + centre.z * 0.4;
            h.set(x, y, z);
        }
        self.blast(&blast_at, 1.35, None, 1.0, cast);
        let crackle = self.random.rand(0.3, 0.8);
        // A wreck for the same station before it is replaced.
        self.sites.retain(|other| other.station != i);
        self.sites.push(Site { station: i, t: 0.0, lo, hi, centre, top, hot, size, smoke: 0.0, flame: 0.0, ember: 0.0, crackle, heat: 0.85, soot: 1.0 });
        self.event(ek::SITE_START, i as f64, 0.0, &Vector3::default(), [0.0; 6]);
    }

    /// One wreck's smoulder for a frame. False once it is out (smoulder).
    // One wreck's smoulder for a frame. Everything fades with its age and with
    // how far a repair weapon has rebuilt it; a wreck far from the camera still
    // ages but emits nothing.
    fn smoulder(&mut self, k: usize, dt: f64) -> bool {
        let station = self.sites[k].station;
        self.sites[k].t += dt;
        let t = self.sites[k].t;
        if !self.stations[station].destroyed || t > SITE_LIFE {
            self.event(ek::SITE_END, station as f64, 0.0, &Vector3::default(), [0.0; 6]);
            return false;
        }
        // A repair weapon rebuilding it smothers it as it goes.
        let repaired = three::clamp(self.stations[station].health / js_max(self.stations[station].max_health, 1e-6), 0.0, 1.0);
        let left = v8math::pow(1.0 - repaired, 0.7);
        let fade = 1.0 - three::smoothstep(t, SITE_LIFE * 0.55, SITE_LIFE);
        // Burning at first, then a small flame flickering on for a minute or so.
        // Burning at first, then a small flame flickering on for a minute or so; the
        // smoke thins over the wreck's life; the crackle lasts a couple of minutes.
        let flames = (v8math::exp(-t / 7.0) + 0.1 * (1.0 - three::smoothstep(t, 40.0, 90.0))) * left;
        let smoke = (0.4 + 0.6 * v8math::exp(-t / 20.0)) * fade * left;
        let crackle = (0.2 + 0.8 * v8math::exp(-t / 12.0)) * (1.0 - three::smoothstep(t, 60.0, 120.0)) * left;
        self.sites[k].heat = 0.85 * v8math::exp(-t / 50.0) * left;
        self.sites[k].soot = 1.0;
        let centre = self.sites[k].centre;
        if self.eye.distance_to_squared(&centre) > SITE_REACH * SITE_REACH {
            return true;
        }
        let (top, size, lo, hi) = (self.sites[k].top, self.sites[k].size, self.sites[k].lo, self.sites[k].hi);
        // Thick black smoke while it burns, a thinner grey wisp as it dies down,
        // dark enough to read against pale stone.
        self.sites[k].smoke += dt * (1.6 + 5.0 * smoke) * js_min(smoke * 3.0, 1.0);
        while self.sites[k].smoke >= 1.0 {
            self.sites[k].smoke -= 1.0;
            // Thick and black while it burns, a thinner grey wisp as it dies down.
            let young = v8math::exp(-t / 25.0);
            let mut a = top;
            a.x += self.random.rand(-0.3, 0.3) * size.x * (0.4 + 0.6 * young);
            a.z += self.random.rand(-0.3, 0.3) * size.z * (0.4 + 0.6 * young);
            let v = Vector3::new(self.random.rand(-0.15, 0.15), self.random.rand(0.6, 1.15), self.random.rand(-0.15, 0.15));
            // Dark enough to read against pale stone even as it thins.
            let life = self.random.rand(4.5, 7.0);
            let s0 = self.random.rand(0.12, 0.2) + 0.1 * young;
            let s1 = self.random.rand(0.7, 1.1) + 0.5 * young;
            let tone = 0.1 + 0.1 * (1.0 - young) + self.random.rand(0.0, 0.06);
            let spin = self.random.rand(-0.25, 0.25);
            self.fx.smoke_particle(&mut self.random, SMOKE, &a, &v, life, s0, s1, -0.12, 0.55, (0.44 + 0.25 * young) * js_min(smoke * 2.5, 1.0), Smoke { tone, heat: 0.7 * flames, wind: 1.0, spin, ..Smoke::default() });
        }
        // Small flames from the hot points.
        self.sites[k].flame += dt * 14.0 * flames;
        while self.sites[k].flame >= 1.0 {
            self.sites[k].flame -= 1.0;
            let pick = (self.random.next() * 3.0).floor() as usize;
            let mut a = self.sites[k].hot[pick];
            let mut d = self.random.direction(1.0, 0.0);
            d.multiply_scalar(0.08);
            a.add(&d);
            let v = Vector3::new(self.random.rand(-0.2, 0.2), self.random.rand(0.5, 1.1), self.random.rand(-0.2, 0.2));
            let life = self.random.rand(0.3, 0.55);
            let s0 = self.random.rand(0.06, 0.09);
            let s1 = self.random.rand(0.18, 0.28) * (0.5 + flames * 0.5);
            self.fx.hot_particle(&mut self.random, FIRE, &a, &v, life, s0, s1, -0.9, 2.2, 0.85, -1e4, 0.0);
        }
        // Embers that rise and drift.
        self.sites[k].ember += dt * (0.8 + 2.5 * flames) * js_min(smoke * 2.5, 1.0);
        while self.sites[k].ember >= 1.0 {
            self.sites[k].ember -= 1.0;
            let pick = (self.random.next() * 3.0).floor() as usize;
            let mut a = self.sites[k].hot[pick];
            let mut d = self.random.direction(1.0, 0.0);
            d.multiply_scalar(self.random.rand(0.0, 0.25));
            a.add(&d);
            let v = Vector3::new(self.random.rand(-0.35, 0.35), self.random.rand(0.4, 1.2), self.random.rand(-0.35, 0.35));
            let life = self.random.rand(1.2, 2.6);
            let s0 = self.random.rand(0.012, 0.02);
            let intensity = self.random.rand(0.6, 1.0);
            self.fx.hot_particle(&mut self.random, EMBER, &a, &v, life, s0, 0.008, -0.04, 0.6, intensity, -1e4, 0.0);
        }
        // Now and then a crackle of sparks, a flash and an arc inside the wreck.
        self.sites[k].crackle -= dt;
        if self.sites[k].crackle <= 0.0 && crackle > 0.05 {
            self.sites[k].crackle = self.random.rand(0.5, 2.6) / crackle;
            let a = Vector3::new(self.random.rand(lo.x, hi.x), self.random.rand(lo.y + size.y * 0.3, hi.y), self.random.rand(lo.z, hi.z));
            let floor = lo.y;
            for _ in 0..(6 + jsrt::conv::js_round(10.0 * crackle) as usize) {
                let mut v = self.random.direction(0.8, 0.35);
                v.multiply_scalar(self.random.rand(1.5, 5.5));
                let life = self.random.rand(0.25, 0.7);
                self.fx.hot_particle(&mut self.random, SPARK, &a, &v, life, 0.014, 0.009, 1.0, 1.2, 1.0, floor, 0.0);
            }
            self.fx.hot_particle(&mut self.random, FLASH, &a, &Vector3::default(), 0.07, 0.06, 0.2, 0.0, 1.0, 1.0, 1.0, 0.0);
            let b = Vector3::new(self.random.rand(lo.x, hi.x), self.random.rand(lo.y + size.y * 0.2, hi.y), self.random.rand(lo.z, hi.z));
            let life = self.random.rand(0.08, 0.18);
            self.fx.arc(&mut self.random, Anchor::WORLD, &a, Anchor::WORLD, &b, life, 0.01, 0.9);
            self.light(ek::FLASH, &a, ELECTRIC, 3.5, 0.1);
        }
        // A flickering orange light over the wreck while it glows.
        let flicker = 0.75 + 0.25 * v8math::sin(t * 23.0 + self.sites[k].hot[0].x) * v8math::sin(t * 7.3);
        let glow = (0.15 + 0.85 * flames) * v8math::exp(-t / 50.0) * left * flicker;
        if glow > 0.03 {
            let mut a = centre;
            a.add_scaled_vector(&UP, 0.4);
            self.light(ek::ADD, &a, EMBERLIGHT, 3.5 + 2.0 * flames, 0.45 * glow);
        }
        true
    }

    /// Executes one command the page wrote (blowApart, died, deathBlast,
    /// blast, clear).
    // One command the page wrote. Nothing in it is trusted to be in range: a slot
    // past the table is none, a count is cut to its block, a handle to what the
    // page has registered.
    fn command(&mut self, i: usize, cast: Cast) {
        let c: [f64; cmd::STRIDE] = self.commands[i * cmd::STRIDE..(i + 1) * cmd::STRIDE].try_into().expect("a record");
        let at = Vector3::new(c[cmd::AT], c[cmd::AT + 1], c[cmd::AT + 2]);
        let velocity = Vector3::new(c[cmd::VELOCITY], c[cmd::VELOCITY + 1], c[cmd::VELOCITY + 2]);
        // Nothing the page writes is trusted to be in range: a slot past the
        // table is none, a handle or index past its block is none, and
        // counts are cut to what the blocks hold.
        let slot = c[cmd::SLOT] as usize;
        let kind = c[cmd::KIND];
        if kind == kind::BLOW_APART {
            self.deaths.retain(|d| d.slot != slot);
            self.burst(slot, c[cmd::WALKER] != 0.0, &at, &velocity, c[cmd::FRAME_START] as usize, c[cmd::FRAME_COUNT] as usize, c[cmd::WEAPON] as i64, c[cmd::MATRIX_START] as usize, c[cmd::SOUND] != 0.0, c[cmd::EXPLOSION] != 0.0, false, cast);
        } else if kind == kind::DIED {
            let walker = c[cmd::WALKER] != 0.0;
            if slot >= dst::SLOTS || self.deaths.len() >= dst::SLOTS {
                return;
            }
            // A droideka's clip runs to its explosion; the simulation ends it.
            self.deaths.push(Death { slot, t: 0.0, length: if walker { 6.0 } else { SHORT_OUT }, arcs: 0.0, sparks: 0.0, flashes: 0.0, smoke: 0.0, walker });
        } else if kind == kind::DEATH_BLAST {
            // A droideka's blast during its death clip (every 0.4 s): at one of
            // its bones, at random.
            // (The commands' matrices end where the droids' own weapon matrices begin.)
            let start = (c[cmd::MATRIX_START] as usize).min(ma::WEAPONS);
            let count = (c[cmd::MATRIX_COUNT] as usize).min(ma::WEAPONS - start);
            if count > 0 {
                let pick = (self.random.next() * count as f64).floor() as usize;
                let m = matrix_of(&self.matrices[(start + pick) * 16..]);
                let mut p = Vector3::default();
                p.set_from_matrix_position(&m);
                self.pop(&p, 1.0, cast);
            }
        } else if kind == kind::BLAST {
            let inherited = if c[cmd::INHERITED] != 0.0 { Some(velocity) } else { None };
            self.blast(&at, c[cmd::SCALE], inherited.as_ref(), c[cmd::ELECTRIC], cast);
        } else {
            self.clear();
        }
    }

    /// Everything gone (clear): the deaths, the wrecks (the page puts their
    /// materials back), the pieces and the effects.
    // Everything gone: the deaths, the wrecks (the page puts their materials back
    // at the events below), the pieces and the effects. The page runs this at
    // once, even while the game is paused.
    fn clear(&mut self) {
        self.deaths.clear();
        for k in 0..self.sites.len() {
            let station = self.sites[k].station;
            self.event(ek::SITE_END, station as f64, 0.0, &Vector3::default(), [0.0; 6]);
        }
        self.sites.clear();
        while !self.pieces.is_empty() {
            self.retire(self.pieces.len() - 1);
        }
        self.fx.clear();
    }

    /// Which slots will burst this frame: the page then brings the frames of
    /// their chunks (Destruction.update's first look at the deaths).
    // Which slots will burst this frame, so the page can bring the frames of
    // their chunks before the phase runs (it is the only one that has them).
    pub fn prepare(&mut self) {
        let dt = three::clamp(self.frame[df::DT], 0.0, 0.1);
        let mut n = 0;
        for d in self.deaths.iter() {
            if self.slot_state[d.slot * sl::STRIDE + sl::VALID] != 0.0 && d.t + dt >= d.length {
                self.result[ra::BURSTS + n] = d.slot as f64;
                n += 1;
            }
        }
        self.result[dr::BURSTS] = n as f64;
    }

    /// The frame (Destruction.update): the commands the page wrote, in order,
    /// then the wrecks watched, the deaths, the pieces, the emitters, the
    /// wrecks' smoulder and the effects' own step.
    // The frame: the commands the page wrote, in order, then the wrecks watched,
    // the deaths, the pieces, the emitters, the wrecks' smoulder and the effects'
    // own step. Nothing runs but the commands and the wrecks watch when the
    // setting is Classic.
    pub fn update(&mut self, cast: Cast) {
        self.begin();
        self.run_commands(cast);
        let dt = three::clamp(self.frame[df::DT], 0.0, 0.1);
        let count = (self.frame[df::STATIONS] as usize).min(dst::STATIONS);
        if self.stations.len() > count {
            self.stations.truncate(count);
        }
        self.watch_stations(cast);
        let mut fx_updated = 0.0;
        if self.enhanced {
            if self.frame[df::HAS_EYE] != 0.0 {
                self.eye = Vector3::new(self.frame[df::EYE], self.frame[df::EYE + 1], self.frame[df::EYE + 2]);
                self.has_eye = true;
            } else {
                self.has_eye = false;
            }
            let mut kept = 0;
            for k in 0..self.deaths.len() {
                if self.short_out(k, dt, cast) {
                    self.deaths.swap(kept, k);
                    kept += 1;
                }
            }
            self.deaths.truncate(kept);
            self.update_pieces(dt, cast);
            self.emit_pieces(dt);
            let mut kept = 0;
            for k in 0..self.sites.len() {
                if self.smoulder(k, dt) {
                    self.sites.swap(kept, k);
                    kept += 1;
                }
            }
            self.sites.truncate(kept);
            self.refresh_anchors();
            self.fx.update(dt, &mut self.fx_result);
            fx_updated = 1.0;
        } else {
            self.fx.report(&mut self.fx_result);
        }
        self.write_result(fx_updated);
    }

    /// What a call starts from: no events yet, and no piece moved or asked
    /// for light since the page last read the records.
    // What each call starts from: no events yet, and no piece moved or asked for
    // light since the page last read the records.
    fn begin(&mut self) {
        self.events = 0;
        self.overflow = 0;
        self.enhanced = self.frame[df::ENHANCED] != 0.0;
        for p in self.pieces.iter_mut() {
            p.moved = false;
            p.sample = None;
        }
    }

    /// The commands the page has written, in order, never past the block
    /// whatever count it gave.
    fn run_commands(&mut self, cast: Cast) {
        // The commands are never run past the block, whatever count the page wrote.
        let commands = (self.frame[df::COMMANDS] as usize).min(dst::COMMANDS);
        for i in 0..commands {
            self.command(i, cast);
        }
        self.frame[df::COMMANDS] = 0.0;
    }

    /// Only the commands: the page calls this when its queue is nearly full
    /// (a long load or a pause makes events with no frame to carry them) and
    /// when an effect must happen now (switching to Classic while paused).
    /// The time does not move, and what the commands made is in the result
    /// as after a frame.
    pub fn apply(&mut self, cast: Cast) {
        self.begin();
        self.run_commands(cast);
        self.fx.report(&mut self.fx_result);
        self.write_result(0.0);
    }

    /// Refreshes the matrices of what arcs hang from before they are drawn:
    /// the pieces' as last placed, a droid's bones as the page last gave them.
    // What arcs hang from is refreshed before they are drawn: a piece's matrix as
    // last placed, a droid's bone as the page last gave it (a bone keeps its last
    // matrix once its droid is gone, so a late arc ends where it was).
    fn refresh_anchors(&mut self) {
        let pieces = &self.pieces;
        let slot_matrices = &self.slot_matrices;
        self.fx.arcs.refresh(&mut |anchor| match anchor.kind {
            crate::fx::AnchorKind::Piece => {
                if let Some(p) = pieces.iter().find(|p| p.uid == anchor.a) {
                    anchor.matrix = p.matrix;
                }
            }
            crate::fx::AnchorKind::Bone => {
                let at = (anchor.a as usize % dst::SLOTS * dst::BONES + (anchor.b as usize).min(dst::BONES - 1)) * 16;
                anchor.matrix.copy_from_slice(&slot_matrices[at..at + 16]);
            }
            crate::fx::AnchorKind::World => {}
        });
    }

    // What the page reads after a call: counts, the slots' holds, the wrecks' look,
    // and every piece's record. The records are the whole truth of the pieces,
    // so a lost event can never leave the page out of step.
    fn write_result(&mut self, fx_updated: f64) {
        let r = &mut self.result;
        r[dr::EVENTS] = self.events as f64;
        r[dr::PIECES] = self.pieces.len() as f64;
        r[dr::SITES] = self.sites.len() as f64;
        r[dr::BLASTS] = self.blasts as f64;
        r[dr::DEATHS] = self.deaths.len() as f64;
        r[dr::FX] = fx_updated;
        r[dr::OVERFLOW] = self.overflow as f64;
        for s in 0..dst::SLOTS {
            let deaths = self.deaths.iter().filter(|d| d.slot == s).count();
            self.result[ra::REFS + s] = (deaths + self.fx.arcs.bone_refs(s as u32)) as f64;
            self.result[ra::ACTIVE + s] = deaths as f64;
        }
        for (k, site) in self.sites.iter().enumerate() {
            let r = &mut self.result[ra::SITES + k * SITE_RECORD..ra::SITES + (k + 1) * SITE_RECORD];
            r[ds::STATION] = site.station as f64;
            r[ds::HEAT] = site.heat;
            r[ds::SOOT] = site.soot;
        }
        for (k, (live, body)) in self.pieces.iter().zip(self.bodies.iter()).enumerate() {
            let r = &mut self.result[ra::PIECES + k * dp::STRIDE..ra::PIECES + (k + 1) * dp::STRIDE];
            r[dp::UID] = live.uid as f64;
            r[dp::SHAPE] = live.shape as f64;
            r[dp::POSITION..dp::POSITION + 3].copy_from_slice(&[live.object_position.x, live.object_position.y, live.object_position.z]);
            r[dp::QUATERNION..dp::QUATERNION + 4].copy_from_slice(&[body.quaternion.x, body.quaternion.y, body.quaternion.z, body.quaternion.w]);
            r[dp::HEAT] = live.heat;
            r[dp::DISSOLVE] = live.dissolve;
            r[dp::MOVED] = f64::from(live.moved);
            r[dp::SHADOWS] = f64::from(live.shadows);
            r[dp::SOOT] = live.soot;
            r[dp::TEAR..dp::TEAR + 4].copy_from_slice(&live.tear);
            r[dp::SAMPLE] = f64::from(live.sample.is_some());
            let at = live.sample.unwrap_or_default();
            r[dp::SAMPLE_POSITION..dp::SAMPLE_POSITION + 3].copy_from_slice(&[at.x, at.y, at.z]);
        }
    }
}
