//! Goal subjects: where they are and whether they live is found apart from
//! whether the released combat can attack them; names fall back to posts
//! and regions only when no mission object has them; the map's regions are
//! bound whenever goal rules are installed.

use super::arena::{Arena, ArenaRules, BotSpec, BuildingSpec, Op};
use crate::ai::goals::{table::StoredArg, GoalKind, Resolved};
use crate::battle::rules::events::{NamedObject, ObjectHandle};
use crate::battle::rules::ModeParameters;
use crate::battle::f4_tail;
use crate::gen::{MobilePost, SpawnPost};
use bfe_family::GameFamily;

fn rifle(team: f64, x: f64, z: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: false, health: 1.0e9, human: false, mixed: false }
}

fn name(text: &str) -> StoredArg { StoredArg::Name(text.as_bytes().into()) }

fn defend(target: StoredArg) -> Op { Op::Add { team: 1, kind: "Defend", weight: 1.0, target, extra: StoredArg::None } }

/// Three guards of team 1 and a far enemy, so nobody fights.
fn guards() -> Vec<BotSpec> { vec![rifle(1.0, -20.0, 0.0), rifle(1.0, -20.0, 3.0), rifle(1.0, -20.0, 6.0), rifle(2.0, 140.0, 140.0)] }

fn parameters() -> ModeParameters { ModeParameters { command_limit: 32, score_limit: Some(99), ..Default::default() } }

fn near(arena: &Arena, bot: usize, at: [f64; 2], within: f64) -> bool {
    let p = arena.unit_pos(bot);
    let (dx, dz) = (p[0] - at[0], p[2] - at[1]);
    (dx * dx + dz * dz).sqrt() < within
}

const REGIONS: &str = r#"[
    {"name":"yard","matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 30,0,-25,1]},
    {"name":"depot","center":[-40,0,40]}
]"#;

#[test]
fn a_named_station_the_released_combat_cannot_attack_is_still_defended() {
    let station = BuildingSpec { name: "orchard_spout", team: 1.0, pos: [20.0, 0.0, 20.0], health: 300.0, targetable: false };
    let mut arena = Arena::build(&guards(), Some(station), 99, ArenaRules::Goals(vec![defend(name("Orchard_Spout"))]));
    arena.activate();
    arena.run(60 * 20);
    let bots = arena.bots().unwrap();
    let goal = bots.goals().next().unwrap();
    assert!(goal.active, "the station's Defend goal is active");
    let subject = bots.subject(goal.slot).expect("the station is found");
    assert_eq!((subject.position.x, subject.position.z, subject.entity), (20.0, 20.0, None));
    for i in 0..3 {
        assert!(near(&arena, i, [20.0, 20.0], 8.0), "guard {i} reached the station");
    }
}

#[test]
fn a_mobile_post_is_defended_where_it_last_was() {
    let mut arena = Arena::build(&guards(), None, 99, ArenaRules::Goals(Vec::new()));
    let label = arena.w.sim.strings.intern_str("crawler_cp");
    let post = arena.w.sim.store.mobile_post.insert(MobilePost {
        name: label,
        team: 1.0,
        active: false,
        mobile: true,
        last_position: [-10.0, 0.0, 30.0],
        ..Default::default()
    });
    let named = vec![NamedObject { object: ObjectHandle::Post(SpawnPost::MobilePost(post)), name: Some("crawler_cp".into()), class: None }];
    arena.ops.borrow_mut().push(defend(name("crawler_cp")));
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), named).is_ok());
    arena.activate();
    arena.run(60 * 15);
    let bots = arena.bots().unwrap();
    let goal = bots.goals().next().unwrap();
    assert!(goal.active);
    assert!(matches!(bots.target(goal.slot), Some(Resolved::Object { .. })));
    for i in 0..3 {
        assert!(near(&arena, i, [-10.0, 30.0], 8.0), "guard {i} reached the post");
    }
}

#[test]
fn an_ambiguous_object_name_never_falls_back_to_a_region_of_that_name() {
    let mut arena = Arena::build_full(&guards(), None, 99, ArenaRules::Goals(Vec::new()), false, Some(REGIONS));
    let mut named = Vec::new();
    for at in [[0.0, 0.0, 50.0], [0.0, 0.0, -50.0]] {
        let label = arena.w.sim.strings.intern_str("depot");
        let post = arena.w.sim.store.mobile_post.insert(MobilePost { name: label, team: 1.0, last_position: at, ..Default::default() });
        named.push(NamedObject { object: ObjectHandle::Post(SpawnPost::MobilePost(post)), name: Some("depot".into()), class: None });
    }
    arena.ops.borrow_mut().extend([defend(name("depot")), defend(name("yard"))]);
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), named).is_ok());
    arena.activate();
    arena.run(60 * 15);
    let bots = arena.bots().unwrap();
    let goals: Vec<_> = bots.goals().copied().collect();
    assert_eq!(bots.target(goals[0].slot), Some(&Resolved::Missing), "two objects share the name");
    assert!(!goals[0].active, "an ambiguous name is never guessed, not even as the region");
    assert_eq!(bots.target(goals[1].slot), Some(&Resolved::Name), "no object has the name");
    assert!(goals[1].active, "the region of a name no object has is defended");
    for i in 0..3 {
        assert!(near(&arena, i, [30.0, -25.0], 8.0), "guard {i} went to the region");
    }
}

#[test]
fn the_map_regions_are_bound_when_goal_rules_replace_others() {
    // Installed without regions, then replaced once the navigation has them.
    let mut arena = Arena::build(&guards(), None, 99, ArenaRules::Goals(vec![defend(name("yard"))]));
    arena.activate();
    arena.run(60);
    assert!(!arena.bots().unwrap().goals().next().unwrap().active, "no region data yet");
    arena.w.ai.battle_mut().unwrap().state.navigation.nav.regions = bfe_content::json::parse(REGIONS).unwrap();
    arena.ops.borrow_mut().push(defend(name("yard")));
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_ok());
    arena.run(60 * 15);
    assert!(arena.bots().unwrap().goals().next().unwrap().active);
    for i in 0..3 {
        assert!(near(&arena, i, [30.0, -25.0], 8.0), "guard {i} went to the region");
    }
}

#[test]
fn a_rebuilt_named_prop_is_found_again_and_a_handle_of_its_old_life_is_not() {
    let mast = BuildingSpec { name: "relay_mast", team: 2.0, pos: [25.0, 0.0, 0.0], health: 300.0, targetable: true };
    let mut arena = Arena::build(&guards(), Some(mast), 99, ArenaRules::Goals(Vec::new()));
    let handle = ObjectHandle::Prop(arena.building_handle.clone().unwrap());
    let destroy = |target| Op::Add { team: 1, kind: "Destroy", weight: 1.0, target, extra: StoredArg::None };
    arena.ops.borrow_mut().extend([destroy(name("relay_mast")), destroy(StoredArg::Object(handle.clone())), destroy(StoredArg::Placement(handle))]);
    arena.activate();
    arena.run(30);
    let active = |arena: &mut Arena| arena.bots().unwrap().goals().map(|g| g.active).collect::<Vec<_>>();
    assert_eq!(active(&mut arena), vec![true, true, true]);
    arena.ops.borrow_mut().push(Op::Health { name: "relay_mast", value: 0.0 });
    arena.run(60);
    assert_eq!(active(&mut arena), vec![false, false, false], "all suspend while the mast is down");
    arena.ops.borrow_mut().push(Op::Health { name: "relay_mast", value: 300.0 });
    arena.run(90);
    let bots = arena.bots().unwrap();
    let goals: Vec<_> = bots.goals().copied().collect();
    assert_eq!(goals.iter().map(|g| g.active).collect::<Vec<_>>(), vec![true, false, true], "the name and saved placement follow the rebuilt mast");
    assert_eq!(bots.target(goals[1].slot), Some(&Resolved::Missing), "the handle names the old life");
    assert_eq!(goals[0].kind, GoalKind::Destroy);
    assert_eq!(goals[0].assigned + goals[2].assigned, 3, "the free bots took the resumed goal");
}

#[test]
fn unchanged_goals_are_looked_up_twice_a_second_and_a_changed_table_at_once() {
    let mast = BuildingSpec { name: "relay_mast", team: 2.0, pos: [25.0, 0.0, 0.0], health: 1.0e9, targetable: true };
    let initial_buildings = crate::ai::goals::census::building_lookups();
    let mut arena = Arena::build(&guards(), Some(mast), 99, ArenaRules::Goals(vec![defend(name("relay_mast"))]));
    arena.activate();
    arena.run(30);
    let before = crate::ai::goals::census::resolves();
    arena.run(600);
    assert_eq!(crate::ai::goals::census::building_lookups().wrapping_sub(initial_buildings), 1);
    let looked = crate::ai::goals::census::resolves().wrapping_sub(before);
    assert!((19..=21).contains(&looked), "{looked} lookups in 600 ticks");
    let before = crate::ai::goals::census::resolves();
    arena.ops.borrow_mut().push(defend(name("relay_mast")));
    arena.run(1);
    assert_eq!(crate::ai::goals::census::resolves().wrapping_sub(before), 1, "a table change is looked up on its tick");
}

#[test]
fn a_region_install_waits_for_the_admitted_map() {
    let mut arena = Arena::build_full(&guards(), None, 99, ArenaRules::Goals(vec![]), false, Some(REGIONS));
    arena.activate();
    f4_tail::select_rules(&mut arena.w, GameFamily::Swbf1, "conquest", Default::default(), vec![]).unwrap();
    arena.ops.borrow_mut().push(defend(name("yard")));
    let ai = arena.w.ai.take_battle().unwrap();
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), vec![]).is_err());
    arena.w.ai.return_battle(ai);
    f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), vec![]).unwrap();
    arena.run(60);
    assert!(arena.bots().unwrap().goals().any(|g| g.active && g.assigned == 3));
}

#[test]
fn recorded_map_centres_survive_rule_replacement_without_a_navigation_graph() {
    use crate::battle::rules::Selection;
    use crate::world::admit::rows::battle::rule_selection;
    let mut a = Arena::build(&guards(), None, 99, ArenaRules::Goals(vec![]));
    a.activate();
    a.w.ai.battle_mut().unwrap().state.navigation.nav.regions = bfe_content::Value::Null;
    a.ops.borrow_mut().push(defend(name("yard")));
    let selection = Selection { family: GameFamily::Swbf2, mode: "arena".into(), parameters: parameters() };
    let map = bfe_content::json::parse(REGIONS).unwrap();
    let record = rule_selection::encode(&selection, vec![], map.as_array(), |_, name| a.w.sim.strings.intern_str(name));
    rule_selection::apply(&mut a.w, &record).unwrap();
    a.run(60);
    assert!(a.bots().unwrap().goals().any(|g| g.active && g.assigned == 3));
    let ai = a.w.ai.take_battle().unwrap();
    let empty = rule_selection::encode(&selection, vec![], Some(&[]), |_, name| a.w.sim.strings.intern_str(name));
    assert!(rule_selection::apply(&mut a.w, &empty).is_err());
    a.w.ai.return_battle(ai);
    a.ops.borrow_mut().push(defend(name("yard")));
    f4_tail::select_rules(&mut a.w, selection.family, &selection.mode, selection.parameters, vec![]).unwrap();
    a.run(60);
    assert!(a.bots().unwrap().goals().any(|g| g.active && g.assigned == 3));
}
