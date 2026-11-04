//! The live turret adapter uses the seats package's existing aim and rig
//! implementation. Constructor geometry is fixed; each command owns its
//! angles, barrel, bone poses and returned points.
use crate::gen::{PaBone, PtInput, PtOperation, PtResult, PtSettings, PtState};
use crate::seats::turrets::{TurretDesc, TurretRig, TurretSide};
use three::{Matrix4, Quaternion, Vector3};

fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn a3(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn q4(v: [f64; 4]) -> Quaternion { Quaternion::new(v[0], v[1], v[2], v[3]) }
fn a4(v: Quaternion) -> [f64; 4] { [v.x, v.y, v.z, v.w] }
fn mat(v: [f64; 16]) -> Matrix4 { let mut m = Matrix4::default(); m.from_array(&v, 0); m }

pub fn side(v: PtState, settings: &PtSettings) -> TurretSide {
    let bones: Vec<_> = v.bones.iter().enumerate().map(|(i, b)| {
        let parent = settings.parents[i]; ((parent >= 0.0).then_some(parent as usize), v3(b.p), q4(b.q), v3(b.s))
    }).collect();
    let worlds: Vec<_> = v.bones.iter().map(|b| b.world).collect();
    let mut rig = TurretRig::new(&settings.root, &bones, Some(&worlds));
    rig.tree.node_mut(rig.root).matrix_world = mat(v.root_world);
    rig.drum = settings.drum.map(|n| rig.bones[n as usize]); rig.gun = settings.gun.map(|n| rig.bones[n as usize]);
    rig.seat = settings.seat.map(|n| rig.bones[n as usize]); rig.fire_points = settings.fire_points.iter().map(|n| rig.bones[*n as usize]).collect();
    TurretSide { index: v.index as usize, yaw: v.yaw, pitch: v.pitch, yaw_vel: v.yaw_vel, pitch_vel: v.pitch_vel,
        gun_yaw: v.gun_yaw, gun_pitch: v.gun_pitch, barrel: v.barrel, rig,
        desc: TurretDesc { facing: settings.facing, position: v3(settings.position), matrix: settings.matrix,
            pitch_limits: settings.pitch_limits, yaw_limits: settings.yaw_limits, wrap_yaw: settings.wrap_yaw,
            gun_pitch_limits: settings.gun_pitch_limits, gun_yaw_limits: settings.gun_yaw_limits, turn_rate: settings.turn_rate, filter: settings.filter,
            gun_pitch_rate: settings.gun_pitch_rate, gun_yaw_rate: settings.gun_yaw_rate, yaw_factor: settings.yaw_factor, pitch_factor: settings.pitch_factor,
            camera_eye: v3(settings.camera_eye), camera_center: v3(settings.camera_center), camera_offset: v3(settings.camera_offset), camera_tilt: settings.camera_tilt,
            pivot: v3(settings.pivot), up: v3(settings.up), drum_parent_turn: settings.drum_parent_turn.map(q4), gun_bind: settings.gun_bind.map(q4), ..TurretDesc::default() },
        ..TurretSide::default() }
}
pub fn state(t: &TurretSide) -> PtState {
    PtState { index: t.index as f64, yaw: t.yaw, pitch: t.pitch, yaw_vel: t.yaw_vel, pitch_vel: t.pitch_vel, gun_yaw: t.gun_yaw, gun_pitch: t.gun_pitch, barrel: t.barrel,
        root_world: t.rig.tree.node(t.rig.root).matrix_world.elements,
        bones: t.rig.bones.iter().map(|&id| { let b = t.rig.tree.node(id); PaBone { p: a3(b.position), q: a4(b.quaternion), s: a3(b.scale), world: b.matrix_world.elements } }).collect() }
}
pub fn run(input: PtInput, settings: &PtSettings) -> PtResult {
    let mut t = side(input.state, settings); let mut result = PtResult::default();
    let target = input.target.map(v3);
    match input.op {
        PtOperation::Steer => t.steer(input.dt, input.turn, input.pitch, input.zoom),
        PtOperation::HoldYaw => result.number = t.hold_yaw(input.yaw),
        PtOperation::ViewDirection => result.point = a3(t.view_direction(input.yaw, input.pitch)),
        PtOperation::Present => {
            let (yaw, pitch) = (t.yaw, t.pitch); t.yaw = input.yaw; t.pitch = input.pitch;
            t.present(); t.yaw = yaw; t.pitch = pitch;
        }
        PtOperation::GunWant => { let (yaw, pitch) = t.gun_want(target.as_ref()); result.pair = [yaw, pitch]; }
        PtOperation::AimGun => t.aim_gun(input.dt, target.as_ref()),
        PtOperation::Pose => t.pose(input.dt, target.as_ref()),
        PtOperation::Muzzle => {
            let mut direction = Vector3::default();
            result.point = a3(t.muzzle(input.advance, input.direction.then_some(&mut direction))); result.direction = a3(direction);
        }
        PtOperation::FireDirection => {
            let along = v3(input.along); let mut out = along;
            if let Some(target) = target {
                out = t.fire_direction_to(&v3(input.from), &target, &along, input.alias);
            }
            result.point = a3(out);
        }
        PtOperation::EyePoint => result.point = a3(t.eye_point(input.yaw, input.pitch)),
        PtOperation::AimOrigin => result.point = a3(t.aim_origin(input.first_person, input.yaw, input.pitch)),
        PtOperation::SeatPoint => result.point = a3(t.seat_point()),
        PtOperation::SeatTurn => {
            let node = t.rig.seat.unwrap_or(t.rig.root); let mut q = Quaternion::default();
            t.rig.tree.get_world_quaternion(node, &mut q); result.turn = a4(q);
        }
        PtOperation::SeatYaw => result.number = t.seat_yaw(),
    }
    result.state = state(&t); result
}
