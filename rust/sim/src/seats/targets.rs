//! A hull and a turret as the registry's members (src/seats.js VehicleTarget,
//! TurretTarget): what the AI's perception and the registry's queries read
//! of them. Each is a getter over its hull's or turret's state.

use super::boarding::controlling_team;
use super::hulls::HullSide;
use super::turrets::TurretSide;
use crate::gen::{SeatLease, Vehicle};
use crate::store::Slab;
use three::{Matrix4, Vector3};

/// The 2004 game's object classifier (seats.js KIND), as the AI's target
/// matchups use it: a hull's class by its base (walkers and command
/// walkers 2, flyers 5, anything else on the ground 6), a turret 8.
pub fn vehicle_kind(walker: bool, flyer: bool) -> f64 {
    if walker {
        2.0
    } else if flyer {
        5.0
    } else {
        6.0
    }
}

/// VehicleTarget.team: its first occupant's, else its own (0 before it
/// spawned, as the registry reads an undefined team).
pub fn vehicle_target_team(v: &Vehicle, side: &HullSide, leases: &Slab<SeatLease>) -> Option<f64> {
    controlling_team(v, side, leases)
}

/// VehicleTarget.category: a creature 1 (an animal), any other hull 3.
pub fn vehicle_target_category(side: &HullSide) -> f64 {
    if side.animal {
        1.0
    } else {
        3.0
    }
}

/// VehicleTarget.sphere: its box's centre in the world, and its bounding
/// radius.
pub fn vehicle_target_sphere(v: &Vehicle) -> (Vector3, f64) {
    let mut m = Matrix4::default();
    m.from_array(&v.matrix, 0);
    let mut c = super::v3(&v.center);
    c.apply_matrix4(&m);
    (c, v.bound_radius)
}

/// TurretTarget.team: whoever mans it's, else its own (0 for none).
pub fn turret_target_team(t: &TurretSide, own: Option<f64>, leases: &Slab<SeatLease>) -> f64 {
    match t.lease.and_then(|r| leases.get(r)) {
        Some(l) => l.team,
        None => own.unwrap_or(0.0),
    }
}

/// TurretTarget.sphere: Building.sphere as it stands.
pub fn turret_target_sphere(t: &TurretSide) -> (Vector3, f64) {
    t.sphere
}
