//! The combat services the control core calls (src/ai/classic/index.js
//! combatAdapter with combat.js classicBinding, and the Enhanced aim's
//! replacements, src/ai/enhanced/aim.js enhancedCombat): one [`CombatAi`]
//! per battle, holding what the JavaScript keeps per battle or per process
//! (the deferred line-of-sight queue, the weapons' AI records, the cluster
//! timestamps, the cover reservations, the aim hook's flag, the Enhanced
//! aim's generator and skills).
//!
//! Every service takes the battle as a [`World`]; controllers and agents
//! are named by the control core's tokens.

use three::Vector3;

use super::agent::{can_engage, Reservations};
use super::aim::{aim_point_for, lead_point, resolve, skill_curves};
use super::enhanced::EnhancedCombat;
use super::perception::{
    acoustic_response, alert_response, broadcast_acoustic, clear_candidate, hazard_response, notify_damage, notify_danger_with, notify_targeted_shot,
    scan, sight_range, thin, tracker_update, Attacker, DangerState, DeferredLos, Packet, Response, TargetTracker,
};
use super::weapons_ai::{consume_fire, def_at, rebuild_protected_humans, selected_weapon, sensor_range, Shared};
use super::world::{Curves, Obj, Ref, World};

/// The combat services' battle state.
#[derive(Clone, Debug, Default)]
pub struct CombatAi {
    pub shared: Shared,
    /// classic/index.js:131: the global deferred line-of-sight queue.
    pub los: DeferredLos,
    pub reservations: Reservations,
    /// combat.js:1408 `aimHooked`: the aim runs from the unit's tick.
    pub aim_hooked: bool,
    /// The Enhanced AI's aim (None: Classic).
    pub enhanced: Option<EnhancedCombat>,
}

/// An event's reference to a unit (src/sim-events.js records hold
/// `{ unit, generation }` refs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventRef {
    None,
    Ref(Ref),
    Unit(Obj),
}

/// The journal's events the combat services read (combat.js:1503
/// ingestEvent): a shot, or damage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Shot { source: EventRef, target: EventRef },
    Damage { target: EventRef, attacker: EventRef, attacker_character: Option<Obj>, amount: Option<f64>, generic: Option<f64>, typed: Option<f64>, known: bool, point: Option<Vector3> },
}

/// combat.js:1525 `eventUnit(ref)`.
fn event_unit<W: World + ?Sized>(w: &W, r: EventRef) -> Option<Obj> {
    match r {
        EventRef::None => None,
        EventRef::Ref(r) => resolve(w, Some(r)),
        EventRef::Unit(u) => w.unit_scalars(u).alive.then_some(u),
    }
}

impl CombatAi {
    /// combat.js:1462: the sides' ai.json weapon records registered.
    pub fn new(enhanced: Option<EnhancedCombat>) -> CombatAi {
        CombatAi { enhanced, ..CombatAi::default() }
    }

    // -- Lifetime (controller bind and unbind).

    /// combat.js:1466 `createPerception(controller)`: a fresh perception.
    pub fn create_perception<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) {
        let p = w.fresh_perception(controller);
        w.ctl_mut(controller).perception = Some(p);
    }

    /// combat.js:1471 `attachController(controller)`: the bot's weapons
    /// draw their spread from generator A.
    pub fn attach_controller<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) {
        if let Some(u) = w.ctl(controller).unit {
            if w.has_arsenal(u) {
                w.use_classic_random(u);
            }
        }
    }

    /// combat.js:1475 `detachController(controller)`.
    pub fn detach_controller<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) {
        clear_candidate(w, controller);
    }

    /// classic/index.js:142 `initAgent(agent)`: every agent's tracker.
    pub fn init_agent(&mut self, controller: Obj) -> TargetTracker {
        TargetTracker::new(controller)
    }

    // -- The scheduler's services.

    /// combat.js:1479 `rebuildProtected()` (at a cycle wrap).
    pub fn rebuild_protected<W: World + ?Sized>(&mut self, w: &mut W) {
        rebuild_protected_humans(w);
    }

    /// classic/index.js:147 `scan(controller)`.
    pub fn scan<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) -> f64 {
        scan(w, self, controller)
    }

    /// classic/index.js:148 `dropLosRequests(controller)`.
    pub fn drop_los_requests<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) {
        self.los.drain(w, controller);
    }

    /// classic/index.js:149 `serviceLos(world, budget)`.
    pub fn service_los<W: World + ?Sized>(&mut self, w: &mut W, budget: f64) -> bool {
        self.los.service(w,&mut self.shared,budget)
    }

    /// classic/index.js:150 `broadcastAcoustic(unit)`.
    pub fn broadcast_acoustic<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj) -> f64 {
        broadcast_acoustic(w, unit)
    }

    /// classic/index.js:151 `clearCandidate(controller)`.
    pub fn clear_candidate<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) {
        clear_candidate(w, controller);
    }

    // -- The agents' services.

    /// classic/index.js:152 `rankTargets(agent)`.
    pub fn rank_targets<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) -> f64 {
        let controller = w.agent_controller(agent);
        let retained = w.tracker(agent).and_then(|t| t.target);
        thin(w, &mut self.los, controller, retained)
    }

    /// classic/index.js:153 `updateTarget(agent)`.
    pub fn update_target<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) {
        if w.tracker(agent).is_some() {
            tracker_update(w, self, agent);
        }
    }

    /// classic/index.js:154 `updateTargetSelection(agent)`.
    pub fn update_target_selection<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) {
        let Some(mut t) = w.tracker(agent).copied() else { return };
        t.select(w);
        *w.tracker(agent).unwrap() = t;
        t.assign_weapon_target(w, &self.shared.weapons);
    }

    /// classic/index.js:160 `gateFire(agent)`.
    pub fn gate_fire<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) {
        if let Some(t) = w.tracker(agent).copied() {
            t.fire_controls(w, &self.shared.weapons);
        }
    }

    /// classic/index.js:161 `currentTarget(agent)`.
    pub fn current_target<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) -> Option<Obj> {
        let t = *w.tracker(agent)?;
        t.fresh_target(w)
    }

    /// classic/index.js:162 `memoryPoint(agent)`.
    pub fn memory_point<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) -> Option<Vector3> {
        let t = *w.tracker(agent)?;
        t.target(w).map(|_| t.point)
    }

    /// classic/index.js:163 `acquiredTarget(agent)`.
    pub fn acquired_target<W: World + ?Sized>(&mut self, w: &mut W, agent: Obj) -> Option<Obj> {
        let controller = w.agent_controller(agent);
        let c = w.ctl(controller).perception.as_ref().and_then(|p| p.candidate);
        resolve(w, c)
    }

    /// classic/index.js:136 `canEngage(controller, target)`.
    pub fn can_engage<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, target: Option<Obj>) -> bool {
        can_engage(w, self, controller, target)
    }

    /// classic/index.js:165 `hasWeapon(controller)`.
    pub fn has_weapon<W: World + ?Sized>(&mut self, w: &W, controller: Obj) -> bool {
        match w.ctl(controller).unit {
            Some(u) => selected_weapon(w, u, Some(controller)).is_some(),
            None => false,
        }
    }

    /// classic/index.js:166 `sensorRange(controller, target)`: the sight
    /// range to it.
    pub fn sensor_range<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, target: Obj) -> f64 {
        let unit = w.ctl(controller).unit.expect("sensorRange: the controller's unit");
        sight_range(w, unit, target)
    }

    /// classic/index.js:167 `acousticRange(controller)`.
    pub fn acoustic_range<W: World + ?Sized>(&mut self, w: &W, controller: Obj) -> f64 {
        let unit = w.ctl(controller).unit.expect("acousticRange: the controller's unit");
        sensor_range(w, &self.shared.weapons, unit, controller)
    }

    /// combat.js:1494 `consumeFire(controller, unit, step, intent)` (the
    /// Enhanced one, src/ai/enhanced/aim.js:225, when Enhanced): the fire
    /// buttons, and the aim while the unit's tick does not run it yet.
    pub fn consume_fire<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, unit: Obj, step: f64, intent: bool) {
        if let Some(mut e) = self.enhanced.take() {
            e.consume_fire(w, self, controller, unit, step, intent);
            self.enhanced = Some(e);
            return;
        }
        consume_fire(w, &mut self.shared, controller, Some(unit));
        if !self.aim_hooked && intent {
            let aim = self.update_aim(w, controller, step);
            w.set_intent_aim(unit, aim);
        }
    }

    /// combat.js:1490 `aimPoint(controller, unit, dt, out)`.
    pub fn aim_point<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, unit: Obj, dt: f64) -> Option<Vector3> {
        aim_point_for(w, controller, unit, dt)
    }

    /// combat.js:1482 `leadPoint(controller, target, dt, out)`.
    pub fn lead_point<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, target: Obj, dt: f64) -> Option<Vector3> {
        let unit = w.ctl(controller).unit?;
        let i = selected_weapon(w, unit, Some(controller))?;
        let def = def_at(w, unit, i);
        Some(lead_point(w, dt, unit, target, Some(&def)).0)
    }

    /// combat.js:1503 `aim(controller, dt)` (the Enhanced one when Enhanced):
    /// the filtered aim into the unit's intent.
    pub fn aim<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, dt: f64) -> Option<Vector3> {
        if let Some(mut e) = self.enhanced.take() {
            let r = e.aim(w, self, controller, dt);
            self.enhanced = Some(e);
            return r;
        }
        self.aim_hooked = true;
        let direction = self.update_aim(w, controller, dt);
        if let Some(u) = w.ctl(controller).unit {
            if w.unit_scalars(u).intent {
                w.set_intent_aim(u, direction);
            }
        }
        direction
    }

    /// combat.js:1509 `updateAim(controller, dt)`.
    pub fn update_aim<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, dt: f64) -> Option<Vector3> {
        let unit = w.ctl(controller).unit?;
        if !w.unit_scalars(unit).alive || w.ctl(controller).perception.is_none() {
            return None;
        }
        let desired = w.forward(unit);
        let curves = curves_of(w, controller, unit);
        let weapon = selected_weapon(w, unit, Some(controller));
        let def = weapon.map(|i| def_at(w, unit, i));
        let hero = w.unit_scalars(unit).hero;
        let p = w.ctl_mut(controller).perception.as_mut().unwrap();
        let (direction, scales) = p.aimer.update(&desired, dt, curves.as_ref(), def.as_deref(), hero);
        if let (Some(i), Some(s)) = (weapon, scales) {
            w.edit_weapon(unit,i,|wv| {
                wv.spread_scale = s.spread_scale;
                wv.sniper_scale = Some(s.sniper_scale);
            });
        }
        Some(direction)
    }

    /// combat.js:1494 `markSkip`: nothing.
    pub fn mark_skip(&mut self) {}

    /// combat.js:1503 `ingest()`: the step's shot and damage events become
    /// controller alerts.
    pub fn ingest<W: World + ?Sized>(&mut self, w: &mut W, events: &[Event]) {
        for e in events {
            match *e {
                Event::Shot { source, target } => {
                    let shooter = event_unit(w, source);
                    let controller = shooter.and_then(|s| w.unit_controller(s));
                    let target = event_unit(w, target).or_else(|| {
                        let c = controller?;
                        let ctl = w.ctl(c);
                        if ctl.perception.is_some() {
                            resolve(w, ctl.target)
                        } else {
                            None
                        }
                    });
                    if let (Some(shooter), Some(target)) = (shooter, target) {
                        let at = w.unit_scalars(shooter).position;
                        notify_targeted_shot(w, shooter, target, &at);
                    }
                }
                Event::Damage { target, attacker, attacker_character, amount, generic, typed, known, point } => {
                    let victim = event_unit(w, target);
                    let attacker = event_unit(w, attacker);
                    let attacker = if let Some(a) = attacker {
                        let character = w.unit(a).character;
                        Some(Attacker { obj: a, character, team: Some(w.unit_scalars(a).team), character_team: character.map(|c| w.character_team(c)) })
                    } else {
                        attacker_character.map(|c| Attacker { obj: c, character: None, team: Some(w.character_team(c)), character_team: None })
                    };
                    if let (Some(victim), Some(attacker)) = (victim, attacker) {
                        let packet = Packet { amount, generic: Some(generic.unwrap_or(1.0)), typed: Some(typed.unwrap_or(1.0)) };
                        notify_damage(w, victim, attacker, packet, known, point);
                    }
                }
            }
        }
    }

    /// combat.js:1527 `grenadeUpdate(round, position, owner, firstContact)`:
    /// the danger broadcast of one live grenade or sticky charge; `state` is
    /// the round's (`round.aiDanger`, made at its first call).
    pub fn grenade_update<W: World + ?Sized>(&mut self, w: &mut W, state: &mut DangerState, position: &Vector3, owner: Option<Obj>, first_contact: f64) -> f64 {
        self.grenade_update_with(w,state,position,owner,first_contact,&mut |_,_|{})
    }
    /// Notify the active Core before a later aim/control feed can overwrite
    /// its pending danger with an older controller observation.
    pub fn grenade_update_with<W:World+?Sized>(&mut self,w:&mut W,state:&mut DangerState,position:&Vector3,owner:Option<Obj>,first_contact:f64,notify:&mut impl FnMut(&mut W,Obj))->f64 {
        if first_contact < state.first_contact {
            state.first_contact = first_contact;
        }
        let d = w.difficulty();
        let skill = if d.present { d.enemy_skill.unwrap_or(14.0) } else { 14.0 };
        notify_danger_with(w, state, position, owner, skill, &mut |w: &mut W, u| w.emit_vo(u, "flee", "FleeSound"),notify)
    }

    // -- The interrupt helper's responses.

    /// classic/index.js:170 `hazardResponse(controller, agent)`.
    pub fn hazard_response<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, agent: Option<Obj>) -> Option<Response> {
        hazard_response(w, controller, agent)
    }

    /// classic/index.js:171 `alertResponse(controller, agent)`.
    pub fn alert_response<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, agent: Option<Obj>) -> Option<Response> {
        alert_response(w, self, controller, agent)
    }

    /// classic/index.js:172 `acousticResponse(controller, agent)`.
    pub fn acoustic_response<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj, agent: Option<Obj>) -> Option<Response> {
        acoustic_response(w, self, controller, agent)
    }
}

/// combat.js:1494 `curvesOf(controller, unit, ai)`: the controller's
/// curves, else its team's from the difficulty.
pub fn curves_of<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj) -> Option<Curves> {
    let d = w.ctl(controller).difficulty;
    if let Some(c) = d {
        if c.a.is_finite() {
            return Some(c);
        }
    }
    let team = w.unit_scalars(unit).team;
    w.curves_for(team)
}

/// Medium's curves (aim.js:243), for a battle without a difficulty.
pub fn fallback_curves() -> Curves {
    skill_curves(14.0)
}
