//! Vehicle.seatEye/seatFrame and the scalar part of poseOut. World readers
//! receive camera geometry, never the retained rig or its node matrices.
use super::*;
use crate::store::block::Block;
struct Row { value: F2Pose, next: Block, last: Block, dirty: bool, delta: super::delta::Delta }
static mut ROWS: Vec<Option<Row>> = Vec::new();
static mut OUTPUT: Option<Block> = None;
fn rows() -> &'static mut Vec<Option<Row>> { unsafe { &mut *addr_of_mut!(ROWS) } }
fn output() -> &'static mut Block { unsafe { (*addr_of_mut!(OUTPUT)).get_or_insert_with(Block::default) } }
pub fn mark(r: Ref<Vehicle>) { if let Some(Some(row)) = rows().get_mut(r.slot as usize) { row.dirty = true; } }
pub fn register(r: Ref<Vehicle>) {
    let p = &poses()[r.slot as usize].as_ref().unwrap().pose;
    let seats = p.seats.iter().map(|s| F2PoseSeat { aimers: s.weapons.iter().flat_map(|w| w.aimers.iter()).map(|_| F2Aimer::default()).collect(), ..Default::default() }).collect();
    let size = 512 + p.seats.len() * 256;
    let make = || Block { words: Vec::with_capacity(size), heap: Vec::with_capacity(size) };
    if rows().len() <= r.slot as usize { rows().resize_with(r.slot as usize + 1, || None); }
    rows()[r.slot as usize] = Some(Row { value: F2Pose { hull: r, seats, ..Default::default() }, next: make(), last: make(), dirty: true, delta: Default::default() });
    rows()[r.slot as usize].as_mut().unwrap().delta.reserve(size);
    output().words.reserve(size); output().heap.reserve(size);
}
/// The published seat frame uses simulation position rather than rendered bob.
/// The root's Euler cache stays observable without asking JS to decompose it.
fn update(row: &mut Row) {
    let r = row.value.hull;
    let mut h = take_hull(r, 0);
    let mut posed = poses()[r.slot as usize].take().unwrap();
    let p = &mut posed.pose;
    let q = p.rig.tree.node(p.rig.root).quaternion;
    let e = p.rig.tree.node_mut(p.rig.root).rotation();
    row.value.age = p.age; row.value.walk_phase = p.walk_phase; row.value.walk_blend = p.walk_blend;
    row.value.visual = v3(&p.visual); row.value.root_q = [q.x,q.y,q.z,q.w]; row.value.root_euler = [e.x,e.y,e.z];
    h.pose = Some(posed.pose);
    for (i, out) in row.value.seats.iter_mut().enumerate() {
        let sp = &h.pose.as_ref().unwrap().seats[i];
        out.last_yaw = sp.last_yaw; out.yaw_rate = sp.yaw_rate; out.has_target = sp.target.is_some(); out.target = sp.target.as_ref().map(v3).unwrap_or_default();
        let mut ai = 0;
        for w in &sp.weapons { for a in &w.aimers {
            out.aimers[ai] = F2Aimer { yaw: a.yaw, pitch: a.pitch, in_limits: a.in_limits, on_target: a.on_target, facing: v3(&a.facing), posed: a.posed }; ai += 1;
        } }
        out.frame = h.seat_frame(i).elements; out.eye = v3(&h.seat_eye(i)); out.forward = v3(&h.seat_forward(i));
    }
    posed.pose = h.pose.take().unwrap(); poses()[r.slot as usize] = Some(posed); recycle(h);
}
pub fn publish() -> *const f64 {
    let mut count = 0;
    for row in rows().iter_mut().flatten() {
        if !row.dirty { continue; }
        if !sim().store.vehicle.contains(row.value.hull) { row.dirty = false; continue; }
        update(row); row.next.begin(1, F2Pose::WORDS);
        let (words, heap) = row.next.row(0); row.value.put(words, heap); row.next.finish();
        if !row.delta.encode(row.value.hull.slot, row.value.hull.gen, &row.next, &row.last) { row.dirty = false; }
        else { count += 1; }
    }
    let out = output(); out.begin(count, F2Delta::WORDS); let mut at = 0;
    for row in rows().iter_mut().flatten() {
        if !row.dirty { continue; }
        let (words,heap) = out.row(at); row.delta.value.put(words,heap); at += 1;
        std::mem::swap(&mut row.next,&mut row.last); row.dirty = false;
    }
    out.finish(); out.ptr()
}
