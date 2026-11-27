//! A hunter makes progress: enemies it cannot reach are given up by what
//! the hunter itself did, never by how they move, and every enemy gets its
//! turn before one is tried again.

use super::arena::{Arena, ArenaRules, BotSpec, Op};
use crate::ai::goals::reach::{set_test_routes, test_route_calls, Route};
use crate::ai::goals::{table::StoredArg, GoalController, GoalRegions};
use crate::gen::UnitBody;

fn deathmatch(team: u8) -> Op { Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None } }

fn rifle(team: f64, x: f64, z: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: false, health: 1.0e9, human: false, mixed: false }
}

fn place(arena: &mut Arena, idx: usize, x: f64, z: f64) {
    let u = arena.w.sim.store.unit.get_mut(arena.units[idx]).unwrap();
    let UnitBody::Soldier(s) = &mut u.soldier else { unreachable!() };
    s.position = [x, s.position[1], z];
    s.previous = s.position;
    s.settled = s.position;
}

struct Run {
    first_reachable: Option<u32>,
    closest: f64,
    target_ticks: [u32; 3],
}

/// One hunter west of a wall the length of the arena, two enemies moving
/// toward and away from the wall on its far side, and one reachable enemy
/// 115 m up the hunter's side.
fn moving_enemies(ticks: u32) -> Run {
    let bots = vec![rifle(1.0, -3.0, 0.0), rifle(2.0, 21.0, 0.0), rifle(2.0, 3.0, 0.0), rifle(2.0, -3.0, 115.0)];
    let mut arena = Arena::build_with_wall(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1)]), true);
    arena.activate();
    let mut run = Run { first_reachable: None, closest: f64::INFINITY, target_ticks: [0; 3] };
    for tick in 0..ticks {
        let c = v8math::cos(2.0 * std::f64::consts::PI * f64::from(tick) / 240.0);
        place(&mut arena, 1, 12.0 + 9.0 * c, 0.0);
        place(&mut arena, 2, 12.0 - 9.0 * c, 0.0);
        arena.step_one();
        if let Some(target) = arena.bots().unwrap().hunting(0) {
            run.target_ticks[target - 1] += 1;
            if target == 3 {
                run.first_reachable.get_or_insert(tick);
            }
        }
        let d = arena.distance(0, 3);
        if d < run.closest {
            run.closest = d;
        }
    }
    run
}

#[test]
fn moving_enemies_behind_a_wall_never_starve_a_reachable_one() {
    let run = moving_enemies(60 * 90);
    let first = run.first_reachable.expect("the reachable enemy was chosen");
    assert!(first < 60 * 40, "chosen at tick {first}");
    assert!(run.closest < 40.0, "closest approach {:.1} m; target ticks {:?}", run.closest, run.target_ticks);
}

#[test]
fn an_enemy_with_no_route_is_tried_after_every_enemy_with_one() {
    // East of the wall the navigation confirms there is no route; the far
    // enemy on the hunter's side has one.
    set_test_routes(Some(|_, to, _| {
        if to.x > 0.0 {
            Route::NoRoute
        } else {
            Route::Reachable { distance: 2400.0 }
        }
    }));
    let calls = test_route_calls();
    let run = moving_enemies(120);
    let asked = test_route_calls().wrapping_sub(calls);
    set_test_routes(None);
    assert_eq!(run.target_ticks[..2], [0, 0], "no tick spent on an enemy without a route while one has a route");
    assert!(run.first_reachable.is_some_and(|t| t < 60));
    assert!(asked <= 3, "each pair is asked once: {asked} questions");
}

#[test]
fn a_walk_the_navigation_never_shortens_is_given_up_and_asked_only_when_its_clock_runs_out() {
    // The navigation claims a 10 m walk to every enemy, the blocked ones too.
    set_test_routes(Some(|_, _, _| Route::Reachable { distance: 10.0 }));
    let calls = test_route_calls();
    let run = moving_enemies(60 * 90);
    let asked = test_route_calls().wrapping_sub(calls);
    set_test_routes(None);
    let first = run.first_reachable.expect("the reachable enemy was chosen");
    assert!(first < 60 * 40, "chosen at tick {first}");
    assert!(run.closest < 40.0, "closest approach {:.1} m", run.closest);
    // Three pairs asked when first considered, then at most once per
    // approach clock (four seconds) of the one hunter.
    assert!(asked <= 3 + 90 / 4 + 1, "{asked} questions in 90 s");
}

#[test]
fn a_given_up_enemy_is_qualified_by_both_lives_and_kept_as_the_roster_grows() {
    let mut bots = GoalController::new();
    bots.admit(32, GoalRegions::default(), 1).unwrap();
    assert_eq!(bots.test_capacity(), 32);
    bots.test_give_up(3, 7);
    bots.test_give_up(31, 30);
    assert!(bots.test_given_up(3, 7) && bots.test_given_up(31, 30));
    // A larger roster than the one admitted keeps every pair.
    bots.test_reserve(64);
    assert!(bots.test_given_up(3, 7) && bots.test_given_up(31, 30));
    assert!(!bots.test_given_up(7, 3) && !bots.test_given_up(3, 40));
    // The enemy's next life is a new candidate.
    bots.test_new_life(7);
    assert!(!bots.test_given_up(3, 7));
    // So is every enemy for the bot's next life.
    bots.test_end_life(31);
    assert!(!bots.test_given_up(31, 30));
}

#[test]
fn a_hunter_closing_on_a_retreating_enemy_keeps_its_pursuit() {
    closing_enemy(1.0, 8.9);
}

#[test]
fn a_slowly_closing_hunter_keeps_the_same_enemy() {
    closing_enemy(0.001, 88.9);
}

fn closing_enemy(speed: f64, initial_gap: f64) {
    let mut bots = [rifle(1., -10., 0.), rifle(2., -2., 0.), rifle(2., -10., 120.)];
    for bot in &mut bots { bot.hero = true; }
    for enemy in &mut bots[1..] { enemy.human = true; }
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    for tick in 0..360 {
        let x = -10. + f64::from(tick) * speed / 60.;
        let gap = initial_gap - f64::from(tick) * speed * 0.013;
        let bodies = [arena.units[0], arena.units[1]];
        arena.step_tail_with(|w| {
            for (body, x) in bodies.into_iter().zip([x, x + gap]) {
                let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(body).unwrap().soldier else { unreachable!() };
                s.position = [x, 0., 0.];
                s.previous = s.position;
                s.settled = s.position;
            }
            crate::battle::f4_tail::f4_tail_in(w);
        });
        if tick > 45 { assert_eq!(arena.bots().unwrap().hunting(0), Some(1), "tick {tick}"); }
    }
}

#[test]
fn hunt_routes_use_the_asking_soldiers_radius_and_try_unknown_ground() {
    let mut targets = [rifle(1., -20., 0.), rifle(1., 20., 0.), rifle(2., 0., 98.)];
    targets[2].human = true;
    let mut arena = Arena::build(&targets, None, 73, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    for (body, radius) in arena.units.iter().zip([0.31, 0.57, 0.83]) {
        arena.w.sim.store.unit.get_mut(*body).unwrap().nav_radius = Some(radius);
    }
    set_test_routes(Some(|from, _, radius| {
        if from.x < 0. {
            assert_eq!(radius, 0.31);
            Route::Unknown
        } else {
            assert_eq!(radius, 0.57);
            Route::NoRoute
        }
    }));
    for _ in 0..60 { arena.step_one(); }
    set_test_routes(None);
    assert_eq!(arena.bots().unwrap().hunting(0), Some(2));
    assert_eq!(arena.bots().unwrap().hunting(1), None);
}

#[test]
fn a_wave_of_hunters_shares_four_route_questions_per_tick() {
    let mut units: Vec<_> = (0..32).map(|i| rifle(1., i as f64 * 2., 0.)).collect();
    let mut opponent = rifle(2., 0., 150.);
    opponent.human = true;
    units.push(opponent);
    let mut arena = Arena::build(&units, None, 99, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    set_test_routes(Some(|_, _, _| Route::Reachable { distance: 160. }));
    let mut passes = Vec::new();
    for _ in 0..32 {
        let before = test_route_calls();
        arena.step_one();
        passes.push(test_route_calls().wrapping_sub(before));
        if (0..32).all(|i| arena.bots().unwrap().hunting(i) == Some(32)) { break; }
    }
    set_test_routes(None);
    let start = passes.iter().position(|&n| n != 0).expect("wave asked about its opponent");
    assert_eq!(&passes[start..], &[4; 8], "queries in character order");
    assert!((0..32).all(|i| arena.bots().unwrap().hunting(i) == Some(32)));
}

#[test]
fn a_hunter_asks_again_after_leaving_ground_without_routes() {
    let mut enemy = rifle(2., 0., 130.);
    enemy.human = true;
    let mut arena = Arena::build(&[rifle(1., 0., 0.), enemy], None, 71, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    set_test_routes(Some(|from, _, _| if from.x < 2. { Route::NoRoute } else { Route::Reachable { distance: 130. } }));
    let start = test_route_calls();
    for _ in 0..185 {
        place(&mut arena, 0, 0., 0.);
        arena.step_one();
    }
    assert_eq!(test_route_calls().wrapping_sub(start), 1);
    assert_eq!(arena.bots().unwrap().hunting(0), None);
    for _ in 0..150 {
        place(&mut arena, 0, 4., 0.);
        arena.step_one();
        if arena.bots().unwrap().hunting(0).is_some() { break; }
    }
    set_test_routes(None);
    assert_eq!(arena.bots().unwrap().hunting(0), Some(1));
    assert_eq!(test_route_calls().wrapping_sub(start), 2);
}

#[test]
fn a_known_walk_outranks_unknown_enemies_even_outside_the_nearest_three() {
    let mut units = vec![rifle(1., 0., 0.)];
    for distance in [90., 95., 100., 180.] {
        let mut unit = rifle(2., 0., distance);
        unit.human = true;
        units.push(unit);
    }
    let mut arena = Arena::build(&units, None, 73, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    set_test_routes(Some(|_, to, _| if to.z < 150. { Route::Unknown } else { Route::Reachable { distance: 2400. } }));
    for _ in 0..125 {
        arena.step_one();
        if arena.bots().unwrap().hunting(0) == Some(4) { break; }
    }
    set_test_routes(None);
    assert_eq!(arena.bots().unwrap().hunting(0), Some(4));
}
