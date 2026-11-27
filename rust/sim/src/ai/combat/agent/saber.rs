//! Foot engagement used by a blade fighter in a goal-driven battle.
use super::super::math::{f, normalize_d3dx};
use super::super::world::{draw_int, draw_range, Gen};
use super::{def_at, now, CombatAgent, CombatAi, GotoMode, Phase, World};
use three::Vector3;

pub(crate) mod approach;
use approach::{Approach, SaberWorld};

pub(crate) const SWING_REACH: f64 = 2.8;
const REFRESH: f64 = 0.35;
const SWBF2_ROUTE_REFRESH: f64 = 2.0;
const LEAP_CLOSE_SQ: f64 = 25.0;
const LEAP_PROBE_DIST: f64 = 25.0;
const LEAP_BLOCKED_RETRY: f64 = 4.0;
const LEAP_MIN_DELAY: f64 = 5.0;
const LEAP_MAX_DELAY: f64 = 10.0;

fn compute_melee_leap(from: &Vector3, to: &Vector3, close_pick: Option<u8>) -> Vector3 {
    let dir = normalize_d3dx(f(to.x - from.x), 0.0, f(to.z - from.z));
    let s45 = f(0.70710677);
    let (mut lx, ly, mut lz) = (f(dir.x * s45), s45, f(dir.z * s45));
    if let Some(pick) = close_pick {
        match pick {
            0 => std::mem::swap(&mut lx, &mut lz),
            1 => {
                let (nx, nz) = (f(-lz), lx);
                lx = nx;
                lz = nz;
            }
            2 => {
                let (nx, nz) = (lz, f(-lx));
                lx = nx;
                lz = nz;
            }
            _ => {}
        }
    }
    Vector3 { x: lx, y: ly, z: lz }
}

pub(crate) fn equipped<W: World + ?Sized>(w: &W, unit: super::Obj) -> bool {
    matches!(w.unit_scalars(unit).kind, 0.0 | 3.0) &&
        (0..2).any(|channel| w.current_weapon(unit, f64::from(channel))
            .is_some_and(|slot| def_at(w, unit, slot).get("base").is_str("melee")))
}

impl CombatAgent {
    pub(crate) fn saber_engagement<W: SaberWorld + ?Sized>(&mut self, w: &mut W, combat: &mut CombatAi, dt: f64, phase: Phase) -> bool {
        // A jet at the target in flight is a pattern of its own, ticked by the pattern machine.
        if phase != Phase::Behave || self.state != 4.0 || !self.tracker.fresh || self.patterns.state == super::TARGET_JET {
            return false;
        }
        let Some(unit) = w.ctl(self.controller).unit else { return false };
        if !w.unit_scalars(unit).alive || !matches!(w.unit_scalars(unit).kind, 0.0 | 3.0) {
            return false;
        }
        let swbf2 = w.uses_swbf2();
        if (swbf2 && self.pattern != 6.0) || !equipped(w, unit) || self.tracker.target(w).is_none() { return false; }

        self.tracker.select(w);
        self.tracker.assign_weapon_target(w, &combat.shared.weapons);
        if self.sync_out(w, |w, agent| w.interrupts_tick(agent, combat)) {
            return true;
        }
        let Some(target) = self.tracker.target(w).filter(|_| self.tracker.fresh) else {
            self.pending = 3.0;
            return true;
        };
        if swbf2 {
            let choice = super::dispatch(w, combat, Some(self.controller), unit, target, true);
            self.tracker.assign_weapon_target(w, &combat.shared.weapons);
            if choice.pattern == 0.0 || choice.pattern.is_nan() {
                self.helper_stop(w);
                self.pending = super::DONE;
                return true;
            }
            if choice.pattern != 6.0 {
                self.pending = 4.0;
                return true;
            }
        }
        // The 2005 melee pattern checks the jet at the target before it chases; the pattern
        // machine takes it from there.
        if swbf2 && self.try_target_jet(w, 6.0) {
            return false;
        }
        let from = w.unit_scalars(unit).position;
        let enemy = w.unit_scalars(target);
        let to = enemy.position;
        let target = super::super::world::Ref { obj: target, generation: enemy.generation };
        let time = now(w);
        let failed = w.order_failed(self.obj);
        let movement = w.fighter(unit).map_or(Approach::Walk, |fighter| {
            fighter.observe_failure(self.obj, failed);
            fighter.approach(target, from, to, time)
        });
        // The 2005 chase never gives up a target it cannot reach: it keeps trying (below).
        if movement == Approach::Leave && !swbf2 {
            self.leave_saber_target(w, combat);
            return true;
        }
        self.stationary = false;
        w.ctl_mut(self.controller).wanted_mode = 1.0;
        w.ctl_mut(self.controller).alert_anim = Some(true);
        let airborne = swbf2 && w.unit_airborne(unit);
        if swbf2 {
            if self.patterns.melee_leap_at < 0.0 {
                self.patterns.melee_leap_at = f(time + draw_range(w, Gen::A, 0.0, 3.0));
            }
            if !airborne && time > self.patterns.melee_leap_at {
                let dx = f(to.x - from.x);
                let dz = f(to.z - from.z);
                let d2 = f(f(dx * dx) + f(dz * dz));
                if d2 > 1e-4 {
                    let close_pick = (d2 <= LEAP_CLOSE_SQ).then(|| draw_int(w, Gen::A, 0.0, 3.0) as u8);
                    let leap = compute_melee_leap(&from, &to, close_pick);
                    let flat = normalize_d3dx(leap.x, 0.0, leap.z);
                    let probe = Vector3 {
                        x: f(from.x + f(flat.x * LEAP_PROBE_DIST)),
                        y: from.y,
                        z: f(from.z + f(flat.z * LEAP_PROBE_DIST)),
                    };
                    if w.swbf2_leap_clear(unit, &from, &probe) {
                        self.helper_stop(w);
                        self.patterns.last_path = -100.0;
                        self.patterns.path_active = false;
                        w.request_leap(self.controller, &leap);
                        self.patterns.melee_leap_at = f(time + draw_range(w, Gen::A, LEAP_MIN_DELAY, LEAP_MAX_DELAY));
                    } else {
                        self.patterns.melee_leap_at = f(time + LEAP_BLOCKED_RETRY);
                    }
                }
            }
        }
        if movement == Approach::Stop {
            if let Some(fighter) = w.fighter(unit) { fighter.ordered(None); }
            self.helper_stop(w);
            self.patterns.path_active = false;
        } else if !airborne && w.has_helper(self.obj) {
            if swbf2 {
                self.chase_swbf2(w, unit, target, from, to, time);
            } else if time >= self.patterns.last_path + REFRESH {
                self.patterns.last_path = time;
                let direct = w.saber_walk_clear(unit, &to) == Some(true);
                if let Some(fighter) = w.fighter(unit) {
                    if !direct && !fighter.may_route(target, from, to) {
                        self.leave_saber_target(w, combat);
                        return true;
                    }
                    fighter.ordered((!direct).then_some((self.obj, target, from, to)));
                }
                self.patterns.path_active = !direct;
                self.helper_goto(w, &to, direct.then_some(GotoMode::Direct));
                if !direct && w.order_failed(self.obj) {
                    if let Some(fighter) = w.fighter(unit) { fighter.observe_failure(self.obj, true); }
                    if from.distance_to_squared(&to) > SWING_REACH * SWING_REACH {
                        self.leave_saber_target(w, combat);
                        return true;
                    }
                    self.helper_stop(w);
                    self.patterns.path_active = false;
                }
            }
        }
        self.face(w, 0.0);
        if w.has_helper(self.obj) { w.helper_tick(self.obj, dt); }
        if w.order_failed(self.obj) {
            if let Some(fighter) = w.fighter(unit) { fighter.observe_failure(self.obj, true); }
        }
        self.tracker.fire_controls(w, &combat.shared.weapons);
        true
    }

    /// The 2005 game's chase: a clear line to the target is walked directly; otherwise a
    /// routed walk is issued, awaited until it ends (done or failed), and issued again at
    /// most every 2 s. A route that fails keeps the target.
    fn chase_swbf2<W: SaberWorld + ?Sized>(&mut self, w: &mut W, unit: super::Obj, target: super::super::world::Ref, from: Vector3, to: Vector3, time: f64) {
        if self.patterns.path_active && !w.helper_arrived(self.obj) {
            return;
        }
        self.patterns.path_active = false;
        if time < self.patterns.last_path + REFRESH {
            return;
        }
        let direct = w.saber_walk_clear(unit, &to) == Some(true);
        if !direct && time < self.patterns.melee_route_at + SWBF2_ROUTE_REFRESH {
            return;
        }
        self.patterns.last_path = time;
        if let Some(fighter) = w.fighter(unit) {
            fighter.ordered((!direct).then_some((self.obj, target, from, to)));
        }
        if !direct {
            self.patterns.melee_route_at = time;
        }
        self.patterns.path_active = !direct;
        self.helper_goto(w, &to, direct.then_some(GotoMode::Direct));
        if !direct && w.order_failed(self.obj) {
            if let Some(fighter) = w.fighter(unit) { fighter.observe_failure(self.obj, true); }
            self.helper_stop(w);
            self.patterns.path_active = false;
        }
    }

    fn leave_saber_target<W: SaberWorld + ?Sized>(&mut self, w: &mut W, combat: &CombatAi) {
        super::super::perception::clear_candidate(w, self.controller);
        self.tracker.reset(w, None);
        self.tracker.assign_weapon_target(w, &combat.shared.weapons);
        self.helper_stop(w);
        self.pending = super::DONE;
    }
}

#[cfg(test)]
mod leap_tests {
    use super::*;

    #[test]
    fn melee_leap_lifts_at_45_degrees_and_varies_direction_at_close_range() {
        let from = Vector3 { x: 0.0, y: 0.0, z: 0.0 };
        let far = Vector3 { x: 10.0, y: 0.0, z: 0.0 };
        let v = compute_melee_leap(&from, &far, None);
        assert!((v.x - f(0.70710677)).abs() < 1e-4 && (v.y - f(0.70710677)).abs() < 1e-4 && v.z.abs() < 1e-4);

        let close = Vector3 { x: 3.0, y: 0.0, z: 0.0 };
        let left = compute_melee_leap(&from, &close, Some(1));
        let right = compute_melee_leap(&from, &close, Some(2));
        let fwd = compute_melee_leap(&from, &close, Some(3));
        assert!((left.y - f(0.70710677)).abs() < 1e-4 && (right.y - f(0.70710677)).abs() < 1e-4);
        assert!((left.z + right.z).abs() < 1e-4 && left.z.abs() > 0.5, "lateral leaps oppose each other: {left:?} vs {right:?}");
        assert!(fwd.x > 0.5 && fwd.z.abs() < 1e-4);
    }

    struct LeapTestWorld {
        now: f64,
        uniform_a: f64,
        leap_clear: bool,
        shooter: super::super::super::world::UnitView,
        target: super::super::super::world::UnitView,
        blade: super::super::super::world::WeaponView,
        ctl: super::super::super::world::Ctl,
        jets: Vec<(f64, Vector3)>,
        /// When the end-jet request was raised.
        ends: Vec<f64>,
        /// Every jet steering request (the point, or none).
        steers: Vec<Option<Vector3>>,
        /// A walking helper whose routed walks all fail (the target cannot be reached on foot).
        unreachable: bool,
        gotos: Vec<(f64, bool)>,
        /// The straight line to the target crosses a barrier.
        blocked: bool,
    }

    impl super::super::super::world::Draws for LeapTestWorld {
        fn uniform_a(&mut self) -> f64 { self.uniform_a }
        fn uniform_b(&mut self) -> f64 { 0.5 }
        fn uniform_c(&mut self) -> f64 { 0.5 }
    }

    impl World for LeapTestWorld {
        fn uses_swbf2(&self) -> bool { true }
        fn swbf2_leap_clear(&mut self, _unit: super::super::Obj, _from: &Vector3, _to: &Vector3) -> bool { self.leap_clear }
        fn time(&self) -> f64 { self.now }
        fn difficulty(&self) -> super::super::super::world::DifficultyView { Default::default() }
        fn mission_stay_in_turrets(&self) -> bool { false }
        fn mission_no_jet_obstacle_test(&self) -> bool { false }
        fn ai_stay_in_turrets(&mut self) -> bool { false }
        fn unit(&self, u: super::super::Obj) -> super::super::super::world::UnitView {
            if u == 1 { self.shooter.clone() } else { self.target.clone() }
        }
        fn has_arsenal(&self, _u: super::super::Obj) -> bool { true }
        fn weapon_count(&self, _u: super::super::Obj) -> usize { 1 }
        fn current_weapon(&self, _u: super::super::Obj, channel: f64) -> Option<usize> { (channel == 0.0).then_some(0) }
        fn weapon(&self, _u: super::super::Obj, index: usize) -> Option<super::super::super::world::WeaponView> {
            (index == 0).then(|| self.blade.clone())
        }
        fn body_lease_count(&self, _u: super::super::Obj) -> usize { 0 }
        fn body_lease(&self, _u: super::super::Obj, _index: usize) -> Option<super::super::super::world::LeaseView> { None }
        fn weapon_settings(&self, _u: super::super::Obj, _index: usize) -> super::super::super::world::WeaponSettings { unimplemented!() }
        fn edit_weapon<R>(&mut self, _u: super::super::Obj, _index: usize, _edit: impl FnOnce(&mut super::super::super::world::WeaponSettings) -> R) -> R { unimplemented!() }
        fn origin(&mut self, u: super::super::Obj) -> Vector3 { if u == 1 { self.shooter.position } else { self.target.position } }
        fn forward(&mut self, _u: super::super::Obj) -> Vector3 { Vector3 { x: 1.0, y: 0.0, z: 0.0 } }
        fn aim_point(&mut self, u: super::super::Obj) -> Vector3 { if u == 1 { self.shooter.position } else { self.target.position } }
        fn sphere(&mut self, _u: super::super::Obj) -> super::super::super::world::Sphere { super::super::super::world::Sphere { center: Vector3::default(), radius: 1.0 } }
        fn set_intent_buttons(&mut self, _u: super::super::Obj, _b0: f64, _b1: f64) {}
        fn set_intent_aim(&mut self, _u: super::super::Obj, _aim: Option<Vector3>) {}
        fn arsenal_select(&mut self, _u: super::super::Obj, _index: usize) -> bool { true }
        fn use_classic_random(&mut self, _u: super::super::Obj) {}
        fn object_buffer(&self) -> crate::ai::owned::scratch::List<super::super::Obj> { unimplemented!() }
        fn query(&mut self, _center: &Vector3, _radius: f64, _filter: super::super::super::world::QueryFilter) -> crate::ai::owned::scratch::List<super::super::Obj> { unimplemented!() }
        fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
        fn living(&mut self) -> crate::ai::owned::scratch::List<super::super::Obj> { unimplemented!() }
        fn los(&mut self, _from: &Vector3, _to: &Vector3, _a: Option<super::super::Obj>, _b: Option<super::super::Obj>) -> bool { true }
        fn curves_for(&mut self, _team: f64) -> Option<super::super::super::world::Curves> { None }
        fn character_team(&self, _character: super::super::Obj) -> f64 { 1.0 }
        fn fresh_perception(&mut self, _c: super::super::Obj) -> super::super::super::perception::Perception { Default::default() }
        fn ctl(&self, _c: super::super::Obj) -> &super::super::super::world::Ctl { &self.ctl }
        fn ctl_mut(&mut self, _c: super::super::Obj) -> &mut super::super::super::world::Ctl { &mut self.ctl }
        fn agent_controller(&self, _agent: super::super::Obj) -> super::super::Obj { 10 }
        fn tracker(&mut self, _agent: super::super::Obj) -> Option<&mut super::super::super::perception::TargetTracker> { None }
        fn is_controller(&self, _c: super::super::Obj) -> bool { true }
        fn agent_accepts_alert(&mut self, _agent: super::super::Obj, _known: bool, _point: &Vector3) -> bool { false }
        fn agent_accepts_hazard(&mut self, _agent: super::super::Obj) -> bool { false }
        fn agent_accepts_acoustic(&mut self, _agent: super::super::Obj, _source: super::super::Obj) -> bool { false }
        fn shared_target(&mut self, _controller: super::super::Obj) -> Option<super::super::Obj> { None }
        fn has_vehicle_dispatch(&self) -> bool { false }
        fn vehicle_dispatch(&mut self, _shared: &mut super::super::super::weapons_ai::Shared, _controller: super::super::Obj, _unit: super::super::Obj, _target: super::super::Obj, _switch_allowed: bool) -> super::super::super::world::Engagement { Default::default() }
        fn has_mount_reach(&self) -> bool { false }
        fn mount_reach(&mut self, _unit: super::super::Obj, _point: &Vector3) -> bool { false }
        fn current_controllable(&mut self, _character: super::super::Obj) -> (bool, Option<super::super::Obj>) { (false, None) }
        fn character_remote(&self, _character: super::super::Obj) -> Option<super::super::Obj> { None }
        fn character_unit(&self, _character: super::super::Obj) -> Option<super::super::Obj> { None }
        fn is_unit(&self, _o: super::super::Obj) -> bool { true }
        // The world's patterns are the mounted ones; a soldier's jet never reaches them.
        fn enter_pattern(&mut self, _agent: super::super::Obj, state: f64) { assert_ne!(state, super::super::TARGET_JET); }
        fn exit_pattern(&mut self, _agent: super::super::Obj, state: f64) { assert_ne!(state, super::super::TARGET_JET); }
        fn tick_pattern(&mut self, _agent: super::super::Obj, state: f64, _dt: f64) { assert_ne!(state, super::super::TARGET_JET); }
        fn sidestep(&mut self, _agent: super::super::Obj) {}
        fn cover_hints(&mut self, _center: &Vector3, _half: f64) -> crate::ai::owned::scratch::List<super::super::super::world::HintView> { unimplemented!() }
        fn has_jet_hints(&self) -> bool { false }
        fn jet_hints_excluded(&mut self, _controller: super::super::Obj) -> bool { false }
        fn jet_hints_find(&mut self, _controller: super::super::Obj) -> Option<super::super::Obj> { None }
        fn jet_hints_enter(&mut self, _controller: super::super::Obj, _hint: Option<super::super::Obj>) {}
        fn jet_hints_exit(&mut self, _controller: super::super::Obj, _hint: Option<super::super::Obj>) {}
        fn jet_hints_tick(&mut self, _controller: super::super::Obj, _hint: Option<super::super::Obj>, _dt: f64) -> bool { true }
        fn nav_point_clear(&mut self, _point: &Vector3, _unit: super::super::Obj) -> Option<bool> { None }
        fn nav_segment_blocked(&mut self, _from: &Vector3, _to: &Vector3, _unit: super::super::Obj) -> Option<bool> { Some(self.blocked) }
        fn nav_direct_clear(&mut self, _unit: super::super::Obj, _point: &Vector3) -> Option<bool> { Some(true) }
        fn nav_in_danger_region(&mut self, _unit: super::super::Obj) -> Option<bool> { None }
        fn agent_sync(&mut self, _agent: &CombatAgent) {}
        fn agent_resync(&mut self, _agent: &mut CombatAgent) {}
        fn interrupts_tick(&mut self, _agent: super::super::Obj, _combat: &mut CombatAi) -> bool { false }
        fn agent_board_policy(&mut self, _agent: super::super::Obj, _candidate: super::super::Obj) -> bool { false }
        fn has_helper(&self, _agent: super::super::Obj) -> bool { self.unreachable }
        fn helper_arrived(&mut self, _agent: super::super::Obj) -> bool { self.unreachable }
        fn helper_flying(&mut self, _agent: super::super::Obj) -> bool { false }
        fn helper_has_follow(&self, _agent: super::super::Obj) -> bool { false }
        fn helper_goto(&mut self, _agent: super::super::Obj, _point: &Vector3, mode: Option<GotoMode>) {
            self.gotos.push((self.now, mode.is_none()));
        }
        fn saber_walk_clear(&mut self, _unit: super::super::Obj, _to: &Vector3) -> Option<bool> { Some(!self.unreachable) }
        fn helper_stop(&mut self, _agent: super::super::Obj) {}
        fn helper_tick(&mut self, _agent: super::super::Obj, _dt: f64) {}
        fn helper_follow(&mut self, _agent: super::super::Obj, _target: super::super::Obj) {}
        fn helper_terminate(&mut self, _agent: super::super::Obj) {}
        fn request_jet(&mut self, _controller: super::super::Obj, vector: &Vector3) {
            self.jets.push((self.now, *vector));
        }
        fn end_jet(&mut self, _controller: super::super::Obj) {
            self.ends.push(self.now);
        }
        fn steer_jet(&mut self, _controller: super::super::Obj, point: Option<&Vector3>) {
            self.steers.push(point.copied());
        }
        fn emit_vo(&mut self, _unit: super::super::Obj, _event: &str, _property: &str) {}
    }

    impl SaberWorld for LeapTestWorld {
        fn fighter(&mut self, _unit: super::super::Obj) -> Option<&mut approach::Fighter> { None }
        fn order_failed(&self, _agent: super::super::Obj) -> bool { self.unreachable }
    }

    fn leap_world(uniform_a: f64, leap_clear: bool) -> (LeapTestWorld, CombatAgent, CombatAi) {
        use crate::combat::defs::{Js, JsObj};
        use std::rc::Rc;
        let blade_def = Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str("melee".into())),
                ("MinRange".into(), Js::Str("0".into())),
                ("MaxRange".into(), Js::Str("4".into())),
            ],
            def: None,
        });
        let w = LeapTestWorld {
            now: 1.0,
            uniform_a,
            leap_clear,
            shooter: super::super::super::world::UnitView {
                obj: 1,
                alive: true,
                team: 1.0,
                kind: 0.0,
                generation: 1.0,
                position: Vector3 { x: 0.0, y: 0.0, z: 0.0 },
                class: Some(super::super::super::world::ClassView {
                    fields: Rc::new(super::super::super::world::ClassFields {
                        forward_speed: Js::Num(7.0),
                        max_speed: Js::Num(7.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            target: super::super::super::world::UnitView {
                obj: 2,
                alive: true,
                team: 2.0,
                kind: 0.0,
                generation: 1.0,
                combat_max_health: Js::Num(100.0),
                position: Vector3 { x: 10.0, y: 0.0, z: 0.0 },
                ..Default::default()
            },
            blade: super::super::super::world::WeaponView {
                def: blade_def,
                bottomless: true,
                rounds: Js::Num(1.0),
                ..Default::default()
            },
            ctl: super::super::super::world::Ctl {
                obj: 10,
                unit: Some(1),
                generation: 1.0,
                perception: Some(super::super::super::perception::Perception {
                    candidate: Some(super::super::super::world::Ref { obj: 2, generation: 1.0 }),
                    candidate_point: Vector3 { x: 10.0, y: 0.0, z: 0.0 },
                    ..Default::default()
                }),
                ..Default::default()
            },
            jets: Vec::new(),
            ends: Vec::new(),
            steers: Vec::new(),
            unreachable: false,
            gotos: Vec::new(),
            blocked: false,
        };
        let mut tracker = super::super::super::perception::TargetTracker::new(10);
        tracker.target = Some(super::super::super::world::Ref { obj: 2, generation: 1.0 });
        tracker.fresh = true;
        let mut agent = CombatAgent::new(&w, 100, 10, tracker, super::super::Goal::default());
        agent.state = 4.0;
        agent.pattern = 6.0;
        let combat = CombatAi::new(Default::default());
        (w, agent, combat)
    }

    /// A hover jetpack with fuel 0.9, 0.1 a second of flight, 0.2 at ignition, top speed 6 m/s:
    /// seven seconds of flight, 42 m at top speed.
    fn hover(w: &mut LeapTestWorld, fuel: f64, target_x: f64) {
        use crate::combat::defs::Js;
        w.shooter.soldier = Some(super::super::super::world::SoldierView {
            jet: true,
            jet_hover: true,
            jet_jump: Js::Num(3.0),
            fuel: Js::Num(fuel),
            jet_cost: 0.1,
            jet_initial: 0.2,
            max_speed: Js::Num(6.0),
            ..Default::default()
        });
        w.target.position = Vector3 { x: target_x, y: 0.0, z: 0.0 };
    }

    #[test]
    fn a_hover_jet_soldier_jets_at_a_target_twenty_metres_off_when_its_fuel_lasts_the_flight() {
        let (mut w, mut agent, mut combat) = leap_world(0.0, false);
        agent.patterns.melee_leap_at = 100.0;
        hover(&mut w, 0.9, 25.0);
        assert!(!agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave), "the pattern machine takes the jet");
        assert_eq!((agent.patterns.pending, agent.patterns.prior), (super::super::TARGET_JET, 6.0));
        // Take-off: the direction to the target lifted by one, times 0.7071; then the flight.
        agent.patterns.state = super::super::TARGET_JET;
        agent.patterns_enter(&mut w, &mut combat);
        let (_, launch) = w.jets[0];
        assert_eq!(w.steers.last().copied().flatten().map(|p| (p.x, p.z)), Some((25.0, 0.0)), "the jet steers at the target");
        assert!((launch.x - 0.707_106_77).abs() < 1e-6 && (launch.y - 0.707_106_77).abs() < 1e-6 && launch.z.abs() < 1e-9);
        agent.patterns.pending = 0.0;
        agent.target_jet_tick(&mut w);
        assert_eq!(agent.patterns.pending, 0.0, "still 25 m off: it flies on");
        w.target.position.x = 2.5;
        agent.patterns_tick(&mut w, &mut combat, 1.0 / 60.0);
        assert_eq!(agent.patterns.pending, 6.0, "inside 3 m it ends and the melee pattern resumes");
        assert!(w.ends.is_empty(), "no end request while the pattern runs");
        agent.patterns_exit(&mut w, &mut combat);
        assert_eq!(w.ends.len(), 1, "leaving the jet pattern asks the controller to end the jet");
        assert_eq!(w.steers.last().copied(), Some(None), "and stops steering it");
        assert_eq!(w.jets.len(), 1, "one request: the controller carries the sequence on");
    }

    #[test]
    fn no_jet_at_a_near_target_without_a_hover_jetpack_short_of_fuel_or_across_a_barrier() {
        for (case, fuel, x, hovers, blocked) in [("near", 0.9, 15.0, true, false), ("no hover", 0.9, 25.0, false, false),
            ("short of fuel", 0.5, 25.0, true, false), ("barrier", 0.9, 25.0, true, true)] {
            let (mut w, mut agent, mut combat) = leap_world(0.0, false);
            agent.patterns.melee_leap_at = 100.0;
            hover(&mut w, fuel, x);
            w.shooter.soldier.as_mut().unwrap().jet_hover = hovers;
            w.blocked = blocked;
            assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave), "{case}: the chase goes on");
            assert_ne!(agent.patterns.pending, super::super::TARGET_JET, "{case}");
        }
        // The ranged patterns ask the same question and come back to themselves.
        let (mut w, mut agent, _) = leap_world(0.0, false);
        hover(&mut w, 0.9, 30.0);
        assert!(agent.try_target_jet(&mut w, 2.0));
        assert_eq!((agent.patterns.pending, agent.patterns.prior), (super::super::TARGET_JET, 2.0));
    }

    #[test]
    fn a_saber_chase_keeps_a_target_no_route_reaches_and_routes_again_every_two_seconds() {
        let (mut w, mut agent, mut combat) = leap_world(0.0, false);
        w.unreachable = true;
        agent.patterns.melee_leap_at = 100.0;
        for t in [1.0, 1.5, 2.4, 2.99, 3.05, 4.0, 5.1] {
            w.now = t;
            assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave));
            assert_ne!(agent.pending, super::super::DONE, "the chase gave up its target at {t}");
            assert!(agent.tracker.target.is_some(), "the target was dropped at {t}");
        }
        assert_eq!(w.gotos, vec![(1.0, true), (3.05, true), (5.1, true)], "one routed walk every 2 s");
    }

    #[test]
    fn melee_leap_timer_spaces_leaps_by_five_to_ten_seconds_and_retries_blocked() {
        // At minimum uniform draw (0.0), consecutive leaps are spaced by LEAP_MIN_DELAY (5.0 s).
        let (mut w, mut agent, mut combat) = leap_world(0.0, true);
        // First tick initializes melee_leap_at = 1.0 + 0.0 = 1.0; next tick at 1.1 triggers the opening leap.
        assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave));
        assert!(w.jets.is_empty());
        w.now = 1.1;
        assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w.jets.len(), 1);
        // Still waiting at +4.5 s, leaps again past +5.0 s.
        w.now = 5.6;
        assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w.jets.len(), 1, "should not leap before LEAP_MIN_DELAY (5.0 s)");
        w.now = 6.2;
        assert!(agent.saber_engagement(&mut w, &mut combat, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w.jets.len(), 2, "should leap once past LEAP_MIN_DELAY (5.0 s)");

        // At maximum uniform draw (~1.0), consecutive leaps wait LEAP_MAX_DELAY (10.0 s).
        let (mut w_max, mut agent_max, mut combat_max) = leap_world(0.999, true);
        agent_max.patterns.melee_leap_at = 0.5;
        w_max.now = 1.0;
        assert!(agent_max.saber_engagement(&mut w_max, &mut combat_max, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w_max.jets.len(), 1);
        w_max.now = 10.5;
        assert!(agent_max.saber_engagement(&mut w_max, &mut combat_max, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w_max.jets.len(), 1, "should not leap before LEAP_MAX_DELAY (10.0 s)");
        w_max.now = 11.1;
        assert!(agent_max.saber_engagement(&mut w_max, &mut combat_max, 1.0 / 60.0, Phase::Behave));
        assert_eq!(w_max.jets.len(), 2, "should leap once past LEAP_MAX_DELAY (10.0 s)");

        // Blocked leap probe skips jumping and retries after LEAP_BLOCKED_RETRY (4.0 s).
        let (mut w_blk, mut agent_blk, mut combat_blk) = leap_world(0.0, false);
        agent_blk.patterns.melee_leap_at = 0.5;
        w_blk.now = 1.0;
        assert!(agent_blk.saber_engagement(&mut w_blk, &mut combat_blk, 1.0 / 60.0, Phase::Behave));
        assert!(w_blk.jets.is_empty());
        assert!((agent_blk.patterns.melee_leap_at - 5.0).abs() < 1e-4);
    }
}
