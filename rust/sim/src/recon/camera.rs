//! The simulation chase view (RemoteDroids.updateCamera, SoldierCamera.update).
//! Position/rotation affect authoritative aim. FOV also travels in the own
//! player correction, so it follows SoldierCamera.updateFov in the same tick.
use super::*;
use three::{Euler, EulerOrder};
use v8math::{atan, exp, tan};

fn clearance<W: World>(world: &mut W, pivot: Vector3, boom: Vector3, q: Quaternion) -> f64 {
    if boom.length_sq() < 1e-8 {
        return 1.0;
    }
    let mut right = Vector3::new(0.4, 0.0, 0.0);
    right.apply_quaternion(&q);
    let mut up = Vector3::new(0.0, 0.4, 0.0);
    up.apply_quaternion(&q);
    let mut reach = 1.0;
    for (x, y) in [
        (0.0, 0.0),
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ] {
        let mut dir = boom;
        dir.add_scaled_vector(&right, x).add_scaled_vector(&up, y);
        let far = dir.length();
        dir.divide_scalar(far);
        if let Some(hit) = world.cast(pivot, dir, far + 0.08) {
            reach = min(reach, max(0.0, hit.distance - 0.08) / far);
        }
    }
    reach
}
impl Authority {
    pub(super) fn update_camera<W: World>(
        &self,
        store: &mut Store,
        world: &mut W,
        context: Ref<ReconContext>,
        dt: f64,
    ) {
        let c = store.recon_context.get(context).unwrap();
        let Some(r) = c.piloted else {
            return;
        };
        let zoom = if c.zoom { self.zoom_limit(store, context, true) } else { 1.0 };
        let zoomed = zoom > 1.0;
        let own = c.first_person || zoomed;
        let d = store.recon_droid.get(r).unwrap();
        let def = &self.defs.droids[d.class as usize];
        let c = &mut store.recon_context.get_mut(context).unwrap().camera;
        // SoldierCamera.updateFov precedes camera placement, including the
        // first chase frame. Zoom toggles later in RemoteDroids.control.
        let base = if own { def.first_fov } else { def.third_fov };
        let want = 2.0 * atan(tan(base / 2.0) / zoom);
        c.fov += (want - c.fov) * (1.0 - exp(-25.0 * dt));
        let mut position = v(c.position);
        let mut q = Quaternion::new(c.rotation[0], c.rotation[1], c.rotation[2], c.rotation[3]);
        if own {
            q.set_from_euler(&Euler::new(
                d.pitch,
                d.yaw + std::f64::consts::PI,
                0.0,
                EulerOrder::YXZ,
            ));
            position = v(d.eye);
            position.add_scaled_vector(&v(d.aim), if zoomed { -0.5 } else { -0.3 });
            c.fresh = true;
        } else {
            let center = def.center;
            let pivot = Vector3::new(
                d.position[0] + sin(d.yaw) * center[2] - cos(d.yaw) * center[0],
                d.position[1] + center[1],
                d.position[2] + cos(d.yaw) * center[2] + sin(d.yaw) * center[0],
            );
            let mut want = Quaternion::default();
            want.set_from_euler(&Euler::new(
                d.pitch - def.tilt * (std::f64::consts::PI / 180.0),
                d.yaw + std::f64::consts::PI,
                0.0,
                EulerOrder::YXZ,
            ));
            if c.fresh || position.distance_to(&pivot) > 80.0 {
                q = want;
            } else {
                q.slerp(&want, 1.0 - exp(-30.0 * dt));
            }
            let mut boom = v(def.offset);
            boom.apply_quaternion(&q);
            let reach = clearance(world, pivot, boom, q);
            if reach < c.reach {
                c.reach = reach;
            } else {
                c.reach += (reach - c.reach) * (1.0 - exp(-6.0 * dt));
            }
            let mut target = pivot;
            target.add_scaled_vector(&boom, c.reach);
            if c.fresh || position.distance_to(&target) > 80.0 {
                position = target;
                c.fresh = false;
            } else {
                let mut inverse = q;
                inverse.invert();
                let mut local = target;
                local
                    .sub(&position)
                    .apply_quaternion(&inverse)
                    .multiply_scalar(1.0 - exp(-15.0 * dt));
                local.apply_quaternion(&q);
                position.add(&local);
                boom = position;
                boom.sub(&pivot);
                let clear = clearance(world, pivot, boom, q);
                if clear < 1.0 {
                    position = pivot;
                    position.add_scaled_vector(&boom, clear);
                }
            }
        }
        c.position = a(position);
        c.rotation = [q.x, q.y, q.z, q.w];
    }
}
