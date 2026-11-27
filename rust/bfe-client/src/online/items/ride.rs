//! A mine riding a hull, as a client online draws it. The world carries a
//! mine stuck to a hull with that hull and publishes it at rest, so its own
//! copy only follows from record to record, behind a hull the client draws
//! between records or ahead of them (the player's own). The records alone say
//! which hull and where on it: a mine its records move while it lies still
//! rides the hull whose own records keep it in one place in that hull's frame.
//! From then on it is drawn at that place on the hull as this frame draws the
//! hull, until it is thrown off (its records move it), its hull is gone, or
//! its records stop following that hull; then its own copy takes it back over
//! a short fade. An item lying still on the ground, or sinking, is never
//! touched: no hull's records move it.
use std::collections::HashMap;
use three::{Euler, EulerOrder, Matrix4, Quaternion, Vector3};

/// How far (m) a mine's place on its hull may wander between its records
/// (their rounding) and still count as riding that hull.
const FOLLOW: f64 = 0.25;
/// How far (m) its place may differ at the two records that find its hull.
const MATCH: f64 = 0.05;
/// How far (m) a still mine's records must move to count as carried.
const MOVED: f64 = 1e-3;
/// The fade (s) from its place on the hull back to its own copy.
const FADE: f64 = 0.15;

/// A hull's identity: its id and generation.
pub type Key = (u32, u32);

#[derive(Clone, Copy)]
struct Record {
    tick: f64,
    frame: Matrix4,
    still: bool,
}

#[derive(Clone, Copy)]
struct Item {
    generation: f64,
    radius: f64,
    // Its two newest records, newest first.
    records: [Option<Record>; 2],
    // The hull it rides and its frame in that hull's frame.
    on: Option<(Key, Matrix4)>,
    // Where it was last drawn on the hull; once let go, the offset its own
    // copy starts from and the fade left.
    drawn: Option<Vector3>,
    released: bool,
    offset: Vector3,
    fade: f64,
}

impl Default for Item {
    fn default() -> Self {
        Self { generation: f64::NAN, radius: 0., records: [None; 2], on: None, drawn: None, released: false, offset: Vector3::default(), fade: 0.0 }
    }
}

/// The items riding hulls, by item id.
pub struct Riders {
    // By hull: the frame when it was last in the records, and its frames at
    // its two newest records, newest first.
    hulls: HashMap<Key, (u64, [Option<(f64, Matrix4)>; 2])>,
    // By hull: its newest tilt and roll (a ground hull's lean comes in its
    // own record, sent when it changes).
    leans: HashMap<Key, (f64, f64)>,
    items: Vec<Item>,
    frame: u64,
}

/// A hull's frame from a record, as the world carries things with it: a
/// ground hull's heading (from its quaternion), tilt and roll; a craft's
/// whole quaternion.
pub fn hull_frame(position: [f64; 3], quaternion: [f64; 4], tilt: f64, roll: f64, flying: bool) -> Matrix4 {
    let mut turn = Quaternion::new(quaternion[0], quaternion[1], quaternion[2], quaternion[3]);
    if !flying {
        let mut heading = Euler::default();
        heading.set_from_quaternion(&turn, Some(EulerOrder::YXZ));
        turn.set_from_euler(&Euler::new(-tilt, heading.y, roll, EulerOrder::YXZ));
    }
    let mut frame = Matrix4::default();
    frame.compose(&Vector3::new(position[0], position[1], position[2]), &turn, &Vector3::new(1.0, 1.0, 1.0));
    frame
}

/// An item's frame from a record: where it lies, upright on its surface and
/// facing its way, as its copy is drawn.
pub fn item_frame(position: [f64; 3], up: [f64; 3], yaw: f64) -> Matrix4 {
    let mut up = Vector3::new(up[0], up[1], up[2]);
    if up.length_sq() < 1e-12 {
        up.set(0.0, 1.0, 0.0);
    }
    up.normalize();
    let turn = bfe_presentation::items::place(&up, yaw);
    let mut frame = Matrix4::default();
    frame.compose(&Vector3::new(position[0], position[1], position[2]), &turn, &Vector3::new(1.0, 1.0, 1.0));
    frame
}

/// `item` in `hull`'s frame.
fn local(hull: &Matrix4, item: &Matrix4) -> Matrix4 {
    let mut inverse = *hull;
    inverse.invert();
    let mut out = Matrix4::default();
    out.multiply_matrices(&inverse, item);
    out
}

fn position(m: &Matrix4) -> Vector3 {
    Vector3::new(m.elements[12], m.elements[13], m.elements[14])
}

impl Riders {
    pub fn new(slots: usize) -> Self {
        Self { hulls: HashMap::new(), leans: HashMap::new(), items: vec![Item::default(); slots], frame: 0 }
    }

    /// Starts a frame's records.
    pub fn begin(&mut self) {
        self.frame += 1;
    }

    /// A hull's lean record: its tilt and roll from now on.
    pub fn lean(&mut self, key: Key, tilt: f64, roll: f64) {
        self.leans.insert(key, (tilt, roll));
    }

    /// A hull in this frame's records: its newest record's tick, position
    /// and quaternion, turned by its newest lean.
    pub fn hull(&mut self, key: Key, tick: f64, position: [f64; 3], quaternion: [f64; 4], flying: bool) {
        let (tilt, roll) = self.leans.get(&key).copied().unwrap_or((0.0, 0.0));
        let frame = hull_frame(position, quaternion, tilt, roll, flying);
        let now = self.frame;
        let entry = self.hulls.entry(key).or_insert((now, [None, None]));
        entry.0 = now;
        if entry.1[0].is_some_and(|(at, _)| at == tick) {
            return;
        }
        entry.1[1] = entry.1[0];
        entry.1[0] = Some((tick, frame));
    }

    /// An item in this frame's records: its newest record's tick and frame,
    /// and whether it has no velocity of its own.
    pub fn item(&mut self, id: usize, generation: f64, active: bool, tick: f64, frame: Matrix4, still: bool, radius: f64) {
        let Some(item) = self.items.get_mut(id) else {
            return;
        };
        if item.generation != generation {
            *item = Item { generation, radius, ..Item::default() };
        }
        if !active {
            if item.on.take().is_some() {
                item.released = true;
            }
            return;
        }
        if item.records[0].is_some_and(|r| r.tick == tick) {
            return;
        }
        item.records[1] = item.records[0];
        item.records[0] = Some(Record { tick, frame, still });
    }

    pub fn forget(&mut self, id: usize) {
        if let Some(item) = self.items.get_mut(id).filter(|item| item.records[0].is_some()) {
            *item = Item::default();
        }
    }

    /// After the frame's records: hulls gone from the records are forgotten,
    /// an item that no longer rides is let go, and one its records moved
    /// while it lay still takes the hull that moved it.
    pub fn settle(&mut self, touches: impl Fn(Key, [f64; 3], f64) -> bool) {
        let now = self.frame;
        self.hulls.retain(|_, hull| hull.0 == now);
        let hulls = &self.hulls;
        self.leans.retain(|key, _| hulls.contains_key(key));
        for item in &mut self.items {
            let Some(newest) = item.records[0] else {
                continue;
            };
            if let Some((key, place)) = item.on {
                // Riding on while it lies still, its hull is in the records,
                // and a record of the hull's at the same tick keeps it there.
                let follows = newest.still
                    && self.hulls.get(&key).is_some_and(|hull| match hull.1[0] {
                        Some((tick, frame)) if tick == newest.tick => {
                            position(&local(&frame, &newest.frame)).distance_to(&position(&place)) <= FOLLOW
                        }
                        _ => true,
                    });
                if !follows {
                    item.on = None;
                    item.released = true;
                }
                continue;
            }
            let (Some(later), Some(earlier)) = (item.records[0], item.records[1]) else {
                continue;
            };
            if !later.still || !earlier.still || position(&later.frame).distance_to(&position(&earlier.frame)) < MOVED {
                continue;
            }
            // The hull whose records at the same two ticks keep the item in one place.
            let mut best: Option<(f64, Key, Matrix4)> = None;
            for (key, (_, frames)) in &self.hulls {
                let (Some((at, now_frame)), Some((before, then_frame))) = (frames[0], frames[1]) else {
                    continue;
                };
                if at != later.tick || before != earlier.tick {
                    continue;
                }
                let place = local(&now_frame, &later.frame);
                let here = position(&place);
                let then = position(&local(&then_frame, &earlier.frame));
                let gap = here.distance_to(&then);
                // A settled mine's centre is 0.3 radii off the face it hit.
                // Allow the same record rounding as the relative-motion test.
                let reach = item.radius * 0.3 + MATCH;
                if gap <= MATCH && best.as_ref().is_none_or(|b| gap < b.0 || (gap == b.0 && *key < b.1))
                    && touches(*key, [here.x, here.y, here.z], reach)
                    && touches(*key, [then.x, then.y, then.z], reach) {
                    best = Some((gap, *key, place));
                }
            }
            if let Some((_, key, place)) = best {
                item.on = Some((key, place));
                item.released = false;
                item.fade = 0.0;
            }
        }
    }

    /// An item's drawn pose this frame: at its place on its hull as `drawn`
    /// gives that hull's drawn frame, or its own copy's pose with what is
    /// left of the fade from the hull.
    pub fn draw(&mut self, id: usize, generation: f64, at: &mut Vector3, turn: &mut Quaternion, dt: f64, drawn: impl Fn(Key) -> Option<Matrix4>) {
        let Some(item) = self.items.get_mut(id) else {
            return;
        };
        if item.generation != generation {
            return;
        }
        if let Some((key, place)) = item.on {
            if let Some(hull) = drawn(key) {
                let mut frame = hull;
                frame.multiply(&place);
                let mut scale = Vector3::default();
                frame.decompose(at, turn, &mut scale);
                item.drawn = Some(*at);
                return;
            }
        }
        if item.released {
            item.released = false;
            if let Some(last) = item.drawn.take() {
                item.offset.sub_vectors(&last, at);
                item.fade = FADE;
            }
        }
        if item.fade > 0.0 {
            at.add_scaled_vector(&item.offset, item.fade / FADE);
            item.fade = (item.fade - dt).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HULL: Key = (3, 1);

    /// A ground hull at `x` along z heading `yaw`, and the mine riding it
    /// 1 m to its left and 0.5 m up (its records: where the world puts it).
    fn tick(riders: &mut Riders, tick: f64, z: f64, yaw: f64, mine: Option<(f64, [f64; 3], bool)>) {
        riders.begin();
        let half = yaw / 2.0;
        // Pitched as it speeds up: its lean comes in a record of its own.
        let tilt = z * 0.02;
        let hull = hull_frame([0.0, 0.0, z], [0.0, half.sin(), 0.0, half.cos()], tilt, 0.0, false);
        riders.lean(HULL, tilt, 0.0);
        riders.hull(HULL, tick, [0.0, 0.0, z], [0.0, half.sin(), 0.0, half.cos()], false);
        let (at, still) = match mine {
            Some((t, at, still)) => (Some((t, at)), still),
            None => {
                let mut on = Vector3::new(1.0, 0.5, 0.0);
                on.apply_matrix4(&hull);
                (Some((tick, [on.x, on.y, on.z])), true)
            }
        };
        if let Some((t, p)) = at {
            riders.item(7, 1.0, true, t, item_frame(p, [0.0, 1.0, 0.0], yaw), still, 0.4);
        }
        riders.settle(|_, point, reach| (point[1] - 0.4).abs() <= reach);
    }

    fn drawn_at(riders: &mut Riders, copy: [f64; 3], hull: Option<Matrix4>) -> Vector3 {
        let mut at = Vector3::new(copy[0], copy[1], copy[2]);
        let mut turn = Quaternion::default();
        riders.draw(7, 1.0, &mut at, &mut turn, 1.0 / 60.0, |key| (key == HULL).then_some(hull).flatten());
        at
    }

    #[test]
    fn a_mine_its_hull_moves_is_drawn_on_the_hull_as_drawn() {
        let mut riders = Riders::new(16);
        // Parked: nothing to tell it rides.
        tick(&mut riders, 10.0, 0.0, 0.0, None);
        assert!(riders.items[7].on.is_none());
        // The hull drives on, turning: two records find it.
        tick(&mut riders, 13.0, 0.5, 0.1, None);
        tick(&mut riders, 16.0, 1.5, 0.2, None);
        assert_eq!(riders.items[7].on.map(|(key, _)| key), Some(HULL));
        // Drawn on the hull where the client draws it (ahead of the records),
        // not at its own copy.
        let ahead = hull_frame([0.0, 0.0, 4.0], [0.0, 0.15f64.sin(), 0.0, 0.15f64.cos()], 0.0, 0.0, false);
        let at = drawn_at(&mut riders, [9.0, 9.0, 9.0], Some(ahead));
        let mut want = Vector3::new(1.0, 0.5, 0.0);
        want.apply_matrix4(&ahead);
        assert!(at.distance_to(&want) < 1e-9, "{at:?} {want:?}");
        // Thrown off (a velocity of its own): its copy takes it back, fading.
        tick(&mut riders, 19.0, 2.0, 0.2, Some((19.0, [5.0, 0.4, 2.0], false)));
        assert!(riders.items[7].on.is_none());
        let first = drawn_at(&mut riders, [5.0, 0.4, 2.0], Some(ahead));
        assert!(first.distance_to(&want) < 1e-9, "the fade starts where it was drawn: {first:?}");
        for _ in 0..20 {
            drawn_at(&mut riders, [5.0, 0.4, 2.0], Some(ahead));
        }
        assert_eq!(drawn_at(&mut riders, [5.0, 0.4, 2.0], Some(ahead)), Vector3::new(5.0, 0.4, 2.0));
    }

    #[test]
    fn a_mine_on_the_ground_or_sinking_is_left_to_its_copy() {
        // Lying still beside a moving hull: its records never move.
        let mut riders = Riders::new(16);
        for (t, z) in [(10.0, 0.0), (13.0, 0.5), (16.0, 1.5)] {
            tick(&mut riders, t, z, 0.0, Some((t, [4.0, 0.0, 0.0], true)));
        }
        assert!(riders.items[7].on.is_none());
        assert_eq!(drawn_at(&mut riders, [4.0, 0.0, 0.0], None), Vector3::new(4.0, 0.0, 0.0));
        // Sinking beside it: it moves, but with a velocity of its own.
        let mut riders = Riders::new(16);
        for (t, z, y) in [(10.0, 0.0, -1.0), (13.0, 0.5, -1.01), (16.0, 1.5, -1.02)] {
            tick(&mut riders, t, z, 0.0, Some((t, [4.0, y, 0.0], false)));
        }
        assert!(riders.items[7].on.is_none());
    }

    #[test]
    fn a_mine_whose_records_stop_following_its_hull_is_let_go() {
        let mut riders = Riders::new(16);
        for (t, z) in [(10.0, 0.0), (13.0, 0.5), (16.0, 1.5)] {
            tick(&mut riders, t, z, 0.0, None);
        }
        assert!(riders.items[7].on.is_some());
        // The hull drives on; the mine's record stays behind, still.
        tick(&mut riders, 19.0, 3.0, 0.0, Some((19.0, [1.0, 0.5, 1.5], true)));
        assert!(riders.items[7].on.is_none());
    }

    #[test]
    fn only_a_mine_in_contact_can_ride_and_equal_motion_chooses_the_lower_id() {
        use super::super::{d, riding_radius};
        let mut definition = [0.; d::STRIDE];
        definition[d::RADIUS] = 0.5;
        assert_eq!(riding_radius(&definition), None);
        definition[d::MINE] = 1.;
        let radius = riding_radius(&definition).unwrap();

        for ids in [&[1][..], &[8, 1, 4], &[4, 1, 8]] {
            let mut riders = Riders::new(1);
            for x in [0., 2., 4.] {
                riders.begin();
                for &id in ids {
                    let offset = if id == 1 { 40. } else { 0. };
                    riders.hull((id, 3), x, [x + offset, 0., 0.], [0., 0., 0., 1.], false);
                }
                let frame = item_frame([x, 0.15, 0.], [0., 1., 0.], 0.);
                riders.item(0, 9., true, x, frame, true, radius);
                riders.settle(|_, point, reach| {
                    point[0].abs() < 2. && point[2].abs() < 2. && point[1].abs() <= reach
                });
            }
            let expected = (ids.len() > 1).then_some((4, 3));
            assert_eq!(riders.items[0].on.map(|(key, _)| key), expected);
            riders.forget(0);
            assert!(riders.items[0].on.is_none());
        }
    }
}
