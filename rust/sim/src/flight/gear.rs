//! src/flight/gear.js, as a world poses a craft: its gear, gear doors and
//! S-foils from its takeoff clip at the craft's own gear (gear 1 the clip's
//! first frame, landed; 0 its last, in flight), its DummyRoot track left
//! out (`poseGearAt`). A craft without the clip keeps the pose it was
//! modelled in. How far below its origin a craft stands on its gear
//! (`gearFoot`) is the content's (rust/content gear.rs); what a page draws
//! of another's gear from its records (`followFlightState`, `shownGear`'s
//! following) is a page's: in a world a craft's drawn gear is its own.

use crate::pose::Pose;
use crate::rig::hull::HullRig;
use crate::vehicles::Hull;
use v8math::{max, min};

/// clamp01(x): `Math.min(1, Math.max(0, x))`.
pub fn clamp01(x: f64) -> f64 {
    min(1.0, max(0.0, x))
}

/// Pose.apply: each touched bone's normalized quaternion and its
/// translation over its weight.
fn apply(pose: &mut Pose, rig: &mut HullRig) {
    pose.apply_out();
    for i in 0..rig.bones.len() {
        if pose.w[i] as f64 <= 1e-6 {
            continue;
        }
        let o = i * 7;
        let node = rig.tree.node_mut(rig.bones[i]);
        node.set_quaternion(pose.out[o], pose.out[o + 1], pose.out[o + 2], pose.out[o + 3]);
        node.position = three::Vector3::new(pose.out[o + 4], pose.out[o + 5], pose.out[o + 6]);
    }
}

impl Hull {
    /// shownGear(v): in a world, the craft's own gear.
    pub fn shown_gear(&self) -> f64 {
        clamp01(self.flight.gear)
    }

    /// poseGearAt(v, gear): the takeoff clip at a gear (1 down, 0 stowed).
    pub fn pose_gear_at(&mut self, gear: f64) {
        if !self.flying {
            return;
        }
        let Some(clip) = self.flight.gear_clip.clone() else { return };
        let p = self.pose.as_mut().expect("a posed hull's rig");
        let mut pose = p.work.gear.take().unwrap_or_else(|| Pose::new(p.rig.bones.len(), -1));
        pose.clear();
        pose.add(&clip.clip, (1.0 - clamp01(gear)) * clip.duration, 1.0, false);
        apply(&mut pose, &mut p.rig);
        p.work.gear = Some(pose);
        if clip.root >= 0 {
            let i = clip.root as usize;
            let (position, quaternion) = (p.rig.bind_p[i], p.rig.bind_q[i]);
            let node = p.rig.tree.node_mut(p.rig.bones[i]);
            node.position = position;
            node.copy_quaternion(&quaternion);
        }
    }

    /// poseFlightGear(v): a craft with a takeoff clip, at the gear shown.
    pub fn pose_flight_gear(&mut self) {
        if !self.flying || self.flight.gear_clip.is_none() {
            return;
        }
        let gear = self.shown_gear();
        self.pose_gear_at(gear);
    }
}
