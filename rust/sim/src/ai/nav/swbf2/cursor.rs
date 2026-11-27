//! A cursor holds the current certified leg. A new order or geometry revision
//! replaces it without retaining a path into the previous graph.
use super::super::math::Vec3;
use super::walk::{Trip, Waypoints};

#[derive(Clone, Debug)]
pub struct Swbf2Cursor {
    pub(super) revision: u64,
    pub(super) here: usize,
    pub(super) goal: usize,
    pub(super) arc: Option<usize>,
    pub(super) first_edge: Option<usize>,
    pub(super) exit_bend: Waypoints,
    pub(super) at_goal: bool,
    pub(super) pending: [Vec3; 10],
    pub(super) count: usize,
    pub(super) flags: u8,
    pub(super) after_jump: u8,
    pub(super) waypoint: Vec3,
    pub(super) best: f64,
    pub(super) idle: f64,
    pub(super) correcting: bool,
    pub(super) in_passage: bool,
    /// A leg of this cursor has been given to the executor.
    pub(super) placed: bool,
    pub recovery_attempts: u8,
}

impl Swbf2Cursor {
    pub(super) fn new(trip: Option<Trip>, destination: Vec3, entry: Vec3, revision: u64) -> Self {
        let mut cursor = Self {
            revision,
            here: 0,
            goal: 0,
            arc: trip.and_then(|t| t.direct_arc),
            first_edge: trip.and_then(|t| t.first_edge),
            exit_bend: Waypoints::NONE,
            at_goal: true,
            pending: [destination; 10],
            count: 0,
            flags: 0,
            after_jump: 0,
            waypoint: entry,
            best: f64::INFINITY,
            idle: 0.,
            recovery_attempts: 0,
            correcting: false,
            in_passage: false,
            placed: false,
        };
        if let Some(t) = trip.filter(|t| !t.direct) {
            cursor.here = t.start.node;
            cursor.goal = t.end.node;
            cursor.exit_bend = t.end.bend;
            cursor.at_goal = false;
            // A mover already on the entrance can enter its corridor
            // immediately. Steering to its own feet can displace it sideways.
            if t.start.length > 1e-8 {
                cursor.leg(t.start.bend, entry, 0);
            }
        } else {
            cursor.leg(Waypoints::NONE, destination, 0);
        }
        cursor
    }

    pub(super) fn leg(&mut self, via: Waypoints, end: Vec3, flags: u8) {
        debug_assert!(via.points().len() < self.pending.len());
        self.pending[0] = end;
        self.count = 1;
        for &mid in via.points().iter().rev() {
            self.pending[self.count] = mid;
            self.count += 1;
        }
        self.flags = flags;
        self.after_jump = 0;
        self.correcting = false;
        self.in_passage = false;
    }
}
