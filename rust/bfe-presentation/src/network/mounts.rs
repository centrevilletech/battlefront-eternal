//! EntityGame.mount/presentMounts: bounded lean, walk-cycle and seat-aim
//! history. The page's existing network frame samples every admitted hull.
use crate::network_layout::{
    net_lease_kind as lease, net_limits as lim, net_mount_frame as f, net_mount_input as i, net_mount_output as o, net_record as r,
};
const HISTORY: usize = lim::MOUNT_HISTORY;
const SEATS: usize = lim::MOUNT_SEATS;
#[derive(Clone, Copy)]
// Mount packets contain gameplay angles as well as lean and walk phase.
// They are independent of the position snapshots consumed by hull posing;
// sampling both at the same clock prevents seats drifting over their hull.
struct Record {
    tick: f64,
    tilt: f64,
    roll: f64,
    phase: f64,
    blend: f64,
    seats: usize,
    angles: [f64; SEATS * 2],
}
impl Default for Record {
    fn default() -> Self {
        Self { tick: 0., tilt: 0., roll: 0., phase: 0., blend: 0., seats: 0, angles: [0.; SEATS * 2] }
    }
}
struct Hull {
    id: f64,
    generation: f64,
    rows: [Record; HISTORY],
    len: usize,
}
pub struct Phase {
    pub frame: Vec<f64>,
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    hulls: Vec<Hull>,
}
impl Default for Phase {
    fn default() -> Self {
        Self::new()
    }
}
impl Phase {
    // Histories, queued arrivals and publication rows reserve their complete
    // bounds before the transport can deliver the first mount record.
    pub fn new() -> Self {
        Self {
            frame: vec![0.; f::STRIDE],
            input: vec![0.; lim::SLOTS * i::STRIDE],
            output: vec![0.; lim::SLOTS * o::STRIDE],
            hulls: Vec::with_capacity(lim::SLOTS),
        }
    }
    // Retirement follows the same explicit identity command as the pose ring.
    // Removing a sparse slot must leave every other hull's history untouched.
    // Generation reuse will be admitted afresh by the next valid packet.
    pub fn remove(&mut self, id: f64) {
        self.hulls.retain(|h| h.id != id);
    }
    pub fn reset(&mut self) {
        self.hulls.clear();
        self.frame.fill(0.);
        self.output.fill(0.);
    }
    // Native receive supplies valid rows before resource admission. A duplicate lookup
    // of the same tick never inserts a second endpoint or prunes its history.
    pub fn ingest(&mut self, row: &[f64]) {
        let id = row[r::ID];
        let generation = row[r::GENERATION];
        let at = if let Some(at) = self.hulls.iter().position(|h| h.id == id) {
            at
        } else {
            if self.hulls.len() == lim::SLOTS {
                return;
            }
            self.hulls.push(Hull { id, generation, rows: [Record::default(); HISTORY], len: 0 });
            self.hulls.len() - 1
        };
        // An id may be reused after destruction. A new generation discards
        // every endpoint of its predecessor; old packets cannot interpolate
        // across two physical hulls. Duplicate stamps add no second sample.
        let hull = &mut self.hulls[at];
        if hull.generation != generation {
            hull.generation = generation;
            hull.len = 0;
        }
        if hull.len > 0 && row[r::TICK] <= hull.rows[hull.len - 1].tick {
            return;
        }
        // Twelve records span the normal playout delay. A long pause drops
        // the oldest endpoint exactly as the old per-hull history did;
        // it never grows a collection while processing resumed datagrams.
        if hull.len == HISTORY {
            hull.rows.copy_within(1..HISTORY, 0);
            hull.len -= 1;
        }
        let mut record = Record {
            tick: row[r::TICK],
            tilt: row[r::TILT],
            roll: row[r::MOUNT_ROLL],
            phase: row[r::WALK_PHASE],
            blend: row[r::WALK_BLEND],
            seats: (row[r::SEAT_COUNT] as usize).min(SEATS),
            ..Default::default()
        };
        record.angles.copy_from_slice(&row[r::SEAT_ANGLES..r::SEAT_ANGLES + SEATS * 2]);
        hull.rows[hull.len] = record;
        hull.len += 1;
    }
    // ownsMountAim compares both the correction life and current lease.
    // A pilot correction owns seat zero plus walk/lean, while a passenger's
    // precise mounted checkpoint owns only its independently aimed seat.
    fn owns(frame: &[f64], id: f64, generation: f64, seat: usize) -> bool {
        frame[f::HAS_PREVIOUS] != 0.
            && frame[f::OWN_ID] == frame[f::PREVIOUS_ID]
            && frame[f::OWN_GENERATION] == frame[f::PREVIOUS_GENERATION]
            && frame[f::LEASE_KIND] == lease::VEHICLE as f64
            && frame[f::LEASE_RELEASED] == 0.
            && frame[f::LEASE_ID] == id
            && frame[f::LEASE_GENERATION] == generation
            && frame[f::LEASE_SEAT] == seat as f64
            && (seat == 0 && frame[f::VEHICLE_ID] == id && frame[f::VEHICLE_GENERATION] == generation
                || frame[f::MOUNT_ID] == id && frame[f::MOUNT_GENERATION] == generation && frame[f::MOUNT_SEAT] == seat as f64)
    }
    // Math.round takes negative half-turns toward positive infinity. Rust's
    // round has a different tie rule; retain the original cyclic arithmetic.
    fn turn(delta: f64, length: f64) -> f64 {
        delta - length * jsrt::conv::js_round(delta / length)
    }
    pub fn present(&mut self, tick: f64) {
        // Output slots pair with explicit input identities, never positions
        // in an independently compacted history list. Sparse admissions and
        // resource retirement therefore cannot redirect another hull's aim.
        let count = (self.frame[f::COUNT] as usize).min(lim::SLOTS);
        for n in 0..count {
            let input = &self.input[n * i::STRIDE..][..i::STRIDE];
            let output = &mut self.output[n * o::STRIDE..][..o::STRIDE];
            output.fill(0.);
            let Some(hull) = self.hulls.iter_mut().find(|h| h.id == input[i::ID] && h.generation == input[i::GENERATION]) else {
                continue;
            };
            if hull.len == 0 || input[i::ALIVE] == 0. {
                continue;
            }
            // Hold the first endpoint before the clock reaches it, and the
            // latest after it. There is no angular or gait extrapolation here;
            // only the position ring has the six-tick coasting contract.
            let mut at = 0;
            while at + 1 < hull.len && hull.rows[at + 1].tick <= tick {
                at += 1;
            }
            let a = hull.rows[at];
            let b = hull.rows[(at + 1).min(hull.len - 1)];
            // Use the same arithmetic order as presentMounts. Even a held
            // endpoint passes through the zero-fraction operations, preserving
            // signed zero and the cyclic wrap at half-turn boundaries.
            let fraction = if b.tick > a.tick && tick > a.tick { (tick - a.tick) / (b.tick - a.tick) } else { 0. };
            output[o::VALID] = 1.;
            output[o::ID] = hull.id;
            output[o::GENERATION] = hull.generation;
            // The exact own checkpoint wins only for the current lease and
            // correction identity. A passenger still samples the pilot's lean
            // and all other seats; leaving immediately resumes remote history.
            let own = Self::owns(&self.frame, hull.id, hull.generation, 0);
            if own {
                output[o::TILT] = input[i::TILT];
                output[o::ROLL] = input[i::ROLL];
                output[o::WALK_PHASE] = input[i::WALK_PHASE];
                output[o::WALK_BLEND] = input[i::WALK_BLEND];
            } else {
                output[o::TILT] = a.tilt + (b.tilt - a.tilt) * fraction;
                output[o::ROLL] = a.roll + (b.roll - a.roll) * fraction;
                // Walk phase lives on the authored cycle, not the angle
                // circle. A missing cycle length retains the newest phase;
                // nonzero cycles use the original two-remainder normalization.
                let length = input[i::WALK_LENGTH];
                output[o::WALK_PHASE] = if length > 0. {
                    let phase = a.phase + Self::turn(b.phase - a.phase, length) * fraction;
                    ((phase % length) + length) % length
                } else {
                    b.phase
                };
                output[o::WALK_BLEND] = a.blend + (b.blend - a.blend) * fraction;
            }
            // Seat arrays are independently counted in both endpoints and
            // the admitted rig. Use their common prefix, including empty rigs.
            // Last-yaw seeds publish only for seats the correction does not own.
            let seats = (input[i::SEAT_COUNT] as usize).min(a.seats).min(b.seats).min(SEATS);
            output[o::SEAT_COUNT] = seats as f64;
            let mut mask = if own { 1u32 } else { 0u32 };
            let mut seed = 0u32;
            for seat in 0..seats {
                let own = Self::owns(&self.frame, hull.id, hull.generation, seat);
                let k = seat * 2;
                if own {
                    mask |= 1 << seat;
                    output[o::SEAT_ANGLES + k..o::SEAT_ANGLES + k + 2].copy_from_slice(&input[i::SEAT_ANGLES + k..i::SEAT_ANGLES + k + 2]);
                } else {
                    output[o::SEAT_ANGLES + k] = a.angles[k] + Self::turn(b.angles[k] - a.angles[k], 2. * std::f64::consts::PI) * fraction;
                    output[o::SEAT_ANGLES + k + 1] = a.angles[k + 1] + (b.angles[k + 1] - a.angles[k + 1]) * fraction;
                    if (input[i::LAST_YAW_NULL] as u32) & (1 << seat) != 0 {
                        seed |= 1 << seat;
                    }
                }
            }
            output[o::OWN_MASK] = mask as f64;
            output[o::SEED_LAST_YAW] = seed as f64;
            // Endpoints the clock passed are no longer drawn, including a
            // later rewind. Keep that original pruning contract explicitly.
            if at > 0 {
                hull.rows.copy_within(at..hull.len, 0);
                hull.len -= at;
            }
        }
    }
}
