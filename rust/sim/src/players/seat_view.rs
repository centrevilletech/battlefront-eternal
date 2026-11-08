//! boarding.js page readers. The authoritative seat owner and the page share
//! eligibility rules; browser resources only identify leases and apply exits.
//!
//! The registry's relation (1 friend, 0 neutral, -1 enemy) without alliances.
//! Whether a unit may get into anything now: alive, a soldier on foot in no
//! seat (not a droideka), of a class that may enter vehicles (NoEnterVehicles);
//! a person not in a jump, fall or jet (the player's E has always waited for
//! his feet).
//! The seat a soldier of `team` boarding `vehicle` takes, or -1 when he may
//! not. As the 2004 game's boarding eligibility has it (docs/
//! ai-vehicles.md "Soldier boarding eligibility"):
//! - the hull is alive;
//! - a hull anyone holds, in any seat (pilot, gunner or passenger), takes
//!   only that crew's friends (relation above 0). An empty hull of any team
//!   takes a person;
//! - a bot boards only its own team's hulls (a hull of no team, anyone's),
//!   and only a free seat;
//! - a person takes `preferred` when it is free (the player's last seat),
//!   else the first seat that is free or a bot holds (Vehicles.displace moves
//!   the bot on). He never takes another person's seat.
//! Makes seat `index` his: a bot holding it moves on to the next free seat,
//! or gets out (Vehicles.displace). Whether the seat is free now.
//! What a soldier's use key (E) gets him into: the nearest vehicle in reach
//! ahead of him that he may board (Vehicles.enterable), else the turret he
//!
//! or manned by a bot lease (TurretSeats.man / the local crew vacates it).
//! Whether the one holding `lease` may get out now. A person may at any time:
//! in flight he bails out beside the hull and falls (the 2004 game's seat
//! exit has no airborne test). A bot stays aboard a craft in flight (the
//! Classic AI never bails out of one).

//!
//! Page rows describe occupants, not durable authority. Seat identity and
//! lease generation remain with the owning fleet; this reader cannot claim
//! or release a seat. A person may displace a bot but never another person.
//! Any hostile active occupant rejects the whole hull before seat preference.
//! A bot's team restriction also applies to an empty team's owned hull.
//! Missing controlling-team data stays permissive, as before spawn publication.
//! NoEnterVehicles retains parseFloat's NaN rejection through the zero test.
//! Airborne eligibility applies to people; bot boarding has its own approach.
//! Bailing out is separate: people may leave in flight while bots stay aboard.
//! Excess seats clamp to the fixed reader bank instead of trapping a frame.

use crate::gen::{PageSeatFrame as F, PageSeatRow as R};
const SEATS: usize = 128;
pub struct View {
    pub frame: [f64; F::WORDS],
    pub rows: [f64; SEATS * R::WORDS],
}
impl View {
    pub fn new() -> Self {
        Self {
            frame: [0.0; F::WORDS],
            rows: [0.0; SEATS * R::WORDS],
        }
    }
    pub fn run(&mut self, op: u32) -> f64 {
        let mut f = F::take(&self.frame, &[]);
        let person = f.human || f.network_human;
        let result = match op {
            0 => f64::from(person),
            1 => f64::from(f.flying && f.alive && f.in_flight),
            2 => f64::from(
                f.alive
                    && !f.dead
                    && !f.seated
                    && !f.walker
                    && f.soldier
                    && f.no_enter == 0.0
                    && (!f.person || !f.air),
            ),
            3 => self.boarding(&f) as f64,
            4 => f64::from(!f.gunner || f.lease && !f.released && !person),
            5 => f64::from(person || !(f.flying && f.alive && f.in_flight)),
            6 => {
                if f.preferred < 0 || f.preferred as u32 >= f.count {
                    -1.0
                } else if f.preferred > 0 {
                    2.0
                } else if f.flying {
                    0.0
                } else {
                    1.0
                }
            }
            // mountInReach uses the soldier's eye and aim, never his camera.
            7 => {
                f.eye = [f.position[0], f.position[1] + 1.7, f.position[2]];
                f.aim = [
                    v8math::sin(f.yaw) * v8math::cos(f.pitch),
                    v8math::sin(f.pitch),
                    v8math::cos(f.yaw) * v8math::cos(f.pitch),
                ];
                f.put(&mut self.frame, &mut Vec::new());
                0.0
            }
            _ => 0.0,
        };
        result
    }
    fn boarding(&self, f: &F) -> i32 {
        let count = (f.count as usize).min(SEATS);
        if !f.alive || count == 0 {
            return -1;
        }
        for row in self.rows[..count * R::WORDS].chunks_exact(R::WORDS) {
            let r = R::take(row, &[]);
            if r.lease && !r.released && !(r.relation > 0.0) {
                return -1;
            }
        }
        let preferred = usize::try_from(f.preferred).ok().filter(|&i| i < count);
        let free = |i: usize| R::take(&self.rows[i * R::WORDS..], &[]).free;
        if !f.person {
            if f.own > 0.0 && f.own != f.team {
                return -1;
            }
            if let Some(i) = preferred {
                return if free(i) { i as i32 } else { -1 };
            }
            if f.preferred >= 0 {
                return -1;
            }
            return (0..count).find(|&i| free(i)).map_or(-1, |i| i as i32);
        }
        if let Some(i) = preferred {
            if free(i) {
                return i as i32;
            }
        }
        for (i, row) in self.rows[..count * R::WORDS]
            .chunks_exact(R::WORDS)
            .enumerate()
        {
            let r = R::take(row, &[]);
            if r.free || r.lease && !r.person {
                return i as i32;
            }
        }
        -1
    }
}
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn page_seat_create() -> *mut View {
        Box::into_raw(Box::new(View::new()))
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_seat_frame(p: *mut View) -> *mut f64 {
        (*p).frame.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_seat_rows(p: *mut View) -> *mut f64 {
        (*p).rows.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_seat_run(p: *mut View, op: u32) -> f64 {
        (*p).run(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excessive_seat_count_clamps_and_preserves_human_occupants() {
        let mut v = View::new();
        F {
            alive: true,
            count: u32::MAX,
            person: true,
            preferred: 127,
            ..Default::default()
        }
        .put(&mut v.frame, &mut Vec::new());
        for row in v.rows.chunks_exact_mut(R::WORDS) {
            R {
                lease: true,
                person: true,
                relation: 1.0,
                ..Default::default()
            }
            .put(row, &mut Vec::new());
        }
        assert_eq!(v.run(3), -1.0);
        R {
            lease: true,
            relation: 1.0,
            ..Default::default()
        }
        .put(&mut v.rows[127 * R::WORDS..], &mut Vec::new());
        assert_eq!(v.run(3), 127.0);
    }
}
