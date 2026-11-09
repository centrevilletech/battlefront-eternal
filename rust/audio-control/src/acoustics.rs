//! The enhanced mode's acoustics, worked out from the map's own collision
//! (the same BVH the game's rays use). None of this is in the 2004 game,
//! which has one reverb "space" per sub-level (Theed's is all but dry); the
//! original mode skips it.
//!
//! - Air absorption: high frequencies fall off with distance, a lowpass whose
//!   corner stays at 20 kHz to 20 m and then drops, to 9 kHz at 100 m and
//!   4 kHz at 300 m: about half of what ISO 9613-1 gives at 20 C and 50%
//!   humidity, so a battle across the plaza keeps its edge.
//! - Occlusion: three rays from the listener to each heard voice, aimed a
//!   metre above its origin (a soldier's origin is at his feet, where the
//!   ground's bumps would block everything) and half a metre to either side
//!   of it. The share blocked eases the voice toward -4.4 dB and a lowpass
//!   near 2.8 kHz: muffled, not buried. Six rays a frame, each voice's next
//!   in turn, so every voice is looked at again every few frames.
//! - Rooms: a fan of 19 rays from the listener, two a frame (the whole fan
//!   about six times a second), measures the space: how many walls are
//!   within 60 m, how far they are, and whether there is a roof. Walls give early reflections; a roof gives the long,
//!   wet tail. Under Theed's arcades a dense, wetter room of about a second;
//!   in the plaza, ringed by buildings but open above, reflections and a
//!   faint, shorter tail.
//!
//! The enhanced mode's loudness window (in the manner of "HDR audio"): the
//! loudest sound playing sets the top of the window, and quieter kinds of
//! sound make room under it. A blast close by pulls the ambience down by up
//! to 6 dB and the foley by up to 3 dB for as long as it rings, then lets
//! them back over about a second and a half; the voice-over duck stays the
//! game's own (its bus attenuation). The original mode has none of this.
//!
use crate::{Collision, Point, length, sub, scale};
use crate::engine::{Engine, Shape, CAPACITY, AMBIENCE, EXPLOSIONS, FOLEY, VOICE, WEAPONS};

#[derive(Clone, Copy)]
pub struct Room {
    pub room: f64, pub room_hf: f64, pub decay: f64, pub hf_ratio: f64,
    pub reflections: f64, pub refl_delay: f64, pub reverb: f64, pub reverb_delay: f64,
    pub diffusion: f64, pub density: f64, pub hf_reference: f64,
}
impl Default for Room {
    fn default() -> Self { Self { room: 0.08, room_hf: 0.8, decay: 0.8, hf_ratio: 0.6, reflections: 0.2, refl_delay: 0.02, reverb: 0.7, reverb_delay: 0.03, diffusion: 1.0, density: 1.0, hf_reference: 5000.0 } }
}

struct Sweep { next: usize, side: f64, sides: f64, up: f64, ups: f64, sum: f64, hits: f64, nearest: f64 }
impl Default for Sweep {
    fn default() -> Self { Self { next: 0, side: 0.0, sides: 0.0, up: 0.0, ups: 0.0, sum: 0.0, hits: 0.0, nearest: 60.0 } }
}

pub struct Acoustics {
    rays: [Point; 19], sweep: Sweep, turn: usize,
    pub room: Room, pub enclosure: f64, pub mean_free: f64, pub roof: f64, pub nearest: f64, pub walls: f64,
    pub room_changed: bool,
    top: f64, pub ambience_gain: f64, pub foley_gain: f64,
}
impl Default for Acoustics {
    fn default() -> Self {
        let mut rays = [[0.0; 3]; 19];
        // ports Acoustics.constructor's fan: twelve horizontal rays, six
        // elevated rays and the roof. Directions never change during play.
        for (i, ray) in rays.iter_mut().enumerate().take(12) { let a = i as f64 / 12.0 * std::f64::consts::PI * 2.0; *ray = [v8math::sin(a), 0.0, v8math::cos(a)]; }
        for i in 0..6 {
            let a = i as f64 / 6.0 * std::f64::consts::PI * 2.0 + 0.3;
            let e = 50.0 * std::f64::consts::PI / 180.0;
            rays[12 + i] = [v8math::sin(a) * v8math::cos(e), v8math::sin(e), v8math::cos(a) * v8math::cos(e)];
        }
        rays[18] = [0.0, 1.0, 0.0];
        Self { rays, sweep: Sweep::default(), turn: 0, room: Room::default(), enclosure: 0.0, mean_free: 60.0, roof: 0.0, nearest: 60.0, walls: 0.0,
            room_changed: false, top: -120.0, ambience_gain: 1.0, foley_gain: 1.0 }
    }
}
impl Acoustics {
    /// ports Acoustics.probe. A partial fan continues as the listener moves;
    /// it is not restarted by motion, and the nineteenth ray ends a sweep.
    fn probe(&mut self, at: Point, collision: &mut impl Collision) {
        self.room_changed = false;
        let s = &mut self.sweep;
        for _ in 0..2 {
            if s.next >= self.rays.len() { break; }
            let index = s.next; s.next += 1;
            let hit = collision.cast(at, self.rays[index], 60.0); let d = hit.map_or(60.0, |h| h.distance);
            if hit.is_some() { s.sum += d; s.hits += 1.0; s.nearest = s.nearest.min(d); }
            if index < 12 { s.sides += 1.0; if hit.is_some() { s.side += 1.0; } }
            else { s.ups += 1.0; if hit.is_some() && d < 25.0 { s.up += 1.0; } }
        }
        if s.next < self.rays.len() { return; }
        self.walls = s.side / s.sides.max(1.0); self.roof = s.up / s.ups.max(1.0);
        self.enclosure = 0.6 * self.walls + 0.4 * self.roof;
        self.mean_free = if s.hits != 0.0 { s.sum / s.hits } else { 60.0 }; self.nearest = s.nearest;
        let size = (self.mean_free / 30.0).min(1.0); let r = &mut self.room;
        // Walls contribute early reflections. A roof controls the wetter,
        // longer tail instead of treating an open plaza as an indoor room.
        r.room = 0.04 + 0.22 * self.roof + 0.08 * self.walls;
        r.decay = 0.4 + (0.5 + 1.2 * size) * self.roof + 0.35 * self.walls * size;
        r.reflections = 0.15 + 0.55 * self.walls;
        r.refl_delay = (2.0 * self.nearest / 343.0).clamp(0.004, 0.09);
        r.reverb_delay = (r.refl_delay + 0.012 + 0.02 * size).min(0.12);
        r.hf_ratio = 0.5 + 0.2 * (1.0 - self.roof); r.room_hf = 0.7; r.density = 0.6 + 0.4 * self.enclosure;
        self.sweep = Sweep::default(); self.room_changed = true;
    }
    /// ports Acoustics.blocked. Offsets are horizontal and normalize only
    /// beyond the original small-span threshold; nearby voices bypass it.
    fn blocked(from: Point, at: Point, aim: f64, collision: &mut impl Collision) -> bool {
        let mut side = [at[2] - from[2], 0.0, from[0] - at[0]]; let span = length(side);
        side = if span > 0.001 { scale(side, 0.5 * aim / span) } else { [0.0; 3] };
        let target = [at[0] + side[0], at[1] + 1.0, at[2] + side[2]];
        let distance = length(sub(target, from)); let direction = scale(sub(target, from), 1.0 / distance.max(0.001));
        collision.cast(from, direction, distance).is_some_and(|h| h.distance < distance - 0.5)
    }
    /// ports Acoustics.update before Voice.measure. Positions and real
    /// flags intentionally describe the preceding audio frame here.
    pub fn update(&mut self, dt: f64, engine: &mut Engine, collision: &mut impl Collision) {
        self.probe(engine.listener.position, collision);
        let mut heard = [0; CAPACITY]; let mut count = 0;
        // The voice list's insertion order, rather than score order, drives
        // the rotating six-ray budget. Ties therefore remain reproducible.
        for k in 0..engine.order_len() {
            let id = engine.order_at(k); let v = &engine.voices[id];
            if v.real && v.mode3d && v.position.is_some() { heard[count] = id; count += 1; }
        }
        let mut rays = 0;
        for k in 0..count {
            if rays >= 6 { break; }
            let v = &mut engine.voices[heard[(self.turn + k) % count]];
            if v.distance <= 4.0 { v.occluded_target = 0.0; continue; }
            v.occlusion_aim = (v.occlusion_aim + 1) % 3;
            let aim = v.occlusion_aim as usize; let offset = [0.0, 1.0, -1.0][aim];
            v.occlusion_rays[aim] = Self::blocked(engine.listener.position, v.position.unwrap(), offset, collision) as u8 as f64;
            v.occluded_target = (v.occlusion_rays[0] + v.occlusion_rays[1] + v.occlusion_rays[2]) / 3.0; rays += 1;
        }
        self.turn = if count != 0 { (self.turn + rays.max(1)) % count } else { 0 };
        for &id in &heard[..count] {
            let v = &mut engine.voices[id]; let goal = v.occluded_target; let now = v.occluded.unwrap_or(goal);
            v.occluded = Some(now + (goal - now) * (6.0 * dt).min(1.0));
        }
    }
    /// ports LoudnessWindow.update. Only previously heard, unfinished voices
    /// contribute, and the top rises immediately but falls eight dB/second.
    pub fn loudness(&mut self, dt: f64, engine: &Engine) {
        let mut loudest: f64 = -120.0;
        for v in &engine.voices {
            if !v.real || v.done || v.audible <= 0.0 { continue; }
            let weight = match v.category { EXPLOSIONS => 6.0, WEAPONS | VOICE => 3.0, FOLEY => -3.0, AMBIENCE => -6.0, _ => 0.0 };
            let level = 20.0 * v8math::log10(v.audible) + weight; loudest = loudest.max(level);
        }
        self.top = if loudest > self.top { loudest } else { loudest.max(self.top - 8.0 * dt) };
        let depth = ((self.top + 12.0) / 12.0).clamp(0.0, 1.0);
        self.ambience_gain = v8math::pow_legacy(10.0, -6.0 * depth / 20.0);
        self.foley_gain = v8math::pow_legacy(10.0, -3.0 * depth / 20.0);
    }
    /// ports Acoustics.shape and GameAudio.enhancedShape. Distance shaping
    /// uses the new measurement, while occlusion keeps its budgeted history.
    pub fn shape(&self, category: usize, mode3d: bool, distance: f64, occluded: f64) -> Shape {
        let d = if mode3d { distance } else { 0.0 };
        let mut cutoff = if d > 20.0 { (20000.0 / (1.0 + (d - 20.0) / 70.0)).clamp(2500.0, 20000.0) } else { 20000.0 };
        if occluded > 0.0 { cutoff = cutoff * (1.0 - occluded) + cutoff.min(2800.0) * occluded; }
        let window = match category { AMBIENCE => self.ambience_gain, FOLEY => self.foley_gain, _ => 1.0 };
        Shape { cutoff, gain: (1.0 - (1.0 - 0.6) * occluded) * window, send: 1.0, lfe: if category == EXPLOSIONS { 1.0 } else { 0.0 }, spread: 0.0 }
    }
}
