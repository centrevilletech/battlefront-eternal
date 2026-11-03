//! src/flight/spawn.js transportPosts: a landed transport of a team, on
//! floor flat enough beside it, is a spawn choice for its team (never a
//! capture objective or a ticket post): the point 2 m off its right side,
//! 0.1 m over the floor a 12 m ray down finds there. The posts' records
//! (their names, labels and AI spawn paths) are the battle's bookkeeping;
//! this is where each stands.

use super::model::DOWN;
use super::{FlightClass, FlightState};
use crate::vehicles::{Hull, VehicleWorld};
use three::Vector3;

/// A transport's spawn point: the hull's place in the battle's list, its
/// team, the point and its heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportPoint {
    pub hull: usize,
    pub team: f64,
    pub point: Vector3,
    pub yaw: f64,
}

/// transportPosts(system)'s points, in the battle's hull order.
pub fn transport_points<W: VehicleWorld>(w: &mut W, hulls: &[Hull]) -> Vec<TransportPoint> {
    let mut out = Vec::new();
    for (i, v) in hulls.iter().enumerate() {
        let team = v.flight.team.unwrap_or(f64::NAN);
        if !v.alive || !v.flying || v.flight.class != FlightClass::Transport || v.flight.state != Some(FlightState::Landed) || !super::model::truthy(team) {
            continue;
        }
        let mut point = Vector3::new(v.bounds_max.x + 2.0, 1.0, 0.0);
        point.apply_quaternion(&v.quaternion).add(&v.position);
        let Some(floor) = w.cast(&point, &DOWN, 12.0, Some(v.body)) else { continue };
        if floor.normal.y < 0.8 {
            continue;
        }
        point.y = floor.point.y + 0.1;
        out.push(TransportPoint { hull: i, team, point, yaw: v.yaw });
    }
    out
}
