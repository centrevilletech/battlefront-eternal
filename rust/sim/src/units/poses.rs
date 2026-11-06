//! riderPoints and riderHitShapes (rider-pose.js) over a retained soldier
//! skeleton. The unit owns the lazy cache; this sampler only fills it when
//! Unit::update_spheres asks, including a creature's held animation.
use crate::gen::HitShape;
use crate::pose::Clip;
use crate::rig::{rider::{self, RiderPoints, RiderSample, RiderScratch, RiderSkeleton}, skeleton::Skeleton};
use three::{Matrix4, Quaternion, Vector3};

pub struct BodyPose {
    skeleton: RiderSkeleton,
    bind_q: Vec<Quaternion>,
    bind_p: Vec<Vector3>,
    scratch: RiderScratch,
    points: RiderPoints,
    shapes: [rider::HitShape; rider::RIDER_SHAPES],
}

impl BodyPose {
    /// SoldierAnimator's bind lengths and joint indices, resolved once when
    /// UnitFactory.build prepares a class. World bones stay in their bind pose.
    pub fn new(skeleton: &Skeleton) -> Self {
        let rider = RiderSkeleton::new(skeleton);
        let scratch = RiderScratch::new(&rider);
        Self {
            skeleton: rider, scratch,
            bind_q: skeleton.bind.iter().map(|b| b.quaternion).collect(),
            bind_p: skeleton.bind.iter().map(|b| b.position).collect(),
            points: RiderPoints::default(), shapes: [rider::HitShape::default(); rider::RIDER_SHAPES],
        }
    }

    pub fn points(&mut self, sample: &RiderSample, root: bool) -> Option<&RiderPoints> {
        if rider::rider_points(&self.skeleton, &self.bind_q, &self.bind_p, sample, &mut self.scratch, &mut self.points, root) {
            Some(&self.points)
        } else { None }
    }

    /// SeatLease.riderShapes and AnimatedProp.heldShapes sample the same body,
    /// with the seat's pitch/turn or the held clip's time, at the first query.
    pub fn hit_shapes(&mut self, sample: &RiderSample, frame: &Matrix4, scale: f64, out: &mut [HitShape]) -> usize {
        if self.points(sample, false).is_none() { return 0; }
        let count = rider::rider_hit_shapes(&self.points, frame, Some(scale), &mut self.shapes);
        for (source, target) in self.shapes[..count].iter().zip(out) { target.set_rider(source); }
        count
    }

    /// AnimatedProp.heldFrame: inverse posed bone_root times the grab node.
    /// Both matrices are stack values; no drawn skeleton or host rig is posed.
    pub fn held_frame(&mut self, clip: &Clip, time: f64, node: &Matrix4, out: &mut Matrix4) -> bool {
        let sample = RiderSample { clip, pitch: 0.0, turn: 0.0, time: Some(v8math::max(time, 0.0)) };
        let Some(points) = self.points(&sample, true) else { return false };
        let mut root = Matrix4::default();
        root.compose(&points.root, &points.root_turn, &Vector3::new(1.0, 1.0, 1.0)).invert();
        out.multiply_matrices(node, &root);
        true
    }
}
