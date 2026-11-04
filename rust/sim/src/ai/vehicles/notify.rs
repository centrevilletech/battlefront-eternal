//! The vehicle notifier (src/ai/classic/vehicles/notify.js), at the
//! controller cycle's wrap. With G the AI cycles per second there are
//! N = 3G buckets; a persistent counter, N at first, goes 0, 1, ..., N and
//! round again. A boardable thing whose id mod N is the counter is
//! considered: not when every seat is taken, nor faster than 3 m/s, a
//! vehicle only where soldiers can reach it; then the units within the
//! notify radius, at most 30, friendly or neutral to it, are each told
//! about it when an AI controller drives them, their class may enter
//! vehicles, a team's vehicle is their team's (one a person drives is open
//! to its allies and neutrals regardless), and their team is not low on
//! reinforcements.

use super::service::{VehicleAI, VehicleStorage, VehicleServices};
use super::world::{QueryFilter, Ref, World};

/// VehicleNotifier's state (notify.js:233): the counter, None before the
/// first pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Notifier {
    pub counter: Option<f64>,
}

impl VehicleAI {
    pub fn notify<W: World + ?Sized>(&mut self, w: &mut W) -> f64 { VehicleNotify::notify(self, w) }
}
impl<T: VehicleStorage> VehicleNotify for T {}
pub trait VehicleNotify: VehicleServices {
    /// notifier.run(vehicles) (notify.js:240): how many were told.
    fn notify<W: World + ?Sized>(&mut self, w: &mut W) -> f64 {
        let g = self.cycles_per_second(w);
        let n = 3.0 * g;
        let counter = self.read(|s| s.notifier.counter.unwrap_or(n));
        let counter = if counter == n { 0.0 } else { counter + 1.0 };
        self.write(|s| s.notifier.counter = Some(counter));
        let mut told = 0.0;
        for entry in self.boardables(w) {
            let id = w.unit(entry).id;
            if n > 0.0 && id % n != counter {
                continue;
            }
            told += self.consider(w, entry);
        }
        told
    }

    /// notifier.consider(vehicles, entry) (notify.js:253).
    fn consider<W: World + ?Sized>(&mut self, w: &mut W, entry: u64) -> f64 {
        let e = w.unit(entry);
        if !e.alive {
            return 0.0;
        }
        if self.seat_count(w, entry) <= self.occupied_seats(w, entry) {
            return 0.0;
        }
        let v = e.velocity;
        if v.x * v.x + v.y * v.y + v.z * v.z > 9.0 {
            return 0.0;
        }
        if e.vehicle.is_some() && !self.reachable(w, entry) {
            return 0.0;
        }
        let radius = w.settings().notify_radius;
        let team = e.team;
        let mut found = w.query(&e.position, radius, QueryFilter::FriendsOf { entry, team });
        found.truncate(30);
        let human_pilot = self.human_aboard(w, entry);
        let mut told = 0.0;
        for unit in found {
            let u = w.unit(unit);
            let Some(controller) = u.controller else { continue };
            if !w.controller(controller).is_unit_controller || !u.soldier || u.seat.is_some() {
                continue;
            }
            let no_enter = u.class.as_ref().map_or(0.0, |c| c.no_enter_vehicles);
            if no_enter != 0.0 {
                continue;
            }
            if team > 0.0 && !(w.notify_relation(team, u.team) > 0.0) && !human_pilot {
                continue;
            }
            if self.low_tickets(w, u.team) {
                continue;
            }
            w.add_vehicle_note(controller, Ref { obj: entry, generation: e.generation });
            told += 1.0;
        }
        told
    }
}
