//! `Box3.js`, `Sphere.js`, `Plane.js`, `Ray.js`, `Line3.js`,
//! `Triangle.js`, `Spherical.js`, `Cylindrical.js` (r170).
//! three's module-level temporaries (`_vector`, `_v0`, ...) become
//! locals here; they hold no state between calls in three either
//! (a sim port must never rely on one surviving a call, and in
//! three it does not — every method rewrites what it reads).
//! Methods that return `null` on a miss return `Option`, with the
//! target written only on a hit, as three leaves it.

use jsrt::conv::{js_max, js_min};
use jsrt::v8math;

use crate::mathutils::clamp;
use crate::matrix::{Matrix3, Matrix4};
use crate::vector::{Vector3, Vector4};

/// `THREE.Spherical`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spherical {
    pub radius: f64,
    pub phi: f64,
    pub theta: f64,
}

impl Default for Spherical {
    /// `new Spherical()`: radius 1, phi 0, theta 0.
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0)
    }
}

impl Spherical {
    pub fn new(radius: f64, phi: f64, theta: f64) -> Self {
        Spherical { radius, phi, theta }
    }

    pub fn set(&mut self, radius: f64, phi: f64, theta: f64) -> &mut Self {
        self.radius = radius;
        self.phi = phi;
        self.theta = theta;
        self
    }

    pub fn copy(&mut self, other: &Spherical) -> &mut Self {
        self.radius = other.radius;
        self.phi = other.phi;
        self.theta = other.theta;
        self
    }

    /// `makeSafe()`: phi into [EPS, PI - EPS].
    pub fn make_safe(&mut self) -> &mut Self {
        const EPS: f64 = 0.000001;
        self.phi = js_max(EPS, js_min(core::f64::consts::PI - EPS, self.phi));
        self
    }

    pub fn set_from_vector3(&mut self, v: &Vector3) -> &mut Self {
        self.set_from_cartesian_coords(v.x, v.y, v.z)
    }

    pub fn set_from_cartesian_coords(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.radius = (x * x + y * y + z * z).sqrt();
        if self.radius == 0.0 {
            self.theta = 0.0;
            self.phi = 0.0;
        } else {
            self.theta = v8math::atan2(x, z);
            self.phi = v8math::acos(clamp(y / self.radius, -1.0, 1.0));
        }
        self
    }
}

/// `THREE.Cylindrical`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylindrical {
    pub radius: f64,
    pub theta: f64,
    pub y: f64,
}

impl Default for Cylindrical {
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0)
    }
}

impl Cylindrical {
    pub fn new(radius: f64, theta: f64, y: f64) -> Self {
        Cylindrical { radius, theta, y }
    }

    pub fn set(&mut self, radius: f64, theta: f64, y: f64) -> &mut Self {
        self.radius = radius;
        self.theta = theta;
        self.y = y;
        self
    }

    pub fn copy(&mut self, other: &Cylindrical) -> &mut Self {
        self.radius = other.radius;
        self.theta = other.theta;
        self.y = other.y;
        self
    }

    pub fn set_from_vector3(&mut self, v: &Vector3) -> &mut Self {
        self.set_from_cartesian_coords(v.x, v.y, v.z)
    }

    pub fn set_from_cartesian_coords(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.radius = (x * x + z * z).sqrt();
        self.theta = v8math::atan2(x, z);
        self.y = y;
        self
    }
}

/// `THREE.Box3`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: Vector3,
    pub max: Vector3,
}

impl Default for Box3 {
    /// `new Box3()`: the empty box (+Inf min, -Inf max).
    fn default() -> Self {
        Box3 {
            min: Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
}

impl Box3 {
    pub fn new(min: Vector3, max: Vector3) -> Self {
        Box3 { min, max }
    }

    pub fn set(&mut self, min: &Vector3, max: &Vector3) -> &mut Self {
        self.min.copy(min);
        self.max.copy(max);
        self
    }

    /// `setFromArray(array)`: triples from a flat array.
    pub fn set_from_array(&mut self, array: &[f64]) -> &mut Self {
        self.make_empty();
        let mut v = Vector3::default();
        let mut i = 0;
        while i < array.len() {
            v.from_array(array, i);
            self.expand_by_point(&v.clone());
            i += 3;
        }
        self
    }

    /// `setFromPoints(points)`.
    pub fn set_from_points(&mut self, points: &[Vector3]) -> &mut Self {
        self.make_empty();
        for p in points {
            let p = *p;
            self.expand_by_point(&p);
        }
        self
    }

    pub fn set_from_center_and_size(&mut self, center: &Vector3, size: &Vector3) -> &mut Self {
        let mut half = *size;
        half.multiply_scalar(0.5);
        self.min.copy(center);
        self.min.sub(&half);
        self.max.copy(center);
        self.max.add(&half);
        self
    }

    pub fn copy(&mut self, box_: &Box3) -> &mut Self {
        self.min.copy(&box_.min);
        self.max.copy(&box_.max);
        self
    }

    pub fn make_empty(&mut self) -> &mut Self {
        self.min.x = f64::INFINITY;
        self.min.y = f64::INFINITY;
        self.min.z = f64::INFINITY;
        self.max.x = f64::NEG_INFINITY;
        self.max.y = f64::NEG_INFINITY;
        self.max.z = f64::NEG_INFINITY;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.max.x < self.min.x || self.max.y < self.min.y || self.max.z < self.min.z
    }

    /// `getCenter(target)`: writes the target and returns it by
    /// value, as the rigs read it.
    pub fn get_center(&self, target: &mut Vector3) -> Vector3 {
        if self.is_empty() {
            target.set(0.0, 0.0, 0.0);
        } else {
            target.add_vectors(&self.min, &self.max);
            target.multiply_scalar(0.5);
        }
        *target
    }

    /// `getSize(target)`.
    pub fn get_size<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        if self.is_empty() {
            target.set(0.0, 0.0, 0.0)
        } else {
            target.sub_vectors(&self.max, &self.min)
        }
    }

    pub fn expand_by_point(&mut self, point: &Vector3) -> &mut Self {
        self.min.min(point);
        self.max.max(point);
        self
    }

    pub fn expand_by_vector(&mut self, vector: &Vector3) -> &mut Self {
        self.min.sub(vector);
        self.max.add(vector);
        self
    }

    pub fn expand_by_scalar(&mut self, scalar: f64) -> &mut Self {
        self.min.add_scalar(-scalar);
        self.max.add_scalar(scalar);
        self
    }

    pub fn contains_point(&self, point: &Vector3) -> bool {
        point.x >= self.min.x && point.x <= self.max.x
            && point.y >= self.min.y && point.y <= self.max.y
            && point.z >= self.min.z && point.z <= self.max.z
    }

    pub fn contains_box(&self, box_: &Box3) -> bool {
        self.min.x <= box_.min.x && box_.max.x <= self.max.x
            && self.min.y <= box_.min.y && box_.max.y <= self.max.y
            && self.min.z <= box_.min.z && box_.max.z <= self.max.z
    }

    /// `getParameter(point, target)`.
    pub fn get_parameter<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.set(
            (point.x - self.min.x) / (self.max.x - self.min.x),
            (point.y - self.min.y) / (self.max.y - self.min.y),
            (point.z - self.min.z) / (self.max.z - self.min.z),
        )
    }

    pub fn intersects_box(&self, box_: &Box3) -> bool {
        box_.max.x >= self.min.x && box_.min.x <= self.max.x
            && box_.max.y >= self.min.y && box_.min.y <= self.max.y
            && box_.max.z >= self.min.z && box_.min.z <= self.max.z
    }

    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        let mut closest = Vector3::default();
        self.clamp_point(&sphere.center, &mut closest);
        closest.distance_to_squared(&sphere.center) <= sphere.radius * sphere.radius
    }

    pub fn intersects_plane(&self, plane: &Plane) -> bool {
        let (mut min, mut max);
        if plane.normal.x > 0.0 {
            min = plane.normal.x * self.min.x;
            max = plane.normal.x * self.max.x;
        } else {
            min = plane.normal.x * self.max.x;
            max = plane.normal.x * self.min.x;
        }
        if plane.normal.y > 0.0 {
            min += plane.normal.y * self.min.y;
            max += plane.normal.y * self.max.y;
        } else {
            min += plane.normal.y * self.max.y;
            max += plane.normal.y * self.min.y;
        }
        if plane.normal.z > 0.0 {
            min += plane.normal.z * self.min.z;
            max += plane.normal.z * self.max.z;
        } else {
            min += plane.normal.z * self.max.z;
            max += plane.normal.z * self.min.z;
        }
        min <= -plane.constant && max >= -plane.constant
    }

    /// `intersectsTriangle(triangle)`: the SAT walk, axes in
    /// three's order.
    pub fn intersects_triangle(&self, triangle: &Triangle) -> bool {
        if self.is_empty() {
            return false;
        }
        let mut center = Vector3::default();
        self.get_center(&mut center);
        let mut extents = Vector3::default();
        extents.sub_vectors(&self.max, &center);
        let mut v0 = Vector3::default();
        let mut v1 = Vector3::default();
        let mut v2 = Vector3::default();
        v0.sub_vectors(&triangle.a, &center);
        v1.sub_vectors(&triangle.b, &center);
        v2.sub_vectors(&triangle.c, &center);
        let mut f0 = Vector3::default();
        let mut f1 = Vector3::default();
        let mut f2 = Vector3::default();
        f0.sub_vectors(&v1, &v0);
        f1.sub_vectors(&v2, &v1);
        f2.sub_vectors(&v0, &v2);
        let axes: [f64; 27] = [
            0.0, -f0.z, f0.y, 0.0, -f1.z, f1.y, 0.0, -f2.z, f2.y,
            f0.z, 0.0, -f0.x, f1.z, 0.0, -f1.x, f2.z, 0.0, -f2.x,
            -f0.y, f0.x, 0.0, -f1.y, f1.x, 0.0, -f2.y, f2.x, 0.0,
        ];
        if !sat_for_axes(&axes, &v0, &v1, &v2, &extents) {
            return false;
        }
        let axes: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        if !sat_for_axes(&axes, &v0, &v1, &v2, &extents) {
            return false;
        }
        let mut normal = Vector3::default();
        normal.cross_vectors(&f0, &f1);
        let axes: [f64; 3] = [normal.x, normal.y, normal.z];
        sat_for_axes(&axes, &v0, &v1, &v2, &extents)
    }

    /// `clampPoint(point, target)`.
    pub fn clamp_point<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.copy(point);
        target.clamp(&self.min, &self.max)
    }

    pub fn distance_to_point(&self, point: &Vector3) -> f64 {
        let mut clamped = Vector3::default();
        self.clamp_point(point, &mut clamped);
        clamped.distance_to(point)
    }

    /// `getBoundingSphere(target)`.
    pub fn get_bounding_sphere<'t>(&self, target: &'t mut Sphere) -> &'t mut Sphere {
        if self.is_empty() {
            target.make_empty();
        } else {
            self.get_center(&mut target.center);
            let mut size = Vector3::default();
            self.get_size(&mut size);
            target.radius = size.length() * 0.5;
        }
        target
    }

    /// `intersect(box)`: a fully empty result when there is no
    /// overlap (makeEmpty, as three insists).
    pub fn intersect(&mut self, box_: &Box3) -> &mut Self {
        self.min.max(&box_.min);
        self.max.min(&box_.max);
        if self.is_empty() {
            self.make_empty();
        }
        self
    }

    pub fn union(&mut self, box_: &Box3) -> &mut Self {
        self.min.min(&box_.min);
        self.max.max(&box_.max);
        self
    }

    /// `applyMatrix4(matrix)`: the eight corners in three's binary
    /// order.
    pub fn apply_matrix4(&mut self, matrix: &Matrix4) -> &mut Self {
        if self.is_empty() {
            return self;
        }
        let (min, max) = (self.min, self.max);
        let corners = [
            (min.x, min.y, min.z), (min.x, min.y, max.z),
            (min.x, max.y, min.z), (min.x, max.y, max.z),
            (max.x, min.y, min.z), (max.x, min.y, max.z),
            (max.x, max.y, min.z), (max.x, max.y, max.z),
        ];
        let mut points = [Vector3::default(); 8];
        for (i, (x, y, z)) in corners.iter().enumerate() {
            points[i].set(*x, *y, *z);
            points[i].apply_matrix4(matrix);
        }
        self.set_from_points(&points);
        self
    }

    pub fn translate(&mut self, offset: &Vector3) -> &mut Self {
        self.min.add(offset);
        self.max.add(offset);
        self
    }

    pub fn equals(&self, box_: &Box3) -> bool {
        box_.min.equals(&self.min) && box_.max.equals(&self.max)
    }
}

// satForAxes, Box3.js. Math.max/Math.min associate as written.
fn sat_for_axes(axes: &[f64], v0: &Vector3, v1: &Vector3, v2: &Vector3, extents: &Vector3) -> bool {
    let mut test_axis = Vector3::default();
    let mut i = 0usize;
    while i + 3 <= axes.len() {
        test_axis.from_array(axes, i);
        let r = extents.x * test_axis.x.abs()
            + extents.y * test_axis.y.abs()
            + extents.z * test_axis.z.abs();
        let p0 = v0.dot(&test_axis);
        let p1 = v1.dot(&test_axis);
        let p2 = v2.dot(&test_axis);
        if js_max(-js_max(js_max(p0, p1), p2), js_min(js_min(p0, p1), p2)) > r {
            return false;
        }
        i += 3;
    }
    true
}

/// `THREE.Sphere`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vector3,
    pub radius: f64,
}

impl Default for Sphere {
    /// `new Sphere()`: center 0, radius -1 (empty).
    fn default() -> Self {
        Sphere { center: Vector3::default(), radius: -1.0 }
    }
}

impl Sphere {
    pub fn new(center: Vector3, radius: f64) -> Self {
        Sphere { center, radius }
    }

    pub fn set(&mut self, center: &Vector3, radius: f64) -> &mut Self {
        self.center.copy(center);
        self.radius = radius;
        self
    }

    /// `setFromPoints(points, optionalCenter)`.
    pub fn set_from_points(&mut self, points: &[Vector3], optional_center: Option<&Vector3>) -> &mut Self {
        match optional_center {
            Some(c) => {
                let c = *c;
                self.center.copy(&c);
            }
            None => {
                let mut box_ = Box3::default();
                box_.set_from_points(points);
                let mut center = Vector3::default();
                box_.get_center(&mut center);
                self.center = center;
            }
        }
        let mut max_radius_sq = 0.0f64;
        for p in points {
            max_radius_sq = js_max(max_radius_sq, self.center.distance_to_squared(p));
        }
        self.radius = max_radius_sq.sqrt();
        self
    }

    pub fn copy(&mut self, sphere: &Sphere) -> &mut Self {
        self.center.copy(&sphere.center);
        self.radius = sphere.radius;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.radius < 0.0
    }

    pub fn make_empty(&mut self) -> &mut Self {
        self.center.set(0.0, 0.0, 0.0);
        self.radius = -1.0;
        self
    }

    pub fn contains_point(&self, point: &Vector3) -> bool {
        point.distance_to_squared(&self.center) <= self.radius * self.radius
    }

    pub fn distance_to_point(&self, point: &Vector3) -> f64 {
        point.distance_to(&self.center) - self.radius
    }

    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        let radius_sum = self.radius + sphere.radius;
        sphere.center.distance_to_squared(&self.center) <= radius_sum * radius_sum
    }

    pub fn intersects_box(&self, box_: &Box3) -> bool {
        box_.intersects_sphere(self)
    }

    pub fn intersects_plane(&self, plane: &Plane) -> bool {
        plane.distance_to_point(&self.center).abs() <= self.radius
    }

    /// `clampPoint(point, target)`.
    pub fn clamp_point<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        let delta_length_sq = self.center.distance_to_squared(point);
        target.copy(point);
        if delta_length_sq > self.radius * self.radius {
            target.sub(&self.center);
            target.normalize();
            target.multiply_scalar(self.radius);
            target.add(&self.center);
        }
        target
    }

    /// `getBoundingBox(target)`.
    pub fn get_bounding_box<'t>(&self, target: &'t mut Box3) -> &'t mut Box3 {
        if self.is_empty() {
            target.make_empty();
            return target;
        }
        let center = self.center;
        target.set(&center, &center);
        target.expand_by_scalar(self.radius);
        target
    }

    pub fn apply_matrix4(&mut self, matrix: &Matrix4) -> &mut Self {
        self.center.apply_matrix4(matrix);
        self.radius *= matrix.get_max_scale_on_axis();
        self
    }

    pub fn translate(&mut self, offset: &Vector3) -> &mut Self {
        self.center.add(offset);
        self
    }

    /// `expandByPoint(point)`.
    pub fn expand_by_point(&mut self, point: &Vector3) -> &mut Self {
        if self.is_empty() {
            self.center.copy(point);
            self.radius = 0.0;
            return self;
        }
        let mut v1 = Vector3::default();
        v1.sub_vectors(point, &self.center);
        let length_sq = v1.length_sq();
        if length_sq > self.radius * self.radius {
            let length = length_sq.sqrt();
            let delta = (length - self.radius) * 0.5;
            self.center.add_scaled_vector(&v1, delta / length);
            self.radius += delta;
        }
        self
    }

    /// `union(sphere)`.
    pub fn union(&mut self, sphere: &Sphere) -> &mut Self {
        if sphere.is_empty() {
            return self;
        }
        if self.is_empty() {
            self.copy(sphere);
            return self;
        }
        if self.center.equals(&sphere.center) {
            self.radius = js_max(self.radius, sphere.radius);
        } else {
            let mut v2 = Vector3::default();
            v2.sub_vectors(&sphere.center, &self.center);
            v2.set_length(sphere.radius);
            let mut p = sphere.center;
            p.add(&v2);
            self.expand_by_point(&p);
            let mut p = sphere.center;
            p.sub(&v2);
            self.expand_by_point(&p);
        }
        self
    }

    pub fn equals(&self, sphere: &Sphere) -> bool {
        sphere.center.equals(&self.center) && sphere.radius == self.radius
    }
}

/// `THREE.Plane`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vector3,
    pub constant: f64,
}

impl Default for Plane {
    /// `new Plane()`: normal (1, 0, 0), constant 0.
    fn default() -> Self {
        Plane { normal: Vector3::new(1.0, 0.0, 0.0), constant: 0.0 }
    }
}

impl Plane {
    pub fn new(normal: Vector3, constant: f64) -> Self {
        Plane { normal, constant }
    }

    pub fn set(&mut self, normal: &Vector3, constant: f64) -> &mut Self {
        self.normal.copy(normal);
        self.constant = constant;
        self
    }

    pub fn set_components(&mut self, x: f64, y: f64, z: f64, w: f64) -> &mut Self {
        self.normal.set(x, y, z);
        self.constant = w;
        self
    }

    pub fn set_from_normal_and_coplanar_point(&mut self, normal: &Vector3, point: &Vector3) -> &mut Self {
        self.normal.copy(normal);
        self.constant = -point.dot(&self.normal);
        self
    }

    /// `setFromCoplanarPoints(a, b, c)`: the normal is
    /// (c - b) x (a - b), normalized by Vector3's rule.
    pub fn set_from_coplanar_points(&mut self, a: &Vector3, b: &Vector3, c: &Vector3) -> &mut Self {
        let mut cb = Vector3::default();
        cb.sub_vectors(c, b);
        let mut ab = Vector3::default();
        ab.sub_vectors(a, b);
        cb.cross(&ab);
        cb.normalize();
        self.set_from_normal_and_coplanar_point(&cb, a)
    }

    pub fn copy(&mut self, plane: &Plane) -> &mut Self {
        self.normal.copy(&plane.normal);
        self.constant = plane.constant;
        self
    }

    /// `normalize()`: scales by 1 / normal.length().
    pub fn normalize(&mut self) -> &mut Self {
        let inverse = 1.0 / self.normal.length();
        self.normal.multiply_scalar(inverse);
        self.constant *= inverse;
        self
    }

    pub fn negate(&mut self) -> &mut Self {
        self.constant *= -1.0;
        self.normal.negate();
        self
    }

    pub fn distance_to_point(&self, point: &Vector3) -> f64 {
        self.normal.dot(point) + self.constant
    }

    pub fn distance_to_sphere(&self, sphere: &Sphere) -> f64 {
        self.distance_to_point(&sphere.center) - sphere.radius
    }

    /// `projectPoint(point, target)`.
    pub fn project_point<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        let d = -self.distance_to_point(point);
        target.copy(point);
        target.add_scaled_vector(&self.normal, d)
    }

    /// `intersectLine(line, target)`: None where three returns null.
    pub fn intersect_line<'t>(&self, line: &Line3, target: &'t mut Vector3) -> Option<&'t mut Vector3> {
        let mut direction = Vector3::default();
        line.delta(&mut direction);
        let denominator = self.normal.dot(&direction);
        if denominator == 0.0 {
            if self.distance_to_point(&line.start) == 0.0 {
                target.copy(&line.start);
                return Some(target);
            }
            return None;
        }
        let t = -(line.start.dot(&self.normal) + self.constant) / denominator;
        if t < 0.0 || t > 1.0 {
            return None;
        }
        target.copy(&line.start);
        target.add_scaled_vector(&direction, t);
        Some(target)
    }

    pub fn intersects_line(&self, line: &Line3) -> bool {
        let start_sign = self.distance_to_point(&line.start);
        let end_sign = self.distance_to_point(&line.end);
        (start_sign < 0.0 && end_sign > 0.0) || (end_sign < 0.0 && start_sign > 0.0)
    }

    pub fn intersects_box(&self, box_: &Box3) -> bool {
        box_.intersects_plane(self)
    }

    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        sphere.intersects_plane(self)
    }

    /// `coplanarPoint(target)`.
    pub fn coplanar_point<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.copy(&self.normal);
        target.multiply_scalar(-self.constant)
    }

    /// `applyMatrix4(matrix, optionalNormalMatrix)`.
    pub fn apply_matrix4(&mut self, matrix: &Matrix4, normal_matrix: Option<&Matrix3>) -> &mut Self {
        let computed;
        let normal_matrix = match normal_matrix {
            Some(m) => m,
            None => {
                computed = {
                    let mut nm = Matrix3::default();
                    nm.get_normal_matrix(matrix);
                    nm
                };
                &computed
            }
        };
        let mut reference = Vector3::default();
        self.coplanar_point(&mut reference);
        reference.apply_matrix4(matrix);
        self.normal.apply_matrix3(normal_matrix);
        self.normal.normalize();
        let normal = self.normal;
        self.constant = -reference.dot(&normal);
        self
    }

    pub fn translate(&mut self, offset: &Vector3) -> &mut Self {
        self.constant -= offset.dot(&self.normal);
        self
    }

    pub fn equals(&self, plane: &Plane) -> bool {
        plane.normal.equals(&self.normal) && plane.constant == self.constant
    }
}

/// `THREE.Line3`, r170.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line3 {
    pub start: Vector3,
    pub end: Vector3,
}

impl Line3 {
    pub fn new(start: Vector3, end: Vector3) -> Self {
        Line3 { start, end }
    }

    pub fn set(&mut self, start: &Vector3, end: &Vector3) -> &mut Self {
        self.start.copy(start);
        self.end.copy(end);
        self
    }

    pub fn copy(&mut self, line: &Line3) -> &mut Self {
        self.start.copy(&line.start);
        self.end.copy(&line.end);
        self
    }

    pub fn get_center<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.add_vectors(&self.start, &self.end);
        target.multiply_scalar(0.5)
    }

    pub fn delta<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.sub_vectors(&self.end, &self.start)
    }

    pub fn distance_sq(&self) -> f64 {
        self.start.distance_to_squared(&self.end)
    }

    pub fn distance(&self) -> f64 {
        self.start.distance_to(&self.end)
    }

    /// `at(t, target)`.
    pub fn at<'t>(&self, t: f64, target: &'t mut Vector3) -> &'t mut Vector3 {
        self.delta(target);
        target.multiply_scalar(t);
        target.add(&self.start)
    }

    pub fn closest_point_to_point_parameter(&self, point: &Vector3, clamp_to_line: bool) -> f64 {
        let mut start_p = Vector3::default();
        start_p.sub_vectors(point, &self.start);
        let mut start_end = Vector3::default();
        start_end.sub_vectors(&self.end, &self.start);
        let start_end2 = start_end.dot(&start_end);
        let start_end_start_p = start_end.dot(&start_p);
        let mut t = start_end_start_p / start_end2;
        if clamp_to_line {
            t = clamp(t, 0.0, 1.0);
        }
        t
    }

    pub fn closest_point_to_point<'t>(
        &self,
        point: &Vector3,
        clamp_to_line: bool,
        target: &'t mut Vector3,
    ) -> &'t mut Vector3 {
        let t = self.closest_point_to_point_parameter(point, clamp_to_line);
        self.delta(target);
        target.multiply_scalar(t);
        target.add(&self.start)
    }

    pub fn apply_matrix4(&mut self, matrix: &Matrix4) -> &mut Self {
        self.start.apply_matrix4(matrix);
        self.end.apply_matrix4(matrix);
        self
    }

    pub fn equals(&self, line: &Line3) -> bool {
        line.start.equals(&self.start) && line.end.equals(&self.end)
    }
}

/// `THREE.Ray`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vector3,
    pub direction: Vector3,
}

impl Default for Ray {
    /// `new Ray()`: origin 0, direction (0, 0, -1).
    fn default() -> Self {
        Ray { origin: Vector3::default(), direction: Vector3::new(0.0, 0.0, -1.0) }
    }
}

impl Ray {
    pub fn new(origin: Vector3, direction: Vector3) -> Self {
        Ray { origin, direction }
    }

    pub fn set(&mut self, origin: &Vector3, direction: &Vector3) -> &mut Self {
        self.origin.copy(origin);
        self.direction.copy(direction);
        self
    }

    pub fn copy(&mut self, ray: &Ray) -> &mut Self {
        self.origin.copy(&ray.origin);
        self.direction.copy(&ray.direction);
        self
    }

    /// `at(t, target)`.
    pub fn at<'t>(&self, t: f64, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.copy(&self.origin);
        target.add_scaled_vector(&self.direction, t)
    }

    pub fn look_at(&mut self, v: &Vector3) -> &mut Self {
        self.direction.copy(v);
        let origin = self.origin;
        self.direction.sub(&origin);
        self.direction.normalize();
        self
    }

    pub fn recast(&mut self, t: f64) -> &mut Self {
        let mut p = Vector3::default();
        self.at(t, &mut p);
        self.origin.copy(&p);
        self
    }

    /// `closestPointToPoint(point, target)`.
    pub fn closest_point_to_point<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.sub_vectors(point, &self.origin);
        let direction_distance = target.dot(&self.direction);
        if direction_distance < 0.0 {
            return target.copy(&self.origin);
        }
        target.copy(&self.origin);
        target.add_scaled_vector(&self.direction, direction_distance)
    }

    pub fn distance_to_point(&self, point: &Vector3) -> f64 {
        self.distance_sq_to_point(point).sqrt()
    }

    pub fn distance_sq_to_point(&self, point: &Vector3) -> f64 {
        let mut v = Vector3::default();
        v.sub_vectors(point, &self.origin);
        let direction_distance = v.dot(&self.direction);
        if direction_distance < 0.0 {
            return self.origin.distance_to_squared(point);
        }
        v.copy(&self.origin);
        v.add_scaled_vector(&self.direction, direction_distance);
        v.distance_to_squared(point)
    }

    /// `distanceSqToSegment(v0, v1, optionalPointOnRay?,
    /// optionalPointOnSegment?)`: the two optional targets are
    /// Options; the region walk is three's.
    pub fn distance_sq_to_segment(
        &self,
        v0: &Vector3,
        v1: &Vector3,
        point_on_ray: Option<&mut Vector3>,
        point_on_segment: Option<&mut Vector3>,
    ) -> f64 {
        let mut seg_center = *v0;
        seg_center.add(v1);
        seg_center.multiply_scalar(0.5);
        let mut seg_dir = *v1;
        seg_dir.sub(v0);
        seg_dir.normalize();
        let mut diff = self.origin;
        diff.sub(&seg_center);
        let seg_extent = v0.distance_to(v1) * 0.5;
        let a01 = -self.direction.dot(&seg_dir);
        let b0 = diff.dot(&self.direction);
        let b1 = -diff.dot(&seg_dir);
        let c = diff.length_sq();
        let det = (1.0 - a01 * a01).abs();
        let (mut s0, mut s1, sqr_dist);
        if det > 0.0 {
            s0 = a01 * b1 - b0;
            s1 = a01 * b0 - b1;
            let ext_det = seg_extent * det;
            if s0 >= 0.0 {
                if s1 >= -ext_det {
                    if s1 <= ext_det {
                        let inv_det = 1.0 / det;
                        s0 *= inv_det;
                        s1 *= inv_det;
                        sqr_dist = s0 * (s0 + a01 * s1 + 2.0 * b0)
                            + s1 * (a01 * s0 + s1 + 2.0 * b1) + c;
                    } else {
                        s1 = seg_extent;
                        s0 = js_max(0.0, -(a01 * s1 + b0));
                        sqr_dist = -s0 * s0 + s1 * (s1 + 2.0 * b1) + c;
                    }
                } else {
                    s1 = -seg_extent;
                    s0 = js_max(0.0, -(a01 * s1 + b0));
                    sqr_dist = -s0 * s0 + s1 * (s1 + 2.0 * b1) + c;
                }
            } else if s1 <= -ext_det {
                s0 = js_max(0.0, -(-a01 * seg_extent + b0));
                s1 = if s0 > 0.0 {
                    -seg_extent
                } else {
                    js_min(js_max(-seg_extent, -b1), seg_extent)
                };
                sqr_dist = -s0 * s0 + s1 * (s1 + 2.0 * b1) + c;
            } else if s1 <= ext_det {
                s0 = 0.0;
                s1 = js_min(js_max(-seg_extent, -b1), seg_extent);
                sqr_dist = s1 * (s1 + 2.0 * b1) + c;
            } else {
                s0 = js_max(0.0, -(a01 * seg_extent + b0));
                s1 = if s0 > 0.0 {
                    seg_extent
                } else {
                    js_min(js_max(-seg_extent, -b1), seg_extent)
                };
                sqr_dist = -s0 * s0 + s1 * (s1 + 2.0 * b1) + c;
            }
        } else {
            s1 = if a01 > 0.0 { -seg_extent } else { seg_extent };
            s0 = js_max(0.0, -(a01 * s1 + b0));
            sqr_dist = -s0 * s0 + s1 * (s1 + 2.0 * b1) + c;
        }
        if let Some(target) = point_on_ray {
            target.copy(&self.origin);
            target.add_scaled_vector(&self.direction, s0);
        }
        if let Some(target) = point_on_segment {
            target.copy(&seg_center);
            target.add_scaled_vector(&seg_dir, s1);
        }
        sqr_dist
    }

    /// `intersectSphere(sphere, target)`.
    pub fn intersect_sphere<'t>(&self, sphere: &Sphere, target: &'t mut Vector3) -> Option<&'t mut Vector3> {
        let mut v = Vector3::default();
        v.sub_vectors(&sphere.center, &self.origin);
        let tca = v.dot(&self.direction);
        let d2 = v.dot(&v) - tca * tca;
        let radius2 = sphere.radius * sphere.radius;
        if d2 > radius2 {
            return None;
        }
        let thc = (radius2 - d2).sqrt();
        let t0 = tca - thc;
        let t1 = tca + thc;
        if t1 < 0.0 {
            return None;
        }
        if t0 < 0.0 {
            return Some(self.at(t1, target));
        }
        Some(self.at(t0, target))
    }

    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        self.distance_sq_to_point(&sphere.center) <= sphere.radius * sphere.radius
    }

    /// `distanceToPlane(plane)`: None where three returns null.
    pub fn distance_to_plane(&self, plane: &Plane) -> Option<f64> {
        let denominator = plane.normal.dot(&self.direction);
        if denominator == 0.0 {
            if plane.distance_to_point(&self.origin) == 0.0 {
                return Some(0.0);
            }
            return None;
        }
        let t = -(self.origin.dot(&plane.normal) + plane.constant) / denominator;
        if t >= 0.0 { Some(t) } else { None }
    }

    /// `intersectPlane(plane, target)`.
    pub fn intersect_plane<'t>(&self, plane: &Plane, target: &'t mut Vector3) -> Option<&'t mut Vector3> {
        let t = self.distance_to_plane(plane)?;
        Some(self.at(t, target))
    }

    pub fn intersects_plane(&self, plane: &Plane) -> bool {
        let dist = plane.distance_to_point(&self.origin);
        if dist == 0.0 {
            return true;
        }
        let denominator = plane.normal.dot(&self.direction);
        denominator * dist < 0.0
    }

    /// `intersectBox(box, target)`, including three's NaN handling
    /// spelled as written (`isNaN(tmin)` in one place,
    /// `tmin !== tmin` in another; the same test).
    pub fn intersect_box<'t>(&self, box_: &Box3, target: &'t mut Vector3) -> Option<&'t mut Vector3> {
        let invdirx = 1.0 / self.direction.x;
        let invdiry = 1.0 / self.direction.y;
        let invdirz = 1.0 / self.direction.z;
        let origin = self.origin;
        let (mut tmin, mut tmax);
        if invdirx >= 0.0 {
            tmin = (box_.min.x - origin.x) * invdirx;
            tmax = (box_.max.x - origin.x) * invdirx;
        } else {
            tmin = (box_.max.x - origin.x) * invdirx;
            tmax = (box_.min.x - origin.x) * invdirx;
        }
        let (tymin, tymax);
        if invdiry >= 0.0 {
            tymin = (box_.min.y - origin.y) * invdiry;
            tymax = (box_.max.y - origin.y) * invdiry;
        } else {
            tymin = (box_.max.y - origin.y) * invdiry;
            tymax = (box_.min.y - origin.y) * invdiry;
        }
        if tmin > tymax || tymin > tmax {
            return None;
        }
        if tymin > tmin || tmin.is_nan() {
            tmin = tymin;
        }
        if tymax < tmax || tmax.is_nan() {
            tmax = tymax;
        }
        let (tzmin, tzmax);
        if invdirz >= 0.0 {
            tzmin = (box_.min.z - origin.z) * invdirz;
            tzmax = (box_.max.z - origin.z) * invdirz;
        } else {
            tzmin = (box_.max.z - origin.z) * invdirz;
            tzmax = (box_.min.z - origin.z) * invdirz;
        }
        if tmin > tzmax || tzmin > tmax {
            return None;
        }
        if tzmin > tmin || tmin.is_nan() {
            tmin = tzmin;
        }
        if tzmax < tmax || tmax.is_nan() {
            tmax = tzmax;
        }
        if tmax < 0.0 {
            return None;
        }
        Some(self.at(if tmin >= 0.0 { tmin } else { tmax }, target))
    }

    pub fn intersects_box(&self, box_: &Box3) -> bool {
        let mut target = Vector3::default();
        self.intersect_box(box_, &mut target).is_some()
    }

    /// `intersectTriangle(a, b, c, backfaceCulling, target)`.
    /// three overwrites its edge temporaries as it goes
    /// (`_edge2.crossVectors(_diff, _edge2)`, then
    /// `_edge1.cross(_diff)`); the reads after each overwrite are
    /// the new values, reproduced here in the same order.
    pub fn intersect_triangle<'t>(
        &self,
        a: &Vector3,
        b: &Vector3,
        c: &Vector3,
        backface_culling: bool,
        target: &'t mut Vector3,
    ) -> Option<&'t mut Vector3> {
        let mut edge1 = Vector3::default();
        let mut edge2 = Vector3::default();
        let mut normal = Vector3::default();
        edge1.sub_vectors(b, a);
        edge2.sub_vectors(c, a);
        normal.cross_vectors(&edge1, &edge2);
        let mut dd_n = self.direction.dot(&normal);
        let sign;
        if dd_n > 0.0 {
            if backface_culling {
                return None;
            }
            sign = 1.0;
        } else if dd_n < 0.0 {
            sign = -1.0;
            dd_n = -dd_n;
        } else {
            return None;
        }
        let mut diff = Vector3::default();
        diff.sub_vectors(&self.origin, a);
        let e2 = edge2;
        edge2.cross_vectors(&diff, &e2);
        let dd_qx_e2 = sign * self.direction.dot(&edge2);
        if dd_qx_e2 < 0.0 {
            return None;
        }
        edge1.cross(&diff);
        let dd_e1x_q = sign * self.direction.dot(&edge1);
        if dd_e1x_q < 0.0 {
            return None;
        }
        if dd_qx_e2 + dd_e1x_q > dd_n {
            return None;
        }
        let qd_n = -sign * diff.dot(&normal);
        if qd_n < 0.0 {
            return None;
        }
        Some(self.at(qd_n / dd_n, target))
    }

    pub fn apply_matrix4(&mut self, matrix4: &Matrix4) -> &mut Self {
        self.origin.apply_matrix4(matrix4);
        self.direction.transform_direction(matrix4);
        self
    }

    pub fn equals(&self, ray: &Ray) -> bool {
        ray.origin.equals(&self.origin) && ray.direction.equals(&self.direction)
    }
}

/// `THREE.Triangle`, r170.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Triangle {
    pub a: Vector3,
    pub b: Vector3,
    pub c: Vector3,
}

impl Triangle {
    pub fn new(a: Vector3, b: Vector3, c: Vector3) -> Self {
        Triangle { a, b, c }
    }

    /// `Triangle.getNormal(a, b, c, target)`.
    pub fn get_normal_of<'t>(a: &Vector3, b: &Vector3, c: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.sub_vectors(c, b);
        let mut v0 = Vector3::default();
        v0.sub_vectors(a, b);
        target.cross(&v0);
        let length_sq = target.length_sq();
        if length_sq > 0.0 {
            return target.multiply_scalar(1.0 / length_sq.sqrt());
        }
        target.set(0.0, 0.0, 0.0)
    }

    /// `Triangle.getBarycoord(point, a, b, c, target)`: None (and
    /// target zeroed) for a collinear or singular triangle.
    pub fn get_barycoord_of<'t>(
        point: &Vector3,
        a: &Vector3,
        b: &Vector3,
        c: &Vector3,
        target: &'t mut Vector3,
    ) -> Option<&'t mut Vector3> {
        let mut v0 = Vector3::default();
        let mut v1 = Vector3::default();
        let mut v2 = Vector3::default();
        v0.sub_vectors(c, a);
        v1.sub_vectors(b, a);
        v2.sub_vectors(point, a);
        let dot00 = v0.dot(&v0);
        let dot01 = v0.dot(&v1);
        let dot02 = v0.dot(&v2);
        let dot11 = v1.dot(&v1);
        let dot12 = v1.dot(&v2);
        let denom = dot00 * dot11 - dot01 * dot01;
        if denom == 0.0 {
            target.set(0.0, 0.0, 0.0);
            return None;
        }
        let inv_denom = 1.0 / denom;
        let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
        let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;
        target.set(1.0 - u - v, v, u);
        Some(target)
    }

    /// `Triangle.containsPoint(point, a, b, c)`.
    pub fn contains_point_of(point: &Vector3, a: &Vector3, b: &Vector3, c: &Vector3) -> bool {
        let mut bary = Vector3::default();
        if Self::get_barycoord_of(point, a, b, c, &mut bary).is_none() {
            return false;
        }
        bary.x >= 0.0 && bary.y >= 0.0 && bary.x + bary.y <= 1.0
    }

    /// `Triangle.getInterpolation(point, p1..p3, v1..v3, target)`
    /// for Vector3 values.
    pub fn get_interpolation_of3<'t>(
        point: &Vector3,
        p1: &Vector3, p2: &Vector3, p3: &Vector3,
        v1: &Vector3, v2: &Vector3, v3: &Vector3,
        target: &'t mut Vector3,
    ) -> Option<&'t mut Vector3> {
        let mut bary = Vector3::default();
        if Self::get_barycoord_of(point, p1, p2, p3, &mut bary).is_none() {
            target.x = 0.0;
            target.y = 0.0;
            target.z = 0.0;
            return None;
        }
        target.set_scalar(0.0);
        target.add_scaled_vector(v1, bary.x);
        target.add_scaled_vector(v2, bary.y);
        target.add_scaled_vector(v3, bary.z);
        Some(target)
    }

    /// `Triangle.getInterpolation(...)` for Vector4 values (three
    /// checks `'w' in target`; the two shapes are distinct here).
    pub fn get_interpolation_of4<'t>(
        point: &Vector3,
        p1: &Vector3, p2: &Vector3, p3: &Vector3,
        v1: &Vector4, v2: &Vector4, v3: &Vector4,
        target: &'t mut Vector4,
    ) -> Option<&'t mut Vector4> {
        let mut bary = Vector3::default();
        if Self::get_barycoord_of(point, p1, p2, p3, &mut bary).is_none() {
            target.x = 0.0;
            target.y = 0.0;
            target.z = 0.0;
            target.w = 0.0;
            return None;
        }
        target.set_scalar(0.0);
        target.add_scaled_vector(v1, bary.x);
        target.add_scaled_vector(v2, bary.y);
        target.add_scaled_vector(v3, bary.z);
        Some(target)
    }

    /// `Triangle.isFrontFacing(a, b, c, direction)`.
    pub fn is_front_facing_of(a: &Vector3, b: &Vector3, c: &Vector3, direction: &Vector3) -> bool {
        let mut v0 = Vector3::default();
        let mut v1 = Vector3::default();
        v0.sub_vectors(c, b);
        v1.sub_vectors(a, b);
        v0.cross(&v1);
        v0.dot(direction) < 0.0
    }

    pub fn set(&mut self, a: &Vector3, b: &Vector3, c: &Vector3) -> &mut Self {
        self.a.copy(a);
        self.b.copy(b);
        self.c.copy(c);
        self
    }

    /// `setFromPointsAndIndices(points, i0, i1, i2)`.
    pub fn set_from_points_and_indices(
        &mut self,
        points: &[Vector3],
        i0: usize, i1: usize, i2: usize,
    ) -> &mut Self {
        self.a.copy(&points[i0]);
        self.b.copy(&points[i1]);
        self.c.copy(&points[i2]);
        self
    }

    pub fn copy(&mut self, triangle: &Triangle) -> &mut Self {
        self.a.copy(&triangle.a);
        self.b.copy(&triangle.b);
        self.c.copy(&triangle.c);
        self
    }

    pub fn get_area(&self) -> f64 {
        let mut v0 = Vector3::default();
        let mut v1 = Vector3::default();
        v0.sub_vectors(&self.c, &self.b);
        v1.sub_vectors(&self.a, &self.b);
        v0.cross(&v1);
        v0.length() * 0.5
    }

    /// `getMidpoint(target)`: `addVectors(a, b).add(c)` then
    /// `* (1 / 3)`, in that order.
    pub fn get_midpoint<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        target.add_vectors(&self.a, &self.b);
        target.add(&self.c);
        target.multiply_scalar(1.0 / 3.0)
    }

    pub fn get_normal<'t>(&self, target: &'t mut Vector3) -> &'t mut Vector3 {
        Self::get_normal_of(&self.a, &self.b, &self.c, target)
    }

    pub fn get_plane<'t>(&self, target: &'t mut Plane) -> &'t mut Plane {
        target.set_from_coplanar_points(&self.a, &self.b, &self.c)
    }

    pub fn get_barycoord<'t>(&self, point: &Vector3, target: &'t mut Vector3) -> Option<&'t mut Vector3> {
        Self::get_barycoord_of(point, &self.a, &self.b, &self.c, target)
    }

    pub fn get_interpolation3<'t>(
        &self,
        point: &Vector3,
        v1: &Vector3, v2: &Vector3, v3: &Vector3,
        target: &'t mut Vector3,
    ) -> Option<&'t mut Vector3> {
        Self::get_interpolation_of3(point, &self.a, &self.b, &self.c, v1, v2, v3, target)
    }

    pub fn contains_point(&self, point: &Vector3) -> bool {
        Self::contains_point_of(point, &self.a, &self.b, &self.c)
    }

    pub fn is_front_facing(&self, direction: &Vector3) -> bool {
        Self::is_front_facing_of(&self.a, &self.b, &self.c, direction)
    }

    pub fn intersects_box(&self, box_: &Box3) -> bool {
        box_.intersects_triangle(self)
    }

    /// `closestPointToPoint(p, target)`: Ericson's region walk as
    /// three spells it.
    pub fn closest_point_to_point<'t>(&self, p: &Vector3, target: &'t mut Vector3) -> &'t mut Vector3 {
        let (a, b, c) = (&self.a, &self.b, &self.c);
        let mut vab = Vector3::default();
        let mut vac = Vector3::default();
        let mut vap = Vector3::default();
        let mut vbp = Vector3::default();
        let mut vcp = Vector3::default();
        let mut vbc = Vector3::default();
        vab.sub_vectors(b, a);
        vac.sub_vectors(c, a);
        vap.sub_vectors(p, a);
        let d1 = vab.dot(&vap);
        let d2 = vac.dot(&vap);
        if d1 <= 0.0 && d2 <= 0.0 {
            return target.copy(a);
        }
        vbp.sub_vectors(p, b);
        let d3 = vab.dot(&vbp);
        let d4 = vac.dot(&vbp);
        if d3 >= 0.0 && d4 <= d3 {
            return target.copy(b);
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            let v = d1 / (d1 - d3);
            target.copy(a);
            return target.add_scaled_vector(&vab, v);
        }
        vcp.sub_vectors(p, c);
        let d5 = vab.dot(&vcp);
        let d6 = vac.dot(&vcp);
        if d6 >= 0.0 && d5 <= d6 {
            return target.copy(c);
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            let w = d2 / (d2 - d6);
            target.copy(a);
            return target.add_scaled_vector(&vac, w);
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            vbc.sub_vectors(c, b);
            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
            target.copy(b);
            return target.add_scaled_vector(&vbc, w);
        }
        let denom = 1.0 / (va + vb + vc);
        let v = vb * denom;
        let w = vc * denom;
        target.copy(a);
        target.add_scaled_vector(&vab, v);
        target.add_scaled_vector(&vac, w)
    }

    pub fn equals(&self, triangle: &Triangle) -> bool {
        triangle.a.equals(&self.a) && triangle.b.equals(&self.b) && triangle.c.equals(&self.c)
    }
}
