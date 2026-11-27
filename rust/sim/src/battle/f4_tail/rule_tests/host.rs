use super::*;
use rules::events::{MissionObjects, NamedObject, ObjectProperty, Outcome, Reinforcements::*, RuleError::*};

struct Probe {
    tick: usize,
    character: Ref<Character>,
    class: StrId,
    object: ObjectHandle,
    timer: StrId,
}

impl EventRules for Probe {
    fn tick(&mut self, _: f64, batch: &EventQueue, a: &mut dyn RuleActions) {
        let object = a.find_object("PoWeR").unwrap();
        assert_eq!(a.object_name(&object), Ok("power"));
        assert_eq!(a.object_class(&object), Ok("generator"));
        assert_eq!(a.find_object("absent"), Err(UnknownName));
        assert_eq!(a.score_limit(), Some(7));
        assert_eq!(a.battle_time(), Ok((self.tick + 1) as f64));
        match self.tick {
            0 => {
                assert_eq!(batch.events(), [Event::ObjectKilled { object: object.clone(), killer: Some(self.character) }]);
                assert_eq!(a.character_team(self.character), Ok(1.0));
                assert_eq!(a.character_is_human(self.character), Ok(true));
                let class = a.character_class(self.character).unwrap();
                assert_eq!(a.text(class).unwrap(), "tester".encode_utf16().collect::<Vec<_>>());
                assert_eq!(a.object_alive(&object), Ok(false));
                assert_eq!(a.object_health(&object), Ok((0.0, 100.0)));
                assert_eq!(a.set_property(&object, ObjectProperty::Team(2.0)), Ok(()));
                assert_eq!(a.object_team(&object), Ok(2.0));
                assert_eq!(a.object_shields(&object), Err(NotAvailable));
                assert_eq!(a.set_property(&object, ObjectProperty::Locked(true)), Err(NotAvailable));
                assert_eq!(a.set_property(&object, ObjectProperty::MaxHealth(150.0)), Ok(()));
                assert_eq!(a.set_health(&object, 150.0), Ok(()));
                let rebuilt = a.find_object("power").unwrap();
                assert_ne!(object, rebuilt);
                assert_eq!(a.kill_object(&object), Err(StaleHandle));
                assert_eq!(a.object_health(&rebuilt), Ok((150.0, 150.0)));
                assert_eq!(a.kill_object(&rebuilt), Ok(()));
                assert_eq!(a.add_team_points(1.0, 4), Ok(4));
                assert_eq!(a.team_points(1.0), Ok(4));
                assert_eq!(a.set_reinforcements(1.0, Finite(3)), Ok(()));
                assert_eq!(a.add_reinforcements(1.0, -1), Ok(Finite(2)));
                assert_eq!(a.set_reinforcements(1.0, Unlimited), Ok(()));
                assert_eq!(a.add_reinforcements(1.0, -42), Ok(Unlimited));
                assert_eq!(a.reinforcements(1.0), Ok(Unlimited));
                assert!(a.start_timer(self.class, 0.0));
                assert!(a.stop_timer(self.class));
                assert!(a.start_timer(self.timer, 0.0));
            }
            1 => {
                assert_eq!(a.object_name(&self.object), Ok("power"));
                assert_eq!(batch.events(), [
                    Event::ObjectKilled { object, killer: None },
                    Event::TeamPointsChanged { team: 1.0, points: 4 },
                    Event::TicketsChanged { team: 1.0, count: 3 },
                    Event::TicketsChanged { team: 1.0, count: 2 },
                    Event::TimerElapsed { timer: self.timer },
                ]);
                assert_eq!(a.finish(Outcome::Loser(2.0)), Ok(()));
            }
            _ => unreachable!(),
        }
        self.tick += 1;
    }
}

#[test]
fn host_resolves_rebuilt_instances_and_receives_only_committed_changes() {
    let mut w = world();
    let (shooter, character) = soldier(&mut w, 1.0);
    let (_, h) = prop(&mut w, false);
    let object = ObjectHandle::Prop(h);
    let class = w.sim.strings.intern_str("tester");
    let c = w.sim.store.character.get_mut(character).unwrap();
    c.human = true;
    c.class_name = Some(class);
    let timer = w.sim.strings.intern_str("clock");
    let objects = MissionObjects::new(vec![NamedObject {
        object: object.clone(), name: Some("power".into()), class: Some("generator".into()),
    }]);
    prepare_tail(&mut w);
    let probe = Probe { tick: 0, character, class, object, timer };
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(probe, 16, 1).with_mission(objects, Some(7)))).unwrap();
    f3::with_services_in(&mut w, |w, services| {
        crate::units::f3_props::command_hit_in(w, 0, 250.0, Some(Attacker::Unit(shooter)), None, services.unwrap());
    });
    tick(&mut w, 1.0);
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.loser, Some(2.0));
}

#[test]
fn a_score_mode_owns_death_tickets_and_the_published_end_state() {
    struct End;
    impl EventRules for End {
        fn tick(&mut self, _: f64, batch: &EventQueue, a: &mut dyn RuleActions) {
            assert!(matches!(batch.events(), [Event::UnitKilled { .. }]));
            assert_eq!(a.reinforcements(1.0), Ok(Finite(2)));
            assert_eq!(a.finish(Outcome::Loser(1.0)), Ok(()));
        }
    }
    let mut w = world();
    let (victim, _) = soldier(&mut w, 1.0);
    prepare_tail(&mut w);
    w.sim.store.battle.reinforcements.set(1.0, 2.0);
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(End, 1, 0))).unwrap();
    damage(&mut w, victim, Attacker::Null);
    tick(&mut w, 1.0);
    let b = &w.sim.store.battle;
    assert!(b.losing);
    assert_eq!(b.victory_clock, crate::store::Nullable::Value(0.0));
    assert_eq!(b.loser, Some(1.0));
}

#[test]
fn classic_respawn_cleanup_respects_the_selected_ticket_policy() {
    for event_mode in [false, true] {
        let mut w = world();
        let (unit, ch) = soldier(&mut w, 2.0);
        w.sim.store.character.get_mut(ch).unwrap().spawn = Some(Spawn::default());
        w.sim.store.spawning.teams = crate::battle::spawning::make_teams(8.0);
        w.sim.store.battle.reinforcements.set(2.0, 2.0);
        prepare_tail(&mut w);
        super::super::super::f4_core::configure_tail(&mut w.battle, true, None);
        if event_mode { record(&mut w); }
        w.tick.running = true;
        damage(&mut w, unit, Attacker::Null);
        let dead = w.sim.store.character.get(ch).unwrap();
        assert_eq!(dead.unit, None);
        assert_eq!(dead.spawn.as_ref().unwrap().timer, crate::battle::spawning::RESPAWN_DELAY);
        assert_eq!(w.sim.store.battle.reinforcements.at(2.0), Some(if event_mode { 2.0 } else { 1.0 }));
    }
}

#[test]
fn declared_outcomes_reach_the_client_match_summary() {
    use crate::wire::{stagger::Stagger, store_capture::Capture, store_summaries::{Link, Links, Summaries}};
    struct Offline;
    impl Links for Offline { fn get(&self, _: Ref<Character>) -> Option<Link> { None } }
    struct Declare(Outcome);
    impl EventRules for Declare {
        fn tick(&mut self, _: f64, _: &EventQueue, a: &mut dyn RuleActions) { a.finish(self.0).unwrap(); }
    }
    for (outcome, loser, losing, clock) in [
        (Outcome::Winner(2.0), 1, true, true),
        (Outcome::Loser(2.0), 2, false, true),
        (Outcome::Draw, 0, false, false),
    ] {
        let mut w = world();
        prepare_tail(&mut w);
        set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(Declare(outcome), 0, 0))).unwrap();
        tick(&mut w, 1.0);
        let maps = Capture::new(&Default::default(), &w.sim.strings).unwrap();
        let mut stagger = Stagger::new();
        Summaries::default().take(1., 1., &w.sim.store, &w.sim.strings, &maps, &Offline, &mut stagger).unwrap();
        let field = stagger.entries.iter().find(|r| r.schema == 1.0).unwrap().field.as_ref().unwrap();
        // Reinforcement count, over/game-over/finished/losing, loser, elapsed and optional clocks.
        let mut expected = vec![0, 1, 0, 0, u8::from(losing), loser];
        expected.extend(0.0f64.to_le_bytes());
        expected.push(u8::from(clock));
        if clock { expected.extend(0.0f64.to_le_bytes()); }
        expected.push(0);
        assert_eq!(field.payload, expected);
    }
}

#[test]
fn a_missing_producer_leaves_event_rules_idle() {
    struct NeverCalled;
    impl EventRules for NeverCalled {
        fn tick(&mut self, _: f64, _: &EventQueue, _: &mut dyn RuleActions) { panic!("no producer"); }
    }
    let mut w = world();
    prepare_tail(&mut w);
    set_rules(&mut w, RuleSet::EventDriven(EventDriven::new(NeverCalled, 0, 0))).unwrap();
    w.battle.rule_events = None;
    tick(&mut w, 1.0);
    assert_eq!(w.sim.store.battle.stats.time, 0.0);
}

use crate::world::admit::Options;
#[path = "../../../../tests/support/admit_context.rs"]
mod admit_context;

#[test]
fn admission_records_and_replays_the_handler_parameters_and_native_names() {
    use crate::world::admit::{rows::battle::live::admit_rules, sequence, Cx, Stage};
    use rules::ModeParameters;
    let (files, options) = admit_context::context_fixture();
    let mut battle = bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files), &options.map, &options.query()).unwrap();
    battle.props = Some(bfe_content::json::parse(r#"{"instances":[{"base":"powerupstation","name":"battery","class":"power","model":"box"}],"models":{"box":{}}}"#).unwrap());
    let content = bfe_content::content::Content::build(battle).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    struct Verify;
    impl EventRules for Verify {
        fn tick(&mut self, _: f64, _: &EventQueue, a: &mut dyn RuleActions) {
            let object = a.find_object("BATTERY").unwrap();
            assert_eq!(a.object_class(&object), Ok("power"));
            let post = ObjectHandle::Post(SpawnPost::Post(Ref::new(0, 1)));
            assert_eq!(a.object_name(&post), Ok("battery"));
            assert_eq!(a.find_object("shared"), Err(AmbiguousName));
            assert_eq!(a.object_health(&object), Ok((100.0, 100.0)));
            assert_eq!(a.score_limit(), Some(8));
            a.finish(Outcome::Draw).unwrap();
        }
    }
    let params = ModeParameters { command_limit: 5, timer_limit: 2, score_limit: Some(8), time_limit: Some(90.0) };
    let build = |factory: bool, request: bool| {
        let mut w = world();
        if factory {
            w.register_rule_factory(move |family, mode, p| {
                assert_eq!((family, mode, *p), (GameFamily::Swbf2, "exercise", params));
                Ok(Box::new(Verify))
            });
        }
        if request { w.request_battle_rules(GameFamily::Swbf2, "exercise", params).unwrap(); }
        let (building, _) = prop(&mut w, false);
        for (name, building) in [("battery", Some(building)), ("shared", None), ("SHARED", None)] {
            let name = w.sim.strings.intern_str(name);
            let post = w.sim.store.post.insert(Post { name, building, ..Default::default() });
            w.sim.store.command_posts.posts.push(post);
        }
        prepare_tail(&mut w);
        w
    };
    let mut legacy = build(false, false);
    let mut cx = Cx::new(&content, &defs, &files, &options, legacy.sim.strings.clone());
    let before = legacy.sim.strings.list.clone();
    let mut calls = Vec::new();
    admit_rules(&mut legacy, &mut cx, &mut calls, &[]).unwrap();
    assert!(calls.is_empty());
    assert_eq!(legacy.sim.strings.list, before);

    let mut original = build(true, true);
    admit_rules(&mut original, &mut cx, &mut calls, &[]).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "f4_select_rules");
    let mut copy = build(true, false);
    let mut cx = Cx::new(&content, &defs, &files, &options, copy.sim.strings.clone());
    sequence::replay(&mut copy, Stage::LiveTail, &calls, &mut cx).unwrap();
    for w in [&mut original, &mut copy] {
        tick(w, 1.0);
        assert!(w.sim.store.battle.over);
        assert_eq!(w.sim.store.battle.loser, None);
    }
    assert!(sequence::replay(&mut legacy, Stage::LiveTail, &calls, &mut cx).is_err());
    let old = calls[0].2.args[2];
    calls[0].2.args[2] = old + 0.5;
    assert!(sequence::replay(&mut copy, Stage::LiveTail, &calls, &mut cx).is_err());
}

#[test]
fn prepared_missions_keep_world_goal_handles_across_load_refusal_and_reset() {
    use crate::battle::script::{ScriptChain, ScriptChunk, ScriptRules, ScriptRulesConfig};
    use crate::ai::goals::GoalHandles;
    let params = rules::ModeParameters { command_limit: 16, ..Default::default() };
    let load = |handles: GoalHandles, previous| {
        let chunk = ScriptChunk {
            name: "catalogue".into(), bytes: include_bytes!("../../../../tests/lua/14_catalogue.luac").as_slice().into(),
        };
        let mut script = ScriptRules::with_goal_handles(ScriptChain::new("catalogue", vec![chunk]),
            ScriptRulesConfig { assault_num_bots: Some(previous), ..Default::default() }, handles).unwrap();
        script.run_load().unwrap();
        script
    };
    let ids = |script: &ScriptRules| script.host().ai_goals.iter().map(|row| row.id).collect::<Vec<_>>();
    let mut w = world();
    let nav = crate::ai::nav::Nav::empty(Default::default());
    let mut settings = crate::gen::AiColdSettings::default();
    settings.capacity.scopes = 1;
    w.ai.begin(nav.into(), &settings, &mut w.sim.strings, Default::default());
    w.battle.map_goal_regions = Some(Default::default());
    let handles = w.goal_handles();
    let first = load(handles.clone(), 0);
    assert_eq!(ids(&first), [1, 2, 3]);
    w.prepare_script_rules("ledger", params, first).unwrap();
    prepare_tail(&mut w);
    let select = |w: &mut Root| select_rules(w, GameFamily::Swbf2, "ledger", params, Vec::new());
    select(&mut w).unwrap();

    // Uninstalled candidates still consumed the numbers they returned to Lua.
    let abandoned = load(handles.clone(), 1);
    assert_eq!(ids(&abandoned), [4, 5, 6]);
    drop(abandoned);
    let second = load(handles.clone(), 1);
    assert_eq!(ids(&second), [7, 8, 9]);
    w.prepare_script_rules("ledger", params, second).unwrap();
    let capacity = state(&mut w.battle).capacity;
    state(&mut w.battle).capacity.registry = usize::MAX;
    assert_eq!(select(&mut w), Err(LimitReached));
    assert_eq!(ids(w.mission_state().unwrap().0), [1, 2, 3]);
    state(&mut w.battle).capacity = capacity;
    select(&mut w).unwrap();
    assert_eq!(ids(w.mission_state().unwrap().0), [7, 8, 9]);

    reset(&mut w.battle);
    assert!(w.goal_handles().same_world(&handles));
    assert_eq!(ids(&load(w.goal_handles(), 7)), [10, 11, 12]);
    assert_eq!(w.prepare_script_rules("ledger", params, load(GoalHandles::default(), 0)), Err(InvalidValue));
}
