//! The pieces of a droid blown apart as rigid bodies (src/destruction-debris.js
//! RigidPiece and PieceSolver): a centre of mass, an orientation, linear and
//! angular velocity, the inertia of a box that size, and a few dozen extreme
//! vertices that touch the world. Each frame a ray straight down from the
//! centre finds the ground under it and a ray along its motion what it is
//! about to hit; both count as planes. Contacts push back with an impulse at
//! the touching point, with Coulomb friction and a bounce that only fast
//! impacts keep. Pieces tumble, roll over and come to rest lying on a face.
//!
//! They are only for show: they touch no soldier or round, and nothing in
//! the simulation reads them.

use three::{Matrix3, Matrix4, Quaternion, Vector3};

// Constants of the pieces' feel, tuned by eye against the 2004 game: they fall
// weightier than 1 g, bounce little, and come to rest quickly. Times are
// seconds, speeds m/s.
const GRAVITY: f64 = 11.0; // m/s², a touch over 1 g: pieces this size read weightier
const AIR_DRAG: f64 = 0.08; // 1/s
// The solver steps at 120 Hz or finer (up to six substeps a frame) so a
// fast piece never passes through a thin wall and a rest is the same at any
// frame rate.
const SUBSTEP: f64 = 1.0 / 120.0;
const MAX_SUBSTEPS: f64 = 6.0;
const ITERATIONS: usize = 4;
const RESTITUTION: f64 = 0.3;
const BOUNCE_MIN: f64 = 1.4; // m/s into a surface before anything bounces
const FRICTION: f64 = 0.62;
// Overlap left alone, so resting contact does not jitter.
const SLOP: f64 = 0.004; // m of overlap left alone, so resting contact doesn't jitter
const CONTACT_DAMP: f64 = 1.6; // 1/s, rolling resistance while touching
const SETTLE_DAMP: f64 = 5.0; // 1/s once nearly still
// A piece at rest this still for this long sleeps (costs no rays, takes no
// steps) until it is let go. A "drowsy" one that only buzzes against its
// neighbours sleeps too, after longer.
const SLEEP_SPEED: f64 = 0.12; // m/s
const SLEEP_SPIN: f64 = 0.4; // rad/s
const SLEEP_TIME: f64 = 0.3; // s
const DROWSY_SPEED: f64 = 0.6;
const DROWSY_SPIN: f64 = 1.4;
const DROWSY_TIME: f64 = 1.2;
// A long piece that comes to rest standing on end is tipped over (up to
// twice) the way wreckage falls, not left standing like a statue.
const TALL: f64 = 1.8; // centre this many smallest half sizes up: standing on end
const NUDGES: u32 = 2; // tips given to a piece balanced on end before it may rest
const NUDGE_SPIN: f64 = 2.6; // rad/s
// The ground under a piece is looked for again only once it has moved off
// the spot (or now and then in the air): rays are the dear part of the solver.
const GROUND_STEP: f64 = 0.05; // m a touching piece moves before the ground is looked for again
const GROUND_REST: f64 = 0.25; // s ... or this long, touching
const GROUND_FLYING: f64 = 0.1; // s between looks in the air
const PAIR_DELAY: f64 = 0.12; // s of flight before pieces of one body push each other
const PAIR_SHRINK: f64 = 0.85; // a piece's box, scaled, for other pieces to touch
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);

/// A piece's shape from its model: the centre of its box, the extreme
/// vertices (from that centre), the box's half size, its reach, and the
/// inverse inertia of a 1 kg box that size (pieceShape).
// What the phase knows of a piece's model: all of it worked out by the page
// from the model's vertices, once, and registered.
pub struct Shape {
    pub center: Vector3,
    pub half: Vector3,
    pub points: Vec<Vector3>,
    pub radius: f64,
    pub inverse_inertia: Vector3,
}

/// What a cast of the page's collision world answers: the point it hit and
/// the surface's normal, or nothing.
pub struct Hit {
    pub point: Vector3,
    pub normal: Vector3,
}
pub type Cast<'a> = &'a mut dyn FnMut(&Vector3, &Vector3, f64) -> Option<Hit>;

#[derive(Clone, Copy)]
struct Plane {
    normal: Vector3,
    point: Vector3,
}

/// The ground last found under a piece: where it was looked for, and when.
#[derive(Clone, Copy)]
pub struct Ground {
    pub found: bool,
    pub normal: Vector3,
    pub point: Vector3,
    at: Vector3,
    age: f64,
}

pub struct Piece {
    pub shape: usize,
    pub position: Vector3,
    pub quaternion: Quaternion,
    pub velocity: Vector3,
    pub spin: Vector3,
    rotation: Matrix3,
    inverse_inertia: Matrix3,
    planes: [Plane; 2],
    plane_count: usize,
    pub ground: Ground,
    pub sleeping: bool,
    still: f64,
    drowsy: f64,
    nudges: u32,
    pub touching: bool,
    pub flight: f64,
    /// The hardest landing since it was last read, m/s, and where.
    pub impact: f64,
    pub impact_point: Vector3,
    pub lowest: f64,
}

fn rotation_of(q: &Quaternion) -> Matrix3 {
    let mut m4 = Matrix4::default();
    m4.make_rotation_from_quaternion(q);
    let mut r = Matrix3::default();
    r.set_from_matrix4(&m4);
    r
}

// A piece's rotation matrix and world-space inverse inertia follow from its
// quaternion; they are rebuilt whenever it turns.
impl Piece {
    /// A body at `position` turned by `quaternion` (RigidPiece.reset).
    pub fn new(shapes: &[Shape], shape: usize, position: &Vector3, quaternion: &Quaternion, velocity: &Vector3, spin: &Vector3) -> Piece {
        let mut q = *quaternion;
        q.normalize();
        let p = Plane { normal: Vector3::default(), point: Vector3::default() };
        let mut piece = Piece {
            shape,
            position: *position,
            quaternion: q,
            velocity: *velocity,
            spin: *spin,
            rotation: Matrix3::default(),
            inverse_inertia: Matrix3::default(),
            planes: [p, p],
            plane_count: 0,
            ground: Ground { found: false, normal: Vector3::default(), point: Vector3::default(), at: Vector3::default(), age: f64::INFINITY },
            sleeping: false,
            still: 0.0,
            drowsy: 0.0,
            nudges: 0,
            touching: false,
            flight: 0.0,
            impact: 0.0,
            impact_point: Vector3::default(),
            lowest: 0.0,
        };
        piece.orient(shapes);
        piece
    }

    /// Its rotation matrix and world-space inverse inertia, from its
    /// quaternion (R diag(k) R^T, written out).
    pub fn orient(&mut self, shapes: &[Shape]) {
        self.rotation = rotation_of(&self.quaternion);
        let e = self.rotation.elements;
        let k = shapes[self.shape].inverse_inertia;
        let i = &mut self.inverse_inertia.elements;
        for row in 0..3 {
            for col in 0..3 {
                i[col * 3 + row] = e[row] * k.x * e[col] + e[3 + row] * k.y * e[3 + col] + e[6 + row] * k.z * e[6 + col];
            }
        }
    }

    pub fn rotation(&self) -> &Matrix3 {
        &self.rotation
    }
}

/// How easily one body gives at offset r along unit direction n (its inverse
/// effective mass): 1/m + n . ((I^-1 (r x n)) x r), with m = 1 (reach).
// How easily a body gives at a point, along a direction: the reciprocal of its
// effective mass there. An impulse that changes the speed there by dv is dv
// over this. (Mass is 1: pieces differ by size, not by weight.)
fn reach(piece: &Piece, r: &Vector3, n: &Vector3) -> f64 {
    let mut a = Vector3::default();
    a.cross_vectors(r, n).apply_matrix3(&piece.inverse_inertia);
    let mut b = Vector3::default();
    1.0 + n.dot(b.cross_vectors(&a, r))
}

// An impulse at a point: it changes the body's velocity and, because it is off
// the centre, its spin (so a piece landing on a corner turns over).
fn push(piece: &mut Piece, r: &Vector3, direction: &Vector3, amount: f64) {
    piece.velocity.add_scaled_vector(direction, amount);
    let mut a = Vector3::default();
    a.cross_vectors(r, direction).apply_matrix3(&piece.inverse_inertia);
    piece.spin.add_scaled_vector(&a, amount);
}

/// Steps every awake piece against the world.
pub struct Solver {
    pub rays: u32,
}

impl Solver {
    pub fn new() -> Solver {
        Solver { rays: 0 }
    }

    /// What a piece may touch this frame: the ground under its centre, and
    /// whatever lies along its motion. The ground is looked for again only
    /// once a piece on it has moved off the spot (or now and then in the
    /// air) (probe).
    // What a piece may touch this frame becomes planes: the ground under its
    // centre and what lies along its motion (only if it is moving into it).
    fn probe(&mut self, shapes: &[Shape], piece: &mut Piece, dt: f64, touched: bool, cast: Cast) {
        let r = shapes[piece.shape].radius;
        piece.ground.age += dt;
        let stale = if touched {
            piece.ground.age >= GROUND_REST || piece.ground.at.distance_to_squared(&piece.position) > GROUND_STEP * GROUND_STEP
        } else {
            piece.ground.age >= GROUND_FLYING
        };
        if stale {
            piece.ground.age = 0.0;
            piece.ground.at = piece.position;
            let mut origin = piece.position;
            origin.add_scaled_vector(&UP, 0.05);
            self.rays += 1;
            let far = r + 0.6 + jsrt::conv::js_max(-piece.velocity.y, 0.0) * GROUND_FLYING;
            match cast(&origin, &DOWN, far) {
                Some(hit) => {
                    piece.ground.found = true;
                    piece.ground.normal = hit.normal;
                    piece.ground.normal.normalize();
                    piece.ground.point = hit.point;
                }
                None => piece.ground.found = false,
            }
        }
        piece.plane_count = 0;
        if piece.ground.found {
            let (n, p) = (piece.ground.normal, piece.ground.point);
            Self::plane(piece, &n, &p);
        }
        let speed = piece.velocity.length();
        if speed > 0.25 {
            let mut dir = piece.velocity;
            dir.divide_scalar(speed);
            self.rays += 1;
            if let Some(hit) = cast(&piece.position, &dir, r + speed * dt + 0.05) {
                if hit.normal.dot(&dir) < -0.05 {
                    Self::plane(piece, &hit.normal, &hit.point);
                }
            }
        }
    }

    // Two planes at most; one that is the same as one it knows is not added twice.
    fn plane(piece: &mut Piece, normal: &Vector3, point: &Vector3) {
        for i in 0..piece.plane_count {
            let known = piece.planes[i];
            let mut a = Vector3::default();
            a.sub_vectors(point, &known.point);
            if known.normal.dot(normal) > 0.98 && known.normal.dot(&a).abs() < 0.02 {
                return;
            }
        }
        if piece.plane_count >= piece.planes.len() {
            return;
        }
        let plane = &mut piece.planes[piece.plane_count];
        piece.plane_count += 1;
        plane.normal = *normal;
        plane.normal.normalize();
        plane.point = *point;
    }

    // Steps every awake piece. Probing happens once a frame; integration and
    // contact happen in substeps; pieces push each other apart each substep; at
    // the end a piece still and touching long enough sleeps.
    pub fn step(&mut self, shapes: &[Shape], pieces: &mut [Piece], dt: f64, cast: Cast) {
        if dt <= 0.0 {
            return;
        }
        for piece in pieces.iter_mut() {
            if !piece.sleeping {
                // A piece that touched something last frame looks for the ground again only
                // when it has moved off the spot; one in the air only now and then.
                let touched = piece.touching;
                piece.touching = false;
                self.probe(shapes, piece, dt, touched, cast);
            }
        }
        // Whole substeps of at most 1/120 s, and at most six: a very long frame
        // (the game was paused) is cut short, not spent.
        let n = jsrt::conv::js_min(jsrt::conv::js_max((dt / SUBSTEP).ceil(), 1.0), MAX_SUBSTEPS);
        let h = dt / n;
        for _ in 0..n as usize {
            for piece in pieces.iter_mut() {
                if piece.sleeping {
                    continue;
                }
                Self::integrate(shapes, piece, h);
                Self::contacts(shapes, piece, h);
            }
            Self::pairs(shapes, pieces);
        }
        for piece in pieces.iter_mut() {
            if piece.sleeping {
                continue;
            }
            // How long it has been in play: pieces of one blast fly out together before
            // they push each other apart.
            piece.flight += dt;
            let speed = piece.velocity.length_sq();
            let spin = piece.spin.length_sq();
            let slow = speed < SLEEP_SPEED * SLEEP_SPEED && spin < SLEEP_SPIN * SLEEP_SPIN;
            // Still while touching long enough means it is at rest; the drowsy test is
            // the same with looser limits.
            piece.still = if piece.touching && slow { piece.still + dt } else { 0.0 };
            piece.drowsy = if piece.touching && speed < DROWSY_SPEED * DROWSY_SPEED && spin < DROWSY_SPIN * DROWSY_SPIN { piece.drowsy + dt } else { 0.0 };
            if piece.still >= SLEEP_TIME || piece.drowsy >= DROWSY_TIME {
                // A piece balanced on end is tipped and stays awake; otherwise it sleeps and
                // its motion is zeroed.
                if Self::balanced(shapes, piece) {
                    continue;
                }
                piece.sleeping = true;
                piece.velocity.set(0.0, 0.0, 0.0);
                piece.spin.set(0.0, 0.0, 0.0);
            }
        }
    }

    /// A long piece come to rest standing on end gets a tip, so pieces lie
    /// down the way wreckage does. Returns whether it was tipped (balanced).
    // If a long piece has come to rest standing on end, tip it about a level axis
    // across its longest way up (alternating sides), and say so.
    fn balanced(shapes: &[Shape], piece: &mut Piece) -> bool {
        if piece.nudges >= NUDGES || piece.plane_count == 0 {
            return false;
        }
        let (normal, point) = (piece.planes[0].normal, piece.planes[0].point);
        let mut d = Vector3::default();
        d.sub_vectors(&piece.position, &point);
        // How high its centre is over the plane under it.
        let height = normal.dot(&d);
        let half = shapes[piece.shape].half;
        if height <= TALL * jsrt::conv::js_min(jsrt::conv::js_min(half.x, half.y), half.z) + 0.02 {
            return false;
        }
        piece.nudges += 1;
        piece.still = 0.0;
        piece.drowsy = 0.0;
        // About a level axis, across the piece's longest way up.
        // The tip is about a level axis across the piece's longest way up.
        let mut t = Vector3::new(1.0, 0.0, 0.0);
        if half.y >= half.x && half.y >= half.z {
            t.set(0.0, 1.0, 0.0);
        } else if half.z >= half.x {
            t.set(0.0, 0.0, 1.0);
        }
        t.apply_matrix3(&piece.rotation);
        let along = t.dot(&normal);
        t.add_scaled_vector(&normal, -along);
        let mut a = Vector3::default();
        if t.length_sq() < 1e-4 {
            t.set(normal.y, -normal.x, 0.0);
            t.add(a.set(0.3, 0.0, 0.7));
        }
        t.normalize();
        a.cross_vectors(&normal, &t);
        a.normalize();
        let sign = if piece.nudges % 2 != 0 { 1.0 } else { -1.0 };
        piece.spin.add_scaled_vector(&a, NUDGE_SPIN * sign);
        piece.velocity.add_scaled_vector(&t, 0.15);
        true
    }

    fn integrate(shapes: &[Shape], piece: &mut Piece, h: f64) {
        // Gravity, a little air drag, then the move; the spin turns the quaternion by
        // half the angle a step (q += (w/2) q h) and it is renormalized.
        piece.velocity.y -= GRAVITY * h;
        piece.velocity.multiply_scalar(1.0 - AIR_DRAG * h);
        let v = piece.velocity;
        piece.position.add_scaled_vector(&v, h);
        let w = piece.spin;
        let q = piece.quaternion;
        let mut dq = Quaternion::default();
        dq.set(w.x * h * 0.5, w.y * h * 0.5, w.z * h * 0.5, 0.0).multiply(&q);
        piece.quaternion.set(q.x + dq.x, q.y + dq.y, q.z + dq.z, q.w + dq.w).normalize();
        piece.orient(shapes);
    }

    /// Impulses where its points are in a plane, then out of it (contacts).
    // Impulses where the piece's points are in a plane: a push out (a bounce only
    // for a fast impact), friction against the sliding (at most mu times the
    // push), then the piece is moved out of the plane by what it sank. Rolling
    // resistance damps it while it touches, and more once it is nearly still.
    fn contacts(shapes: &[Shape], piece: &mut Piece, h: f64) {
        let shape = piece.shape;
        for p in 0..piece.plane_count {
            let (normal, point) = (piece.planes[p].normal, piece.planes[p].point);
            // Four passes of impulses over the plane's points (the first allows a
            // bounce), then the piece is lifted out by how deep it sank.
            let mut deepest = 0.0;
            for iteration in 0..ITERATIONS {
                for k in 0..shapes[shape].points.len() {
                    let local = shapes[shape].points[k];
                    let mut r = local;
                    r.apply_matrix3(&piece.rotation);
                    let mut world = Vector3::default();
                    world.add_vectors(&piece.position, &r);
                    let mut off = world;
                    off.sub(&point);
                    // How far this point is in the plane (negative: above it).
                    let depth = -normal.dot(&off);
                    if depth < 0.0 {
                        continue;
                    }
                    piece.touching = true;
                    if depth > deepest {
                        deepest = depth;
                    }
                    let mut vel = Vector3::default();
                    vel.cross_vectors(&piece.spin, &r).add(&piece.velocity);
                    let into = vel.dot(&normal);
                    if into >= 0.0 {
                        continue;
                    }
                    if iteration == 0 && -into > piece.impact {
                        piece.impact = -into;
                        // (Kept as the offset from the plane's point, as the page's code
                        // left it in its scratch vector: the dust puffs and sparks of a
                        // landing are laid there.)
                        piece.impact_point = off;
                    }
                    // Only a fast first impact keeps a bounce; resting contact keeps none, so
                    // pieces settle.
                    let target = if iteration == 0 && into < -BOUNCE_MIN { -RESTITUTION * into } else { 0.0 };
                    let j = (target - into) / reach(piece, &r, &normal);
                    push(piece, &r, &normal, j);
                    // Friction against the sliding, at most mu times the push.
                    vel.cross_vectors(&piece.spin, &r).add(&piece.velocity);
                    let along = vel.dot(&normal);
                    let mut t = vel;
                    t.add_scaled_vector(&normal, -along);
                    let slide = t.length();
                    if slide < 1e-5 {
                        continue;
                    }
                    t.divide_scalar(slide);
                    let amount = -jsrt::conv::js_min(slide / reach(piece, &r, &t), FRICTION * j);
                    push(piece, &r, &t, amount);
                }
            }
            if deepest > SLOP {
                piece.position.add_scaled_vector(&normal, deepest - SLOP);
            }
        }
        if piece.touching {
            let slow = piece.velocity.length_sq() < 0.36 && piece.spin.length_sq() < 2.5;
            let damp = v8math::exp(-(if slow { SETTLE_DAMP } else { CONTACT_DAMP }) * h);
            piece.spin.multiply_scalar(damp);
            if slow {
                piece.velocity.multiply_scalar(damp);
            }
        }
    }

    /// Pieces against each other: a point of one inside another's box is
    /// pushed out through the nearest face. A sleeping piece stays put
    /// (pairs).
    // Pieces against each other: those close enough (their reach together) are
    // tested corner against box. Pieces of one blast do not push each other in
    // their first moments (they fly out together).
    fn pairs(shapes: &[Shape], pieces: &mut [Piece]) {
        for i in 0..pieces.len() {
            for k in i + 1..pieces.len() {
                if pieces[i].sleeping && pieces[k].sleeping {
                    continue;
                }
                if pieces[i].flight < PAIR_DELAY || pieces[k].flight < PAIR_DELAY {
                    continue;
                }
                let span = shapes[pieces[i].shape].radius + shapes[pieces[k].shape].radius;
                if !(pieces[i].position.distance_to_squared(&pieces[k].position) <= span * span) {
                    continue;
                }
                // Two bodies of the one list, borrowed apart.
                let (left, right) = pieces.split_at_mut(k);
                let (a, b) = (&mut left[i], &mut right[0]);
                if !a.sleeping {
                    Self::inside(shapes, a, b);
                }
                if !b.sleeping {
                    Self::inside(shapes, b, a);
                }
            }
        }
    }

    // A point of one inside another's box (shrunk a little) is pushed out through
    // the nearest face, the pair sharing the move (a sleeping piece stays put and
    // the other moves all of it).
    fn inside(shapes: &[Shape], a: &mut Piece, b: &mut Piece) {
        let half = shapes[b.shape].half;
        for k in 0..shapes[a.shape].points.len() {
            let local = shapes[a.shape].points[k];
            let mut ra = local;
            ra.apply_matrix3(&a.rotation);
            let mut p = Vector3::default();
            p.add_vectors(&a.position, &ra);
            // Into b's frame: R^T (p - centre).
            let mut transposed = Matrix3::default();
            transposed.copy(&b.rotation);
            transposed.transpose();
            let mut local_p = Vector3::default();
            local_p.sub_vectors(&p, &b.position);
            local_p.apply_matrix3(&transposed);
            let ex = half.x * PAIR_SHRINK - local_p.x.abs();
            let ey = half.y * PAIR_SHRINK - local_p.y.abs();
            let ez = half.z * PAIR_SHRINK - local_p.z.abs();
            if !(ex > 0.0 && ey > 0.0 && ez > 0.0) {
                continue;
            }
            let depth = jsrt::conv::js_min(jsrt::conv::js_min(ex, ey), ez);
            // The face a corner is nearest, signed (a zero counts as positive, as the
            // page's `Math.sign(x) || 1` did).
            let sign = |v: f64| {
                let s = jsrt::conv::js_sign(v);
                if s == 0.0 || s.is_nan() { 1.0 } else { s }
            };
            let mut normal = Vector3::default();
            if depth == ex {
                normal.set(sign(local_p.x), 0.0, 0.0);
            } else if depth == ey {
                normal.set(0.0, sign(local_p.y), 0.0);
            } else {
                normal.set(0.0, 0.0, sign(local_p.z));
            }
            normal.apply_matrix3(&b.rotation).normalize();
            let mut rb = Vector3::default();
            rb.sub_vectors(&p, &b.position);
            let mut vel = Vector3::default();
            vel.cross_vectors(&a.spin, &ra).add(&a.velocity);
            if !b.sleeping {
                let mut vb = Vector3::default();
                vb.cross_vectors(&b.spin, &rb).add(&b.velocity);
                vel.sub(&vb);
            }
            let into = vel.dot(&normal);
            a.touching = true;
            if into < 0.0 {
                let response = reach(a, &ra, &normal) + if b.sleeping { 0.0 } else { reach(b, &rb, &normal) };
                let j = -into * 1.1 / response;
                push(a, &ra, &normal, j);
                if !b.sleeping {
                    push(b, &rb, &normal, -j);
                }
            }
            let share = if b.sleeping { 1.0 } else { 0.5 };
            a.position.add_scaled_vector(&normal, depth * share);
            if !b.sleeping {
                b.position.add_scaled_vector(&normal, -depth * share);
            }
        }
    }
}

impl Default for Solver {
    fn default() -> Self {
        Self::new()
    }
}
