//! Cloth pieces of 2005 characters (capes, robes, skirts, hair), moved for drawing only.
//!
//! Each piece moves in its own frame: its parent joint's pose times the authored placement, as
//! the 2005 game does. A step is position Verlet with gravity, the body's acceleration, drag
//! against the air and wind along the vertex normals, then the fixed vertices go to their
//! joints, the authored vertex pairs (cross, bend, stretch) are restored to their rest lengths
//! once, and the free vertices are pushed out of the body's collision shapes. The battle
//! simulation never sees any of it. Steps are a fixed 1/144 s (the game steps once per drawn
//! frame: this is the game at 144 frames a second), at most ten per drawn frame, and a drawn
//! frame blends the last two steps, so the cloth moves the same at any frame rate.
use three::Matrix4;

pub(super) type V3 = [f32; 3];

/// One simulation step, in seconds. The game updates its cloth once per drawn frame, so this is
/// the game drawn at 144 frames a second, at any frame rate here.
pub(super) const STEP: f32 = 1. / 144.;
/// Steps one drawn frame may run (about 70 ms); a longer hitch drops the rest of its time.
pub(super) const MAX_STEPS: u32 = 10;
/// A fixed point moving faster than this in its piece frame since it was last placed, on a drawn frame whose
/// pose crosses one of the body's clip changes (a clip begun at its first frame, a new tumble phase), marks a
/// pose that jumped (the thrown somersault moves a shoulder about 6 m/s). The game has no rule for it: its
/// cloth is thrown by the jump. Here the piece restarts from its rest shape carried by the new pose, as
/// after a teleport. Inside a clip the authored motion goes into the cloth however fast it is.
pub(super) const SNAP_SPEED: f32 = 15.;
/// The game clamps the frame's speed before it drags the cloth (no class property sets it).
const MAX_FRAME_SPEED: f32 = 10.;
const GRAVITY: V3 = [0., -9.8, 0.];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn clamp_length(v: V3, max: f32) -> V3 {
    let squared = dot(v, v);
    if squared > max * max {
        scale(v, max / squared.sqrt())
    } else {
        v
    }
}

/// An affine transform in single precision: three axis columns and a translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Affine([f32; 12]);
impl Default for Affine {
    fn default() -> Self {
        Self([1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.])
    }
}
impl Affine {
    pub fn from(m: &Matrix4) -> Self {
        let e = &m.elements;
        Self([
            e[0] as f32, e[1] as f32, e[2] as f32, e[4] as f32, e[5] as f32, e[6] as f32,
            e[8] as f32, e[9] as f32, e[10] as f32, e[12] as f32, e[13] as f32, e[14] as f32,
        ])
    }
    fn axis(&self, i: usize) -> V3 {
        [self.0[i * 3], self.0[i * 3 + 1], self.0[i * 3 + 2]]
    }
    fn origin(&self) -> V3 {
        self.axis(3)
    }
    pub fn dir(&self, v: V3) -> V3 {
        let m = &self.0;
        [
            m[0] * v[0] + m[3] * v[1] + m[6] * v[2],
            m[1] * v[0] + m[4] * v[1] + m[7] * v[2],
            m[2] * v[0] + m[5] * v[1] + m[8] * v[2],
        ]
    }
    pub fn point(&self, v: V3) -> V3 {
        add(self.dir(v), self.origin())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Sphere,
    Cylinder,
    Box,
}
/// A collision shape on a joint: `local` places it in the joint's frame.
#[derive(Clone, Debug)]
pub(super) struct Shape {
    pub joint: usize,
    pub kind: Kind,
    pub size: V3,
    pub local: Matrix4,
}
/// A piece's numbers from its cloth class; the defaults are the game's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Params {
    pub wind_speed: f32,
    pub wind_direction: V3,
    pub dampening: f32,
    pub drag: f32,
    pub mass: f32,
    pub max_acceleration: f32,
}
impl Default for Params {
    fn default() -> Self {
        Self {
            wind_speed: 10.,
            wind_direction: [1., 0., 0.],
            dampening: 0.5,
            drag: -2.,
            mass: 1.,
            max_acceleration: 20.,
        }
    }
}
/// Two vertices held at their rest distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Link {
    pub a: u16,
    pub b: u16,
    pub rest: f32,
}
/// What a piece is: its rest shape in its own frame and how it hangs on the body.
#[derive(Clone, Debug)]
pub(super) struct Piece {
    pub parent: usize,
    pub placement: Matrix4,
    pub rest: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    /// Vertices `0..fixed` follow joints (or stay put when `pins` is empty).
    pub fixed: usize,
    /// Per fixed vertex: an index into `joints` and its place in that joint's bind frame.
    pub pins: Vec<(usize, V3)>,
    pub joints: Vec<usize>,
    /// In the order of one pass: cross pairs, bend pairs, stretch pairs.
    pub links: Vec<Link>,
    pub shapes: Vec<Shape>,
    pub triangles: Vec<[u16; 3]>,
    pub params: Params,
    /// Where a restart puts each vertex: the rest shape carried by up to three nearest fixed
    /// points' joints (index into `joints`, place in that joint's bind frame, weight), so a
    /// respawn in any pose starts unstretched. Empty: the restart uses `rest` as it is.
    pub start: Vec<[(usize, V3, f32); 3]>,
}
/// The time and the body motion one piece of one character has seen.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Clock {
    pub accumulator: f32,
    pending: f32,
    origin: V3,
    velocity: V3,
    acceleration: V3,
    seeded: bool,
    random: u32,
    /// Steps the last drawn frame ran (for the trace).
    pub last_steps: u32,
}
impl Clock {
    pub fn new(seed: u32) -> Self {
        Self {
            random: seed,
            ..Self::default()
        }
    }
    /// Measure the body's motion afresh from the next update (it was out of view).
    pub fn reseed(&mut self) {
        self.seeded = false;
    }
    /// A uniform value in [0, 1), from the same generator and bit layout the game uses.
    fn gust(&mut self) -> f32 {
        let first = self.random.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        self.random = first.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        f32::from_bits(((self.random & 0xffff_01ff) | (first >> 16)) >> 9 | 0x3f80_0000) - 1.
    }
}
/// One character's particles for one piece.
pub(super) struct Particles<'a> {
    pub pos: &'a mut [V3],
    pub old: &'a mut [V3],
    pub normal: &'a mut [V3],
}
impl Piece {
    /// Transforms one frame needs, in `scratch`: the world-to-piece transform, then each pin
    /// joint's bind frame to the piece frame, then each shape to the piece frame.
    pub fn scratch_len(&self) -> usize {
        1 + self.joints.len() + self.shapes.len()
    }
    /// The piece frame in the world for this pose.
    pub fn frame(&self, bones: &[Matrix4]) -> Matrix4 {
        let mut frame = bones[self.parent];
        frame.multiply(&self.placement);
        frame
    }
    pub fn prepare(&self, frame: &Matrix4, bones: &[Matrix4], scratch: &mut [Affine]) {
        let mut inverse = *frame;
        inverse.invert();
        scratch[0] = Affine::from(&inverse);
        for (k, &joint) in self.joints.iter().enumerate() {
            let mut m = inverse;
            m.multiply(&bones[joint]);
            scratch[1 + k] = Affine::from(&m);
        }
        let at = 1 + self.joints.len();
        for (k, shape) in self.shapes.iter().enumerate() {
            let mut m = inverse;
            m.multiply(&bones[shape.joint]).multiply(&shape.local);
            scratch[at + k] = Affine::from(&m);
        }
    }
    /// One drawn frame of the piece (`scratch` from `prepare`): a restart when asked (`cut`) or when the
    /// pose jumped across a clip change (`crossing`, `SNAP_SPEED`), else on from where it was; back from
    /// out of view the body's motion is measured afresh. Returns the steps run and whether it restarted.
    #[allow(clippy::too_many_arguments)]
    pub fn frame_step(
        &self,
        p: &mut Particles,
        clock: &mut Clock,
        scratch: &[Affine],
        origin: V3,
        elapsed: f32,
        cut: bool,
        crossing: bool,
        seed: u32,
    ) -> (u32, bool) {
        let snapped = crossing
            && !cut
            && clock.seeded
            && elapsed <= GAP
            && self.snapped(p.pos, scratch, clock.pending + elapsed);
        let restart = cut || snapped;
        if restart {
            self.reset(p, clock, scratch, seed);
        } else if elapsed > GAP {
            clock.reseed();
        }
        (self.advance(p, clock, scratch, origin, if restart { 0. } else { elapsed }), restart)
    }
    /// Where this frame's pose puts the fixed points, in the piece frame (`scratch` from `prepare`).
    #[cfg(test)]
    pub fn pin_targets(&self, scratch: &[Affine], out: &mut Vec<V3>) {
        out.clear();
        out.extend(self.pins.iter().take(self.fixed).map(|&(joint, local)| scratch[1 + joint].point(local)));
    }
    /// Where the rest shape carried by this frame's pose puts each vertex (the probes' yardstick).
    #[cfg(test)]
    pub fn carried(&self, scratch: &[Affine], out: &mut Vec<V3>) {
        out.clear();
        out.extend((0..self.rest.len()).map(|i| self.carried_at(i, scratch)));
    }
    /// Where the rest shape carried by this frame's pose puts vertex `i` (`scratch` from `prepare`).
    fn carried_at(&self, i: usize, scratch: &[Affine]) -> V3 {
        match self.start.get(i) {
            Some(start) => start.iter().fold([0.; 3], |sum, &(joint, local, w)| add(sum, scale(scratch[1 + joint].point(local), w))),
            None => self.rest[i],
        }
    }
    /// Whether a fixed point would move faster than `SNAP_SPEED` from where the last step put it to this
    /// frame's pose, `since` seconds later.
    fn snapped(&self, pos: &[V3], scratch: &[Affine], since: f32) -> bool {
        if since <= 0. {
            return false;
        }
        let reach = SNAP_SPEED * since;
        self.pins.iter().take(self.fixed).zip(pos).any(|(&(joint, local), x)| {
            let d = sub(scratch[1 + joint].point(local), *x);
            dot(d, d) > reach * reach
        })
    }
    /// Restart from the rest shape in this frame's pose (`scratch` from `prepare`).
    pub fn reset(&self, p: &mut Particles, clock: &mut Clock, scratch: &[Affine], seed: u32) {
        if self.start.len() == self.rest.len() {
            for (x, start) in p.pos.iter_mut().zip(&self.start) {
                *x = start.iter().fold([0.; 3], |sum, &(joint, local, w)| {
                    add(sum, scale(scratch[1 + joint].point(local), w))
                });
            }
        } else {
            p.pos.copy_from_slice(&self.rest);
            self.pin(p.pos, scratch);
        }
        p.old.copy_from_slice(p.pos);
        self.normals(p.pos, p.normal);
        *clock = Clock::new(seed);
    }
    /// Advance by a drawn frame's `elapsed` seconds; `origin` is the piece frame's place in the
    /// world. Returns the steps run.
    pub fn advance(
        &self,
        p: &mut Particles,
        clock: &mut Clock,
        scratch: &[Affine],
        origin: V3,
        elapsed: f32,
    ) -> u32 {
        Self::seed(clock, origin);
        let elapsed = if elapsed.is_finite() {
            elapsed.clamp(0., STEP * MAX_STEPS as f32)
        } else {
            0.
        };
        clock.accumulator += elapsed;
        clock.pending += elapsed;
        if clock.accumulator < STEP {
            clock.last_steps = 0;
            return 0;
        }
        self.follow_body(clock, origin, 1. / clock.pending);
        let mut steps = 0;
        while clock.accumulator >= STEP && steps < MAX_STEPS {
            self.step(p, clock, scratch, STEP);
            clock.accumulator -= STEP;
            steps += 1;
        }
        clock.accumulator = clock.accumulator.min(STEP * 0.999);
        clock.last_steps = steps;
        steps
    }
    /// A fresh clock measures the body's motion from here.
    fn seed(clock: &mut Clock, origin: V3) {
        if !clock.seeded {
            clock.seeded = true;
            clock.origin = origin;
            clock.velocity = [0.; 3];
            clock.acceleration = [0.; 3];
            clock.pending = 0.;
        }
    }
    /// The body's motion since the last update (`rate`: one over its duration), clamped as the game clamps it.
    fn follow_body(&self, clock: &mut Clock, origin: V3, rate: f32) {
        let velocity = clamp_length(scale(sub(origin, clock.origin), rate), MAX_FRAME_SPEED);
        clock.acceleration = clamp_length(scale(sub(velocity, clock.velocity), rate), self.params.max_acceleration);
        clock.velocity = velocity;
        clock.origin = origin;
        clock.pending = 0.;
    }
    /// The game's own stepping, for the probes: one update over a drawn frame's whole time (at most 0.039 s).
    #[cfg(test)]
    pub fn advance_once(&self, p: &mut Particles, clock: &mut Clock, scratch: &[Affine], origin: V3, elapsed: f32) {
        Self::seed(clock, origin);
        let dt = elapsed.min(0.039);
        if dt > 0. {
            self.follow_body(clock, origin, 1. / dt);
            self.step(p, clock, scratch, dt);
        }
    }
    pub fn step(&self, p: &mut Particles, clock: &mut Clock, scratch: &[Affine], dt: f32) {
        let to_piece = &scratch[0];
        let k = &self.params;
        let gravity = to_piece.dir(sub(GRAVITY, scale(clock.acceleration, k.dampening)));
        let wind = to_piece.dir(k.wind_direction);
        let air = to_piece.dir(clock.velocity);
        let gust = k.wind_speed * clock.gust();
        let per_mass = 1. / k.mass;
        let (rate, dt2) = (1. / dt, dt * dt);
        for i in self.fixed..self.rest.len() {
            let (x, o, n) = (p.pos[i], p.old[i], p.normal[i]);
            let push = scale(n, dot(n, wind) * gust);
            let drag = scale(add(air, scale(sub(x, o), rate)), k.drag);
            let a = add(gravity, scale(add(push, drag), per_mass));
            p.pos[i] = add(add(x, sub(x, o)), scale(a, dt2));
            p.old[i] = x;
        }
        self.pin(p.pos, scratch);
        for &link in &self.links {
            project(p.pos, self.fixed, link);
        }
        let at = 1 + self.joints.len();
        for (shape, m) in self.shapes.iter().zip(&scratch[at..]) {
            for x in &mut p.pos[self.fixed..] {
                *x = collide(shape.kind, shape.size, m, *x);
            }
        }
        self.normals(p.pos, p.normal);
    }
    fn pin(&self, pos: &mut [V3], scratch: &[Affine]) {
        for (x, &(joint, local)) in pos.iter_mut().zip(&self.pins) {
            *x = scratch[1 + joint].point(local);
        }
    }
    fn normals(&self, pos: &[V3], normal: &mut [V3]) {
        normal.fill([0.; 3]);
        for t in &self.triangles {
            let [a, b, c] = t.map(usize::from);
            let n = cross(sub(pos[b], pos[a]), sub(pos[c], pos[a]));
            for i in [a, b, c] {
                normal[i] = add(normal[i], n);
            }
        }
        for n in normal {
            let length = dot(*n, *n).sqrt();
            if length > 0. {
                *n = scale(*n, 1. / length);
            }
        }
    }
    /// Interleaved position, normal and UV for drawing in the piece frame: free vertices blend
    /// the last two steps, fixed ones sit on this frame's joints. Returns the largest distance
    /// from the frame's origin.
    pub fn draw(&self, p: &Particles, clock: &Clock, scratch: &[Affine], out: &mut [f32]) -> f32 {
        let t = (clock.accumulator / STEP).clamp(0., 1.);
        let mut reach = 0f32;
        for (i, row) in out.chunks_exact_mut(8).take(self.rest.len()).enumerate() {
            let x = match self.pins.get(i) {
                Some(&(joint, local)) => scratch[1 + joint].point(local),
                None if i < self.fixed => p.pos[i],
                None => add(p.old[i], scale(sub(p.pos[i], p.old[i]), t)),
            };
            reach = reach.max(dot(x, x));
            let n = p.normal[i];
            let uv = self.uvs[i];
            row.copy_from_slice(&[x[0], x[1], x[2], n[0], n[1], n[2], uv[0], uv[1]]);
        }
        reach.sqrt()
    }
}
/// What one drawn frame of a piece shows, for the trace: the drawn mesh edge longest against its
/// rest length, how far the free vertices sit from the rest shape carried by the current pose
/// (mean and largest), and how far they moved in the last step, all in the piece frame.
pub(super) struct Measure {
    pub ratio: f32,
    pub excess: f32,
    pub edge: (u16, u16),
    pub off: f32,
    pub off_max: f32,
    pub motion: f32,
    /// The mean drawn place of the fixed vertices.
    pub pins: V3,
}
impl Piece {
    pub fn measure(&self, p: &Particles, scratch: &[Affine], drawn: &[f32]) -> Measure {
        let at = |i: usize| [drawn[i * 8], drawn[i * 8 + 1], drawn[i * 8 + 2]];
        let mut m = Measure { ratio: 0., excess: 0., edge: (0, 0), off: 0., off_max: 0., motion: 0., pins: [0.; 3] };
        for i in 0..self.fixed {
            m.pins = add(m.pins, scale(at(i), 1. / self.fixed as f32));
        }
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (i, j) = (usize::from(a), usize::from(b));
                let rest = dot(sub(self.rest[j], self.rest[i]), sub(self.rest[j], self.rest[i])).sqrt();
                let d = sub(at(j), at(i));
                let length = dot(d, d).sqrt();
                if rest > 1e-4 && length / rest > m.ratio {
                    (m.ratio, m.excess, m.edge) = (length / rest, length - rest, (a, b));
                }
            }
        }
        let free = self.rest.len().saturating_sub(self.fixed).max(1) as f32;
        for i in self.fixed..self.rest.len() {
            let off = sub(at(i), self.carried_at(i, scratch));
            let off = dot(off, off).sqrt();
            m.off += off / free;
            m.off_max = m.off_max.max(off);
            let step = sub(p.pos[i], p.old[i]);
            m.motion += dot(step, step).sqrt() / free;
        }
        m
    }
}
/// Restore a pair to its rest length: only the free end moves when the other is fixed, else
/// both move half the way (as the game does, it tests end `a` before end `b`).
fn project(pos: &mut [V3], fixed: usize, link: Link) {
    let (a, b) = (usize::from(link.a), usize::from(link.b));
    let d = sub(pos[b], pos[a]);
    let length = dot(d, d).sqrt();
    if length <= 0. {
        return;
    }
    let s = (length - link.rest) / length;
    if a < fixed {
        pos[b] = sub(pos[b], scale(d, s));
    } else if b < fixed {
        pos[a] = add(pos[a], scale(d, s));
    } else {
        let half = scale(d, 0.5 * s);
        pos[a] = add(pos[a], half);
        pos[b] = sub(pos[b], half);
    }
}
/// Push a free vertex out of one shape, given in the piece frame.
fn collide(kind: Kind, size: V3, m: &Affine, x: V3) -> V3 {
    let d = sub(x, m.origin());
    match kind {
        Kind::Sphere => {
            let r = size[0];
            let squared = dot(d, d);
            if squared < r * r && squared > 0. {
                add(m.origin(), scale(d, r / squared.sqrt()))
            } else {
                x
            }
        }
        Kind::Cylinder => {
            // The axis is the shape's Y and reaches twice size.y either way. When the axial
            // depth is the smaller, the game always pushes toward +Y.
            let (ax, ay, az) = (m.axis(0), m.axis(1), m.axis(2));
            let along = 2. * size[1] - dot(d, ay).abs();
            if along <= 0. {
                return x;
            }
            let (dx, dz) = (dot(d, ax), dot(d, az));
            let radial = (dx * dx + dz * dz).sqrt();
            let around = size[0] - radial;
            if around <= 0. {
                x
            } else if along <= around {
                add(x, scale(ay, along))
            } else if radial > 0. {
                add(x, scale(add(scale(ax, dx), scale(az, dz)), around / radial))
            } else {
                x
            }
        }
        Kind::Box => {
            let axes = [m.axis(0), m.axis(1), m.axis(2)];
            let at = [dot(d, axes[0]), dot(d, axes[1]), dot(d, axes[2])];
            let depth = [0, 1, 2].map(|i| size[i] - at[i].abs());
            if depth.iter().any(|&v| v <= 0.) {
                return x;
            }
            let i = if depth[1] <= depth[0] {
                if depth[2] <= depth[1] {
                    2
                } else {
                    1
                }
            } else if depth[2] <= depth[0] {
                2
            } else {
                0
            };
            let side = if at[i] < 0. { -1. } else { 1. };
            add(x, scale(axes[i], depth[i] * side))
        }
    }
}

/// A character out of view longer than this (seconds) comes back without a kick from the
/// distance its body moved meanwhile.
const GAP: f32 = 0.25;
/// Characters whose cloth can move at once. One more character with cloth in view than this
/// draws its pieces in their rest shape until a place frees up.
pub(super) const INSTANCES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Holder {
    slot: usize,
    id: (u32, u32),
    model: usize,
}
#[derive(Clone, Copy, Default)]
struct Instance {
    holder: Option<Holder>,
    /// The drawn time of the last update, in ticks.
    last: f64,
    /// The drawn frame of the last update.
    drawn: u32,
    reset: bool,
}
/// The moving cloth of the characters in view: particles, clocks and one dynamic geometry per
/// piece, all made when the battle loads; a frame allocates nothing.
#[derive(Default)]
pub(super) struct Pool {
    instances: Vec<Instance>,
    clocks: Vec<Clock>,
    frames: Vec<Matrix4>,
    reach: Vec<f32>,
    pos: Vec<V3>,
    old: Vec<V3>,
    normal: Vec<V3>,
    /// Vertices of the largest model's pieces together, and the most pieces of one model.
    stride: usize,
    pieces: usize,
    scratch: Vec<Affine>,
    vertices: Vec<f32>,
    indices: Vec<u32>,
    geometries: Vec<u32>,
    frame: u32,
    pub steps: usize,
    pub moved: usize,
    trace: Option<Trace>,
}
/// `BFE_CLOTH_TRACE=1`: every 120 drawn frames one line with what moving the cloth cost.
#[derive(Default)]
struct Trace {
    /// `BFE_CLOTH_TRACE=2`: also one line per moved piece and drawn frame (see `Piece::measure`).
    every_frame: bool,
    frames: u32,
    busy: f64,
    worst: f64,
    this: f64,
    moved: usize,
    most: usize,
    steps: usize,
    cuts: usize,
    reach: f32,
    stretch: f32,
    excess: f32,
}
impl Pool {
    pub fn new<'a>(
        models: impl Iterator<Item = &'a [Piece]> + Clone,
        bridge: &mut crate::render_bridge::Bridge,
        key: &str,
    ) -> Result<Self, String> {
        let stride = models.clone().map(|m| m.iter().map(|p| p.rest.len()).sum::<usize>()).max().unwrap_or(0);
        let pieces = models.clone().map(<[Piece]>::len).max().unwrap_or(0);
        if pieces == 0 {
            return Ok(Self::default());
        }
        let all = || models.clone().flatten();
        let vertices = all().map(|p| p.rest.len()).max().unwrap_or(0);
        let indices = all().map(|p| p.triangles.len() * 3).max().unwrap_or(0);
        let scratch = all().map(Piece::scratch_len).max().unwrap_or(1);
        let mut geometry = bfe_content_native::geometry::Geometry::model(
            &serde_json::json!({"vertexCount": vertices, "vertexOffset": 0, "indexOffset": 0, "indexCount": indices}),
            crate::session::bytes(&vec![0f32; vertices * 8]),
            crate::session::bytes(&vec![0u32; indices]),
        )?;
        geometry.groups = vec![bfe_content_native::geometry::Group {
            first: 0,
            count: indices,
            material: 0,
        }];
        let geometries = (0..INSTANCES * pieces)
            .map(|_| geometry.register(&mut bridge.r, key))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            instances: vec![Instance::default(); INSTANCES],
            clocks: vec![Clock::default(); INSTANCES * pieces],
            frames: vec![Matrix4::default(); INSTANCES * pieces],
            reach: vec![0.; INSTANCES * pieces],
            pos: vec![[0.; 3]; INSTANCES * stride],
            old: vec![[0.; 3]; INSTANCES * stride],
            normal: vec![[0.; 3]; INSTANCES * stride],
            stride,
            pieces,
            scratch: vec![Affine::default(); scratch],
            vertices: vec![0.; vertices * 8],
            indices: Vec::with_capacity(indices),
            geometries,
            frame: 0,
            steps: 0,
            moved: 0,
            trace: match std::env::var("BFE_CLOTH_TRACE").as_deref() {
                Ok("1") => Some(Trace::default()),
                Ok("2") => Some(Trace { every_frame: true, ..Trace::default() }),
                _ => None,
            },
        })
    }
    /// Whether the per-frame cloth trace is on (`BFE_CLOTH_TRACE=2`).
    pub fn tracing_frames(&self) -> bool {
        self.trace.as_ref().is_some_and(|t| t.every_frame)
    }
    pub fn begin(&mut self) {
        if let Some(t) = &mut self.trace {
            t.frames += 1;
            t.busy += t.this;
            t.worst = t.worst.max(t.this);
            t.moved += self.moved;
            t.most = t.most.max(self.moved);
            t.steps += self.steps;
            if t.frames == 120 {
                eprintln!(
                    "cloth: frames=120 moved={:.1} most={} steps={} cuts={} us={:.1} worst_us={:.1} reach={:.2} stretch={:.2} excess_m={:.3}",
                    t.moved as f64 / 120.,
                    t.most,
                    t.steps,
                    t.cuts,
                    t.busy / 120.,
                    t.worst,
                    t.reach,
                    t.stretch,
                    t.excess
                );
                *t = Trace { every_frame: t.every_frame, ..Trace::default() };
            }
            t.this = 0.;
        }
        self.frame = self.frame.wrapping_add(1);
        self.steps = 0;
        self.moved = 0;
    }
    pub fn clear(&mut self) {
        self.instances.fill(Instance::default());
    }
    /// Move one character's cloth to the drawn time `shown` (ticks) for the pose `bones` and
    /// write its vertices. `cut` restarts it from the rest shape (a respawn or a teleport);
    /// `crossing` lets a pose that jumped restart it (the pose crosses one of the body's clip changes).
    /// Returns its place, or None when every place is taken by a character drawn this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        slot: usize,
        id: (u32, u32),
        model: usize,
        pieces: &[Piece],
        bones: &[Matrix4],
        shown: f64,
        cut: bool,
        crossing: bool,
        bridge: &mut crate::render_bridge::Bridge,
    ) -> Option<usize> {
        if pieces.is_empty() || pieces.len() > self.pieces {
            return None;
        }
        let started = self.trace.as_ref().map(|_| std::time::Instant::now());
        let holder = Holder { slot, id, model };
        let k = if let Some(k) = self.instances.iter().position(|i| i.holder == Some(holder)) {
            self.instances[k].reset |= cut;
            k
        } else {
            let frame = self.frame;
            let k = self
                .instances
                .iter()
                .position(|i| i.holder.is_none_or(|o| o.slot == slot))
                .or_else(|| {
                    let stale = self.instances.iter().enumerate().filter(|(_, i)| i.drawn < frame);
                    stale.min_by_key(|(_, i)| i.drawn).map(|(k, _)| k)
                })?;
            for (c, piece) in pieces.iter().enumerate() {
                self.indices.clear();
                self.indices.extend(piece.triangles.iter().flatten().map(|&i| u32::from(i)));
                let g = self.geometries[k * self.pieces + c];
                bridge.r.core.geometry_index_sub_data(g, 0, crate::session::bytes(&self.indices));
                bridge.r.core.geometry_count(g, self.indices.len());
            }
            self.instances[k] = Instance { holder: Some(holder), last: shown, drawn: 0, reset: true };
            k
        };
        let instance = &mut self.instances[k];
        let elapsed = ((shown - instance.last) / 60.) as f32;
        instance.last = shown;
        instance.drawn = self.frame;
        let reset = std::mem::take(&mut instance.reset);
        let mut at = k * self.stride;
        for (c, piece) in pieces.iter().enumerate() {
            let n = piece.rest.len();
            let frame = piece.frame(bones);
            let scratch = &mut self.scratch[..piece.scratch_len()];
            piece.prepare(&frame, bones, scratch);
            let mut p = Particles {
                pos: &mut self.pos[at..at + n],
                old: &mut self.old[at..at + n],
                normal: &mut self.normal[at..at + n],
            };
            let place = k * self.pieces + c;
            let clock = &mut self.clocks[place];
            let e = &frame.elements;
            let origin = [e[12] as f32, e[13] as f32, e[14] as f32];
            let seed = id.0.wrapping_mul(31).wrapping_add(c as u32) | 1;
            let (steps, reset) = piece.frame_step(&mut p, clock, scratch, origin, elapsed, reset, crossing, seed);
            self.steps += steps as usize;
            let out = &mut self.vertices[..n * 8];
            self.reach[place] = piece.draw(&p, clock, scratch, out);
            if let Some(t) = self.trace.as_ref().filter(|t| t.every_frame) {
                let m = piece.measure(&p, scratch, out);
                // The free points' mean height above the fixed ones, in the world (negative: hanging).
                let height = |i: usize| e[1] * f64::from(out[i * 8]) + e[5] * f64::from(out[i * 8 + 1]) + e[9] * f64::from(out[i * 8 + 2]);
                let mean = |r: std::ops::Range<usize>| {
                    let k = r.len().max(1) as f64;
                    r.map(height).sum::<f64>() / k
                };
                let hang = mean(piece.fixed..n) - mean(0..piece.fixed);
                // How upright the piece frame stands: its up axis against the world's (1 upright, 0 lying).
                let up = e[5] / (e[4] * e[4] + e[5] * e[5] + e[6] * e[6]).sqrt().max(1e-9);
                eprintln!(
                    "cloth: frame shown={shown:.3} slot={slot} piece={c} steps={} edge={:.3} edge_m={:.3} edge_ab={}-{} off_cm={:.2} off_max_cm={:.2} move_cm={:.3} pins={:.3},{:.3},{:.3} origin={:.3},{:.3},{:.3} hang_cm={:.1} up={up:.3} reset={reset} frames={}",
                    clock.last_steps, m.ratio, m.excess, m.edge.0, m.edge.1, m.off * 100., m.off_max * 100., m.motion * 100., m.pins[0], m.pins[1], m.pins[2],
                    origin[0], origin[1], origin[2], hang * 100., t.frames
                );
                if m.off_max > 0.3 {
                    // Where everything is in the piece frame: particles, last step's, shape centres.
                    let round = |v: V3| v.map(|c| (c * 1000.).round() / 1000.);
                    let at = 1 + piece.joints.len();
                    let shapes: Vec<_> = scratch[at..at + piece.shapes.len()].iter().map(|m| round(m.origin())).collect();
                    let pos: Vec<_> = p.pos.iter().map(|v| round(*v)).collect();
                    let old: Vec<_> = p.old.iter().map(|v| round(*v)).collect();
                    eprintln!("cloth: detail shown={shown:.3} pos={pos:?} old={old:?} shapes={shapes:?}");
                }
            }
            self.frames[place] = frame;
            bridge.r.core.geometry_buffer_sub_data(self.geometries[place], 0, 0, crate::session::bytes(out));
            at += n;
        }
        self.moved += 1;
        if let (Some(t), Some(started)) = (&mut self.trace, started) {
            t.this += started.elapsed().as_secs_f64() * 1e6;
            t.cuts += usize::from(reset);
            let first = k * self.pieces;
            t.reach = self.reach[first..first + pieces.len()].iter().fold(t.reach, |a, &b| a.max(b));
            let mut at = k * self.stride;
            for (c, piece) in pieces.iter().enumerate() {
                let pos = &self.pos[at..at + piece.rest.len()];
                // Pairs with a free end only: two fixed ends on different joints part by design.
                let (stretch, excess) = piece
                    .links
                    .iter()
                    .filter(|l| usize::from(l.a.max(l.b)) >= piece.fixed)
                    .map(|l| {
                        let d = sub(pos[usize::from(l.b)], pos[usize::from(l.a)]);
                        let length = dot(d, d).sqrt();
                        (length / l.rest.max(1e-4), length - l.rest)
                    })
                    .fold((0f32, 0f32), |(most, over), (ratio, past)| (most.max(ratio), over.max(past)));
                t.stretch = t.stretch.max(stretch);
                t.excess = t.excess.max(excess);
                if excess > 0.15 {
                    eprintln!("cloth: stretched shown={shown:.1} slot={slot} piece={c} a free pair {excess:.2} m past its rest ({stretch:.2} times), reset={reset}");
                }
                at += piece.rest.len();
            }
        }
        Some(k)
    }
    /// A moved piece's geometry, its frame in the world and how far it reaches from the frame.
    pub fn piece(&self, k: usize, c: usize) -> (u32, &Matrix4, f32) {
        let place = k * self.pieces + c;
        (self.geometries[place], &self.frames[place], self.reach[place])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An invented strip: two fixed vertices on joint 1, four free ones below, links down and across.
    fn strip(shapes: Vec<Shape>, params: Params) -> Piece {
        let rest: Vec<V3> = vec![
            [-0.2, 1., 0.],
            [0.2, 1., 0.],
            [-0.2, 0.7, 0.],
            [0.2, 0.7, 0.],
            [-0.2, 0.4, 0.],
            [0.2, 0.4, 0.],
        ];
        let pairs = [(0, 2), (1, 3), (2, 3), (2, 4), (3, 5), (4, 5), (0, 3), (1, 2)];
        let links = pairs
            .iter()
            .map(|&(a, b)| Link {
                a,
                b,
                rest: dot(sub(rest[b as usize], rest[a as usize]), sub(rest[b as usize], rest[a as usize])).sqrt(),
            })
            .collect();
        Piece {
            parent: 0,
            placement: Matrix4::default(),
            uvs: vec![[0., 0.]; rest.len()],
            fixed: 2,
            pins: vec![(0, rest[0]), (0, rest[1])],
            joints: vec![1],
            rest,
            links,
            shapes,
            triangles: vec![[0, 2, 1], [1, 2, 3], [2, 4, 3], [3, 4, 5]],
            params,
            start: Vec::new(),
        }
    }
    fn still() -> Params {
        Params {
            wind_speed: 0.,
            ..Params::default()
        }
    }
    struct Run {
        pos: Vec<V3>,
        old: Vec<V3>,
        normal: Vec<V3>,
        clock: Clock,
        scratch: Vec<Affine>,
    }
    impl Run {
        fn new(piece: &Piece) -> Self {
            let n = piece.rest.len();
            let mut run = Self {
                pos: vec![[0.; 3]; n],
                old: vec![[0.; 3]; n],
                normal: vec![[0.; 3]; n],
                clock: Clock::default(),
                scratch: vec![Affine::default(); piece.scratch_len()],
            };
            let mut p = Particles { pos: &mut run.pos, old: &mut run.old, normal: &mut run.normal };
            let bones = bones_at(0.);
            piece.prepare(&piece.frame(&bones), &bones, &mut run.scratch);
            piece.reset(&mut p, &mut run.clock, &run.scratch, 7);
            run
        }
        fn frame(&mut self, piece: &Piece, bones: &[Matrix4], elapsed: f32) -> u32 {
            self.frame_restarts(piece, bones, elapsed, true).0
        }
        /// The steps alone, without the pool's restart and reseed rules.
        fn advance_only(&mut self, piece: &Piece, bones: &[Matrix4], elapsed: f32) -> u32 {
            let frame = piece.frame(bones);
            piece.prepare(&frame, bones, &mut self.scratch);
            let e = &frame.elements;
            let mut p = Particles { pos: &mut self.pos, old: &mut self.old, normal: &mut self.normal };
            piece.advance(&mut p, &mut self.clock, &self.scratch, [e[12] as f32, e[13] as f32, e[14] as f32], elapsed)
        }
        /// One drawn frame as the pool runs it (`crossing`: the pose crosses a clip change): steps, and
        /// whether the piece restarted.
        fn frame_restarts(&mut self, piece: &Piece, bones: &[Matrix4], elapsed: f32, crossing: bool) -> (u32, bool) {
            let frame = piece.frame(bones);
            piece.prepare(&frame, bones, &mut self.scratch);
            let e = &frame.elements;
            let origin = [e[12] as f32, e[13] as f32, e[14] as f32];
            let mut p = Particles { pos: &mut self.pos, old: &mut self.old, normal: &mut self.normal };
            piece.frame_step(&mut p, &mut self.clock, &self.scratch, origin, elapsed, false, crossing, 7)
        }
    }
    fn near(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }
    fn bones_at(x: f64) -> Vec<Matrix4> {
        let mut root = Matrix4::default();
        root.set_position(x, 0., 0.);
        vec![root, root]
    }

    #[test]
    fn free_vertices_fall_and_swing_back_under_their_pins() {
        let mut piece = strip(Vec::new(), still());
        // Start the free vertices out to the side, level with the pins.
        for (i, x) in piece.rest.iter_mut().enumerate().skip(2) {
            *x = [0.3 * i as f32, 1., 0.];
        }
        let mut run = Run::new(&piece);
        let bones = bones_at(0.);
        for _ in 0..600 {
            run.frame(&piece, &bones, STEP);
        }
        for x in &run.pos[2..] {
            assert!(x[1] < 0.9, "a free vertex still at the pins' height: {x:?}");
            assert!(x[0].abs() < 0.35, "a free vertex did not swing back under the pins: {x:?}");
            assert!(x.iter().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn fixed_vertices_follow_their_joint() {
        let piece = strip(Vec::new(), still());
        let mut run = Run::new(&piece);
        let mut bones = bones_at(0.);
        bones[1].set_position(0.5, 0.25, -1.);
        run.frame(&piece, &bones, STEP);
        assert!(near(&run.pos[0], &[0.3, 1.25, -1.]), "{:?}", run.pos[0]);
        // Drawn between steps, a fixed vertex is where this frame's joint puts it.
        bones[1].set_position(0.5, 0.5, -1.);
        run.frame(&piece, &bones, STEP * 0.5);
        let mut out = vec![0.; 8 * piece.rest.len()];
        let p = Particles { pos: &mut run.pos, old: &mut run.old, normal: &mut run.normal };
        piece.draw(&p, &run.clock, &run.scratch, &mut out);
        assert!(near(&out[8..11], &[0.7, 1.5, -1.]), "{:?}", &out[8..11]);
    }
    #[test]
    fn a_restart_starts_from_the_rest_shape_in_the_current_pose() {
        // Every vertex rides joint 1 in the start table; the joint has moved and turned.
        let mut piece = strip(Vec::new(), still());
        piece.start = piece.rest.iter().map(|&x| [(0, x, 1.), (0, [0.; 3], 0.), (0, [0.; 3], 0.)]).collect();
        let mut bones = bones_at(0.);
        bones[1].make_rotation_z(0.5).set_position(0.3, 0.2, 0.);
        let mut run = Run::new(&piece);
        let frame = piece.frame(&bones);
        piece.prepare(&frame, &bones, &mut run.scratch);
        let mut p = Particles { pos: &mut run.pos, old: &mut run.old, normal: &mut run.normal };
        piece.reset(&mut p, &mut run.clock, &run.scratch, 3);
        for link in &piece.links {
            let d = sub(run.pos[usize::from(link.b)], run.pos[usize::from(link.a)]);
            assert!((dot(d, d).sqrt() - link.rest).abs() < 1e-5, "a pair starts stretched");
        }
        assert!(near(&run.pos[0], &scratch_point(&run.scratch[1], piece.rest[0])));
        assert_eq!(run.pos, run.old);
    }
    fn scratch_point(m: &Affine, x: V3) -> V3 {
        m.point(x)
    }
    #[test]
    fn a_pair_moves_only_its_free_end_when_the_other_is_fixed() {
        let mut pos = vec![[0., 0., 0.], [3., 0., 0.], [0., 4., 0.], [0., 0., 0.]];
        project(&mut pos, 1, Link { a: 0, b: 1, rest: 1. });
        assert_eq!(pos[0], [0., 0., 0.]);
        assert!(near(&pos[1], &[1., 0., 0.]), "{:?}", pos[1]);
        project(&mut pos, 1, Link { a: 2, b: 0, rest: 2. });
        assert_eq!(pos[2], [0., 2., 0.]);
        let mut free = vec![[0., 0., 0.], [0., 0., 4.]];
        project(&mut free, 0, Link { a: 0, b: 1, rest: 2. });
        assert_eq!(free, vec![[0., 0., 1.], [0., 0., 3.]]);
    }
    #[test]
    fn shapes_push_vertices_out_as_the_game_does() {
        let mut m = Matrix4::default();
        m.set_position(1., 2., 3.);
        let at = Affine::from(&m);
        // A sphere moves the vertex to its surface along the centre line.
        let check = |kind, size, x, want: V3| {
            let got = collide(kind, size, &at, x);
            assert!(near(&got, &want), "{kind:?} {x:?}: {got:?}, not {want:?}");
        };
        // A sphere moves the vertex to its surface along the centre line.
        check(Kind::Sphere, [2., 0., 0.], [1., 3., 3.], [1., 4., 3.]);
        check(Kind::Sphere, [0.5, 0., 0.], [1., 3., 3.], [1., 3., 3.]);
        // A cylinder: radially when that is shallower, else along +Y even from below its middle.
        check(Kind::Cylinder, [1., 1., 0.], [1.75, 2., 3.], [2., 2., 3.]);
        check(Kind::Cylinder, [1., 1., 0.], [1., 0.25, 3.], [1., 0.5, 3.]);
        check(Kind::Cylinder, [1., 1., 0.], [1., -0.5, 3.], [1., -0.5, 3.]);
        // A box: along the axis of least depth, toward the vertex's own side.
        check(Kind::Box, [1., 1., 1.], [1.25, 2., 2.25], [1.25, 2., 2.]);
        check(Kind::Box, [1., 1., 1.], [1.25, 2., 4.5], [1.25, 2., 4.5]);
        check(Kind::Box, [1., 1., 1.], [0.25, 2.5, 3.], [0., 2.5, 3.]);
    }
    #[test]
    fn a_sphere_keeps_the_strip_off_the_body() {
        let mut local = Matrix4::default();
        local.set_position(0., 0.55, -0.1);
        let shape = Shape { joint: 1, kind: Kind::Sphere, size: [0.3, 0.3, 0.3], local };
        let piece = strip(vec![shape], still());
        let mut run = Run::new(&piece);
        let bones = bones_at(0.);
        for _ in 0..300 {
            run.frame(&piece, &bones, STEP);
        }
        for x in &run.pos[2..] {
            let d = sub(*x, [0., 0.55, -0.1]);
            assert!(dot(d, d).sqrt() >= 0.299, "a free vertex inside the sphere: {x:?}");
        }
    }
    #[test]
    fn the_same_motion_at_60_144_and_uncapped_frames_and_after_a_hitch() {
        let piece = strip(Vec::new(), still());
        let mut ends = Vec::new();
        for frames in [60usize, 144, 397] {
            let mut run = Run::new(&piece);
            let mut steps = 0;
            for f in 0..frames * 3 {
                // The body walks along X at 3 m/s and stops after two seconds.
                let t = (f + 1) as f64 / frames as f64;
                steps += run.frame(&piece, &bones_at(3. * t.min(2.)), 1. / frames as f32);
            }
            let expected = (3. / STEP).round() as u32;
            assert!(steps.abs_diff(expected) <= 1, "{frames} frames a second ran {steps} steps, not {expected}");
            ends.push(run.pos.clone());
        }
        for end in &ends[1..] {
            for (a, b) in end.iter().zip(&ends[0]) {
                assert!(dot(sub(*a, *b), sub(*a, *b)).sqrt() < 0.02, "{a:?} against {b:?}");
            }
        }
        // A half-second hitch runs at most MAX_STEPS steps and leaves every vertex finite and near.
        let mut run = Run::new(&piece);
        run.frame(&piece, &bones_at(0.), STEP);
        assert_eq!(run.frame(&piece, &bones_at(4.), 0.5), MAX_STEPS);
        for x in &run.pos {
            assert!(x.iter().all(|v| v.is_finite()) && dot(*x, *x) < 4., "{x:?}");
        }
    }
    #[test]
    fn a_snap_without_a_restart_moves_the_cloth_with_the_body() {
        // A correction moves the body 5 m in one drawn frame, unflagged (no restart). The cloth
        // lives in the piece frame, so it moves with the body; the frame's speed (10 m/s) and
        // acceleration (MaxAcceleration) clamps bound the push the jump gives it.
        let piece = strip(Vec::new(), still());
        let mut run = Run::new(&piece);
        for _ in 0..60 {
            run.frame(&piece, &bones_at(0.), 1. / 60.);
        }
        let settled = run.pos.clone();
        let (mut longest_pair, mut farthest) = (0f32, 0f32);
        for f in 0..120 {
            run.frame(&piece, &bones_at(if f == 0 { 0. } else { 5. }), 1. / 60.);
            longest_pair = longest_pair.max(longest(&piece, &run.pos));
            for (a, b) in run.pos.iter().zip(&settled) {
                farthest = farthest.max(dot(sub(*a, *b), sub(*a, *b)).sqrt());
            }
        }
        println!("after a 5 m snap: longest pair {longest_pair:.3} of rest, farthest point {farthest:.3} m from where it hung");
        assert!(longest_pair < 1.1, "a pair stretched to {longest_pair:.3}");
        assert!(farthest < 0.1, "a point went {farthest:.3} m from where it hung");
        for (a, b) in run.pos.iter().zip(&settled) {
            assert!(dot(sub(*a, *b), sub(*a, *b)).sqrt() < 0.02, "the cloth settles back: {a:?} against {b:?}");
        }
    }
    #[test]
    fn a_pose_that_jumps_restarts_the_cloth_only_at_a_clip_change() {
        // The fixed points' joint jumps 1 m up within one 60 Hz frame (60 m/s; no body moves so) and stays.
        // As in every stock piece, the start table carries the rest shape on the pose (here all on that joint).
        let mut piece = strip(Vec::new(), still());
        piece.start = piece.rest.iter().map(|&r| [(0, r, 1.), (0, r, 0.), (0, r, 0.)]).collect();
        let mut up = bones_at(0.);
        up[1].set_position(0., 1., 0.);
        // How far the highest free point stands above the lowest fixed one (below zero: hanging).
        let above = |pos: &[V3]| {
            let free = pos[piece.fixed..].iter().map(|x| x[1]).fold(f32::MIN, f32::max);
            free - pos[..piece.fixed].iter().map(|x| x[1]).fold(f32::MAX, f32::min)
        };
        let mut runs = Vec::new();
        for (guarded, crossing) in [(false, false), (true, true), (true, false)] {
            let mut run = Run::new(&piece);
            for _ in 0..60 {
                run.frame(&piece, &bones_at(0.), 1. / 60.);
            }
            let mut highest = f32::MIN;
            let mut restarts = 0;
            for f in 0..30 {
                if guarded {
                    // The jump's frame is the one whose pose crosses the clip change, when there is one.
                    restarts += usize::from(run.frame_restarts(&piece, &up, 1. / 60., crossing && f == 0).1);
                } else {
                    // The steps alone, as before the rule: the jump goes into the cloth.
                    run.advance_only(&piece, &up, 1. / 60.);
                }
                highest = highest.max(above(&run.pos));
            }
            let how = match (guarded, crossing) {
                (false, _) => "steps alone",
                (true, true) => "restart rule, at a clip change",
                (true, false) => "restart rule, inside a clip",
            };
            println!("1 m pose jump, {how}: free points up to {highest:.3} m above the fixed ones, {restarts} restarts");
            runs.push((highest, restarts));
        }
        assert!(runs[0].0 > 0., "without the rule the jump throws the strip above its fixed points ({:.3})", runs[0].0);
        assert_eq!(runs[1].1, 1, "the jump at a clip change restarts the piece once");
        assert!(runs[1].0 < 0., "after the restart the strip hangs below its fixed points ({:.3})", runs[1].0);
        assert_eq!(runs[2].1, 0, "a jump inside a clip never restarts the piece");
        assert_eq!(runs[2].0, runs[0].0, "inside a clip the jump goes into the cloth as the steps alone take it");
        // A swing a body makes (0.5 m out and back in 0.3 s, at most 5.2 m/s) never restarts.
        let mut run = Run::new(&piece);
        for f in 0..60 {
            let t = (f + 1) as f64 / 60.;
            let mut bones = bones_at(0.);
            bones[1].set_position(if t < 0.3 { 0.5 * (std::f64::consts::PI * t / 0.3).sin() } else { 0. }, 0., 0.);
            assert!(!run.frame_restarts(&piece, &bones, 1. / 60., true).1, "a 5 m/s swing restarted the piece at {t:.3} s");
        }
    }
    #[test]
    fn the_body_moving_forward_trails_the_cloth_behind() {
        let piece = strip(Vec::new(), still());
        let mut run = Run::new(&piece);
        for f in 0..120 {
            run.frame(&piece, &bones_at(f64::from(f) * 0.1), STEP);
        }
        // Drag against the air pushes the free vertices back (toward -X in the piece frame).
        let mean = run.pos[2..].iter().map(|x| x[0]).sum::<f32>() / 4.;
        assert!(mean < -0.05, "{mean}");
    }
    #[test]
    fn coming_back_into_view_does_not_kick_the_cloth() {
        let piece = strip(Vec::new(), still());
        let moved = |reseed: bool| {
            let mut run = Run::new(&piece);
            run.frame(&piece, &bones_at(0.), STEP);
            if reseed {
                run.clock.reseed();
                run.frame(&piece, &bones_at(5.), 0.5);
            } else {
                run.advance_only(&piece, &bones_at(5.), 0.5);
            }
            run.pos[2..].iter().zip(&piece.rest[2..]).map(|(x, r)| (x[0] - r[0]).abs()).fold(0f32, f32::max)
        };
        assert!(moved(false) > 0.02, "the body's jump drags the cloth when nothing reseeds it");
        assert!(moved(true) < 0.005, "{}", moved(true));
    }
    /// `cargo test --release ... cost_of_a_large_piece -- --ignored --nocapture`: what one step and one
    /// drawn frame of an invented 23 x 10 grid with ten spheres cost on this machine.
    /// A grid piece of `n` points about 23 wide (pairs to the next and second-next point across, down
    /// and diagonally), points 0 to `fixed` on joint 0 and `spheres` spheres on joint 1.
    fn grid(n: usize, fixed: usize, spheres: usize) -> Piece {
        let w = n.min(23);
        let rest: Vec<V3> = (0..n).map(|i| [(i % w) as f32 * 0.04 - 0.44, 1.2 - (i / w) as f32 * 0.09, 0.]).collect();
        let mut links = Vec::new();
        let mut triangles = Vec::new();
        for i in 0..n {
            let (x, y) = (i % w, i / w);
            for (dx, dy) in [(1, 0), (0, 1), (1, 1), (2, 0), (0, 2)] {
                let j = i + dx + dy * w;
                if x + dx < w && j < n {
                    let d = sub(rest[j], rest[i]);
                    links.push(Link { a: i as u16, b: j as u16, rest: dot(d, d).sqrt() });
                }
            }
            if x + 1 < w && i + w + 1 < n {
                triangles.push([i as u16, (i + w) as u16, (i + 1) as u16]);
                triangles.push([(i + 1) as u16, (i + w) as u16, (i + w + 1) as u16]);
            }
            let _ = y;
        }
        let shapes = (0..spheres)
            .map(|k| {
                let mut local = Matrix4::default();
                local.set_position(0., 0.4 + 0.08 * k as f64, -0.15);
                Shape { joint: 1, kind: Kind::Sphere, size: [0.15; 3], local }
            })
            .collect();
        Piece {
            parent: 0,
            placement: Matrix4::default(),
            uvs: vec![[0., 0.]; rest.len()],
            fixed,
            pins: (0..fixed).map(|i| (0, rest[i])).collect(),
            joints: vec![1],
            rest,
            links,
            shapes,
            triangles,
            params: Params::default(),
            start: Vec::new(),
        }
    }
    /// Microseconds one step and one draw of `piece` take, the body walking.
    fn cost(piece: &Piece) -> f64 {
        let mut run = Run::new(piece);
        let mut out = vec![0.; piece.rest.len() * 8];
        let frames = 20_000;
        let started = std::time::Instant::now();
        for f in 0..frames {
            run.frame(piece, &bones_at(f64::from(f) * 0.05), STEP);
            let p = Particles { pos: &mut run.pos, old: &mut run.old, normal: &mut run.normal };
            piece.draw(&p, &run.clock, &run.scratch, &mut out);
        }
        assert!(run.pos.iter().flatten().all(|v| v.is_finite()));
        started.elapsed().as_secs_f64() * 1e6 / f64::from(frames)
    }
    #[test]
    #[ignore = "timing"]
    fn cost_of_a_large_piece() {
        let piece = grid(230, 23, 10);
        let us = cost(&piece);
        println!("cloth cost: {} vertices, {} pairs, {} triangles, 10 spheres: {us:.2} us per step and drawn frame", piece.rest.len(), piece.links.len(), piece.triangles.len());
    }
    #[test]
    #[ignore = "timing"]
    fn cost_by_piece_size() {
        // Invented pieces from 25 to 250 points (a third of them fixed, one sphere per 50 points).
        let mut all = 0.;
        for n in [25usize, 50, 75, 100, 125, 150, 175, 200, 225, 250] {
            let piece = grid(n, n / 3, n / 50);
            let us = cost(&piece);
            // Steps a drawn frame: 2.4 at 60 frames a second, 1 at 144.
            println!("cloth cost {n} points: {} pairs, {us:.2} us a step; a drawn frame {:.1} us at 60, {us:.1} us at 144", piece.links.len(), us * 144. / 60.);
            all += us;
        }
        println!("cloth cost all ten sizes: {all:.1} us a step");
    }
    /// The longest pair with a free end against its rest length.
    fn longest(piece: &Piece, pos: &[V3]) -> f32 {
        piece
            .links
            .iter()
            .filter(|l| usize::from(l.a.max(l.b)) >= piece.fixed)
            .map(|l| dot(sub(pos[usize::from(l.b)], pos[usize::from(l.a)]), sub(pos[usize::from(l.b)], pos[usize::from(l.a)])).sqrt() / l.rest)
            .fold(0f32, f32::max)
    }
    #[test]
    fn a_strip_keeps_its_length_when_its_joint_swings_fast() {
        // The pins' joint swings 0.5 m sideways and back within the piece frame in 0.3 s (a head
        // whipping against the body), drawn at 60 frames a second.
        let piece = strip(Vec::new(), still());
        let mut run = Run::new(&piece);
        let mut worst = 0f32;
        for f in 0..120 {
            let t = f as f64 / 60.;
            let swing = if t < 0.3 { 0.5 * (std::f64::consts::PI * t / 0.3).sin() } else { 0. };
            let mut bones = bones_at(0.);
            bones[1].set_position(swing, 0., 0.);
            run.frame(&piece, &bones, 1. / 60.);
            worst = worst.max(longest(&piece, &run.pos));
        }
        println!("longest pair while swinging: {worst:.3}");
        assert!(worst < 1.2, "a pair stretched to {worst:.3} times its rest length");
    }
    #[test]
    fn the_gust_is_uniform_in_the_unit_interval() {
        let mut clock = Clock::new(1);
        let draws: Vec<f32> = (0..4000).map(|_| clock.gust()).collect();
        assert!(draws.iter().all(|v| (0. ..1.).contains(v)));
        let mean = draws.iter().sum::<f32>() / draws.len() as f32;
        assert!((mean - 0.5).abs() < 0.03, "{mean}");
    }
}
