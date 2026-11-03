//! The per-player views the level-of-detail gate reads (runtime/
//! network-views.js): one broad perspective camera per living,
//! connected, non-spectator player, at its unit's eye along its aim
//! (the vehicle's own forward for a driver). The offline world's
//! single simulation camera reaches the gate the same way; its chase
//! math runs before the AI phase and is not this package's.

use super::super::nav::math::Vec3;
use super::world::CameraView;

/// Where one view stands and looks: resolved by the caller from the
/// player's unit and seat (network-views.js:18).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewSource {
    pub eye: Vec3,
    pub direction: Vec3,
}

/// The aim direction for a passenger or a soldier on foot
/// (network-views.js:26): yaw about +Y from +Z, pitch up.
pub fn aim_direction(yaw: f64, pitch: f64) -> Vec3 {
    Vec3::new(v8math::sin(yaw) * v8math::cos(pitch), v8math::sin(pitch), v8math::cos(yaw) * v8math::cos(pitch))
}

/// A PerspectiveCamera(100, 4, 0.1, 2000) at the eye, looked along the
/// direction, matrixWorld updated (three's lookAt basis: -Z toward
/// the target, columns in elements order).
pub fn network_view(source: ViewSource) -> CameraView {
    let target = super::vec::add(source.eye, source.direction);
    let eye = Vec3::new(source.eye.x, source.eye.y + 1.7, source.eye.z);
    let target = super::vec::add(target, Vec3::new(0.0, 1.7, 0.0));
    // three Matrix4.lookAt(eye, target, up=(0,1,0)), transposed into
    // the camera's world orientation (Object3D.lookAt for a camera).
    let mut z = super::vec::sub(eye, target);
    if super::vec::length_sq(z) == 0.0 {
        z.z = 1.0;
    }
    z = super::vec::normalize(z);
    let up = Vec3::new(0.0, 1.0, 0.0);
    let mut x = super::vec::cross(up, z);
    if super::vec::length_sq(x) == 0.0 {
        if v8math::abs(up.z) == 1.0 {
            z.x += 0.0001;
        } else {
            z.z += 0.0001;
        }
        z = super::vec::normalize(z);
        x = super::vec::cross(up, z);
    }
    x = super::vec::normalize(x);
    let y = super::vec::cross(z, x);
    let elements = [
        x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, eye.x, eye.y, eye.z, 1.0,
    ];
    CameraView { elements, fov: 100.0, aspect: 4.0, near: 0.1, far: 2000.0, zoom: 1.0 }
}
