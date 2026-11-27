//! A placed door's attached skin palette, sampled from consecutive fixed ticks.
use bfe_content::{map::MapAdmission, Value};
use bfe_pose::{
    rig::{
        content::{clip, hull_records},
        hull::HullRig,
    },
    Clip, Pose,
};
use three::Matrix4;

use super::{door_animation, SpaceDoorSnapshot};

pub struct SpaceDoorSkin {
    pub instance: usize,
    clip: Clip,
    duration: f64,
    rig: HullRig,
    pose: Pose,
    inverses: Vec<Matrix4>,
    palette: Vec<f32>,
}

impl SpaceDoorSkin {
    /// Build the render resources once. Instances keep their own placement and reuse their pose storage.
    pub fn admit(map: &MapAdmission) -> Result<Vec<Self>, String> {
        let mut skins = Vec::new();
        for binding in door_animation::load(map)? {
            let Some(animation) = binding.animation else {
                continue;
            };
            let placed = &map.placed[binding.instance];
            let model = placed
                .model
                .as_deref()
                .and_then(|name| map.props.as_ref()?.get("models")?.get(name))
                .ok_or("animated door has no model")?;
            let desc = hull_records(&Value::Null, model, &[], Vec::new(), true);
            let root = desc
                .bones
                .iter()
                .position(|b| {
                    matches!(
                        b.name.to_ascii_lowercase().as_str(),
                        "dummyroot" | "dummy_root"
                    )
                })
                .unwrap_or(0);
            let mut rig = HullRig::new(&desc);
            let inverses: Vec<_> = rig
                .bones
                .iter()
                .chain(std::iter::once(&rig.static_bone))
                .map(|id| {
                    let mut inverse = rig.tree.node(*id).matrix_world;
                    inverse.invert();
                    inverse
                })
                .collect();
            let node = rig.tree.node_mut(rig.root);
            node.matrix_auto_update = false;
            node.matrix.elements = placed.matrix;
            let mut skin = Self {
                instance: placed.index,
                clip: clip(&animation),
                duration: animation.duration,
                pose: Pose::new(rig.bones.len(), root as i32),
                palette: vec![0.; 16 * inverses.len()],
                inverses,
                rig,
            };
            skin.sample_fraction(0.);
            skins.push(skin);
        }
        Ok(skins)
    }

    /// World-space skin matrices in the same order as the placed mesh's joint streams.
    pub fn sample(&mut self, state: &SpaceDoorSnapshot, alpha: f64) -> Option<&[f32]> {
        if state.instance != self.instance || !alpha.is_finite() {
            return None;
        }
        let fraction = state.previous_fraction
            + alpha.clamp(0., 1.) * (state.open_fraction - state.previous_fraction);
        self.sample_fraction(fraction);
        Some(self.matrices())
    }

    pub fn matrices(&self) -> &[f32] {
        &self.palette
    }

    fn sample_fraction(&mut self, fraction: f64) {
        self.pose.clear();
        self.pose
            .add(&self.clip, fraction * self.duration, 1., false);
        self.pose.apply_out();
        for (i, &id) in self.rig.bones.iter().enumerate() {
            let node = self.rig.tree.node_mut(id);
            if self.pose.w[i] > 1e-6 {
                let q = &self.pose.out[7 * i..];
                node.set_quaternion(q[0], q[1], q[2], q[3]);
                node.position.set(q[4], q[5], q[6]);
            } else {
                node.copy_quaternion(&self.rig.bind_q[i]);
                node.position = self.rig.bind_p[i];
            }
        }
        self.rig.tree.update_matrix_world(self.rig.root, true);
        for ((id, inverse), out) in self
            .rig
            .bones
            .iter()
            .chain(std::iter::once(&self.rig.static_bone))
            .zip(&self.inverses)
            .zip(self.palette.chunks_exact_mut(16))
        {
            let mut m = self.rig.tree.node(*id).matrix_world;
            m.multiply(inverse);
            out.copy_from_slice(&m.elements.map(|v| v as f32));
        }
    }
    pub fn bone_count(&self) -> usize {
        self.inverses.len()
    }
    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.rig.by_name.get(name)
    }
}

impl crate::abi::context::Context {
    /// Number of placed space doors for a local frame's cold reservation.
    pub fn space_door_count(&self) -> Result<usize, crate::abi::context::Error> {
        self.world(|w| w.space().map_or(0, |s| s.doors.len()))
    }

    /// Copy the last two fixed-tick openness values onto the local render thread.
    pub fn space_doors_into(
        &self,
        out: &mut Vec<crate::space::SpaceDoorSnapshot>,
    ) -> Result<(), crate::abi::context::Error> {
        self.world(|w| {
            out.clear();
            if let Some(space) = w.space() {
                out.extend(space.doors.iter().map(crate::space::SpaceDoor::snapshot));
            }
        })
    }
}
