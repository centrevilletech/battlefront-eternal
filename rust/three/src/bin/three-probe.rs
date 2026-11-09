//! The native half of the differential harness (tools/diff.mjs).
//! Each mode reads little-endian f64 cases of a fixed arity (the
//! harness writes them; the arities are per mode below) and writes
//! the f64 results. Modes marked (libm) reach a transcendental,
//! which jsrt's seam takes from rust/v8math; the harness gates them
//! with the rest.

use std::io::{Read, Write};

use three::euler::{Euler, EulerOrder};
use three::geom::*;
use three::mathutils as mu;
use three::matrix::{Matrix3, Matrix4};
use three::quaternion::Quaternion;
use three::transform::{TransformNode, TransformTree};
use three::vector::{Vector2, Vector3, Vector4};

fn read_f64s() -> Vec<f64> {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap();
    buf.as_chunks::<8>()
        .0
        .iter()
        .copied()
        .map(f64::from_le_bytes)
        .collect()
}

fn write_f64s(values: &[f64]) {
    let mut out = std::io::stdout().lock();
    for v in values {
        out.write_all(&v.to_le_bytes()).unwrap();
    }
}

fn v3(p: &[f64]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}
fn q4(p: &[f64]) -> Quaternion {
    Quaternion::new(p[0], p[1], p[2], p[3])
}
fn m4(p: &[f64]) -> Matrix4 {
    let mut m = Matrix4::default();
    m.elements.copy_from_slice(&p[..16]);
    m
}
fn push3(out: &mut Vec<f64>, v: &Vector3) {
    out.extend_from_slice(&[v.x, v.y, v.z]);
}
fn pushq(out: &mut Vec<f64>, q: &Quaternion) {
    out.extend_from_slice(&[q.x, q.y, q.z, q.w]);
}

fn orders(i: f64) -> EulerOrder {
    match i as i32 {
        0 => EulerOrder::XYZ,
        1 => EulerOrder::YXZ,
        2 => EulerOrder::ZXY,
        3 => EulerOrder::ZYX,
        4 => EulerOrder::YZX,
        _ => EulerOrder::XZY,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let input = read_f64s();
    let mut out: Vec<f64> = Vec::new();
    match mode {
        // in: a(3) b(3) s(1) = 7
        // out: add sub mul div cross | dot lenSq len distSq dist manh |
        //      addScaled lerp normalize(a) setLength clampLength
        //      projectOnVector angleTo(libm, separate mode) = 3*8+5+3 = see below
        "v3a" => {
            for p in input.as_chunks::<7>().0 {
                let a = v3(&p[0..3]);
                let b = v3(&p[3..6]);
                let s = p[6];
                let mut r = a;
                r.add(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.sub(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.multiply(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.divide(&b);
                push3(&mut out, &r);
                let mut r = Vector3::default();
                r.cross_vectors(&a, &b);
                push3(&mut out, &r);
                out.push(a.dot(&b));
                out.push(a.length_sq());
                out.push(a.length());
                out.push(a.distance_to_squared(&b));
                out.push(a.distance_to(&b));
                out.push(a.manhattan_distance_to(&b));
                let mut r = a;
                r.add_scaled_vector(&b, s);
                push3(&mut out, &r);
                let mut r = Vector3::default();
                r.lerp_vectors(&a, &b, s);
                push3(&mut out, &r);
                let mut r = a;
                r.normalize();
                push3(&mut out, &r);
                let mut r = a;
                r.set_length(s);
                push3(&mut out, &r);
                let mut r = a;
                r.clamp_length(0.5, 2.0);
                push3(&mut out, &r);
                let mut r = a;
                r.project_on_vector(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.project_on_plane(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.reflect(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.min(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.max(&b);
                push3(&mut out, &r);
                let mut r = a;
                r.floor();
                push3(&mut out, &r);
                let mut r = a;
                r.round();
                push3(&mut out, &r);
            }
        }
        // (libm) in: a(3) b(3) angle(1): angleTo, applyAxisAngle,
        // setFromSphericalCoords(radius=a.x, phi=angle, theta=b.x)
        "v3libm" => {
            for p in input.as_chunks::<7>().0 {
                let a = v3(&p[0..3]);
                let b = v3(&p[3..6]);
                let angle = p[6];
                out.push(a.angle_to(&b));
                let mut r = a;
                r.apply_axis_angle(&b, angle);
                push3(&mut out, &r);
                let mut r = Vector3::default();
                r.set_from_spherical_coords(a.x, angle, b.x);
                push3(&mut out, &r);
                let mut s = Spherical::default();
                s.set_from_vector3(&a);
                out.extend_from_slice(&[s.radius, s.phi, s.theta]);
                let mut cy = Cylindrical::default();
                cy.set_from_vector3(&a);
                out.extend_from_slice(&[cy.radius, cy.theta, cy.y]);
                let mut r = Vector3::default();
                r.set_from_cylindrical_coords(a.x, angle, b.y);
                push3(&mut out, &r);
            }
        }
        // in: v(3) m(16) = 19
        "v3m" => {
            for p in input.as_chunks::<19>().0 {
                let a = v3(&p[0..3]);
                let m = m4(&p[3..19]);
                let mut r = a;
                r.apply_matrix4(&m);
                push3(&mut out, &r);
                let mut r = a;
                r.transform_direction(&m);
                push3(&mut out, &r);
                let mut r = Vector3::default();
                r.set_from_matrix_position(&m);
                push3(&mut out, &r);
                let mut r = Vector3::default();
                r.set_from_matrix_scale(&m);
                push3(&mut out, &r);
                let mut m3 = Matrix3::default();
                m3.set_from_matrix4(&m);
                let mut r = a;
                r.apply_matrix3(&m3);
                push3(&mut out, &r);
                let mut r = a;
                r.apply_normal_matrix(&m3);
                push3(&mut out, &r);
            }
        }
        // in: v(3) q(4) = 7 (applyQuaternion)
        "v3q" => {
            for p in input.as_chunks::<7>().0 {
                let mut a = v3(&p[0..3]);
                let q = q4(&p[3..7]);
                a.apply_quaternion(&q);
                push3(&mut out, &a);
            }
        }
        // in: q(4) r(4) t(1) m(16) = 25. slerp is (libm) in part;
        // harness splits outputs: exact part first.
        // out exact: mul(4) premul-of-q-by-r(4) dot lenSq len
        //            normalize(q)(4) invert(4) setFromRotationMatrix(4)
        //            setFromUnitVectors as Vector3s(4)
        "quat" => {
            for p in input.as_chunks::<25>().0 {
                let q = q4(&p[0..4]);
                let r = q4(&p[4..8]);
                let m = m4(&p[9..25]);
                let mut s = Quaternion::default();
                s.multiply_quaternions(&q, &r);
                pushq(&mut out, &s);
                let mut s = q;
                s.premultiply(&r);
                pushq(&mut out, &s);
                out.push(q.dot(&r));
                out.push(q.length_sq());
                out.push(q.length());
                let mut s = q;
                s.normalize();
                pushq(&mut out, &s);
                let mut s = q;
                s.invert();
                pushq(&mut out, &s);
                let mut s = Quaternion::default();
                s.set_from_rotation_matrix(&m);
                pushq(&mut out, &s);
                let va = Vector3::new(q.x, q.y, q.z);
                let vb = Vector3::new(r.x, r.y, r.z);
                let mut s = Quaternion::default();
                s.set_from_unit_vectors(&va, &vb);
                pushq(&mut out, &s);
            }
        }
        // (libm) in: q(4) r(4) t(1) axis(3) = 12:
        // slerp(4) slerpQuaternions(4) angleTo setFromAxisAngle(4)
        // rotateTowards(4) slerpFlat(4)
        "quatlibm" => {
            for p in input.as_chunks::<12>().0 {
                let q = q4(&p[0..4]);
                let r = q4(&p[4..8]);
                let t = p[8];
                let axis = v3(&p[9..12]);
                let mut s = q;
                s.slerp(&r, t);
                pushq(&mut out, &s);
                let mut s = Quaternion::default();
                s.slerp_quaternions(&q, &r, t);
                pushq(&mut out, &s);
                out.push(q.angle_to(&r));
                let mut s = Quaternion::default();
                s.set_from_axis_angle(&axis, t);
                pushq(&mut out, &s);
                let mut s = q;
                s.rotate_towards(&r, t.abs());
                pushq(&mut out, &s);
                let mut flat = [0.0f64; 4];
                Quaternion::slerp_flat(
                    &mut flat, 0,
                    &[q.x, q.y, q.z, q.w], 0,
                    &[r.x, r.y, r.z, r.w], 0,
                    t,
                );
                out.extend_from_slice(&flat);
            }
        }
        // (libm) in: e(3) order(1) = 4:
        // quaternion.setFromEuler(4) matrix.makeRotationFromEuler(16)
        // euler.setFromQuaternion back (3)
        "euler" => {
            for p in input.as_chunks::<4>().0 {
                let e = Euler::new(p[0], p[1], p[2], orders(p[3]));
                let mut q = Quaternion::default();
                q.set_from_euler(&e);
                pushq(&mut out, &q);
                let mut m = Matrix4::default();
                m.make_rotation_from_euler(&e);
                out.extend_from_slice(&m.elements);
                let mut back = Euler::default();
                back.set_from_quaternion(&q, Some(orders(p[3])));
                out.extend_from_slice(&[back.x, back.y, back.z]);
                let mut re = e;
                // The harness wraps the order as `orders[(c[3] + 1) % 6]`.
                re.reorder(orders((p[3] + 1.0) % 6.0));
                out.extend_from_slice(&[re.x, re.y, re.z]);
            }
        }
        // (libm) in: m(16) order(1) = 17: euler.setFromRotationMatrix
        "eulerm" => {
            for p in input.as_chunks::<17>().0 {
                let m = m4(&p[0..16]);
                let mut e = Euler::default();
                e.set_from_rotation_matrix(&m, Some(orders(p[16])));
                out.extend_from_slice(&[e.x, e.y, e.z]);
            }
        }
        // in: a(16) b(16) v(3) s(1) = 36:
        // multiply(16) determinant invert(16) transpose(16)
        // compose from (v, quat from b rotation part?, scale) — harness
        // instead passes compose inputs in mode compose.
        "mat4" => {
            for p in input.as_chunks::<36>().0 {
                let a = m4(&p[0..16]);
                let b = m4(&p[16..32]);
                let s = p[35];
                let mut r = Matrix4::default();
                r.multiply_matrices(&a, &b);
                out.extend_from_slice(&r.elements);
                out.push(a.determinant());
                let mut r = a;
                r.invert();
                out.extend_from_slice(&r.elements);
                let mut r = a;
                r.transpose();
                out.extend_from_slice(&r.elements);
                let mut r = a;
                r.multiply_scalar(s);
                out.extend_from_slice(&r.elements);
                out.push(a.get_max_scale_on_axis());
                let mut r = Matrix4::default();
                r.extract_rotation(&a);
                out.extend_from_slice(&r.elements);
                let mut m3 = Matrix3::default();
                m3.get_normal_matrix(&a);
                out.extend_from_slice(&m3.elements);
                let mut m3b = Matrix3::default();
                m3b.set_from_matrix4(&a);
                out.push(m3b.determinant());
                m3b.invert();
                out.extend_from_slice(&m3b.elements);
            }
        }
        // in: pos(3) quat(4) scale(3) = 10: compose(16) then
        // decompose back pos(3) quat(4) scale(3)
        "compose" => {
            for p in input.as_chunks::<10>().0 {
                let pos = v3(&p[0..3]);
                let q = q4(&p[3..7]);
                let sc = v3(&p[7..10]);
                let mut m = Matrix4::default();
                m.compose(&pos, &q, &sc);
                out.extend_from_slice(&m.elements);
                let (mut p2, mut q2, mut s2) =
                    (Vector3::default(), Quaternion::default(), Vector3::default());
                m.decompose(&mut p2, &mut q2, &mut s2);
                push3(&mut out, &p2);
                pushq(&mut out, &q2);
                push3(&mut out, &s2);
                // makeRotationFromQuaternion
                let mut m2 = Matrix4::default();
                m2.make_rotation_from_quaternion(&q);
                out.extend_from_slice(&m2.elements);
            }
        }
        // in: eye(3) target(3) up(3) = 9: Matrix4.lookAt(16)
        "lookat" => {
            for p in input.as_chunks::<9>().0 {
                let eye = v3(&p[0..3]);
                let target = v3(&p[3..6]);
                let up = v3(&p[6..9]);
                let mut m = Matrix4::default();
                // three keeps the last row/col from the old matrix;
                // the harness starts from identity in JS too.
                m.look_at(&eye, &target, &up);
                out.extend_from_slice(&m.elements);
            }
        }
        // in: box1(6) box2(6) sphere(4:center+radius) plane(4:normal+constant)
        //     point(3) tri(9) m(16) = 48
        "box" => {
            for p in input.as_chunks::<48>().0 {
                let b1 = Box3::new(v3(&p[0..3]), v3(&p[3..6]));
                let b2 = Box3::new(v3(&p[6..9]), v3(&p[9..12]));
                let sph = Sphere::new(v3(&p[12..15]), p[15]);
                let pl = Plane::new(v3(&p[16..19]), p[19]);
                let pt = v3(&p[20..23]);
                let tri = Triangle::new(v3(&p[23..26]), v3(&p[26..29]), v3(&p[29..32]));
                let m = m4(&p[32..48]);
                out.push(b1.is_empty() as i32 as f64);
                out.push(b1.contains_point(&pt) as i32 as f64);
                out.push(b1.contains_box(&b2) as i32 as f64);
                out.push(b1.intersects_box(&b2) as i32 as f64);
                out.push(b1.intersects_sphere(&sph) as i32 as f64);
                out.push(b1.intersects_plane(&pl) as i32 as f64);
                out.push(b1.intersects_triangle(&tri) as i32 as f64);
                out.push(b1.distance_to_point(&pt));
                let mut v = Vector3::default();
                b1.get_center(&mut v);
                push3(&mut out, &v);
                let mut v = Vector3::default();
                b1.get_size(&mut v);
                push3(&mut out, &v);
                let mut v = Vector3::default();
                b1.clamp_point(&pt, &mut v);
                push3(&mut out, &v);
                let mut v = Vector3::default();
                b1.get_parameter(&pt, &mut v);
                push3(&mut out, &v);
                let mut s2 = Sphere::default();
                b1.get_bounding_sphere(&mut s2);
                push3(&mut out, &s2.center);
                out.push(s2.radius);
                let mut u = b1;
                u.union(&b2);
                push3(&mut out, &u.min);
                push3(&mut out, &u.max);
                let mut u = b1;
                u.intersect(&b2);
                push3(&mut out, &u.min);
                push3(&mut out, &u.max);
                let mut u = b1;
                u.apply_matrix4(&m);
                push3(&mut out, &u.min);
                push3(&mut out, &u.max);
                // sphere + plane behaviour
                out.push(sph.contains_point(&pt) as i32 as f64);
                out.push(sph.distance_to_point(&pt));
                out.push(sph.intersects_plane(&pl) as i32 as f64);
                out.push(pl.distance_to_point(&pt));
                out.push(pl.distance_to_sphere(&sph));
                let mut v = Vector3::default();
                sph.clamp_point(&pt, &mut v);
                push3(&mut out, &v);
                let mut v = Vector3::default();
                pl.project_point(&pt, &mut v);
                push3(&mut out, &v);
                let mut bb = Box3::default();
                sph.get_bounding_box(&mut bb);
                push3(&mut out, &bb.min);
                push3(&mut out, &bb.max);
                let mut s3 = sph;
                s3.expand_by_point(&pt);
                push3(&mut out, &s3.center);
                out.push(s3.radius);
                let mut s4 = sph;
                s4.apply_matrix4(&m);
                push3(&mut out, &s4.center);
                out.push(s4.radius);
                let mut pl2 = pl;
                pl2.normalize();
                push3(&mut out, &pl2.normal);
                out.push(pl2.constant);
            }
        }
        // in: ray o(3) d(3) sphere(4) box(6) plane(4) tri(9) pt(3) seg a(3) b(3) = 38
        // out: flags + points with hit=1/0; on miss the point is
        // whatever was in target (harness inits targets to (9,9,9)).
        "ray" => {
            for p in input.as_chunks::<38>().0 {
                let ray = Ray::new(v3(&p[0..3]), v3(&p[3..6]));
                let sph = Sphere::new(v3(&p[6..9]), p[9]);
                let bx = Box3::new(v3(&p[10..13]), v3(&p[13..16]));
                let pl = Plane::new(v3(&p[16..19]), p[19]);
                let ta = v3(&p[20..23]);
                let tb = v3(&p[23..26]);
                let tc = v3(&p[26..29]);
                let pt = v3(&p[29..32]);
                let sa = v3(&p[32..35]);
                let sb = v3(&p[35..38]);
                for culling in [false, true] {
                    let mut t = Vector3::new(9.0, 9.0, 9.0);
                    let hit = ray.intersect_triangle(&ta, &tb, &tc, culling, &mut t).is_some();
                    out.push(hit as i32 as f64);
                    push3(&mut out, &t);
                }
                let mut t = Vector3::new(9.0, 9.0, 9.0);
                let hit = ray.intersect_sphere(&sph, &mut t).is_some();
                out.push(hit as i32 as f64);
                push3(&mut out, &t);
                let mut t = Vector3::new(9.0, 9.0, 9.0);
                let hit = ray.intersect_box(&bx, &mut t).is_some();
                out.push(hit as i32 as f64);
                push3(&mut out, &t);
                let mut t = Vector3::new(9.0, 9.0, 9.0);
                let hit = ray.intersect_plane(&pl, &mut t).is_some();
                out.push(hit as i32 as f64);
                push3(&mut out, &t);
                out.push(ray.distance_to_plane(&pl).unwrap_or(f64::NAN));
                out.push(ray.distance_to_point(&pt));
                out.push(ray.distance_sq_to_point(&pt));
                let mut t = Vector3::new(9.0, 9.0, 9.0);
                ray.closest_point_to_point(&pt, &mut t);
                push3(&mut out, &t);
                let mut on_ray = Vector3::new(9.0, 9.0, 9.0);
                let mut on_seg = Vector3::new(9.0, 9.0, 9.0);
                out.push(ray.distance_sq_to_segment(&sa, &sb, Some(&mut on_ray), Some(&mut on_seg)));
                push3(&mut out, &on_ray);
                push3(&mut out, &on_seg);
                out.push(ray.intersects_sphere(&sph) as i32 as f64);
                out.push(ray.intersects_plane(&pl) as i32 as f64);
                out.push(ray.intersects_box(&bx) as i32 as f64);
                // Triangle statics on the same points.
                let tri = Triangle::new(ta, tb, tc);
                let mut n = Vector3::new(9.0, 9.0, 9.0);
                tri.get_normal(&mut n);
                push3(&mut out, &n);
                out.push(tri.get_area());
                let mut bary = Vector3::new(9.0, 9.0, 9.0);
                let ok = tri.get_barycoord(&pt, &mut bary).is_some();
                out.push(ok as i32 as f64);
                push3(&mut out, &bary);
                out.push(tri.contains_point(&pt) as i32 as f64);
                out.push(tri.is_front_facing(&ray.direction) as i32 as f64);
                let mut cp = Vector3::new(9.0, 9.0, 9.0);
                tri.closest_point_to_point(&pt, &mut cp);
                push3(&mut out, &cp);
                let mut mid = Vector3::new(9.0, 9.0, 9.0);
                tri.get_midpoint(&mut mid);
                push3(&mut out, &mid);
            }
        }
        // MathUtils scalars. in: 5 per case.
        "mathutils" => {
            for p in input.as_chunks::<5>().0 {
                let (x, y, a, b, t) = (p[0], p[1], p[2], p[3], p[4]);
                out.push(mu::clamp(x, a, b));
                out.push(mu::euclidean_modulo(x, y));
                out.push(mu::map_linear(x, a, b, y, t));
                out.push(mu::inverse_lerp(a, b, x));
                out.push(mu::lerp(x, y, t));
                out.push(mu::smoothstep(x, a, b));
                out.push(mu::smootherstep(x, a, b));
                out.push(mu::pingpong(x, y));
                out.push(mu::deg_to_rad(x));
                out.push(mu::rad_to_deg(x));
                out.push(mu::is_power_of_two(x) as i32 as f64);
            }
        }
        // (libm) MathUtils damp + power-of-two helpers.
        "mathutilslibm" => {
            for p in input.as_chunks::<5>().0 {
                let (x, y, a, b, t) = (p[0], p[1], p[2], p[3], p[4]);
                out.push(mu::damp(x, y, a, t));
                out.push(mu::ceil_power_of_two(x.abs()));
                out.push(mu::floor_power_of_two(x.abs()));
                let _ = b;
            }
        }
        // Vector2/Vector4 spread. in: a2(2) b2(2) a4(4) b4(4) s(1) m16 = 29
        "v24" => {
            for p in input.as_chunks::<29>().0 {
                let a2 = Vector2::new(p[0], p[1]);
                let b2 = Vector2::new(p[2], p[3]);
                let a4v = Vector4::new(p[4], p[5], p[6], p[7]);
                let b4v = Vector4::new(p[8], p[9], p[10], p[11]);
                let s = p[12];
                let m = m4(&p[13..29]);
                let mut r = a2;
                r.add(&b2);
                out.extend_from_slice(&[r.x, r.y]);
                out.push(a2.dot(&b2));
                out.push(a2.cross(&b2));
                out.push(a2.length());
                let mut r = a2;
                r.normalize();
                out.extend_from_slice(&[r.x, r.y]);
                let mut r = a2;
                r.lerp(&b2, s);
                out.extend_from_slice(&[r.x, r.y]);
                let mut m3 = Matrix3::default();
                m3.set_from_matrix4(&m);
                let mut r = a2;
                r.apply_matrix3(&m3);
                out.extend_from_slice(&[r.x, r.y]);
                let mut r = a4v;
                r.apply_matrix4(&m);
                out.extend_from_slice(&[r.x, r.y, r.z, r.w]);
                out.push(a4v.dot(&b4v));
                out.push(a4v.length());
                let mut r = a4v;
                r.clamp(&b4v, &a4v.clone());
                out.extend_from_slice(&[r.x, r.y, r.z, r.w]);
                // setAxisAngleFromRotationMatrix
                let mut r = Vector4::default();
                r.set_axis_angle_from_rotation_matrix(&m);
                out.extend_from_slice(&[r.x, r.y, r.z, r.w]);
            }
        }
        // TransformTree scenario. The harness builds the same
        // hierarchy in three.js and reads the same values.
        // in: per node i in 0..8: pos(3) euler(3) scale(3) parent(1)
        // = 80; then a quaternion(4) written onto node 3, a lookAt
        // target(3) for node 5, attach + rotate params(4).
        "tree" => {
            for p in input.as_chunks::<91>().0 {
                let mut tree = TransformTree::new();
                let mut ids = Vec::new();
                for i in 0..8 {
                    let base = i * 10;
                    let mut node = TransformNode::default();
                    node.position = v3(&p[base..base + 3]);
                    node.set_rotation(
                        p[base + 3],
                        p[base + 4],
                        p[base + 5],
                        Some(orders((i % 6) as f64)),
                    );
                    node.scale = v3(&p[base + 6..base + 9]);
                    // Node 2 keeps a hand-set matrix in three: no
                    // auto update; node 4 does not auto-update world.
                    if i == 2 {
                        node.matrix_auto_update = false;
                        node.update_matrix();
                    }
                    if i == 4 {
                        node.matrix_world_auto_update = false;
                    }
                    ids.push(tree.add_node(node));
                }
                for i in 0..8 {
                    let parent = p[i * 10 + 9] as i32;
                    if parent >= 0 && (parent as usize) < i {
                        tree.add_child(ids[parent as usize], ids[i]);
                    }
                }
                tree.update_matrix_world(ids[0], false);
                for id in &ids {
                    out.extend_from_slice(&tree.node(*id).matrix_world.elements);
                }
                // Write a quaternion directly onto node 3 and read
                // the synced Euler (three's heading quirk lives here).
                let q = q4(&p[80..84]);
                tree.node_mut(ids[3]).copy_quaternion(&q);
                let rot3 = tree.node_mut(ids[3]).rotation();
                out.extend_from_slice(&[rot3.x, rot3.y, rot3.z]);
                // Write the Euler on node 3, read the quaternion.
                tree.node_mut(ids[3]).set_rotation(p[84], p[85], p[86], Some(EulerOrder::YXZ));
                pushq(&mut out, &tree.node(ids[3]).quaternion);
                // World getters for the leaf.
                let mut wp = Vector3::default();
                tree.get_world_position(ids[7], &mut wp);
                push3(&mut out, &wp);
                let mut wq = Quaternion::default();
                tree.get_world_quaternion(ids[7], &mut wq);
                pushq(&mut out, &wq);
                let mut ws = Vector3::default();
                tree.get_world_scale(ids[7], &mut ws);
                push3(&mut out, &ws);
                let mut wd = Vector3::default();
                tree.get_world_direction(ids[7], &mut wd);
                push3(&mut out, &wd);
                // lookAt on node 5, localToWorld on node 7.
                let target = v3(&p[87..90]);
                tree.look_at(ids[5], &target);
                pushq(&mut out, &tree.node(ids[5]).quaternion);
                let mut local = v3(&p[87..90]);
                tree.local_to_world(ids[7], &mut local);
                push3(&mut out, &local);
                tree.world_to_local(ids[7], &mut local);
                push3(&mut out, &local);
                // attach node 6 under node 1, read its world matrix.
                tree.attach(ids[1], ids[6]);
                out.extend_from_slice(&tree.node(ids[6]).matrix_world.elements);
                push3(&mut out, &tree.node(ids[6]).position);
                pushq(&mut out, &tree.node(ids[6]).quaternion);
                // rotate/translate node 2 (matrixAutoUpdate false:
                // its matrix stays until updateMatrix; the world
                // read goes through updateWorldMatrix which
                // respects the flag the same way).
                tree.rotate_y(ids[2], p[90]);
                tree.translate_z(ids[2], p[90]);
                tree.update_world_matrix(ids[2], true, true);
                out.extend_from_slice(&tree.node(ids[2]).matrix_world.elements);
            }
        }
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(2);
        }
    }
    write_f64s(&out);
}
