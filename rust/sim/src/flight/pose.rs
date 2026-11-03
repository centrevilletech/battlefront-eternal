//! A flyer's pose at the tick (src/vehicles.js, the flyer branches of
//! placeRoot, aimTick and render): the craft drawn by the frame it flies by,
//! slerped from the tick's start to its end, and leaned into its turn about
//! its nose (`drawFlyer`); its gear posed at its own gear; and, in a world's
//! render after aimTick, nothing moved but its barrels' kick.
//!
//! The hulls' pose (crate::vehicles::pose) calls these where a hull flies.

use crate::rig::hull::HullRig;
use crate::vehicles::Hull;
use three::{lerp, Quaternion, Vector3};

/// The drawn hull's nose (vehicles.js NOSE).
const NOSE: Vector3 = Vector3::new(0.0, 0.0, 1.0);

impl Hull {
    /// placeRoot's flyer frame and lean: `this.q.slerpQuaternions(
    /// this.flightPreviousQ || this.quaternion, this.quaternion, alpha)`
    /// and `lerp(this.previousRoll ?? this.roll, this.roll, alpha)`.
    pub(crate) fn flyer_frame(&self, alpha: f64) -> (Quaternion, f64) {
        let mut frame = Quaternion::default();
        frame.slerp_quaternions(&self.flight.previous_q, &self.quaternion, alpha);
        (frame, lerp(self.previous_roll, self.roll, alpha))
    }
}

/// drawFlyer(frame, bank): the frame kept (flightFrameQ, which a chase
/// camera follows) and the root turned by it, leaned by the bank about the
/// nose.
pub(crate) fn draw_flyer(rig: &mut HullRig, frame_q: &mut Option<Quaternion>, frame: &Quaternion, bank: f64) {
    *frame_q = Some(*frame);
    let mut lean = Quaternion::default();
    lean.set_from_axis_angle(&NOSE, super::model::or_zero(bank));
    let root = rig.root;
    rig.tree.node_mut(root).with_quaternion(|q| {
        q.copy(frame).multiply(&lean);
    });
}
