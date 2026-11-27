//! Unit tests with compiled Lua 5.0 fixtures and ignored owner-copy
//! integration tests (`BFE_SWBF2_PREPARED`).

use std::path::PathBuf;

use bfe_family::GameFamily;
use bfe_wide::{Fnv, Wide};

use super::*;
use crate::battle::{
    f4_core, f4_live, f4_tail,
    rules::{
        self,
        events::{NamedObject, ObjectHandle},
        ModeParameters, END_SEQUENCE, GAME_OVER_DELAY,
    },
};
use crate::gen::*;
use crate::store::Ref;
use crate::units::f3;
use crate::world::owner::World as Root;

mod bot_blade;
mod flight_approaches;
mod flight_sorties;

const FIXTURE_ARENA_ROSTER: &[u8] = include_bytes!("../../../tests/lua/01_arena_roster.luac");
const FIXTURE_ARENA_MATCH: &[u8] = include_bytes!("../../../tests/lua/01_arena_match.luac");
const FIXTURE_FINITE_TICKETS: &[u8] =
    include_bytes!("../../../tests/lua/02_finite_tickets_timers.luac");
const FIXTURE_TARGETS_SYSTEMS: &[u8] =
    include_bytes!("../../../tests/lua/03_targets_and_systems.luac");
const FIXTURE_ERRORS_UNBOUND: &[u8] =
    include_bytes!("../../../tests/lua/04_errors_and_unbound.luac");
const FIXTURE_RNG_DET: &[u8] = include_bytes!("../../../tests/lua/05_rng_determinism.luac");
const FIXTURE_TIMER_EDGES: &[u8] = include_bytes!("../../../tests/lua/06_timer_edge_cases.luac");
const FIXTURE_INIT_OUTCOMES: &[u8] =
    include_bytes!("../../../tests/lua/07_init_tickets_and_outcomes.luac");
const FIXTURE_IDENTITY_SYSTEMS: &[u8] =
    include_bytes!("../../../tests/lua/08_identity_and_systems.luac");
const FIXTURE_BUDGET_ABORT: &[u8] =
    include_bytes!("../../../tests/lua/09_budget_and_abort.luac");
const FIXTURE_API_CONTRACTS: &[u8] =
    include_bytes!("../../../tests/lua/10_api_contracts.luac");

fn synthetic_world() -> Box<Root> {
    let mut w = Root::new();
    let positions = w.collision.positions_new(0, 0);
    let tree = w.collision.tree_new(positions, 0, 0);
    let collision = w.collision.world_new(tree, -1, -1);
    w.fleet.collision = Some(crate::vehicles::native::Collision::new(
        collision as usize,
        None,
    ));
    f3::f3_init_in(&mut w, collision, f64::NAN, 0);
    w.sim.calls.result(2, |row, _| row.copy_from_slice(&[7.0, 0.0]));
    crate::combat::module::cmb_class_in(&mut w, 0);
    w.sim
        .calls
        .result(F3UnitItems::WORDS, |r, h| F3UnitItems::default().put(r, h));
    crate::units::f3_items::f3_unit_items_in(&mut w);
    w.sim.store.battle = rules::new_battle(Default::default(), 1.0, 2.0);
    w
}

fn spawn_soldier(w: &mut Root, team: f64, human: bool) -> (Ref<Unit>, Ref<Character>) {
    let s = &mut w.sim.store;
    let character = s.character.insert(Character {
        index: s.battle.characters.len() as f64,
        team,
        human,
        ..Default::default()
    });
    let unit = s.unit.insert(Unit {
        alive: true,
        team,
        character: Some(character),
        soldier: UnitBody::Soldier(crate::units::class::soldier_body(
            &bfe_content::Value::Null,
            100.0,
            0.0,
            0.0,
        )),
        combat: Combat {
            health: 100.0,
            max_health: 100.0,
            team,
            ..Default::default()
        },
        ..Default::default()
    });
    s.character.get_mut(character).unwrap().unit = Some(unit);
    s.battle.characters.push(character);
    s.units.list.push(Member::Unit(unit));
    let key = F3UnitKey::of(unit, s.unit.get(unit).unwrap());
    F3Adoption {
        key,
        ..Default::default()
    }
    .put(f3::command_row_in(&mut w.units), &mut Vec::new());
    assert_eq!(f3::f3_adopt_in(w), 1);
    (unit, character)
}

fn prepare_tail(w: &mut Root) {
    w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| {
        F4LiveSetup::default().put(row, heap)
    });
    f4_live::f4_live_init_in(w);
    w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| {
        F4BattleSetup::default().put(row, heap)
    });
    f4_core::f4_battle_init_in(w);
    let setup = F4TailSetup {
        registry_capacity: w.sim.store.units.list.len().max(8) as u32,
        ..Default::default()
    };
    w.sim.calls.result(F4TailSetup::WORDS, |row, heap| {
        setup.put(row, heap)
    });
    f4_tail::f4_tail_init_in(w);
    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass {
            captures: true,
            ..Default::default()
        }
        .put(row, heap);
    });
    f4_tail::f4_tail_unit_class_in(w);
}

/// Missions with a scripted goal table also admit the AI before selecting their rules.
fn prepare_mission_tail(w: &mut Root) {
    prepare_tail(w);
    if w.ai.battle().is_none() {
        let nav = crate::ai::nav::Nav::empty(Default::default());
        let mut settings = crate::gen::AiColdSettings::default();
        settings.capacity.scopes = 1;
        settings.capacity.characters = w.sim.store.character.slots();
        w.ai.begin(nav.into(), &settings, &mut w.sim.strings, Default::default());
        w.battle.map_goal_regions = Some(Default::default());
    }
}

pub(crate) fn tick_world(w: &mut Root, dt: f64) {
    let input = F4TailInput {
        dt,
        server: true,
        ..Default::default()
    };
    let bank = f4_live::tail_input_bank_mut_in(w).unwrap();
    input.put(bank, &mut Vec::new());
    w.tick.running = true;
    f4_tail::f4_tail_in(w);
    w.tick.running = false;
}

fn damage_kill_soldier(w: &mut Root, victim: Ref<Unit>, attacker: Attacker) {
    f3::with_services_in(w, |w, services| {
        crate::combat::live::damage_in(w, victim, 1000.0, attacker, services.unwrap());
    });
}

fn revive_soldier(w: &mut Root, unit: Ref<Unit>, character: Ref<Character>) {
    let body = w.sim.store.unit.get_mut(unit).unwrap();
    body.character = Some(character);
    body.alive = true;
    body.dead = false;
    body.combat.dead = false;
    body.combat.health = 100.0;
    w.sim.store.character.get_mut(character).unwrap().unit = Some(unit);
}

fn add_test_building(
    w: &mut Root,
    slot: u32,
    name: &str,
    team: f64,
    max_health: f64,
) -> (Ref<Building>, F4Handle) {
    let building = Ref::<Building>::new(slot, 1);
    let key = w.sim.strings.intern_str(name);
    let settings = PbSettings {
        max_health,
        has_destroyed: true,
        matrix: three::Matrix4::default().elements,
        models: vec![None; 3],
        ..Default::default()
    };
    let setup = F3BuildingSetup {
        index: slot,
        target: RepairTarget::Building(building),
        key,
        settings: settings.clone(),
        state: F3BuildingState {
            state: PbStateName::Built,
            health: max_health,
            team,
            target_team: team,
            ..Default::default()
        },
        bodies: vec![None; 3],
        live: F4BuildingBirth {
            settings,
            state: PbStation {
                building: PbState {
                    state: PbStateName::Built,
                    health: max_health,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    w.sim
        .calls
        .result(F3BuildingSetup::WORDS, |row, heap| setup.put(row, heap));
    crate::units::f3_props::f3_building_in(w);
    let handle = crate::units::f3_props::owner_in(w, slot as usize);
    (building, handle)
}

fn damage_building(w: &mut Root, slot: usize, dmg: f64, attacker: Option<Attacker>) {
    f3::with_services_in(w, |w, services| {
        crate::units::f3_props::command_hit_in(w, slot, dmg, attacker, None, services.unwrap());
    });
}

/// Isolated binding tests install their own compiled fixture and named objects.
fn admit_test_script(
    w: &mut Root,
    chain: ScriptChain,
    config: ScriptRulesConfig,
    mode: &str,
    score_limit: Option<u32>,
    objects: Vec<NamedObject>,
) -> (LoadSetup, ScriptTelemetryHandle, ScriptInspector) {
    let params = ModeParameters {
        command_limit: DEFAULT_SCRIPT_COMMAND_LIMIT,
        timer_limit: DEFAULT_SCRIPT_TIMER_LIMIT,
        score_limit,
        time_limit: None,
    };
    let mut config = config;
    config.assault_score_limit = config.assault_score_limit.or(score_limit);
    let mut rules = ScriptRules::with_goal_handles(chain, config, w.goal_handles()).unwrap();
    let setup = rules.run_load().unwrap().clone();
    apply_load_setup_to_world(&setup, w);
    let telemetry = rules.attach_telemetry();
    let inspector = rules.attach_inspector();
    w.prepare_script_rules(mode, params, rules).unwrap();
    prepare_mission_tail(w);
    f4_tail::select_rules(w, GameFamily::Swbf2, mode, params, objects).expect("select_rules");
    // Run tick 0 (dt=0.0) so ScriptPostLoad executes inside the active RuleActions scope.
    tick_world(w, 0.0);
    (setup, telemetry, inspector)
}

#[test]
fn tdm_script_full_lifecycle_in_native_world() {
    let mut w = synthetic_world();
    let (shooter_u, _) = spawn_soldier(&mut w, 1.0, true);
    let (ally_u, _) = spawn_soldier(&mut w, 1.0, false);
    let (enemy_u, enemy_c) = spawn_soldier(&mut w, 2.0, false);

    let chain = ScriptChain::new(
        "01_arena_match",
        vec![
            ScriptChunk {
                name: "01_arena_match".to_string(),
                bytes: FIXTURE_ARENA_MATCH.into(),
            },
            ScriptChunk {
                name: "01_arena_roster".to_string(),
                bytes: FIXTURE_ARENA_ROSTER.into(),
            },
        ],
    );
    let mut cfg = ScriptRulesConfig::strict(99);
    cfg.assault_num_bots = Some(12);

    let (setup, telemetry, inspector) =
        admit_test_script(&mut w, chain, cfg, "eli", Some(3), Vec::new());

    assert_eq!(setup.loaded_scripts, ["01_arena_match", "01_arena_roster"]);
    assert_eq!(setup.world_extents, Some(420.0));
    assert_eq!(setup.teams.get(&1).unwrap().name.as_deref(), Some("vanguard"));
    assert_eq!(
        setup.teams.get(&1).unwrap().initial_reinforcements,
        Some(rules::events::Reinforcements::Unlimited)
    );
    assert_eq!(setup.teams.get(&1).unwrap().classes.len(), 2);

    let snap = inspector.snapshot();
    assert!(snap.outcome.is_none());
    assert_eq!(w.sim.store.battle.reinforcements.at(1.0), Some(-1.0));
    assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(-1.0));

    damage_kill_soldier(&mut w, ally_u, Attacker::Null);
    for expected in 1..=3 {
        revive_soldier(&mut w, enemy_u, enemy_c);
        damage_kill_soldier(&mut w, enemy_u, Attacker::Unit(shooter_u));
        tick_world(&mut w, 1.0 / 60.0);
        assert_eq!(telemetry.read().scores[2], expected);
    }
    tick_world(&mut w, 1.0 / 60.0);
    let snap = inspector.snapshot();
    assert_eq!(snap.outcome, Some(BattleOutcome::Victory(2)));
    assert!(snap.diagnostics.errors.is_empty());
    assert!(snap.diagnostics.unbound_calls.is_empty());
    assert!(w.sim.store.battle.over);
    assert_eq!(w.sim.store.battle.loser, Some(1.0));
    assert!(!w.sim.store.battle.game_over);

    tick_world(&mut w, GAME_OVER_DELAY);
    assert!(w.sim.store.battle.game_over);
    assert!(!w.sim.store.battle.finished);
    tick_world(&mut w, END_SEQUENCE);
    assert!(w.sim.store.battle.finished);
}

#[test]
fn finite_tickets_and_timer_ordering() {
    let mut w = synthetic_world();
    let chain = ScriptChain::new(
        "02_finite_tickets_timers",
        vec![ScriptChunk {
            name: "02_finite_tickets_timers".to_string(),
            bytes: FIXTURE_FINITE_TICKETS.into(),
        }],
    );
    let (_, telemetry, inspector) = admit_test_script(
        &mut w,
        chain,
        ScriptRulesConfig::strict(7),
        "tickets",
        None,
        Vec::new(),
    );

    for _ in 0..239 { tick_world(&mut w, 1.0 / 60.0); }
    assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(9.0));
    assert!(telemetry.read().hud_timer_running);
    tick_world(&mut w, 1.0 / 60.0);
    assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(16.0));
    tick_world(&mut w, 1.0 / 60.0);
    assert_eq!(telemetry.read().scores[2], 48);
    assert!(inspector.snapshot().diagnostics.errors.is_empty());

}

#[test]
fn space_objects_properties_and_real_building_kills() {
    let mut w = synthetic_world();
    let (shooter_u, _) = spawn_soldier(&mut w, 1.0, true);
    let mut named=Vec::new();
    for (i,name) in ["hand_press","linen_tray","umber_tray"].iter().enumerate() {
        let (_,handle)=add_test_building(&mut w,i as u32,name,2.0,86.0);
        named.push(NamedObject {object:ObjectHandle::Prop(handle),name:Some(name.to_string()),class:None});
    }
    let rack = w.sim.store.post.insert(Post::default());
    named.push(NamedObject {
        object: ObjectHandle::Post(SpawnPost::Post(rack)),
        name: Some("paper_rack".into()),
        class: None,
    });
    let chain = ScriptChain::new(
        "03_targets_and_systems",
        vec![ScriptChunk {
            name: "03_targets_and_systems".to_string(),
            bytes: FIXTURE_TARGETS_SYSTEMS.into(),
        }],
    );
    let (_,telemetry,inspector)=admit_test_script(&mut w,chain,ScriptRulesConfig::strict(11),"ass",None,named);
    assert!(telemetry.read().not_available_ops >= 8);
    assert_eq!(telemetry.read().missing_objects, 1);
    assert!(inspector.snapshot().diagnostics.errors.is_empty(),"{:?}",inspector.snapshot().diagnostics);
    let dt=1.0/60.0;
    for (slot,points) in [(2,11),(1,18)] {
        damage_building(&mut w,slot,190.0,Some(Attacker::Unit(shooter_u)));
        tick_world(&mut w,dt);
        assert_eq!(telemetry.read().scores[1],points);
        assert!(!w.sim.store.battle.over);
    }
    damage_building(&mut w,0,190.0,Some(Attacker::Unit(shooter_u)));
    tick_world(&mut w,dt);
    assert!(inspector.snapshot().notifications.iter().any(|n| matches!(&n.notification,
        ScriptNotification::BroadcastVoiceOver {sound_id,team:Some(2)} if sound_id.as_ref()=="press.jammed")));
    assert_eq!(w.mission_system_state(&w.mission_system("hand_press").unwrap()).unwrap().team,Some(0.0));
    damage_building(&mut w,0,86.0,None);
    tick_world(&mut w,dt);
    assert!(w.sim.store.battle.over);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
    assert_eq!(inspector.snapshot().outcome, Some(BattleOutcome::Victory(1)));
    assert!(inspector.snapshot().diagnostics.errors.is_empty());
}

#[test]
fn unbound_strict_vs_permissive_and_error_isolation() {
    let chain = ScriptChain::new(
        "04_errors_and_unbound",
        vec![ScriptChunk {
            name: "04_errors_and_unbound".to_string(),
            bytes: FIXTURE_ERRORS_UNBOUND.into(),
        }],
    );

    // 1. Strict mode fails on unbound global call during ScriptPostLoad, records unbound count,
    //    marks terminal Faulted state, and deterministically aborts battle as Draw.
    let mut w_strict = synthetic_world();
    let (_, t_strict, i_strict) = admit_test_script(
        &mut w_strict,
        chain.clone(),
        ScriptRulesConfig::strict(1),
        "err",
        None,
        Vec::new(),
    );
    assert!(t_strict.read().faulted);
    assert_eq!(t_strict.read().unbound_total, 1);
    assert_eq!(
        i_strict
            .snapshot()
            .diagnostics
            .unbound_calls
            .get("UnboundHostFunctionAlpha"),
        Some(&1)
    );
    assert!(w_strict.sim.store.battle.over);
    assert_eq!(w_strict.sim.store.battle.loser, None);

    // 2. Permissive mode logs unbound call as no-op, isolates runtime error & runaway loop.
    let mut w = synthetic_world();
    let (shooter_u, _) = spawn_soldier(&mut w, 1.0, false);
    let (enemy_u, _) = spawn_soldier(&mut w, 2.0, false);

    let cfg = ScriptRulesConfig {
        tick_instruction_budget: 2_000,
        ..ScriptRulesConfig::default()
    };
    let (_, telemetry, inspector) = admit_test_script(&mut w, chain, cfg, "err", None, Vec::new());

    let snap = inspector.snapshot();
    assert_eq!(
        snap.diagnostics.unbound_calls.get("UnboundHostFunctionAlpha"),
        Some(&1)
    );

    // The healthy listener still receives the same event.
    damage_kill_soldier(&mut w, enemy_u, Attacker::Unit(shooter_u));
    tick_world(&mut w, 0.05);
    assert!(!telemetry.read().faulted);
    assert_eq!(telemetry.read().callbacks_dropped, 0);
    assert_eq!(telemetry.read().callback_errors, 1);
    assert_eq!(telemetry.read().scores[1], 1);
    assert!(!w.sim.store.battle.over);
}

#[test]
fn seeded_rng_determinism_and_identical_runs() {
    fn run_once(seed: u32) -> (u32, Wide, u32, u32) {
        let mut w = synthetic_world();
        let (shooter_u, _) = spawn_soldier(&mut w, 1.0, false);
        let (enemy_u, enemy_c) = spawn_soldier(&mut w, 2.0, false);

        let chain = ScriptChain::new(
            "05_rng_determinism",
            vec![ScriptChunk {
                name: "05_rng_determinism".to_string(),
                bytes: FIXTURE_RNG_DET.into(),
            }],
        );
        let (_, telemetry, _) = admit_test_script(
            &mut w,
            chain,
            ScriptRulesConfig::strict(seed),
            "rng",
            None,
            Vec::new(),
        );

        for _ in 0..8 {
            revive_soldier(&mut w, enemy_u, enemy_c);
            damage_kill_soldier(&mut w, enemy_u, Attacker::Unit(shooter_u));
            tick_world(&mut w, 1.0 / 60.0);
        }
        let t = telemetry.read();
        (t.scores[1], t.published_digest, t.rng_state, t.rng_draws)
    }

    let run_a = run_once(42);
    let run_b = run_once(42);
    let run_c = run_once(43);
    assert_eq!(run_a, run_b, "identical seeds must produce identical bits");
    assert_eq!(run_a.3, 8, "each kill draws one random number");
    assert_ne!(run_a, run_c, "different seeds must diverge");
}

#[test]
fn swbf1_conquest_creates_no_event_queue_or_vm() {
    let mut w = synthetic_world();
    prepare_tail(&mut w);
    assert!(f4_tail::select_rules(
        &mut w,
        GameFamily::Swbf1,
        "conquest",
        ModeParameters::default(),
        Vec::new(),
    )
    .is_ok());
    assert!(w.battle.rule_events.is_none());
    assert!(w.mission_view().is_none());
}

#[test]
fn due_win_timer_survives_128_deaths_and_multi_listener_cap_and_invalidation() {
    // Part 2: Multi-listener timer interrupted mid-event by callback cap=1 +
    // death listener stopping/resetting/destroying just-expired timers on the same tick.
    let mut w2 = synthetic_world();
    let (victim_u, _) = spawn_soldier(&mut w2, 2.0, false);
    let chain2 = ScriptChain::new(
        "06_timer_edge_cases",
        vec![ScriptChunk {
            name: "06_timer_edge_cases".to_string(),
            bytes: FIXTURE_TIMER_EDGES.into(),
        }],
    );
    let mut cfg2 = ScriptRulesConfig::strict(102);
    cfg2.max_callbacks_per_tick = 1;
    let (_, t2, _) = admit_test_script(&mut w2, chain2, cfg2, "timers", None, Vec::new());

    // Tick 0.25s: dual_clock elapses. Cap=1 executes listener 1 (+2 pts), retains listener 2!
    tick_world(&mut w2, 0.25);
    assert_eq!(t2.read().scores[1], 2);
    // Next tick (0.05s): listener 2 resumes and adds +5 pts (total 7)!
    tick_world(&mut w2, 0.05);
    assert_eq!(
        t2.read().scores[1],
        7,
        "second OnTimerElapse listener must resume across callback cap"
    );

    // At 0.50s total (0.20s more), stop_clock/reset_clock/destroy_clock all expire,
    // while a death arrives BEFORE them in the same batch and stops/resets/destroys all three.
    damage_kill_soldier(&mut w2, victim_u, Attacker::Null);
    tick_world(&mut w2, 0.20);
    // Run extra ticks: none of the three invalidated timers may fire!
    for _ in 0..5 {
        tick_world(&mut w2, 0.1);
    }
    assert_eq!(
        t2.read().scores[2],
        0,
        "stopped, reset, or destroyed just-expired timers must not fire"
    );
    // After 5.0s total from reset, reset_clock eventually fires (+20 pts).
    tick_world(&mut w2, 4.6);
    tick_world(&mut w2, 0.1);
    assert_eq!(t2.read().scores[2], 20);

    // A full delivery queue preserves the admitted prefix and starts a single deadline.
    let mut w3 = synthetic_world();
    let mut overflow_rules = ScriptRules::new(
        ScriptChain::new(
            "06_timer_edge_cases",
            vec![ScriptChunk {
                name: "06_timer_edge_cases".to_string(),
                bytes: FIXTURE_TIMER_EDGES.into(),
            }],
        ),
        ScriptRulesConfig::strict(104),
    )
    .unwrap();
    struct DrawTrap {
        outcome: Option<rules::events::Outcome>,
    }
    impl rules::events::RuleActions for DrawTrap {
        fn score(&self, _: f64) -> Option<u32> { Some(0) }
        fn start_timer(&mut self, _: crate::store::StrId, _: f64) -> bool { true }
        fn stop_timer(&mut self, _: crate::store::StrId) -> bool { true }
        fn finish(&mut self, o: rules::events::Outcome) -> rules::events::RuleResult<()> {
            self.outcome = Some(o);
            Ok(())
        }
    }
    let empty_q = rules::events::EventQueue::new(4);
    let mut trap = DrawTrap { outcome: None };
    rules::events::EventRules::tick(&mut overflow_rules, 0.0, &empty_q, &mut trap);
    // Occupy the physical reservation before delivering another batch.
    for _ in 0..MAX_PENDING_EVENTS {
        assert!(overflow_rules.host.queue_delivery(state::PendingEvent::TimerElapsed { timer_id: 1, arm_gen: 1 }));
    }
    let mut one_ev = rules::events::EventQueue::new(1);
    let (_, victim) = spawn_soldier(&mut w3, 1.0, false);
    one_ev.push(rules::events::Event::MissionSoldierKilled { victim, victim_team: 1, human: false, hero: false, credit: None, death: Default::default() });
    rules::events::EventRules::tick(&mut overflow_rules, 0.1, &one_ev, &mut trap);
    assert!(!overflow_rules.host().diagnostics.faulted);
    assert_eq!(overflow_rules.host().diagnostics.queue_overflows, 1);
    assert_eq!(overflow_rules.host().degraded_deadline, Some(36_002));
    assert_eq!(trap.outcome, None);
    assert_eq!(overflow_rules.host().outcome, None);
}

#[test]
fn init_ticket_boundary_rosters_unlimited_spawn_and_outcome_validation() {
    let mut w = synthetic_world();
    w.sim.store.spawning.teams = crate::battle::spawning::make_teams(16.0);
    let (u1, _) = spawn_soldier(&mut w, 1.0, true);
    let (u2, _) = spawn_soldier(&mut w, 2.0, false);

    let chain = ScriptChain::new(
        "07_init_tickets_and_outcomes",
        vec![ScriptChunk {
            name: "07_init_tickets_and_outcomes".to_string(),
            bytes: FIXTURE_INIT_OUTCOMES.into(),
        }],
    );
    let (_, telemetry, inspector) = admit_test_script(
        &mut w,
        chain.clone(),
        ScriptRulesConfig::strict(201),
        "init_test",
        None,
        Vec::new(),
    );

    // 1. LoadSetup rosters, unit counts, and spawn delays must be installed before spawning.
    assert_eq!(w.sim.store.spawning.teams[1].unit_count, 10.0);
    assert_eq!(w.sim.store.spawning.teams[1].classes.len(), 1);
    assert_eq!(w.sim.store.spawning.teams[2].unit_count, 8.0);
    assert_eq!(w.sim.store.spawning.teams[2].classes.len(), 1);
    assert_eq!(w.sim.store.spawning.a, 6.0);
    assert_eq!(w.sim.store.spawning.b, f64::from(0.3f32));
    assert!(w.sim.store.battle.teams.get(1.0).is_some());
    assert!(w.sim.store.battle.teams.get(2.0).is_some());

    // 2. Finite-to-infinite (3 -> 0 -> -1) must NOT fire stale init ticket callbacks,
    //    and MissionVictory(0) / invalid team writes must be refused without latching!
    tick_world(&mut w, 0.1);
    tick_world(&mut w, 0.1);
    assert!(!w.sim.store.battle.over, "stale init tickets or MissionVictory(0) must not end match");
    assert_eq!(w.sim.store.battle.reinforcements.at(1.0), Some(-1.0));
    assert_eq!(inspector.snapshot().outcome, None);
    assert!(
        telemetry.read().refused_writes >= 3,
        "invalid team score, ticket, and MissionVictory(0) writes must be counted as refused"
    );

    // 3. Subsequent valid MissionVictory(1) MUST succeed (not blocked by refused MissionVictory(0)).
    damage_kill_soldier(&mut w, u2, Attacker::Unit(u1));
    tick_world(&mut w, 0.1);
    assert!(w.sim.store.battle.over);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
    assert_eq!(inspector.snapshot().outcome, Some(BattleOutcome::Victory(1)));

    // 4. MissionVictory({1, 2}) must commit an authoritative Draw (loser == None).
    let mut w_draw = synthetic_world();
    w_draw.sim.store.spawning.teams = crate::battle::spawning::make_teams(16.0);
    let (du1, _) = spawn_soldier(&mut w_draw, 1.0, false);
    let (du2, _) = spawn_soldier(&mut w_draw, 2.0, false);
    let (_, _, i_draw) = admit_test_script(
        &mut w_draw,
        chain,
        ScriptRulesConfig::strict(202),
        "init_draw",
        None,
        Vec::new(),
    );
    damage_kill_soldier(&mut w_draw, du1, Attacker::Unit(du2));
    tick_world(&mut w_draw, 0.1);
    assert!(w_draw.sim.store.battle.over);
    assert_eq!(
        w_draw.sim.store.battle.loser, None,
        "MissionVictory({{1, 2}}) must publish a true authoritative Draw"
    );
    assert_eq!(
        i_draw.snapshot().outcome,
        Some(BattleOutcome::Draw(vec![1, 2]))
    );
}

#[test]
fn object_life_scoring_and_character_identity_preservation() {
    let mut w = synthetic_world();
    let (human_u, human_c) = spawn_soldier(&mut w, 1.0, true);
    let (enemy_u, enemy_c) = spawn_soldier(&mut w, 2.0, false);

    let (_, enemy_bldg_h) = add_test_building(&mut w, 0, "crit_enemy", 2.0, 500.0);
    let (_, friendly_bldg_h) = add_test_building(&mut w, 1, "crit_friendly", 1.0, 500.0);
    let (_, paired_bldg_h) = add_test_building(&mut w, 2, "crit_paired", 2.0, 500.0);
    let (_, dup_a_h) = add_test_building(&mut w, 3, "dup_bldg", 2.0, 100.0);
    let (_, dup_b_h) = add_test_building(&mut w, 4, "dup_bldg", 2.0, 100.0);

    let named = vec![
        NamedObject { object: ObjectHandle::Prop(enemy_bldg_h), name: Some("crit_enemy".into()), class: None },
        NamedObject { object: ObjectHandle::Prop(friendly_bldg_h), name: Some("crit_friendly".into()), class: None },
        NamedObject { object: ObjectHandle::Prop(paired_bldg_h), name: Some("crit_paired".into()), class: None },
        NamedObject { object: ObjectHandle::Prop(dup_a_h), name: Some("dup_bldg".into()), class: None },
        NamedObject { object: ObjectHandle::Prop(dup_b_h), name: Some("dup_bldg".into()), class: None },
    ];

    let chain = ScriptChain::new(
        "08_identity_and_systems",
        vec![ScriptChunk {
            name: "08_identity_and_systems".to_string(),
            bytes: FIXTURE_IDENTITY_SYSTEMS.into(),
        }],
    );
    let (_, telemetry, inspector) = admit_test_script(
        &mut w,
        chain,
        ScriptRulesConfig::strict(301),
        "identity",
        None,
        named,
    );

    // Missing and ambiguous objects counted without fabricating shadow objects.
    assert_eq!(telemetry.read().missing_objects, 3);
    assert_eq!(inspector.snapshot().diagnostics.ambiguous_objects, 1);

    // 1. The friendly debit starts at the zero floor. Each explicit award counts (including
    //    the duplicate call). The linked kill has no attacker.
    damage_building(&mut w, 1, 600.0, Some(Attacker::Unit(human_u)));
    damage_building(&mut w, 0, 600.0, Some(Attacker::Unit(human_u)));
    tick_world(&mut w, 0.1);
    assert_eq!(telemetry.read().scores[1], 20);
    // Next tick delivers paired crit_paired ObjectKilled (killer=None): score stays 20.
    tick_world(&mut w, 0.05);
    assert_eq!(telemetry.read().scores[1], 20);

    // 2. At 0.25s total (+0.10s), repair_clock repairs crit_enemy to life 2 and tries
    //    KillObject(first_life_ud) which MUST be refused as StaleHandle!
    let refused_before = telemetry.read().refused_writes;
    tick_world(&mut w, 0.10);
    assert!(
        telemetry.read().refused_writes > refused_before,
        "KillObject on stale first-life userdata must be refused"
    );
    // Real lethal damage on rebuilt life 2 scores another 20!
    damage_building(&mut w, 0, 600.0, Some(Attacker::Unit(human_u)));
    tick_world(&mut w, 0.1);
    assert_eq!(telemetry.read().scores[1], 40);

    // 3. Character identity checks:
    // (a) UnitKilled with victim: None must NOT fire OnCharacterDeath.
    w.battle.rule_events.as_mut().unwrap().push(rules::events::Event::UnitKilled {
        unit: F3UnitKey::of(enemy_u, w.sim.store.unit.get(enemy_u).unwrap()),
        victim: None,
        victim_team: 2.0,
        killer: Some(human_c),
        killer_team: Some(1.0),
        relation: -1.0,
    });
    tick_world(&mut w, 0.1);
    assert_eq!(telemetry.read().scores[2], 0, "nil victim must not fire OnCharacterDeath");

    // (b) Live human killer scores +1 + 10 + 100 = 111.
    damage_kill_soldier(&mut w, enemy_u, Attacker::Unit(human_u));
    tick_world(&mut w, 0.1);
    assert_eq!(telemetry.read().scores[2], 111);

    // (c) Launch-snapshot killer_team (1.0) + preserved human flag even after
    //     character changed team AND was removed from the battle!
    w.sim.store.battle.characters.retain(|&c| c != human_c);
    w.sim.store.character.remove(human_c);
    revive_soldier(&mut w, enemy_u, enemy_c);
    w.battle.rule_events.as_mut().unwrap().push(rules::events::Event::UnitKilled {
        unit: F3UnitKey::of(enemy_u, w.sim.store.unit.get(enemy_u).unwrap()),
        victim: Some(enemy_c),
        victim_team: 2.0,
        killer: Some(human_c),
        killer_team: Some(1.0),
        relation: -1.0,
    });
    tick_world(&mut w, 0.1);
    assert_eq!(
        telemetry.read().scores[2],
        222,
        "stale human character ref must preserve previously known human flag and launch team"
    );

    // (d) Attributed killer_team (1.0) with killer: None scores +1 + 10 = 11 (total 233).
    w.battle.rule_events.as_mut().unwrap().push(rules::events::Event::UnitKilled {
        unit: F3UnitKey::of(enemy_u, w.sim.store.unit.get(enemy_u).unwrap()),
        victim: Some(enemy_c),
        victim_team: 2.0,
        killer: None,
        killer_team: Some(1.0),
        relation: -1.0,
    });
    tick_world(&mut w, 0.1);
    assert_eq!(
        telemetry.read().scores[2],
        233,
        "attributed killer_team with no surviving killer ref must still report killer team"
    );
    assert!(inspector.snapshot().diagnostics.errors.iter().all(|e| e.kind == DiagnosticKind::RefusedWrite));
}

#[test]
fn shared_tick_budget_retains_unreached_listeners_and_load_errors_are_terminal() {
    let mut w = synthetic_world();
    let (shooter_u, _) = spawn_soldier(&mut w, 1.0, false);
    let (enemy_u, _) = spawn_soldier(&mut w, 2.0, false);

    let chain = ScriptChain::new(
        "09_budget_and_abort",
        vec![ScriptChunk {
            name: "09_budget_and_abort".to_string(),
            bytes: FIXTURE_BUDGET_ABORT.into(),
        }],
    );
    let mut cfg = ScriptRulesConfig::strict(401);
    // Shared budget allows ~2-3 listeners per tick (~200 insts each) but NOT all 8 together!
    cfg.tick_instruction_budget = 600;

    let (_, telemetry, inspector) = admit_test_script(&mut w, chain, cfg, "budget", None, Vec::new());

    // 1. Host AI goal cap must prevent unbounded host allocation outside the VM heap.
    assert!(
        inspector.snapshot().ai_goals.len() <= state::MAX_AI_GOALS,
        "ai_goals exceeded hard cap: {}",
        inspector.snapshot().ai_goals.len()
    );
    assert!(telemetry.read().refused_writes > 0);

    damage_kill_soldier(&mut w, enemy_u, Attacker::Unit(shooter_u));
    tick_world(&mut w, 0.1);
    let first = telemetry.read();
    assert!(first.scores[1] < 8);
    assert_eq!(first.callback_budget_refusals, 0);
    assert!(!first.faulted && !w.sim.store.battle.over);
    for _ in 0..8 { tick_world(&mut w, 0.1); }
    let last = telemetry.read();
    assert!(last.scores[1] > first.scores[1]);
    assert_eq!(last.scores[1] + last.callback_budget_refusals, 8);
    assert_eq!(last.callbacks_dropped, 0);
    assert!(!last.faulted && !w.sim.store.battle.over);

    // 4. Failed load transitions to terminal Faulted state and refuses retry.
    let bad_chain = ScriptChain::new("missing_entry", Vec::new());
    let mut bad_rules = ScriptRules::new(bad_chain, ScriptRulesConfig::strict(402)).unwrap();
    assert!(matches!(
        bad_rules.run_load(),
        Err(ScriptHostError::MissingEntry(_))
    ));
    assert_eq!(bad_rules.host().lifecycle, LifecycleState::Faulted);
    assert!(matches!(bad_rules.run_load(), Err(ScriptHostError::Faulted)));
}

#[test]
fn api_return_types_typed_targets_releases_boundaries_digest_and_replay_admission() {
    // 1. Numeric boundary conversions (overflow clamp, exact 2_147_483_520, 2^24 handle cap).
    assert_eq!(adapter::f64_to_lua(1e300), f32::MAX);
    assert_eq!(adapter::f64_to_lua(-1e300), f32::MIN);
    assert_eq!(adapter::f64_to_lua(f64::NAN), 0.0);
    assert_eq!(adapter::f64_to_lua(-0.0).to_bits(), 0.0f32.to_bits());
    assert_eq!(adapter::lua_to_i32(2_147_483_520.0f32), 2_147_483_520);
    assert_eq!(adapter::lua_to_i32(2_147_483_648.0f32), i32::MAX);
    assert_eq!(adapter::lua_to_i32(-2_147_483_648.0f32), i32::MIN);
    assert_eq!(
        adapter::lua_to_exact_handle(adapter::MAX_EXACT_LUA_HANDLE as f32),
        Some(adapter::MAX_EXACT_LUA_HANDLE)
    );
    assert_eq!(
        adapter::lua_to_exact_handle((adapter::MAX_EXACT_LUA_HANDLE + 2) as f32),
        None
    );

    // 2. Full API contracts fixture: return types, VO completion clock, typed AI goal targets,
    //    family-checked releases, strict pcall unbound counting, sequenced ring buffer, raw bytes.
    let mut w = synthetic_world();
    let (shooter_u, _) = spawn_soldier(&mut w, 1.0, true);
    let (victim_u, _) = spawn_soldier(&mut w, 2.0, false);
    let (_, target_h) = add_test_building(&mut w, 0, "goal_target", 2.0, 200.0);
    let named = vec![NamedObject {
        object: ObjectHandle::Prop(target_h.clone()),
        name: Some("goal_target".into()),
        class: None,
    }];

    let chain = ScriptChain::new(
        "10_api_contracts",
        vec![ScriptChunk {
            name: "10_api_contracts".to_string(),
            bytes: FIXTURE_API_CONTRACTS.into(),
        }],
    );
    let mut cfg = ScriptRulesConfig::strict(501);
    cfg.voice_over_duration_ms = 500;
    let (setup, telemetry, inspector) = admit_test_script(&mut w, chain, cfg, "api", None, named);

    // Audio configuration recorded (not discarded).
    assert_eq!(setup.camera_shots, vec![[0.0, 0.0, 0.0, 1.0, 10.0, 20.0, 30.0]]);
    assert_eq!(setup.audio_config.ambient_music.len(), 1);
    assert_eq!(setup.audio_config.victory_music.len(), 1);
    assert_eq!(setup.audio_config.defeat_music.len(), 1);
    assert_eq!(setup.audio_config.sound_effects.len(), 1);
    assert_eq!(setup.audio_config.bleeding_vo.len(), 1);
    assert_eq!(setup.audio_config.low_reinforcements_vo.len(), 1);
    assert_eq!(setup.audio_config.out_of_bounds_vo.len(), 1);
    assert_eq!(setup.audio_config.doppler_factor, Some(0.75));
    assert_eq!(setup.audio_config.scaled_parameters.len(), 1);

    // Strict pcall unbound call WAS counted!
    let snap0 = inspector.snapshot();
    assert_eq!(snap0.diagnostics.unbound_calls.get("MissingApiProbe"), Some(&1));
    assert_eq!(snap0.diagnostics.unbound_total, 1);

    // Mismatched ReleaseCharacterDeath(timer_listener) recorded InvalidArgument and kept timer.
    assert!(snap0
        .diagnostics
        .errors
        .iter()
        .any(|e| e.kind == DiagnosticKind::InvalidArgument));

    // Sequenced notification ring evicted earliest entries and retained the final cue.
    assert_eq!(snap0.notifications.len(), MAX_NOTIFICATIONS);
    let last_note = snap0.notifications.last().unwrap();
    assert!(last_note.seq > MAX_NOTIFICATIONS as u32);
    assert_eq!(
        last_note.notification,
        ScriptNotification::ShowMessageText {
            key: "msg.final.cue".into(),
            team: Some(1),
        }
    );

    let host = w.mission_state().unwrap().0.host();
    let view = w.mission_view().unwrap();
    assert_eq!(view.host.last_notification_sequence(), last_note.seq);
    let clock = host.timers.iter().find(|timer| timer.name_bytes == b"vo_check").unwrap();
    assert_eq!(view.timer_due(clock.id, 900), Some(930));
    assert_eq!(view.timer_due(u32::MAX, 900), None);
    assert!(host.notifications_after(0).is_none());
    assert!(host.notifications_after(last_note.seq + 1).is_none());
    assert_eq!(host.notifications_after(last_note.seq).unwrap().count(), 0);
    for _ in 0..2 {
        let mut updates = host.notifications_after(last_note.seq - 1).unwrap();
        assert_eq!(updates.next(), Some(last_note));
        assert_eq!(updates.next(), None);
    }

    // Digest changes while pending timer advances.
    let dig_t0 = telemetry.read().published_digest;
    tick_world(&mut w, 0.25);
    let dig_t1 = telemetry.read().published_digest;
    assert_ne!(dig_t0, dig_t1, "digest must reflect advancing pending timers");

    // Character death registers typed Character AI goal; building kill registers typed Object goal
    // and releases itself via ReleaseObjectKill.
    damage_kill_soldier(&mut w, victim_u, Attacker::Unit(shooter_u));
    damage_building(&mut w, 0, 300.0, Some(Attacker::Unit(shooter_u)));
    tick_world(&mut w, 0.25);
    // At 0.50s total: vo_check elapsed, verified AudioStreamComplete == 1 -> +5 pts;
    // goal_target kill -> +3 pts (total 8 pts).
    assert_eq!(telemetry.read().scores[1], 8);
    let goals = inspector.snapshot().ai_goals;
    assert_eq!(goals.len(), 3);
    assert_eq!(
        goals[0].target1,
        Some(AiGoalTarget::Name(b"drop_zone".to_vec()))
    );
    assert_eq!(
        goals[0].target2,
        Some(AiGoalTarget::Name(b"flag_alpha".to_vec()))
    );
    assert!(matches!(goals[1].target1, Some(AiGoalTarget::Character(h)) if h == w.sim.store.unit.get(shooter_u).unwrap().character.unwrap()));
    assert_eq!(
        goals[2].target1,
        Some(AiGoalTarget::Object(ObjectHandle::Prop(target_h)))
    );

    let before = w.mission_digest().unwrap();
    crate::units::f3_props::set_mission_turret(&mut w.units, 0);
    assert_ne!(before, w.mission_digest().unwrap(), "later destruction now carries turret credit");
    // A bot can keep the subject while the placement is destroyed, then
    // resolve the rebuilt life without changing or reinstalling the goal.
    let subject = match w.event_rules().unwrap().ai_goals().unwrap().row(2).target {
        crate::ai::goals::GoalArg::Placement(object) => object.clone(),
        _ => panic!("expected a placed object"),
    };
    let life = w.current_mission_object(&subject).unwrap();
    assert!(!w.mission_system_state(&life).unwrap().alive);
    damage_building(&mut w, 0, 200.0, None);
    let life = w.current_mission_object(&subject).unwrap();
    assert!(w.mission_system_state(&life).unwrap().alive);
    assert_eq!(w.event_rules().unwrap().ai_goals().unwrap().revision(), 3);

    let ObjectHandle::Prop(old) = life else { unreachable!() };
    let buildings = &mut crate::battle::f4::live_in(&mut w.battle).buildings;
    let value = buildings.checkout(&old).unwrap();
    assert!(buildings.retire(&old));
    let replacement = buildings.adopt(old.life, value);
    assert_eq!(replacement.slot, old.slot);
    assert_ne!(replacement.generation, old.generation);
    assert!(matches!(w.current_mission_object(&subject), Err(crate::battle::rules::objects::RuleError::StaleHandle)));

    // 3. First-match duplicate chunk lookup in ScriptChain.
    let dup_chain = ScriptChain::new(
        "05_rng_determinism",
        vec![
            ScriptChunk {
                name: "05_rng_determinism".to_string(),
                bytes: FIXTURE_RNG_DET.into(),
            },
            ScriptChunk {
                name: "05_RNG_DETERMINISM".to_string(),
                bytes: b"corrupt_second_duplicate".as_slice().into(),
            },
        ],
    );
    let mut dup_rules = ScriptRules::new(dup_chain, ScriptRulesConfig::strict(502)).unwrap();
    dup_rules.run_load().expect("first duplicate chunk must win");


}

/// A network capture says of each critical system what its present object is:
/// its wear at its own pace, its fate at once, and a rebuilt life as the same
/// system. Every field it prepares is one its reader takes.
#[test]
fn a_network_capture_says_each_system_as_its_object_lives() {
    use crate::wire::mission::{read_state, read_system, Bank, Placed, State, System, STATE_SCHEMA, SYSTEM_SCHEMA};
    let mut w = synthetic_world();
    let (human_u, human_c) = spawn_soldier(&mut w, 1.0, true);
    let (_, enemy) = add_test_building(&mut w, 0, "crit_enemy", 2.0, 500.0);
    let (_, friendly) = add_test_building(&mut w, 1, "crit_friendly", 1.0, 500.0);
    let (_, paired) = add_test_building(&mut w, 2, "crit_paired", 2.0, 500.0);
    let (_, spare) = add_test_building(&mut w, 3, "dup_bldg", 2.0, 100.0);
    let named = [("crit_enemy", enemy), ("crit_friendly", friendly), ("crit_paired", paired), ("dup_bldg", spare.clone()), ("dup_bldg", spare)]
        .into_iter().map(|(name, handle)| NamedObject { object: ObjectHandle::Prop(handle), name: Some(name.into()), class: None }).collect();
    let chain = ScriptChain::new("08_identity_and_systems", vec![ScriptChunk { name: "08_identity_and_systems".to_string(), bytes: FIXTURE_IDENTITY_SYSTEMS.into() }]);
    admit_test_script(&mut w, chain, ScriptRulesConfig::strict(301), "identity", None, named);

    struct Said { systems: Vec<(String, System<'static>, u64)>, points: [u32; 2], personal: Vec<(usize, i32)> }
    fn say(w: &Root, bank: &mut Bank) -> Said {
        bank.sample(w, 40);
        let mut said = Said { systems: Vec::new(), points: [0; 2], personal: Vec::new() };
        for offer in bank.offers() {
            let body = &offer.payload[2..];
            match offer.schema {
                SYSTEM_SCHEMA => {
                    let system = read_system(body).unwrap();
                    assert_eq!(offer.id, u32::from(system.index));
                    said.systems.push((system.name.to_string(), system.unnamed(), offer.state));
                }
                STATE_SCHEMA => {
                    let state = read_state(body).unwrap();
                    assert_eq!(offer.id, state.id());
                    match state {
                        State::Match(state) => said.points = state.points,
                        State::Personal(personal) => said.personal.extend(personal.rows.iter().enumerate()
                            .filter_map(|(slot, points)| Some((usize::from(personal.field) * 32 + slot, (*points)?)))),
                        State::Timer(_) | State::Objectives(_) | State::Deployments(_) | State::Classes(_) | State::ClassRoom(_) | State::Leaving(_) => {}
                    }
                }
                schema => panic!("schema {schema:#x}"),
            }
        }
        said
    }
    let mut bank = Bank::default();
    let first = say(&w, &mut bank);
    let facts: Vec<_> = first.systems.iter().map(|(name, s, _)| (name.as_str(), s.alive, s.marker, s.team, s.placed, s.points, s.health, s.shield)).collect();
    let whole = Some([500.0, 500.0]);
    assert_eq!(facts, [("crit_enemy", true, true, 2, Placed::Building(0), 10, whole, None), ("crit_friendly", true, true, 1, Placed::Building(1), 10, whole, None),
        ("crit_paired", true, true, 2, Placed::Building(2), 10, whole, None)]);
    assert!(first.systems.iter().all(|(_, s, _)| s.hud.is_some()));

    damage_building(&mut w, 0, 180.0, Some(Attacker::Unit(human_u)));
    let worn = say(&w, &mut bank);
    assert_eq!((worn.systems[0].1.alive, worn.systems[0].1.health), (true, Some([320.0, 500.0])));
    assert_eq!(worn.systems[0].2, first.systems[0].2, "wear alone goes at its own pace");

    damage_building(&mut w, 0, 600.0, Some(Attacker::Unit(human_u)));
    tick_world(&mut w, 0.1);
    let fallen = say(&w, &mut bank);
    assert_eq!((fallen.systems[0].1.alive, fallen.systems[0].1.health), (false, Some([0.0, 500.0])));
    assert_ne!(fallen.systems[0].2, first.systems[0].2, "its fate goes at once");
    // The fall earned its points, for the team and for the player.
    let slot = w.sim.store.battle.characters.iter().position(|c| *c == human_c).unwrap();
    assert!(fallen.points[0] >= 20 && fallen.personal.iter().any(|(at, points)| *at == slot && *points > 0), "{:?} {:?}", fallen.points, fallen.personal);
    // The script repairs it: the same system, in its present life.
    tick_world(&mut w, 0.15);
    let rebuilt = say(&w, &mut bank);
    assert_eq!((rebuilt.systems[0].0.as_str(), rebuilt.systems[0].1.alive, rebuilt.systems[0].1.placed), ("crit_enemy", true, Placed::Building(0)));
    assert_eq!(bank.omitted, 0);
}

#[test]
fn a_network_capture_replicates_class_room_open_to_a_human() {
    use crate::battle::spawning::{make_teams, mission::Waves};
    use crate::wire::mission::{read_state, Bank, ClassRoom, State, STATE_SCHEMA};
    let mut w = synthetic_world();
    let chain = ScriptChain::new(
        "08_identity_and_systems",
        vec![ScriptChunk { name: "08_identity_and_systems".to_string(), bytes: FIXTURE_IDENTITY_SYSTEMS.into() }],
    );
    admit_test_script(&mut w, chain, ScriptRulesConfig::strict(301), "identity", None, Vec::new());

    let scout = w.sim.strings.intern_str("scout");
    let guard = w.sim.strings.intern_str("guard");
    let raider = w.sim.strings.intern_str("raider");
    w.sim.store.spawning.teams = make_teams(8.0);
    w.sim.store.spawning.teams[1].classes = [scout, guard]
        .map(|name| SpawnClass { name, target: 1.0, hero: false })
        .to_vec();
    w.sim.store.spawning.teams[2].classes = vec![SpawnClass { name: raider, target: 1.0, hero: false }];

    // Configure team setups and waves with max_units = 1 for scout and raider.
    let mut t1 = TeamSetup::new(1);
    t1.classes = [
        UnitClassSetup { class_name: "scout".into(), min_units: 1, max_units: Some(1) },
        UnitClassSetup { class_name: "guard".into(), min_units: 1, max_units: Some(2) },
    ]
    .to_vec();
    let mut t2 = TeamSetup::new(2);
    t2.classes = vec![UnitClassSetup { class_name: "raider".into(), min_units: 1, max_units: Some(1) }];
    let setups: std::collections::BTreeMap<i32, TeamSetup> = [(1, t1), (2, t2)].into();
    let waves = Waves::new(10.0, 0.5, true, &setups);
    if let Some(rules) = f4_tail::event_rules_mut_for_test(&mut w) {
        rules.waves = Some(waves);
        if let Some(script) = rules.mission_mut() {
            script.host.setup.teams = setups;
        }
    }

    let sample_room = |w: &Root, bank: &mut Bank| -> (ClassRoom, u64) {
        bank.sample(w, 10);
        for offer in bank.offers() {
            if offer.schema == STATE_SCHEMA {
                if let Ok(State::ClassRoom(cr)) = read_state(&offer.payload[2..]) {
                    assert_eq!(offer.id, State::ClassRoom(cr).id());
                    return (cr, offer.state);
                }
            }
        }
        panic!("missing ClassRoom field in Bank::sample");
    };

    let mut bank = Bank::default();
    let (cr0, st0) = sample_room(&w, &mut bank);
    assert_eq!(cr0, ClassRoom { count: [2, 1], available: [0b11, 0b1] });
    // Unchanged world keeps the exact same delta state hash.
    let (cr0b, st0b) = sample_room(&w, &mut bank);
    assert_eq!((cr0b, st0b), (cr0, st0));

    // Bots fill team 1's scout (bit 0) and team 2's raider (bit 0).
    let u1 = w.sim.store.unit.insert(Unit { alive: true, class_name: scout, ..Default::default() });
    let c1 = w.sim.store.character.insert(Character { team: 1.0, unit: Some(u1), ..Default::default() });
    w.sim.store.spawning.teams[1].characters.push(c1);
    let u2 = w.sim.store.unit.insert(Unit { alive: true, class_name: raider, ..Default::default() });
    let c2 = w.sim.store.character.insert(Character { team: 2.0, unit: Some(u2), ..Default::default() });
    w.sim.store.spawning.teams[2].characters.push(c2);

    // A human may take any class his team lists, whatever its members hold
    // (the 2005 game limits him only in a class with points to unlock, and no
    // prepared class has any): the room stays open, and the field is not sent
    // again.
    let (cr1, st1) = sample_room(&w, &mut bank);
    assert_eq!(cr1, ClassRoom { count: [2, 1], available: [0b11, 0b1] });
    assert_eq!(st1, st0);
    w.sim.store.unit.get_mut(u2).unwrap().alive = false;
    let (cr2, st2) = sample_room(&w, &mut bank);
    assert_eq!((cr2, st2), (cr1, st1));
}

#[test]
fn ordinary_admission_live_spawning_weapons_and_arena_script_victory() {
    run_flat_battle(FlatMission::invented(), 707);
}

#[derive(Clone, Copy)]
struct FlatMission<'a> {
    map: &'a str, era: &'a str, mode: &'a str, entry: &'a str, prepared: bool,
}
impl FlatMission<'_> {
    fn invented() -> Self { Self { map: "arena", era: "cw", mode: "eli", entry: "01_arena_match", prepared: false } }
}
fn prepared_root() -> PathBuf {
    PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").expect("prepared 2005 family root"))
}

fn flat_files(mission: FlatMission, seed: u32, wall: bool)
    -> (bfe_content::files::MemFiles, crate::world::admit::Options, crate::world::admit::mission::MissionSelection) {
    use crate::world::admit::{self, Bots, Options};
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use bfe_content::files::MemFiles;

    let mut files = MemFiles::default();
    let map_json = r#"{
        "wireBounds":{"min":[-100,-4,-100],"max":[100,12,100]},
        "family":"swbf2","name":"arena","missions":{"01_arena_match":{}},
        "mission": {
            "sides": ["rep", "cis"],
            "spawnDelay": [1.0, 0.2],
            "teams": {
                "1": { "name": "rep", "units": 3, "reinforcements": 5, "classes": [["rep_soldier", 3]] },
                "2": { "name": "cis", "units": 3, "reinforcements": 5, "classes": [["cis_soldier", 3]] }
            }
        },
        "spawns": {
            "first": [[0.0, 0.0, 0.0, 0.0]],
            "late": [],
            "north_spawn": [[0.0, 0.0, 0.0, 0.0], [1.5, 0.0, 0.0, 0.0], [-1.5, 0.0, 0.0, 0.0]],
            "south_spawn": [[0.0, 0.0, 10.0, 3.14159], [1.5, 0.0, 10.0, 3.14159], [-1.5, 0.0, 10.0, 3.14159]]
        },
        "commandPosts": [
            {
                "name": "cp_north",
                "team": 1,
                "active": true,
                "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
                "spawnPath": "north_spawn"
            },
            {
                "name": "cp_south",
                "team": 2,
                "active": true,
                "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,10,1],
                "spawnPath": "south_spawn"
            }
        ]
    }"#;
    let mut map: Json = serde_json::from_str(map_json).unwrap();
    map["name"] = mission.map.into();
    let manifest_path = format!("maps/{}/scripts/{}-{}.json", mission.map, mission.era, mission.mode);
    let mut classes = std::collections::BTreeMap::<String, Vec<String>>::new();
    if mission.prepared {
        let root = prepared_root();
        let read = |path: PathBuf| -> Json { serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap() };
        let real = read(root.join(format!("maps/{}/map.json", mission.map)));
        let record = &real["missions"][mission.entry];
        map["missions"] = json!({mission.entry:record});
        let manifest = read(root.join(&manifest_path));
        for row in manifest["scripts"].as_array().unwrap() {
            if let Some(path) = row["file"].as_str() { files.insert(path, std::fs::read(root.join(path)).unwrap()); }
        }
        files.insert(&manifest_path, serde_json::to_vec(&manifest).unwrap());
        for side in std::fs::read_dir(root.join("sides")).unwrap().flatten() {
            let path = side.path().join("units.json");
            if !path.is_file() { continue; }
            let units = read(path);
            classes.insert(side.file_name().into_string().unwrap(),units["units"].as_object().unwrap().keys().cloned().collect());
        }
    } else {
        let mut rows = Vec::new();
        let mut names = Vec::new();
        for (name, bytes) in [(mission.entry, FIXTURE_ARENA_MATCH),("01_arena_roster",FIXTURE_ARENA_ROSTER)] {
            let sha = format!("{:x}", Sha256::digest(bytes));
            let file = format!("scripts/{sha}.luac");
            rows.push(json!({"id":name,"name":name,"archive":"trials.lvl","file":file,"sha256":sha,"length":bytes.len()})); names.push(name);
            files.insert(&file, bytes.to_vec());
        }
        files.insert(&manifest_path,serde_json::to_vec(&json!({"version":1,"family":"swbf2","map":mission.map,"era":mission.era,
            "mode":mission.mode,"entry":mission.entry,"chunks":names,"scripts":rows})).unwrap());
        map["missions"] = json!({mission.entry:{"scriptManifest":manifest_path,"scripts":rows}});
        classes.insert("rep".into(),vec!["rep_soldier".into(),"rep_runner".into()]);
        classes.insert("cis".into(),vec!["cis_soldier".into()]);
    }
    map["missions"][mission.entry]["layers"] = json!([]);
    map["missions"][mission.entry]["sides"] = Json::Object(classes.keys().map(|side| (side.clone(), json!([]))).collect());
    files.insert(&format!("maps/{}/map.json",mission.map),serde_json::to_vec(&map).unwrap());

    let anims_json = r#"{"fps":30,"clips":{"idle":{"frames":1,"tracks":[],"travel":[0,0,0]}}}"#;
    files.insert("common/anims.json", anims_json.as_bytes().to_vec());
    files.insert("common/common.json", b"{}".to_vec());
    files.insert("common/effects.json", b"{}".to_vec());

    let actor_json = r#"{
        "bones": [
            {"name": "DummyRoot", "parent": -1, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1]},
            {"name": "bone_head", "parent": 0,  "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,1.5,0,1]}
        ],
        "parts": [{"vertexOffset": 0, "vertexCount": 2}]
    }"#;
    let actor_verts: [f32; 18] = [
        -0.4, 0.0, -0.4, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
         0.4, 1.8,  0.4, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
    ];
    let actor_bin: Vec<u8> = actor_verts.iter().flat_map(|v| v.to_le_bytes()).collect();

    let soldier_weapon = r#"{
        "class": "arena_blaster",
        "base": "blaster",
        "channel": 0,
        "ammo": 500,
        "ShotDelay": 0.15,
        "RoundsPerClip": 50,
        "ClipCount": 10,
        "SalvoCount": 1,
        "ShotsPerSalvo": 1,
        "MaxRange": 60,
        "OptimalRange": 15,
        "FireVelocity": 80,
        "ordnance": {
            "base": "bolt",
            "Damage": 60,
            "MaxDamage": 60,
            "Velocity": 80,
            "LifeSpan": 1.5
        }
    }"#;

    let mut controls = Vec::new();
    for (index, mode) in ["stand", "crouch", "jump", "jet", "sprint", "roll", "tumble"].iter().enumerate() {
        // The jump row is an airborne acceleration factor, so it stays small.
        let row = if *mode == "jump" { "0.15 0.12 1".to_string() } else { format!("{} 2 1", index + 2) };
        controls.push(json!(["ControlSpeed", format!("{mode} {row}")]));
    }
    for (name, value) in [
        ("SprintAccelerateTime", "0.42"), ("EnergyBar", "61"), ("EnergyRestore", "3"),
        ("EnergyRestoreIdle", "7"), ("EnergyDrainSprint", "4"), ("EnergyMinSprint", "9"),
        ("EnergyCostJump", "5"), ("EnergyCostRoll", "3"), ("JumpHeight", "1.3"),
        ("JumpForwardSpeedFactor", "0.72"), ("JumpStrafeSpeedFactor", "0.81"), ("RollSpeedFactor", "1.4"),
    ] {
        controls.push(json!([name, value]));
    }
    for (side, ids) in classes {
        let mut units = json!({"units":{}});
        for cls in ids {
            units["units"][&cls] = json!({"class":cls,"model":"person","actor":"actors/person.json","base":"soldier","MaxHealth":100,
                "properties":controls, "weapons":[serde_json::from_str::<Json>(soldier_weapon).unwrap()]});
        }
        files.insert(&format!("sides/{side}/units.json"), serde_json::to_vec(&units).unwrap());
        files.insert(&format!("sides/{side}/weapons.json"), b"{}".to_vec());
        files.insert(&format!("sides/{side}/actors/person.json"), actor_json.as_bytes().to_vec());
        files.insert(&format!("sides/{side}/actors/person.bin"), actor_bin.clone());
        files.insert(&format!("sides/{side}/actors/person_index.bin"), Vec::new());
        files.insert(&format!("sides/{side}/weapons.bin"), Vec::new());
        files.insert(&format!("sides/{side}/weapons_index.bin"), Vec::new());
    }

    let verts: [f32; 12] = [
        -100.0, 0.0, -100.0,
         100.0, 0.0, -100.0,
         100.0, 0.0,  100.0,
        -100.0, 0.0,  100.0,
    ];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];
    files.insert(
        &format!("maps/{}/terrain_collision.bin", mission.map),
        verts.iter().flat_map(|v| v.to_le_bytes()).collect(),
    );
    files.insert(
        &format!("maps/{}/terrain_collision_index.bin", mission.map),
        indices.iter().flat_map(|i| i.to_le_bytes()).collect(),
    );
    for path in [
        "common/anims.bin",
        "maps/arena/collision.bin",
        "maps/arena/collision_index.bin",
        "maps/arena/collision_poly.bin",
        "common/fxmodels.bin",
        "common/fxmodels_index.bin",
    ] {
        files.insert(&path.replace("maps/arena/", &format!("maps/{}/",mission.map)), Vec::new());
    }

    if wall {
        let vertices = [64f32,0.,4., 66.,0.,4., 66.,4.,4., 64.,4.,4.];
        let triangles = [0u32,2,1, 0,3,2, 0,1,2, 0,2,3];
        for (name, bytes) in [
            ("collision.bin",vertices.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()),
            ("collision_index.bin",triangles.iter().flat_map(|x| x.to_le_bytes()).collect()),
            ("collision_poly.bin",(0u32..4).flat_map(u32::to_le_bytes).collect()),
        ] {
            files.insert(&format!("maps/{}/{}",mission.map,name),bytes);
        }
    }

    let options = Options {
        family: GameFamily::Swbf2, mode: mission.mode.into(), practice_target: None,
        map: mission.map.into(),
        side: if mission.era == "gcw" { "all" } else { "rep" }.into(),
        enhanced: false,
        seed,
        heroes: false,
        unit: None,
        name: Some("Tester".into()),
        bots: Bots {
            fielded: true,
            tier: 3,
            scale_percent: 100,
            friendly_fire: true,
        },
    };

    let selection = admit::mission::MissionSelection {
        family: GameFamily::Swbf2, map: options.map.clone(), seed: options.seed,
        era: mission.era.into(), mode: mission.mode.into(),
    };
    (files, options, selection)
}

fn flat_admission(mission: FlatMission, seed: u32, target: Option<u32>, wall: bool)
    -> (Box<Root>, crate::world::admit::Admitted, ScriptTelemetryHandle) {
    use crate::world::admit;
    let (files, options, selection) = flat_files(mission, seed, wall);
    let mut world = Root::new();
    let (admitted, journal, telemetry) = admit::mission::admit_developer_mission(&mut world, &files, &options, &selection,
        admit::mission::MissionPreferences { assault_target: target, bots_per_team: Some(3), direct_spawn: true }).unwrap();
    assert_eq!(journal.selection, selection);
    world.step_init(&admitted.setup);

    (world, admitted, telemetry)
}

#[test]
fn failed_mission_bind_releases_player_and_character() {
    use crate::abi::context::Context;
    use crate::world::admit::mission::MissionPreferences;
    let (files, options, selection) = flat_files(FlatMission::invented(), 707, false);
    let context = Context::new();
    context
        .world_network_mission_content(&files, &options, &selection, MissionPreferences { bots_per_team: Some(3), ..Default::default() })
        .unwrap();
    let (mailbox, players_before, humans_before) = context.owned(|w| {
        let codes = crate::world::admit::session::input_codes().iter().map(|c| w.sim.strings.intern_str(c)).collect();
        let setup = InputMailboxSetup { epoch: 1., capacity: 256., codes };
        w.sim.calls.result(InputMailboxSetup::WORDS, |r, h| setup.put(r, h));
        let mailbox = crate::input::module::input_mailbox_create_in(w);
        // Remove all mission spawning slots so f4_connections::mission::bind fails after players_create_in.
        w.sim.store.spawning.characters.clear();
        let players = w.input.players.pool.live();
        let humans = w.sim.store.character.iter().filter(|(_, c)| c.human).count();
        (mailbox, players, humans)
    }).unwrap();
    let err = context
        .world_network_join(&files, 1, "peer-1", "Tester", false, true, false, mailbox, 1000.)
        .err()
        .expect("join must fail when mission::bind rejects the character");
    assert!(err.to_string().contains("no place for the player"), "{err}");
    context.owned(|w| {
        let players_after = w.input.players.pool.live();
        let humans_after = w.sim.store.character.iter().filter(|(_, c)| c.human).count();
        assert_eq!(players_after, players_before, "failed mission::bind leaked a player handle");
        assert_eq!(humans_after, humans_before, "failed mission::bind leaked a reserved character");
    }).unwrap();
    context.dispose().unwrap();
}

fn prepared_world(mission: FlatMission, seed: u32, bots: u32) -> (Box<Root>,crate::world::admit::Admitted,ScriptTelemetryHandle) {
    prepared_world_at(prepared_root(), mission, seed, bots)
}

fn prepared_world_at(root: PathBuf, mission: FlatMission, seed: u32, bots: u32)
    -> (Box<Root>, crate::world::admit::Admitted, ScriptTelemetryHandle) {
    let files = bfe_content::files::DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let admitted = prepared_world_from(&files, mission, seed, bots);
    assert!(!files.asked.borrow().iter().any(|p| p == "common/anims.json"));
    admitted
}

fn prepared_world_from(files: &dyn bfe_content::files::Files, mission: FlatMission, seed: u32, bots: u32)
    -> (Box<Root>, crate::world::admit::Admitted, ScriptTelemetryHandle) {
    use crate::world::admit::{mission::*, Bots, Options};
    let options = Options { family: GameFamily::Swbf2, mode: mission.mode.into(), practice_target: None, map:mission.map.into(), side:if mission.era=="gcw" { "all" } else {"rep"}.into(),
        enhanced:false,seed,heroes:false,unit:None,name:None,bots:Bots {fielded:true,tier:3,scale_percent:100,friendly_fire:true} };
    let selection = MissionSelection {family:GameFamily::Swbf2,map:options.map.clone(),era:mission.era.into(),mode:mission.mode.into(),seed};
    let mut world = Root::new();
    let (admitted,journal,tel) = admit_developer_mission(&mut world,files,&options,&selection,MissionPreferences {bots_per_team:Some(bots),direct_spawn:true,..Default::default()}).unwrap();
    assert_eq!(journal.selection,selection);
    world.step_init(&admitted.setup);
    (world,admitted,tel)
}

pub(crate) fn seated_combat_scene(exposed: bool) -> (Box<Root>, Ref<Unit>, Ref<Unit>, Ref<Vehicle>) {
    let mut world = synthetic_world();
    let (rider, _) = spawn_soldier(&mut world,2.,false);
    let (attacker, _) = spawn_soldier(&mut world,1.,true);
    let vehicle = f4_tail::rule_tests::crewed_hull_with_rider(&mut world,rider,exposed);
    let member = *world.sim.store.units.list.last().unwrap();
    world.sim.calls.result(F3HullSetup::WORDS, |row,heap| F3HullSetup {
        hull:vehicle, member:Some(member), health:3, ..Default::default()
    }.put(row,heap));
    crate::units::f3_hulls::f3_hull_in(&mut world);
    prepare_tail(&mut world);
    world.tick.running = true;
    (world,attacker,rider,vehicle)
}

fn hero_assault() -> FlatMission<'static> {
    FlatMission { map:"tat2", era:"gcw", mode:"eli", entry:"tat2g_eli", prepared:true }
}

pub(crate) fn prepared_combat_scene(space: bool) -> (Box<Root>, Ref<Unit>) {
    let mission = if space { space_mission() } else { hero_assault() };
    let (mut world, admitted, _) = prepared_world(mission, 59, 3);
    let mut clock = WorldClockState { hz:60., max_steps:4., max_debt:8., budget:f64::INFINITY, ..Default::default() };
    for tick in 1..=30 { flat_step(&mut world,admitted.units,&mut clock,tick); }
    let shooter = spawn_soldier(&mut world,1.,true).0;
    (world,shooter)
}

#[test]
#[ignore = "requires a prepared 2005 mission"]
fn registered_bot_frame_reaches_the_coupled_soldier_and_returns_its_committed_state() {
    use crate::units::infantry::{bot_input::{self, Frame, Stamp}, contact};
    let (mut world, admitted, _) = prepared_world(hero_assault(), 31, 8);
    let mut clock = WorldClockState {hz:60.,max_steps:4.,max_debt:8.,budget:f64::INFINITY,..Default::default()};
    let installation = 11;
    world.install_bot_combat_input(installation);
    let mut driven = None;
    let mut guarded = false;
    let mut attacked = false;
    for tick in 1..=1200 {
        let defs = crate::combat::module::defs_in(&mut world.combat);
        let mut stamp = None;
        let mut lives = Vec::new();
        world.visit_melee(|life,_,_,_,_| lives.push(life));
        for life in lives {
            if driven.is_some_and(|(old,_)| old!=life) { continue; }
            let body = Ref::new(life.slot,life.slab);
            let Some(unit) = world.sim.store.unit.get_mut(body) else { continue };
            if unit.human || unit.seat.is_some() || contact::identity(body,unit)!=life {continue;}
            let Some(binding) = bot_input::equipped(&mut unit.combat,&defs.weapons) else {continue};
            let (_,started) = *driven.get_or_insert((life,tick));
            let age=tick-started;
            let key=Stamp {installation,tick,life,binding:Some(binding)};
            let frame=Frame {stamp:key,fire:false,fire_pressed:age>=120 && age%30==0,
                guard:false,guard_pressed:age==5 || age==75,hold_to_block:false,forward:0.,right:0.};
            let stale = contact::LifeId {generation:life.generation.wrapping_sub(1),..life};
            assert!(!world.stage_bot_combat_input(Frame {stamp:Stamp {life:stale,..key},..frame}));
            assert!(world.stage_bot_combat_input(frame));
            assert!(!world.stage_bot_combat_input(frame));
            stamp=Some(key);
            break;
        }
        flat_step(&mut world,admitted.units,&mut clock,tick);
        if let Some(stamp)=stamp {
            let report=world.take_bot_combat_feedback(stamp).expect("coupled soldier did not commit its frame");
            assert!(world.take_bot_combat_feedback(stamp).is_none());
            world.visit_melee(|life,_,_,energy,swing| {
                if life==stamp.life { assert_eq!(report.energy,energy); attacked |= swing; }
            });
            guarded |= report.guarding;
            if guarded && attacked {return;}
        }
    }
    panic!("registered soldier guard={guarded} attack={attacked}");
}

/// A fixed tick with all three authority banks set, also used by contact integration tests.
fn flat_step(world: &mut Root, units: u32, clock: &mut WorldClockState, step_idx: u32) -> u32 {
    use crate::world::admit;
    let dt = 1./60.;
    let tick_f = f64::from(step_idx);
    // Sync body enabled flags between movement and collision kernels, then
    // write the per-tick bank headers (`F4_LIVE_INPUT`, `F4_TAIL_INPUT`, `AI_HEADER`).
    {
        let n = world.movement.flags_mut().len().min(world.collision.enabled_mut().len());
        let mut tmp = vec![0u8; n];
        tmp.copy_from_slice(&world.movement.flags_mut()[..n]);
        world.collision.enabled_mut()[..n].copy_from_slice(&tmp);
    }
    let live_in = F4LiveInput { dt, tick: tick_f };
    let mut live_words = [0.0f64; F4LiveInput::WORDS];
    live_in.put(&mut live_words, &mut Vec::new());
    let live_bytes: Vec<u8> = live_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    admit::banks::write(world, "F4_LIVE_INPUT", &live_bytes);

    let tail_in = F4TailInput {
        dt,
        tick: tick_f,
        server: true,
        human_commands: 0,
        human_count: 0,
    };
    let mut tail_words = [0.0f64; F4TailInput::WORDS];
    tail_in.put(&mut tail_words, &mut Vec::new());
    let tail_bytes: Vec<u8> = tail_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    admit::banks::write(world, "F4_TAIL_INPUT", &tail_bytes);

    let ai_hdr = AiPhaseHeader {
        facts: AiPhaseFacts {
            dt,
            tick: step_idx,
            network_views: true,
            lod_disabled: false,
            reticle: AiPhaseReticle {
                pilot: None,
                pilot_life: 0.0,
                target: None,
                target_life: 0.0,
            },
        },
        time: tick_f / 60.0,
        paused: false,
        network_secondary: false,
        unit_phase_follows: true,
        cameras: 0,
        events: 0,
        sequence: 0.0,
    };
    let mut ai_words = [0.0f64; AiPhaseHeader::WORDS];
    ai_hdr.put(&mut ai_words, &mut Vec::new());
    let ai_bytes: Vec<u8> = ai_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    admit::banks::write(world, "AI_HEADER", &ai_bytes);

    let input = WorldTickInput {
        dt,
        tick: tick_f,
        at: tick_f * (1000.0 / 60.0),
        mouse_scale: 1.0,
        clock: clock.clone(),
        units,
        ai: true,
        live: true,
        ..Default::default()
    };
    let out = world.step(&input);
    assert!(out.accepted, "world.step must accept tick {step_idx}");
    // A refused soldier no longer fails the world, so a healthy battle asserts it here.
    assert!(
        soldier_fault::EXPECTED.with(|e| e.get()) || world.soldier_movement_faults() == 0,
        "a soldier's movement was refused by tick {step_idx}"
    );
    *clock = out.clock.clone();
    out.ordnance
}

/// The host's bool step over the same helper: whether this tick emitted ordnance.
fn step_combat(world: &mut Root, units: u32, step_idx: u32, clock: &mut WorldClockState) -> bool {
    flat_step(world, units, clock, step_idx) > 0
}

pub(crate) fn combat_arena() -> Box<Root> {
    let (mut world, admitted, _) = flat_admission(FlatMission::invented(), 707, Some(50), true);
    let mut clock = WorldClockState { hz:60., max_steps:4., max_debt:8., budget:f64::INFINITY, ..Default::default() };
    for tick in 1..=400 {
        flat_step(&mut world, admitted.units, &mut clock, tick);
        let alive = |team| world.sim.store.unit.iter().filter(|(_, u)| u.team == team && u.alive && !u.dead && !u.removed).count();
        if alive(1.) >= 2 && alive(2.) >= 2 {
            // Spawning follows movement. Step the fielded soldiers once to
            // initialize their native movement and deflection state.
            flat_step(&mut world, admitted.units, &mut clock, tick + 1);
            return world;
        }
    }
    panic!("test population did not field both teams: {:?}", world.sim.store.unit.iter().filter(|(_,u)|u.alive && !u.dead && !u.removed).map(|(_,u)|(u.team,u.human,u.generation)).collect::<Vec<_>>());
}

fn run_flat_battle(mission: FlatMission, seed: u32) -> (u32, u32, Wide, Wide) {
    let (mut world, admitted, telemetry) = flat_admission(mission, seed, Some(3), false);
    let mut digest = Fnv::new();
    let mut summary = digest;
    let mut finish = 0;
    let mix = |h: &mut Fnv, bytes: &[u8]| h.bytes(bytes);
    let mut clock = WorldClockState {
        hz: 60.0,
        max_steps: 4.0,
        max_debt: 8.0,
        budget: f64::INFINITY,
        ..Default::default()
    };

    let mut first_to_target: Option<u32> = None;
    let mut saw_team1_alive = false;
    let mut saw_team2_alive = false;
    let mut saw_ordnance = false;
    let mut saw_respawn = false;
    let mut prev_dead = false;

    for step_idx in 1..=9000u32 {
        let ordnance = flat_step(&mut world, admitted.units, &mut clock, step_idx);

        if ordnance > 0
            || !world.sim.store.ordnance.bolts.is_empty()
            || !world.sim.store.ordnance.rounds.is_empty()
        {
            saw_ordnance = true;
        }
        let mut t1_alive = 0usize;
        let mut t2_alive = 0usize;
        let mut any_dead = false;
        for (_, u) in world.sim.store.unit.iter() {
            if u.removed {
                continue;
            }
            if u.alive && !u.dead {
                if u.team == 1.0 {
                    t1_alive += 1;
                } else if u.team == 2.0 {
                    t2_alive += 1;
                }
            } else if u.dead {
                any_dead = true;
            }
        }
        if t1_alive > 0 {
            saw_team1_alive = true;
        }
        if t2_alive > 0 {
            saw_team2_alive = true;
        }
        if any_dead {
            prev_dead = true;
        } else if prev_dead && t1_alive > 0 && t2_alive > 0 {
            saw_respawn = true;
        }
        let step_scores = telemetry.read().scores;
        if mission.prepared {
            let mut kills = [0.0; 3];
            for (_, character) in world.sim.store.character.iter() {
                if let Some(count) = kills.get_mut(character.team as usize) {
                    *count += character.kills - character.team_kills;
                }
            }
            assert_eq!(
                [step_scores[1], step_scores[2]].map(f64::from),
                kills[1..3],
                "score and enemy kills at tick {step_idx}"
            );
        }
        if first_to_target.is_none() {
            first_to_target = match (step_scores[1] >= 3, step_scores[2] >= 3) {
                (true, false) => Some(1),
                (false, true) => Some(2),
                (true, true) => Some(0),
                (false, false) => None,
            };
        }
        mix(&mut digest, &world.mission_digest().unwrap().to_le_bytes());
        mix(&mut summary, &capture_match_summary_bytes(&world));
        if world.sim.store.battle.over {
            finish = step_idx;
            break;
        }
    }

    let snap = telemetry.read();
    let r1 = world.sim.store.battle.reinforcements.at(1.0);
    let r2 = world.sim.store.battle.reinforcements.at(2.0);
    let over = world.sim.store.battle.over;
    let loser = world.sim.store.battle.loser;
    assert!(!snap.faulted, "script host must not fault in live battle");
    assert_eq!(snap.unbound_total, 0, "no unbound calls in strict mode");
    assert_eq!(
        (
            snap.callback_errors,
            snap.callbacks_dropped,
            snap.refused_writes,
            snap.refused_commands,
        ),
        (0, 0, 0, 0),
        "live battle must run without script host errors or refusals"
    );
    assert_eq!(
        r1,
        Some(-1.0),
        "mission must keep team 1 on unlimited reinforcements"
    );
    assert_eq!(
        r2,
        Some(-1.0),
        "mission must keep team 2 on unlimited reinforcements"
    );
    assert!(
        saw_team1_alive && saw_team2_alive,
        "both rosters must spawn live soldiers through ordinary admission"
    );
    assert!(saw_ordnance, "soldiers must fire real weapon ordnance");
    assert!(
        over,
        "live soldier combat must drive {} to MissionVictory (scores: {:?})",
        mission.entry,
        snap.scores
    );
    let loser_team = match loser {
        Some(1.0) => 1u32,
        Some(2.0) => 2u32,
        other => panic!("finished battle must name team 1 or 2 as loser (got {other:?})"),
    };
    let winner_team = 3 - loser_team;
    assert!(
        snap.scores[winner_team as usize] >= 3,
        "winning team {winner_team} must reach score_limit=3 from real soldier kills (got {:?})",
        snap.scores
    );
    if let Some(first @ (1 | 2)) = first_to_target {
        assert_eq!(
            winner_team, first,
            "team {first} reached score_limit=3 first, but team {loser_team} was recorded as loser (scores: {:?})",
            snap.scores
        );
    }
    assert_eq!(
        world.mission_state().unwrap().0.host().outcome,
        Some(BattleOutcome::Victory(winner_team as i32)),
        "host outcome must match the battle loser"
    );
    let _ = saw_respawn;
    (loser_team, finish, digest.finish(), summary.finish())
}

fn find_hero_bot_outcomes(m: FlatMission) -> [(u32, (u32, u32, Wide, Wide)); 2] {
    let mut found: [Option<(u32, (u32, u32, Wide, Wide))>; 2] = [None, None];
    for seed in 1..=64u32 {
        let run = run_flat_battle(m, seed);
        let slot = usize::from(run.0 == 2);
        if found[slot].is_none() {
            found[slot] = Some((seed, run));
            if found[0].is_some() && found[1].is_some() {
                break;
            }
        }
    }
    let loss_team1 = found[0].expect("expected a team 1 defeat across seeds 1..=64");
    let loss_team2 = found[1].expect("expected a team 2 defeat across seeds 1..=64");
    [loss_team1, loss_team2]
}

#[test]
#[ignore = "requires prepared 2005 mission content in BFE_SWBF2_PREPARED"]
fn owner_hero_bots_win_and_loss() {
    let m = FlatMission { map: "tat2", era: "gcw", mode: "eli", entry: "tat2g_eli", prepared: true };
    let outcomes = find_hero_bot_outcomes(m);
    assert_eq!(outcomes[0].1.0, 1);
    assert_eq!(outcomes[1].1.0, 2);
    for (seed, run) in outcomes {
        assert_eq!(run, run_flat_battle(m, seed));
        println!("Hero bots seed={seed} loser={} tick={} mission={:016x} summary={:016x}",run.0,run.1,run.2,run.3);
    }
}

#[test]
fn all_lua_fixtures_embed_bare_sibling_chunk_names() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua");
    let scripts: Vec<_> = std::fs::read_dir(root).unwrap().map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "lua")).collect();
    assert!(!scripts.is_empty());
    for script in scripts {
        let expected_file = script.file_name().unwrap().to_str().unwrap();
        let expected_source = format!("@{expected_file}");
        let bytes = std::fs::read(script.with_extension("luac")).unwrap();
        let chunk = bfe_script::load_chunk(&bytes)
            .unwrap_or_else(|e| panic!("{expected_file}: failed to parse compiled chunk: {e}"));
        let source_bytes = chunk
            .source()
            .unwrap_or_else(|| panic!("{expected_file}: compiled chunk missing source name"));
        let source = String::from_utf8_lossy(source_bytes);
        assert_eq!(
            source, expected_source,
            "{expected_file}: chunk must be compiled from its sibling file with a bare filename"
        );
        assert!(
            !source.contains('/') && !source.contains('\\'),
            "{expected_file}: chunk source must not contain directory paths: {source}"
        );
    }
}

#[test]
fn configured_limits_override_provisional_defaults() {
    let default_cfg = ScriptRulesConfig::default();
    assert_eq!(default_cfg.assault_score_limit, None);
    assert_eq!(default_cfg.effective_assault_score_limit(None), 180);
    assert_eq!(default_cfg.effective_assault_score_limit(Some(42)), 42);

    let custom = ScriptRulesConfig {
        assault_score_limit: Some(15),
        hunt_score_limit: Some(25),
        hunt_max_time_limit: Some(90),
        uber_score_limit: Some(75),
        assault_num_bots: Some(8),
        ..ScriptRulesConfig::default()
    };
    assert_eq!(custom.effective_assault_score_limit(Some(99)), 15);
    assert_eq!(custom.effective_hunt_score_limit(), 25);
    assert_eq!(custom.effective_hunt_max_time_limit(), 90);
    assert_eq!(custom.effective_uber_score_limit(), 75);
    assert_eq!(custom.effective_assault_num_bots(), 8);
}

fn capture_match_summary_bytes(w: &Root) -> Vec<u8> {
    use crate::wire::{
        stagger::Stagger,
        store_capture::Capture,
        store_summaries::{Link, Links, Summaries},
    };
    struct Offline;
    impl Links for Offline {
        fn get(&self, _: Ref<Character>) -> Option<Link> {
            None
        }
    }
    let posts = w.sim.store.command_posts.posts.iter().enumerate().map(|(i,r)| WireStoreName {
        id:i as u32 + 1, name:w.sim.store.post.get(*r).unwrap().name,
    }).collect();
    let maps = Capture::new(&WireStoreSetup { posts, ..Default::default() }, &w.sim.strings).unwrap();
    let mut stagger = Stagger::new();
    Summaries::default()
        .take(
            1.0,
            1.0,
            &w.sim.store,
            &w.sim.strings,
            &maps,
            &Offline,
            &mut stagger,
        )
        .unwrap();
    stagger
        .entries
        .iter()
        .find(|r| r.schema == 1.0)
        .unwrap()
        .field
        .as_ref()
        .unwrap()
        .payload
        .clone()
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn retail_tat2g_eli_live_combat_busy_win_and_determinism() {
    use crate::world::admit::mission::MissionSelection;
    let root = prepared_root();
    let files = bfe_content::files::DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2);
    let map = serde_json::from_slice(&std::fs::read(root.join("maps/tat2/map.json")).unwrap()).unwrap();
    let selection = MissionSelection {
        family: GameFamily::Swbf2, map: "tat2".into(), era: "gcw".into(), mode: "eli".into(), seed: 1337,
    };
    let (chain, _) = selection.read_chain(&files, &map).unwrap();

    struct HeroRun {
        digests: Vec<Wide>,
        summaries: Vec<Vec<u8>>,
        snap: ScriptDisplaySnapshot,
        loser: Option<f64>,
        game_over: bool,
        finished: bool,
    }

    fn run_hero_assault(chain: &ScriptChain, target_score: u32) -> HeroRun {
        let mut w = synthetic_world();
        w.sim.store.spawning.teams = crate::battle::spawning::make_teams(16.0);
        let (hero_u, hero_c) = spawn_soldier(&mut w, 1.0, true);
        let (hero_ally_u, _) = spawn_soldier(&mut w, 1.0, false);
        let (villain_u, villain_c) = spawn_soldier(&mut w, 2.0, false);

        let cfg = ScriptRulesConfig::strict(1337);
        let (setup, telemetry, inspector) = admit_test_script(
            &mut w,
            chain.clone(),
            cfg,
            "eli",
            Some(target_score),
            Vec::new(),
        );
        assert!(!setup.loaded_scripts.is_empty());
        assert!(setup.max_fly_height.is_some());
        assert!(setup.spawn_wave_delay.is_some());
        assert!(!setup.teams.get(&1).unwrap().classes.is_empty());
        assert!(!setup.teams.get(&2).unwrap().classes.is_empty());
        assert!(!w.sim.store.spawning.teams[1].classes.is_empty());
        assert!(!w.sim.store.spawning.teams[2].classes.is_empty());
        assert_eq!(w.sim.store.battle.reinforcements.at(1.0), Some(-1.0));
        assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(-1.0));

        let mut digests = vec![telemetry.read().published_digest];
        let mut summaries = vec![capture_match_summary_bytes(&w)];
        let dt = 1.0 / 60.0;

        // World/suicide and teamkill award 0 points and keep unlimited tickets intact.
        damage_kill_soldier(&mut w, villain_u, Attacker::Null);
        damage_kill_soldier(&mut w, hero_ally_u, Attacker::Unit(hero_u));
        tick_world(&mut w, dt);
        assert!(w.mission_state().unwrap().1.is_empty());
        assert_eq!(telemetry.read().scores[1], 0);
        assert_eq!(telemetry.read().scores[2], 0);
        assert_eq!(w.sim.store.battle.reinforcements.at(1.0), Some(-1.0));
        assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(-1.0));
        assert!(!w.sim.store.battle.over);

        // Team 2 scores a few kills below target.
        let rival_kills = target_score.saturating_sub(2).min(4);
        for _ in 0..rival_kills {
            revive_soldier(&mut w, hero_u, hero_c);
            damage_kill_soldier(&mut w, hero_u, Attacker::Unit(villain_u));
            tick_world(&mut w, dt);
        }
        assert_eq!(telemetry.read().scores[2], rival_kills);
        digests.push(telemetry.read().published_digest);
        summaries.push(capture_match_summary_bytes(&w));

        // Team 1 scores up to configured target_score -> starts victory delay timer.
        for _ in 0..target_score {
            revive_soldier(&mut w, villain_u, villain_c);
            damage_kill_soldier(&mut w, villain_u, Attacker::Unit(hero_u));
            tick_world(&mut w, dt);
        }
        let personal = w.mission_state().unwrap().1;
        assert_eq!(personal.iter().find(|p| p.character == hero_c).map(|p| p.points), Some(target_score as i32));
        assert_eq!(personal.iter().find(|p| p.character == villain_c).map(|p| p.points), Some(rival_kills as i32));
        assert_eq!(telemetry.read().scores[1], target_score);
        assert!(!w.sim.store.battle.over);
        // Capture digest while victory delay timer is pending!
        digests.push(telemetry.read().published_digest);
        summaries.push(capture_match_summary_bytes(&w));

        for _ in 0..180 {
            tick_world(&mut w, dt);
            if w.sim.store.battle.over {
                break;
            }
        }
        assert!(w.sim.store.battle.over, "tat2g_eli should reach MissionVictory");
        assert_eq!(w.sim.store.battle.loser, Some(2.0));
        digests.push(telemetry.read().published_digest);
        summaries.push(capture_match_summary_bytes(&w));

        let snap = inspector.snapshot();
        assert_eq!(snap.outcome, Some(BattleOutcome::Victory(1)));
        assert!(snap.diagnostics.errors.is_empty(), "errors: {:?}", snap.diagnostics.errors);
        assert!(
            snap.diagnostics.unbound_calls.is_empty(),
            "unbound: {:?}",
            snap.diagnostics.unbound_calls
        );

        tick_world(&mut w, GAME_OVER_DELAY);
        let game_over = w.sim.store.battle.game_over;
        tick_world(&mut w, END_SEQUENCE);
        let finished = w.sim.store.battle.finished;
        summaries.push(capture_match_summary_bytes(&w));
        HeroRun {
            digests,
            summaries,
            snap,
            loser: w.sim.store.battle.loser,
            game_over,
            finished,
        }
    }

    let a = run_hero_assault(&chain, 9);
    let b = run_hero_assault(&chain, 9);
    assert!(a.game_over && a.finished);
    assert_eq!(a.loser, Some(2.0));
    assert_eq!((a.loser, a.game_over, a.finished), (b.loser, b.game_over, b.finished));
    assert_eq!(a.digests, b.digests);
    assert_eq!(a.summaries, b.summaries);
    assert_eq!(a.snap, b.snap);

    // Busy winning tick proof on unmodified tat2g_eli:
    // Reach score_limit = 1, then flood 128 deaths on the due win-timer tick
    // so `max_callbacks_per_tick` saturates; the victory timer MUST NOT be
    // dropped and must fire MissionVictory(1) on the following tick.
    let default = ScriptRulesConfig::strict(1338);
    // Check the stock allowance, then isolate the count cap by providing enough
    // work to start all 256 callbacks before a work-reservation deferral.
    for (config, expect_count_cap) in [(default.clone(), false), (ScriptRulesConfig {
        tick_instruction_budget: default.tick_instruction_budget * 2,
        tick_native_ops_budget: default.tick_native_ops_budget * 2,
        ..default
    }, true)] {
        let mut w_busy = synthetic_world();
        let (bh_u, _) = spawn_soldier(&mut w_busy, 1.0, true);
        let (bv_u, bv_c) = spawn_soldier(&mut w_busy, 2.0, false);
        let (_, tel_busy, insp_busy) = admit_test_script(
            &mut w_busy,
            chain.clone(),
            config,
            "eli",
            Some(1),
            Vec::new(),
        );
        damage_kill_soldier(&mut w_busy, bv_u, Attacker::Unit(bh_u));
        tick_world(&mut w_busy, 1.0 / 60.0);
        assert_eq!(tel_busy.read().scores[1], 1);
        assert!(!w_busy.sim.store.battle.over);

        // Flood 128 deaths on the tick that advances past tat2g_eli's victory timer.
        for _ in 0..128 {
            revive_soldier(&mut w_busy, bv_u, bv_c);
            damage_kill_soldier(&mut w_busy, bv_u, Attacker::Unit(bh_u));
        }
        tick_world(&mut w_busy, 5.0);
        // Victory timer must survive saturation (fires either this tick or next tick).
        for _ in 0..5 {
            if w_busy.sim.store.battle.over {
                break;
            }
            tick_world(&mut w_busy, 1.0 / 60.0);
        }
        assert!(
            w_busy.sim.store.battle.over,
            "tat2g_eli win timer must survive 128 simultaneous deaths on its due tick"
        );
        assert_eq!(w_busy.sim.store.battle.loser, Some(2.0));
        let busy_snap = insp_busy.snapshot();
        assert_eq!(busy_snap.outcome, Some(BattleOutcome::Victory(1)));
        assert!(
            !expect_count_cap || busy_snap
                .diagnostics
                .errors
                .iter()
                .any(|e| e.kind == DiagnosticKind::CallbackCapReached),
            "busy tick must actually saturate callback cap: {:?}",
            busy_snap.diagnostics.errors
        );
        assert!(
            busy_snap
                .diagnostics
                .errors
                .iter()
                .all(|e| e.kind == DiagnosticKind::CallbackCapReached),
            "unexpected non-cap errors on busy tick: {:?}",
            busy_snap.diagnostics.errors
        );
        assert_eq!(busy_snap.diagnostics.callback_budget_refusals, 0);
    }
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn retail_spa6c_ass_prepared_instances_real_building_kills() {
    let m = FlatMission { map:"spa6",era:"cw",mode:"ass",entry:"spa6c_ass",prepared:true };
    let (mut w, _, telemetry) = prepared_world(m, 2005, 3);
    tick_world(&mut w, 0.0);
    let host = w.mission_state().unwrap().0.host();
    assert!(host.diagnostics.errors.is_empty(), "{:?}, systems={:?}", host.diagnostics,host.critical_systems.iter().map(|r| &r.name).collect::<Vec<_>>());
    assert_eq!(host.diagnostics.missing_objects,0);
    assert_eq!(host.diagnostics.ambiguous_objects, 0);
    assert!(host.diagnostics.unbound_calls.is_empty());
    assert_eq!(host.critical_systems.len(), 21);
    for team in [1, 2] {
        assert_eq!(host.objectives.iter().filter(|o| o.team == team).count(), 8);
    }
    assert!(!host.linked_critical_systems.is_empty());
    for group in &host.linked_critical_systems {
        assert!(group.len() >= 2);
        for name in group {
            assert!(host.critical_systems.iter().any(|system| system.name == *name));
        }
    }
    assert_eq!(host.cached_score_limit, Some(180));
    let systems: Vec<_> = host.critical_systems.iter().map(|c| {
        let ObjectHandle::Prop(h) = &c.object else { panic!("expected native building"); };
        let slot = crate::units::f3_props::building_of_in(&w.units,h).unwrap();
        (slot, crate::units::f3_props::rule_team(&w.units,slot) as i32,u32::try_from(c.points).unwrap())
    }).collect();
    let dt = 1.0 / 60.0;
    for _ in 0..180 { tick_world(&mut w,dt); }
    let bodies = |team| w.sim.store.spawning.characters.iter().copied().filter_map(|r| {
        let c = w.sim.store.character.get(r)?;
        (c.team == team).then_some((c.unit?,r))
    }).collect::<Vec<_>>();
    let pilots = bodies(1.0);
    let (human,character) = pilots[0];
    let (bot,bot_character) = pilots[1];
    let (victim,victim_character) = bodies(2.0)[0];
    w.sim.store.character.get_mut(character).unwrap().human=true;
    let hit = |w:&mut Root,slot,attacker| {
        damage_building(w,slot,1e45,Some(Attacker::Unit(attacker)));
    };
    let hostile = systems.iter().find(|s| s.1 == 2 && s.2 > 0).unwrap();
    let friendly = systems.iter().find(|s| s.1 == 1 && s.2 > 0).unwrap();
    hit(&mut w,hostile.0,human);
    w.sim.store.character.get_mut(character).unwrap().team=2.0;
    tick_world(&mut w,dt);
    assert_eq!(telemetry.read().scores[1],hostile.2, "impact credit must survive the killer changing teams");
    w.sim.store.character.get_mut(character).unwrap().team=1.0;
    hit(&mut w,friendly.0,human);
    tick_world(&mut w,dt);
    let mut expected=hostile.2.saturating_sub(friendly.2);
    assert_eq!(telemetry.read().scores[1],expected);
    let personal = |w:&Root, r| w.mission_state().unwrap().1.iter().find(|p| p.character==r).unwrap().points;
    assert_eq!(personal(&w,character),hostile.2 as i32 - friendly.2 as i32);

    revive_soldier(&mut w,victim,victim_character);
    w.sim.store.character.get_mut(victim_character).unwrap().human=true;
    damage_kill_soldier(&mut w,victim,Attacker::Unit(bot));
    tick_world(&mut w,dt);
    expected+=2;
    assert_eq!(telemetry.read().scores[1],expected);
    assert_eq!(personal(&w,bot_character),2);
    w.sim.store.character.get_mut(victim_character).unwrap().human=false;

    // A friendly bot kill reduces personal points while leaving the team total alone.
    w.sim.store.character.get_mut(character).unwrap().human=false;
    damage_kill_soldier(&mut w,human,Attacker::Unit(bot));
    tick_world(&mut w,dt);
    assert_eq!(personal(&w,bot_character),1);
    revive_soldier(&mut w,human,character);
    w.sim.store.character.get_mut(character).unwrap().human=true;
    assert_eq!(telemetry.read().scores[1],expected);
    for &(slot,team,pts) in &systems {
        if team != 2 || pts==0 { continue; }
        if crate::units::f3_props::wire_building_in(&w.units,&w.battle,slot).unwrap().health <= 0.0 { continue; }
        hit(&mut w,slot,human);
        tick_world(&mut w,dt);
        tick_world(&mut w,dt);
        expected+=pts;
        assert_eq!(telemetry.read().scores[1],expected, "slot {slot}, diagnostics {:?}, body {:?}, health {:?}",
            w.mission_state().unwrap().0.host().diagnostics, w.sim.store.unit.get(human).unwrap().character,
            crate::units::f3_props::wire_building_in(&w.units,&w.battle,slot).map(|b| b.health));
    }
    let infrastructure_score = expected;
    while expected < 180 {
        revive_soldier(&mut w,victim,victim_character);
        damage_kill_soldier(&mut w,victim,Attacker::Unit(human));
        tick_world(&mut w,dt);
        expected+=1;
        assert_eq!(telemetry.read().scores[1],expected);
    }
    for _ in 0..420 { if w.sim.store.battle.over {break;} tick_world(&mut w,dt); }
    assert_eq!(w.sim.store.battle.loser,Some(2.0));
    assert!(!telemetry.read().faulted);
    println!("Space systems={} before final soldier kills={infrastructure_score} final={expected} personal={} digest={:016x}",
        systems.len(),personal(&w,character),w.mission_state().unwrap().0.authoritative_digest());
}

fn probe_chain(name: &str, bytes: &[u8]) -> ScriptChain {
    ScriptChain::new(name, vec![ScriptChunk {name:name.into(),bytes:bytes.into()}])
}

#[test]
fn mission_initialization_and_post_load_share_one_work_budget() {
    let make = |budget| {
        let mut rules = ScriptRules::new(probe_chain("trial", FIXTURE_RNG_DET), Default::default()).unwrap();
        // Measure the mission calls after the fixed standard library and host bindings exist.
        rules.host.config.load_instruction_budget = budget;
        rules.vm.set_instruction_budget(budget);
        rules
    };
    let mut measured = make(adapter::ScriptLimits::LOAD_BUDGET);
    measured.run_load().unwrap();
    let entry = measured.vm.work_used();
    measured.run_post_load_in_scope().unwrap();
    let post = measured.vm.work_used();
    assert!(entry > 0 && post > 0);

    let mut exact = make(entry + post);
    exact.run_load().unwrap();
    exact.run_post_load_in_scope().unwrap();
    let mut short = make(entry + post - 1);
    short.run_load().unwrap();
    assert!(matches!(short.run_post_load_in_scope(), Err(ScriptHostError::Execution {
        phase: ScriptPhase::PostLoad, error: ScriptError::InstructionLimit { .. },
    })));
}

#[test]
fn private_host_entry_points_survive_collection() {
    let mut rules = ScriptRules::new(probe_chain("signal", FIXTURE_RNG_DET), Default::default()).unwrap();
    rules.vm.collect_garbage();
    rules.run_load().unwrap();
    rules.vm.collect_garbage();
    rules.run_post_load_in_scope().unwrap();
    rules.vm.collect_garbage();
    rules.vm.call_into(&mut rules.host, ScriptValue::NativeFn(rules.tick_trampoline), &[],
        &mut rules.call_results).unwrap();
}

#[test]
fn cached_message_labels_survive_collection_until_eviction() {
    let mut rules = ScriptRules::new(probe_chain("signal", FIXTURE_RNG_DET), Default::default()).unwrap();
    let show = rules.vm.get_global("ShowMessageText").unwrap();
    let remember = |rules: &mut ScriptRules, text: &str| {
        let id = rules.vm.intern_str(text).unwrap();
        rules.vm.call_into(&mut rules.host, show, &[ScriptValue::String(id)], &mut rules.call_results).unwrap();
        id
    };
    let first = remember(&mut rules, "folding-chair");
    rules.vm.collect_garbage();
    assert_eq!(rules.vm.string_bytes(first), Some(b"folding-chair".as_slice()));
    assert_eq!(remember(&mut rules, "folding-chair"), first);
    for i in 0..super::state::MAX_NOTIFICATIONS {
        remember(&mut rules, &format!("seat-{i}"));
    }
    rules.vm.collect_garbage();
    assert_eq!(rules.host.labels.len(), super::state::MAX_NOTIFICATIONS);
    assert!(rules.vm.string_bytes(first).is_none());
}

#[test]
fn a_reader_can_resync_after_all_retained_notifications_are_drained() {
    let mut host = HostState::new(probe_chain("notice", FIXTURE_RNG_DET), ScriptRulesConfig::strict(13));
    assert_eq!(host.last_notification_sequence(), 0);
    host.push_notification(ScriptNotification::ShowMessageText { key: "open".into(), team: Some(2) });
    assert_eq!(host.notifications_after(0).unwrap().count(), 1);
    let last = host.last_notification_sequence();
    host.drain_notifications();
    assert!(host.notifications_after(0).is_none());
    assert_eq!(host.notifications_after(last).unwrap().count(), 0);
}

#[test]
fn invalid_team_declarations_refuse_at_the_call_and_leave_the_roster_usable() {
    let mut rules = ScriptRules::new(probe_chain("library", FIXTURE_RNG_DET), ScriptRulesConfig::strict(11)).unwrap();
    let label = ScriptValue::String(rules.vm.intern_str("annex_keeper").unwrap());
    let count = ScriptValue::Number(2.0);
    let teams = rules.host.setup.teams.clone();
    let mut results = Vec::new();
    for (op, team, value) in [
        ("SetTeamName", 99.0, label), ("SetTeamIcon", 0.0, label),
        ("SetUnitCount", 8.0, count), ("AddUnitClass", 1.25, label),
        ("SetReinforcementCount", -1.0, count), ("AddReinforcements", 0.0, count),
        ("SetTeamPoints", 8.0, count), ("AddTeamPoints", 0.0, count),
    ] {
        let function = rules.vm.get_global(op).unwrap();
        rules.vm.call_into(&mut rules.host, function, &[ScriptValue::Number(team), value], &mut results).unwrap();
        assert!(matches!(results.as_slice(), [ScriptValue::Bool(false)]));
        let diagnostic = rules.host.diagnostics.errors.last().unwrap();
        assert_eq!(diagnostic.kind, DiagnosticKind::RefusedWrite);
        assert!(diagnostic.message.starts_with(op));
        assert_eq!(rules.host.setup.teams, teams);
    }
    let function = rules.vm.get_global("AddUnitClass").unwrap();
    rules.vm.call_into(&mut rules.host, function, &[ScriptValue::Number(1.0), label, count], &mut results).unwrap();
    assert_eq!(rules.host.setup.teams.len(), teams.len());
    assert_eq!(rules.host.team_setup(1).unwrap().classes[0].class_name, "annex_keeper");
    assert_eq!(rules.host.diagnostics.refused_writes, 8);
}
const LIMIT_PROBE: &[u8] = include_bytes!("../../../tests/lua/11_work_limits.luac");

#[test]
fn callback_errors_and_limits_produce_deterministic_typed_diagnostics() {
    fn run(case:u32) -> (DiagnosticKind,Wide) {
        let mut w = synthetic_world();
        let (a,_) = spawn_soldier(&mut w,1.0,true);
        let (b,_) = spawn_soldier(&mut w,2.0,false);
        let budget = match case { 3 => 4_000_000, 8 => 0, _ => 20_000 };
        let cfg = ScriptRulesConfig {assault_num_bots:Some(case), tick_instruction_budget: budget,
            max_call_depth:32,memory_limit:512*1024,..ScriptRulesConfig::strict(313)};
        let (_,tel,_) = admit_test_script(&mut w,probe_chain("limits",LIMIT_PROBE),cfg,"probe",None,Vec::new());
        assert!(!tel.read().faulted);
        damage_kill_soldier(&mut w,b,Attacker::Unit(a));
        tick_world(&mut w,1.0/60.0);
        if case == 8 {
            // The dispatcher had no allowance to enter a listener.
            tick_world(&mut w, 1.0 / 60.0);
            assert_eq!(tel.read().callback_budget_refusals, 0);
        }
        let rules = w.mission_state().unwrap().0;
        assert!(!tel.read().faulted);
        assert!(!w.sim.store.battle.over);
        if case == 3 {
            assert_eq!(tel.read().callback_budget_refusals, 1);
            assert_eq!(tel.read().callbacks_dropped, 0);
            assert_eq!(rules.host().lifecycle, LifecycleState::Active);
        }
        assert!(w.sim.store.battle.loser.is_none());
        assert_eq!(tel.read().callback_errors, u32::from(case != 8));
        assert!(rules.host().diagnostics.errors.iter().all(|d| d.message.capacity() <= state::MAX_DIAGNOSTIC_BYTES));
        (rules.host().diagnostics.errors.first().unwrap().kind,rules.authoritative_digest())
    }
    use DiagnosticKind::{InstructionLimit, MemoryLimit, RuntimeError, StackOverflow};
    for (i, expected) in [InstructionLimit, StackOverflow, MemoryLimit, RuntimeError,
        RuntimeError, RuntimeError, RuntimeError, InstructionLimit].into_iter().enumerate() {
        let case = i as u32 + 1;
        let outcome = run(case);
        assert_eq!(outcome.0, expected);
        assert_eq!(run(case), outcome);
        println!("callback limit {case} {outcome:?}");
    }
    let chain = probe_chain("short",&LIMIT_PROBE[..20]);
    let mut broken = ScriptRules::new(chain,ScriptRulesConfig::strict(313)).unwrap();
    assert!(broken.run_load().is_err());
    assert_eq!(broken.host().lifecycle,LifecycleState::Faulted);
}

#[test]
fn execution_digest_distinguishes_captured_locals_and_retained_points() {
    let load = |n| {
        let mut r = ScriptRules::new(probe_chain("limits",LIMIT_PROBE), ScriptRulesConfig {assault_num_bots:Some(n),..Default::default()}).unwrap();
        r.run_load().unwrap();
        r.host.config.assault_num_bots = None;
        r
    };
    let mut a=load(12);
    let b=load(19);
    assert_ne!(a.authoritative_digest(),b.authoritative_digest());
    assert_eq!(a.authoritative_digest(),load(12).authoritative_digest());
    assert!(a.host.queue_delivery(state::PendingEvent::TeamPointsChanged {team:1,points:6,seq:1}));
    let first=a.authoritative_digest();
    a.host.pending_events[0]=state::PendingEvent::TeamPointsChanged {team:1,points:17,seq:1};
    assert_ne!(a.authoritative_digest(),first);
}

#[test]
fn named_damage_death_and_repair_remain_allocation_free() {
    let mut w=synthetic_world();
    let (_,beacon)=add_test_building(&mut w,0,"beacon",2.0,50.0);
    let (_,silent)=add_test_building(&mut w,1,"quiet",2.0,50.0);
    let mut rules=ScriptRules::with_goal_handles(probe_chain("beacon",include_bytes!("../../../tests/lua/12_beacon.luac")),Default::default(), w.goal_handles()).unwrap();
    let tel=rules.attach_telemetry();
    rules.run_load().unwrap();
    let params=ModeParameters {command_limit:64,timer_limit:8,score_limit:None,time_limit:None};
    w.prepare_script_rules("beacon",params,rules).unwrap();
    prepare_mission_tail(&mut w);
    let objects=vec![(beacon,"beacon"),(silent,"quiet")].into_iter().map(|(h,n)|NamedObject{object:ObjectHandle::Prop(h),name:Some(n.into()),class:None}).collect();
    f4_tail::select_rules(&mut w,GameFamily::Swbf2,"beacon",params,objects).unwrap();
    tick_world(&mut w,0.0);
    let cycle=|w:&mut Root| {
        for i in 0..=1 {
            damage_building(w,i,7.0,Some(Attacker::Null));
            damage_building(w,i,100.0,Some(Attacker::Null));
        }
        tick_world(w,1.0/60.0);
        for i in 0..=1 { damage_building(w,i,50.0,None); }
        tick_world(w,1.0/60.0);
    };
    cycle(&mut w);
    let before=tel.read().callbacks_executed;
    let allocs=crate::ai::owned::tests::count_rule_allocations(||for _ in 0..24 {cycle(&mut w);});
    assert!(!tel.read().faulted);
    assert_eq!(tel.read().callbacks_executed-before,72);
    assert_eq!(allocs,0);
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_hero_rosters_and_offline_and_network_waves() {
    use crate::world::admit::{mission::*,Bots,Options};
    fn observe(w:&mut Root, network:bool) {
        let dt=1.0/60.0;
        tick_world(w,0.0);
        assert_eq!(w.event_rules().unwrap().score_limit(),Some(180));
        let host = w.mission_state().unwrap().0.host();
        let setup = host.setup.clone();
        let expected = [1, 2].map(|team| setup.teams[&team].classes.iter()
            .map(|class| class.class_name.as_str()).collect::<Vec<_>>());
        let period = if network { 900 } else { v8math::round(setup.spawn_wave_delay.unwrap() * 60.0) as usize };
        let stagger = v8math::round(setup.spawn_slot_delay.unwrap() * 60.0) as usize;
        let mut spawned:[Vec<usize>;2]=Default::default();
        let mut waves=Vec::new();
        let mut previous=w.sim.store.spawning.wave;
        let mut seen=std::collections::BTreeSet::new();
        for tick in 1..=period+7 {
            tick_world(w,dt);
            let s=&w.sim.store;
            if s.spawning.wave!=previous {previous=s.spawning.wave;waves.push(tick);}
            for team in [1usize,2] {
                let roster=&s.spawning.teams[team];
                let declared=&setup.teams[&(team as i32)];
                assert_eq!(roster.unit_count as u32,declared.unit_count.unwrap().min(10));
                assert_eq!(roster.classes.iter().map(|k|String::from_utf16_lossy(w.sim.strings.units(k.name))).collect::<Vec<_>>(),
                    expected[team - 1]);
                let mut counts=vec![0usize;roster.classes.len()];
                for r in &roster.characters {
                    let c=s.character.get(*r).unwrap();
                    let Some(unit)=c.unit.and_then(|u|s.unit.get(u)) else {continue};
                    if !unit.alive {continue}
                    let index=roster.classes.iter().position(|k|k.name==unit.class_name).unwrap();
                    counts[index]+=1;
                    if c.human || !seen.insert((r.slot,r.gen)) {continue}
                    let name=String::from_utf16_lossy(w.sim.strings.units(unit.class_name));
                    assert!(declared.classes.iter().any(|c| c.class_name==name));
                    spawned[team-1].push(tick);
                }
                for (count,class) in counts.iter().zip(&roster.classes) {
                    assert!(*count as f64 <= class.target,"tick {tick} team {team} class {}: {count}/{}",String::from_utf16_lossy(w.sim.strings.units(class.name)),class.target);
                }
                if tick==period {assert!(counts.iter().filter(|n|**n>0).count()>1);}
            }
        }
        assert_eq!(waves,vec![6,6+period]);
        for ticks in &spawned {
            assert!(ticks.len()>2 && ticks[0]==6+stagger && ticks.windows(2).all(|pair|pair[1]-pair[0]>=stagger), "{ticks:?}");
        }
        let people:Vec<_>=w.sim.store.spawning.teams[1].characters.iter().filter_map(|&r|{
            let c=w.sim.store.character.get(r)?;
            Some((r,c.unit?))
        }).take(2).collect();
        let (human,h_unit)=people[0];
        let (bot,b_unit)=people[1];
        w.sim.store.character.get_mut(human).unwrap().human=true;
        w.sim.store.character.get_mut(bot).unwrap().human=false;
        for unit in [h_unit,b_unit] {
            f3::with_services_in(w, |w,services|crate::combat::live::damage_in(w,unit,f64::INFINITY,Attacker::Null,services.unwrap()));
            assert!(w.sim.store.unit.get(unit).unwrap().combat.dead);
        }
        for _ in 0..239 {tick_world(w,dt);}
        for (r,_) in &people {
            let c=w.sim.store.character.get(*r).unwrap();
            assert!(c.unit.is_none());
            assert_eq!(c.spawn.as_ref().unwrap().timer,1.0/60.0);
        }
        tick_world(w,dt);
        for (r,_) in &people {assert_eq!(w.sim.store.character.get(*r).unwrap().spawn.as_ref().unwrap().timer,0.0);}
        crate::battle::spawning::on_human_ready_named(&mut w.sim.store,human,Default::default(),&w.sim.strings,|_|None);
        tick_world(w,dt);
        assert_eq!(w.sim.store.character.get(human).unwrap().unit.is_some(),!network);
        assert!(w.sim.store.character.get(bot).unwrap().unit.is_none());
        let mut replacements=Vec::new();
        for tick in period+7+242..=2*period+6+3*stagger {
            let prior=people.iter().map(|(r,_)|w.sim.store.character.get(*r).unwrap().unit.is_some()).collect::<Vec<_>>();
            tick_world(w,dt);
            for (i,(r,_)) in people.iter().enumerate() {
                if !prior[i] && w.sim.store.character.get(*r).unwrap().unit.is_some() {replacements.push(tick);}
            }
        }
        assert_eq!(replacements.len(),if network{2}else{1});
        assert!(replacements[0]>=2*period+6+stagger);
        println!("Respawn network={network} personal=240 replacements={replacements:?}");
        println!("Mos Eisley network={network} classes={:?}, spawned={spawned:?}, wave={waves:?}",
            [setup.teams[&1].classes.len(),setup.teams[&2].classes.len()]);
    }
    let m=FlatMission{map:"tat2",era:"gcw",mode:"eli",entry:"tat2g_eli",prepared:true};
    let (mut w,_,_)=prepared_world(m,321,10);
    observe(&mut w,false);
    let files=bfe_content::files::DirFiles::from_resolved_root(prepared_root(), bfe_family::GameFamily::Swbf2);
    let ctx=crate::abi::context::Context::new();
    let opt=Options{family:GameFamily::Swbf2, mode:m.mode.into(), practice_target:None, map:m.map.into(),side:"all".into(),enhanced:false,seed:321,heroes:false,unit:None,name:None,
        bots:Bots{fielded:true,tier:3,scale_percent:100,friendly_fire:true}};
    let selection=MissionSelection{map:m.map.into(),era:m.era.into(),mode:m.mode.into(),family:GameFamily::Swbf2,seed:opt.seed};
    let receipt=ctx.world_network_admit_mission(&files,&opt,&selection,MissionPreferences{bots_per_team:Some(10),..Default::default()}).unwrap();
    ctx.world_step_init(receipt.setup).unwrap();
    ctx.owned(|w|observe(w,true)).unwrap();
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn space_controller_kill_and_repair_allocate_nothing_in_the_rules_tick() {
    let m=FlatMission{map:"spa6",era:"cw",mode:"ass",entry:"spa6c_ass",prepared:true};
    let (mut w,_,tel)=prepared_world(m,88,3);
    tick_world(&mut w,0.0);
    let host=w.mission_state().unwrap().0.host();
    // Choose an actual controller with both callbacks, by its admitted identity.
    let controllers: Vec<_>=w.event_rules().unwrap().named_objects().iter().filter_map(|s| {
        let key=Some(crate::battle::rules::objects::key(&s.object));
        if !host.listeners_obj_respawn.iter().any(|l|l.object==key) || !host.listeners_obj_kill.iter().any(|l|l.object==key) {return None}
        let ObjectHandle::Prop(h)=&s.object else{return None};
        Some((s.name.clone()?,crate::units::f3_props::building_of_in(&w.units,h)?))
    }).collect();
    assert!(controllers.len()>=4,"{controllers:?}");
    for (name,slot) in controllers {
    let health=crate::units::f3_props::wire_building_in(&w.units,&w.battle,slot).unwrap().health;
    let cycle=|w:&mut Root|{
        damage_building(w,slot,1e45,Some(Attacker::Null));
        tick_world(w,1.0/60.0);
        damage_building(w,slot,health,None);
        tick_world(w,1.0/60.0);
    };
    for _ in 0..4 {cycle(&mut w);}
    let before=tel.read().callbacks_executed;
    let allocs=crate::ai::owned::tests::count_rule_allocations(||for _ in 0..20 {cycle(&mut w);});
    let callbacks=tel.read().callbacks_executed-before;
    assert!(callbacks>=40 && !tel.read().faulted,"{name} callbacks={callbacks} {:?}",w.mission_state().unwrap().0.host().diagnostics);
    assert_eq!(allocs,0,"{name} callbacks={callbacks}");
    println!("Space controller {name} kill/repair x20: callbacks={callbacks} allocations={allocs}");
    }
}


#[test]
fn occupied_craft_and_infantry_have_distinct_native_awards() {
    use crate::battle::rules::awards::Destroyed::*;
    for (kind,expected) in [
        (Soldier{human:false,hero:false},1),(Soldier{human:true,hero:false},2),(Soldier{human:false,hero:true},5),
        (OccupiedVehicle{victim_kind:4,killer_kind:0},3),(OccupiedVehicle{victim_kind:4,killer_kind:4},3),
        (OccupiedVehicle{victim_kind:4,killer_kind:5},3),(OccupiedVehicle{victim_kind:1,killer_kind:2},1),
        (OccupiedVehicle{victim_kind:2,killer_kind:2},2),(OccupiedVehicle{victim_kind:8,killer_kind:2},10),
        (SelfPilotedTurret,1),(EligibleEmptyVehicle,1),(Other,0),
    ] {assert_eq!(kind.points(),expected);}
}

#[test]
fn an_eligible_empty_fleet_hull_awards_only_its_enemy() {
    for (owner, eligible, attributed, points) in [
        (2.0, true, true, 1), (2.0, false, true, 0), (1.0, true, true, 0),
        (0.0, true, true, 0), (2.0, true, false, 0),
    ] {
        let mut world = synthetic_world();
        let (rider, _) = spawn_soldier(&mut world, 2.0, false);
        let (shooter, player) = spawn_soldier(&mut world, 1.0, true);
        let vehicle = f4_tail::rule_tests::crewed_hull(&mut world, rider);
        let hull = world.sim.store.vehicle.get_mut(vehicle).unwrap();
        hull.team = Some(owner);
        hull.seats[0].lease = None;
        world.sim.store.unit.get_mut(rider).unwrap().seat = None;
        let mut rules = ScriptRules::with_goal_handles(probe_chain("hull", FIXTURE_RNG_DET), ScriptRulesConfig::strict(31), world.goal_handles()).unwrap();
        rules.run_load().unwrap();
        rules.host.match_flags.space_assault_enabled = true;
        let params = ModeParameters { command_limit: DEFAULT_SCRIPT_COMMAND_LIMIT, timer_limit: DEFAULT_SCRIPT_TIMER_LIMIT, ..Default::default() };
        world.prepare_script_rules("ass", params, rules).unwrap();
        prepare_mission_tail(&mut world);
        f4_tail::select_rules(&mut world, GameFamily::Swbf2, "ass", params, Vec::new()).unwrap();
        tick_world(&mut world, 0.0);
        world.set_mission_empty_vehicle_eligible(vehicle, eligible).unwrap();
        let attacker = if attributed { OrdnanceOwner::Unit(shooter) } else { OrdnanceOwner::Null };
        world.tick.running = true;
        for _ in 0..2 {
            f3::with_services_in(&mut world, |w, services| {
                crate::units::f3_hulls::mutate_in(w, vehicle, 200.0, Some(attacker), services.unwrap());
            });
        }
        tick_world(&mut world, 1.0 / 60.0);
        assert!(!world.sim.store.vehicle.get(vehicle).unwrap().alive, "owner={owner} eligible={eligible} attributed={attributed}");
        assert_eq!(world.event_rules().unwrap().team_points()[1], points);
        let personal = world.mission_state().unwrap().1.iter().find(|p| p.character == player);
        assert_eq!(personal.map_or(0, |p| p.points), points as i32);
    }
}


#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_hero_roster_and_awarded_health() {
    let m=FlatMission{map:"tat2",era:"gcw",mode:"eli",entry:"tat2g_eli",prepared:true};
    let (mut w,_,_)=prepared_world(m,71,16);
    let dt=1.0/60.0;
    for _ in 0..270 {tick_world(&mut w,dt);}
    let mut heroes=Vec::new();
    let mut opponents=Vec::new();
    for &character in &w.sim.store.spawning.characters {
        let c=w.sim.store.character.get(character).unwrap();
        let Some(r)=c.unit else{continue};
        let u=w.sim.store.unit.get(r).unwrap();
        assert!(!u.hero && c.hero != Some(true), "a roster class must spawn as an ordinary combatant");
        let class=String::from_utf16_lossy(w.sim.strings.units(u.class_name));
        if class.ends_with("obiwan") || class.ends_with("macewindu") {
            heroes.push((class,r,character,u.combat.health));
        } else if c.team==2.0 {opponents.push(r);}
    }
    assert!(heroes.iter().any(|h|h.0.ends_with("obiwan")) && heroes.iter().any(|h|h.0.ends_with("macewindu")),"{heroes:?}");
    for (_,_,r,_) in &heroes {w.sim.store.character.get_mut(*r).unwrap().human=true;}
    for _ in 0..(26*60) {tick_world(&mut w,dt);}
    for (_,body,_,health) in &heroes {assert_eq!(w.sim.store.unit.get(*body).unwrap().combat.health,*health);}
    // An award is a character role assigned by other modes. Use the admitted
    // bodies to exercise its health policy independently of their roster role.
    for (_,_,r,_) in &heroes {w.sim.store.character.get_mut(*r).unwrap().hero=Some(true);}
    for _ in 0..60 {tick_world(&mut w,dt);}
    for (_,body,_,health) in &heroes {
        let loss=health-w.sim.store.unit.get(*body).unwrap().combat.health;
        assert!((loss-9.9).abs()<=0.4,"one-second drain {loss}");
    }
    let mace=heroes.iter().find(|h|h.0.ends_with("macewindu")).unwrap().1;
    let combat=&mut w.sim.store.unit.get_mut(mace).unwrap().combat;
    combat.health=combat.max_health-320.0;
    for &victim in &opponents[..2] {
        let before=w.sim.store.unit.get(mace).unwrap().combat.health;
        f3::with_services_in(&mut w,|w,services|crate::combat::live::damage_in(w,victim,f64::INFINITY,Attacker::Unit(mace),services.unwrap()));
        let gain=w.sim.store.unit.get(mace).unwrap().combat.health-before;
        assert!((gain-150.0).abs()<=12.0,"credited kill recovery {gain}");
    }
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_republic_bridge_and_sensor_awards() {
    let m=FlatMission{map:"spa6",era:"cw",mode:"ass",entry:"spa6c_ass",prepared:true};
    let (mut w,_,_)=prepared_world(m,15,4);
    for _ in 0..120 {tick_world(&mut w,1.0/60.0);}
    let mut systems=Vec::new();
    for system in &w.mission_state().unwrap().0.host().critical_systems {
        if !(system.name.contains("bridge") || system.name.contains("sensor")) {continue;}
        let ObjectHandle::Prop(h)=&system.object else{continue};
        let id=crate::units::f3_props::building_of_in(&w.units,h).unwrap();
        if crate::units::f3_props::rule_team(&w.units,id)==1.0 {systems.push(id);}
    }
    assert_eq!(systems.len(),2);
    let pilot=w.sim.store.spawning.teams[2].characters.iter().find_map(|r|w.sim.store.character.get(*r)?.unit).unwrap();
    let raw=std::fs::read(prepared_root().join("maps/spa6/map.json")).unwrap();
    let map:serde_json::Value=serde_json::from_slice(&raw).unwrap();
    let mut guns=[const { None };3];
    for row in w.event_rules().unwrap().named_objects().iter() {
        let (Some(class),ObjectHandle::Prop(h))=(&row.class,&row.object) else{continue};
        let Some(properties)=map["classes"][class]["properties"].as_array() else{continue};
        let pilot_type=properties.iter().find_map(|p|(p[0]=="PilotType").then(||p[1].as_str()).flatten());
        let Some(index)=crate::units::f3_props::building_of_in(&w.units,h) else {continue};
        let team=crate::units::f3_props::rule_team(&w.units,index) as i32;
        match (pilot_type,team) {
            (Some("self"),1)=>{guns[0].get_or_insert((index,row.object.clone()));}
            (Some("self"),2)=>{guns[1].get_or_insert((index,row.object.clone()));}
            (Some("remote"),_)=>{guns[2].get_or_insert((index,row.object.clone()));}
            _=>()
        }
    }
    for (slot,(gun,award)) in guns.into_iter().zip([1,0,0]).enumerate() {
        let before=w.event_rules().unwrap().team_points()[2];
        let (id,handle)=gun.expect("admitted turret of the requested pilot type and affiliation");
        if slot==2 {
            w.change_mission_system(&handle,crate::battle::rules::objects::ObjectProperty::Team(1.0)).unwrap();
        }
        damage_building(&mut w,id,1e45,Some(Attacker::Unit(pilot)));
        tick_world(&mut w,1.0/60.0);
        assert_eq!(w.event_rules().unwrap().team_points()[2],before+award);
    }
    for id in systems {
        let before=w.event_rules().unwrap().team_points()[2];
        damage_building(&mut w,id,1e45,Some(Attacker::Unit(pilot)));
        tick_world(&mut w,1.0/60.0);
        assert_eq!(w.event_rules().unwrap().team_points()[2],before+18);
    }
}

#[test]
fn goal_reader_preserves_slot_order_handles_and_raw_subjects() {
    use crate::ai::goals::{GoalArg, GoalKind, GoalTable, GOAL_SLOTS};

    let mut world = synthetic_world();
    let script = include_bytes!("../../../tests/lua/13_shelves.luac");
    let fresh = ScriptRules::new(probe_chain("shelves", script), ScriptRulesConfig::strict(907)).unwrap();
    assert!(fresh.host().is_empty());
    assert_eq!(fresh.host().revision(), 0);
    assert_eq!(fresh.host().last_notification_sequence(), 0);
    assert_eq!(fresh.host().notifications_after(0).unwrap().count(), 0);
    let (_, telemetry, _) = admit_test_script(
        &mut world, probe_chain("shelves", script), ScriptRulesConfig::strict(907),
        "library", None, Vec::new(),
    );
    assert!(!telemetry.read().faulted);
    let goals = world.event_rules().unwrap().ai_goals().unwrap();
    assert_eq!((goals.revision(), goals.len()), (22, GOAL_SLOTS));
    for index in 0..GOAL_SLOTS {
        let row = goals.row(index);
        assert_eq!(usize::from(row.slot), index);
        if index == 4 {
            assert_eq!((row.handle, row.team, row.kind, row.weight), (21, 3, GoalKind::CtfOffense, 3.5));
            assert_eq!(row.target, GoalArg::Name(&[233]));
            assert_eq!(row.extra, GoalArg::Name(b"index"));
        } else {
            assert_eq!(row.handle as usize, index + 1);
            assert_eq!(row.team, if index < 10 { 0 } else { 7 });
            assert_eq!(row.kind, GoalKind::Defend);
            assert_eq!(row.weight, (index + 1) as f32 * 0.25);
            assert_eq!(row.target, GoalArg::Name(b"oak"));
        }
    }
    for _ in 0..5 {
        tick_world(&mut world, 1.0 / 60.0);
    }
    let goals = world.event_rules().unwrap().ai_goals().unwrap();
    assert_eq!((goals.revision(), goals.len()), (23, 10));
    assert_eq!(goals.row(4).handle, 21);
}

/// Read the shielded native buildings installed by this mission.
fn capital_sections(w: &Root) -> [Vec<(ObjectHandle, usize)>; 2] {
    let mut sides: [Vec<_>; 2] = Default::default();
    for system in w.event_rules().unwrap().named_objects().iter() {
        let Ok(object) = w.current_mission_object(&system.object) else { continue };
        let ObjectHandle::Prop(handle) = &object else { continue };
        let (_, maximum, _) = w.mission_system_shields(&object).unwrap();
        if maximum > 0.0 {
            let team = w.mission_system_state(&object).unwrap().team.unwrap() as usize;
            let slot = crate::units::f3_props::building_of_in(&w.units, handle).unwrap();
            sides[team - 1].push((object.clone(), slot));
        }
    }
    assert!(sides.iter().all(|parts| parts.len() >= 2), "{:?}", w.mission_state().unwrap().0.host().diagnostics);
    sides
}

fn space_mission() -> FlatMission<'static> {
    FlatMission { map: "spa6", era: "cw", mode: "ass", entry: "spa6c_ass", prepared: true }
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_space_both_sides_reach_the_target_deterministically() {
    fn run(winner: usize) -> (u32, Wide, Wide) {
        let (mut world, _, telemetry) = prepared_world(space_mission(), 777, 3);
        let loser = 3 - winner;
        let mut summary = Fnv::new();
        let mut step = |world: &mut Root| {
            tick_world(world, 1.0 / 60.0);
            summary.bytes(&capture_match_summary_bytes(world));
        };
        for _ in 0..180 { step(&mut world); }
        let body = |team: usize| world.sim.store.spawning.teams[team].characters.iter().find_map(|&r| {
            Some((world.sim.store.character.get(r)?.unit?, r))
        }).unwrap();
        let (pilot, player) = body(winner);
        let (infantry, victim) = body(loser);
        world.sim.store.character.get_mut(player).unwrap().human = true;
        // The scorer's present life must be human too: the floor that keeps bots from finishing
        // a frigate (its AI damage threshold) reads the unit, which keeps the flag it spawned
        // with. The Republic's first soldier is the local player; the Separatists' is a bot.
        world.sim.store.unit.get_mut(pilot).unwrap().human = true;
        world.sim.store.character.get_mut(victim).unwrap().human = false;
        // A normal spawn copies the character's ownership into the unit life.
        // The fixture assigns its two roles after the units have spawned.
        world.sim.store.unit.get_mut(pilot).unwrap().human = true;
        world.sim.store.unit.get_mut(infantry).unwrap().human = false;
        let targets: Vec<_> = world.mission_state().unwrap().0.host().critical_systems.iter()
            .filter(|row| row.points > 0 && world.mission_system_state(&row.object).unwrap().team == Some(loser as f64))
            .map(|row| row.object.clone()).collect();
        for object in targets {
            let life = world.current_mission_object(&object).unwrap();
            if !world.mission_system_state(&life).unwrap().alive { continue; }
            let ObjectHandle::Prop(handle) = life else { panic!("expected a native building") };
            let slot = crate::units::f3_props::building_of_in(&world.units, &handle).unwrap();
            damage_building(&mut world, slot, 1e45, Some(Attacker::Unit(pilot)));
            step(&mut world);
            step(&mut world);
        }
        assert_eq!(telemetry.read().scores[winner], 144);
        let target = world.event_rules().unwrap().score_limit().unwrap();
        assert_eq!(target, 180);
        for score in 145..=target {
            revive_soldier(&mut world, infantry, victim);
            damage_kill_soldier(&mut world, infantry, Attacker::Unit(pilot));
            step(&mut world);
            assert_eq!(telemetry.read().scores[winner], score);
        }
        let reached = world.mission_state().unwrap().0.host().tick;
        for _ in 0..420 {
            if world.sim.store.battle.over { break; }
            step(&mut world);
        }
        assert!(!telemetry.read().faulted);
        assert_eq!(world.sim.store.battle.loser, Some(loser as f64));
        assert_eq!(world.mission_state().unwrap().0.host().tick - reached, 301);
        (world.mission_state().unwrap().0.host().tick, world.mission_digest().unwrap(), summary.finish())
    }
    for winner in [1, 2] {
        let result = run(winner);
        assert_eq!(result, run(winner));
        println!("Space winner={winner} target=180 tick={} mission={:016x} summary={:016x}", result.0, result.1, result.2);
    }
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_capital_shield_write_order() {
    use rules::objects::ObjectProperty::{AddShield, CurrentShield, MaxShield};
    let (mut w, _, _) = prepared_world(space_mission(), 47, 3);
    tick_world(&mut w, 0.0);
    let sides = capital_sections(&w);
    for parts in &sides {
        for (h, _) in parts {
            let (charge, maximum, rate) = w.mission_system_shields(h).unwrap();
            assert!(rate > 0.0);
            assert_eq!(charge, maximum, "mission setup must leave the capital shields full");
        }
    }
    let (section, slot) = &sides[0][0];
    let health = w.mission_system_state(section).unwrap().health;
    let (before, _, _) = w.mission_system_shields(section).unwrap();
    damage_building(&mut w, *slot, before / 40.0, Some(Attacker::Null));
    tick_world(&mut w, 1.0 / 60.0);
    assert_eq!(w.mission_system_state(section).unwrap().health, health);
    let after = w.mission_system_shields(section).unwrap();
    assert!(after.0 > 0.0 && after.0 < before);
    for (h, _) in &sides[0] {
        assert_eq!(w.mission_system_shields(h).unwrap(), after);
    }

    for order in [[CurrentShield(61.0), MaxShield(93.0)], [MaxShield(93.0), CurrentShield(61.0)]] {
        for property in [CurrentShield(0.0), MaxShield(0.0), AddShield(0.0)] {
            w.change_mission_system(section, property).unwrap();
        }
        for property in order { w.change_mission_system(section, property).unwrap(); }
        assert_eq!(w.mission_system_shields(section).unwrap(), (61.0, 93.0, 0.0));
        w.change_mission_system(section, MaxShield(97.0)).unwrap();
        assert_eq!(w.mission_system_shields(section).unwrap().0, 61.0);
    }
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_capital_shield_damage_batches() {
    use rules::objects::ObjectProperty::CurrentShield;
    let (mut w, _, telemetry) = prepared_world(space_mission(), 73, 3);
    tick_world(&mut w, 0.0);
    let sides = capital_sections(&w);
    for (h, _) in sides.iter().flatten() {
        let (_, maximum, _) = w.mission_system_shields(h).unwrap();
        w.change_mission_system(h, CurrentShield(maximum)).unwrap();
    }
    for hits in [30, 100, 257, 257, 257, 257, 257, 257, 257, 257, 257, 257] {
        let before = telemetry.read();
        let initial = sides.each_ref().map(|parts| w.mission_system_shields(&parts[0].0).unwrap().0);
        for (_, slot) in sides.iter().flatten() {
            for _ in 0..hits { damage_building(&mut w, *slot, 1.0, Some(Attacker::Null)); }
        }
        tick_world(&mut w, 1.0 / 60.0);
        let now = telemetry.read();
        assert!(!now.faulted, "{hits} hits per section faulted the mission");
        assert_eq!(before.refused_writes, now.refused_writes);
        assert_eq!(now.callbacks_executed - before.callbacks_executed, sides.iter().map(Vec::len).sum::<usize>() as u32);
        for (side, parts) in sides.iter().enumerate() {
            let triple = w.mission_system_shields(&parts[0].0).unwrap();
            let expected = initial[side] - (parts.len() * hits) as f64 + triple.2 / 60.0;
            assert!((triple.0 - expected).abs() <= triple.1 * f64::from(f32::EPSILON),
                "each linked part's absorbed damage must survive propagation: {triple:?}, expected {expected}");
            for (h, _) in parts { assert_eq!(w.mission_system_shields(h).unwrap(), triple); }
        }
    }

    // Scratch one section before breaking another in the same batch. The earlier
    // callback must neither recharge the broken shield nor erase the down edge.
    for parts in &sides {
        let (h, slot) = &parts[1];
        let health = w.mission_system_state(h).unwrap().health;
        let charge = w.mission_system_shields(h).unwrap().0;
        damage_building(&mut w, parts[0].1, 3.0, Some(Attacker::Null));
        damage_building(&mut w, *slot, charge, Some(Attacker::Null));
        assert_eq!(w.mission_system_state(h).unwrap().health, health);
    }
    for _ in 0..2 { tick_world(&mut w, 1.0 / 60.0); }
    assert!(!telemetry.read().faulted);
    for (h, _) in sides.iter().flatten() {
        let (charge, _, rate) = w.mission_system_shields(h).unwrap();
        assert_eq!((charge, rate), (0.0, 0.0));
    }
    let generators: Vec<_> = w.event_rules().unwrap().named_objects().iter()
        .filter(|system| system.name.as_deref().is_some_and(|name| name.ends_with("shieldgen")))
        .map(|system| w.current_mission_object(&system.object).unwrap()).collect();
    assert_eq!(generators.len(), sides.len());
    for generator in generators {
        let state = w.mission_system_state(&generator).unwrap();
        assert!(!state.alive);
        let ObjectHandle::Prop(h) = generator else { panic!("generator must be a building") };
        let slot = crate::units::f3_props::building_of_in(&w.units, &h).unwrap();
        damage_building(&mut w, slot, state.health.unwrap().1, None);
    }
    for _ in 0..2 { tick_world(&mut w, 1.0 / 60.0); }
    for (h, _) in sides.iter().flatten() {
        let (charge, maximum, rate) = w.mission_system_shields(h).unwrap();
        assert!(rate > 0.0);
        assert_eq!(charge, maximum);
    }
    assert!(!telemetry.read().faulted);
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_downed_capital_shield_under_sustained_fire() {
    let (mut w, _, telemetry) = prepared_world(space_mission(), 619, 8);
    tick_world(&mut w, 0.0);
    let sides = capital_sections(&w);
    let (section, slot) = &sides[1][0];
    let charge = w.mission_system_shields(section).unwrap().0;
    damage_building(&mut w, *slot, charge, Some(Attacker::Null));
    for _ in 0..3 { tick_world(&mut w, 1.0 / 60.0); }
    assert_eq!(w.mission_system_shields(section).unwrap().0, 0.0);
    let before = telemetry.read();
    for _ in 0..600 {
        for (parts, damage) in sides.iter().zip([30.0, 1.0]) {
            for (_, slot) in parts { damage_building(&mut w, *slot, damage, Some(Attacker::Null)); }
        }
        tick_world(&mut w, 1.0 / 60.0);
        for parts in &sides {
            let expected = w.mission_system_shields(&parts[0].0).unwrap();
            for (h, _) in parts { assert_eq!(w.mission_system_shields(h).unwrap(), expected); }
        }
    }
    let host = w.mission_state().unwrap().0.host();
    let after = telemetry.read();
    assert!(!after.faulted);
    assert_eq!(after.refused_writes, before.refused_writes);
    assert_eq!(host.diagnostics.refused_commands, 0);
    let [up, down] = sides.each_ref().map(Vec::len);
    let simultaneous = up * up + 2 * down * down;
    assert!(host.diagnostics.peak_commands >= simultaneous);
    println!("Sustained fire: callbacks={} peak commands={} refused commands={}",
        after.callbacks_executed - before.callbacks_executed, host.diagnostics.peak_commands,
        host.diagnostics.refused_commands);

    let (section, slot) = &sides[0][0];
    let charge = w.mission_system_shields(section).unwrap().0;
    damage_building(&mut w, *slot, charge, Some(Attacker::Null));
    for _ in 0..3 { tick_world(&mut w, 1.0 / 60.0); }
    for (_, slot) in sides.iter().flatten() {
        damage_building(&mut w, *slot, 1.0, Some(Attacker::Null));
    }
    tick_world(&mut w, 1.0 / 60.0);
    for (section, _) in sides.iter().flatten() {
        assert_eq!(w.mission_system_shields(section).unwrap().0, 0.0);
    }
    let d = &w.mission_state().unwrap().0.host().diagnostics;
    assert!(d.peak_commands >= 2 * (up * up + down * down));
    assert_eq!(d.refused_commands, 0);
    println!("Both pools depleted: peak commands={} refused commands={}", d.peak_commands, d.refused_commands);
}

#[test]
#[ignore = "requires prepared 2005 mission content"]
fn prepared_capital_shield_callback_allocations() {
    let (mut w, _, telemetry) = prepared_world(space_mission(), 41, 3);
    tick_world(&mut w, 0.0);
    let sides = capital_sections(&w);
    let step = |w: &mut Root| {
        for (_, slot) in sides.iter().flatten() {
            for _ in 0..128 { damage_building(w, *slot, 1.0, Some(Attacker::Null)); }
        }
        tick_world(w, 1.0 / 60.0);
    };
    for _ in 0..3 { step(&mut w); }
    let before = telemetry.read();
    let allocations = crate::ai::owned::tests::count_rule_allocations(|| step(&mut w));
    let after = telemetry.read();
    let calls = after.callbacks_executed - before.callbacks_executed;
    assert!(!after.faulted);
    assert_eq!(calls as usize, sides.iter().map(Vec::len).sum::<usize>());
    assert_eq!(calls, 18);
    println!("Capital shield callbacks={calls} rules allocations={allocations}");
    assert_eq!(allocations, 0);
}

#[test]
fn a_failed_kill_listener_leaves_the_match_playable() {
    let mut failures = Vec::new();
    for (case, kind) in [(7, DiagnosticKind::RuntimeError), (1, DiagnosticKind::InstructionLimit)] {
        let mut w = synthetic_world();
        let (shooter, _) = spawn_soldier(&mut w, 1., true);
        let (victim, _) = spawn_soldier(&mut w, 2., false);
        let cfg = ScriptRulesConfig { assault_num_bots: Some(case), tick_instruction_budget: 20_000,
            ..ScriptRulesConfig::strict(73) };
        let (_, telemetry, _) = admit_test_script(&mut w, probe_chain("limits", LIMIT_PROBE), cfg, "probe", None, Vec::new());
        damage_kill_soldier(&mut w, victim, Attacker::Unit(shooter));
        tick_world(&mut w, 1./60.);
        let host = w.mission_state().unwrap().0.host();
        assert!(host.diagnostics.errors.iter().any(|err| err.kind == kind));
        let before = host.tick;
        if telemetry.read().faulted || w.sim.store.battle.over {
            failures.push(format!("recoverable {kind:?} ended the match at tick {before}"));
            continue;
        }
        tick_world(&mut w, 1./60.);
        assert_eq!(w.mission_state().unwrap().0.host().tick, before+1);
        assert!(!w.sim.store.battle.over);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_unusable_mission_ends_at_the_native_standings() {
    use rules::events::{Event, EventQueue, EventRules, Outcome, RuleActions, RuleResult};
    struct Table {
        scores: [u32; 2],
        result: Option<Outcome>,
    }
    impl RuleActions for Table {
        fn score(&self, team: f64) -> Option<u32> {
            (team == 1. || team == 2.).then(|| self.scores[team as usize - 1])
        }
        fn score_limit(&self) -> Option<u32> { Some(12) }
        fn start_timer(&mut self, _:crate::store::StrId, _:f64) -> bool { true }
        fn stop_timer(&mut self, _:crate::store::StrId) -> bool { true }
        fn finish(&mut self, outcome: Outcome) -> RuleResult<()> { self.result = Some(outcome); Ok(()) }
    }
    let mut outcomes = Vec::new();
    for scores in [[12, 7], [4, 9], [6, 6]] {
        let mut table = Table {scores,result:None};
        let mut rules = ScriptRules::new(probe_chain("limits",LIMIT_PROBE),ScriptRulesConfig::strict(845)).unwrap();
        rules.tick(0., &EventQueue::new(1), &mut table);
        // A host fault leaves no usable mission state; the battle ends on the native standings.
        rules.host.diagnostics.faulted = true;
        let mut delivery = EventQueue::new(1);
        delivery.push(Event::TeamPointsChanged {team:1.,points:scores[0]});
        rules.tick(1./60., &delivery, &mut table);
        assert!(rules.host().diagnostics.faulted);
        outcomes.push(table.result);
    }
    assert_eq!(outcomes,[Some(Outcome::Winner(1.)),Some(Outcome::Winner(2.)),Some(Outcome::Draw)]);
}

const TALLY_CALLBACKS: &[u8] = include_bytes!("../../../tests/lua/15_callbacks.luac");

#[test]
fn script_errors_keep_applied_effects_and_count_each_site() {
    for fixture in [TALLY_CALLBACKS, include_bytes!("../../../tests/lua/15_callbacks.stripped.luac").as_slice()] {
        let mut w = synthetic_world();
        let (shooter, _) = spawn_soldier(&mut w, 1.0, true);
        let (unit, character) = spawn_soldier(&mut w, 2.0, false);
        let cfg = ScriptRulesConfig { assault_num_bots: Some(1), ..ScriptRulesConfig::strict(23) };
        let (_, telemetry, inspector) = admit_test_script(&mut w, probe_chain("tally", fixture), cfg, "tally", None, vec![]);
        for _ in 0..6 {
            revive_soldier(&mut w, unit, character);
            damage_kill_soldier(&mut w, unit, Attacker::Unit(shooter));
            tick_world(&mut w, 1.0 / 60.0);
        }
        let t = telemetry.read();
        assert_eq!(&t.scores[1..3], &[6, 6]);
        assert_eq!((t.callback_errors, t.callbacks_dropped, t.faulted), (12, 0, false));
        let sites = inspector.snapshot().diagnostics.errors;
        assert_eq!(sites.len(), 3);
        assert!(sites.iter().all(|d| d.site.is_some() && d.message.starts_with("tally function ")));
        assert!(sites.iter().all(|d| d.kind == DiagnosticKind::RuntimeError));
        let receipt = sites.iter().find(|d| d.message.contains("receipt")).unwrap();
        let parcel = sites.iter().find(|d| d.message.contains("parcel")).unwrap();
        assert_eq!(receipt.listener_id, parcel.listener_id);
        assert_eq!((receipt.occurrences, parcel.occurrences), (3, 3));
        assert_eq!(receipt.site.unwrap().function, parcel.site.unwrap().function);
        assert_ne!(receipt.site.unwrap().instruction, parcel.site.unwrap().instruction);
        assert_eq!(sites.iter().find(|d| d.listener_id != receipt.listener_id).unwrap().occurrences, 6);
        assert!(!w.sim.store.battle.over);
    }
}

#[test]
fn only_consecutive_budget_failures_disable_the_responsible_listener() {
    let mut w = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut w, 1.0, true);
    let (unit, character) = spawn_soldier(&mut w, 2.0, false);
    let cfg = ScriptRulesConfig { assault_num_bots: Some(2), tick_native_ops_budget: 12, ..ScriptRulesConfig::strict(24) };
    let (_, telemetry, inspector) = admit_test_script(&mut w, probe_chain("tally", TALLY_CALLBACKS), cfg, "tally", None, vec![]);
    for visit in 1..=8 {
        revive_soldier(&mut w, unit, character);
        damage_kill_soldier(&mut w, unit, Attacker::Unit(shooter));
        tick_world(&mut w, 1.0 / 60.0);
        tick_world(&mut w, 1.0 / 60.0);
        let t = telemetry.read();
        assert_eq!(&t.scores[1..3], &[visit.min(6), visit]);
        assert_eq!(t.callbacks_dropped, u32::from(visit >= 6));
        assert_eq!(t.callback_budget_refusals, visit.min(6) - u32::from(visit >= 3));
        assert!(!t.faulted && !w.sim.store.battle.over);
    }
    let errors = inspector.snapshot().diagnostics.errors;
    assert_eq!(errors.iter().find(|d| d.kind == DiagnosticKind::InstructionLimit).unwrap().occurrences, 5);
    assert_eq!(errors.iter().filter(|d| d.kind == DiagnosticKind::ListenerDisabled).count(), 1);
}

#[test]
fn a_failed_timer_listener_cannot_turn_a_two_point_win_into_a_draw() {
    let mut w = synthetic_world();
    let cfg = ScriptRulesConfig { assault_num_bots: Some(3), ..ScriptRulesConfig::strict(25) };
    let (_, telemetry, inspector) = admit_test_script(&mut w, probe_chain("tally", TALLY_CALLBACKS), cfg, "tally", Some(2), vec![]);
    for _ in 0..6 { tick_world(&mut w, 1.0 / 60.0); }
    assert_eq!(&telemetry.read().scores[1..3], &[2, 0]);
    assert_eq!(telemetry.read().callback_errors, 1);
    assert!(!telemetry.read().faulted);
    assert_eq!(inspector.snapshot().outcome, Some(BattleOutcome::Victory(1)));
    assert!(w.sim.store.battle.over);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
}

#[test]
fn callback_memory_errors_preserve_writes_and_scripted_results() {
    for (mode, target, scores) in [(4, 10, [0, 1]), (5, 2, [2, 5]), (6, 20, [2, 2]), (7, 20, [4, 0])] {
        let mut w = synthetic_world();
        let (shooter, _) = spawn_soldier(&mut w, 1.0, true);
        let (unit, _) = spawn_soldier(&mut w, 2.0, false);
        let cfg = ScriptRulesConfig { assault_num_bots: Some(mode), tick_instruction_budget: 4_000_000,
            memory_limit: 512 * 1024, ..ScriptRulesConfig::strict(26) };
        let (_, telemetry, inspector) = admit_test_script(&mut w, probe_chain("tally", TALLY_CALLBACKS), cfg, "tally", Some(target), vec![]);
        damage_kill_soldier(&mut w, unit, Attacker::Unit(shooter));
        tick_world(&mut w, 1.0 / 60.0);
        let snap = inspector.snapshot();
        assert!(!telemetry.read().faulted);
        assert!(snap.diagnostics.errors.iter().any(|d| d.kind == DiagnosticKind::MemoryLimit && d.listener_id.is_some()));
        assert_eq!(snap.outcome, (mode == 7).then_some(BattleOutcome::Victory(2)), "mode {mode}");
        assert_eq!(w.sim.store.battle.over, mode == 7);
        assert_eq!(telemetry.read().scores[1..3], scores);
    }
}

#[test]
fn an_invalid_runtime_root_preserves_the_native_result_or_retries_the_original_standings() {
    use rules::events::{EventQueue, EventRules, Outcome, RuleActions, RuleError, RuleResult};
    struct Scoreboard { scores: [u32; 2], decided: Option<Outcome>, refuse: bool }
    impl RuleActions for Scoreboard {
        fn score(&self, team: f64) -> Option<u32> { match team { 1.0 => Some(self.scores[0]), 2.0 => Some(self.scores[1]), _ => None } }
        fn committed_outcome(&self) -> Option<Outcome> { self.decided }
        fn start_timer(&mut self, _: crate::store::StrId, _: f64) -> bool { true }
        fn stop_timer(&mut self, _: crate::store::StrId) -> bool { true }
        fn finish(&mut self, result: Outcome) -> RuleResult<()> {
            if std::mem::take(&mut self.refuse) { return Err(RuleError::LimitReached); }
            self.decided = Some(result);
            Ok(())
        }
    }
    for decided in [Some(Outcome::Loser(1.0)), None] {
        let cfg = ScriptRulesConfig { assault_num_bots: Some(3), ..ScriptRulesConfig::strict(27) };
        let mut script = ScriptRules::new(probe_chain("tally", TALLY_CALLBACKS), cfg).unwrap();
        let queue = EventQueue::new(8);
        let mut actions = Scoreboard { scores: [2, 0], decided, refuse: true };
        script.tick(0.0, &queue, &mut actions);
        let listener = &script.host.listener_bucket(state::ListenerFamily::TimerElapse)[0];
        let (timer_id, root) = (listener.timer_id, listener.callback);
        let arm_gen = script.host.timer(timer_id).unwrap().arm_gen;
        script.vm.release_ref(root);
        script.host.pending_events.push_back(state::PendingEvent::TimerElapsed { timer_id, arm_gen });
        script.tick(0.05, &queue, &mut actions);
        assert!(script.host.diagnostics.errors.iter().any(|d| d.kind == DiagnosticKind::InternalFault));
        assert!(script.host.diagnostics.faulted);
        if decided.is_some() {
            assert_eq!(script.host.outcome, Some(BattleOutcome::Defeat(1)));
        } else {
            assert_eq!(script.host.outcome, None);
            actions.scores = [0, 10];
            script.tick(0.0, &queue, &mut actions);
            assert_eq!(actions.decided, Some(Outcome::Winner(1.0)));
            assert_eq!(script.host.outcome, Some(BattleOutcome::Victory(1)));
        }
    }
}

#[test]
fn a_small_command_limit_records_every_refusal() {
    let mut w = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut w, 1.0, true);
    let (unit, character) = spawn_soldier(&mut w, 2.0, false);
    let cfg = ScriptRulesConfig { assault_num_bots: Some(1), ..ScriptRulesConfig::strict(28) };
    let mut script = ScriptRules::with_goal_handles(probe_chain("tally", TALLY_CALLBACKS), cfg, w.goal_handles()).unwrap();
    script.run_load().unwrap();
    let inspector = script.attach_inspector();
    let params = ModeParameters { command_limit: 1, timer_limit: DEFAULT_SCRIPT_TIMER_LIMIT, score_limit: None, time_limit: None };
    w.prepare_script_rules("tally", params, script).unwrap();
    prepare_mission_tail(&mut w);
    f4_tail::select_rules(&mut w, GameFamily::Swbf2, "tally", params, vec![]).unwrap();
    tick_world(&mut w, 0.0);
    for _ in 0..3 {
        revive_soldier(&mut w, unit, character);
        damage_kill_soldier(&mut w, unit, Attacker::Unit(shooter));
        tick_world(&mut w, 1.0 / 60.0);
    }
    let d = inspector.snapshot().diagnostics;
    assert_eq!((d.refused_commands, d.peak_commands), (3, 1));
    assert_eq!(d.errors.iter().filter(|e| e.kind == DiagnosticKind::CommandLimit).count(), 1);
}

#[test]
fn a_full_error_log_retains_the_first_command_refusal_and_terminal_failure() {
    for first in [DiagnosticKind::CommandLimit, DiagnosticKind::HostAborted] {
        let mut diagnostics = ScriptDiagnostics::default();
        for id in 0..MAX_DIAGNOSTICS {
            diagnostics.record_error(1, ScriptPhase::Tick, Some(id as u32), DiagnosticKind::RuntimeError, format!("station {id}"));
        }
        let second = if first == DiagnosticKind::CommandLimit { DiagnosticKind::HostAborted } else { DiagnosticKind::CommandLimit };
        diagnostics.record_error(2, ScriptPhase::Tick, None, first, "first".into());
        diagnostics.record_error(3, ScriptPhase::Tick, None, second, "second".into());
        assert_eq!(diagnostics.errors.len(), MAX_DIAGNOSTICS);
        assert!(diagnostics.errors.iter().any(|d| d.kind == first && d.tick == 2));
        assert!(diagnostics.errors.iter().any(|d| d.kind == second && d.tick == 3));
    }
}

#[test]
fn accepted_score_and_ticket_commands_fit_the_delivery_queue() {
    use crate::battle::spawning::TEAM_COUNT;
    const WRITES: &[u8] = include_bytes!("../../../tests/lua/16_score_writes.luac");
    let mut world = synthetic_world();
    let (_, telemetry, _) = admit_test_script(&mut world, probe_chain("counter", WRITES),
        ScriptRulesConfig::strict(12), "counter", None, vec![]);
    for _ in 0..5 { tick_world(&mut world, 1.0 / 60.0); }
    let t = telemetry.read();
    assert_eq!((t.scores[1], t.reinforcements[2]), (2048, 2048));
    assert_eq!(t.refused_commands, 904);
    assert_eq!(t.queue_overflows, 0);
    assert!(!t.faulted && !world.sim.store.battle.over);

    // A suspended observer can carry changes across many batches. All final
    // values survive compaction, including while physical delivery is full.
    let mut script = ScriptRules::new(probe_chain("counter", WRITES), ScriptRulesConfig::strict(12)).unwrap();
    let host = &mut script.host;
    host.reserve_delivery(1, 16).unwrap();
    assert!(host.queue_delivery(state::PendingEvent::TimerElapsed { timer_id: 1, arm_gen: 1 }));
    for seq in 1..=5000 {
        let team = (seq % 7 + 1) as i32;
        assert!(host.queue_delivery(state::PendingEvent::TeamPointsChanged { team, points: seq as u32, seq }));
        assert!(host.queue_delivery(state::PendingEvent::TicketsChanged { team, count: seq as u32, seq }));
    }
    assert!(host.pending_events.len() <= 1 + 16 + 2 * TEAM_COUNT);
    assert!(matches!(host.pop_delivery(), Some(state::PendingEvent::TimerElapsed { .. })));
    let mut last = [[0; TEAM_COUNT]; 2];
    while let Some(event) = host.pop_delivery() {
        match event {
            state::PendingEvent::TeamPointsChanged { team, points, .. } => last[0][team as usize] = points,
            state::PendingEvent::TicketsChanged { team, count, .. } => last[1][team as usize] = count,
            _ => unreachable!(),
        }
    }
    for seq in 4994..=5000 { assert_eq!([last[0][seq%7+1], last[1][seq%7+1]], [seq as u32; 2]); }

    // Sequence counters saturate. Compaction uses queue order, even with
    // a paused command budget and identical sequence numbers at every slot.
    host.reserve_delivery(1, 0).unwrap();
    for points in 0..100 {
        for team in 0..TEAM_COUNT as i32 {
            assert!(host.queue_delivery(state::PendingEvent::TeamPointsChanged { team, points, seq: u32::MAX }));
            assert!(host.queue_delivery(state::PendingEvent::TicketsChanged { team, count: points, seq: u32::MAX }));
        }
        assert!(host.pending_events.len() <= 2 * TEAM_COUNT);
    }
}

#[test]
fn a_depth_failure_releases_its_frame_and_delivers_every_remaining_death() {
    const NESTED: &[u8] = include_bytes!("../../../tests/lua/17_nested_counts.luac");
    let mut world = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut world, 1.0, false);
    let (unit, character) = spawn_soldier(&mut world, 2.0, false);
    let config = ScriptRulesConfig { max_call_depth: 16, ..ScriptRulesConfig::strict(31) };
    let (_, telemetry, _) = admit_test_script(&mut world, probe_chain("nested", NESTED), config, "nested", None, vec![]);
    for tick in 1..=500 {
        for _ in 0..3 {
            revive_soldier(&mut world, unit, character);
            damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
        }
        tick_world(&mut world, 1.0 / 60.0);
        let t = telemetry.read();
        assert_eq!((t.scores[1], t.callback_errors), (3 * tick, 3 * tick));
        assert_eq!((t.callbacks_dropped, t.queue_overflows), (0, 0));
        assert!(!t.faulted && !world.sim.store.battle.over);
    }
}

#[test]
fn a_light_scorer_after_a_busy_listener_gets_a_fresh_allowance() {
    const SHARED: &[u8] = include_bytes!("../../../tests/lua/18_shared_work.luac");
    let mut world = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut world, 1.0, false);
    let (unit, character) = spawn_soldier(&mut world, 2.0, false);
    let (_, telemetry, _) = admit_test_script(&mut world, probe_chain("shared", SHARED),
        ScriptRulesConfig::strict(31), "shared", None, vec![]);
    for death in 1..=10 {
        revive_soldier(&mut world, unit, character);
        damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
        tick_world(&mut world, 1.0 / 60.0);
        assert_eq!(telemetry.read().scores[1], death - 1);
        tick_world(&mut world, 1.0 / 60.0);
        let t = telemetry.read();
        assert_eq!(t.scores[1], death);
        assert_eq!((t.callback_budget_refusals, t.callbacks_dropped), (0, 0));
    }
}

#[test]
fn identical_errors_at_different_stripped_instructions_have_distinct_counters() {
    let mut world = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut world, 1.0, false);
    let (unit, character) = spawn_soldier(&mut world, 2.0, false);
    let config = ScriptRulesConfig { assault_num_bots: Some(8), ..ScriptRulesConfig::strict(32) };
    let fixture = include_bytes!("../../../tests/lua/15_callbacks.stripped.luac");
    let (_, _, inspector) = admit_test_script(&mut world, probe_chain("ledger", fixture), config, "ledger", None, vec![]);
    for _ in 0..4 {
        revive_soldier(&mut world, unit, character);
        damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
        tick_world(&mut world, 1.0 / 60.0);
    }
    let sites = inspector.snapshot().diagnostics.errors;
    assert_eq!(sites.len(), 4);
    let mut counts = sites.iter().map(|d| d.occurrences).collect::<Vec<_>>();
    counts.sort();
    assert_eq!(counts, [2, 2, 4, 4]);
    assert!(sites.iter().all(|d| d.site.is_some() && d.message.starts_with("ledger function ")));
}

#[test]
fn target_and_degraded_deadlines_use_authority_ticks_and_standings() {
    let mission = include_bytes!("../../../tests/lua/19_decisions.luac");
    for (scenario, target, finish, expected) in [
        (1, 2, 40_000, None),
        (2, 2, 1_801, Some(BattleOutcome::Victory(1))),
        (3, 2, 1_801, Some(BattleOutcome::Victory(2))),
        (4, 2, 1_801, Some(BattleOutcome::Draw(vec![]))),
        (5, 10, 36_002, Some(BattleOutcome::Victory(2))),
        (6, 10, 36_002, Some(BattleOutcome::Draw(vec![]))),
        (7, 10, 36_004, Some(BattleOutcome::Draw(vec![]))),
        (8, 10, 36_002, Some(BattleOutcome::Draw(vec![]))),
        (9, 10, 36_002, Some(BattleOutcome::Draw(vec![]))),
        (10, 2, 1_801, Some(BattleOutcome::Victory(2))),
        (11, 2, 1_801, Some(BattleOutcome::Victory(2))),
        (12, 2, 4_201, Some(BattleOutcome::Victory(2))),
    ] {
        let mut world = synthetic_world();
        let (shooter, _) = spawn_soldier(&mut world, 1.0, false);
        let (unit, character) = spawn_soldier(&mut world, 2.0, false);
        let config = ScriptRulesConfig { assault_num_bots: Some(scenario), ..ScriptRulesConfig::strict(41) };
        let (_, _, _) = admit_test_script(&mut world, probe_chain("meeting", mission), config, "meeting", Some(target), vec![]);
        for tick in 2..=finish {
            if (scenario == 7 && tick <= 4) || (scenario == 9 && (tick == 2 || tick == 31_000)) {
                revive_soldier(&mut world, unit, character);
                damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
            }
            assert!(!world.sim.store.battle.over, "scenario {scenario} ended before {tick}");
            tick_world(&mut world, 1.0 / 60.0);
        }
        let host = world.mission_state().unwrap().0.host();
        assert_eq!(host.outcome, expected, "scenario {scenario}");
        if scenario >= 11 {
            println!("Target deductions case={scenario} tick={finish} digest={:016x}",
                world.mission_state().unwrap().0.authoritative_digest());
        }
        assert_eq!(host.diagnostics.errors.iter().filter(|e| e.kind == DiagnosticKind::DecisionDeadline).count(),
            usize::from((5..=9).contains(&scenario)), "scenario {scenario}");
    }
}

#[test]
fn failing_setup_commits_a_draw_even_after_scoring_and_declaring_victory() {
    use rules::events::{EventQueue, EventRules, Outcome, RuleActions, RuleResult};
    #[derive(Default)]
    struct Match { score: u32, result: Option<Outcome> }
    impl RuleActions for Match {
        fn score(&self, team: f64) -> Option<u32> { Some(if team == 1.0 { self.score } else { 0 }) }
        fn set_team_points(&mut self, _: f64, n: u32) -> RuleResult<()> { self.score = n; Ok(()) }
        fn start_timer(&mut self, _: crate::store::StrId, _: f64) -> bool { true }
        fn stop_timer(&mut self, _: crate::store::StrId) -> bool { true }
        fn finish(&mut self, result: Outcome) -> RuleResult<()> {
            assert!(self.result.replace(result).is_none());
            Ok(())
        }
    }
    for (case, phase) in [(1, ScriptPhase::Init), (2, ScriptPhase::PostLoad), (3, ScriptPhase::PostLoad)] {
        let mut actions = Match::default();
        let fixture = include_bytes!("../../../tests/lua/20_setup_result.luac");
        let mut rules = ScriptRules::new(probe_chain("register", fixture), ScriptRulesConfig {
            assault_num_bots: Some(case), ..ScriptRulesConfig::strict(13)
        }).unwrap();
        rules.tick(0.0, &EventQueue::new(0), &mut actions);
        if case == 3 {
            assert_eq!(actions.result, Some(Outcome::Winner(1.0)));
            assert!(rules.setup_failure().is_none());
            continue;
        }
        assert_eq!(actions.result, Some(Outcome::Draw));
        assert_eq!(rules.host.outcome, Some(BattleOutcome::FaultedDraw));
        assert_eq!(rules.setup_failure(), Some("The selected mission could not start. The battle ended in a draw."));
        assert_eq!(rules.host.diagnostics.errors.len(), 1);
        let err = &rules.host.diagnostics.errors[0];
        assert_eq!(err.phase, phase);
        assert!(err.message.contains("register function") && err.message.contains("instruction"));
        rules.tick(0.0, &EventQueue::new(0), &mut actions);
        assert_eq!(rules.host.diagnostics.errors.len(), 1);
    }
}

#[test]
#[ignore = "runs a full degraded-match deadline"]
fn incoming_deaths_under_backpressure_preserve_the_match_until_its_deadline() {
    let fixture = include_bytes!("../../../tests/lua/22_listener_work.luac");
    let mut world = synthetic_world();
    let (shooter, _) = spawn_soldier(&mut world, 1.0, true);
    let victims = [spawn_soldier(&mut world, 2.0, false), spawn_soldier(&mut world, 2.0, false)];
    let config = ScriptRulesConfig { assault_num_bots: Some(1), ..ScriptRulesConfig::strict(42) };
    admit_test_script(&mut world, probe_chain("attendance", fixture), config, "attendance", None, vec![]);
    let mut first_loss = None;
    for tick in 2..=45_000 {
        if tick <= 6001 {
            for &(unit, character) in &victims {
                revive_soldier(&mut world, unit, character);
                damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
            }
        }
        tick_world(&mut world, 1.0 / 60.0);
        let rules = world.mission_state().unwrap().0;
        let host = rules.host();
        if host.diagnostics.queue_overflows > 0 { first_loss.get_or_insert(tick); }
        if tick == 4185 { assert!(host.outcome.is_none()); }
        if let Some(first) = first_loss {
            let due = first + 36_000;
            assert_eq!(host.degraded_deadline, Some(due));
            if tick == due {
                assert_eq!(host.outcome, Some(BattleOutcome::Victory(1)));
                let d = &host.diagnostics;
                assert_eq!((d.callback_errors, d.callbacks_dropped), (0, 0));
                assert_eq!(d.errors.len(), 1);
                assert_eq!(d.errors[0].kind, DiagnosticKind::EventQueueOverflow);
                assert_eq!(d.errors[0].occurrences, d.queue_overflows);
                assert!(d.errors[0].message.contains("Reduce listener work"));
                println!("Death pressure first={first} due={due} lost={} digest={:016x}", d.queue_overflows, rules.authoritative_digest());
                return;
            }
        }
        assert!(host.outcome.is_none());
    }
    panic!("deferred deaths never reached a standings decision");
}

#[test]
#[ignore = "runs ten minutes of scripted callbacks"]
fn listener_cost_reservations_protect_completed_work_and_charge_full_tick_exhaustion() {
    let fixture = include_bytes!("../../../tests/lua/22_listener_work.luac");
    for (case, ticks) in [(2, 36_000), (3, 10)] {
        let mut world = synthetic_world();
        let (shooter, _) = spawn_soldier(&mut world, 1.0, true);
        let (unit, victim) = spawn_soldier(&mut world, 2.0, false);
        let config = ScriptRulesConfig { assault_num_bots: Some(case), ..ScriptRulesConfig::strict(43) };
        admit_test_script(&mut world, probe_chain("meeting", fixture), config, "meeting", None, vec![]);
        let mut deaths = 0;
        for tick in 0..ticks {
            let arrivals = if case == 2 { u32::from(tick % 3 == 0) * 2 } else { 1 };
            for _ in 0..arrivals {
                revive_soldier(&mut world, unit, victim);
                damage_kill_soldier(&mut world, unit, Attacker::Unit(shooter));
                deaths += 1;
            }
            tick_world(&mut world, 1.0 / 60.0);
        }
        for _ in 0..4 { tick_world(&mut world, 1.0 / 60.0); }
        let rules = world.mission_state().unwrap().0;
        let host = rules.host();
        let diagnostics = &host.diagnostics;
        assert_eq!(host.cached_scores[1], deaths);
        if case == 2 {
            assert!((17_000..20_000).contains(&host.listeners_death[0].peak_work));
            assert_eq!((diagnostics.callback_budget_refusals, diagnostics.callbacks_dropped), (0, 0));
            assert_eq!(host.degraded_deadline, None);
        } else {
            assert_eq!((diagnostics.callback_budget_refusals, diagnostics.callbacks_dropped), (3, 1));
        }
        println!("Listener cost case={case} score={deaths} refused={} disabled={} digest={:016x}",
            diagnostics.callback_budget_refusals, diagnostics.callbacks_dropped, rules.authoritative_digest());
    }
}

struct WrappedMission {
    files: bfe_content::files::DirFiles,
    overrides: Vec<(String, std::rc::Rc<[u8]>)>,
}

impl bfe_content::files::Files for WrappedMission {
    fn read(&self, path: &str) -> Option<std::rc::Rc<[u8]>> {
        let trimmed = path.trim_start_matches('/');
        if let Some((_, bytes)) = self.overrides.iter().find(|(k, _)| k == trimmed) {
            return Some(std::rc::Rc::clone(bytes));
        }
        self.files.read(path)
    }
}

fn wrap_prepared_mission(root: PathBuf, mission: FlatMission, wrapper_bytes: &[u8]) -> WrappedMission {
    use bfe_content::files::Files;
    use serde_json::{json, Value as Json};
    use sha2::{Digest, Sha256};

    let files = bfe_content::files::DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let manifest_path = format!("maps/{}/scripts/{}-{}.json", mission.map, mission.era, mission.mode);
    let map_path = format!("maps/{}/map.json", mission.map);
    let mut manifest: Json = serde_json::from_slice(&files.read(&manifest_path).expect("prepared mission manifest")).unwrap();
    let mut map: Json = serde_json::from_slice(&files.read(&map_path).expect("prepared map.json")).unwrap();

    let digest = format!("{:x}", Sha256::digest(wrapper_bytes));
    let stored = format!("scripts/{digest}.luac");
    let scripts = manifest["scripts"].as_array_mut().expect("manifest scripts array");
    let entry = scripts[0].clone();
    let mut wrapped = entry.clone();
    wrapped["file"] = json!(stored);
    wrapped["length"] = json!(wrapper_bytes.len());
    wrapped["sha256"] = json!(digest);
    let mut previous = entry;
    previous["name"] = json!("previous_setup");
    previous["id"] = json!("previous_setup");
    scripts[0] = wrapped;
    scripts.insert(1, previous);

    let chunks = manifest["chunks"].as_array_mut().expect("manifest chunks array");
    chunks.insert(1, json!("previous_setup"));

    let entry_name = manifest["entry"].as_str().expect("manifest entry").to_owned();
    map["missions"][&entry_name]["scripts"] = manifest["scripts"].clone();

    let overrides = vec![
        (stored, std::rc::Rc::from(wrapper_bytes)),
        (manifest_path, serde_json::to_vec(&manifest).unwrap().into()),
        (map_path, serde_json::to_vec(&map).unwrap().into()),
    ];
    WrappedMission { files, overrides }
}

#[test]
#[ignore = "requires prepared 2005 mission content in BFE_SWBF2_PREPARED (wraps the Space mission in memory with 23_damage_observer.luac; BFE_SCRIPT_DAMAGE_CONTENT overrides)"]
fn prepared_damage_listeners_under_space_event_pressure() {
    let mut world = if let Some(root) = std::env::var_os("BFE_SCRIPT_DAMAGE_CONTENT") {
        prepared_world_at(PathBuf::from(root), space_mission(), 616, 8).0
    } else {
        let root = PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").expect(
            "set BFE_SWBF2_PREPARED to a prepared 2005 content root (or BFE_SCRIPT_DAMAGE_CONTENT to a pre-wrapped root)",
        ));
        let files = wrap_prepared_mission(root, space_mission(), include_bytes!("../../../tests/lua/23_damage_observer.luac"));
        let (world, _, _) = prepared_world_from(&files, space_mission(), 616, 8);
        assert!(!files.files.asked.borrow().iter().any(|p| p == "common/anims.json"));
        world
    };
    tick_world(&mut world, 0.0);
    let sides = capital_sections(&world);
    let sections = sides.iter().map(Vec::len).sum::<usize>();
    for _ in 0..6_000 {
        for (_, slot) in sides.iter().flatten() { damage_building(&mut world, *slot, 2.0, Some(Attacker::Null)); }
        tick_world(&mut world, 1.0 / 60.0);
    }
    let rules = world.mission_state().unwrap().0;
    let host = rules.host();
    let d = &host.diagnostics;
    assert!(host.listeners_obj_damage.iter().any(|row| row.peak_work > 17_000));
    assert!(!d.faulted && host.outcome.is_none());
    assert_eq!(d.callbacks_dropped, 0);
    let log = d.errors.iter().filter(|e| e.kind == DiagnosticKind::EventQueueOverflow).collect::<Vec<_>>();
    assert_eq!(log.len(), 1);
    assert!(d.queue_overflows > 0);
    assert_eq!(log[0].occurrences, d.queue_overflows);
    assert_eq!(host.degraded_deadline, Some(log[0].tick + 36_000));
    println!("Space pressure sections={sections} ticks=6000 callbacks={} refused={} disabled={} first={} lost={} digest={:016x}",
        d.callbacks_executed, d.callback_budget_refusals, d.callbacks_dropped, log[0].tick, d.queue_overflows, rules.authoritative_digest());
}

#[test]
fn producer_overflow_counts_every_missed_event_and_expires_the_native_timer_once() {
    let fixture = include_bytes!("../../../tests/lua/19_decisions.luac");
    let mut world = synthetic_world();
    let (_, victim) = spawn_soldier(&mut world, 1.0, true);
    let config = ScriptRulesConfig { assault_num_bots: Some(6), ..ScriptRulesConfig::strict(42) };
    admit_test_script(&mut world, probe_chain("rollcall", fixture), config, "rollcall", None, vec![]);
    let producer = world.battle.rule_events.as_mut().unwrap();
    let death = rules::events::Event::MissionSoldierKilled {
        victim, victim_team: 1, human: true, hero: false, credit: None, death: Default::default(),
    };
    while producer.push(death.clone()) {}
    assert!(!producer.push(death));
    // Two deaths have been dropped. The timer expires behind the full native batch.
    for _ in 0..60 { tick_world(&mut world, 1.0 / 60.0); }
    let rules = world.mission_state().unwrap().0;
    let host = rules.host();
    assert!(host.outcome.is_none() && !host.timers[0].running);
    assert_eq!(host.timers[0].remaining, 0.0);
    assert_eq!(host.degraded_deadline, Some(36_002));
    assert_eq!(host.diagnostics.queue_overflows, 3);
    assert_eq!(host.diagnostics.callback_errors, 0);
    assert_eq!(host.diagnostics.errors.len(), 1);
    assert_eq!(host.diagnostics.errors[0].occurrences, 3);
    println!("Producer overflow lost=3 digest={:016x}", rules.authoritative_digest());
}

pub(super) mod acceptance;
mod memory;
mod soldier_fault;

// A timer the script shows keeps the ticks it has left when it is stopped, which is
// what the HUD holds on screen; one with no time left has none to show.
#[test]
fn a_shown_timer_reports_the_ticks_it_has_left_running_or_stopped() {
    use super::{state::ScriptTimer, ShownTimer};
    let timer = |remaining: f64, running: bool| ScriptTimer {
        id: 3, name_bytes: b"lantern".to_vec(), remaining, rate: 1., running, alive: true, arm_gen: 0, started_tick: 0,
    };
    assert_eq!(ShownTimer::of(&timer(2.5, false), None), ShownTimer { name: b"lantern", due: None, left: 150 });
    assert_eq!(ShownTimer::of(&timer(2.5, true), Some(930)), ShownTimer { name: b"lantern", due: Some(930), left: 150 });
    // A part of a tick counts as a tick; nothing, a negative time and a missing number as none.
    assert_eq!(ShownTimer::of(&timer(0.001, false), None).left, 1);
    for remaining in [0., -3., f64::NAN] {
        assert_eq!(ShownTimer::of(&timer(remaining, false), None).left, 0, "{remaining}");
    }
}
