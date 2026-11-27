//! Exercise the native rules slot with synthetic battle content.

use super::*;
use crate::battle::rules::{
    self,
    events::{Event, EventDriven, EventQueue, EventRules, ObjectHandle, RuleActions},
    END_SEQUENCE, GAME_OVER_DELAY,
};
use crate::store::Ref;
use bfe_family::GameFamily;

fn world() -> Box<Root> {
    let mut w = Root::new();
    let positions = w.collision.positions_new(0, 0);
    let tree = w.collision.tree_new(positions, 0, 0);
    let collision = w.collision.world_new(tree, -1, -1);
    w.fleet.collision = Some(crate::vehicles::native::Collision::new(collision as usize, None));
    f3::f3_init_in(&mut w, collision, f64::NAN, 0);
    w.sim.calls.result(2, |row, _| row.copy_from_slice(&[7.0, 0.0]));
    crate::combat::module::cmb_class_in(&mut w, 0);
    w.sim.calls.result(F3UnitItems::WORDS, |r, h| F3UnitItems::default().put(r, h));
    crate::units::f3_items::f3_unit_items_in(&mut w);
    w.sim.store.battle = rules::new_battle(Default::default(), 1.0, 2.0);
    w
}

fn soldier(w: &mut Root, team: f64) -> (Ref<Unit>, Ref<Character>) {
    let s = &mut w.sim.store;
    let character = s.character.insert(Character {
        index: s.battle.characters.len() as f64,
        team,
        ..Default::default()
    });
    let unit = s.unit.insert(Unit {
        alive: true, team, character: Some(character),
        soldier: UnitBody::Soldier(crate::units::class::soldier_body(&bfe_content::Value::Null, 100., 0., 0.)),
        combat: Combat { health: 100.0, max_health: 100.0, team, ..Default::default() },
        ..Default::default()
    });
    s.character.get_mut(character).unwrap().unit = Some(unit);
    s.battle.characters.push(character);
    s.units.list.push(Member::Unit(unit));
    let key = F3UnitKey::of(unit, s.unit.get(unit).unwrap());
    F3Adoption { key, ..Default::default() }.put(f3::command_row_in(&mut w.units), &mut Vec::new());
    assert_eq!(f3::f3_adopt_in(w), 1);
    (unit, character)
}

fn prepare_tail(w: &mut Root) {
    prepare_tail_with_boundary(w, BfConfig::default());
}

fn prepare_tail_with_boundary(w: &mut Root, boundary: BfConfig) {
    w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| F4LiveSetup::default().put(row, heap));
    super::super::f4_live::f4_live_init_in(w);
    w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| F4BattleSetup::default().put(row, heap));
    super::super::f4_core::f4_battle_init_in(w);
    let setup = F4TailSetup {
        boundary,
        registry_capacity: w.sim.store.units.list.len() as u32,
        ..Default::default()
    };
    w.sim.calls.result(F4TailSetup::WORDS, |row, heap| setup.put(row, heap));
    f4_tail_init_in(w);
    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass { captures: true, ..Default::default() }.put(row, heap);
    });
    f4_tail_unit_class_in(w);
}

fn tick(w: &mut Root, dt: f64) {
    let input = F4TailInput { dt, server: true, ..Default::default() };
    let bank = super::super::f4_live::tail_input_bank_mut_in(w).unwrap();
    input.put(bank, &mut Vec::new());
    w.tick.running = true;
    f4_tail_in(w);
    w.tick.running = false;
}

fn damage(w: &mut Root, victim: Ref<Unit>, attacker: Attacker) {
    f3::with_services_in(w, |w, services| {
        crate::combat::live::damage_in(w, victim, 1000.0, attacker, services.unwrap());
    });
}

struct TeamDeathmatch {
    score_limit: u32,
}

impl EventRules for TeamDeathmatch {
    fn tick(&mut self, _: f64, events: &EventQueue, actions: &mut dyn RuleActions) {
        assert!(!events.overflowed());
        for event in events.events() {
            let Event::UnitKilled { killer: Some(_), killer_team: Some(team), relation, .. } = *event else {
                continue;
            };
            if relation < 0.0 {
                let score = actions.score(team).unwrap() + 1;
                assert!(actions.set_score(team, score));
                if score == self.score_limit {
                    assert!(actions.end_battle(team));
                    return;
                }
            }
        }
    }
}

#[test]
fn team_deathmatch_wins_by_enemy_kills_without_posts() {
    let mut w = world();
    let (shooter, _) = soldier(&mut w, 1.0);
    let (friendly, _) = soldier(&mut w, 1.0);
    let (enemy_one, _) = soldier(&mut w, 2.0);
    let (enemy_two, _) = soldier(&mut w, 2.0);
    let (environment, _) = soldier(&mut w, 2.0);
    prepare_tail(&mut w);
    let params = rules::ModeParameters { score_limit: Some(2), command_limit: 4, ..Default::default() };
    assert!(select_rules(&mut w, GameFamily::Swbf1, "conquest", Default::default(), Vec::new()).is_ok());
    assert!(select_rules(&mut w, GameFamily::Swbf2, "conquest", params, Vec::new()).is_err());
    assert!(select_rules(&mut w, GameFamily::Swbf1, "deathmatch", params, Vec::new()).is_err());
    w.register_rule_factory(|family, mode, parameters| {
        assert_eq!((family, mode), (GameFamily::Swbf2, "deathmatch"));
        Ok(Box::new(TeamDeathmatch { score_limit: parameters.score_limit.unwrap() }))
    });
    select_rules(&mut w, GameFamily::Swbf2, "deathmatch", params, Vec::new()).unwrap();

    damage(&mut w, enemy_one, Attacker::Unit(shooter));
    damage(&mut w, friendly, Attacker::Unit(shooter));
    damage(&mut w, environment, Attacker::Null);
    tick(&mut w, 1.0);
    assert!(!w.sim.store.battle.over);
    tick(&mut w, 1.0);
    assert!(!w.sim.store.battle.over);

    damage(&mut w, enemy_two, Attacker::Unit(shooter));
    assert!(!w.sim.store.battle.over);
    tick(&mut w, 1.0);
    assert!(w.sim.store.command_posts.posts.is_empty());
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
    assert!(w.sim.store.battle.over);
    assert!(!w.sim.store.battle.game_over);

    tick(&mut w, GAME_OVER_DELAY);
    assert!(w.sim.store.battle.game_over);
    assert!(!w.sim.store.battle.finished);
    tick(&mut w, END_SEQUENCE);
    assert!(w.sim.store.battle.finished);
}

struct ObjectiveProbe {
    post: Ref<Post>,
    object: F4Handle,
    timer: StrId,
}

impl EventRules for ObjectiveProbe {
    fn tick(&mut self, _: f64, events: &EventQueue, actions: &mut dyn RuleActions) {
        assert!(!events.overflowed());
        if actions.score(1.0) == Some(0) {
            assert_eq!(events.events(), [Event::PostCaptured { post: self.post, team: 1.0 }]);
            let stale = F4Handle { life: self.object.life + 1.0, ..self.object.clone() };
            assert!(!actions.set_building_health(&stale, 0.0));
            assert!(actions.set_building_health(&self.object, 0.0));
            assert!(actions.start_timer(self.timer, 10.0));
            assert!(actions.start_timer(self.timer, 0.0));
            assert!(actions.set_score(1.0, 1));
        } else {
            assert_eq!(events.events(), [
                Event::ObjectKilled { object: ObjectHandle::Prop(self.object.clone()), killer: None },
                Event::TeamPointsChanged { team: 1.0, points: 1 },
                Event::TimerElapsed { timer: self.timer },
            ]);
            assert!(actions.end_battle(1.0));
        }
    }
}

fn prop(w: &mut Root, trap: bool) -> (Ref<Building>, F4Handle) {
    configured_prop(w, trap, |_| {})
}

fn configured_prop(w: &mut Root, trap: bool, configure: impl FnOnce(&mut F3BuildingSetup)) -> (Ref<Building>, F4Handle) {
    let building = Ref::<Building>::new(0, 1);
    let name = w.sim.strings.intern_str("test objective");
    let settings = PbSettings {
        max_health: 100.0,
        has_destroyed: true,
        matrix: three::Matrix4::default().elements,
        models: vec![None; 3],
        ..Default::default()
    };
    let mut object = F3BuildingSetup {
        target: RepairTarget::Building(building),
        key: name,
        settings: settings.clone(),
        trap: trap.then(|| F4TrapBirth {
            settings: TrSettings { max_health: 100.0, ..Default::default() },
            state: TrState { built: true, health: 100.0, ..Default::default() },
            ..Default::default()
        }),
        state: F3BuildingState { state: PbStateName::Built, health: 100.0, ..Default::default() },
        bodies: vec![None; 3],
        live: F4BuildingBirth {
            settings,
            state: PbStation {
                building: PbState { state: PbStateName::Built, health: 100.0, ..Default::default() },
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    configure(&mut object);
    w.sim.calls.result(F3BuildingSetup::WORDS, |row, heap| object.put(row, heap));
    crate::units::f3_props::f3_building_in(w);
    let object = crate::units::f3_props::owner_in(w, 0);
    (building, object)
}

#[test]
fn rules_see_capture_then_their_own_object_death_and_timer_on_the_next_tick() {
    let mut w = world();
    w.fleet.collision = Some(crate::vehicles::native::Collision::new(0, None));
    let (_, soldier) = soldier(&mut w, 1.0);
    let post = w.sim.store.post.insert(Post {
        active: true,
        capture_time: 1.0,
        neutralize_time: 1.0,
        capture: Some(Built { r#type: RegionType::Sphere, radius: 5.0, ..Default::default() }),
        ..Default::default()
    });
    w.sim.store.command_posts.posts.push(post);
    let (building, object) = prop(&mut w, false);
    let timer = w.sim.strings.intern_str("test timer");
    prepare_tail(&mut w);
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(ObjectiveProbe { post, object, timer }, 4, 1))).unwrap();

    tick(&mut w, 1.0);
    assert!(!w.sim.store.battle.over);
    assert_eq!(w.sim.store.character.get(soldier).unwrap().captures, 1.0);
    assert!(!crate::units::f3_props::battle_building_in(&mut w, building).0);

    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
}

#[test]
fn due_timer_survives_a_full_world_batch() {
    struct FinishOnTimer { tick: usize, timer: StrId }
    impl EventRules for FinishOnTimer {
        fn tick(&mut self, _: f64, events: &EventQueue, actions: &mut dyn RuleActions) {
            match self.tick {
                0 => assert!(actions.start_timer(self.timer, 0.0)),
                1 => {
                    assert!(events.overflowed());
                    assert!(matches!(events.events(), [Event::UnitKilled { .. }]));
                }
                2 => {
                    assert_eq!(events.events(), [Event::TimerElapsed { timer: self.timer }]);
                    assert!(actions.end_battle(1.0));
                }
                _ => panic!("timer did not end the battle"),
            }
            self.tick += 1;
        }
    }
    let mut w = world();
    let (victim, _) = soldier(&mut w, 2.0);
    prepare_tail(&mut w);
    let timer = w.sim.strings.intern_str("finish");
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(FinishOnTimer { tick: 0, timer }, 1, 1))).unwrap();
    let RuleSet::EventDriven(mode) = &mut w.battle.tail.as_mut().unwrap().owner.tail.rules else { unreachable!() };
    mode.force_overflow(w.battle.rule_events.as_mut().unwrap(), 1);
    tick(&mut w, 1.0);
    damage(&mut w, victim, Attacker::Null);
    tick(&mut w, 1.0);
    assert!(!w.sim.store.battle.over);
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
}

#[test]
fn admission_reserves_for_the_whole_registry() {
    let mut w = world();
    let (killer, _) = soldier(&mut w, 1.0);
    let victims = [0; 3].map(|_| soldier(&mut w, 2.0).0);
    prepare_tail(&mut w);
    struct Count;
    impl EventRules for Count {
        fn tick(&mut self, _: f64, batch: &EventQueue, actions: &mut dyn RuleActions) {
            assert_eq!(batch.events().len(), 3);
            assert!(!batch.overflowed());
            assert!(actions.end_battle(1.0));
        }
    }
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(Count, 1, 0))).unwrap();
    for victim in victims { damage(&mut w, victim, Attacker::Unit(killer)); }
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
}

use std::{cell::RefCell, rc::Rc};
struct Record(Rc<RefCell<Vec<Event>>>);
impl EventRules for Record {
    fn tick(&mut self, _: f64, events: &EventQueue, _: &mut dyn RuleActions) {
        self.0.borrow_mut().extend_from_slice(events.events());
    }
}

#[test]
fn placed_post_neutralisation_precedes_recapture() {
    let mut w = world();
    soldier(&mut w, 1.0);
    let post = w.sim.store.post.insert(Post {
        team: 2.0, active: true, capture_time: 1.0, neutralize_time: 1.0,
        capture: Some(Built { r#type: RegionType::Sphere, radius: 5.0, ..Default::default() }),
        ..Default::default()
    });
    w.sim.store.command_posts.posts.push(post);
    prepare_tail(&mut w);
    let events = Rc::new(RefCell::new(Vec::new()));
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(Record(events.clone()), 4, 0))).unwrap();
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.post.get(post).unwrap().team, 0.0);
    assert_eq!(events.borrow().len(), 1);
    tick(&mut w, 1.0);
    tick(&mut w, 1.0);
    assert_eq!(events.borrow().as_slice(), [
        Event::PostNeutralized { post, from: 2.0 },
        Event::PostCaptured { post, team: 1.0 },
    ]);
}

fn record(w: &mut Root) -> Rc<RefCell<Vec<Event>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    set_rules(w, RuleSet::EventDriven(EventDriven::new(Record(events.clone()), 8, 0))).unwrap();
    events
}

fn unit_death(w: &Root, unit: Ref<Unit>, victim: Ref<Character>, killer: Option<Ref<Character>>, team: Option<f64>, relation: f64) -> Event {
    let body = w.sim.store.unit.get(unit).unwrap();
    Event::UnitKilled {
        unit: F3UnitKey::of(unit, body), victim: Some(victim), victim_team: body.team, killer, killer_team: team, relation,
    }
}

fn prepare_blast(w: &mut Root) {
    let fields = ["Damage", "DamageRadiusInner", "DamageRadiusOuter"].map(|name| w.sim.strings.intern_str(name));
    w.sim.calls.result(11, |r, _| r.copy_from_slice(&[7., 3., fields[0].0 as f64, 4., 200., fields[1].0 as f64, 4., 4., fields[2].0 as f64, 4., 5.]));
    crate::combat::module::cmb_explosion_in(w, 0);
    w.sim.calls.result(F3BlastBinding::WORDS, |r,h| F3BlastBinding::default().put(r,h));
    f3::f3_blast_binding_in(w);
}

#[test]
fn native_blast_chain_reports_nested_victim_before_its_source() {
    let mut w = world();
    let (source, sc) = soldier(&mut w, 1.0);
    let (victim, vc) = soldier(&mut w, 2.0);
    let (shooter, kc) = soldier(&mut w, 2.0);
    // The launch owner survives a change of team. Leave its body outside the blast.
    w.sim.store.unit.get_mut(shooter).unwrap().removed = true;
    w.sim.store.character.get_mut(kc).unwrap().team = 1.0;
    let owner = w.sim.store.owner_snapshot.insert(OwnerSnapshot { character: Some(kc), team: 2.0, ..Default::default() });
    prepare_blast(&mut w);
    w.sim.calls.result(F3UnitItems::WORDS, |r,h| F3UnitItems { explosion: Some(0), ..Default::default() }.put(r,h));
    crate::units::f3_items::f3_unit_items_in(&mut w);
    let combat = &mut w.sim.store.unit.get_mut(source).unwrap().combat;
    combat.chunks = 1.0;
    combat.blow_apart = true;
    prepare_tail(&mut w);
    let events = record(&mut w);
    damage(&mut w, source, Attacker::OwnerSnapshot(owner));
    damage(&mut w, source, Attacker::Null);
    damage(&mut w, victim, Attacker::Null);
    tick(&mut w, 1.0);
    assert_eq!(events.borrow().as_slice(), [
        unit_death(&w, victim, vc, Some(sc), Some(1.0), -1.0),
        unit_death(&w, source, sc, Some(kc), Some(2.0), -1.0),
    ]);
}

#[test]
fn own_blast_and_world_damage_each_deliver_one_death() {
    let mut w = world();
    let (source, sc) = soldier(&mut w, 1.0);
    let (other, oc) = soldier(&mut w, 2.0);
    w.sim.store.unit.get_mut(other).unwrap().removed = true;
    prepare_blast(&mut w);
    w.sim.calls.result(F3ImpactRules::WORDS, |r,h| F3ImpactRules { friendly_fire: true }.put(r,h));
    f3::f3_impact_rules_in(&mut w);
    prepare_tail(&mut w);
    let events = record(&mut w);
    for _ in 0..2 {
        f3::with_services_in(&mut w, |w, services| crate::combat::live::blast_in(w, crate::store::DefId(0), [0.;3], OrdnanceOwner::Unit(source), services));
        damage(&mut w, other, Attacker::Null);
    }
    tick(&mut w, 1.0);
    assert_eq!(events.borrow().as_slice(), [
        unit_death(&w, source, sc, Some(sc), Some(1.0), 1.0),
        unit_death(&w, other, oc, None, None, 0.0),
    ]);
}

#[test]
fn lost_objective_kills_its_region_once_in_the_post_phase() {
    let mut w = world();
    let (victim, vc) = soldier(&mut w, 2.0);
    let (building, object) = prop(&mut w, false);
    let post = w.sim.store.post.insert(Post {
        active: true, building: Some(building), team: 2.0,
        kill_region: crate::store::Nullable::Value(super::super::regions::region_desc(RegionType::Sphere, None, None, [5.,0.,0.])),
        ..Default::default()
    });
    w.sim.store.command_posts.posts.push(post);
    prepare_tail(&mut w);
    let events = record(&mut w);
    for _ in 0..2 {
        f3::with_services_in(&mut w, |w, services| {
            crate::units::f3_props::command_hit_in(w, 0, 250.0, Some(Attacker::Null), None, services.unwrap());
        });
        tick(&mut w, 1.0);
    }
    assert_eq!(events.borrow().as_slice(), [
        Event::ObjectKilled { object: ObjectHandle::Prop(object), killer: None },
        Event::PostLost { post, from: 2.0 },
        unit_death(&w, victim, vc, None, None, 0.0),
    ]);
}

#[test]
fn a_sprung_trap_does_not_repeat_its_lethal_hit() {
    let mut w = world();
    let (_, object) = prop(&mut w, true);
    prepare_tail(&mut w);
    let events = record(&mut w);
    for _ in 0..2 {
        f3::with_services_in(&mut w, |w, services| {
            crate::units::f3_props::command_hit_in(w, 0, 200.0, Some(Attacker::Null), None, services.unwrap());
        });
    }
    tick(&mut w, 1.0);
    assert_eq!(events.borrow().as_slice(), [Event::ObjectKilled { object: ObjectHandle::Prop(object), killer: None }]);
}

pub(in crate::battle) fn crewed_hull(w: &mut Root, rider: Ref<Unit>) -> Ref<Vehicle> {
    crewed_hull_with_rider(w,rider,false)
}
pub(in crate::battle) fn crewed_hull_with_rider(w: &mut Root, rider: Ref<Unit>, exposed: bool) -> Ref<Vehicle> {
    use crate::vehicles::module::{self as fleet, f2};
    let name = w.sim.strings.intern_str("carrier");
    let class = w.sim.strings.intern_str("{}");
    let pads = w.sim.strings.intern_str("[{}]");
    fleet::veh_class_in(w, name.0, class.0, 0.0);
    w.sim.store.vehicles.slots.push(SpawnSlot::default());
    f2::f2_init_in(w, pads.0, 0.0, 0.0);
    let vehicle = w.sim.store.vehicle.insert(Vehicle {
        name, alive: true, team: Some(2.0), health: 100.0, max_health: 100.0, target_generation: 1.0,
        seats: vec![VehicleSeat::default()], seat_views: vec![SeatView::default()], seat_leases: vec![SeatLeaseView::default()], ..Default::default()
    });
    let pack: &[u8] = if exposed {
        br#"{"models":{"frame":{"parts":[],"bones":[{"name":"perch","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]}},"collision":{"frame":{}},"classes":{"carrier":{"model":"frame","seats":[{"settings":{"PilotPosition":"perch","PilotAnimation":"sit"}}]}}}"#
    } else {
        br#"{"models":{"frame":{"parts":[],"bones":[]}},"collision":{"frame":{}},"classes":{"carrier":{"model":"frame","seats":[{}]}}}"#
    };
    let bytes = fleet::veh_bytes_in(w, (pack.len().next_multiple_of(4)) as u32).unwrap();
    bytes[..pack.len()].copy_from_slice(pack);
    assert_eq!(fleet::veh_pack_in(w, pack.len() as u32, 0, 0, 0), 1);
    let (pose, _) = fleet::new_pose_in(w, "carrier").unwrap();
    let words = crate::vehicles::posing::pull_len(&pose) as u32;
    fleet::veh_input_in(w, words);
    assert_eq!(fleet::veh_pull_in(w, vehicle.slot, 1.0, words, 0), 1);
    f2::f2_register_in(w, vehicle.slot, vehicle.gen, 0);
    let id = w.sim.store.units.list.len() as u32;
    let member = Member::VehicleTarget(Ref::new(id, 0));
    w.sim.store.units.list.push(member);
    w.sim.calls.result(F3HullSetup::WORDS, |r,h| F3HullSetup { hull: vehicle, member: Some(member), ..Default::default() }.put(r,h));
    crate::units::f3_hulls::f3_hull_in(w);
    w.sim.calls.result(SeatHullFacts::WORDS, |r,h| SeatHullFacts {
        vehicle, weapons: vec![0.0], turret_nodes: vec![false], ..Default::default()
    }.put(r,h));
    f2::live_seats::f2_seats_adopt_in(w, 0);
    let lease = w.sim.store.seat_lease.insert(SeatLease {
        vehicle: Some(vehicle), unit: Some(rider), team: 1.0, ..Default::default()
    });
    w.sim.store.unit.get_mut(rider).unwrap().seat = Some(lease);
    w.sim.store.vehicle.get_mut(vehicle).unwrap().seats[0].lease = Some(lease);
    vehicle
}

#[test]
fn crew_death_precedes_hull_credit_and_mobile_post_loss() {
    let mut w = world();
    let (rider, rc) = soldier(&mut w, 1.0);
    let (shooter, kc) = soldier(&mut w, 2.0);
    let vehicle = crewed_hull(&mut w, rider);
    let mp = super::super::mobile::new_mobile_post(0., &mut w.sim.strings);
    let post = w.sim.store.mobile_post.insert(MobilePost {
        vehicle: Some(vehicle), life: Some(1.0), active: true, team: 2.0,
        reported: Reported { active: true, team: 2.0, vehicle: Some(vehicle), life: Some(1.0) }, ..mp
    });
    w.sim.store.mobile_posts.posts.push(post);
    prepare_tail(&mut w);
    state(&mut w.battle).owner.prepared.pads.adopt(0, PsPad {
        variants: vec![PsHull { vehicle, command: true, ..Default::default() }], ..Default::default()
    });
    let events = record(&mut w);
    tick(&mut w, 1.0);
    w.tick.running = true;
    for _ in 0..2 {
        f3::with_services_in(&mut w, |w, services| {
            crate::units::f3_hulls::mutate_in(w, vehicle, 200., Some(OrdnanceOwner::Unit(shooter)), services.unwrap());
        });
    }
    tick(&mut w, 1.0);
    tick(&mut w, 1.0);
    assert_eq!(events.borrow().as_slice(), [
        Event::MobilePostOwner { post, from: 2.0, to: 1.0 },
        unit_death(&w, rider, rc, Some(kc), Some(2.0), -1.0),
        Event::ObjectKilled { object: ObjectHandle::Vehicle { vehicle, life: 1.0 }, killer: Some(kc) },
        Event::MobilePostLost { post, from: 1.0 },
    ]);
}

#[test]
fn a_score_change_reaches_the_next_callback() {
    struct Score(bool);
    impl EventRules for Score {
        fn tick(&mut self, _: f64, batch: &EventQueue, actions: &mut dyn RuleActions) {
            if self.0 {
                assert_eq!(batch.events(), [Event::TeamPointsChanged { team: 2.0, points: 3 }]);
                assert!(actions.end_battle(2.0));
            } else {
                assert!(actions.set_score(2.0, 3));
                assert!(actions.set_score(2.0, 3));
            }
            self.0 = true;
        }
    }
    let mut w = world();
    prepare_tail(&mut w);
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(Score(false), 4, 0))).unwrap();
    tick(&mut w, 1.0);
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.loser, Some(1.0));
}

struct Callback<F>(F);

impl<F: FnMut(&EventQueue, &mut dyn RuleActions)> EventRules for Callback<F> {
    fn tick(&mut self, _: f64, batch: &EventQueue, actions: &mut dyn RuleActions) {
        (self.0)(batch, actions);
    }
}

fn callback(w: &mut Root, commands: usize, run: impl FnMut(&EventQueue, &mut dyn RuleActions) + 'static) {
    set_rules(w, RuleSet::EventDriven(EventDriven::new(Callback(run), commands, 0))).unwrap();
}

fn live_step(w: &mut Root, tick: f64) {
    use crate::battle::f4_live;
    F4LiveInput { dt: 1. / 60., tick }.put(f4_live::live_input_bank_mut_in(w).unwrap(), &mut Vec::new());
    w.tick.running = true;
    f4_live::f4_live_in(w);
}

mod destruction;
mod host;
mod properties;
