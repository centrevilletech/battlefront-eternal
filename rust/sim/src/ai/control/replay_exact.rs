//! Object.is equality for nested replay state, including signed zero.
//! The simulation's PartialEq implementations intentionally keep normal
//! numeric equality; a conformance replay must compare every float's bits.

use super::*;

pub(super) trait Exact {
    fn exact(&self, other: &Self) -> bool;
}

impl Exact for f64 {
    fn exact(&self, other: &Self) -> bool {
        sim_replay::same_number(*self, *other)
    }
}

macro_rules! scalar {
    ($($ty:ty),* $(,)?) => {$(
        impl Exact for $ty {
            fn exact(&self, other: &Self) -> bool { self == other }
        }
    )*};
}
scalar!(bool, usize, u64, String, Policy, HelperKind, ForeignKind, ObjectiveFlavor);

impl<T: Exact> Exact for Option<T> {
    fn exact(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(a), Some(b)) => a.exact(b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl<T: Exact> Exact for Vec<T> {
    fn exact(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(a, b)| a.exact(b))
    }
}

impl<T: Exact> Exact for Box<T> {
    fn exact(&self, other: &Self) -> bool { (**self).exact(&**other) }
}

impl<A: Exact, B: Exact> Exact for (A, B) {
    fn exact(&self, other: &Self) -> bool { self.0.exact(&other.0) && self.1.exact(&other.1) }
}

macro_rules! fields {
    ($ty:ty, $($field:ident),+ $(,)?) => {
        impl Exact for $ty {
            fn exact(&self, other: &Self) -> bool { true $(&& self.$field.exact(&other.$field))+ }
        }
    };
}
fields!(Vec3, x, y, z);
fields!(Fsm, state, pending);
fields!(EntRef, unit, generation);
fields!(SourceGoal, kind, ref_a, ref_b, post, point, mode);
fields!(CommandQueue, current, queue, latch, current_done);
fields!(MoveRequest, valid, steering_target, waypoint, segment_direction, mode, lower, upper);
fields!(SquadCommand, source, kind, time, target);
fields!(EnhancedMemory, look_until, look_point, heard);
fields!(Curves, a, d, b, s);
fields!(CharacterSnap, index, team, human, unit, objective);
fields!(SchedulerStats, serviced, skipped, considered, budget);
fields!(Scheduler, counter, parity, input_parity, skips, stats);
fields!(Objectives, enhanced, threat_at, recruit_at, poll_at);
fields!(Reservation, target, ammo, owner, time);
fields!(Futile, owner, generation, target, ammo, until);
fields!(RepairServices, reservations, futile);
fields!(Interrupts, checked_at, charge, last);
fields!(HelperObs, arrived, failed, status, has_path);
fields!(Direct, target, active, arrived, failed, status, has_path, radius);
fields!(Helper, kind, obs, direct);
fields!(ControlObs, status, has_executor, avoid_side, stance, jet_pending, jet_vector, request);
fields!(ForeignShared, commands, last_command, pending, published, aim_target, leader, reboard_at,
    secondary_wait_until, exit_request, jet_pending, jet_vector, enhanced);
fields!(Patrol, fsm, post, hint, start_time, look, look_point, stance, expires, candidates, centered);
fields!(Snipe, fsm, scout, hint, stance, look, look_point, full, retry_at, revalidate_at, block_aim);
fields!(Mine, fsm, full, hint, placement, aim_point, fired_at, retry_at);
fields!(JobRef, target, ammo);
fields!(ServiceState, approach_at, progress_at, saved_health, toward, closing, workable);
fields!(ObjectiveAgent, flavor, enhanced, post, patrol, snipe, mine, start_position, center, offset,
    entry_time, support, mine_tries, deadline, job_at, job, service);

impl Exact for AgentKind {
    fn exact(&self, other: &Self) -> bool {
        use AgentKind::*;
        match (self, other) {
            (Wait { since: a, look_point: ap }, Wait { since: b, look_point: bp }) => a.exact(b) && ap.exact(bp),
            (Random { start: a, last_retry: ar, retried_at: ap }, Random { start: b, last_retry: br, retried_at: bp }) =>
                a.exact(b) && ar.exact(br) && ap.exact(bp),
            (Follow { leader: a, leader_point: ap, countdown: ac }, Follow { leader: b, leader_point: bp, countdown: bc }) =>
                a.exact(b) && ap.exact(bp) && ac.exact(bc),
            (Hold { countdown: a, look_point: ap }, Hold { countdown: b, look_point: bp }) => a.exact(b) && ap.exact(bp),
            (Board { vehicle: a }, Board { vehicle: b }) => a.exact(b),
            (Objective(a), Objective(b)) => a.exact(b),
            (Foreign(a), Foreign(b)) => a.exact(b),
            (WaitSecondary, WaitSecondary) | (Unboard, Unboard) | (MissingCombat, MissingCombat) => true,
            _ => false,
        }
    }
}
