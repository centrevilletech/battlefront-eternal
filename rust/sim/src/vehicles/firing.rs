//! The geometry read by vehicles.js's firing methods. The hull keeps its
//! existing pose and rig; prepared keys identify the same fire nodes and
//! aimer limits without constructing a second skeleton.
use super::hull::{truthy, Hull, UP};
use super::pose::aim_vector;
use three::{NodeId, Vector3};

/// Keys resolved from a weapon's prepared class and its existing rig.
#[derive(Clone, Debug, Default)]
pub struct WeaponGeometry {
    /// Weapon FirePointName, used when a nonempty muzzle array is indexed
    /// by a value that is not one of its array indices.
    pub weapon_node: Option<NodeId>,
    /// First aimer's FirePointName || node || weapon FirePointName, or
    /// just the weapon FirePointName when there is no first aimer.
    pub fallback_node: Option<NodeId>,
    /// Each aimer's FirePointName node, falling back to its bone.
    pub direction_nodes: Vec<Option<NodeId>>,
    /// num(aimer.FireOutsideLimits ?? seat.settings.FireOutsideLimits, 0).
    pub outside_limits: Vec<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Muzzle {
    pub node: Option<NodeId>,
    pub aimer: Option<usize>,
    /// A muzzle's emitter, or the weapon's emitter when None.
    pub emitter: Option<usize>,
}

/// JavaScript's array lookup after `barrel++ % length`. An invalid or
/// fractional index leaves the selected muzzle undefined but still steps
/// the numeric barrel field.
pub fn next_barrel(barrel: &mut f64, length: usize) -> Option<usize> {
    if length == 0 {
        return None;
    }
    let index = *barrel % length as f64;
    *barrel += 1.0;
    (index >= 0.0 && index < length as f64 && index.fract() == 0.0).then_some(index as usize)
}

impl Hull {
    pub fn fire_nose(&self) -> Vector3 {
        let mut out = Vector3::new(
            self.center.x,
            self.center.y + 0.5,
            self.center.z + self.size.z * 0.5,
        );
        out.apply_matrix4(&self.matrix);
        out
    }

    pub fn reticle_ray(&self, seat: usize, from: &Vector3) -> Vector3 {
        if let Some(target) = self.pose.as_ref().expect("a firing rig").seats[seat].target {
            let mut out = Vector3::default();
            out.sub_vectors(&target, from);
            if out.length_sq() > 1e-6 {
                out.normalize();
                return out;
            }
        }
        self.seat_forward(seat)
    }

    pub fn bot_aim(&self, seat: usize) -> Vector3 {
        let pose = &self.pose.as_ref().expect("a firing rig").seats[seat];
        if self.seats[seat].lease.is_some() && pose.aiming {
            pose.aim
        } else {
            self.seat_forward(seat)
        }
    }

    /// Step the barrel before the clock callback, then write that barrel's
    /// kick time and read the fire point from the current retained rig.
    /// A world publishes/reloads the hull around clock when its callback
    /// can inspect or change it. The prepared rig's object layout is fixed.
    pub fn muzzle_of(
        &mut self,
        seat: usize,
        weapon: usize,
        keys: &WeaponGeometry,
        mut clock: impl FnMut(&mut Hull) -> f64,
        out: &mut Vector3,
    ) -> Muzzle {
        let (selected, count) = {
            let p = self.pose.as_mut().expect("a firing rig");
            let w = &mut p.seats[seat].weapons[weapon];
            (next_barrel(&mut w.barrel, w.muzzles.len()), w.muzzles.len())
        };
        let barrel = selected.and_then(|k| {
            self.pose.as_ref().unwrap().seats[seat].weapons[weapon].muzzles[k].barrel
        });
        if barrel.is_some() {
            let now = clock(self);
            self.pose.as_mut().unwrap().seats[seat].weapons[weapon].muzzles[selected.unwrap()]
                .kick_at = Some(now);
        }
        let p = self.pose.as_mut().unwrap();
        let rig = &p.rig.seats[seat].weapons[weapon];
        let node = selected
            .map(|k| rig.muzzles[k].fire_node)
            .or(if count == 0 {
                keys.fallback_node
            } else {
                keys.weapon_node
            });
        let aimer = if count > 0 {
            selected.and_then(|k| rig.muzzles[k].source_aimer)
        } else {
            p.seats[seat].weapons[weapon]
                .aimers
                .first()
                .and_then(|a| a.bone.map(|_| 0))
        };
        if let Some(node) = node {
            p.rig.tree.get_world_position(node, out);
        } else {
            *out = self.fire_nose();
        }
        Muzzle {
            node,
            aimer,
            emitter: selected,
        }
    }

    pub fn muzzle_of_seat(
        &mut self,
        seat: usize,
        weapon: usize,
        keys: &WeaponGeometry,
        clock: impl FnMut(&mut Hull) -> f64,
        out: &mut Vector3,
    ) -> Muzzle {
        let mut result = self.muzzle_of(seat, weapon, keys, clock, out);
        let p = self.pose.as_mut().unwrap();
        if result.node.is_none() {
            if let Some(node) = p.seats[seat].turret {
                result.node = Some(node);
                p.rig.tree.get_world_position(node, out);
            }
        }
        result
    }

    pub fn held_by_limits(
        &self,
        seat: usize,
        weapon: usize,
        aimer: Option<usize>,
        keys: &WeaponGeometry,
    ) -> bool {
        let Some(index) = aimer else { return false };
        let a = &self.pose.as_ref().unwrap().seats[seat].weapons[weapon].aimers[index];
        a.bone.is_some() && a.in_limits == Some(false) && keys.outside_limits[index] == 0.0
    }

    pub fn shot_direction(
        &self,
        seat: usize,
        weapon: usize,
        aimer: Option<usize>,
        keys: &WeaponGeometry,
        from: &Vector3,
        target: Option<&Vector3>,
        fallback: Option<&Vector3>,
    ) -> Vector3 {
        let p = self.pose.as_ref().expect("a firing rig");
        if let Some(a) = aimer
            .map(|k| &p.seats[seat].weapons[weapon].aimers[k])
            .filter(|a| a.bone.is_some())
        {
            if a.on_target {
                if let Some(target) = target {
                    let mut out = Vector3::default();
                    out.sub_vectors(target, from).normalize();
                    return out;
                }
            }
            if !a.posed {
                return a.facing;
            }
            let node = keys.direction_nodes[aimer.unwrap()].or(a.bone).unwrap();
            let mut out = Vector3::new(0.0, 0.0, 1.0);
            out.transform_direction(&p.rig.tree.node(node).matrix_world);
            return out;
        }
        if let Some(fallback) = fallback {
            return *fallback;
        }
        if let Some(target) = target {
            let mut out = Vector3::default();
            out.sub_vectors(target, from).normalize();
            return out;
        }
        let view = &self.seats[self.seat_index];
        aim_vector(view.aim_yaw, view.pitch)
    }

    pub fn beam_direction(
        &mut self,
        seat: usize,
        weapon: usize,
        node: Option<NodeId>,
        aimer: Option<usize>,
        keys: &WeaponGeometry,
        beam_from: &mut Vector3,
    ) -> Vector3 {
        let from = if let Some(node) = node {
            self.pose
                .as_mut()
                .expect("a firing rig")
                .rig
                .tree
                .get_world_position(node, beam_from)
        } else {
            *beam_from = self.fire_nose();
            *beam_from
        };
        let target = self.pose.as_ref().unwrap().seats[seat].target;
        let fallback = self.crewed(seat).then(|| self.bot_aim(seat));
        self.shot_direction(
            seat,
            weapon,
            aimer,
            keys,
            &from,
            target.as_ref(),
            fallback.as_ref(),
        )
    }
}

/// The normal scratch vector is written only for a truthy pitch.
pub fn spread(yaw: f64, pitch: f64, direction: &mut Vector3, normal: &mut Vector3) {
    if truthy(yaw) {
        direction.apply_axis_angle(&UP, yaw);
    }
    if truthy(pitch) {
        normal.cross_vectors(direction, &UP).normalize();
        direction.apply_axis_angle(normal, pitch);
    }
}

pub fn tow_held(tow_cable: bool, target: f64) -> bool {
    tow_cable && target >= 0.0
}
