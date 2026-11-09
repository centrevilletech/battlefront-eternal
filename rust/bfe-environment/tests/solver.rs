//! The pieces' rigid bodies: they tumble, settle lying flat on a face and
//! sleep without sinking, a rod tips over, they bounce off walls, and a
//! sleeping piece costs no rays.

use bfe_environment::solver::{Hit, Piece, Shape, Solver};
use three::{Quaternion, Vector3};

/// A box piece's shape: its eight corners, as pieceShape finds them.
fn box_shape(w: f64, h: f64, d: f64) -> Shape {
    let half = Vector3::new(w / 2.0, h / 2.0, d / 2.0);
    let mut points = Vec::new();
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                points.push(Vector3::new(x * half.x, y * half.y, z * half.z));
            }
        }
    }
    let radius = points.iter().map(|p| p.length()).fold(0.05, f64::max);
    let (x2, y2, z2) = (half.x * half.x, half.y * half.y, half.z * half.z);
    let inertia = Vector3::new(((y2 + z2) / 3.0).max(0.002), ((x2 + z2) / 3.0).max(0.002), ((x2 + y2) / 3.0).max(0.002));
    Shape { center: Vector3::default(), half, points, radius, inverse_inertia: Vector3::new(1.0 / inertia.x, 1.0 / inertia.y, 1.0 / inertia.z) }
}

/// Half-spaces as a collision world: the nearest plane a ray enters.
struct World {
    planes: Vec<(Vector3, Vector3)>,
    casts: u32,
}

impl World {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<Hit> {
        self.casts += 1;
        let mut best: Option<(f64, usize)> = None;
        for (i, (normal, point)) in self.planes.iter().enumerate() {
            let into = direction.dot(normal);
            if into >= -1e-9 {
                continue;
            }
            let mut d = Vector3::default();
            d.sub_vectors(point, origin);
            let t = normal.dot(&d) / into;
            if t < -1e-6 || t > far {
                continue;
            }
            if best.is_none_or(|(b, _)| t < b) {
                best = Some((t, i));
            }
        }
        best.map(|(t, i)| {
            let mut p = *origin;
            p.add_scaled_vector(direction, t);
            Hit { point: p, normal: self.planes[i].0 }
        })
    }
}

fn floor() -> (Vector3, Vector3) {
    (Vector3::new(0.0, 1.0, 0.0), Vector3::default())
}

struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 as f64 / 4294967296.0
    }
}

/// The lowest point of a piece's hull above a horizontal floor.
fn clearance(shapes: &[Shape], piece: &Piece) -> f64 {
    shapes[piece.shape].points.iter().map(|p| {
        let mut w = *p;
        w.apply_matrix3(piece.rotation());
        piece.position.y + w.y
    }).fold(f64::INFINITY, f64::min)
}

fn run(shapes: &[Shape], world: &mut World, piece: &mut Piece, seconds: f64) -> (f64, Option<f64>) {
    let mut solver = Solver::new();
    let (mut deepest, mut slept) = (f64::INFINITY, None);
    let mut t = 0.0;
    while t < seconds {
        solver.step(shapes, std::slice::from_mut(piece), 1.0 / 60.0, &mut |o, d, f| world.cast(o, d, f));
        deepest = deepest.min(clearance(shapes, piece));
        if piece.sleeping && slept.is_none() {
            slept = Some(t);
        }
        t += 1.0 / 60.0;
    }
    (deepest, slept)
}

#[test]
fn thrown_pieces_settle_lying_flat_and_sleep_without_sinking() {
    let mut random = Lcg(17);
    for (w, h, d) in [(0.6, 0.12, 0.35), (1.0, 0.12, 0.12), (0.35, 0.35, 0.35), (0.7, 0.25, 0.5)] {
        for _ in 0..4 {
            let shapes = [box_shape(w, h, d)];
            let mut q = Quaternion::default();
            q.set(random.next() - 0.5, random.next() - 0.5, random.next() - 0.5, random.next() + 0.1).normalize();
            let mut v = Vector3::new(random.next() - 0.5, 0.6 + random.next(), random.next() - 0.5);
            v.normalize().multiply_scalar(6.0);
            let mut spin = Vector3::new(random.next() - 0.5, random.next() - 0.5, random.next() - 0.5);
            spin.normalize().multiply_scalar(10.0 * random.next());
            let mut piece = Piece::new(&shapes, 0, &Vector3::new(0.0, 1.2, 0.0), &q, &v, &spin);
            let mut world = World { planes: vec![floor()], casts: 0 };
            let (deepest, slept) = run(&shapes, &mut world, &mut piece, 6.0);
            assert!(deepest > -0.01, "{w}x{h}x{d} sank {deepest} m into the floor");
            assert!(slept.is_some_and(|t| t < 4.0), "{w}x{h}x{d} still moving after 4 s");
            assert!(clearance(&shapes, &piece).abs() < 0.01, "resting on the floor, not above it");
            // It lies on a face: one of its own axes points up.
            let up = [Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, 1.0)].iter().map(|a| {
                let mut a = *a;
                a.apply_matrix3(piece.rotation());
                a.y.abs()
            }).fold(0.0, f64::max);
            assert!(up > 0.99, "{w}x{h}x{d} came to rest on an edge or corner");
        }
    }
}

#[test]
fn a_rod_landing_on_its_end_falls_over_rather_than_standing() {
    for tilt in [0.2, 0.03, 0.0] {
        let shapes = [box_shape(0.12, 1.0, 0.12)];
        let mut q = Quaternion::default();
        q.set(0.0, 0.0, (tilt / 2.0f64).sin(), (tilt / 2.0f64).cos());
        let mut piece = Piece::new(&shapes, 0, &Vector3::new(0.0, 0.52, 0.0), &q, &Vector3::default(), &Vector3::default());
        let mut world = World { planes: vec![floor()], casts: 0 };
        run(&shapes, &mut world, &mut piece, 6.0);
        let mut long = Vector3::new(0.0, 1.0, 0.0);
        long.apply_matrix3(piece.rotation());
        assert!(long.y.abs() < 0.1, "tilted {tilt}: the rod's long axis should lie flat, y = {}", long.y);
        assert!(piece.position.y < 0.1 && piece.sleeping, "lying down, asleep");
    }
}

#[test]
fn pieces_bounce_off_walls_at_any_frame_rate() {
    for hz in [30.0, 60.0, 144.0] {
        let shapes = [box_shape(0.3, 0.12, 0.12)];
        let mut piece = Piece::new(&shapes, 0, &Vector3::new(0.0, 0.5, 0.0), &Quaternion::default(), &Vector3::new(12.0, 0.0, 0.0), &Vector3::default());
        let mut world = World { planes: vec![floor(), (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(2.0, 0.0, 0.0))], casts: 0 };
        let mut solver = Solver::new();
        let mut t = 0.0;
        while t < 2.0 {
            solver.step(&shapes, std::slice::from_mut(&mut piece), 1.0 / hz, &mut |o, d, f| world.cast(o, d, f));
            assert!(piece.position.x < 2.0 + 0.02, "{hz} Hz: through the wall at {}", piece.position.x);
            t += 1.0 / hz;
        }
    }
}

#[test]
fn a_sleeping_piece_costs_no_rays() {
    let shapes = [box_shape(0.35, 0.35, 0.35)];
    let mut piece = Piece::new(&shapes, 0, &Vector3::new(0.0, 0.18, 0.0), &Quaternion::default(), &Vector3::default(), &Vector3::default());
    let mut world = World { planes: vec![floor()], casts: 0 };
    run(&shapes, &mut world, &mut piece, 3.0);
    assert!(piece.sleeping);
    let before = world.casts;
    run(&shapes, &mut world, &mut piece, 1.0);
    assert_eq!(world.casts, before, "a sleeping piece casts nothing");
}

fn slope(angle: f64) -> (Vector3, Vector3) {
    (Vector3::new(angle.sin(), angle.cos(), 0.0), Vector3::default())
}

/// How far a piece resting on a slope slides in three seconds.
fn slide(angle: f64) -> f64 {
    let shapes = [box_shape(0.3, 0.12, 0.2)];
    let (normal, point) = slope(angle);
    let mut start = Vector3::new(0.0, 0.0, 0.0);
    start.add_scaled_vector(&normal, 0.07);
    let mut q = Quaternion::default();
    q.set(0.0, 0.0, -(angle / 2.0).sin(), (angle / 2.0).cos());
    let mut piece = Piece::new(&shapes, 0, &start, &q, &Vector3::default(), &Vector3::default());
    let mut world = World { planes: vec![(normal, point)], casts: 0 };
    run(&shapes, &mut world, &mut piece, 3.0);
    piece.position.distance_to(&start)
}

#[test]
fn friction_holds_a_piece_on_a_gentle_slope_and_lets_it_go_on_a_steep_one() {
    assert!(slide(0.2) < 0.5, "a gentle slope holds it: {}", slide(0.2));
    assert!(slide(0.9) > 2.0, "a steep one lets it go: {}", slide(0.9));
}

#[test]
fn a_pile_of_pieces_separates_settles_and_sleeps() {
    for trial in 1..=4u32 {
        let mut random = Lcg(trial * 977);
        let shapes: Vec<Shape> = [(0.6, 0.15, 0.35), (1.0, 0.12, 0.12), (0.35, 0.35, 0.35), (0.7, 0.25, 0.5), (0.4, 0.1, 0.4), (0.5, 0.2, 0.2)].iter().map(|s| box_shape(s.0, s.1, s.2)).collect();
        let mut pieces: Vec<Piece> = (0..shapes.len())
            .map(|i| {
                // (The draws in the order the piece is described: place, turn, motion, spin.)
                let at = Vector3::new((random.next() - 0.5) * 0.4, 0.6 + i as f64 * 0.35, (random.next() - 0.5) * 0.4);
                let euler = three::Euler { x: random.next() * 6.0, y: random.next() * 6.0, z: random.next() * 6.0, order: three::EulerOrder::XYZ };
                let mut q = Quaternion::default();
                q.set_from_euler(&euler);
                let v = Vector3::new(random.next() - 0.5, 0.0, random.next() - 0.5);
                let w = Vector3::new(random.next() * 4.0, random.next() * 4.0, random.next() * 4.0);
                Piece::new(&shapes, i, &at, &q, &v, &w)
            })
            .collect();
        for p in pieces.iter_mut() {
            p.flight = 1.0;
        }
        let mut world = World { planes: vec![floor()], casts: 0 };
        let mut solver = Solver::new();
        let mut t = 0.0;
        while t < 10.0 {
            solver.step(&shapes, &mut pieces, 1.0 / 60.0, &mut |o, d, f| world.cast(o, d, f));
            t += 1.0 / 60.0;
        }
        assert!(pieces.iter().all(|p| p.sleeping), "trial {trial}: still moving after 10 s");
        for p in &pieces {
            assert!(clearance(&shapes, p) > -0.01, "trial {trial}: sank into the floor");
        }
        // No corner deep inside another piece.
        let mut worst: f64 = 0.0;
        for (i, a) in pieces.iter().enumerate() {
            for (k, b) in pieces.iter().enumerate() {
                if i == k {
                    continue;
                }
                let mut inverse = three::Matrix3::default();
                inverse.copy(b.rotation());
                inverse.transpose();
                for point in &shapes[a.shape].points {
                    let mut local = *point;
                    local.apply_matrix3(a.rotation()).add(&a.position).sub(&b.position).apply_matrix3(&inverse);
                    let h = shapes[b.shape].half;
                    worst = worst.max((h.x - local.x.abs()).min(h.y - local.y.abs()).min(h.z - local.z.abs()));
                }
            }
        }
        assert!(worst < 0.06, "trial {trial}: a corner {worst} m inside another piece");
    }
}
