//! The AI's camera facts for a dedicated match (src/runtime/network-views.js
//! networkViews): AI scheduling uses authoritative bodies and aim, never a
//! client camera position. One broad view a living player, where his body
//! stands and the way his seat or his soldier looks, composed as the page's
//! camera object did (three.js Object3D.lookAt and updateMatrixWorld).
use super::owner::Players;
use crate::gen::{AiPhaseCamera, AiPhaseHeader, UnitBody};
use crate::world::owner::World;
use three::{Matrix4, Quaternion, Vector3};

/// new PerspectiveCamera(100, 4, 0.1, 2000): one shared view shape.
const FOV: f64 = 100.0;
const ASPECT: f64 = 4.0;
const NEAR: f64 = 0.1;
const FAR: f64 = 2000.0;

/// A camera's world matrix after lookAt: its rotation from the eye to the
/// target with Y up, then composed with its position at unit scale.
pub fn camera_elements(position: &Vector3, direction: &Vector3) -> [f64; 16] {
    // Object3D.lookAt for a camera: eye at its position, then the rotation
    // matrix becomes its quaternion; updateMatrixWorld composes it again.
    let mut target = *position;
    target.add(direction);
    let mut rotation = Matrix4::default();
    rotation.look_at(position, &target, &Vector3::new(0.0, 1.0, 0.0));
    let mut quaternion = Quaternion::default();
    quaternion.set_from_rotation_matrix(&rotation);
    let mut matrix = Matrix4::default();
    matrix.compose(position, &quaternion, &Vector3::new(1.0, 1.0, 1.0));
    matrix.elements
}

/// networkViews' update: the camera rows the AI phase decodes, as many as
/// its capacity admits, in the players' order.
pub fn cameras(w: &mut World, players: &mut Players) {
    let Some(capacity) = w.ai.battle().map(|battle| battle.exchange.camera_words.len() / AiPhaseCamera::WORDS) else { return };
    let mut count = 0usize;
    let mut direction = Vector3::default();
    for &key in &players.order {
        if count >= capacity { break; }
        let Some(p) = players.pool.get(key) else { continue };
        if p.spectator || p.disconnected_at.is_some() { continue; }
        let Some(unit) = super::step::body(w, p) else { continue };
        let Some(u) = w.sim.store.unit.get(unit) else { continue };
        if !u.alive { continue; }
        let (feet, aim_yaw, aim_pitch) = match &u.soldier {
            UnitBody::Soldier(s) => (s.position, s.aim_yaw, s.aim_pitch),
            UnitBody::Droideka(d) => (d.position, d.aim_yaw, d.aim_pitch),
        };
        let lease = u.seat.and_then(|r| w.sim.store.seat_lease.get(r)).map(|l| (l.index as usize, l.vehicle, l.turret));
        let mut position = Vector3::new(feet[0], feet[1], feet[2]);
        position.y += 1.7;
        // A craft's pilot looks along his hull; a turret has yaw and pitch
        // but no aim yaw (the source's `seat?.aimYaw ?? soldier.aimYaw`);
        // a hull's other seats have both.
        let pilot = lease.and_then(|(index, hull, _)| hull.filter(|_| index == 0)).and_then(|hull| w.sim.store.vehicle.get(hull)).map(|v| v.quaternion);
        let seat = lease.and_then(|(index, hull, _)| hull.filter(|_| index > 0).and_then(|hull| w.sim.store.vehicle.get(hull)).and_then(|v| v.seats.get(index))).map(|s| (s.aim_yaw, s.pitch));
        let turret = lease.and_then(|(_, _, turret)| turret);
        if let Some(q) = pilot {
            direction.set(0.0, 0.0, 1.0);
            direction.apply_quaternion(&Quaternion::new(q[0], q[1], q[2], q[3]));
        } else {
            let (yaw, pitch) = if let Some(turret) = turret {
                let pitch = crate::vehicles::module::f2::live_seats::seats_ref(&w.fleet).crews.turrets.iter().find(|t| t.turret == turret).map(|t| t.pitch);
                (aim_yaw, pitch.unwrap_or(aim_pitch))
            } else { seat.unwrap_or((aim_yaw, aim_pitch)) };
            direction.set(v8math::sin(yaw) * v8math::cos(pitch), v8math::sin(pitch), v8math::cos(yaw) * v8math::cos(pitch));
        }
        let camera = AiPhaseCamera { elements: camera_elements(&position, &direction), fov: FOV, aspect: ASPECT, near: NEAR, far: FAR, zoom: 1.0 };
        let battle = w.ai.battle_mut().unwrap();
        camera.put(&mut battle.exchange.camera_words[count * AiPhaseCamera::WORDS..], &mut Vec::new());
        count += 1;
    }
    let battle = w.ai.battle_mut().unwrap();
    let mut header = AiPhaseHeader::take(&battle.exchange.header, &[]);
    header.cameras = count as u32;
    header.put(&mut battle.exchange.header, &mut Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_looks_along_its_direction_from_its_position() {
        let elements = camera_elements(&Vector3::new(10.0, 1.7, -3.0), &Vector3::new(0.0, 0.0, 1.0));
        assert_eq!([elements[12], elements[13], elements[14]], [10.0, 1.7, -3.0]);
        // three.js cameras look down their local -Z: the third column is -direction.
        assert!((elements[10] + 1.0).abs() < 1e-12 && elements[8].abs() < 1e-12);
        let turned = camera_elements(&Vector3::new(0.0, 0.0, 0.0), &Vector3::new(1.0, 0.0, 0.0));
        assert!((turned[8] + 1.0).abs() < 1e-12 && turned[10].abs() < 1e-12);
    }
}
