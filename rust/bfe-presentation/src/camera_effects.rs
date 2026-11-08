//! DeathTrace.show: the replay line's camera-facing endpoints and glow span.
//!
//! The killing shot's trace for the Enhanced death camera (src/death-shot.js),
//! as a broadcast replay draws a ball's flight: a glowing line from the killer
//! to the body. Its head runs from him to the body with the line glowing
//! behind it, then it fades. It is drawn over the scene, as a telestrator's
//! line is, additive: red for an enemy's shot, green for a teammate's. Only
//! the page builds it.
//!
//! DeathTrace.show publishes a four-vertex camera-facing strip. Each endpoint
//! uses its own distance to the camera for the minimum apparent glow width.
//! The side is direction cross eye-vector; when those are nearly parallel it
//! uses the original upward fallback before normalizing. The two halves are
//! written separately so their rounded endpoints retain the same winding.
//!
//! The strip span uses the complete killer-to-body distance, even while the
//! visible head has travelled only part of that distance. Head, opacity and
//! elapsed shimmer time remain independent shader inputs. A hidden trace does
//! not need to rewrite old vertices because its scene anchor is no longer drawn.
//!
//! Gunner's shown direction comes from its world yaw and up-positive pitch.
//! The turret aim frame uses the retained root, local pivot and authored up.
//! Its eye offset and chase offsets are immutable seat content. Canonical seat
//! eyes/chase origins, when supplied, are already world facts and must not be
//! transformed through the drawn root a second time.
//!
//! A third-person origin projects the chase rest forward to the eye plane.
//! The camera's actual position can differ after obstruction or smoothing; the
//! reticle must still converge on the point selected from that aim origin.
//! The host answers wall and unit queries in order. Rust owns their default
//! reach and final point, then normalizes point minus the displayed camera.
//! A nearly zero difference retains the shown aim instead of producing NaNs.

//!
//! All strip vertices cross the float32 boundary after endpoint arithmetic.
//! Direction, span and the two independent widths remain doubles until then.
//! Uniform colour is linear numeric input, matching the original Color values.
//! A friendly flag changes only colour and cannot alter the strip geometry.

use crate::{
    unit_effects::{put, vector},
    visual_effects_layout::{gunner_frame as g, trace_frame as f},
};
use three::Vector3;
pub struct Trace {
    pub input: Vec<f64>,
    pub positions: Vec<f32>,
}
impl Default for Trace {
    fn default() -> Self {
        Self {
            input: vec![0.; f::STRIDE],
            positions: vec![0.; 12],
        }
    }
}
impl Trace {
    pub fn frame(&mut self) {
        let x = &mut self.input;
        if x[f::OPACITY] <= 0. || x[f::HEAD] <= 0. {
            x[f::VISIBLE] = 0.;
            return;
        }
        let from = vector(x, f::START);
        let to = vector(x, f::END);
        let eye = vector(x, f::CAMERA);
        let friendly = x[f::FRIENDLY] != 0.;
        put(
            x,
            f::COLOR,
            if friendly {
                Vector3::new(0.3, 1., 0.45)
            } else {
                Vector3::new(1., 0.34, 0.18)
            },
        );
        let mut direction = to;
        direction.sub(&from);
        x[f::SPAN] = direction.length().max(1e-3);
        direction.normalize();
        for i in 0..2 {
            let end = if i == 0 { from } else { to };
            let mut to_eye = eye;
            to_eye.sub(&end);
            let width = 0.34f64.max(to_eye.length() * 0.006) / 2.;
            let mut side = Vector3::default();
            side.cross_vectors(&direction, &to_eye);
            if side.length_sq() < 1e-10 {
                side.set(0., 1., 0.);
            }
            side.normalize().multiply_scalar(width);
            let at = i * 6;
            self.positions[at] = (end.x - side.x) as f32;
            self.positions[at + 1] = (end.y - side.y) as f32;
            self.positions[at + 2] = (end.z - side.z) as f32;
            self.positions[at + 3] = (end.x + side.x) as f32;
            self.positions[at + 4] = (end.y + side.y) as f32;
            self.positions[at + 5] = (end.z + side.z) as f32;
        }
        x[f::VISIBLE] = 1.;
    }
}
// Gunner's display frame publishes both directions. The host collision
// service supplies the converged point between these two bounded rounds.
pub struct Gunner {
    pub input: Vec<f64>,
}
impl Default for Gunner {
    fn default() -> Self {
        Self {
            input: vec![0.; g::STRIDE],
        }
    }
}
impl Gunner {
    pub fn frame(&mut self) {
        let x = &mut self.input;
        let yaw = x[g::YAW];
        let pitch = x[g::PITCH];
        let direction = if x[g::ACTIVE] != 0. {
            Vector3::new(
                v8math::sin(yaw) * v8math::cos(pitch),
                v8math::sin(pitch),
                v8math::cos(yaw) * v8math::cos(pitch),
            )
        } else {
            Vector3::new(0., 0., 1.)
        };
        put(x, g::DIRECTION, direction);
        if x[g::ACTIVE] == 0. {
            return;
        }
        let mut root = three::Matrix4::default();
        root.elements.copy_from_slice(&x[g::ROOT..g::ROOT + 16]);
        let mut pivot = vector(x, g::PIVOT);
        pivot.apply_matrix4(&root);
        let mut up = vector(x, g::UP);
        up.transform_direction(&root);
        // Gunner.eyePoint passed a local yaw to Turret.aimFrame. Preserve
        // subtracting and restoring the heading before its trigonometry.
        let aim_yaw = x[g::FACING] + (yaw - x[g::FACING]);
        let aim = Vector3::new(
            v8math::sin(aim_yaw) * v8math::cos(pitch),
            v8math::sin(pitch),
            v8math::cos(aim_yaw) * v8math::cos(pitch),
        );
        let turn = crate::vehicles::look_basis(&aim, &up);
        let mut frame = three::Matrix4::default();
        frame.compose(&pivot, &turn, &Vector3::new(1., 1., 1.));
        let mut eye = vector(x, g::EYE_OFFSET);
        eye.apply_matrix4(&frame);
        if x[g::CANONICAL] != 0. {
            eye = vector(x, g::CANONICAL_EYE);
        }
        put(x, g::EYE, eye);
        let mut origin = eye;
        if x[g::FIRST] == 0. {
            if x[g::CANONICAL] != 0. {
                origin = vector(x, g::CANONICAL_CHASE);
            } else {
                let mut center = vector(x, g::CENTER_OFFSET);
                center.apply_matrix4(&frame);
                let mut backward = aim;
                backward.negate();
                let mut camera_turn = crate::vehicles::look_basis(&backward, &up);
                if x[g::TILT] != 0. {
                    let mut tilt = three::Quaternion::default();
                    tilt.set_from_axis_angle(&Vector3::new(1., 0., 0.), -x[g::TILT]);
                    camera_turn.multiply(&tilt);
                }
                origin = vector(x, g::TRACK_OFFSET);
                origin.apply_quaternion(&camera_turn).add(&center);
                let mut to_eye = eye;
                to_eye.sub(&origin);
                origin.add_scaled_vector(&aim, to_eye.dot(&aim).max(0.));
            }
        }
        put(x, g::ORIGIN, origin);
        x[g::REACH] = 512.;
    }
    pub fn wall(&mut self) {
        self.input[g::REACH] = if self.input[g::WALL] != 0. {
            self.input[g::WALL_DISTANCE]
        } else {
            512.
        };
    }
    // turretAimPoint and Gunner.reticleDirection finish only after both query
    // services answer, so close walls and units preserve the same convergence.
    pub fn resolve(&mut self) {
        let x = &mut self.input;
        let far = if x[g::UNIT] != 0. {
            x[g::UNIT_DISTANCE]
        } else {
            x[g::REACH]
        };
        let direction = vector(x, g::DIRECTION);
        let mut point = vector(x, g::ORIGIN);
        point.add_scaled_vector(&direction, far);
        put(x, g::POINT, point);
        point.sub(&vector(x, g::CAMERA));
        if point.length_sq() > 1e-8 {
            point.normalize();
        } else {
            point = direction;
        }
        put(x, g::RETICLE, point);
    }
}
