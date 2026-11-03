//! The tow cables (src/vehicles.js updateTethers, releaseTow, towGun): a
//! snowspeeder's harpoon holds an enemy command walker's leg (launchTow, on
//! the fire path), and each tick the cable winds as the hull circles the
//! walker; three circuits trip the walker, too long a span snaps the cable,
//! and either end dying lets it go.

use super::hull::{angle, Hull};
use super::VehicleWorld;
use crate::gen::OrdnanceOwner;
use crate::rig::math;
use three::Vector3;

/// A cable a hull has fired (an entry of Vehicles.tethers, in the order the
/// map holds them): whose, the walker it last held, the seat that fired it,
/// its length, whether it carries its weapon's definition (the release
/// sound's), and its two ends as the last tick found them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tether {
    pub source: usize,
    pub target: Option<usize>,
    pub seat: usize,
    pub length: f64,
    pub has_def: bool,
    pub start: Vector3,
    pub end: Vector3,
}

/// vehicles.js releaseTow: the cable let go.
pub fn release_tow<W: VehicleWorld>(source: &mut Hull, has_def: bool, reason: &str, w: &mut W) {
    if source.tow_target < 0.0 {
        return;
    }
    source.tow_target = -1.0;
    source.tow_winding = 0.0;
    source.tow_leg.clear();
    if has_def && reason != "destroyed" {
        w.tow_released(source.index, reason);
    }
}

/// vehicles.js towGun: the gun end of a hull's cable, its tow seat's turret
/// node in the world (`gun`, worked out from the hull's rig), else its
/// centre in its frame.
pub fn tow_gun(source: &Hull, gun: Option<Vector3>) -> Vector3 {
    if let Some(gun) = gun {
        return gun;
    }
    let mut out = source.center;
    out.apply_matrix4(&source.matrix);
    out
}

/// vehicles.js updateTethers: each held cable's winding, breaking and the
/// walker's fall, in the cables' order. `guns[i]`: hull i's tow turret in
/// the world, where it has one.
pub fn update_tethers<W: VehicleWorld>(hulls: &mut [Hull], tethers: &mut [Tether], guns: &[Option<Vector3>], w: &mut W) {
    for t in tethers.iter_mut() {
        let s = t.source;
        if hulls[s].tow_target < 0.0 {
            continue;
        }
        t.start = tow_gun(&hulls[s], guns[s]);
        let target = t.target.expect("a held cable's walker");
        let mut end = hulls[s].tow_local;
        end.apply_matrix4(&hulls[target].matrix);
        t.end = end;
        let span = t.start.distance_to(&t.end);
        if !hulls[s].alive || !hulls[target].alive {
            release_tow(&mut hulls[s], t.has_def, "destroyed", w);
            continue;
        }
        if span > t.length * 1.2 {
            release_tow(&mut hulls[s], t.has_def, "snapped", w);
            continue;
        }
        let around = math::atan2(hulls[s].position.x - hulls[target].position.x, hulls[s].position.z - hulls[target].position.z);
        hulls[s].tow_winding += angle(around - hulls[s].tow_angle);
        hulls[s].tow_angle = around;
        if v8math::abs(hulls[s].tow_winding) >= 6.0 * std::f64::consts::PI {
            release_tow(&mut hulls[s], t.has_def, "tripped", w);
            // Credited to the bot on the cable's seat, else the hull.
            let attacker = match hulls[s].seats.get(t.seat).and_then(|seat| seat.lease).and_then(|l| l.unit) {
                Some(unit) => OrdnanceOwner::Unit(unit),
                None => OrdnanceOwner::Vehicle(hulls[s].id),
            };
            hulls[target].damage(w, f64::INFINITY, attacker);
        }
    }
}
