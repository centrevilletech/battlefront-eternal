//! Why one kernel for all of it: the client used to push a quad per streak into
//! a particle batch, a light request per round and a mesh move per rocket from
//! JavaScript, one round at a time, every frame. The draw state is small and
//! entirely numeric, so it is computed here in one call and the host only
//! applies the results to the objects it owns (batches, meshes, trails, the
//! light pool, the sniper trace). Every number is computed in f64 in the
//! JavaScript's operation order, because the quads reach the GPU as f32 of
//! those doubles and a different last bit is a different pixel.
//!
//! Ordnance.present and the draw helpers it calls (drawBolt, drawBeam,
//! drawProjectileView, boltSpan, linearBoltFrame), plus the network
//! projectile presentation (ProjectilePresentation.receive/end/present/smooth)
//! as one coarse frame. The client writes a record for every round it wants
//! drawn, in draw order; the kernel returns the instanced quads of each
//! batch, the blaster capsules, and an ordered list of what the host still
//! does with its own objects (light requests, sniper trace touches, mesh and
//! trail moves). Arithmetic follows the JS operation by operation in f64, so
//! the quads the batches receive are the ones the JS pushed.
pub mod codec;
pub mod host;
pub mod style;
pub mod network;

use crate::ordnance_layout::{
    call_kind as ck, flight, net_command as nc, net_flag, net_kind as nk, net_type as nt,
    ordnance_call as cl, ordnance_flag as fl, ordnance_frame as fr, ordnance_header as hd,
    ordnance_kind as kind, ordnance_limits as lim, ordnance_record as rc, ordnance_status as status,
    ordnance_style as st, VERSION,
};
use three::{Quaternion, Vector3};
use v8math::{exp, hypot3, max, min};

/// The frame a view survives without being drawn before its state is
/// recycled: a bolt is drawn every frame it lives, and a clock run back
/// brings it back within a second or two.
const VIEW_WINDOW: u32 = 120;
/// MERGE: the metres over which a bolt drawn from the muzzle
/// joins its path.
const MERGE: f64 = 8.0;
/// CORE_TAPER and GLOW_TAPER: a streak's tail share of its width.
const CORE_TAPER: [f64; 3] = [0.5, 0.0, 0.0];
const GLOW_TAPER: [f64; 3] = [0.8, 0.0, 0.0];
/// STREAK.
const STREAK: f32 = 1.0;
/// A muzzle further than this (squared, 20 m) from its round is a respawn or
/// a seat, and is ignored.
const FAR_MUZZLE: f64 = 400.0;
/// constants.
const COAST_TICKS: f64 = 6.0;
const COAST_FULL_TICKS: f64 = 3.0;
const SMOOTH_SECONDS: f64 = 0.08;
const MAX_OFFSET: f64 = 6.0;
// The JavaScript keeps eight, enough while the display clock runs two ticks
// behind the newest sample. A slow frame takes in many ticks' samples at
// once and the clock then runs up to ten ticks behind: with eight the
// samples around the displayed tick were already dropped and the round was
// drawn ahead of where it was.
const SAMPLES: usize = 32;

/// JS `x || d` on a number.
#[inline(always)]
fn or(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        d
    } else {
        x
    }
}

/// A style is what the client knows about an ordnance class (numbers from its
/// definition, the blaster look, which batches draw it), registered once.
#[derive(Clone, Copy, Default)]
struct Style {
    // The batch handles the classic look draws into, -1 for none: a bolt's
    // core texture and the shared lightglow, and a beam's texture.
    core: i32,
    glow: i32,
    beam: i32,
    // The class's numbers them. `||` defaults (a
    // LaserWidth of 0 is "not named") are applied where they were, at draw.
    laser_width: f64,
    laser_length: f64,
    // `glowLength(def)`, which falls back to three times the core; and
    // `def.GlowLength` itself, which linearBoltFrame reads instead.
    glow_length: f64,
    glow_raw: f64,
    blur_length: f64,
    fade_out: f64,
    light_radius: f64,
    // Colours with their alpha folded in (color01).
    light_color: [f64; 3],
    glow_color: [f64; 3],
    // Ordnance.blasterLook, worked out by the client once the texture's hue is
    // known: the sheath colour and the capsule's proportions.
    look_color: [f64; 3],
    look_glow: f64,
    look_core: f64,
    look_length: f64,
    look_brightness: f64,
    look_light_radius: f64,
    // How a replicated round is followed past its last sample, and the
    // class's gravity for a shell.
    flight: u32,
    gravity: f64,
    // The width of a beam's second pass; 0 for a beam that draws only its core.
    beam_glow_width: f64,
    set: bool,
}

/// What the kernel keeps of a bolt between frames: ordnance.js' `presented`
/// (it has drawn once) and `seed` (its flicker phase).
#[derive(Clone, Copy)]
struct View {
    // NaN while the slot has never been used; lookups run past used slots.
    key: f64,
    // The frame the view was last drawn on, for recycling.
    stamp: u32,
    // NaN until the Enhanced look draws the bolt (`source.seed ??=`).
    seed: f64,
    presented: bool,
}
const UNUSED: f64 = f64::NAN;
const EMPTY_VIEW: View = View { key: UNUSED, stamp: 0, seed: f64::NAN, presented: false };

/// Open addressing over a fixed table. A view not drawn for VIEW_WINDOW
/// frames is stale and its slot is reused; a table three quarters full of
/// ever-used slots is rebuilt into the spare one.
// kept `presented` and `seed` as properties on each bolt object.
// Here a bolt is named by a number the client gives it (its simulation slot and
// generation, or a counter for the replicated and predicted ones), and the
// table is sized once: a long battle recycles views instead of growing.
struct Views {
    table: Vec<View>,
    spare: Vec<View>,
    used: usize,
    bits: u32,
}

impl Views {
    fn new(capacity: usize) -> Self {
        let size = (capacity * 2).next_power_of_two();
        Views {
            table: vec![EMPTY_VIEW; size],
            spare: vec![EMPTY_VIEW; size],
            used: 0,
            bits: size.trailing_zeros(),
        }
    }
    #[inline]
    // Fibonacci hashing: the keys are small counters with long runs, which a
    // plain modulo would pile into neighbouring slots.
    fn hash(&self, key: f64) -> usize {
        ((key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - self.bits)) as usize
    }
    // A slot not touched for VIEW_WINDOW frames is free for another bolt;
    // the wrapping subtraction keeps that true across the frame counter's wrap.
    fn live(v: &View, frame: u32) -> bool {
        frame.wrapping_sub(v.stamp) <= VIEW_WINDOW
    }
    // Open addressing never forgets a slot was used (a lookup must run past
    // it), so stale slots would eventually lengthen every probe. Copying the
    // live ones into the spare table, which was made with the first, clears
    // them without allocating.
    fn rebuild(&mut self, frame: u32) {
        for v in self.spare.iter_mut() {
            *v = EMPTY_VIEW;
        }
        let mask = self.table.len() - 1;
        let mut used = 0;
        for i in 0..self.table.len() {
            let v = self.table[i];
            if v.key.is_nan() || !Self::live(&v, frame) {
                continue;
            }
            let mut at = self.hash(v.key);
            while !self.spare[at].key.is_nan() {
                at = (at + 1) & mask;
            }
            self.spare[at] = v;
            used += 1;
        }
        std::mem::swap(&mut self.table, &mut self.spare);
        self.used = used;
    }
    /// The slot of `key`, made if it has none; None when the table is full
    /// of live views (the caller draws that round without persistent state).
    fn find(&mut self, key: f64, frame: u32) -> Option<usize> {
        // Keep the table at most three quarters of never-reused slots, so a
        // probe for an absent key finds an empty one quickly.
        if self.used * 4 > self.table.len() * 3 {
            self.rebuild(frame);
        }
        let mask = self.table.len() - 1;
        let mut at = self.hash(key);
        // The first stale slot met on the way: a new key may take it, but
        // only after the probe has proved the key is not further along.
        let mut reuse = usize::MAX;
        for _ in 0..self.table.len() {
            let v = self.table[at];
            if v.key.is_nan() {
                let slot = if reuse != usize::MAX { reuse } else { self.used += 1; at };
                self.table[slot] = View { key, stamp: frame, seed: f64::NAN, presented: false };
                return Some(slot);
            }
            if v.key == key {
                // The same bolt back after the window (a clock run back):
                // its old state is gone, as a recycled object's would be.
                if !Self::live(&v, frame) {
                    self.table[at] = View { key, stamp: frame, seed: f64::NAN, presented: false };
                }
                self.table[at].stamp = frame;
                return Some(at);
            }
            if reuse == usize::MAX && !Self::live(&v, frame) {
                reuse = at;
            }
            at = (at + 1) & mask;
        }
        // The whole table was probed: every slot is a live view of another
        // bolt. The caller draws this one without remembered state.
        if reuse != usize::MAX {
            self.table[reuse] = View { key, stamp: frame, seed: f64::NAN, presented: false };
            return Some(reuse);
        }
        None
    }
}

/// One sample of a network projectile (network-combat-projectile.js'
/// `samples` entry).
// ProjectilePresentation kept `samples` as an array of the events' objects and
// shifted the oldest off past eight. These are fixed arrays of the same eight.
#[derive(Clone, Copy, Default)]
struct Sample {
    tick: f64,
    expiry: f64,
    life: f64,
    position: Vector3,
    velocity: Vector3,
    quaternion: [f64; 4],
    end: Vector3,
    changed: bool,
}

/// A network projectile (ProjectilePresentation's `objects` entry), by the
/// slot the client gave it.
#[derive(Clone, Copy)]
struct Net {
    active: bool,
    // The live projectiles in the order they arrived: the JavaScript's Map
    // iterated in insertion order, and that order is the draw order.
    next: u32,
    prev: u32,
    style: u32,
    kind: u32,
    flags: u32,
    generation: f64,
    // The earliest tick a terminal record named (`endTick`); the projectile
    // is gone once the clock reaches it.
    end_tick: f64,
    len: usize,
    samples: [Sample; SAMPLES],
    // The correction hiding a new sample, which fades over SMOOTH_SECONDS,
    // and what was shown last frame (`p.shown`) to carry it from.
    offset: Vector3,
    shown: bool,
    shown_raw: Vector3,
    shown_velocity: Vector3,
    shown_tick: f64,
    shown_latest: f64,
}
const NONE: u32 = u32::MAX;

impl Net {
    fn empty() -> Self {
        Net {
            active: false,
            next: NONE,
            prev: NONE,
            style: 0,
            kind: 0,
            flags: 0,
            generation: 0.0,
            end_tick: f64::INFINITY,
            len: 0,
            samples: [Sample::default(); SAMPLES],
            offset: Vector3::default(),
            shown: false,
            shown_raw: Vector3::default(),
            shown_velocity: Vector3::default(),
            shown_tick: 0.0,
            shown_latest: 0.0,
        }
    }
}

// The outputs sit in one struct apart from the styles and views so a draw can
// hold all three at once: the borrow checker would not allow `&mut self`
// methods to touch a style and push a quad in the same expression.
/// Everything a frame produces, kept apart from the styles and views so a
/// draw can borrow them all at once.
struct Out {
    // Host work, in order (one row of `ordnance_call` each).
    calls: Vec<f64>,
    call_count: usize,
    dropped_calls: usize,
    // Quads per batch handle, and the six attribute arrays they fill, in
    // particles.js' attribute order. A handle's region is QUADS_PER_HANDLE
    // quads long; a quad that does not fit is dropped and counted.
    counts: Vec<u32>,
    dropped_quads: usize,
    position: Vec<f32>,
    color: Vec<f32>,
    shape: Vec<f32>,
    axis: Vec<f32>,
    axis2: Vec<f32>,
    frame: Vec<f32>,
    // The Enhanced look's capsules, which all share one batch (BlasterBatch).
    blasters: usize,
    dropped_blasters: usize,
    head: Vec<f32>,
    tail: Vec<f32>,
    blaster_color: Vec<f32>,
    size: Vec<f32>,
    /// `boltAxis`, the scratch direction boltSpan leaves.
    bolt_axis: [f64; 3],
}

impl Out {
    /// `style.core/glow/beam.push(...)`: one quad into batch `handle`'s
    /// region. A handle past the table, or a full region, drops the quad.
    #[allow(clippy::too_many_arguments)]
    // The region per batch handle is fixed, not packed: the client copies each
    // handle's quads to its batch with one typed-array set per attribute,
    // and no sorting by batch is needed to keep each batch's order as the
    // JavaScript pushed it.
    fn quad(
        &mut self,
        handle: i32,
        p: &Vector3,
        rgba: [f64; 4],
        size: f64,
        length: f64,
        axis: [f64; 3],
        axis2: Option<[f64; 3]>,
    ) {
        if handle < 0 || handle as usize >= lim::HANDLES {
            return;
        }
        let h = handle as usize;
        let n = self.counts[h] as usize;
        if n >= lim::QUADS_PER_HANDLE {
            self.dropped_quads += 1;
            return;
        }
        self.counts[h] += 1;
        let i = h * lim::QUADS_PER_HANDLE + n;
        self.position[i * 3] = p.x as f32;
        self.position[i * 3 + 1] = p.y as f32;
        self.position[i * 3 + 2] = p.z as f32;
        // The doubles are cut to f32 here, once, as Float32Array would on
        // the JavaScript push (round to nearest even).
        for (k, &c) in rgba.iter().enumerate() {
            self.color[i * 4 + k] = c as f32;
        }
        self.shape[i * 4] = size as f32;
        self.shape[i * 4 + 1] = 0.0;
        self.shape[i * 4 + 2] = length as f32;
        self.shape[i * 4 + 3] = STREAK;
        for (k, &ax) in axis.iter().enumerate() {
            self.axis[i * 3 + k] = ax as f32;
        }
        let a2 = axis2.unwrap_or([1.0, 0.0, 0.0]);
        for (k, &ax2) in a2.iter().enumerate() {
            self.axis2[i * 3 + k] = ax2 as f32;
        }
        // Particle batches show the whole texture: frame [0, 0, 1, 1]. A
        // streak's axis2 is its tail's share of the width, then two zeros.
        self.frame[i * 4] = 0.0;
        self.frame[i * 4 + 1] = 0.0;
        self.frame[i * 4 + 2] = 1.0;
        self.frame[i * 4 + 3] = 1.0;
    }

    /// `BlasterBatch.push(head, tail, rgb, brightness, core, glow, seed, taper)`.
    #[allow(clippy::too_many_arguments)]
    // The client's BlasterBatch has one array set for every Enhanced bolt and
    // beam, in the order they were drawn.
    fn blaster(
        &mut self,
        head: &Vector3,
        tail: &Vector3,
        rgb: [f64; 3],
        brightness: f64,
        core: f64,
        glow: f64,
        seed: f64,
        taper: f64,
    ) {
        if self.blasters >= lim::BLASTERS {
            self.dropped_blasters += 1;
            return;
        }
        let i = self.blasters;
        self.blasters += 1;
        self.head[i * 3] = head.x as f32;
        self.head[i * 3 + 1] = head.y as f32;
        self.head[i * 3 + 2] = head.z as f32;
        self.tail[i * 3] = tail.x as f32;
        self.tail[i * 3 + 1] = tail.y as f32;
        self.tail[i * 3 + 2] = tail.z as f32;
        self.blaster_color[i * 4] = rgb[0] as f32;
        self.blaster_color[i * 4 + 1] = rgb[1] as f32;
        self.blaster_color[i * 4 + 2] = rgb[2] as f32;
        self.blaster_color[i * 4 + 3] = brightness as f32;
        self.size[i * 4] = core as f32;
        self.size[i * 4 + 1] = glow as f32;
        self.size[i * 4 + 2] = seed as f32;
        self.size[i * 4 + 3] = taper as f32;
    }

    /// The next call row, zeroed, or None past the table.
    // The ordered list of host work: its order is the JavaScript's call
    // order, so light requests and sniper touches interleave as they did
    // (LightPool keeps equal weights in the order asked).
    fn call(&mut self, kind: u32, source: f64) -> Option<usize> {
        if self.call_count >= lim::CALLS {
            self.dropped_calls += 1;
            return None;
        }
        let at = self.call_count * cl::STRIDE;
        self.call_count += 1;
        let row = &mut self.calls[at..at + cl::STRIDE];
        for v in row.iter_mut() {
            *v = 0.0;
        }
        row[cl::KIND] = kind as f64;
        row[cl::SOURCE] = source;
        Some(at)
    }

    fn put3(&mut self, at: usize, field: usize, v: &Vector3) {
        self.calls[at + field] = v.x;
        self.calls[at + field + 1] = v.y;
        self.calls[at + field + 2] = v.z;
    }

    /// `LightPool.add(position, rgb, radius, scale)`: refused for a radius
    /// at or below zero, as the pool does.
    fn light(&mut self, p: &Vector3, rgb: [f64; 3], radius: f64) {
        // A NaN radius passes (`NaN <= 0` is false), as it did in the pool.
        if radius <= 0.0 {
            return;
        }
        if let Some(at) = self.call(ck::LIGHT, 0.0) {
            self.put3(at, cl::A, p);
            self.calls[at + cl::B] = rgb[0];
            self.calls[at + cl::B + 1] = rgb[1];
            self.calls[at + cl::B + 2] = rgb[2];
            self.calls[at + cl::S0] = radius;
            self.calls[at + cl::S1] = 1.0;
        }
    }

    /// boltSpan: the visible path converges from the displayed
    /// muzzle to the gameplay ray; both ends of the streak are computed on
    /// it. Leaves the streak's axis in `bolt_axis` and returns its length.
    fn bolt_span(&mut self, b: &BoltDraw, span_in: f64) -> f64 {
        let mut span = span_in;
        // A bolt first drawn after it has already flown several metres
        // (it was fired between frames) shows that stretch back to the muzzle.
        if b.first_draw {
            span = max(span, b.flown);
        }
        // How much of the muzzle-to-path offset is still left at the head and
        // at the tail: the streak bends from the drawn muzzle onto the ray
        // over `merge` metres of flight, and the part of the offset between
        // the two ends tilts it.
        let head = max(1.0 - b.flown / b.merge, 0.0);
        let tail = max(1.0 - max(0.0, b.flown - span) / b.merge, 0.0);
        let offset = head - tail;
        let (ox, oy, oz) = match &b.offset {
            Some(o) => (or(o.x, 0.0), or(o.y, 0.0), or(o.z, 0.0)),
            None => (0.0, 0.0, 0.0),
        };
        let mut dir = [
            b.direction.x * span + ox * offset,
            b.direction.y * span + oy * offset,
            b.direction.z * span + oz * offset,
        ];
        // Math.hypot(...dir), as V8 computes it: the compensated sum is not
        // sqrt(x*x+y*y+z*z) in the last bit.
        let length = hypot3(dir[0], dir[1], dir[2]);
        if length > 1e-8 {
            dir[0] /= length;
            dir[1] /= length;
            dir[2] /= length;
        } else {
            dir = [b.direction.x, b.direction.y, b.direction.z];
        }
        self.bolt_axis = dir;
        length
    }
}

/// The parts of a bolt drawBolt reads once `Object.assign(bolt, source)`
/// and the interpolation have run.
struct BoltDraw {
    // The interpolated (or linear-frame) distances, not the source's own.
    flown: f64,
    drawn: f64,
    merge: f64,
    direction: Vector3,
    // Where the drawn muzzle sits against the ray, or none for a bolt drawn
    // exactly on it.
    offset: Option<Vector3>,
    first_draw: bool,
}

/// boltFade: all of a bolt in flight; after its end a class
/// with a FadeOutTime fades linearly, and any other is gone at once.
fn bolt_fade(fade_out: f64, after: f64) -> f64 {
    if after < 0.0 {
        return 1.0;
    }
    let time = or(fade_out, 0.0);
    if time > 0.0 {
        max(0.0, 1.0 - after / time)
    } else {
        0.0
    }
}

// The client writes into `input`, `net_input` and `style_input` and reads the
// rest through the pointers the client binding gives it; a native host
// (the replay tool, the tests) uses these fields directly. Everything is
// sized once, so a frame never allocates.
pub struct Ordnance {
    pub host: host::Host,
    /// Frame header, then `records` rows of `ordnance_record::STRIDE`.
    pub input: Vec<f64>,
    /// Network commands, in arrival order.
    pub net_input: Vec<f64>,
    pub header: Vec<f64>,
    /// A style's row, written by the client before `set_style`.
    pub style_input: Vec<f64>,
    styles: Vec<Style>,
    views: Views,
    net: Vec<Net>,
    net_head: u32,
    net_tail: u32,
    net_live: usize,
    refused_net: usize,
    skipped_net: usize,
    refused_views: usize,
    out: Out,
    frame: u32,
    /// `boltSeed`.
    bolt_seed: f64,
}

impl Default for Ordnance {
    fn default() -> Self {
        Self::new()
    }
}

impl Ordnance {
    pub fn new() -> Self {
        let quads = lim::HANDLES * lim::QUADS_PER_HANDLE;
        Ordnance {
            host: host::Host::new(),
            input: vec![0.0; fr::STRIDE + lim::RECORDS * rc::STRIDE],
            net_input: vec![0.0; lim::NET_COMMANDS * nc::STRIDE],
            header: vec![0.0; hd::STRIDE],
            style_input: vec![0.0; st::STRIDE],
            styles: vec![Style::default(); lim::STYLES],
            views: Views::new(lim::VIEWS),
            net: vec![Net::empty(); lim::NET_SLOTS],
            net_head: NONE,
            net_tail: NONE,
            net_live: 0,
            refused_net: 0,
            skipped_net: 0,
            refused_views: 0,
            out: Out {
                calls: vec![0.0; lim::CALLS * cl::STRIDE],
                call_count: 0,
                dropped_calls: 0,
                counts: vec![0; lim::HANDLES],
                dropped_quads: 0,
                position: vec![0.0; quads * 3],
                color: vec![0.0; quads * 4],
                shape: vec![0.0; quads * 4],
                axis: vec![0.0; quads * 3],
                axis2: vec![0.0; quads * 3],
                frame: vec![0.0; quads * 4],
                blasters: 0,
                dropped_blasters: 0,
                head: vec![0.0; lim::BLASTERS * 3],
                tail: vec![0.0; lim::BLASTERS * 3],
                blaster_color: vec![0.0; lim::BLASTERS * 4],
                size: vec![0.0; lim::BLASTERS * 4],
                bolt_axis: [0.0, 0.0, 1.0],
            },
            frame: 0,
            bolt_seed: 0.0,
        }
    }

    pub fn calls(&self) -> &[f64] {
        &self.out.calls
    }
    pub fn counts(&self) -> &[u32] {
        &self.out.counts
    }
    pub fn quad_arrays(&self) -> [&[f32]; 6] {
        let o = &self.out;
        [&o.position, &o.color, &o.shape, &o.axis, &o.axis2, &o.frame]
    }
    pub fn blaster_arrays(&self) -> [&[f32]; 4] {
        let o = &self.out;
        [&o.head, &o.tail, &o.blaster_color, &o.size]
    }

    /// Registers the class whose numbers are in `style_input` as style
    /// `index` (the client registers a class once, and again when a texture
    /// hue arrives); a bad index is ignored.
    pub fn set_style(&mut self, index: usize) {
        if index >= lim::STYLES {
            return;
        }
        // The row was written by the client just before this call (a class's
        // numbers, or the same class again when a texture's hue arrives).
        let s = &self.style_input;
        let v3 = |at: usize| [s[at], s[at + 1], s[at + 2]];
        self.styles[index] = Style {
            core: s[st::CORE_BATCH] as i32,
            glow: s[st::GLOW_BATCH] as i32,
            beam: s[st::BEAM_BATCH] as i32,
            laser_width: s[st::LASER_WIDTH],
            laser_length: s[st::LASER_LENGTH],
            glow_length: s[st::GLOW_LENGTH],
            glow_raw: s[st::GLOW_RAW],
            blur_length: s[st::BLUR_LENGTH],
            fade_out: s[st::FADE_OUT_TIME],
            light_radius: s[st::LIGHT_RADIUS],
            light_color: v3(st::LIGHT_COLOR),
            glow_color: v3(st::GLOW_COLOR),
            look_color: v3(st::LOOK_COLOR),
            look_glow: s[st::LOOK_GLOW],
            look_core: s[st::LOOK_CORE],
            look_length: s[st::LOOK_LENGTH],
            look_brightness: s[st::LOOK_BRIGHTNESS],
            look_light_radius: s[st::LOOK_LIGHT_RADIUS],
            flight: s[st::FLIGHT] as u32,
            gravity: s[st::GRAVITY],
            beam_glow_width: s[st::BEAM_GLOW_WIDTH],
            set: true,
        };
    }

    /// The style-input row a set style was made from, for a client deriving
    /// another style from it. None for a style never set.
    pub fn style_row(&self, index: usize) -> Option<[f64; st::STRIDE]> {
        let s = self.styles.get(index).filter(|s| s.set)?;
        let mut row = [0.0; st::STRIDE];
        row[st::CORE_BATCH] = s.core as f64;
        row[st::GLOW_BATCH] = s.glow as f64;
        row[st::BEAM_BATCH] = s.beam as f64;
        row[st::LASER_WIDTH] = s.laser_width;
        row[st::LASER_LENGTH] = s.laser_length;
        row[st::GLOW_LENGTH] = s.glow_length;
        row[st::GLOW_RAW] = s.glow_raw;
        row[st::BLUR_LENGTH] = s.blur_length;
        row[st::FADE_OUT_TIME] = s.fade_out;
        row[st::LIGHT_RADIUS] = s.light_radius;
        row[st::LIGHT_COLOR..st::LIGHT_COLOR + 3].copy_from_slice(&s.light_color);
        row[st::GLOW_COLOR..st::GLOW_COLOR + 3].copy_from_slice(&s.glow_color);
        row[st::LOOK_COLOR..st::LOOK_COLOR + 3].copy_from_slice(&s.look_color);
        row[st::LOOK_GLOW] = s.look_glow;
        row[st::LOOK_CORE] = s.look_core;
        row[st::LOOK_LENGTH] = s.look_length;
        row[st::LOOK_BRIGHTNESS] = s.look_brightness;
        row[st::LOOK_LIGHT_RADIUS] = s.look_light_radius;
        row[st::FLIGHT] = f64::from(s.flight);
        row[st::GRAVITY] = s.gravity;
        row[st::BEAM_GLOW_WIDTH] = s.beam_glow_width;
        Some(row)
    }

    /// The frame: net commands, then every record in order (Ordnance.present's
    /// beams, bolts, rounds and grenades, then whatever `additional` drew),
    /// then the network projectiles. A frame never fails: a full table drops
    /// what it cannot hold and counts it in the header.
    pub fn present(&mut self) {
        // A client built against another layout would read garbage rows; it is
        // refused whole and the client reports it, which is a build error.
        let version_ok = self.input[fr::VERSION] == VERSION as f64;
        self.header[hd::VERSION] = VERSION as f64;
        self.header[hd::STATUS] = if version_ok { status::OK } else { status::VERSION_MISMATCH } as f64;
        self.frame = self.frame.wrapping_add(1);
        // The outputs are the frame's alone; nothing from the last one stays.
        let o = &mut self.out;
        o.call_count = 0;
        o.dropped_calls = 0;
        o.blasters = 0;
        o.dropped_blasters = 0;
        o.dropped_quads = 0;
        for c in o.counts.iter_mut() {
            *c = 0;
        }
        if version_ok {
            // Commands first: a sample received this frame belongs to the
            // present that follows, as `receive` ran before `present`.
            self.net_flush(clamp_count(self.input[fr::NET_COMMANDS], lim::NET_COMMANDS));
            let enhanced = self.input[fr::ENHANCED_BOLTS] != 0.0;
            let sniper = self.input[fr::ENHANCED_SNIPER] != 0.0;
            // A count the host wrote is held to the block: this is the
            // boundary where a bad number would otherwise index past it.
            let records = clamp_count(self.input[fr::RECORDS], lim::RECORDS);
            for n in 0..records {
                self.record(fr::STRIDE + n * rc::STRIDE, enhanced, sniper);
            }
        }
        // The header tells the host how much to read and what was refused, so
        // the client can report a crowded frame once instead of failing it.
        let o = &self.out;
        self.header[hd::CALLS] = o.call_count as f64;
        self.header[hd::BLASTERS] = o.blasters as f64;
        self.header[hd::DROPPED_QUADS] = o.dropped_quads as f64;
        self.header[hd::DROPPED_BLASTERS] = o.dropped_blasters as f64;
        self.header[hd::DROPPED_CALLS] = o.dropped_calls as f64;
        self.header[hd::VIEWS] = self.views.used as f64;
        self.header[hd::REFUSED_VIEWS] = self.refused_views as f64;
        self.header[hd::NET_LIVE] = self.net_live as f64;
        self.header[hd::REFUSED_NET] = self.refused_net as f64;
        self.header[hd::SKIPPED_NET] = self.skipped_net as f64;
    }

    /// Applies the first `count` network commands, in order. The client calls
    /// this itself when its block fills between frames (a long load or a
    /// pause with events but no update), so no command is ever lost.
    pub fn net_flush(&mut self, count: usize) {
        for n in 0..count.min(lim::NET_COMMANDS) {
            self.net_command(n * nc::STRIDE);
        }
    }

    // A record or sample naming a class the client never registered draws
    // nothing (it cannot be told how).
    fn style(&self, index: f64) -> Option<Style> {
        let i = index as usize;
        if index >= 0.0 && i < self.styles.len() && self.styles[i].set {
            Some(self.styles[i])
        } else {
            None
        }
    }

    fn record(&mut self, at: usize, enhanced: bool, sniper: bool) {
        // One copy of the row, so the draw works on stable numbers while it
        // writes outputs (a plain array on the stack, no allocation).
        let r: [f64; rc::STRIDE] = {
            let mut row = [0.0; rc::STRIDE];
            row.copy_from_slice(&self.input[at..at + rc::STRIDE]);
            row
        };
        if r[rc::KIND] as u32 == kind::NET {
            // Where ProjectilePresentation.present ran among the draws, so
            // its batch pushes and lights keep their order.
            self.present_net(r[rc::CLOCK_TICK], enhanced);
            return;
        }
        // Flags are named bits (the layout's `OrdnanceFlag`): which optional
        // fields of the row the client filled this frame.
        let flags = r[rc::FLAGS] as u32;
        let has = |f: u32| flags & (1 << f) != 0;
        let v3 = |k: usize| Vector3::new(r[k], r[k + 1], r[k + 2]);
        if r[rc::KIND] as u32 == kind::GRENADE {
            // Ordnance.present: a grenade's mesh between its last two steps.
            if has(fl::HAS_MESH) {
                let mut p = Vector3::default();
                p.lerp_vectors(&v3(rc::PREVIOUS), &v3(rc::POSITION), r[rc::ALPHA]);
                if let Some(c) = self.out.call(ck::MESH, r[rc::SOURCE]) {
                    self.out.put3(c, cl::A, &p);
                }
            }
            return;
        }
        let Some(style) = self.style(r[rc::STYLE]) else {
            return;
        };
        let k = r[rc::KIND] as u32;
        // The kinds map to the client's four lists: Ordnance.present drew
        // `beams`, `bolts`, `rounds` and `grenades` in that order, and the
        // page writes its records in the same order.
        if k == kind::BEAM {
            let direction = v3(rc::DIRECTION);
            self.draw_beam(&style, &v3(rc::POSITION), &v3(rc::END), &direction, r[rc::LIFE], enhanced);
        } else if k == kind::BOLT {
            self.draw_bolt(&r, &style, flags, enhanced, sniper);
        } else if k == kind::ROUND {
            let mut offset = v3(rc::OFFSET);
            let mut has_offset = has(fl::HAS_MUZZLE_OFFSET);
            // A rocket leaves the drawn muzzle and joins its path over MERGE
            // metres, as a bolt does; its offset is kept on the round and
            // given back each frame. Ordnance.present: a round's drawn muzzle, resolved once against
            // where it was launched; far from it (a respawn) is ignored.
            if has(fl::RESOLVE_ROUND) && has(fl::HAS_LAUNCH) {
                let m = v3(rc::MUZZLE);
                offset = Vector3::new(m.x, m.y, m.z);
                offset.sub(&v3(rc::LAUNCH));
                if offset.length_sq() > FAR_MUZZLE {
                    offset.set(0.0, 0.0, 0.0);
                }
                has_offset = true;
                if let Some(c) = self.out.call(ck::OFFSET, r[rc::SOURCE]) {
                    self.out.put3(c, cl::A, &offset);
                    self.out.calls[c + cl::S0] = 1.0;
                }
            }
            // The optional parts of a round, passed on as the draw reads them.
            let launch = if has(fl::HAS_LAUNCH) { Some(v3(rc::LAUNCH)) } else { None };
            let q = if has(fl::HAS_QUATERNION) {
                Some(Quaternion::new(r[rc::QUATERNION], r[rc::QUATERNION + 1], r[rc::QUATERNION + 2], r[rc::QUATERNION + 3]))
            } else {
                None
            };
            let velocity = if has(fl::HAS_VELOCITY) { Some(v3(rc::VELOCITY)) } else { None };
            let muzzle = if has_offset { Some(offset) } else { None };
            self.draw_projectile(
                &style,
                r[rc::SOURCE],
                &v3(rc::PREVIOUS),
                &v3(rc::POSITION),
                r[rc::ALPHA],
                muzzle.as_ref(),
                launch.as_ref(),
                q.as_ref(),
                velocity.as_ref(),
                has(fl::HAS_MESH),
                has(fl::HAS_TRAIL),
                false,
            );
        }
    }

    /// drawBeam.
    // A beam is a straight fat ray from its source to where it struck; it
    // fades over its last 0.08 s (or over FadeOutTime after its end when set).
    // Classic draws one textured streak at the far end back along the ray;
    // Enhanced draws a capsule from end to source.
    fn draw_beam(&mut self, s: &Style, position: &Vector3, end: &Vector3, direction: &Vector3, life: f64, enhanced: bool) {
        let fade = min(1.0, max(0.0, life) / 0.08);
        self.draw_beam_fade(s, position, end, direction, fade, enhanced);
    }

    fn draw_beam_fade(&mut self, s: &Style, position: &Vector3, end: &Vector3, direction: &Vector3, fade: f64, enhanced: bool) {
        let length = position.distance_to(end);
        if enhanced {
            let c = s.look_color;
            self.out
                .blaster(end, position, c, s.look_brightness * fade, s.look_core, s.look_glow, 0.37, 1.0);
            self.out
                .light(position, [c[0] * 0.75, c[1] * 0.75, c[2] * 0.75], s.look_light_radius);
            return;
        }
        let width = or(s.laser_width, 0.3) / 2.0;
        self.out.quad(
            s.beam,
            end,
            [1.0, 1.0, 1.0, fade],
            width,
            length,
            [direction.x, direction.y, direction.z],
            None,
        );
        self.draw_beam_glow(s, end, length, direction, fade);
        self.out.light(position, s.glow_color, or(s.light_radius, 3.0));
    }

    /// The second pass of a 2005 beam in the Classic look, drawn over the core pass for a beam of
    /// the battle's own rounds and for a replicated one alike: a strip of the shared glow texture
    /// along the same ray, as wide as the class's glow width, in its glow colour (which already
    /// holds the colour's own alpha), fading with the core. A style that names no glow width,
    /// which is every 2004 beam, draws nothing here.
    fn draw_beam_glow(&mut self, s: &Style, end: &Vector3, length: f64, direction: &Vector3, fade: f64) {
        if s.glow < 0 || s.beam_glow_width.is_nan() || s.beam_glow_width <= 0.0 {
            return;
        }
        let [r, g, b] = s.glow_color;
        self.out.quad(
            s.glow,
            end,
            [r, g, b, fade],
            s.beam_glow_width / 2.0,
            length,
            [direction.x, direction.y, direction.z],
            None,
        );
    }

    /// drawProjectileView (and, for a network projectile, the
    /// same call with alpha 1 and no launch). `wants_light` is its third
    /// argument.
    #[allow(clippy::too_many_arguments)]
    // A rocket, shell or missile is a model flown dead along its path with a
    // trail effect at its tail; the draw is only where that model and trail
    // are put this frame, and (for a replicated one) its light.
    fn draw_projectile(
        &mut self,
        s: &Style,
        source: f64,
        previous: &Vector3,
        position: &Vector3,
        alpha: f64,
        muzzle_offset: Option<&Vector3>,
        launch: Option<&Vector3>,
        quaternion: Option<&Quaternion>,
        velocity: Option<&Vector3>,
        mesh: bool,
        trail: bool,
        wants_light: bool,
    ) {
        let mut drawn = Vector3::default();
        drawn.lerp_vectors(previous, position, alpha);
        if let Some(offset) = muzzle_offset {
            let away = launch.map_or(0.0, |l| position.distance_to(l));
            drawn.add_scaled_vector(offset, max(0.0, 1.0 - away / MERGE));
        }
        if mesh {
            self.mesh_call(ck::MESH, source, &drawn, quaternion);
        }
        if trail {
            if let Some(c) = self.out.call(ck::TRAIL, source) {
                self.out.put3(c, cl::A, &drawn);
                if let Some(q) = quaternion {
                    self.put_quaternion(c, q);
                    self.out.calls[c + cl::S0] = 1.0;
                }
                if let Some(v) = velocity {
                    self.out.put3(c, cl::E, v);
                    self.out.calls[c + cl::S1] = 1.0;
                }
            }
        }
        if wants_light {
            self.out.light(&drawn, s.light_color, or(s.light_radius, 0.0));
        }
    }

    fn put_quaternion(&mut self, at: usize, q: &Quaternion) {
        self.out.calls[at + cl::D] = q.x;
        self.out.calls[at + cl::D + 1] = q.y;
        self.out.calls[at + cl::D + 2] = q.z;
        self.out.calls[at + cl::D + 3] = q.w;
    }

    // The client moves a mesh with this call; `q` is absent for a model that
    // keeps its orientation (a grenade, which only moves).
    fn mesh_call(&mut self, kind: u32, source: f64, p: &Vector3, q: Option<&Quaternion>) {
        if let Some(c) = self.out.call(kind, source) {
            self.out.put3(c, cl::A, p);
            if let Some(q) = q {
                self.put_quaternion(c, q);
                self.out.calls[c + cl::S0] = 1.0;
            }
        }
    }

    /// drawBolt, with linear-bolt.js linearBoltFrame for a
    /// replicated bolt flown on the playout clock.
    fn draw_bolt(&mut self, r: &[f64; rc::STRIDE], s: &Style, flags: u32, enhanced: bool, sniper: bool) {
        let has = |f: u32| flags & (1 << f) != 0;
        let v3 = |k: usize| Vector3::new(r[k], r[k + 1], r[k + 2]);
        let source_flown = r[rc::FLOWN];
        let position = v3(rc::POSITION);
        let direction = v3(rc::DIRECTION);
        let mut offset = if has(fl::HAS_OFFSET) { Some(v3(rc::OFFSET)) } else { None };
        // Ordnance.present / CombatPresentation.present: the gun as drawn
        // now, taken once at the bolt's first draw, and ignored when far.
        if has(fl::RESOLVE_BOLT) || has(fl::RESOLVE_SHOWN) {
            let mut o = v3(rc::MUZZLE);
            o.sub(&position);
            if has(fl::RESOLVE_BOLT) {
                o.add_scaled_vector(&direction, source_flown);
            }
            if o.length_sq() > FAR_MUZZLE {
                o.set(0.0, 0.0, 0.0);
            }
            if let Some(c) = self.out.call(ck::OFFSET, r[rc::SOURCE]) {
                self.out.put3(c, cl::A, &o);
            }
            offset = Some(o);
        }
        // Two ways a bolt's extent is known. A replicated bolt carries only its
        // launch sample, and its flight is worked out from the displayed tick
        // (linearBoltFrame). A bolt the client steps carries
        // this and the previous step's distances, drawn between them at alpha.
        let (flown, drawn, ended, fade);
        if has(fl::LINEAR) {
            // linearBoltFrame(source, clockTick ?? networkFlightTick): the
            // bolt's flight at the displayed tick; nothing before it starts.
            let tick = if has(fl::HAS_CLOCK) { r[rc::CLOCK_TICK] } else { r[rc::FLIGHT_TICK] };
            if !r[rc::FLIGHT_TICK].is_finite() || !tick.is_finite() {
                return;
            }
            let elapsed = (tick - r[rc::FLIGHT_TICK]) / 60.0;
            let f = bolt_fade(s.fade_out, elapsed - r[rc::LIFE]);
            if source_flown + r[rc::SPEED] * elapsed < 0.0 || f.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return;
            }
            flown = source_flown + r[rc::SPEED] * min(elapsed, r[rc::LIFE]);
            drawn = min(
                flown,
                max(max(or(s.laser_length, 1.0), or(s.glow_raw, 0.0)), or(s.blur_length, 0.0)),
            );
            ended = elapsed >= r[rc::LIFE];
            fade = f;
        } else {
            let pf = if has(fl::HAS_PREVIOUS_FLOWN) { r[rc::PREVIOUS_FLOWN] } else { source_flown };
            let pd = if has(fl::HAS_PREVIOUS_DRAWN) { r[rc::PREVIOUS_DRAWN] } else { r[rc::DRAWN] };
            flown = pf + (source_flown - pf) * r[rc::ALPHA];
            drawn = pd + (r[rc::DRAWN] - pd) * r[rc::ALPHA];
            ended = r[rc::ENDED] != 0.0;
            fade = r[rc::FADE];
        }
        // A sniper round that has stopped fades and thins to a tenth of its
        // width (0.9 x fade + 0.1), as the 2004 laser did.
        let thin = 0.9 * fade + 0.1;
        // The view: has this bolt drawn before, and its flicker phase.
        let slot = self.views.find(r[rc::KEY], self.frame);
        if slot.is_none() {
            self.refused_views += 1;
        }
        let presented = slot.is_some_and(|i| self.views.table[i].presented);
        // A fast bolt can travel several metres before its first rendered
        // frame: show that first interval back to the muzzle.
        let first_draw = !presented && !ended;
        if flown > 1e-8 {
            if let Some(i) = slot {
                self.views.table[i].presented = true;
            }
        }
        let bolt = BoltDraw { flown, drawn, merge: r[rc::MERGE], direction, offset, first_draw };
        // The head: the source's position moved to the interpolated flight,
        // then the drawn muzzle's offset, which shrinks to nothing as the
        // bolt flies MERGE metres (it joins the ray; it never moves the ray).
        let mut p = position;
        p.add_scaled_vector(&direction, flown - source_flown);
        if let Some(o) = &bolt.offset {
            p.add_scaled_vector(o, max(1.0 - flown / bolt.merge, 0.0));
        }
        // The light rides at the head; once the flight is over it stays lit
        // at the middle of the last step until the bolt is gone.
        let light_at = if ended {
            let mut l = p;
            l.add_scaled_vector(&direction, -min(flown, r[rc::SPEED] / 60.0) / 2.0);
            l
        } else {
            p
        };
        // Enhanced sniper shots: a rail from the gun as drawn to where the
        // round is, which the host's trace object draws and fades.
        if sniper && s.fade_out > 0.0 {
            let mut origin = position;
            origin.add_scaled_vector(&direction, -source_flown);
            let mut start = origin;
            if let Some(o) = &bolt.offset {
                start.add(o);
            }
            let mut head = origin;
            head.add_scaled_vector(&direction, flown);
            if let Some(c) = self.out.call(ck::TOUCH, r[rc::SOURCE]) {
                self.out.put3(c, cl::A, &start);
                self.out.put3(c, cl::B, &head);
                self.out.put3(c, cl::C, &direction);
                self.out.put3(c, cl::E, &origin);
                self.out.calls[c + cl::S0] = ended as u8 as f64;
                self.out.calls[c + cl::S1] = fade;
            }
            return;
        }
        // Enhanced bolts: one capsule from head back to tail, in the shared
        // blaster batch.
        if enhanced {
            let length = self.out.bolt_span(&bolt, min(s.look_length, bolt.drawn));
            let axis = self.out.bolt_axis;
            let mut tail = Vector3::new(axis[0], axis[1], axis[2]);
            tail.multiply_scalar(-length).add(&p);
            // Presentation only: a stable flicker phase for each bolt.
            let seed = match slot {
                Some(i) if !self.views.table[i].seed.is_nan() => self.views.table[i].seed,
                other => {
                    self.bolt_seed = ((self.bolt_seed) + 0.618034) % 1.0;
                    if let Some(i) = other {
                        self.views.table[i].seed = self.bolt_seed;
                    }
                    self.bolt_seed
                }
            };
            let c = s.look_color;
            self.out.blaster(&p, &tail, c, s.look_brightness * fade, s.look_core * thin, s.look_glow * thin, seed, 0.55);
            self.out
                .light(&light_at, [c[0] * 0.75, c[1] * 0.75, c[2] * 0.75], s.look_light_radius);
            return;
        }
        // Classic bolts: the 2004 game's two textured strips trailing back from
        // the head, the core (LaserLength by LaserWidth, half as wide at the
        // tail) and a glow twice as wide in LaserGlowColor, drawn twice.
        let width = or(s.laser_width, 0.1) * thin;
        // Fading, the core's alpha and every channel of the glow's colour are
        // scaled by what is left of the bolt.
        if s.core >= 0 {
            let length = self.out.bolt_span(&bolt, min(or(s.laser_length, 1.0), bolt.drawn));
            let axis = self.out.bolt_axis;
            self.out.quad(s.core, &p, [1.0, 1.0, 1.0, fade], width / 2.0, length, axis, Some(CORE_TAPER));
        }
        if s.glow >= 0 && !(s.glow_length == 0.0 || s.glow_length.is_nan()) {
            let length = self.out.bolt_span(&bolt, min(s.glow_length, bolt.drawn));
            let axis = self.out.bolt_axis;
            let [cr, cg, cb] = s.glow_color;
            let bright = 1.35 * fade;
            self.out.quad(
                s.glow,
                &p,
                [cr * bright, cg * bright, cb * bright, fade],
                width * 1.2,
                length,
                axis,
                Some(GLOW_TAPER),
            );
            self.out.quad(
                s.glow,
                &p,
                [cr * fade, cg * fade, cb * fade, 0.28 * fade],
                width * 3.2,
                length,
                axis,
                Some(GLOW_TAPER),
            );
        }
        self.out.light(&light_at, s.light_color, or(s.light_radius, 0.0));
    }

    // ---- network projectiles ----

    // Leaves the live list and frees the slot for the client to reuse.
    fn unlink(&mut self, slot: u32) {
        let (prev, next) = (self.net[slot as usize].prev, self.net[slot as usize].next);
        if prev == NONE {
            self.net_head = next;
        } else {
            self.net[prev as usize].next = next;
        }
        if next == NONE {
            self.net_tail = prev;
        } else {
            self.net[next as usize].prev = prev;
        }
        self.net[slot as usize].active = false;
        self.net_live -= 1;
    }

    /// ProjectilePresentation.receive/end/remove/reset, as one command.
    fn net_command(&mut self, at: usize) {
        let c: [f64; nc::STRIDE] = {
            let mut row = [0.0; nc::STRIDE];
            row.copy_from_slice(&self.net_input[at..at + nc::STRIDE]);
            row
        };
        let kind = c[nc::KIND] as u32;
        // RESET is the epoch changing: the client has already let go of every
        // mesh and trail, so only the kernel's state is cleared here.
        if kind == nk::RESET {
            let mut slot = self.net_head;
            while slot != NONE {
                let next = self.net[slot as usize].next;
                self.net[slot as usize].active = false;
                slot = next;
            }
            self.net_head = NONE;
            self.net_tail = NONE;
            self.net_live = 0;
            return;
        }
        // The slot is a number from the host. A NaN, a negative or a huge one
        // is skipped and counted, never used as an index.
        let index = c[nc::SLOT];
        if !(index >= 0.0 && (index as usize) < lim::NET_SLOTS) {
            self.skipped_net += 1;
            return;
        }
        let slot = index as u32;
        if kind == nk::REMOVE {
            if self.net[slot as usize].active {
                self.unlink(slot);
            }
            return;
        }
        let v3 = |k: usize| Vector3::new(c[k], c[k + 1], c[k + 2]);
        if kind == nk::END {
            let n = &mut self.net[slot as usize];
            // A terminal for a projectile this client never held, or held in
            // another life, is nothing to end.
            if !n.active || n.generation != c[nc::GENERATION] || n.len == 0 {
                return;
            }
            let last = n.samples[n.len - 1];
            n.end_tick = min(n.end_tick, c[nc::TICK]);
            // An impact (schema 2) adds a last stretch flown into its point.
            if c[nc::SCHEMA] == 2.0 && c[nc::TICK] > last.tick {
                let mut s = last;
                s.tick = c[nc::TICK];
                s.expiry = c[nc::TICK] + 18.0;
                s.position = v3(nc::POSITION);
                s.end = v3(nc::POSITION);
                s.life = 0.0;
                s.changed = false;
                push_sample(n, s);
            }
            return;
        }
        // Past the kinds above only a sample is left; an unknown kind from a
        // host built against another layout is skipped, not guessed at.
        if kind != nk::SAMPLE {
            self.skipped_net += 1;
            return;
        }
        // The first sample for a slot makes the projectile. The client decides
        // identity: it frees a slot (and sends REMOVE) when an id comes back
        // in another life, so a sample never lands on a stranger's state.
        if !self.net[slot as usize].active {
            let n = &mut self.net[slot as usize];
            *n = Net::empty();
            n.active = true;
            n.style = c[nc::STYLE] as u32;
            n.kind = c[nc::TYPE] as u32;
            n.flags = c[nc::FLAGS] as u32;
            n.generation = c[nc::GENERATION];
            // Insertion order, as the JS Map keeps it: it is the draw order.
            n.prev = self.net_tail;
            n.next = NONE;
            if self.net_tail == NONE {
                self.net_head = slot;
            } else {
                self.net[self.net_tail as usize].next = slot;
            }
            self.net_tail = slot;
            self.net_live += 1;
        }
        let n = &mut self.net[slot as usize];
        let mut s = Sample {
            tick: c[nc::TICK],
            expiry: c[nc::EXPIRY_TICK],
            life: c[nc::LIFE],
            position: v3(nc::POSITION),
            velocity: v3(nc::VELOCITY),
            quaternion: [c[nc::QUATERNION], c[nc::QUATERNION + 1], c[nc::QUATERNION + 2], c[nc::QUATERNION + 3]],
            end: v3(nc::END),
            changed: false,
        };
        if n.len > 0 {
            s.changed = changed_flight(&n.samples[n.len - 1].velocity, &s.velocity);
        }
        push_sample(n, s);
    }

    /// ProjectilePresentation.present(tick).
    // The list is walked with the next link saved first, because drawing can
    // end a projectile and take it out of the list.
    fn present_net(&mut self, tick: f64, enhanced: bool) {
        let mut slot = self.net_head;
        while slot != NONE {
            let next = self.net[slot as usize].next;
            self.present_one(slot, tick, enhanced);
            slot = next;
        }
    }

    fn present_one(&mut self, slot: u32, tick: f64, enhanced: bool) {
        let n = self.net[slot as usize];
        if n.len == 0 {
            return;
        }
        let Some(style) = self.style(n.style as f64) else {
            self.skipped_net += 1;
            return;
        };
        let last = n.samples[n.len - 1];
        // Rockets and fat rays fly dead straight past their last sample for
        // the life it carried, shells fall; everything else coasts briefly.
        let followed = if n.kind == nt::ROUND { style.flight } else { flight::NONE };
        let fade_ticks = if n.kind == nt::BEAM && style.fade_out > 0.0 {
            style.fade_out * 60.0
        } else {
            0.0
        };
        let expires = if followed != flight::NONE { last.tick + last.life * 60.0 + 18.0 } else { last.expiry };
        // Gone once the clock reaches its terminal (+ FadeOutTime for beams),
        // or passes the time its last sample said it would last (plus slack).
        if tick >= n.end_tick + fade_ticks || tick > expires + fade_ticks {
            // The client frees the slot, its mesh and its trail on this call.
            self.out.call(ck::NET_REMOVE, slot as f64);
            self.unlink(slot);
            return;
        }
        // The two samples the clock lies between (the newest at or before it,
        // and the first after), interpolated by how far through that gap the
        // clock is. Samples are in tick order.
        let render_tick = tick;
        let (mut a, mut b) = (n.samples[0], n.samples[0]);
        for i in 0..n.len {
            let s = n.samples[i];
            if s.tick <= render_tick {
                a = s;
                b = s;
            } else {
                b = s;
                break;
            }
        }
        let fraction = if b.tick > a.tick {
            max(0.0, min(1.0, (render_tick - a.tick) / (b.tick - a.tick)))
        } else {
            0.0
        };
        let mut position = a.position;
        position.lerp(&b.position, fraction);
        let mut velocity = a.velocity;
        velocity.lerp(&b.velocity, fraction);
        let mut path = Vector3::default();
        if b.tick > a.tick && render_tick >= a.tick {
            path = b.position;
            path.sub(&a.position).multiply_scalar(60.0 / (b.tick - a.tick));
        }
        if render_tick >= last.tick && followed != flight::NONE {
            // A rocket or shell flies on along its path for the life it had
            // left, so a sample that is late, or never sent, costs nothing.
            let age = min(last.life * 60.0, render_tick - last.tick) / 60.0;
            let gravity = if followed == flight::BALLISTIC { -9.8 * style.gravity } else { 0.0 };
            position.add_scaled_vector(&velocity, age);
            position.y += gravity * age * age / 2.0;
            if render_tick - last.tick < last.life * 60.0 {
                path = velocity;
                path.y += gravity * age;
            }
        } else if render_tick >= last.tick && n.kind != nt::BEAM {
            // Short, bounded cosmetic coast only: brake the final ticks and
            // never extrapolate past the authority's remaining sampled life.
            let limit = min(COAST_TICKS, last.life * 60.0);
            let full = min(COAST_FULL_TICKS, limit);
            let tail = limit - full;
            let age = min(limit, render_tick - last.tick);
            let brake = max(0.0, age - full);
            let along = (age - if tail != 0.0 { brake * brake / (2.0 * tail) } else { 0.0 }) / 60.0;
            position.add_scaled_vector(&velocity, along);
            if age < limit {
                path = velocity;
                path.multiply_scalar(if tail != 0.0 { 1.0 - brake / tail } else { 1.0 });
            }
        }
        // A beam is its sampled endpoints, drawn as they are; everything else
        // gets the correction that hides a new sample arriving off its path.
        if n.kind != nt::BEAM {
            self.smooth(slot, &mut position, &velocity, &path, tick, last.tick, a.changed || b.changed, render_tick >= n.samples[0].tick);
        }
        let previous = position;
        let mut q = Quaternion::new(a.quaternion[0], a.quaternion[1], a.quaternion[2], a.quaternion[3]);
        q.slerp(&Quaternion::new(b.quaternion[0], b.quaternion[1], b.quaternion[2], b.quaternion[3]), fraction);
        let mut end = a.end;
        end.lerp(&b.end, fraction);
        let mut direction = Vector3::default();
        direction.sub_vectors(&end, &position).normalize();
        if n.kind == nt::BEAM {
            if fade_ticks > 0.0 {
                let fade_start = min(n.end_tick, last.tick + last.life * 60.0);
                let fade = if tick <= fade_start {
                    1.0
                } else {
                    max(0.0, 1.0 - (tick - fade_start) / fade_ticks)
                };
                if fade > 0.0 {
                    self.draw_beam_fade(&style, &position, &end, &direction, fade, enhanced);
                } else {
                    self.out.call(ck::NET_REMOVE, slot as f64);
                    self.unlink(slot);
                }
            } else {
                self.draw_beam(&style, &position, &end, &direction, last.life, enhanced);
            }
        } else {
            self.draw_net_view(&style, slot, &previous, &position, &q, &velocity, n.flags);
        }
    }

    /// drawProjectileView(p, 1, true) for a network projectile: its mesh and
    /// trail at the position, and its light.
    #[allow(clippy::too_many_arguments)]
    fn draw_net_view(&mut self, s: &Style, slot: u32, previous: &Vector3, position: &Vector3, q: &Quaternion, velocity: &Vector3, flags: u32) {
        let mut drawn = Vector3::default();
        drawn.lerp_vectors(previous, position, 1.0);
        if flags & (1 << net_flag::MESH) != 0 {
            self.mesh_call(ck::NET_MESH, slot as f64, &drawn, Some(q));
        }
        if flags & (1 << net_flag::TRAIL) != 0 {
            if let Some(c) = self.out.call(ck::NET_TRAIL, slot as f64) {
                self.out.put3(c, cl::A, &drawn);
                self.put_quaternion(c, q);
                self.out.put3(c, cl::E, velocity);
                self.out.calls[c + cl::S0] = 1.0;
                self.out.calls[c + cl::S1] = 1.0;
            }
        }
        self.out.light(&drawn, s.light_color, or(s.light_radius, 0.0));
    }

    /// ProjectilePresentation.smooth: a correction carried across a new
    /// sample and faded out over SMOOTH_SECONDS.
    #[allow(clippy::too_many_arguments)]
    // `flying`: the clock has reached the round's first sample. A frame
    // before that shows it waiting at its launch point with no motion to
    // carry on from, so the frame it starts on takes no correction either.
    // (The JavaScript carries one there, which holds the round at its
    // launch point for a frame and then catches it up: a tick's flight at 60
    // frames a second, several metres at 20.)
    fn smooth(&mut self, slot: u32, position: &mut Vector3, velocity: &Vector3, path: &Vector3, tick: f64, latest: f64, discontinuous: bool, flying: bool) {
        // A bounce or authoritative stop (`discontinuous`), a clock run back,
        // or the first frame shown, starts again from no correction.
        let n = &mut self.net[slot as usize];
        if !n.shown || tick < n.shown_tick || discontinuous {
            n.offset.set(0.0, 0.0, 0.0);
        } else {
            let dt = (tick - n.shown_tick) / 60.0;
            n.offset.multiply_scalar(exp(-dt / SMOOTH_SECONDS));
            // A new sample arrived since last frame: how far the path it
            // describes is from where last frame's motion would have put the
            // projectile becomes a correction, bounded to a tenth of its
            // speed (at least a quarter metre, at most MAX_OFFSET), and one
            // more than twice that is a jump taken whole.
            if latest != n.shown_latest {
                let mut expected = n.shown_raw;
                expected.add_scaled_vector(&n.shown_velocity, dt).sub(position);
                let bound = min(MAX_OFFSET, max(0.25, velocity.length() * 0.1));
                if expected.length() > bound * 2.0 {
                    n.offset.set(0.0, 0.0, 0.0);
                } else {
                    n.offset.add(&expected).clamp_length(0.0, bound);
                }
            }
        }
        n.shown = flying;
        n.shown_raw = *position;
        n.shown_velocity = *path;
        n.shown_tick = tick;
        n.shown_latest = latest;
        position.add(&n.offset);
    }
}

/// A count read from the host, held to what the block can hold.
// (A NaN or a negative count is none; JavaScript's own Number-to-index
// conversions would not have been safe here.)
fn clamp_count(v: f64, limit: usize) -> usize {
    if v >= 1.0 {
        (v as usize).min(limit)
    } else {
        0
    }
}

/// Appends a sample, dropping the oldest past the ring (`samples.shift()`).
// (copy_within moves the seven newest down one place; the ring is eight long
// and a sample is a few dozen bytes, so this is cheaper than a ring index.)
fn push_sample(n: &mut Net, s: Sample) {
    if n.len == SAMPLES {
        n.samples.copy_within(1.., 0);
        n.len -= 1;
    }
    n.samples[n.len] = s;
    n.len += 1;
}

/// changedFlight: a sharp turn or authoritative
/// stop can be a bounce or contact, so no correction carries across it.
fn changed_flight(a: &Vector3, b: &Vector3) -> bool {
    let speed_a = hypot3(a.x, a.y, a.z);
    let speed_b = hypot3(b.x, b.y, b.z);
    // `a.reduce((sum, n, i) => sum + n * b[i], 0)`: left to right from 0.
    let dot = 0.0 + a.x * b.x + a.y * b.y + a.z * b.z;
    speed_a > 1e-6 && (speed_b < speed_a * 0.25 || dot < speed_a * speed_b * 0.5)
}

pub mod events;

#[cfg(test)]
mod beam_fade_tests {
    use super::*;

    #[test]
    fn net_beam_persists_and_fades_over_fade_out_time() {
        let mut ord = Ordnance::new();
        let s = &mut ord.style_input[..st::STRIDE];
        s.fill(0.0);
        s[st::BEAM_BATCH] = 0.0;
        s[st::LASER_WIDTH] = 0.35;
        s[st::FADE_OUT_TIME] = 0.40; // 24 ticks at 60 Hz
        s[st::LOOK_COLOR] = 0.8;
        s[st::LOOK_COLOR + 1] = 0.5;
        s[st::LOOK_COLOR + 2] = 1.0;
        s[st::LOOK_BRIGHTNESS] = 1.6;
        s[st::LOOK_CORE] = 0.12;
        s[st::LOOK_GLOW] = 0.28;
        ord.set_style(0);

        // Send beam sample at tick 10 (life 0.20s = 12 ticks -> active through tick 22)
        // and terminal END at tick 22.
        let sample = &mut ord.net_input[..nc::STRIDE];
        sample.fill(0.0);
        sample[nc::KIND] = nk::SAMPLE as f64;
        sample[nc::SLOT] = 0.0;
        sample[nc::GENERATION] = 1.0;
        sample[nc::SCHEMA] = 6.0;
        sample[nc::TICK] = 10.0;
        sample[nc::EXPIRY_TICK] = 32.0;
        sample[nc::STYLE] = 0.0;
        sample[nc::TYPE] = nt::BEAM as f64;
        sample[nc::POSITION + 2] = 0.0;
        sample[nc::END + 2] = 35.0;
        sample[nc::LIFE] = 0.20;

        let end = &mut ord.net_input[nc::STRIDE..2 * nc::STRIDE];
        end.fill(0.0);
        end[nc::KIND] = nk::END as f64;
        end[nc::SLOT] = 0.0;
        end[nc::GENERATION] = 1.0;
        end[nc::SCHEMA] = 7.0;
        end[nc::TICK] = 22.0;
        end[nc::EXPIRY_TICK] = 44.0;
        ord.net_flush(2);

        let present_at = |ord: &mut Ordnance, tick: f64| -> (usize, f32) {
            ord.input[fr::VERSION] = VERSION as f64;
            ord.input[fr::FRAME] = tick;
            ord.input[fr::RECORDS] = 1.0;
            ord.input[fr::NET_COMMANDS] = 0.0;
            ord.input[fr::ENHANCED_BOLTS] = 1.0;
            let r = &mut ord.input[fr::STRIDE..fr::STRIDE + rc::STRIDE];
            r.fill(0.0);
            r[rc::KIND] = kind::NET as f64;
            r[rc::CLOCK_TICK] = tick;
            ord.present();
            let count = ord.header[hd::BLASTERS] as usize;
            let brightness = if count > 0 { ord.blaster_arrays()[2][3] } else { 0.0 };
            (count, brightness)
        };

        // During active LifeSpan (tick 16): full brightness (1.6).
        let (n, b) = present_at(&mut ord, 16.0);
        assert_eq!(n, 1);
        assert!((b - 1.6).abs() < 1e-4, "active brightness {b}");

        // Halfway through 24-tick fade (tick 22 + 12 = 34.0): half brightness (0.8).
        let (n, b) = present_at(&mut ord, 34.0);
        assert_eq!(n, 1);
        assert!((b - 0.8).abs() < 1e-4, "mid-fade brightness {b}");

        // Past end_tick + fade_ticks (tick 47.0): removed and no longer drawn.
        let (n, _) = present_at(&mut ord, 47.0);
        assert_eq!(n, 0);
    }

    /// A replicated beam, which the presenter draws from its samples, gets the glow strip of a
    /// 2005 beam in the Classic look too, and it fades with the core.
    #[test]
    fn net_beam_draws_its_glow_strip_in_the_classic_look_and_fades_with_the_core() {
        let mut ord = Ordnance::new();
        let s = &mut ord.style_input[..st::STRIDE];
        s.fill(0.0);
        s[st::CORE_BATCH] = 0.0;
        s[st::GLOW_BATCH] = 1.0;
        s[st::BEAM_BATCH] = 2.0;
        s[st::LASER_WIDTH] = 0.2;
        s[st::FADE_OUT_TIME] = 0.40; // 24 ticks at 60 Hz
        s[st::GLOW_COLOR..st::GLOW_COLOR + 3].copy_from_slice(&[0.5, 0.25, 0.125]);
        s[st::BEAM_GLOW_WIDTH] = 0.5;
        ord.set_style(0);
        let sample = &mut ord.net_input[..nc::STRIDE];
        sample.fill(0.0);
        sample[nc::KIND] = nk::SAMPLE as f64;
        sample[nc::GENERATION] = 1.0;
        sample[nc::SCHEMA] = 6.0;
        sample[nc::TICK] = 10.0;
        sample[nc::EXPIRY_TICK] = 32.0;
        sample[nc::TYPE] = nt::BEAM as f64;
        sample[nc::END + 2] = 35.0;
        sample[nc::LIFE] = 0.20;
        let end = &mut ord.net_input[nc::STRIDE..2 * nc::STRIDE];
        end.fill(0.0);
        end[nc::KIND] = nk::END as f64;
        end[nc::GENERATION] = 1.0;
        end[nc::SCHEMA] = 7.0;
        end[nc::TICK] = 22.0;
        end[nc::EXPIRY_TICK] = 44.0;
        ord.net_flush(2);
        let present_at = |ord: &mut Ordnance, tick: f64, enhanced: bool| {
            ord.input[fr::VERSION] = VERSION as f64;
            ord.input[fr::FRAME] = tick;
            ord.input[fr::RECORDS] = 1.0;
            ord.input[fr::NET_COMMANDS] = 0.0;
            ord.input[fr::ENHANCED_BOLTS] = if enhanced { 1.0 } else { 0.0 };
            let r = &mut ord.input[fr::STRIDE..fr::STRIDE + rc::STRIDE];
            r.fill(0.0);
            r[rc::KIND] = kind::NET as f64;
            r[rc::CLOCK_TICK] = tick;
            ord.present();
        };
        let glow = lim::QUADS_PER_HANDLE;
        present_at(&mut ord, 16.0, false);
        assert_eq!((ord.counts()[1], ord.counts()[2]), (1, 1), "one glow strip over one core strip");
        let [_, color, shape, _, _, _] = ord.quad_arrays();
        assert!((shape[glow * 4] - 0.25).abs() < 1e-6, "half of the glow width");
        assert!((shape[glow * 4 + 2] - 35.0).abs() < 1e-6, "the beam's whole length");
        assert_eq!(&color[glow * 4..glow * 4 + 3], &[0.5, 0.25, 0.125]);
        assert!((color[glow * 4 + 3] - 1.0).abs() < 1e-6);
        // Halfway through its 24-tick fade (tick 34): both strips at half strength.
        present_at(&mut ord, 34.0, false);
        let [_, color, _, _, _, _] = ord.quad_arrays();
        assert!((color[glow * 4 + 3] - 0.5).abs() < 1e-6, "glow alpha {}", color[glow * 4 + 3]);
        assert!((color[2 * glow * 4 + 3] - 0.5).abs() < 1e-6, "core alpha");
        // The Enhanced look draws its capsule and no strip.
        present_at(&mut ord, 16.0, true);
        assert_eq!((ord.counts()[1], ord.counts()[2]), (0, 0));
    }
}

#[cfg(test)]
mod style_row_tests {
    use super::*;

    #[test]
    fn a_set_style_reads_back_as_the_row_it_was_made_from() {
        let mut ord = Ordnance::new();
        assert!(ord.style_row(3).is_none(), "an unset style has no row");
        let row: Vec<f64> = (0..st::STRIDE).map(|i| match i {
            st::CORE_BATCH | st::GLOW_BATCH | st::BEAM_BATCH => i as f64 - 1.0,
            st::FLIGHT => 2.0,
            _ => 0.25 * i as f64 + 0.5,
        }).collect();
        ord.style_input[..st::STRIDE].copy_from_slice(&row);
        ord.set_style(3);
        assert_eq!(ord.style_row(3).unwrap().to_vec(), row);
    }

    /// A style made from another style's row, as a fading class's steps are, keeps the beam's
    /// glow strip: it draws the second pass at the width of the style it was made from.
    #[test]
    fn a_style_made_from_a_beams_row_still_draws_the_beams_glow_strip() {
        let mut ord = Ordnance::new();
        ord.style_input[..st::STRIDE].fill(0.0);
        ord.style_input[st::CORE_BATCH] = 0.0;
        ord.style_input[st::GLOW_BATCH] = 1.0;
        ord.style_input[st::BEAM_BATCH] = 2.0;
        ord.style_input[st::LASER_WIDTH] = 0.5;
        ord.style_input[st::GLOW_COLOR..st::GLOW_COLOR + 3].copy_from_slice(&[0.5, 0.25, 0.125]);
        ord.style_input[st::BEAM_GLOW_WIDTH] = 1.25;
        ord.set_style(0);
        let row = ord.style_row(0).unwrap();
        ord.style_input[..st::STRIDE].copy_from_slice(&row);
        ord.set_style(5);
        ord.input[fr::VERSION] = VERSION as f64;
        ord.input[fr::RECORDS] = 1.0;
        let at = fr::STRIDE;
        ord.input[at..at + rc::STRIDE].fill(0.0);
        ord.input[at + rc::KIND] = kind::BEAM as f64;
        ord.input[at + rc::STYLE] = 5.0;
        ord.input[at + rc::LIFE] = 1.0;
        ord.input[at + rc::END + 2] = 10.0;
        ord.input[at + rc::DIRECTION + 2] = 1.0;
        ord.present();
        assert_eq!(ord.counts()[1], 1, "the derived style draws one glow strip");
        let [_, color, shape, _, _, _] = ord.quad_arrays();
        let glow = lim::QUADS_PER_HANDLE;
        assert!((shape[glow * 4] - 0.625).abs() < 1e-6, "half of the glow width");
        assert_eq!(&color[glow * 4..glow * 4 + 3], &[0.5, 0.25, 0.125]);
    }
}
