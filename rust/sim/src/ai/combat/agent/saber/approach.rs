//! The walks one fighter has attempted against living targets.
use crate::ai::combat::world::{Obj, Ref};
use three::Vector3;

#[derive(Clone, Copy)]
struct Walk {
    target: Ref,
    distance: f64,
    since: f64,
    seen: f64,
    stopped: bool,
}

#[derive(Clone, Copy)]
struct Rejection {
    target: Ref,
    from: Vector3,
    to: Vector3,
}

#[derive(Default)]
pub(crate) struct Fighter {
    life: Option<Ref>,
    walk: Option<Walk>,
    rejected: Vec<Rejection>,
    failed_routes: Vec<Rejection>,
    route: Option<(Obj, Ref, Vector3, Vector3)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Approach { Walk, Stop, Leave }

impl Fighter {
    pub(crate) fn reserve(&mut self, targets: usize) -> Option<()> {
        self.rejected.try_reserve_exact(targets.saturating_sub(self.rejected.len())).ok()?;
        self.failed_routes.try_reserve_exact(targets.saturating_sub(self.failed_routes.len())).ok()
    }

    pub(crate) fn for_life(&mut self, life: Ref) -> &mut Fighter {
        if self.life != Some(life) {
            self.life = Some(life);
            self.walk = None;
            self.rejected.clear();
            self.new_round();
        }
        self
    }

    pub(crate) fn new_round(&mut self) { self.route = None; self.failed_routes.clear(); }

    pub(crate) fn has_rejections(&self) -> bool { !self.rejected.is_empty() || !self.failed_routes.is_empty() }

    pub(crate) fn ordered(&mut self, route: Option<(Obj, Ref, Vector3, Vector3)>) { self.route = route; }

    pub(crate) fn observe_failure(&mut self, agent: Obj, failed: bool) {
        let Some((issuer, target, from, to)) = self.route else { return };
        if issuer != agent { self.route = None; return; }
        if failed {
            self.route = None;
            self.failed_routes.retain(|r| r.target.obj as u32 != target.obj as u32);
            self.failed_routes.push(Rejection { target, from, to });
            self.walk = None;
        }
    }

    fn failed_route(&mut self, target: Ref, to: Vector3) -> Option<&mut Rejection> {
        let i = self.failed_routes.iter().position(|r| r.target.obj as u32 == target.obj as u32)?;
        let r = self.failed_routes[i];
        if r.target == target && r.to.distance_to_squared(&to) < 0.25 {
            return self.failed_routes.get_mut(i);
        }
        self.failed_routes.swap_remove(i);
        None
    }

    pub(crate) fn may_route(&mut self, target: Ref, from: Vector3, to: Vector3) -> bool {
        if let Some(r) = self.failed_route(target, to) {
            r.from = from;
            return false;
        }
        true
    }

    pub(crate) fn excludes(&mut self, target: Ref, from: Vector3, to: Vector3) -> bool {
        let outside = from.distance_to_squared(&to) > super::SWING_REACH * super::SWING_REACH;
        if self.failed_route(target, to).is_some_and(|r| r.from.distance_to_squared(&from) < 0.25) { return outside; }
        let Some(i) = self.rejected.iter().position(|r| r.target.obj as u32 == target.obj as u32) else { return false };
        let r = self.rejected[i];
        if r.target == target && r.from.distance_to_squared(&from) < 0.25 && r.to.distance_to_squared(&to) < 0.25 {
            return outside;
        }
        self.rejected.swap_remove(i);
        false
    }

    pub(crate) fn approach(&mut self, target: Ref, from: Vector3, to: Vector3, now: f64) -> Approach {
        let distance = from.distance_to(&to);
        if distance <= super::SWING_REACH && self.failed_route(target, to).is_some() { return Approach::Stop; }
        if self.excludes(target, from, to) { return Approach::Leave; }
        let continuing = self.walk.is_some_and(|walk| walk.target == target && now - walk.seen <= 0.75);
        let stopped = distance <= 2.4 || distance <= super::SWING_REACH &&
            continuing && self.walk.is_some_and(|walk| walk.stopped);
        let fresh = Walk { target, distance, since: now, seen: now, stopped };
        let walk = self.walk.get_or_insert(fresh);
        if !continuing || distance > 5.0 || distance <= super::SWING_REACH || distance < walk.distance - 0.02 {
            *walk = fresh;
        } else if now - walk.since >= 2.0 {
            self.rejected.push(Rejection { target, from, to });
            self.walk = None;
            return Approach::Leave;
        }
        walk.seen = now;
        walk.stopped = stopped;
        if stopped { Approach::Stop } else { Approach::Walk }
    }
}

/// Family-specific movement used only by a saber engagement.
pub(crate) trait SaberWorld: crate::ai::combat::world::World {
    fn fighter(&mut self, unit: Obj) -> Option<&mut Fighter>;
    fn order_failed(&self, agent: Obj) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_walk_inside_blade_reach_keeps_the_opportunity_to_strike() {
        let mut fighter = Fighter::default();
        fighter.reserve(1).unwrap();
        let from = Vector3::default();
        let to = Vector3::new(2.65, 0., 0.);
        let target = Ref { obj: 3, generation: 1. };
        fighter.ordered(Some((1, target, from, to)));
        fighter.observe_failure(1, true);
        assert!(!fighter.excludes(target, from, to));
        assert_eq!(fighter.approach(target, from, to, 1.), Approach::Stop);
        assert!(!fighter.may_route(target, from, to));
    }

    #[test]
    fn only_a_failed_route_to_this_life_excludes_another_route_order() {
        let mut fighter = Fighter::default();
        fighter.reserve(3).unwrap();
        let a = Vector3::default();
        let b = Vector3::new(10., 2., 0.);
        let enemy = Ref { obj: 2, generation: 3. };
        fighter.ordered(Some((8, enemy, a, b)));
        fighter.observe_failure(7, true);
        assert!(fighter.may_route(enemy, a, b));
        fighter.ordered(Some((8, enemy, a, b)));
        fighter.observe_failure(8, false);
        assert!(fighter.may_route(enemy, a, b));
        fighter.observe_failure(8, true);
        assert!(!fighter.may_route(enemy, a, b));
        let moved = Vector3::new(3., 0., 0.);
        assert!(!fighter.excludes(enemy, moved, b));
        assert!(!fighter.may_route(enemy, moved, b));
        assert!(fighter.may_route(enemy, moved, Vector3::new(11., 2., 0.)));
        fighter.ordered(Some((8, enemy, a, b)));
        fighter.observe_failure(8, true);
        assert!(fighter.may_route(Ref { generation: 4., ..enemy }, a, b));
        fighter.ordered(Some((8, enemy, a, b)));
        fighter.observe_failure(8, true);
        fighter.new_round();
        assert!(fighter.may_route(enemy, a, b));
    }

    #[test]
    fn a_saber_stays_on_foot_until_close_and_resumes_only_outside_reach() {
        let target = Ref { obj: 4, generation: 1. };
        let mut fighter = Fighter::default();
        let mut approach = |distance, target| fighter.approach(target, Vector3::default(), Vector3::new(distance, 0., 0.), 1.);
        assert_eq!(approach(2.7, target), Approach::Walk);
        assert_eq!(approach(2.4, target), Approach::Stop);
        for distance in [2.56, 2.79, 2.41, 2.8] { assert_eq!(approach(distance, target), Approach::Stop); }
        assert_eq!(approach(2.81, target), Approach::Walk);
        assert_eq!(approach(2.5, target), Approach::Walk);
        assert_eq!(approach(2.39, target), Approach::Stop);
        assert_eq!(approach(2.6, Ref { generation: 2., ..target }), Approach::Walk);
    }

    #[test]
    fn time_away_from_the_pursuit_does_not_count_as_time_stalled() {
        let mut fighter = Fighter::default();
        let enemy = Ref { obj: 1, generation: 1. };
        let target = Vector3::new(4.5, 0., 0.);
        for tick in [0, 30, 600] {
            assert_eq!(fighter.approach(enemy, Vector3::default(), target, f64::from(tick) / 60.), Approach::Walk);
        }
        for tick in 601..961 {
            let from = Vector3::new(f64::from(tick - 600) * 0.0005, 0., 0.);
            assert_eq!(fighter.approach(enemy, from, target, f64::from(tick) / 60.), Approach::Walk);
        }
    }

    #[test]
    fn a_stalled_ledge_is_excluded_until_either_body_moves_or_the_life_changes() {
        let mut fighter = Fighter::default();
        fighter.reserve(4).unwrap();
        let origin = Vector3::default();
        let step = Vector3::new(2.77, 1.77, 0.);
        let enemy = Ref { obj: 3, generation: 1. };
        for t in 0..120 { assert_eq!(fighter.approach(enemy, origin, step, t as f64 / 60.), Approach::Walk); }
        assert_eq!(fighter.approach(enemy, origin, step, 2.), Approach::Leave);
        let other = Ref { obj: 4, ..enemy };
        assert_eq!(fighter.approach(other, origin, step, 3.), Approach::Walk);
        assert_eq!(fighter.approach(enemy, origin, step, 4.), Approach::Leave);
        let moved = Vector3::new(1.4, 0., 0.);
        assert_eq!(fighter.approach(enemy, moved, step, 5.), Approach::Stop);
        assert_eq!(fighter.approach(enemy, origin, step, 6.), Approach::Walk);
        for t in 12..16 { assert_eq!(fighter.approach(enemy, origin, step, f64::from(t) / 2.), Approach::Walk); }
        assert_eq!(fighter.approach(enemy, origin, step, 8.), Approach::Leave);
        assert_eq!(fighter.approach(enemy, origin, Vector3::new(1.3, 1.77, 0.), 9.), Approach::Stop);
        assert_eq!(fighter.approach(enemy, origin, step, 10.), Approach::Walk);
        for t in 20..24 { assert_eq!(fighter.approach(enemy, origin, step, f64::from(t) / 2.), Approach::Walk); }
        assert_eq!(fighter.approach(enemy, origin, step, 12.), Approach::Leave);
        assert_eq!(fighter.approach(Ref { generation: 2., ..enemy }, origin, step, 13.), Approach::Walk);
    }
}
