//! Vehicle.seatEye/seatFrame and the scalar part of poseOut. World readers
//! receive camera geometry, never the retained rig or its node matrices.
use super::*;
use crate::store::block::Block;
struct Row { value: F2Pose, next: Block, last: Block, dirty: bool, delta: super::delta::Delta }
pub(crate) struct Geometry { rows: Vec<Option<Row>>, output: Option<Block> }
impl Geometry {
    pub(in crate::vehicles::module) const fn new() -> Self { Self { rows: Vec::new(), output: None } }
}
// Geometry publishes its detached bank while rig sampling borrows the other
// owners. A sampled row never needs a second geometry publication bank.
pub(crate) fn mark_in(fleet: &mut crate::vehicles::module::Fleet, r: Ref<Vehicle>) { fleet.geometry.mark(r) }
pub(crate) fn register_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>) { world_ctx.fleet.geometry.register(r, &world_ctx.fleet.poses[r.slot as usize].as_ref().unwrap().pose) }
/// The published seat frame uses simulation position rather than rendered bob.
/// The root's Euler cache stays observable without asking JS to decompose it.
fn update_in(world_ctx: &mut crate::world::owner::World, value: &mut F2Pose) {
    let r = value.hull;
    let mut h = take_hull_in(world_ctx, r, 0);
    let mut posed = world_ctx.fleet.poses[r.slot as usize].take().unwrap();
    let p = &mut posed.pose;
    let q = p.rig.tree.node(p.rig.root).quaternion;
    let e = p.rig.tree.node_mut(p.rig.root).rotation();
    value.age = p.age; value.walk_phase = p.walk_phase; value.walk_blend = p.walk_blend;
    value.visual = v3(&p.visual); value.root_q = [q.x,q.y,q.z,q.w]; value.root_euler = [e.x,e.y,e.z];
    h.pose = Some(posed.pose);
    for (i, out) in value.seats.iter_mut().enumerate() {
        let sp = &h.pose.as_ref().unwrap().seats[i];
        out.last_yaw = sp.last_yaw; out.yaw_rate = sp.yaw_rate; out.has_target = sp.target.is_some(); out.target = sp.target.as_ref().map(v3).unwrap_or_default();
        let mut ai = 0;
        for w in &sp.weapons { for a in &w.aimers {
            out.aimers[ai] = F2Aimer { yaw: a.yaw, pitch: a.pitch, in_limits: a.in_limits, on_target: a.on_target, facing: v3(&a.facing), posed: a.posed }; ai += 1;
        } }
        out.frame = h.seat_frame(i).elements; out.eye = v3(&h.seat_eye(i)); out.forward = v3(&h.seat_forward(i));
    }
    posed.pose = h.pose.take().unwrap(); world_ctx.fleet.poses[r.slot as usize] = Some(posed); recycle_in(world_ctx, h);
}
pub(crate) fn publish_in(world_ctx: &mut crate::world::owner::World) -> &[f64] {
    let mut geometry = std::mem::replace(&mut world_ctx.fleet.geometry, Geometry::new());
    geometry.publish(|value| { if !world_ctx.sim.store.vehicle.contains(value.hull) { return false; } update_in(world_ctx, value); true });
    world_ctx.fleet.geometry = geometry;
    words_in(&world_ctx.fleet.geometry)
}

impl Geometry {
    pub(super) fn mark(&mut self, r: Ref<Vehicle>) { if let Some(Some(row)) = self.rows.get_mut(r.slot as usize) { row.dirty = true; } }
    fn register(&mut self, r: Ref<Vehicle>, p: &HullPose) {
        let seats = p.seats.iter().map(|s| F2PoseSeat { aimers: s.weapons.iter().flat_map(|w| w.aimers.iter()).map(|_| F2Aimer::default()).collect(), ..Default::default() }).collect();
        let size = 512 + p.seats.len() * 256;
        let make = || Block { words: Vec::with_capacity(size), heap: Vec::with_capacity(size) };
        if self.rows.len() <= r.slot as usize { self.rows.resize_with(r.slot as usize + 1, || None); }
        self.rows[r.slot as usize] = Some(Row { value: F2Pose { hull: r, seats, ..Default::default() }, next: make(), last: make(), dirty: true, delta: Default::default() });
        self.rows[r.slot as usize].as_mut().unwrap().delta.reserve(size);
        self.output.get_or_insert_with(Block::default).words.reserve(size); self.output.get_or_insert_with(Block::default).heap.reserve(size);
    }
    fn publish(&mut self, mut sample: impl FnMut(&mut F2Pose) -> bool) {
        let mut count = 0;
        for row in self.rows.iter_mut().flatten() {
            if !row.dirty { continue; }
            if !sample(&mut row.value) { row.dirty = false; continue; }
            row.next.begin(1, F2Pose::WORDS);
            let (words, heap) = row.next.row(0); row.value.put(words, heap); row.next.finish();
            if !row.delta.encode(row.value.hull.slot, row.value.hull.gen, &row.next, &row.last) { row.dirty = false; }
            else { count += 1; }
        }
        let out = self.output.get_or_insert_with(Block::default); out.begin(count, F2Delta::WORDS); let mut at = 0;
        for row in self.rows.iter_mut().flatten() {
            if !row.dirty { continue; }
            let (words,heap) = out.row(at); row.delta.value.put(words,heap); at += 1;
            std::mem::swap(&mut row.next,&mut row.last); row.dirty = false;
        }
        out.finish();
    }
}


pub(super) fn words_in(geometry: &Geometry) -> &[f64] { &geometry.output.as_ref().expect("prepared geometry output").words }
/// The same bank for a native host reading by name; None before the first pose.
pub(super) fn bank_in(geometry: &Geometry) -> Option<&[f64]> { geometry.output.as_ref().map(|block| &block.words[..]) }
