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
    source.tow_leg = None;
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

/// The battle's hulls as the world holds them (Vehicles.vehicles), for a
/// pass that reaches several: a hull whose own method calls the world is
/// taken out for the call and put back after it, so what the world does
/// meanwhile to the others (a tripped walker's blast hurting the hull that
/// tripped it) lands on them as it does in JavaScript, and the hull taken
/// out reads as gone (a default hull: not alive) to anything that looks.
pub trait Fleet {
    fn hull(&mut self, i: usize) -> &mut Hull;
    fn take(&mut self, i: usize) -> Hull;
    fn put(&mut self, i: usize, hull: Hull);
}

/// vehicles.js updateTethers: each held cable's winding, breaking and the
/// walker's fall, in the cables' order. `guns[i]`: hull i's tow turret in
/// the world, where it has one.
pub fn update_tethers<W: VehicleWorld + Fleet>(tethers: &mut [Tether], guns: &[Option<Vector3>], w: &mut W) {
    for t in tethers.iter_mut() {
        let s = t.source;
        if w.hull(s).tow_target < 0.0 {
            continue;
        }
        t.start = tow_gun(w.hull(s), guns[s]);
        let target = t.target.expect("a held cable's walker");
        let mut end = w.hull(s).tow_local;
        end.apply_matrix4(&w.hull(target).matrix);
        t.end = end;
        let span = t.start.distance_to(&t.end);
        if !w.hull(s).alive || !w.hull(target).alive {
            let mut source = w.take(s);
            release_tow(&mut source, t.has_def, "destroyed", w);
            w.put(s, source);
            continue;
        }
        if span > t.length * 1.2 {
            let mut source = w.take(s);
            release_tow(&mut source, t.has_def, "snapped", w);
            w.put(s, source);
            continue;
        }
        let (sp, tp) = (w.hull(s).position, w.hull(target).position);
        let around = math::atan2(sp.x - tp.x, sp.z - tp.z);
        let source = w.hull(s);
        source.tow_winding += angle(around - source.tow_angle);
        source.tow_angle = around;
        if v8math::abs(source.tow_winding) >= 6.0 * std::f64::consts::PI {
            let mut source = w.take(s);
            release_tow(&mut source, t.has_def, "tripped", w);
            // Credited to the bot on the cable's seat, else the hull.
            let attacker = match source.seats.get(t.seat).and_then(|seat| seat.lease).and_then(|l| l.unit) {
                Some(unit) => OrdnanceOwner::Unit(unit),
                None => OrdnanceOwner::Vehicle(source.id),
            };
            w.put(s, source);
            let mut walker = w.take(target);
            walker.damage(w, f64::INFINITY, attacker);
            w.put(target, walker);
        }
    }
}
