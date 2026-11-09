//! CameraRig's critically damped motion and drag inertia for the Page stage.
use bfe_shell::page::SpawnLayout;
use three::{CoordinateSystem, Matrix4, Quaternion, Vector3 as V};
#[derive(Clone, Copy)]
struct Spring {
    value: f64,
    velocity: f64,
    target: f64,
}
impl Spring {
    fn new(value: f64) -> Self {
        Self {
            value,
            velocity: 0.,
            target: value,
        }
    }
    fn step(&mut self, dt: f64) {
        let w = 2.4;
        let x = self.value - self.target;
        let c = self.velocity + w * x;
        let e = (-w * dt).exp();
        self.value = self.target + (x + c * dt) * e;
        self.velocity = (self.velocity - w * c * dt) * e;
    }
}
pub(super) struct Rig {
    yaw: Spring,
    pitch: Spring,
    dolly: Spring,
    crane: Spring,
    pub projection: Matrix4,
    pub inverse: Matrix4,
    pub position: V,
    distance: f64,
    shift: [f64; 2],
    pub time: f64,
    pub spin: f64,
    velocity: f64,
    drag_pitch: f64,
    pub dragging: bool,
    pub turned: bool,
    since_drag: f64,
    pointer: [f64; 2],
    pub map: Matrix4,
    pub radius: f64,
    pub center: V,
}
impl Rig {
    pub fn new() -> Self {
        Self {
            yaw: Spring::new(0.),
            pitch: Spring::new(0.),
            dolly: Spring::new(1.),
            crane: Spring::new(0.),
            projection: Matrix4::default(),
            inverse: Matrix4::default(),
            position: V::default(),
            distance: 5.2,
            shift: [0.; 2],
            time: 0.,
            spin: 0.,
            velocity: 0.,
            drag_pitch: 0.,
            dragging: false,
            turned: false,
            since_drag: 0.,
            pointer: [0.; 2],
            map: Matrix4::default(),
            radius: 1.,
            center: V::default(),
        }
    }
    pub fn open(&mut self) {
        self.yaw.velocity = 0.;
        self.pitch.velocity = 0.;
        self.dolly.velocity = 0.;
        self.crane.velocity = 0.;
        self.yaw.value = 0.62;
        self.pitch.value = 0.2;
        self.dolly.value = 1.75;
        self.crane.value = 0.9;
        self.spin = 0.;
        self.velocity = 0.;
        self.drag_pitch = 0.;
        self.turned = false;
    }
    pub fn changed(&mut self, team: bool) {
        if team {
            self.yaw.velocity -= 2.4;
            self.crane.velocity += 0.6;
        } else {
            self.yaw.velocity += 0.85;
            self.dolly.velocity -= 0.3;
        }
    }
    pub fn pointer(&mut self, x: f64, y: f64) {
        self.pointer = [x, y];
    }
    pub fn drag(&mut self, dx: f64, dy: f64, dt: f64) {
        let turn = dx * 0.012;
        self.spin += turn;
        self.velocity = (self.velocity * 0.5 + turn / dt.max(1. / 120.) * 0.5).clamp(-9., 9.);
        self.drag_pitch = (self.drag_pitch - dy * 0.004).clamp(-0.2, 0.2);
        self.since_drag = 0.;
        self.turned = true;
    }
    pub fn release(&mut self) {
        self.dragging = false;
        if self.since_drag > 0.09 {
            self.velocity = 0.;
        }
        self.since_drag = 0.;
    }
    fn pose(&mut self, aspect: f64, rest: bool) {
        let yaw = if rest {
            0.
        } else {
            self.yaw.value + 0.022 * (self.time * 0.23).sin()
        };
        let pitch = 8f64.to_radians()
            + if rest {
                0.
            } else {
                self.pitch.value + 0.012 * (self.time * 0.17 + 1.3).sin()
            };
        let d = self.distance * if rest { 1. } else { self.dolly.value };
        let pivot = V::new(
            0.,
            0.95 + if rest {
                0.
            } else {
                self.crane.value + 0.025 * (self.time * 0.29 + 0.4).sin()
            },
            0.,
        );
        self.position = V::new(
            yaw.sin() * pitch.cos() * d,
            pivot.y + pitch.sin() * d,
            yaw.cos() * pitch.cos() * d,
        );
        self.inverse
            .look_at(&self.position, &pivot, &V::new(0., 1., 0.))
            .set_position_v3(&self.position)
            .invert();
        let top = 0.05 * 16f64.to_radians().tan();
        self.projection.make_perspective(
            -top * aspect,
            top * aspect,
            top,
            -top,
            0.05,
            600.,
            CoordinateSystem::WebGL,
        );
        self.projection.elements[8] -= self.shift[0];
        self.projection.elements[9] -= self.shift[1];
    }
    fn project(&self, p: V) -> V {
        let mut p = p;
        p.apply_matrix4(&self.inverse)
            .apply_matrix4(&self.projection);
        p
    }
    pub fn frame(&mut self, l: SpawnLayout, cone: bool) {
        let aspect = l.width / l.height;
        let x = l.center / l.width * 2. - 1.;
        let top = 1. - (l.figure[1] + (l.figure[3] - l.figure[1]) * 0.04) / l.height * 2.;
        let bottom = 1. - (l.figure[3] - (l.figure[3] - l.figure[1]) * 0.13) / l.height * 2.;
        let height = top - bottom;
        self.distance = 1.9 / (height * 16f64.to_radians().tan());
        self.shift = [0.; 2];
        for _ in 0..4 {
            self.pose(aspect, true);
            let feet = self.project(V::default());
            let head = self.project(V::new(0., 1.9, 0.));
            self.distance *= (head.y - feet.y) / height;
        }
        self.pose(aspect, true);
        let feet = self.project(V::default());
        let head = self.project(V::new(0., 1.9, 0.));
        self.shift = [x - feet.x, (top + bottom) * 0.5 - (head.y + feet.y) * 0.5];
        self.pose(aspect, true);
        // Stage.placeMap intersects the slot's centre ray with z=.55, then
        // shrinks eight times until its tilted top fits below the objectives.
        let slot = l.map;
        let sx = (slot[0] + slot[2]) / l.width - 1.;
        let sy = 1. - (slot[1] + slot[3]) / l.height;
        let mut inverse_projection = self.projection;
        inverse_projection.invert();
        let mut world = self.inverse;
        world.invert();
        let mut near = V::new(sx, sy, -1.);
        near.apply_matrix4(&inverse_projection)
            .apply_matrix4(&world);
        let mut far = V::new(sx, sy, 1.);
        far.apply_matrix4(&inverse_projection).apply_matrix4(&world);
        let k = (0.55 - near.z) / (far.z - near.z);
        let at = V::new(
            near.x + (far.x - near.x) * k,
            near.y + (far.y - near.y) * k,
            0.55,
        );
        let mut radius = ((slot[2] - slot[0]) / l.width
            * at.distance_to(&self.position)
            * 16f64.to_radians().tan()
            * aspect
            * 0.8)
            .max(0.05);
        let floor = if cone { 0.42 } else { 0.09 } + 0.12;
        let tilt = 44f64.to_radians();
        let top = 1. - slot[1] / l.height * 2.;
        for _ in 0..8 {
            self.center = V::new(at.x, floor + radius * tilt.cos(), 0.55);
            let high = self.project(V::new(
                at.x,
                self.center.y + radius * tilt.cos(),
                0.55 - radius * tilt.sin(),
            ));
            if high.y <= top {
                break;
            }
            let low = self.project(V::new(at.x, floor, 0.55 + radius * tilt.sin()));
            radius *= ((top - low.y) / (high.y - low.y).max(1e-3)).clamp(0.5, 0.98);
        }
        self.radius = radius;
        let target = V::new(self.position.x, self.center.y, self.position.z);
        let mut face = Matrix4::default();
        face.look_at(&target, &self.center, &V::new(0., 1., 0.));
        let mut tilt_m = Matrix4::default();
        tilt_m.make_rotation_x(-tilt);
        face.multiply(&tilt_m);
        let mut q = Quaternion::default();
        q.set_from_rotation_matrix(&face);
        self.map
            .compose(&self.center, &q, &V::new(radius, radius, radius));
        self.pose(aspect, false);
    }
    pub fn update(&mut self, dt: f64, aspect: f64) {
        self.time += dt;
        self.since_drag += dt;
        if !self.dragging {
            self.spin += self.velocity * dt;
            self.velocity *= (-2.6 * dt).exp();
            if self.since_drag > 2.2 {
                let home = (self.spin / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                self.spin += (home - self.spin) * (dt * 2.5).min(1.);
                self.velocity *= (-6. * dt).exp();
            }
        }
        self.yaw.target = -self.pointer[0] * 0.07;
        self.pitch.target = -self.pointer[1] * 0.035 + self.drag_pitch;

        self.yaw.step(dt);
        self.pitch.step(dt);
        self.dolly.step(dt);
        self.crane.step(dt);
        self.pose(aspect, false);
    }
    pub fn parent(&self, scale: f64) -> [f64; 16] {
        let mut m = Matrix4::default();
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&V::new(0., 1., 0.), self.spin);
        m.compose(&V::default(), &q, &V::new(scale, scale, scale));
        m.elements
    }
    pub fn map_point(&self, x: f64, y: f64, l: SpawnLayout) -> [f64; 2] {
        let mut p = V::new(x, y, 0.);
        p.apply_matrix4(&self.map);
        let p = self.project(p);
        [(p.x + 1.) * 0.5 * l.width, (1. - p.y) * 0.5 * l.height]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_frames_the_body_and_projects_map_picks_after_a_drag() {
        for size in [[1280., 720.], [2560., 1440.]] {
            let layout = SpawnLayout::new(size[0], size[1], 2);
            let mut rig = Rig::new();
            rig.frame(layout, false);
            // frame restores the animated pose. Measure the rest pose that
            // CameraRig.frame fits before testing drag motion separately.
            rig.pose(size[0] / size[1], true);
            let head = rig.project(V::new(0., 1.9, 0.));
            let feet = rig.project(V::default());
            assert!(
                (head.y
                    - feet.y
                    - ((layout.figure[3] - layout.figure[1]) * 0.83 / layout.height * 2.))
                    .abs()
                    < 1e-4
            );
            let before = rig.map_point(0., 0., layout);
            assert!(before[0] > layout.map[0] && before[0] < layout.map[2]);
            rig.dragging = true;
            rig.drag(120., 20., 0.2);
            rig.update(0.1, size[0] / size[1]);
            assert!(rig.turned && rig.spin > 1.);
            let pick = rig.map_point(0., 0., layout);
            assert!(pick.iter().all(|v| v.is_finite()));
            assert_ne!(before, pick);
            rig.release();
            assert!(!rig.dragging);
        }
    }
}
