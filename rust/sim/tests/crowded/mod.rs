//! A crowded wide-box battle generated from the protocol's limits and this
//! file's own random distributions: entity counts, seats, positions, speeds,
//! world field sizes and payload bytes are drawn from a seeded generator, not
//! taken from any battle.
#![allow(dead_code)]
use bfe_wide::Wide;
use bfe_sim::{
    gen::{WireField, WireRecord, WireRecordKind as Kind, WireRecordState, WireSeatAngles, WireSnapshot},
    store::Strings,
    wire::{records::{snapshot_from_records, SnapshotSlotTable, SLOT_COUNT}, PositionBounds},
};

pub struct Rng(pub Wide);
impl Rng {
    pub fn next(&mut self) -> Wide {
        self.0 = self.0.xor(self.0.shl(13));
        self.0 = self.0.xor(self.0.shr(7));
        self.0 = self.0.xor(self.0.shl(17));
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 { self.next().rem(n) }
    /// A uniform number in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 { lo + (hi - lo) * self.next().shr(11).to_f64() / 9_007_199_254_740_992.0 }
}

/// How many of each kind the generated battle holds, all within the 512-slot
/// domain: chosen to exercise seats, wide positions and every record class.
#[derive(Clone, Copy)]
pub struct Mix { pub hulls: u32, pub crewed: u32, pub soldiers: u32, pub seated: u32, pub items: u32, pub remotes: u32 }
pub const MIX: Mix = Mix { hulls: 37, crewed: 9, soldiers: 101, seated: 52, items: 29, remotes: 7 };

/// World fields by schema family, in generated proportions: per-soldier unit
/// and display fields, then static fields and summaries.
pub fn field_plan(total: usize) -> Vec<(u16, u32, usize)> {
    let mut r = Rng(Wide::new(0x00f1_e1d5, 0xe170_0007));
    (0..total).map(|i| {
        let (schema, id) = match i % 7 {
            0 | 1 => (6, 100_000 + i as u32),
            2 => (0x0203, 150_000 + i as u32),
            3 | 4 => (7, 200_000 + i as u32),
            _ => (4, 70_000 + i as u32),
        };
        let size = match schema { 0x0203 => if r.below(4) == 0 { 11 } else { 5 }, 6 => 40 + r.below(48) as usize, 7 => 20 + r.below(64) as usize, _ => 60 + r.below(100) as usize };
        (schema, id, size)
    }).collect()
}

/// The battle at `frame`: every body moves along its own generated course,
/// and with `changing` every world field carries fresh bytes each frame.
pub fn snapshot(bounds: PositionBounds, frame: u32, fields: usize, changing: bool) -> WireSnapshot {
    snapshot_of(MIX, bounds, frame, fields, changing)
}

/// `snapshot` for another mix of entities.
pub fn snapshot_of(mix: Mix, bounds: PositionBounds, frame: u32, fields: usize, changing: bool) -> WireSnapshot {
    let world = field_plan(fields).into_iter().map(|(schema, id, size)| {
        let salt = if changing { frame } else { 0 };
        WireField { id: f64::from(id), schema: f64::from(schema), generation: 1.,
            payload: (0..size).map(|i| (i as u32).wrapping_mul(31).wrapping_add(id).wrapping_add(salt.wrapping_mul(7)) as u8).collect() }
    }).collect();
    snapshot_from_records(f64::from(frame * 3), 1., &records_of(mix, bounds, frame, false), world, &mut SnapshotSlotTable::default(), &bounds, None, &Strings::default()).unwrap()
}

/// The battle's entity records at `frame`. With `churn`, soldiers and hulls
/// die, leave for a respawn delay and come back as new lives.
pub fn records(bounds: PositionBounds, frame: u32, churn: bool) -> Vec<WireRecord> {
    records_of(MIX, bounds, frame, churn)
}

/// `records` for another mix of entities.
pub fn records_of(m: Mix, bounds: PositionBounds, frame: u32, churn: bool) -> Vec<WireRecord> {
    let (min, extent) = bounds.padded_axes();
    let mut r = Rng(Wide::new(0x5e, 0xedc0_ffee));
    let mut records = Vec::new();
    let place = |r: &mut Rng, speed: f64| -> ([f64; 3], [f64; 3]) {
        let start: [f64; 3] = std::array::from_fn(|a| min[a] + extent[a] * r.range(0.1, 0.9));
        let velocity: [f64; 3] = std::array::from_fn(|a| if a == 1 { r.range(-0.1, 0.1) * speed } else { r.range(-1., 1.) * speed });
        let t = f64::from(frame) / 20.;
        (std::array::from_fn(|a| start[a] + velocity[a] * t), velocity)
    };
    // A life lasts a generated number of frames; the opening frames of every
    // life but the earliest are the respawn delay.
    let life = |id: u32, period: u32| -> (f64, bool) {
        if !churn { return (1., true); }
        let at = frame + id.wrapping_mul(37) % period;
        (f64::from(1 + at / period), at / period == 0 || at % period >= 12)
    };
    for hull in 0..m.hulls {
        let id = f64::from(hull + 1);
        let (position, velocity) = place(&mut r, 180.);
        let (generation, alive) = life(hull, 431);
        if !alive { continue; }
        records.push(WireRecord { kind: Kind::Vehicle, id, generation, class_id: Some(f64::from(1 + hull % 3)), team: Some(1. + f64::from(hull % 2)),
            health: Some(r.range(100., 900.).floor()), state: WireRecordState::F64(1.), active: true, yaw: Some(r.range(-3., 3.)), pitch: Some(r.range(-0.5, 0.5)),
            position: Some(position), velocity: Some(velocity), quaternion: Some([0., 0.6, 0., 0.8]), ..Default::default() });
        let seats = if hull < m.crewed { 4 } else { 1 };
        records.push(WireRecord { kind: Kind::Mount, id, seats: Some(vec![WireSeatAngles::default(); seats]), ..Default::default() });
    }
    let seats_left = m.crewed * 3 + (m.hulls - m.crewed);
    assert!(m.seated <= seats_left && m.soldiers <= m.seated + 64);
    let mut taken = vec![0u32; m.hulls as usize];
    for soldier in 0..m.soldiers {
        let id = f64::from(1000 + soldier);
        let (position, velocity) = place(&mut r, 8.);
        let (generation, alive) = life(soldier, 173);
        if !alive { continue; }
        records.push(WireRecord { kind: Kind::Soldier, id, generation, class_id: Some(f64::from(4 + soldier % 5)), team: Some(1. + f64::from(soldier % 2)),
            health: Some(r.range(1., 300.).floor()), state: WireRecordState::F64(1.), active: true, yaw: Some(r.range(-3., 3.)), pitch: Some(0.),
            position: Some(position), velocity: Some(velocity), ..Default::default() });
        if soldier < m.seated && !churn {
            // Fill the crewed hulls' passenger seats first, then the single seats.
            let hull = (0..m.hulls).find(|&h| taken[h as usize] < if h < m.crewed { 3 } else { 1 }).unwrap();
            taken[hull as usize] += 1;
            records.push(WireRecord { kind: Kind::Seat, id, vehicle_id: Some(f64::from(hull + 1)), seat_index: Some(f64::from(taken[hull as usize])), ..Default::default() });
        }
    }
    for item in 0..m.items + m.remotes {
        let kind = if item < m.items { Kind::Item } else { Kind::Remote };
        let (position, velocity) = place(&mut r, if kind == Kind::Remote { 12. } else { 0. });
        records.push(WireRecord { kind, id: f64::from(5000 + item), generation: 1., class_id: Some(2.), team: Some(1.),
            health: Some(30.), state: WireRecordState::F64(1.), active: true, yaw: Some(0.), pitch: Some(0.),
            position: Some(position), velocity: Some(velocity), quaternion: Some([0., 0., 0., 1.]), ..Default::default() });
    }
    assert!(records.iter().filter(|r| !matches!(r.kind, Kind::Mount | Kind::Seat)).count() <= SLOT_COUNT);
    records
}

/// A wide box drawn from the protocol's limits: spans of a few kilometres on
/// each axis, far beyond the 2004 box, well inside the 65,536 m maximum.
pub fn bounds() -> PositionBounds {
    PositionBounds::from_map_extents([-3170., -1240., -2760.], [2830., 1960., 3540.]).unwrap()
}
