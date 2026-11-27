//! Pilot decisions on invented geometry, craft and paths.

use super::routes::{Routes, PROVISIONAL_EXIT_INSIDE, PROVISIONAL_EXIT_OUTSIDE};
use super::steer::{escape_from, lead, traffic_escape, Traffic};
use super::targets::{choose, object_rate, object_weapon, usable, Loadout, PilotTarget, System};
use super::vec::{dist, dot, len, sub, unit};
use super::*;
use crate::space::bounds::{HangarForcefield, SpaceBounds};
use crate::space::flight::{SpaceFlightProfile, SpaceVehiclePad, SpaceWeaponSpec};
use bfe_content::json::parse;

fn pad(team: i32, position: [f64; 3]) -> SpaceVehiclePad {
    SpaceVehiclePad {
        name: format!("pad{team}"),
        team,
        profile_index: 0,
        control_zone: String::new(),
        position,
        yaw: 0.0,
        matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, position[0], position[1], position[2], 1.0],
        spawn_count: 1,
        spawn_time: 1.0,
        expire_time_enemy: 1.0,
        expire_time_field: 1.0,
        decay_time: 1.0,
        respawn_timer: 1.0,
        floor_y: position[1],
    }
}

/// An opening in a wall whose normal is the world Z axis (either sign).
fn opening(center: [f64; 3]) -> HangarForcefield {
    let mut m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    m[12..15].copy_from_slice(&center);
    HangarForcefield {
        name: String::from("sheet"),
        team: 0,
        center,
        matrix: m,
        inverse: m,
        half_extents: [30.0, 9.0, 1.0],
        kernel_body: None,
        authored_opening: true,
    }
}

#[test]
fn exits_open_away_from_their_team_and_take_the_nearest_launch_path() {
    let mut bounds = SpaceBounds::from_world_json(&parse("{}").unwrap());
    // Team 1's deck lies south of an opening at z = 0, team 2's north of one at z = 400.
    bounds.forcefields.push(opening([0.0, 5.0, 0.0]));
    bounds.forcefields.push(opening([0.0, 5.0, 400.0]));
    let pads = [pad(1, [10.0, 0.0, -40.0]), pad(1, [-10.0, 0.0, -50.0]), pad(2, [0.0, 0.0, 450.0])];
    let world = parse(
        r#"{"paths":[
            {"name":"type_EntityPath far","nodes":[{"position":[0,0,2000]},{"position":[0,0,2100]}]},
            {"name":"type_EntityPath out","nodes":[{"position":[5,5,150]},{"position":[0,50,300]},{"position":[0,90,320]}]},
            {"name":"ground walk","nodes":[{"position":[0,5,90]},{"position":[0,5,95]}]},
            {"name":"type_EntityPath back","nodes":[{"position":[0,5,250]},{"position":[0,5,100]}]}
        ]}"#,
    )
    .unwrap();
    let routes = Routes::build(&bounds, &pads, &world);
    assert_eq!(routes.exits.len(), 2);
    let south = &routes.exits[0];
    assert_eq!(south.team, 1);
    assert!(south.outward[2] > 0.99, "{:?}", south.outward);
    assert!(dist(south.outside, [0.0, 5.0, PROVISIONAL_EXIT_OUTSIDE]) < 1e-9);
    assert!(dist(south.inside, [0.0, 5.0, -PROVISIONAL_EXIT_INSIDE]) < 1e-9);
    // The path starting just outside, not the far one, the ground walk or the one inbound.
    let path = &routes.nodes[south.path.0 as usize..south.path.1 as usize];
    assert_eq!(path.len(), 3);
    assert_eq!(path[0], [5.0, 5.0, 150.0]);
    let north = &routes.exits[1];
    assert_eq!(north.team, 2);
    assert!(north.outward[2] < -0.99);
    assert!(routes.nodes[north.path.0 as usize..north.path.1 as usize].first() == Some(&[0.0, 5.0, 250.0]));
    // Tracks keep a margin from the edges, depths count inward.
    assert_eq!(south.track([100.0, 0.0, -10.0], 8.0), 22.0);
    assert!((south.depth([0.0, 0.0, -12.0]) - 12.0).abs() < 1e-12);
    assert!(dist(south.track_point(-4.0, 20.0), [-4.0, 5.0, -20.0]) < 1e-12);
    assert_eq!(routes.nearest_exit(2, [0.0, 0.0, 0.0]), Some(1));
}

#[test]
fn lead_meets_a_crossing_target() {
    let target = [0.0, 0.0, 500.0];
    let velocity = [97.0, 0.0, 0.0];
    let point = lead([0.0; 3], target, velocity, 1030.0);
    let t = (point[0] - target[0]) / velocity[0];
    assert!(t > 0.4 && t < 0.6, "{t}");
    assert!((len(point) - 1030.0 * t).abs() < 1e-6);
    // A target faster than the rounds that runs away is shot where it is.
    assert_eq!(lead([0.0; 3], target, [0.0, 0.0, 1990.0], 1030.0), target);
}

#[test]
fn only_closing_craft_count_as_coming_collisions() {
    let me = Traffic { id: 0, generation: 1, team: 1, position: [0.0; 3], velocity: [0.0, 0.0, 77.0], reach: 5.0 };
    let head_on = Traffic { id: 1, generation: 1, team: 2, position: [3.0, 0.0, 100.0], velocity: [0.0, 0.0, -77.0], reach: 5.0 };
    let away = Traffic { velocity: [0.0, 0.0, 190.0], ..head_on };
    let far = Traffic { position: [0.0, 0.0, 400.0], ..head_on };
    let wide = Traffic { position: [60.0, 0.0, 100.0], ..head_on };
    let out = traffic_escape(&me, &[head_on], [0.0, 1.0, 0.0]).expect("head-on pair");
    assert!(out[0] < 0.0, "turns away from the side the other passes: {out:?}");
    assert!(traffic_escape(&me, &[away, far, wide], [0.0, 1.0, 0.0]).is_none());
    assert!(traffic_escape(&me, &[me], [0.0, 1.0, 0.0]).is_none());
}

#[test]
fn a_surface_ahead_turns_the_craft_along_and_away_from_it() {
    let heading = [0.0, -0.6, 0.8];
    let normal = [0.0, 1.0, 0.0];
    let out = escape_from(heading, normal, [0.0, 1.0, 0.0]);
    assert!(out[1] > 0.5 && out[2] > 0.3, "{out:?}");
    // Straight at a wall: up and away.
    let out = escape_from([0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    assert!(out[1] > 0.5 && out[2] < 0.0, "{out:?}");
}

fn weapon(channel: u8, vehicle: bool, building: bool, min: f64, max: f64) -> SpaceWeaponSpec {
    SpaceWeaponSpec {
        channel,
        ai_targets_vehicle: vehicle,
        ai_targets_building: building,
        ai_min_range: min,
        max_range: max,
        ..Default::default()
    }
}

#[test]
fn weapons_are_used_on_what_their_flags_allow_and_bombers_prefer_systems() {
    // A fighter whose missile does 500 a second to a ship object, a bomber
    // whose bombs do 19,000.
    let missile = SpaceWeaponSpec { max_damage: 37.0, shot_delay: 0.074, ..weapon(1, true, true, 163.0, 1170.0) };
    let bomb = SpaceWeaponSpec { max_damage: 2470.0, shot_delay: 0.13, ..weapon(1, false, true, 23.0, 265.0) };
    let fighter = SpaceFlightProfile { weapons: vec![weapon(0, true, false, 0.0, 870.0), missile], ..Default::default() };
    let bomber = SpaceFlightProfile { weapons: vec![weapon(0, true, false, 0.0, 410.0), bomb], ..Default::default() };
    let (lf, lb) = (Loadout::of(&fighter), Loadout::of(&bomber));
    assert!(lf.hits_craft && lf.hits_buildings && !lf.bomber);
    assert!(lb.hits_craft && lb.hits_buildings && lb.bomber);
    assert!((lf.shield_rate - 500.0).abs() < 1e-6 && (lb.shield_rate - 19000.0).abs() < 1e-6);
    assert!(usable(&bomber.weapons[1], true, 100.0));
    assert!(!usable(&bomber.weapons[1], false, 100.0));
    assert!(!usable(&bomber.weapons[1], true, 10.0));
    assert!(!usable(&fighter.weapons[0], false, 871.0));

    let me = Traffic { id: 0, generation: 1, team: 1, position: [0.0; 3], velocity: [0.0; 3], reach: 4.0 };
    let enemy = Traffic { id: 7, generation: 3, team: 2, position: [0.0, 0.0, 400.0], velocity: [0.0; 3], reach: 4.0 };
    let friend = Traffic { id: 8, team: 1, position: [0.0, 0.0, 100.0], ..enemy };
    let object = |index: usize, team: i32, z: f64| System {
        index,
        life: index as f64,
        team,
        center: [0.0, 0.0, z],
        radius: 23.0,
        bodies: [-1; 3],
        gun_reach: 0.0,
        lockable: true,
        shield: 0.0,
        exposure: 0.0,
        hull: false,
        left: 1.0,
    };
    let systems = [object(40, 1, 50.0), object(41, 2, 300.0)];
    let traffic = [enemy, friend];
    // The fighter's weapons hurt an object little: the craft ranks first.
    let pick = choose(&traffic, &systems, &lf, &me, [0.0, 0.0, 1.0]);
    assert_eq!(pick[0], PilotTarget::Craft { index: 7, generation: 3 });
    assert_eq!(pick[1], PilotTarget::Building { index: 41, life: 41.0 });
    // The bomber goes for the object, never its own team's 40.
    let pick = choose(&traffic, &systems, &lb, &me, [0.0, 0.0, 1.0]);
    assert_eq!(pick[0], PilotTarget::Building { index: 41, life: 41.0 }, "never the own team's system 40");
    // Of two objects at the same distance, the one its own guns cover ranks
    // after; one whose shield is nearly gone ranks first.
    let covered = System { exposure: 1.9, ..object(42, 2, 300.0) };
    let open = object(43, 2, 300.0);
    assert_eq!(choose(&[], &[covered, open], &lb, &me, [0.0, 0.0, 1.0])[0], PilotTarget::Building { index: 43, life: 43.0 });
    let fresh = System { shield: 7700.0, left: 1.0, ..object(44, 2, 280.0) };
    let nearly = System { shield: 340.0, left: 0.045, ..object(45, 2, 600.0) };
    assert_eq!(choose(&[], &[fresh, nearly], &lb, &me, [0.0, 0.0, 1.0])[0], PilotTarget::Building { index: 45, life: 45.0 });
    // A hull section carries the shared shield only while it is up.
    let section = System { hull: true, shield: 0.0, ..object(46, 2, 100.0) };
    assert_eq!(choose(&[], &[section], &lb, &me, [0.0, 0.0, 1.0])[0], PilotTarget::None);
    // A weapon that does an object next to nothing leaves it alone.
    let feeble = SpaceFlightProfile { weapons: vec![SpaceWeaponSpec { max_damage: 1.0, shot_delay: 1.0, ..weapon(1, false, true, 0.0, 870.0) }], ..Default::default() };
    assert_eq!(choose(&[], &[open], &Loadout::of(&feeble), &me, [0.0, 0.0, 1.0])[0], PilotTarget::None);
}

#[test]
fn a_weapons_damage_to_an_object_counts_every_round_of_its_salvos() {
    let cannon = SpaceWeaponSpec { max_damage: 23.0, shot_delay: 0.7, salvo_count: 2, shots_per_salvo: 3, ..weapon(0, true, true, 0.0, 870.0) };
    assert!((object_rate(&cannon, false) - 6.0 * 23.0 / 0.7).abs() < 1e-9);
    // A shot takes its first salvo's delay, the time between its salvos (0.25 s
    // on average) and the shot delay.
    let slow = SpaceWeaponSpec { initial_salvo_delay: 0.15, salvo_delay: 0.2, salvo_delay_max: 0.3, ..cannon };
    assert!((object_rate(&slow, false) - 6.0 * 23.0 / (0.15 + 0.25 + 0.7)).abs() < 1e-9);
}

#[test]
fn a_lock_weapon_counts_only_against_objects_that_can_be_locked() {
    // A fighter whose gun is for craft and whose missile for objects fires on
    // a lock; a bomber whose bombs need none.
    let gun = || weapon(0, true, false, 0.0, 870.0);
    let missile = SpaceWeaponSpec { max_damage: 37.0, shot_delay: 0.074, lock_on_range: 415.0, ..weapon(1, true, true, 163.0, 1170.0) };
    let bomb = SpaceWeaponSpec { max_damage: 2470.0, shot_delay: 0.13, ..weapon(1, false, true, 23.0, 265.0) };
    let fighter = SpaceFlightProfile { weapons: vec![gun(), missile], ..Default::default() };
    let bomber = SpaceFlightProfile { weapons: vec![gun(), bomb], ..Default::default() };
    let (lf, lb) = (Loadout::of(&fighter), Loadout::of(&bomber));
    let me = Traffic { id: 0, generation: 1, team: 1, position: [0.0; 3], velocity: [0.0; 3], reach: 4.0 };
    let object = |index: usize, lockable: bool, z: f64| System {
        index,
        life: index as f64,
        team: 2,
        center: [0.0, 0.0, z],
        radius: 23.0,
        bodies: [-1; 3],
        gun_reach: 0.0,
        lockable,
        shield: 0.0,
        exposure: 0.0,
        hull: false,
        left: 1.0,
    };
    let (near, far) = (object(41, false, 300.0), object(42, true, 700.0));
    let pick = |systems: &[System], l: &Loadout| choose(&[], systems, l, &me, [0.0, 0.0, 1.0])[0];
    // The nearer object cannot be locked: the fighter takes the farther one,
    // and nothing when the nearer is all there is.
    assert_eq!(pick(&[near, far], &lf), PilotTarget::Building { index: 42, life: 42.0 });
    assert_eq!(pick(&[near], &lf), PilotTarget::None);
    assert_eq!(pick(&[near, far], &lb), PilotTarget::Building { index: 41, life: 41.0 });
    // The weapon a run is laid out for, shield up or down.
    for shielded in [false, true] {
        assert_eq!(object_weapon(&fighter, &lf, shielded, false), None);
        assert_eq!(object_weapon(&fighter, &lf, shielded, true), Some(1));
        assert_eq!(object_weapon(&bomber, &lb, shielded, false), Some(1));
    }
}

#[test]
fn a_dogfight_without_a_shot_is_given_up_for_a_while() {
    let limit = (PROVISIONAL_NO_SHOT_LIMIT_S / crate::space::SPACE_FIXED_DT) as u32;
    let other = PilotTarget::Craft { index: 3, generation: 2 };
    let mut bot = Bot { target: other, shot_tick: 100, ..Bot::fresh(1.0, 0) };
    assert!(!bot.no_shot(100 + limit), "not before the limit");
    assert!(bot.no_shot(100 + limit + 1));
    bot.give_up(100 + limit + 1);
    assert_eq!(bot.target, PilotTarget::None);
    assert!(bot.shuns(other, 100 + limit + 2));
    assert!(!bot.shuns(PilotTarget::Craft { index: 3, generation: 3 }, 100 + limit + 2), "a new craft on that pad is fair game");
    assert!(!bot.shuns(other, bot.shun_until));
    // Runs on ship objects have their own way of coming round.
    let run = Bot { target: PilotTarget::Building { index: 1, life: 1.0 }, ..bot };
    assert!(!run.no_shot(u32::MAX / 2));
}

fn craft() -> (SpaceFighter, SpaceFlightProfile) {
    let profile = SpaceFlightProfile { contact_bounds_min: [-4.0, -1.0, -5.0], contact_bounds_max: [4.0, 2.0, 5.0], ..Default::default() };
    let f = SpaceFighter::spawned_on_pad(0, 1, 0, &pad(1, [100.0, 10.0, 100.0]), 0, &profile, 10.0, None, None);
    (f, profile)
}

#[test]
fn a_walker_heads_for_the_nearest_side_of_the_hull() {
    let (f, profile) = craft();
    // From the right, the point lies just right of the right wing on the deck.
    let p = board_point(&f, &profile, [130.0, 10.0, 101.0]);
    assert!((p[0] - (f.position[0] + 4.0 + PROVISIONAL_BOARD_STANDOFF)).abs() < 1e-9, "{p:?}");
    assert!((p[2] - 101.0).abs() < 1e-9);
    assert!((p[1] - (f.position[1] - 1.0)).abs() < 1e-9);
    // From under the nose, out to the nearer side.
    let p = board_point(&f, &profile, [100.5, 10.0, 104.5]);
    let local = sub(p, f.position);
    assert!(local[2] > 5.0 || local[0].abs() > 4.0, "{local:?}");
}

#[test]
fn inside_a_hangar_a_pilot_turns_before_it_creeps_and_brakes_after_contact() {
    let (f, profile) = craft();
    let view = f.clone_view();
    let pv = ProfileView::of(&profile);
    let ahead = view.forward;
    let side = [view.right[0], view.right[1], view.right[2]];
    assert_eq!(hangar_throttle(&view, &pv, side, true, false), 0.0, "hover while turning");
    assert!(hangar_throttle(&view, &pv, ahead, true, false) > 0.0, "creep when facing the way");
    assert_eq!(hangar_throttle(&view, &pv, ahead, true, true), 0.0, "brake after a contact");
    assert!(hangar_throttle(&view, &pv, ahead, false, false) > 0.0, "outside: up to the slowest speed");
    let fast = CraftView { airspeed: PROVISIONAL_HANGAR_SPEED + 1.0, ..view };
    assert_eq!(hangar_throttle(&fast, &pv, ahead, true, false), 0.0, "no throttle above the hangar pace");
    // Positive yaw stick turns away from local +X, as the flight model does.
    let (sx, sy) = stick_toward(&view, side);
    assert!(sx < 0.0 && sy.abs() < 1e-9, "{sx} {sy}");
    assert!(dot(ahead, view.forward) > 0.999);
}

// Battles on invented decks: craft parked on pads of team 1, bots on foot
// beside them, walls as solid boxes.

use crate::gen::Unit as UnitRow;
use crate::space::tests::{add_synthetic_box, make_synthetic_f3_world, prepare_synthetic_tail};
use crate::space::world_anim::SpaceWorldAnims;
use crate::space::{SpaceBattlefield, SPACE_FIXED_DT};
use crate::store::Ref;

/// An invented fighter (every flight figure made up for the tests).
fn fighter_profile() -> SpaceFlightProfile {
    SpaceFlightProfile {
        contact_bounds_min: [-3.0, -1.0, -4.0],
        contact_bounds_max: [3.0, 1.0, 4.0],
        seat_count: 1,
        max_health: 2150.0,
        min_speed: 31.0,
        mid_speed: 58.0,
        max_speed: 81.0,
        boost_speed: 118.0,
        acceleration: 37.5,
        turn_rate: 1.12,
        pitch_rate: 1.05,
        roll_rate: 2.9,
        pilot_skill_repair_scale: 0.023,
        ..Default::default()
    }
}

/// A scene: craft of either team (profile index, team, position), bots on
/// foot (team, position), walls, and ship objects made by `objects`.
fn scene(
    profiles: Vec<SpaceFlightProfile>,
    craft: &[(usize, i32, [f64; 3])],
    bots: &[(i32, [f64; 3])],
    walls: &[([f64; 3], [f64; 3])],
    objects: impl FnOnce(&mut World, usize) -> Vec<crate::space::objects::SpaceBuildingAttachment>,
) -> (Box<World>, Vec<F3UnitKey>) {
    let (mut w, coll) = make_synthetic_f3_world();
    for (lo, hi) in walls {
        add_synthetic_box(&mut w, coll, *lo, *hi, true);
    }
    let attachments = objects(&mut w, coll);
    // Craft left empty in flight do not decay during a test.
    let pads: Vec<SpaceVehiclePad> = craft
        .iter()
        .map(|(p, team, at)| SpaceVehiclePad { profile_index: *p, expire_time_enemy: 1e6, expire_time_field: 1e6, decay_time: 1e6, ..pad(*team, *at) })
        .collect();
    let fighters: Vec<SpaceFighter> = pads
        .iter()
        .enumerate()
        .map(|(i, p)| SpaceFighter::spawned_on_pad(i, 1, i, p, p.profile_index, &profiles[p.profile_index], 0.0, None, None))
        .collect();
    let buildings = attachments.len();
    let mut space = SpaceBattlefield::from_parts(
        SpaceBounds::from_world_json(&parse("{}").unwrap()),
        SpaceWorldAnims::default(),
        profiles,
        pads,
        fighters,
        attachments,
        vec![],
        coll,
        w.collision.state.bodies.len(),
    );
    space.pilots = PilotBots::build(&space.bounds, &space.pads, &space.fighters, &space.profiles, &parse("{}").unwrap(), 64, buildings);
    w.space = Some(Box::new(space));
    let keys = bots
        .iter()
        .map(|(team, p)| {
            let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
            soldier.position = *p;
            let mut unit = UnitRow { alive: true, team: f64::from(*team), soldier: UnitBody::Soldier(soldier), controller: Some(Ref::new(0, 1)), ..Default::default() };
            unit.combat.health = 117.0;
            unit.combat.max_health = 117.0;
            crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit))
        })
        .collect();
    prepare_synthetic_tail(&mut w);
    (w, keys)
}

fn deck(craft: &[[f64; 3]], bots: &[[f64; 3]], walls: &[([f64; 3], [f64; 3])]) -> (Box<World>, Vec<F3UnitKey>) {
    let craft: Vec<(usize, i32, [f64; 3])> = craft.iter().map(|c| (0, 1, *c)).collect();
    let bots: Vec<(i32, [f64; 3])> = bots.iter().map(|b| (1, *b)).collect();
    scene(vec![fighter_profile()], &craft, &bots, walls, |_, _| Vec::new())
}

/// Puts craft `craft` in flight at `at`, still, with no one aboard.
fn fly(w: &mut World, craft: usize, at: [f64; 3]) {
    let f = &mut w.space_mut().unwrap().fighters[craft];
    f.state = SpaceFlightState::Flying;
    f.position = at;
    f.velocity = [0.0; 3];
    f.airspeed = 0.0;
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        after_ai_in(w, SPACE_FIXED_DT);
    }
}

/// Seats bot `key` in craft `craft`, aloft and flying, in phase `phase`.
fn aloft(w: &mut World, key: &F3UnitKey, craft: usize, phase: PilotPhase) {
    let seat = w.space_board_seat(key, craft, 0).expect("the bot stands in reach of its craft");
    let life = w.sim.store.unit.get(key.reference()).unwrap().generation;
    let space = w.space_mut().unwrap();
    let f = &mut space.fighters[craft];
    f.state = SpaceFlightState::Flying;
    f.position[1] += 300.0;
    f.velocity = [0.0, 0.0, 47.0];
    f.airspeed = 47.0;
    let slot = key.reference().slot as usize;
    space.pilots.bots[slot] = Bot { phase, receipt: Some(seat), craft, ..Bot::fresh(life, 0) };
}

#[test]
fn a_bot_pilot_near_the_edge_turns_back_inside() {
    // Nothing turns a craft at the boundary, so a bot flying out of it turns
    // back itself: 100 m inside the east edge, heading 45 degrees out.
    let (mut w, bots) = deck(&[[0.0, 0.0, 0.0]], &[[4.0, 0.0, 0.0]], &[]);
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    let edge = space.bounds.max[0];
    let f = &mut space.fighters[0];
    f.position = [edge - 100.0, 300.0, 0.0];
    let q = crate::space::flight::quat_from_axis_angle([0.0, 1.0, 0.0], std::f64::consts::FRAC_PI_4);
    f.quaternion = [q.x, q.y, q.z, q.w];
    let fwd = f.forward();
    f.velocity = [fwd[0] * 47.0, fwd[1] * 47.0, fwd[2] * 47.0];
    let inward = |w: &World| {
        let f = &w.space().unwrap().fighters[0];
        let to_middle = unit(sub(w.space().unwrap().bounds.center, f.position));
        dot(f.forward(), to_middle)
    };
    let start = inward(&w);
    assert!(start < -0.6, "heading out: {start}");
    let mut farthest: f64 = 0.0;
    for _ in 0..240 {
        run(&mut w, 1);
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        farthest = farthest.max(w.space().unwrap().fighters[0].position[0]);
    }
    let end = inward(&w);
    assert!(end > 0.5, "turned back toward the middle: {start} -> {end}");
    assert!(farthest < edge + 50.0, "went {} m past the edge", farthest - edge);
    assert!(w.space().unwrap().fighters[0].alive);
}

#[test]
fn a_free_bot_takes_the_next_seat_when_the_walk_to_the_nearest_is_blocked() {
    // The nearest craft stands behind a wall, a farther one in the open.
    let (mut w, bots) = deck(&[[12.0, 0.0, 0.0], [-25.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]], &[([4.0, -5.0, -6.0], [6.0, 5.0, 6.0])]);
    run(&mut w, 31);
    let view = w.space_pilot_view(bots[0].reference()).expect("steered by the pilots");
    assert_eq!((view.phase, view.craft), (PilotPhase::Walk, Some(1)));
    assert!(w.space().unwrap().pilots.census.blocked_seats >= 1);
}

#[test]
fn a_hold_ends_a_walk_names_one_life_and_refuses_a_stale_reference() {
    let (mut w, bots) = deck(&[[12.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]], &[]);
    let me = bots[0].reference();
    run(&mut w, 31);
    assert_eq!(w.space_pilot_view(me).map(|v| v.phase), Some(PilotPhase::Walk));
    assert!(w.space_pilot_hold(me, true));
    run(&mut w, 61);
    assert_eq!(w.space_pilot_view(me).map(|v| v.phase), Some(PilotPhase::Free), "held: back on foot and not lured again");
    assert_eq!(w.space().unwrap().pilots.claims[0][0], 0, "the seat is free for another");
    // A reference to another occupant of the slot is refused.
    let stale = Ref::<UnitRow>::new(me.slot, me.gen + 1);
    assert!(!w.space_pilot_hold(stale, true));
    assert_eq!(w.space().unwrap().pilots.census.holds_refused, 1);
    // The next life in the slot starts unheld and is lured again.
    w.sim.store.unit.get_mut(me).unwrap().generation += 1.0;
    run(&mut w, 61);
    assert_eq!(w.space_pilot_view(me).map(|v| v.phase), Some(PilotPhase::Walk));
}

#[test]
fn a_pilot_with_nothing_to_fight_searches_on_its_own_think_ticks_only() {
    // Eight craft aloft over an empty field.
    let craft: Vec<[f64; 3]> = (0..8).map(|i| [f64::from(i) * 40.0, 0.0, 0.0]).collect();
    let crew: Vec<[f64; 3]> = craft.iter().map(|c| [c[0] + 4.0, c[1], c[2]]).collect();
    let (mut w, bots) = deck(&craft, &crew, &[]);
    for (i, key) in bots.iter().enumerate() {
        aloft(&mut w, key, i, PilotPhase::Fight);
    }
    let before = w.space().unwrap().pilots.census.target_searches;
    run(&mut w, 60);
    let searches = w.space().unwrap().pilots.census.target_searches - before;
    assert!((8..=16).contains(&searches), "{searches} searches by eight pilots in 60 ticks");
}

#[test]
fn a_launch_path_node_that_is_never_reached_is_skipped() {
    let (mut w, bots) = deck(&[[0.0, 0.0, 0.0]], &[[4.0, 0.0, 0.0]], &[]);
    aloft(&mut w, &bots[0], 0, PilotPhase::Launch);
    let space = w.space_mut().unwrap();
    space.pilots.routes = Routes { exits: vec![super::routes::Exit { team: 1, path: (0, 2), ..Default::default() }], nodes: vec![[0.0, 300.0, 8700.0], [0.0, 300.0, 9900.0]] };
    let slot = bots[0].reference().slot as usize;
    space.pilots.bots[slot].exit = 0;
    space.pilots.bots[slot].path_end = 2;
    // The craft never moves here (no flight step), so the node is never reached.
    run(&mut w, (PROVISIONAL_NODE_LIMIT_S * 60.0) as u32 + 2);
    let p = &w.space().unwrap().pilots;
    assert_eq!(p.census.nodes_skipped, 1);
    assert_eq!(p.bots[slot].node, 1);
}

#[test]
fn a_path_name_with_a_wide_character_at_the_tag_boundary_is_not_a_flyer_path() {
    let bounds = SpaceBounds::from_world_json(&parse("{}").unwrap());
    let world = parse(r#"{"paths":[{"name":"type_entitypathéx","nodes":[{"position":[0,0,0]},{"position":[0,0,9]}]}]}"#).unwrap();
    assert!(Routes::build(&bounds, &[], &world).nodes.is_empty());
}

fn gunship() -> SpaceFlightProfile {
    let gun = |seat: u8, channel: u8| SpaceWeaponSpec { seat, channel, ai_targets_vehicle: true, max_range: 955.0, velocity: 1870.0, ..Default::default() };
    SpaceFlightProfile { seat_count: 2, weapons: vec![gun(0, 0), gun(1, 2)], ..fighter_profile() }
}

#[test]
fn a_gunner_holds_fire_through_a_wall_or_a_friendly_craft() {
    // Our transport at the origin with a gunner aboard; an enemy craft north
    // behind a wall; later the enemy south with one of our craft in the line.
    let craft = [(0, 1, [0.0, 0.0, 0.0]), (0, 2, [0.0, 0.0, 150.0]), (0, 1, [0.0, 0.0, -400.0])];
    let (mut w, bots) = scene(vec![gunship()], &craft, &[(1, [4.0, 0.0, 0.0])], &[([-20.0, -20.0, 60.0], [20.0, 20.0, 70.0])], |_, _| Vec::new());
    let seat = w.space_board_seat(&bots[0], 0, 1).expect("in reach of the gunner seat");
    let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
    let slot = bots[0].reference().slot as usize;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Gunner, receipt: Some(seat), craft: 0, seat: 1, ..Bot::fresh(life, 0) };
    fly(&mut w, 0, [0.0, 0.0, 0.0]);
    fly(&mut w, 1, [0.0, 0.0, 150.0]);
    run(&mut w, 61);
    let fired = |w: &World| w.space().unwrap().fighters[0].gunner_inputs[0].fire_primary;
    assert_eq!(w.space().unwrap().pilots.bots[slot].target, PilotTarget::None, "an enemy behind the wall is not chosen");
    assert!(!fired(&w));
    // South, in the open, but our own craft is in the line.
    fly(&mut w, 1, [0.0, 0.0, -150.0]);
    fly(&mut w, 2, [0.0, 0.0, -75.0]);
    run(&mut w, 61);
    assert!(!fired(&w), "no round toward our own craft");
    // The line clears: the gunner takes the enemy and fires once its sight is on.
    fly(&mut w, 2, [0.0, 0.0, 400.0]);
    let mut shot = false;
    for _ in 0..240 {
        after_ai_in(&mut w, SPACE_FIXED_DT);
        shot |= fired(&w);
    }
    assert!(matches!(w.space().unwrap().pilots.bots[slot].target, PilotTarget::Craft { index: 1, .. }));
    assert!(shot);
}

#[test]
fn a_gunner_fires_a_beam_within_its_weapons_range_and_never_ahead_of_its_target() {
    use crate::space::flight::{parse_space_weapon, SpaceOrdnanceKind};
    // Each tick's aim and trigger of a gunner whose gun `json` describes, at
    // an enemy craft held at `at` with a speed.
    let aims = |json: &str, at: [f64; 3], velocity: [f64; 3]| {
        let gun = SpaceWeaponSpec { seat: 1, channel: 2, max_damage: 0.0, ..parse_space_weapon(&parse(json).unwrap(), 0) };
        let mut profile = gunship();
        profile.weapons[1] = gun;
        let craft = [(0, 1, [0.0; 3]), (0, 2, at)];
        let (mut w, bots) = scene(vec![profile], &craft, &[(1, [4.0, 0.0, 0.0])], &[], |_, _| Vec::new());
        let seat = w.space_board_seat(&bots[0], 0, 1).expect("in reach of the gunner seat");
        let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
        let slot = bots[0].reference().slot as usize;
        w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Gunner, receipt: Some(seat), craft: 0, seat: 1, ..Bot::fresh(life, 0) };
        (0..180)
            .map(|_| {
                fly(&mut w, 0, [0.0; 3]);
                fly(&mut w, 1, at);
                w.space_mut().unwrap().fighters[1].velocity = velocity;
                after_ai_in(&mut w, SPACE_FIXED_DT);
                let input = w.space().unwrap().fighters[0].gunner_inputs[0];
                (input.aim, input.fire_primary)
            })
            .collect::<Vec<_>>()
    };
    let fires = |seen: &[(Option<[f64; 3]>, bool)]| seen.iter().any(|&(_, fire)| fire);
    let (near, far, crossing) = ([60.0, 0.0, -380.0], [60.0, 0.0, -560.0], [70.0, 0.0, 0.0]);
    // An invented beam that reaches 1130 m, for use within 470 m.
    let beam = r#"{"class":"test_beam","properties":[["ShotDelay","0.23"],["MaxRange","470"],["AITargetVehicle","1"]],"ordnance":{"base":"beam","properties":[["Range","1130"]]}}"#;
    assert_eq!(parse_space_weapon(&parse(beam).unwrap(), 0).ordnance_kind, SpaceOrdnanceKind::Beam);
    let still = aims(beam, near, [0.0; 3]);
    assert!(fires(&still));
    assert!(!fires(&aims(beam, far, [0.0; 3])), "past the range its class gives bots");
    // It strikes at once, so a crossing craft is aimed at where it is; rounds
    // that take their time are aimed ahead of it.
    assert_eq!(aims(beam, near, crossing), still);
    let rounds = r#"{"class":"test_rounds","properties":[["ShotDelay","0.23"],["MaxRange","470"],["AITargetVehicle","1"]],"ordnance":{"base":"bolt","properties":[["Velocity","640"],["LifeSpan","0.95"]]}}"#;
    assert_ne!(aims(rounds, near, crossing), aims(rounds, near, [0.0; 3]));
}

#[test]
fn a_bot_gunner_aims_off_a_crafts_lead_by_its_sphere_of_miss_and_less_when_bots_ride_it() {
    use crate::space::{aim::MissBubble, flight::SpacePilotId};
    // How far, in radians, a bot gunner's sight lies off a craft held still
    // 290 m away after a second, the craft ridden by a human or by a bot.
    let off = |human: bool| {
        let gun = SpaceWeaponSpec { seat: 1, channel: 2, max_damage: 0.0, velocity: 640.0, life_span: 0.95, max_range: 470.0,
            ai_targets_vehicle: true, ai_bubble: MissBubble { size: 0.5, divider: 60.0, clamp: 4.0 }, ..Default::default() };
        let mut profile = gunship();
        profile.weapons[1] = gun;
        (profile.bounds_min, profile.bounds_max) = ([-6.0, -2.0, -6.0], [6.0, 2.0, 6.0]);
        let at = [-260.0, 20.0, 120.0];
        let craft = [(0, 1, [0.0; 3]), (0, 2, at)];
        let (mut w, bots) = scene(vec![profile], &craft, &[(1, [4.0, 0.0, 0.0]), (2, [0.0, 900.0, 0.0])], &[], |_, _| Vec::new());
        let seat = w.space_board_seat(&bots[0], 0, 1).expect("in reach of the gunner seat");
        let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
        let slot = bots[0].reference().slot as usize;
        w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Gunner, receipt: Some(seat), craft: 0, seat: 1, ..Bot::fresh(life, 0) };
        let owner = w.space_launch_owner(&bots[1]).unwrap();
        w.space_mut().unwrap().fighters[1].pilot = Some(SpacePilotId { owner, team: 2, is_human: human, repair_capable: false });
        for _ in 0..60 {
            fly(&mut w, 0, [0.0; 3]);
            fly(&mut w, 1, at);
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            after_ai_in(&mut w, SPACE_FIXED_DT);
        }
        let sight = w.space().unwrap().fighters[0].gunner_inputs[0].aim.expect("the gunner aims");
        let exact = unit(at);
        v8math::acos((sight[0] * exact[0] + sight[1] * exact[1] + sight[2] * exact[2]).clamp(-1.0, 1.0))
    };
    let (human, bot) = (off(true), off(false));
    assert!(human > 0.01, "the gunner aims at a human's craft exactly ({human} rad)");
    assert!((bot / human - crate::space::aim::BOT_TARGET_SHARE).abs() < 0.05, "bot {bot} rad against human {human} rad");
}

#[test]
fn a_chair_bot_aims_its_gun_off_a_crafts_lead_by_its_sphere_of_miss_and_less_when_bots_ride_it() {
    use crate::space::{aim::MissBubble, flight::SpacePilotId, objects::{SpaceTerminalState, SpaceTurretPilotType}};
    // How far, in radians, the aim a chair bot hands its gun lies off a craft
    // held still about 290 m away after five seconds, ridden by a human or a bot.
    let off = |human: bool| {
        let at = [-260.0, 20.0, 120.0];
        let mut profile = fighter_profile();
        (profile.bounds_min, profile.bounds_max) = ([-6.0, -2.0, -6.0], [6.0, 2.0, 6.0]);
        let (mut w, bots) = scene(vec![profile], &[(0, 2, at)], &[(1, [7.0, 0.0, 4.0]), (2, [0.0, 900.0, 0.0])], &[], |w, _| {
            crate::space::tests::add_synthetic_building(w, 0, 0.0, 410.0, [0.0; 3], [None; 3], false);
            crate::space::tests::add_synthetic_building(w, 1, 1.0, 410.0, [7.0, 0.0, 1.0], [None; 3], false);
            let mut remote = gun(0, [0.0; 3]);
            let t = remote.turret.as_mut().unwrap();
            t.pilot_type = SpaceTurretPilotType::Remote;
            t.weapon.ai_bubble = MissBubble { size: 0.5, divider: 60.0, clamp: 4.0 };
            let mut chair = gun(1, [7.0, 0.0, 1.0]);
            chair.turret = None;
            chair.base = "remoteterminal".into();
            chair.terminal = Some(SpaceTerminalState { remote_turret: Some(0), next_terminal: Some(1), prev_terminal: Some(1), ..Default::default() });
            vec![remote, chair]
        });
        let seat = w.space_board_terminal(&bots[0], 1).expect("the bot stands at the chair");
        let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
        let slot = bots[0].reference().slot as usize;
        w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Chair, terminal: Some(seat), chair: 1, ..Bot::fresh(life, 0) };
        let owner = w.space_launch_owner(&bots[1]).unwrap();
        w.space_mut().unwrap().fighters[0].pilot = Some(SpacePilotId { owner, team: 2, is_human: human, repair_capable: false });
        for _ in 0..300 {
            fly(&mut w, 0, at);
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            after_ai_in(&mut w, SPACE_FIXED_DT);
        }
        let sight = w.space().unwrap().buildings[0].turret.as_ref().unwrap().pending_remote_aim.expect("the bot aims the gun");
        let exact = unit(at);
        v8math::acos((sight[0] * exact[0] + sight[1] * exact[1] + sight[2] * exact[2]).clamp(-1.0, 1.0))
    };
    let (human, bot) = (off(true), off(false));
    assert!(human > 0.01, "the chair's gun is aimed at a human's craft exactly ({human} rad)");
    assert!((bot / human - crate::space::aim::BOT_TARGET_SHARE).abs() < 0.05, "bot {bot} rad against human {human} rad");
}

/// A bot seated at a remote turret chair 40 m from its gun, which fires
/// `weapon`; an enemy craft waits north behind a wall. Returns the bot's slot.
fn chair_scene(weapon: SpaceWeaponSpec) -> (Box<World>, usize) {
    use crate::space::combat::SpaceDamageCategory;
    use crate::space::objects::{SpaceBuildingAttachment, SpaceTerminalState, SpaceTurretPilotType, SpaceTurretState, TurretTargetFilter};
    use crate::space::tests::add_synthetic_building;
    use three::Matrix4;
    let object = |index: usize, name: &str, at: [f64; 3], turret: Option<SpaceTurretState>, terminal: Option<SpaceTerminalState>| {
        let mut m = Matrix4::default().elements;
        m[12..15].copy_from_slice(&at);
        SpaceBuildingAttachment {
            building_index: index,
            instance: index,
            name: name.into(),
            class: name.into(),
            base: if terminal.is_some() { "remoteterminal".into() } else { "armedbuilding".into() },
            bodies: [None; 3],
            animated_index: None,
            matrix: m,
            position: at,
            center: at,
            local_center_offset: [0.0; 3],
            radius: 2.3,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret,
            terminal,
        }
    };
    let gun = SpaceTurretState {
        pilot_type: SpaceTurretPilotType::Remote,
        weapon,
        filter: TurretTargetFilter { target_person: false, target_vehicle: true, target_building: false, min_range: 0.0, max_range: 470.0 },
        yaw: 0.0,
        pitch: 0.0,
        prev_yaw: 0.0,
        prev_pitch: 0.0,
        yaw_min: -std::f64::consts::PI,
        yaw_max: std::f64::consts::PI,
        pitch_min: -1.0,
        pitch_max: 1.0,
        turn_rate: 3.7,
        pitch_rate: 3.7,
        clock: Default::default(),
        barrage_remaining: 0,
        barrage_timer: 0.0,
        trigger_was_held: false,
        warm_up: 0.0,
        heat: 0.0,
        overheated: false,
        shots_fired: 0,
        muzzle_offsets: Vec::new(),
        remote_operator: None,
        pending_remote_aim: None,
        pending_remote_fire: false,
        pending_remote_spread_scale: None,
        kick: Default::default(),
    };
    let chair = SpaceTerminalState {
        remote_object_name: String::new(),
        next_terminal_name: String::new(),
        prev_terminal_name: String::new(),
        remote_turret: Some(0),
        next_terminal: Some(1),
        prev_terminal: Some(1),
        occupant: None,
        seat: None,
        feet: None,
    };
    // The gun at the origin, its chair 40 m away; an enemy craft north behind a wall.
    let craft = [(0, 2, [0.0, 0.0, 120.0])];
    let (mut w, bots) = scene(vec![fighter_profile()], &craft, &[(1, [40.0, 0.0, 1.0])], &[([-20.0, -20.0, 50.0], [20.0, 20.0, 60.0])], |w, _| {
        add_synthetic_building(w, 0, 0.0, 330.0, [0.0; 3], [None; 3], false);
        add_synthetic_building(w, 1, 1.0, 330.0, [40.0, 0.0, 0.0], [None; 3], false);
        vec![object(0, "gun", [0.0; 3], Some(gun), None), object(1, "chair", [40.0, 0.0, 0.0], None, Some(chair))]
    });
    let seat = w.space_board_terminal(&bots[0], 1).expect("the bot stands at the chair");
    let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
    let slot = bots[0].reference().slot as usize;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Chair, terminal: Some(seat), chair: 1, ..Bot::fresh(life, 0) };
    fly(&mut w, 0, [0.0, 0.0, 120.0]);
    (w, slot)
}

#[test]
fn a_chair_bot_with_no_target_leaves_after_ten_seconds() {
    // The only enemy craft is behind a wall: the gun never has a target.
    let (mut w, slot) = chair_scene(SpaceWeaponSpec { shot_delay: 0.31, velocity: 530.0, life_span: 0.8, max_damage: 9.0, max_range: 470.0, ..Default::default() });
    let phase = |w: &World| w.space().unwrap().pilots.bots[slot].phase;
    for _ in 0..(9 * 60) {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    assert_eq!(phase(&w), PilotPhase::Chair, "still seated after nine idle seconds");
    for _ in 0..(2 * 60) {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    assert_ne!(phase(&w), PilotPhase::Chair, "gone after eleven idle seconds");
}

#[test]
fn a_free_bot_takes_a_chair_only_with_a_target_unless_the_mission_keeps_bots_in_turrets() {
    use crate::space::objects::{SpaceTerminalState, SpaceTurretPilotType};
    // A free chair 3 m from a bot on its own ship, its gun beside it; no craft
    // waits and the bot's controller has no target.
    let (mut w, bots) = scene(vec![fighter_profile()], &[], &[(1, [7.0, 0.0, 4.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 0.0, 410.0, [0.0; 3], [None; 3], false);
        crate::space::tests::add_synthetic_building(w, 1, 1.0, 410.0, [7.0, 0.0, 1.0], [None; 3], false);
        let mut remote = gun(0, [0.0; 3]);
        remote.turret.as_mut().unwrap().pilot_type = SpaceTurretPilotType::Remote;
        let mut chair = gun(1, [7.0, 0.0, 1.0]);
        chair.turret = None;
        chair.base = "remoteterminal".into();
        chair.terminal = Some(SpaceTerminalState { remote_turret: Some(0), next_terminal: Some(1), prev_terminal: Some(1), ..Default::default() });
        vec![remote, chair]
    });
    w.space_mut().unwrap().pilots.routes.exits.push(super::routes::Exit { team: 1, center: [0.0, 0.0, -20.0], ..Default::default() });
    let slot = bots[0].reference().slot as usize;
    let run = |w: &mut World| {
        for _ in 0..120 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            after_ai_in(w, SPACE_FIXED_DT);
        }
        w.space().unwrap().pilots.bots[slot].phase
    };
    assert_eq!(run(&mut w), PilotPhase::Free, "no target, no chair");
    assert!(w.space_set_pilot_stay_in_turrets(true));
    assert!(matches!(run(&mut w), PilotPhase::ToChair | PilotPhase::Chair), "kept in turrets, the bot takes it");
}

#[test]
fn a_chair_that_shows_its_operator_seats_his_body_and_lets_him_up_where_he_stood() {
    use crate::space::objects::{SpaceTerminalState, SpaceTurretPilotType};
    // A chair turned a quarter round, its seat 0.6 m up and 0.2 m along its
    // own forward; the soldier stands 1.5 m beside it.
    let (mut w, bots) = scene(vec![fighter_profile()], &[], &[(1, [11.5, 0.0, 3.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 0.0, 410.0, [0.0; 3], [None; 3], false);
        crate::space::tests::add_synthetic_building(w, 1, 1.0, 410.0, [10.0, 0.0, 3.0], [None; 3], false);
        let mut remote = gun(0, [0.0; 3]);
        remote.turret.as_mut().unwrap().pilot_type = SpaceTurretPilotType::Remote;
        let mut chair = gun(1, [10.0, 0.0, 3.0]);
        chair.turret = None;
        chair.base = "remoteterminal".into();
        // Local +Z of the chair points along world +X.
        chair.matrix[0..3].copy_from_slice(&[0.0, 0.0, -1.0]);
        chair.matrix[8..11].copy_from_slice(&[1.0, 0.0, 0.0]);
        chair.terminal = Some(SpaceTerminalState { remote_turret: Some(0), next_terminal: Some(1), prev_terminal: Some(1),
            seat: Some([0.0, 0.6, 0.2]), ..Default::default() });
        vec![remote, chair]
    });
    let body = |w: &World| match &w.sim.store.unit.get(bots[0].reference()).unwrap().soldier {
        UnitBody::Soldier(s) => s.position,
        _ => unreachable!(),
    };
    let seat = w.space_board_terminal(&bots[0], 1).expect("the soldier stands at the chair");
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
    assert!(near(body(&w), [10.2, 0.6, 3.0]), "seated at {:?}", body(&w));
    // The unit step holds him there whatever his input.
    let slot = bots[0].reference().slot;
    let mut row = w.sim.store.unit.get(bots[0].reference()).unwrap().clone();
    if let UnitBody::Soldier(s) = &mut row.soldier { s.position = [0.0; 3]; }
    assert!(crate::space::move_mounted(&w, slot, &mut row));
    assert!(matches!(&row.soldier, UnitBody::Soldier(s) if near(s.position, [10.2, 0.6, 3.0])));
    assert!(w.space_leave_terminal(seat));
    assert!(near(body(&w), [11.5, 0.0, 3.0]), "up at {:?}", body(&w));
    let mut row = w.sim.store.unit.get(bots[0].reference()).unwrap().clone();
    assert!(!crate::space::move_mounted(&w, slot, &mut row), "free again");
}

#[test]
fn a_free_bot_takes_a_chair_for_an_enemy_craft_it_sees_in_front_within_its_sight_range() {
    use crate::space::{flight::SpacePilotId, objects::{SpaceTerminalState, SpaceTurretPilotType}};
    // A free chair 3 m from a bot facing north; an enemy craft with a bot
    // aboard at `at`; its model sphere 5 m, so it is seen to 130 m.
    let phase = |at: [f64; 3]| {
        let mut profile = fighter_profile();
        profile.sphere = [0.0, 0.0, 0.0, 5.0];
        let (mut w, bots) = scene(vec![profile], &[(0, 2, at)], &[(1, [7.0, 0.0, 4.0]), (2, [0.0, 900.0, 0.0])], &[], |w, _| {
            crate::space::tests::add_synthetic_building(w, 0, 0.0, 410.0, [0.0; 3], [None; 3], false);
            crate::space::tests::add_synthetic_building(w, 1, 1.0, 410.0, [7.0, 0.0, 1.0], [None; 3], false);
            let mut remote = gun(0, [0.0; 3]);
            remote.turret.as_mut().unwrap().pilot_type = SpaceTurretPilotType::Remote;
            let mut chair = gun(1, [7.0, 0.0, 1.0]);
            chair.turret = None;
            chair.base = "remoteterminal".into();
            chair.terminal = Some(SpaceTerminalState { remote_turret: Some(0), next_terminal: Some(1), prev_terminal: Some(1), ..Default::default() });
            vec![remote, chair]
        });
        w.space_mut().unwrap().pilots.routes.exits.push(super::routes::Exit { team: 1, center: [0.0, 0.0, -20.0], ..Default::default() });
        let owner = w.space_launch_owner(&bots[1]).unwrap();
        w.space_mut().unwrap().fighters[0].pilot = Some(SpacePilotId { owner, team: 2, is_human: false, repair_capable: false });
        let slot = bots[0].reference().slot as usize;
        for _ in 0..120 {
            fly(&mut w, 0, at);
            after_ai_in(&mut w, SPACE_FIXED_DT);
        }
        w.space().unwrap().pilots.bots[slot].phase
    };
    assert!(matches!(phase([7.0, 20.0, 120.0]), PilotPhase::ToChair | PilotPhase::Chair), "seen in front: the bot takes the chair");
    assert_eq!(phase([7.0, 20.0, -110.0]), PilotPhase::Free, "behind him: not seen");
    assert_eq!(phase([7.0, 20.0, 150.0]), PilotPhase::Free, "beyond the sight range");
}

#[test]
fn a_chair_gun_holds_fire_through_a_wall_and_lets_go_when_its_target_is_gone() {
    let (mut w, slot) = chair_scene(SpaceWeaponSpec { shot_delay: 0.23, velocity: 640.0, life_span: 0.95, max_damage: 11.0, max_range: 470.0, ..Default::default() });
    let step = |w: &mut World, ticks: u32| {
        let mut held = false;
        for _ in 0..ticks {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            after_ai_in(w, SPACE_FIXED_DT);
            held |= w.space().unwrap().buildings[0].turret.as_ref().unwrap().pending_remote_fire;
        }
        held
    };
    assert!(!step(&mut w, 61), "never a shot at a craft behind the wall");
    assert_eq!(w.space().unwrap().pilots.bots[slot].target, PilotTarget::None);
    // In the open: chosen, followed, fired on.
    fly(&mut w, 0, [60.0, 0.0, -120.0]);
    assert!(step(&mut w, 120));
    // The craft is gone: the trigger is let go at once.
    w.space_mut().unwrap().fighters[0].alive = false;
    step(&mut w, 2);
    assert!(!w.space().unwrap().buildings[0].turret.as_ref().unwrap().pending_remote_fire);
}

#[test]
fn a_self_piloted_gun_whose_target_is_gone_finishes_its_shot() {
    // An invented ship gun whose shot is five salvos 0.2 s apart. Its target
    // is out of reach after the first; the other four still go out, the way
    // the gun points, and no new shot begins.
    let (mut w, _) = scene(vec![fighter_profile()], &[(0, 2, [0.0, 0.0, 400.0])], &[], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 1.0, 57300.0, [0.0; 3], [None; 3], false);
        let mut ship_gun = gun(0, [0.0; 3]);
        let weapon = &mut ship_gun.turret.as_mut().unwrap().weapon;
        (weapon.salvo_count, weapon.salvo_delay, weapon.salvo_delay_max, weapon.shot_delay) = (5, 0.2, 0.2, 0.37);
        vec![ship_gun]
    });
    fly(&mut w, 0, [0.0, 0.0, 400.0]);
    let fired = |w: &World| w.space().unwrap().buildings[0].turret.as_ref().unwrap().shots_fired;
    let step = |w: &mut World, ticks: u32| (0..ticks).for_each(|_| assert!(w.space_step_tick(SPACE_FIXED_DT)));
    step(&mut w, 2);
    assert_eq!(fired(&w), 1);
    fly(&mut w, 0, [0.0, 0.0, 7300.0]);
    step(&mut w, 120);
    assert_eq!(fired(&w), 5);
    let rounds = &w.space().unwrap().ordnance.slots;
    assert!(rounds.iter().filter(|r| r.generation > 0).all(|r| r.velocity[2] > 1100.0), "all northward");
}

#[test]
fn a_bots_skill_scales_the_kick_its_chair_gun_gathers() {
    use crate::space::flight::parse_space_weapon;
    use crate::space::SpaceEvent;
    // An invented chair gun with no spread of its own whose salvos kick 1
    // degree each and never fall back: unscaled, its k-th round strays at most
    // k - 1 degrees on an axis. The battle's skill (here the default, 14)
    // hands the gun a spread scale of 1.6, so some rounds stray further.
    let gun = SpaceWeaponSpec {
        shot_delay: 0.21,
        velocity: 640.0,
        life_span: 0.95,
        max_damage: 0.0,
        max_range: 470.0,
        ..parse_space_weapon(&parse(r#"{"class":"test_kick_chair","properties":[["SpreadPerShot","1.0"],["SpreadLimit","40"]]}"#).unwrap(), 0)
    };
    let (mut w, _) = chair_scene(gun);
    fly(&mut w, 0, [60.0, 0.0, -120.0]);
    let deg = std::f64::consts::PI / 180.0;
    let (mut rounds, mut beyond) = (0u32, 0u32);
    for _ in 0..240 {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        after_ai_in(&mut w, SPACE_FIXED_DT);
        let space = w.space().unwrap();
        let object = &space.buildings[0];
        let bore = barrel(object.turret.as_ref().unwrap(), &object.matrix);
        let up_hint = [object.matrix[4], object.matrix[5], object.matrix[6]];
        let right = super::vec::unit(super::vec::cross(up_hint, bore));
        let up = super::vec::cross(bore, right);
        for e in space.events() {
            if let SpaceEvent::WeaponFired { building: Some(0), direction, .. } = e {
                let pitch = v8math::asin(dot(*direction, up)).abs();
                let yaw = v8math::atan2(dot(*direction, right), dot(*direction, bore)).abs();
                if v8math::max(pitch, yaw) > f64::from(rounds) * deg + 1e-9 {
                    beyond += 1;
                }
                rounds += 1;
            }
        }
    }
    assert!(rounds > 10, "the bot fires on the craft: {rounds}");
    assert!(beyond > 0, "no round strayed past the unscaled kick in {rounds}");
}

#[test]
fn the_battles_skill_reaches_aim_through_the_chosen_weapon() {
    use crate::ai::combat::aim::Aimer;
    use crate::ai::control::difficulty::skill_curves;
    use crate::space::flight::SpaceOrdnanceKind;
    let curves = |skill: f64| {
        let c = skill_curves(skill);
        crate::ai::combat::world::Curves { a: c.a, d: c.d, b: Some(c.b), s: Some(c.s) }
    };
    let pilots = PilotBots::build(&SpaceBounds::from_world_json(&parse("{}").unwrap()), &[], &[], &[], &parse("{}").unwrap(), 1, 0);
    let cannon = SpaceWeaponSpec::default();
    let rocket = SpaceWeaponSpec { ordnance_kind: SpaceOrdnanceKind::GuidedRocket, ..Default::default() };
    let x = Vector3::new(1.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    // A remote weapon (the guided rocket) halves the aim's response, as the
    // released aim code does for anything but a cannon.
    let after = |w: &SpaceWeaponSpec| {
        let mut aim = Aimer::default();
        aim.reset(&x);
        let mut d = x;
        for _ in 0..20 {
            d = aim.update(&z, SPACE_FIXED_DT, Some(&curves(14.0)), Some(pilots.def_of(w)), false).0;
        }
        d.z
    };
    assert!(after(&rocket) < after(&cannon), "{} {}", after(&rocket), after(&cannon));
}

#[test]
fn a_bot_aims_the_same_whatever_spread_its_weapon_authors() {
    use crate::space::flight::parse_space_weapon;
    // The skill's spread scale reaches only a weapon's kick, never its own
    // spread: two chair guns that differ only in an invented MaxSpread are
    // aimed alike, tick for tick.
    let gun = |json: &str| SpaceWeaponSpec {
        shot_delay: 0.23,
        velocity: 640.0,
        life_span: 0.95,
        max_damage: 0.0,
        max_range: 470.0,
        ..parse_space_weapon(&parse(json).unwrap(), 0)
    };
    let aims = |weapon: SpaceWeaponSpec| {
        let (mut w, _) = chair_scene(weapon);
        fly(&mut w, 0, [60.0, 0.0, -120.0]);
        let mut seen = Vec::new();
        for _ in 0..120 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            after_ai_in(&mut w, SPACE_FIXED_DT);
            let turret = w.space().unwrap().buildings[0].turret.as_ref().unwrap();
            seen.push((turret.pending_remote_aim, turret.pending_remote_fire));
        }
        seen
    };
    let plain = aims(gun(r#"{"class":"test_chair_gun","properties":[]}"#));
    let spread = aims(gun(r#"{"class":"test_chair_gun","properties":[["MaxSpread","0.63"]]}"#));
    assert!(plain.iter().any(|&(_, fire)| fire), "the bot follows the craft and fires on it");
    assert_eq!(plain, spread);
}

#[test]
fn a_chair_bot_fires_a_beam_within_its_guns_targeting_range_and_never_ahead_of_its_target() {
    use crate::space::flight::SpaceOrdnanceKind;
    // Each tick's aim and trigger of a bot whose gun (it looks for targets
    // within 470 m) fires `weapon`, at a craft held at `at` with a speed.
    let aims = |weapon: SpaceWeaponSpec, at: [f64; 3], velocity: [f64; 3]| {
        let (mut w, _) = chair_scene(weapon);
        let mut seen = Vec::new();
        for _ in 0..120 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            fly(&mut w, 0, at);
            w.space_mut().unwrap().fighters[0].velocity = velocity;
            after_ai_in(&mut w, SPACE_FIXED_DT);
            let turret = w.space().unwrap().buildings[0].turret.as_ref().unwrap();
            seen.push((turret.pending_remote_aim, turret.pending_remote_fire));
        }
        seen
    };
    let fires = |seen: &[(Option<[f64; 3]>, bool)]| seen.iter().any(|&(_, fire)| fire);
    let (near, far, crossing) = ([60.0, 0.0, -380.0], [60.0, 0.0, -560.0], [70.0, 0.0, 0.0]);
    // An invented beam that reaches 1130 m.
    let beam = || SpaceWeaponSpec { ordnance_kind: SpaceOrdnanceKind::Beam, shot_delay: 0.23, velocity: 1130.0, max_damage: 0.0, ..Default::default() };
    let still = aims(beam(), near, [0.0; 3]);
    assert!(fires(&still));
    assert!(!fires(&aims(beam(), far, [0.0; 3])), "past the range it looks for targets in");
    // It strikes at once, so a crossing craft is aimed at where it is; rounds
    // that take their time are aimed ahead of it.
    assert_eq!(aims(beam(), near, crossing), still);
    let rounds = || SpaceWeaponSpec { shot_delay: 0.23, velocity: 640.0, life_span: 0.95, max_damage: 0.0, ..Default::default() };
    assert_ne!(aims(rounds(), near, crossing), aims(rounds(), near, [0.0; 3]));
}

#[test]
fn an_empty_chairs_gun_runs_on_as_one_whose_gunner_lets_go() {
    use crate::space::flight::parse_space_weapon;
    // An invented chair gun that warms up, heats, kicks and cools down. Its
    // gunner leaves in the middle of a shot delay; another lets the trigger go
    // there and stays. The two guns go through the same states tick by tick.
    let json = r#"{"class":"test_chair_gun","properties":[["ShotDelay","0.43"],["WarmUpTime","0.35"],["HeatPerShot","0.37"],["HeatRecoverRate","0.19"],["HeatThreshold","0.11"],["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["CoolDownTime","1.3"]]}"#;
    let gun = || SpaceWeaponSpec { velocity: 640.0, life_span: 0.95, max_damage: 0.0, max_range: 470.0, ..parse_space_weapon(&parse(json).unwrap(), 0) };
    let fired = || {
        let (mut w, slot) = chair_scene(gun());
        let seat = w.space().unwrap().pilots.bots[slot].terminal.unwrap();
        for _ in 0..52 {
            assert!(w.space_control_terminal(seat, [0.0, 0.0, -1.0], true));
            assert!(w.space_step_tick(SPACE_FIXED_DT));
        }
        (w, seat)
    };
    let ((mut kept, seat), (mut left, leaver)) = (fired(), fired());
    assert!(left.space_leave_terminal(leaver));
    let state = |w: &World| {
        let t = w.space().unwrap().buildings[0].turret.as_ref().unwrap();
        (t.clock.cooldown, t.heat, t.overheated, t.warm_up, t.kick)
    };
    let (shot_delay_left, .., kick) = state(&left);
    assert!(shot_delay_left > 0.2 && kick.value > 0.0);
    for tick in 0..300 {
        assert!(kept.space_control_terminal(seat, [0.0, 0.0, -1.0], false));
        assert!(kept.space_step_tick(SPACE_FIXED_DT) && left.space_step_tick(SPACE_FIXED_DT));
        assert_eq!(state(&left), state(&kept), "tick {tick}");
    }
    assert_eq!(state(&left), (0.0, 0.0, false, 0.0, Default::default()));
}

/// An invented self-piloted gun at `at` whose rounds reach about 838 m.
fn gun(index: usize, at: [f64; 3]) -> crate::space::objects::SpaceBuildingAttachment {
    use crate::space::combat::SpaceDamageCategory;
    use crate::space::objects::{SpaceBuildingAttachment, SpaceTurretPilotType, SpaceTurretState, TurretTargetFilter};
    let mut m = three::Matrix4::default().elements;
    m[12..15].copy_from_slice(&at);
    SpaceBuildingAttachment {
        building_index: index,
        instance: index,
        name: format!("gun{index}"),
        class: "gun".into(),
        base: "armedbuilding".into(),
        bodies: [None; 3],
        animated_index: None,
        matrix: m,
        position: at,
        center: at,
        local_center_offset: [0.0; 3],
        radius: 4.2,
        sphere: None,
        visible: true,
        lock_on_enabled: false,
        ai_damage_threshold: 0.0,
        damage_category: SpaceDamageCategory::Building,
        death_explosion: None,
        last_life: None,
        turret: Some(SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon: SpaceWeaponSpec { max_range: 935.0, velocity: 1180.0, life_span: 0.71, ..Default::default() },
            filter: TurretTargetFilter { target_person: false, target_vehicle: true, target_building: false, min_range: 0.0, max_range: 935.0 },
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -3.2,
            yaw_max: 3.2,
            pitch_min: -1.45,
            pitch_max: 1.45,
            turn_rate: 1.15,
            pitch_rate: 1.13,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        }),
        terminal: None,
    }
}

/// A home exit of team 1 far to the south.
fn home_far_south() -> Routes {
    Routes { exits: vec![super::routes::Exit { team: 1, center: [0.0, 300.0, -3000.0], outward: [0.0, 0.0, 1.0], across: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], half: [40.0, 15.0], ..Default::default() }], nodes: vec![] }
}

/// Our home opening far south, its patrol point out of the gun's reach.
fn home_far_south_with_outside() -> Routes {
    let mut r = home_far_south();
    r.exits[0].outside = [0.0, 300.0, -2900.0];
    r
}

#[test]
fn a_damaged_pilot_inside_enemy_guns_reach_flies_clear_before_heading_home() {
    // Our craft under half its hull, 300 m north of an enemy gun and well
    // inside its reach; its pilot's class repairs the craft in flight.
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 300.0])], &[(1, [4.0, 0.0, 300.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 2340.0, [0.0, 300.0, 0.0], [None; 3], false);
        vec![gun(0, [0.0, 300.0, 0.0])]
    });
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    space.pilots.routes = home_far_south_with_outside();
    let f = &mut space.fighters[0];
    f.max_health = 2150.0;
    f.health = 960.0;
    f.pilot.as_mut().unwrap().repair_capable = true;
    run(&mut w, 31);
    let slot = bots[0].reference().slot as usize;
    let p = &w.space().unwrap().pilots;
    let b = p.bots[slot];
    assert_eq!(b.phase, PilotPhase::Fight, "no turn for home under the gun");
    assert!(b.disengage_until > p.tick, "it flies clear first");
    assert!(b.disengage_dir[2] > 0.9, "away from the gun, not home past it: {:?}", b.disengage_dir);
    assert_eq!((p.census.disengages, p.census.returns), (1, 0));
    // Out of the gun's reach it mends the craft on its way to its patrol
    // point rather than flying home.
    w.space_mut().unwrap().fighters[0].position = [0.0, 300.0, 1200.0];
    run(&mut w, 31);
    let p = &w.space().unwrap().pilots;
    assert_eq!(p.bots[slot].phase, PilotPhase::Fight);
    assert!(p.bots[slot].mending);
    assert_eq!((p.census.mends, p.census.returns), (1, 0));
    // Still badly hurt when it reaches that point, it goes in to land.
    w.space_mut().unwrap().fighters[0].position = [0.0, 300.0, -2350.0];
    run(&mut w, 31);
    let p = &w.space().unwrap().pilots;
    assert_eq!(p.bots[slot].phase, PilotPhase::Return);
    assert_eq!(p.census.returns, 1);
}

#[test]
fn a_gun_that_fires_only_on_ship_objects_is_no_danger_to_a_pilot() {
    // The same hurt craft near an enemy gun, whose weapon may be used on
    // ship objects and not on craft: its reach threatens nothing, so the pilot
    // neither flies clear nor counts the gun against the object beside it.
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 300.0])], &[(1, [4.0, 0.0, 300.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 2340.0, [0.0, 300.0, 0.0], [None; 3], false);
        crate::space::tests::add_synthetic_building(w, 1, 2.0, 57300.0, [0.0, 300.0, -80.0], [None; 3], false);
        let mut ship_gun = gun(0, [0.0, 300.0, 0.0]);
        let filter = &mut ship_gun.turret.as_mut().unwrap().filter;
        (filter.target_vehicle, filter.target_building) = (false, true);
        vec![ship_gun, lockable(1, [0.0, 300.0, -80.0], 30.0, None, 0.0)]
    });
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    space.pilots.routes = home_far_south_with_outside();
    let f = &mut space.fighters[0];
    f.max_health = 2150.0;
    f.health = 1410.0;
    run(&mut w, 31);
    let slot = bots[0].reference().slot as usize;
    let p = &w.space().unwrap().pilots;
    assert_eq!((p.census.disengages, p.census.mends), (0, 0));
    assert!(p.bots[slot].disengage_until == 0 && !p.bots[slot].mending);
    let object = p.systems.iter().find(|s| s.index == 1).expect("the object is known");
    assert_eq!(object.exposure, 0.0);
    assert!(p.systems.iter().any(|s| s.index == 0 && s.gun_reach == 0.0));
}

#[test]
fn a_mending_pilot_flies_away_from_the_enemys_ships() {
    // A hurt craft of a repairing class 600 m north of an enemy object, its
    // home and its patrol point far to the south, past the object. It flies
    // north, away, at full throttle.
    let at = [0.0, 300.0, -600.0];
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 57300.0, at, [None; 3], false);
        vec![lockable(0, at, 30.0, None, 0.0)]
    });
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    space.pilots.routes = home_far_south_with_outside();
    let f = &mut space.fighters[0];
    f.max_health = 2150.0;
    f.health = 960.0;
    f.pilot.as_mut().unwrap().repair_capable = true;
    for _ in 0..240 {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    let space = w.space().unwrap();
    let f = &space.fighters[0];
    assert!(space.pilots.bots[bots[0].reference().slot as usize].mending);
    assert!(f.velocity[2] > 50.0 && f.position[2] > 150.0, "{:?} {:?}", f.position, f.velocity);
    assert!(f.pending_input.throttle > 0.99);
}

#[test]
fn a_crew_that_cannot_repair_its_craft_fights_on_instead_of_flying_home() {
    // The same hurt craft clear of every gun, but its crew cannot repair it:
    // the deck mends nothing, so it stays in the fight.
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 1200.0])], &[(1, [4.0, 0.0, 1200.0])], &[], |_, _| Vec::new());
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    space.pilots.routes = home_far_south_with_outside();
    space.fighters[0].health = 640.0;
    assert!(!space.fighters[0].pilot.unwrap().repair_capable);
    run(&mut w, 61);
    let p = &w.space().unwrap().pilots;
    let slot = bots[0].reference().slot as usize;
    assert_eq!(p.bots[slot].phase, PilotPhase::Fight);
    assert!(!p.bots[slot].mending);
    assert_eq!((p.census.mends, p.census.returns), (0, 0));
}

/// An invented bomber: a gun for craft and a falling bomb for ship objects
/// only (every figure made up for the tests).
fn bomber_profile() -> SpaceFlightProfile {
    let gun = SpaceWeaponSpec { channel: 0, ai_targets_vehicle: true, ai_targets_building: false, max_range: 640.0, ..Default::default() };
    let bomb = SpaceWeaponSpec {
        channel: 1,
        ordnance_kind: crate::space::flight::SpaceOrdnanceKind::BombShell,
        ai_targets_person: false,
        ai_targets_vehicle: false,
        ai_targets_building: true,
        ai_min_range: 23.0,
        ai_optimal_range: 137.0,
        max_range: 285.0,
        velocity: 215.0,
        life_span: 9.3,
        gravity: 6.1,
        max_damage: 2470.0,
        shot_delay: 0.13,
        ..Default::default()
    };
    SpaceFlightProfile { weapons: vec![gun, bomb], min_speed: 33.5, mid_speed: 57.0, max_speed: 79.0, turn_rate: 0.82, pitch_rate: 1.07, ..fighter_profile() }
}

/// An enemy ship object the mission makes lockable, with one collision body.
fn lockable(index: usize, at: [f64; 3], radius: f64, body: Option<u32>, floor: f64) -> crate::space::objects::SpaceBuildingAttachment {
    let mut a = gun(index, at);
    a.name = format!("system{index}");
    a.base = "destructablebuilding".into();
    a.turret = None;
    a.bodies = [body, None, None];
    a.radius = radius;
    a.lock_on_enabled = true;
    a.ai_damage_threshold = floor;
    a
}

#[test]
fn a_bomber_dives_from_outside_a_shielded_object_and_releases_its_bombs_where_they_meet_it() {
    use crate::battle::rules::objects::ObjectProperty as P;
    use crate::space::{SpaceImpactCause, SpaceImpactTarget};
    // A 200 m wide enemy hull section 650 m north, shielded: its bounding
    // sphere reaches 116 m from its centre, past the bombs' 23 to 285 m.
    let (lo, hi) = ([-100.0, 270.0, 650.0], [100.0, 330.0, 750.0]);
    let (mut w, bots) = scene(vec![bomber_profile()], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[], |w, coll| {
        let (core, body) = add_synthetic_box(w, coll, lo, hi, true);
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 57300.0, [0.0, 300.0, 700.0], [Some(core), None, None], false);
        vec![lockable(0, [0.0, 300.0, 700.0], 116.0, Some(body), 0.0)]
    });
    for prop in [P::MaxShield(187300.0), P::CurrentShield(187300.0), P::AddShield(0.0)] {
        assert!(w.space_set_building_property(0, prop).is_ok());
    }
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let slot = bots[0].reference().slot as usize;
    let (mut on_target, mut elsewhere, mut farthest_release) = (0u32, 0u32, 0.0f64);
    for _ in 0..(40 * 60) {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        after_ai_in(&mut w, SPACE_FIXED_DT);
        let space = w.space().unwrap();
        let f = &space.fighters[0];
        assert!(f.alive, "the bomber never flies into the object");
        let b = space.pilots.bots[slot];
        if f.pending_input.fire_secondary {
            farthest_release = v8math::max(farthest_release, dist(f.position, b.run_aim));
        }
        for i in space.impacts.iter().filter(|i| i.cause == SpaceImpactCause::Round { weapon: 1 }) {
            if matches!(i.target, SpaceImpactTarget::Building { index: 0 }) {
                on_target += 1;
            } else {
                elsewhere += 1;
            }
        }
    }
    let space = w.space().unwrap();
    let b = space.pilots.bots[slot];
    // The run starts outside the sphere and aims at the face its line meets.
    assert!(dist(b.run_from, [0.0, 300.0, 700.0]) > 116.0 + 285.0, "{:?}", b.run_from);
    assert!(b.run_surface && (b.run_aim[2] - 650.0).abs() < 1.0, "{:?}", b.run_aim);
    assert!(space.pilots.census.object_dives >= 1);
    // Released within the bombs' reach, and every bomb meets the object.
    assert!(farthest_release > 0.0 && farthest_release <= 285.0 + PROVISIONAL_AIM_RADIUS, "{farthest_release}");
    assert!(on_target >= 10, "{on_target}");
    assert_eq!(elsewhere, 0);
    let shield = crate::units::f3_props::mission_shields(&w.units, 0).current;
    assert!(shield <= 187300.0 - 2470.0 * f64::from(on_target) + 1.0, "{shield}");
}

#[test]
fn a_dive_steers_round_a_body_that_is_not_its_target() {
    // A bomber diving on an enemy object 500 m to the north-east flies north
    // toward a wall 150 m ahead, inside its look-ahead.
    let (at, aim) = ([380.0, 300.0, 340.0], [350.0, 300.0, 330.0]);
    let wall = ([-90.0, 250.0, 150.0], [90.0, 350.0, 170.0]);
    let (mut w, bots) = scene(vec![bomber_profile()], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[wall], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 57300.0, at, [None; 3], false);
        vec![lockable(0, at, 30.0, None, 0.0)]
    });
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let target = PilotTarget::Building { index: 0, life: crate::units::f3_props::owner_in(&w, 0).life };
    let slot = bots[0].reference().slot as usize;
    let bot = &mut w.space_mut().unwrap().pilots.bots[slot];
    (bot.target, bot.run_for) = (target, target);
    bot.step = 1;
    bot.run_surface = true;
    bot.run_local_aim = sub(aim, at);
    bot.run_local_from = sub([20.0, 300.0, 100.0], at);
    run(&mut w, 4);
    let p = &w.space().unwrap().pilots;
    let b = p.bots[slot];
    assert!(p.census.hull_avoidances >= 1 && b.avoid_hull, "it turns from the wall");
    assert_eq!((b.run, b.step, b.target), (Run::Approach, 1, target), "and stays in its dive");
    assert_eq!(p.census.object_breaks, 0);
}

#[test]
fn a_run_on_a_moving_object_moves_with_it() {
    // A frame turned a quarter round and moved, then moved on 19 m.
    let mut m = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 100.0, 20.0, -50.0, 1.0];
    let aim = [130.0, 25.0, -40.0];
    let local = to_local(&m, aim);
    assert!(dist(to_world(&m, local), aim) < 1e-9);
    m[12] += 19.0;
    assert!(dist(to_world(&m, local), [149.0, 25.0, -40.0]) < 1e-9, "the aim point goes where the object goes");
}

#[test]
fn objects_bots_cannot_destroy_are_never_attacked() {
    // A near enemy object the mission floors for bots, a far one it does not.
    let (mut w, _) = scene(vec![fighter_profile()], &[], &[], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 81500.0, [0.0, 0.0, 300.0], [None; 3], false);
        crate::space::tests::add_synthetic_building(w, 1, 2.0, 57300.0, [0.0, 0.0, 900.0], [None; 3], false);
        vec![lockable(0, [0.0, 0.0, 300.0], 50.0, None, 0.27), lockable(1, [0.0, 0.0, 900.0], 50.0, None, 0.0)]
    });
    run(&mut w, 2);
    let systems: Vec<usize> = w.space().unwrap().pilots.systems.iter().map(|s| s.index).collect();
    assert_eq!(systems, vec![1]);
}

#[test]
fn a_gunner_with_no_craft_in_reach_fires_on_an_enemy_object_it_can_see() {
    // Our transport aloft with a gunner whose gun may be used on objects; an
    // enemy object 400 m north, no enemy craft anywhere.
    let mut profile = gunship();
    profile.weapons[1].ai_targets_building = true;
    let (mut w, bots) = scene(vec![profile], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[], |w, _| {
        crate::space::tests::add_synthetic_building(w, 0, 2.0, 57300.0, [0.0, 0.0, 400.0], [None; 3], false);
        vec![lockable(0, [0.0, 0.0, 400.0], 30.0, None, 0.0)]
    });
    let seat = w.space_board_seat(&bots[0], 0, 1).expect("in reach of the gunner seat");
    let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
    let slot = bots[0].reference().slot as usize;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Gunner, receipt: Some(seat), craft: 0, seat: 1, ..Bot::fresh(life, 0) };
    fly(&mut w, 0, [0.0, 0.0, 0.0]);
    let mut shot = false;
    for _ in 0..240 {
        after_ai_in(&mut w, SPACE_FIXED_DT);
        shot |= w.space().unwrap().fighters[0].gunner_inputs[0].fire_primary;
    }
    assert!(matches!(w.space().unwrap().pilots.bots[slot].target, PilotTarget::Building { index: 0, .. }));
    assert!(shot);
}

#[test]
fn a_traffic_turn_does_not_overrule_a_hull_ahead() {
    // North at 77 m/s toward a wall 140 m ahead while an enemy closes head on.
    let wall = ([-300.0, 200.0, 140.0], [300.0, 400.0, 160.0]);
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 0.0]), (0, 2, [500.0, 0.0, 500.0])], &[(1, [4.0, 0.0, 0.0])], &[wall], |_, _| Vec::new());
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    fly(&mut w, 1, [8.0, 301.15, 110.0]);
    let space = w.space_mut().unwrap();
    space.fighters[0].velocity = [0.0, 0.0, 77.0];
    space.fighters[0].airspeed = 77.0;
    space.fighters[1].velocity = [0.0, 0.0, -77.0];
    space.fighters[1].airspeed = 77.0;
    run(&mut w, 3);
    let slot = bots[0].reference().slot as usize;
    let b = w.space().unwrap().pilots.bots[slot];
    assert!(b.avoid_hull, "the hull ahead keeps the turn");
    assert!(b.avoid_dir[2] < 0.5, "the turn leans away from the wall: {:?}", b.avoid_dir);
}

#[test]
fn a_fast_slow_turning_craft_sees_a_wall_as_far_off_as_it_needs_to_turn() {
    // 103 m/s with 0.47 radians a second to turn: a wall 400 m ahead is
    // inside its turning distance (beyond a fixed 2.6 s look-ahead).
    let slow_turner = SpaceFlightProfile { turn_rate: 0.47, pitch_rate: 0.47, ..fighter_profile() };
    let wall = ([-300.0, 200.0, 400.0], [300.0, 400.0, 420.0]);
    let (mut w, bots) = scene(vec![slow_turner], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[wall], |_, _| Vec::new());
    aloft(&mut w, &bots[0], 0, PilotPhase::Fight);
    let space = w.space_mut().unwrap();
    space.fighters[0].velocity = [0.0, 0.0, 103.0];
    space.fighters[0].airspeed = 103.0;
    run(&mut w, 2);
    assert!(w.space().unwrap().pilots.census.hull_avoidances >= 1);
}

#[test]
fn a_craft_pinned_on_its_way_in_backs_off_and_lines_up_again() {
    let (mut w, bots) = scene(vec![fighter_profile()], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0])], &[], |_, _| Vec::new());
    aloft(&mut w, &bots[0], 0, PilotPhase::Return);
    w.space_mut().unwrap().pilots.routes = home_far_south();
    let slot = bots[0].reference().slot as usize;
    // The hull grinds against a surface every tick for a second.
    for _ in 0..60 {
        let f = &mut w.space_mut().unwrap().fighters[0];
        f.contacts += 1;
        f.contact_normal = [0.0, 0.0, -1.0];
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    let p = &w.space().unwrap().pilots;
    assert!(p.census.pinned >= 1);
    assert!(p.bots[slot].scrapes >= 2, "one long grind counts more than once");
    assert!(p.bots[slot].step <= 1, "lining up again");
}

#[test]
fn a_transport_waits_on_its_deck_for_crew() {
    let transport = SpaceFlightProfile {
        is_command_flyer: true,
        spawn_point_count: 3,
        seat_count: 3,
        weapons: vec![SpaceWeaponSpec { seat: 0, ai_targets_vehicle: true, ..Default::default() }],
        ..fighter_profile()
    };
    // The pilot beside it, a passenger far off on the deck.
    let (mut w, bots) = scene(vec![transport], &[(0, 1, [0.0, 0.0, 0.0])], &[(1, [4.0, 0.0, 0.0]), (1, [900.0, 0.0, 0.0])], &[], |_, _| Vec::new());
    let seat = w.space_board_seat(&bots[0], 0, 0).expect("the pilot is in reach");
    let life = w.sim.store.unit.get(bots[0].reference()).unwrap().generation;
    let slot = bots[0].reference().slot as usize;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Hangar, receipt: Some(seat), craft: 0, ..Bot::fresh(life, 0) };
    run(&mut w, 120);
    assert_eq!(w.space().unwrap().pilots.census.takeoff_presses, 0, "no crew yet");
    // The passenger comes over and takes a seat without a weapon.
    if let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(bots[1].reference()).unwrap().soldier {
        s.position = [-4.0, 0.0, 0.0];
    }
    assert!(w.space_board_seat(&bots[1], 0, 2).is_some());
    run(&mut w, 2);
    let census = w.space().unwrap().pilots.census;
    assert_eq!((census.takeoff_presses, census.crewed_departures), (1, 1));
}

#[test]
fn a_waiting_craft_lures_bots_from_the_missions_notify_radius() {
    // A craft 80 m from a bot: out of reach at the released default radius,
    // in reach once the mission host passes a radius of 125 m.
    let (mut w, bots) = deck(&[[80.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]], &[]);
    assert_eq!(w.space_pilot_notify_radius(), Some(RELEASED_NOTIFY_RADIUS));
    run(&mut w, 31);
    assert_eq!(w.space_pilot_view(bots[0].reference()).map(|v| v.phase), Some(PilotPhase::Free));
    for refused in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        assert!(!w.space_set_pilot_notify_radius(refused), "{refused}");
    }
    assert!(w.space_set_pilot_notify_radius(125.0));
    assert_eq!(w.space_pilot_notify_radius(), Some(125.0));
    run(&mut w, 31);
    let view = w.space_pilot_view(bots[0].reference()).expect("steered by the pilots");
    assert_eq!((view.phase, view.craft), (PilotPhase::Walk, Some(0)));
}

/// Makes unit `key` a human on foot moving at `velocity`.
fn human(w: &mut World, key: &F3UnitKey, at: [f64; 3], velocity: [f64; 3]) {
    let u = w.sim.store.unit.get_mut(key.reference()).unwrap();
    u.human = true;
    u.controller = None;
    if let UnitBody::Soldier(s) = &mut u.soldier {
        s.position = at;
        s.velocity = velocity;
    }
}

#[test]
fn a_bot_leaves_a_human_the_craft_they_are_walking_to() {
    // One craft; a bot 13 m from it and a human 14 m from it walking to it.
    let (mut w, keys) = deck(&[[12.0, 0.0, 0.0]], &[[0.0, 0.0, -6.0], [0.0, 0.0, 8.0]], &[]);
    let toward = [12.0 * 3.0 / 14.42, 0.0, -8.0 * 3.0 / 14.42];
    human(&mut w, &keys[1], [0.0, 0.0, 8.0], toward);
    run(&mut w, 31);
    let bot = |w: &World| w.space_pilot_view(keys[0].reference()).map(|v| (v.phase, v.craft));
    assert_eq!(bot(&w), Some((PilotPhase::Free, None)), "the human's craft is not claimed");
    assert!(w.space_pilot_view(keys[1].reference()).is_none(), "a human is never steered");
    // The human stops far off: the craft is the bots' again.
    human(&mut w, &keys[1], [0.0, 0.0, 40.0], [0.0; 3]);
    run(&mut w, 31);
    assert_eq!(bot(&w), Some((PilotPhase::Walk, Some(0))));
    // A human heading for it and nearer to it than the walking bot gets it.
    human(&mut w, &keys[1], [6.0, 0.0, 0.0], [3.0, 0.0, 0.0]);
    run(&mut w, 2);
    assert_eq!(bot(&w).map(|b| b.0), Some(PilotPhase::Free));
    assert_eq!(w.space().unwrap().pilots.census.yields_to_humans, 1);
}

fn admit_test_swbf2_nav(w: &mut World, coll: usize, hubs: &[([f64; 3], f64)], arcs: &[(usize, usize)], doors: &[usize]) {
    admit_test_swbf2_nav_with_barriers(w, coll, hubs, arcs, doors, "");
}

fn admit_test_swbf2_nav_with_barriers(
    w: &mut World,
    coll: usize,
    hubs: &[([f64; 3], f64)],
    arcs: &[(usize, usize)],
    doors: &[usize],
    barriers_json: &str,
) {
    use bfe_content::install::CollisionKernel;
    use crate::gen::{AiCapabilities, AiCapacity, AiColdSettings, AiNavPools, AiNavSetup, AiWorldSetup};
    if w.collision.state.worlds[coll].statics.is_none() {
        let tree = w.collision.state.worlds[coll].tree as u32;
        let set = CollisionKernel::fixed_set(&mut w.collision, tree, &[], &[], 0.0);
        w.collision.world_statics(coll as u32, set);
    }
    let mut hub_slots: Vec<Vec<usize>> = vec![Vec::new(); hubs.len()];
    let mut arcs_json = Vec::with_capacity(arcs.len());
    for (idx, &(a, b)) in arcs.iter().enumerate() {
        hub_slots[a].push(idx);
        hub_slots[b].push(idx);
        arcs_json.push(format!(
            r#"{{"name":"arc_{idx}","start":{a},"end":{b},"filterFlags":31,"typeFlags":0}}"#
        ));
    }
    let mut hubs_json = Vec::with_capacity(hubs.len());
    for (idx, (&(pos, radius), slots)) in hubs.iter().zip(&hub_slots).enumerate() {
        let mut s = [255usize; 8];
        for (k, &a) in slots.iter().take(8).enumerate() {
            s[k] = a;
        }
        hubs_json.push(format!(
            r#"{{"name":"hub_{idx}","position":[{},{},{}],"radius":{radius},"slots":[{},{},{},{},{},{},{},{}],"weightCounts":[0,0,0,0,0],"weights":[]}}"#,
            pos[0], pos[1], pos[2], s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        ));
    }
    let json = format!(
        r#"{{"schema":2,"map":"invented_hangar","plan":{{"branchCount":5,"hubs":[{}],"arcs":[{}]}},"barriers":[{}],"hints":[]}}"#,
        hubs_json.join(","),
        arcs_json.join(","),
        barriers_json,
    )
    .into_bytes();
    let setup = AiWorldSetup {
        settings: AiColdSettings {
            seed: 7,
            capacity: AiCapacity {
                registry: 96,
                characters: 64,
                posts: 4,
                hulls: 0,
                buildings: 4,
                controls: 8,
                scopes: 4,
                cameras: 1,
                support_keys: 4,
                repair_jobs: 4,
                grenades: 4,
            },
            capabilities: AiCapabilities {
                navigation: true,
                population: true,
                classes: true,
                random_b: true,
                ..Default::default()
            },
            ..Default::default()
        },
        navigation: AiNavSetup {
            json,
            pools: AiNavPools {
                paths: 256.0,
                path_nodes: 2048.0,
                requests: 32.0,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    w.sim
        .calls
        .result(AiWorldSetup::WORDS, |row, heap| setup.put(row, heap));
    assert_eq!(
        crate::ai::owned::entry::ai_world_begin_swbf2(w, None, doors, &[]).unwrap(),
        1
    );
}

fn step_pilot_mover(w: &mut World, unit_ref: Ref<UnitRow>, mover_id: u32, dt: f64) {
    use crate::movement::soldier::{Input, V3};
    let input = {
        let row = w.sim.store.unit.get(unit_ref).unwrap();
        let i = &row.intent;
        Input {
            forward: i.forward,
            right: i.right,
            turn: i.turn,
            pitch: i.pitch,
            jump: i.jump,
            crouch: i.crouch,
            prone: i.prone,
            jet_direction: i.jet_direction.map(|v| V3::new(v[0], v[1], v[2])),
        }
    };
    w.movement
        .advance(&mut w.collision.state, mover_id, &input, dt);
    let m = w.movement.mover(mover_id);
    let (pos, settled, vel, yaw, pitch, grounded) = (
        [m.position.x, m.position.y, m.position.z],
        [m.settled.x, m.settled.y, m.settled.z],
        [m.velocity.x, m.velocity.y, m.velocity.z],
        m.aim_yaw,
        m.aim_pitch,
        m.grounded,
    );
    let row = w.sim.store.unit.get_mut(unit_ref).unwrap();
    if let UnitBody::Soldier(s) = &mut row.soldier {
        s.position = pos;
        s.settled = settled;
        s.velocity = vel;
        s.aim_yaw = yaw;
        s.aim_pitch = pitch;
        s.grounded = grounded;
    }
}

#[test]
fn a_blocked_bot_routes_round_a_wall_and_doorway_to_board_its_craft_with_zero_allocs() {
    use crate::ai::owned::tests::phase::{ALLOCATIONS, COUNTING};
    use crate::movement::soldier::{Mover, V3};
    use crate::world::owner::NavWalkStatus;

    // Hangar floor, wall blocking direct line from soldier (4,0,-4) to craft (22,1,-4),
    // and a closed proximity shutter across the corner doorway (x=12, z in 4..14).
    let (mut w, coll) = make_synthetic_f3_world();
    add_synthetic_box(&mut w, coll, [-6.0, -1.0, -12.0], [30.0, 0.0, 14.0], true);
    add_synthetic_box(&mut w, coll, [11.8, 0.0, -12.0], [12.2, 4.0, 4.0], true);
    let (_, door_body) = add_synthetic_box(&mut w, coll, [11.8, 0.0, 4.0], [12.2, 4.0, 14.0], true);
    let door_place = w.collision.state.worlds[coll]
        .order
        .iter()
        .position(|&b| b == door_body)
        .unwrap();

    let profile = fighter_profile();
    let p = SpaceVehiclePad {
        profile_index: 0,
        expire_time_enemy: 1e6,
        expire_time_field: 1e6,
        decay_time: 1e6,
        ..pad(1, [22.0, 1.0, -4.0])
    };
    let fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &p, 0, &profile, 0.0, None, None);
    let mut space = SpaceBattlefield::from_parts(
        SpaceBounds::from_world_json(&parse("{}").unwrap()),
        SpaceWorldAnims::default(),
        vec![profile],
        vec![p],
        vec![fighter],
        vec![],
        vec![],
        coll,
        w.collision.state.bodies.len(),
    );
    space.pilots = PilotBots::build(
        &space.bounds,
        &space.pads,
        &space.fighters,
        &space.profiles,
        &parse("{}").unwrap(),
        64,
        0,
    );
    w.space = Some(Box::new(space));

    let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
    soldier.position = [4.0, 0.0, -4.0];
    soldier.settled = soldier.position;
    soldier.grounded = true;
    soldier.max_speed = 6.5;
    soldier.max_strafe = 4.5;
    soldier.max_turn = 6.0;
    soldier.max_pitch = 4.0;
    soldier.accel = 70.0;
    let mut unit = UnitRow {
        alive: true,
        team: 1.0,
        nav_radius: Some(0.45),
        soldier: UnitBody::Soldier(soldier),
        controller: Some(Ref::new(0, 1)),
        ..Default::default()
    };
    unit.combat.health = 117.0;
    unit.combat.max_health = 117.0;
    let key = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit));
    let unit_ref = key.reference();
    prepare_synthetic_tail(&mut w);

    let mover = w.movement.new_mover(coll, |m| {
        *m = Mover::new(coll);
        m.max_speed = 6.5;
        m.max_strafe = 4.5;
        m.max_jump = 7.5;
        m.max_turn = 6.0;
        m.max_pitch = 4.0;
        m.accel = 70.0;
        m.position = V3::new(4.0, 0.0, -4.0);
        m.settled = m.position;
        m.grounded = true;
    });
    w.movement.reserve_mover(mover, 256, 16, 512);
    w.movement.prepare_world_gathers(&w.collision.state, coll);

    admit_test_swbf2_nav(
        &mut w,
        coll,
        &[
            ([4.0, 0.0, -4.0], 4.0),
            ([4.0, 0.0, 8.0], 4.0),
            ([20.0, 0.0, 8.0], 4.0),
            ([20.0, 0.0, -4.0], 4.0),
        ],
        &[(0, 1), (1, 2), (2, 3), (0, 3)],
        &[door_body as usize],
    );

    let mut max_z = -4.0f64;
    let mut door_wait_ticks = 0usize;
    let mut door_opened = false;
    let mut boarded = false;
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    for tick in 1..=1000u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
        if w.space_pilot_view(unit_ref).is_some_and(|v| v.phase == PilotPhase::Hangar) {
            boarded = true;
            break;
        }
        step_pilot_mover(&mut w, unit_ref, mover, SPACE_FIXED_DT);
        let p = w.sim.store.unit.get(unit_ref).unwrap().position();
        max_z = max_z.max(p.z);
        if !door_opened && p.x > 8.5 && p.z > 4.0 {
            door_wait_ticks += 1;
            if door_wait_ticks >= 15 {
                w.movement.flags_mut()[door_place] = 0;
                door_opened = true;
            }
        }
    }
    COUNTING.with(|on| on.set(false));
    let allocs = ALLOCATIONS.with(|n| n.get());

    assert!(door_opened, "bot should route round the wall to the closed corner doorway");
    assert!(boarded, "bot should cross the opened doorway and board the craft");
    assert!(max_z > 4.5, "bot must detour round the wall corner, max_z={max_z}");
    assert_eq!(w.space().unwrap().pilots.census.boardings, 1);
    assert!(w.space().unwrap().pilots.census.blocked_seats >= 1);
    assert_eq!(allocs, 0, "routed pilot walk and boarding must allocate 0 times");
    assert_eq!(
        w.nav_walk_step(unit_ref, SPACE_FIXED_DT),
        NavWalkStatus::Failed,
        "external walk must be released upon boarding"
    );
}

#[test]
fn a_routed_pilot_walk_respects_per_tick_cap_and_gives_up_on_taken_craft_death_or_hold() {
    use crate::world::owner::NavWalkStatus;

    let (mut w, coll) = make_synthetic_f3_world();
    add_synthetic_box(&mut w, coll, [-6.0, -1.0, -14.0], [30.0, 0.0, 14.0], true);
    add_synthetic_box(&mut w, coll, [11.8, 0.0, -14.0], [12.2, 4.0, 4.0], true);

    let profile = fighter_profile();
    let pads: Vec<SpaceVehiclePad> = [[22.0, 1.0, -4.0], [22.0, 1.0, -10.0], [22.0, 1.0, 2.0]]
        .into_iter()
        .map(|at| SpaceVehiclePad {
            profile_index: 0,
            expire_time_enemy: 1e6,
            expire_time_field: 1e6,
            decay_time: 1e6,
            ..pad(1, at)
        })
        .collect();
    let fighters: Vec<SpaceFighter> = pads
        .iter()
        .enumerate()
        .map(|(i, p)| SpaceFighter::spawned_on_pad(i, 1, i, p, 0, &profile, 0.0, None, None))
        .collect();
    let mut space = SpaceBattlefield::from_parts(
        SpaceBounds::from_world_json(&parse("{}").unwrap()),
        SpaceWorldAnims::default(),
        vec![profile],
        pads,
        fighters,
        vec![],
        vec![],
        coll,
        w.collision.state.bodies.len(),
    );
    space.pilots = PilotBots::build(
        &space.bounds,
        &space.pads,
        &space.fighters,
        &space.profiles,
        &parse("{}").unwrap(),
        96,
        0,
    );
    w.space = Some(Box::new(space));

    // Place 3 bots at slots 0, 30, 60 so all 3 share the same THINK_TICKS phase
    // and attempt routed walks on the exact same tick (tick 30).
    let mut live_keys = Vec::new();
    for slot in 0..=60usize {
        let active = slot == 0 || slot == 30 || slot == 60;
        let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
        soldier.position = [4.0 + (slot / 30) as f64, 0.0, -4.0];
        soldier.settled = soldier.position;
        soldier.grounded = true;
        let mut unit = UnitRow {
            alive: active,
            team: 1.0,
            nav_radius: Some(0.45),
            soldier: UnitBody::Soldier(soldier),
            controller: active.then_some(Ref::new(0, 1)),
            ..Default::default()
        };
        unit.combat.health = if active { 117.0 } else { 0.0 };
        unit.combat.max_health = 117.0;
        unit.combat.dead = !active;
        let key = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit));
        if active {
            live_keys.push(key);
        }
    }
    // Add an extra friendly soldier next to craft 0 to take its seat later.
    let mut taker = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
    taker.position = [22.0, 0.0, -4.0];
    let mut taker_unit = UnitRow {
        alive: true,
        team: 1.0,
        human: true,
        soldier: UnitBody::Soldier(taker),
        ..Default::default()
    };
    taker_unit.combat.health = 117.0;
    taker_unit.combat.max_health = 117.0;
    let taker_key = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, taker_unit));
    prepare_synthetic_tail(&mut w);

    admit_test_swbf2_nav(
        &mut w,
        coll,
        &[
            ([4.0, 0.0, -4.0], 4.0),
            ([4.0, 0.0, 8.0], 4.0),
            ([20.0, 0.0, 8.0], 4.0),
            ([20.0, 0.0, -4.0], 4.0),
        ],
        &[(0, 1), (1, 2), (2, 3)],
        &[],
    );

    // Advance to tick 30 (all 3 bots think on tick 30).
    for tick in 1..=30u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    let walking: Vec<usize> = live_keys
        .iter()
        .enumerate()
        .filter(|(_, k)| w.space_pilot_view(k.reference()).is_some_and(|v| v.phase == PilotPhase::Walk))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        walking.len(),
        usize::from(PROVISIONAL_ROUTED_STARTS_PER_TICK),
        "per-tick routed start cap must admit exactly 2 walks on tick 30"
    );
    let u0 = live_keys[0].reference();
    let u1 = live_keys[1].reference();
    let u2 = live_keys[2].reference();
    assert_eq!(w.space_pilot_view(u2).map(|v| v.phase), Some(PilotPhase::Free));

    // 1. Taken craft: human boards bot 0's claimed craft -> bot 0 gives up walk & releases seam.
    let craft0 = w.space_pilot_view(u0).unwrap().craft.unwrap();
    assert!(w.space_board_seat(&taker_key, craft0, 0).is_some());
    w.ai.begin_nav_tick(31);
    after_ai_in(&mut w, SPACE_FIXED_DT);
    assert_eq!(w.space_pilot_view(u0).map(|v| v.phase), Some(PilotPhase::Free));
    assert_eq!(w.nav_walk_step(u0, SPACE_FIXED_DT), NavWalkStatus::Failed);

    // 2. Death: kill bot 1 -> next tick releases seam.
    w.sim.store.unit.get_mut(u1).unwrap().alive = false;
    w.ai.begin_nav_tick(32);
    after_ai_in(&mut w, SPACE_FIXED_DT);
    assert_eq!(w.nav_walk_step(u1, SPACE_FIXED_DT), NavWalkStatus::Failed);

    // 3. On next think (tick 60), bot 2 starts its routed walk, then hold cancels it cleanly.
    for tick in 33..=60u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    assert_eq!(w.space_pilot_view(u2).map(|v| v.phase), Some(PilotPhase::Walk));
    assert!(w.space_pilot_hold(u2, true));
    assert_eq!(w.space_pilot_view(u2).map(|v| v.phase), Some(PilotPhase::Free));
    assert_eq!(w.nav_walk_step(u2, SPACE_FIXED_DT), NavWalkStatus::Failed);
}

#[test]
fn a_blocked_pilot_routes_down_a_bent_ramp_to_a_craft_beyond_node_reach_with_zero_allocs() {
    use crate::ai::owned::tests::phase::{ALLOCATIONS, COUNTING};
    use crate::movement::soldier::{Mover, V3};
    use crate::world::owner::NavWalkStatus;
    use bfe_content::{
        bvh::{build, Positions},
        install::CollisionKernel,
    };

    // Upper balcony (y=4, x in -6..4, z in -8..8) with parapet blocking the direct
    // line to the lower deck (y=0, x in 13.5..90, z in -20..20). A ramp descends
    // offset in +Z (x=3..14, z=0.8..10.0) so the single arc Hub 0 (0,4,0) <->
    // Hub 1 (80,0,0) certifies only via a bend around (10, 1.45, 5). A parked
    // hull beside Hub 1 (enabled after admission, like a spawned fighter) blocks
    // routes returning from Hub 1; the free craft at (40,1,-5) lies > 40 m from
    // both hubs (beyond node_reach = 27 m), beside the lower bent segment where
    // linear mid.y floats ~0.9 m above the flat deck.
    let (mut w, coll) = make_synthetic_f3_world();
    add_synthetic_box(&mut w, coll, [-6.0, 3.0, -8.0], [4.0, 4.0, 8.0], true);
    add_synthetic_box(&mut w, coll, [3.8, 4.0, -8.0], [4.2, 7.0, 0.6], true);
    add_synthetic_box(&mut w, coll, [13.5, -1.0, -20.0], [90.0, 0.0, 20.0], true);
    let (_, parked_blocker) =
        add_synthetic_box(&mut w, coll, [72.0, 0.0, -3.5], [76.0, 3.0, 3.5], false);
    let blocker_order = w.collision.state.worlds[coll]
        .order
        .iter()
        .position(|&b| b == parked_blocker)
        .unwrap();

    // Add sloped ramp quad from (x=3, y=4) to (x=14, y=0) across z in [0.8..10.0].
    {
        let verts: [f32; 12] = [
            3.0, 4.0, 0.8,
            14.0, 0.0, 0.8,
            14.0, 0.0, 10.0,
            3.0, 4.0, 10.0,
        ];
        let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];
        let bvh = build(Positions::F32(&verts), &indices);
        let pos_id = w.collision.positions(&verts);
        let tree = w.collision.tree(pos_id, &indices, &bvh);
        let polys = w.collision.tree_polygon_ids(tree, 2);
        polys[0] = 0;
        polys[1] = 0;
        let body = w.collision.body_new(tree, 0) as u32;
        w.collision
            .body_offset(body, (body as f64 + 1.0) * 16_777_216.0);
        let io = w.collision.io_mut();
        io[20..23].copy_from_slice(&[3.0, 0.0, 0.8]);
        io[23..26].copy_from_slice(&[14.0, 4.0, 10.0]);
        w.collision.body_set(body);
        w.collision.state.worlds[coll].order.push(body);
        w.collision.state.worlds[coll].version += 1;
        let order = w.collision.state.worlds[coll].order.len() - 1;
        w.movement.flags_mut()[order] = 1;
        w.collision.enable_body(body, true);
    }

    let profile = fighter_profile();
    let p = SpaceVehiclePad {
        profile_index: 0,
        expire_time_enemy: 1e6,
        expire_time_field: 1e6,
        decay_time: 1e6,
        ..pad(1, [40.0, 1.0, -5.0])
    };
    let fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &p, 0, &profile, 0.0, None, None);
    let mut space = SpaceBattlefield::from_parts(
        SpaceBounds::from_world_json(&parse("{}").unwrap()),
        SpaceWorldAnims::default(),
        vec![profile],
        vec![p],
        vec![fighter],
        vec![],
        vec![],
        coll,
        w.collision.state.bodies.len(),
    );
    space.pilots = PilotBots::build(
        &space.bounds,
        &space.pads,
        &space.fighters,
        &space.profiles,
        &parse("{}").unwrap(),
        64,
        0,
    );
    w.space = Some(Box::new(space));

    let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
    soldier.position = [0.0, 4.0, 0.0];
    soldier.settled = soldier.position;
    soldier.grounded = true;
    soldier.max_speed = 6.5;
    soldier.max_strafe = 4.5;
    soldier.max_turn = 6.0;
    soldier.max_pitch = 4.0;
    soldier.accel = 70.0;
    let mut unit = UnitRow {
        alive: true,
        team: 1.0,
        nav_radius: Some(0.45),
        soldier: UnitBody::Soldier(soldier),
        controller: Some(Ref::new(0, 1)),
        ..Default::default()
    };
    unit.combat.health = 117.0;
    unit.combat.max_health = 117.0;
    let key = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit));
    let unit_ref = key.reference();
    prepare_synthetic_tail(&mut w);

    let mover = w.movement.new_mover(coll, |m| {
        *m = Mover::new(coll);
        m.max_speed = 6.5;
        m.max_strafe = 4.5;
        m.max_jump = 7.5;
        m.max_turn = 6.0;
        m.max_pitch = 4.0;
        m.accel = 70.0;
        m.position = V3::new(0.0, 4.0, 0.0);
        m.settled = m.position;
        m.grounded = true;
    });
    w.movement.reserve_mover(mover, 256, 16, 512);
    w.movement.prepare_world_gathers(&w.collision.state, coll);

    admit_test_swbf2_nav(
        &mut w,
        coll,
        &[([0.0, 4.0, 0.0], 3.0), ([80.0, 0.0, 0.0], 3.0)],
        &[(0, 1)],
        &[],
    );
    w.collision.state.bodies[parked_blocker as usize].moving = true;
    w.collision.enable_body(parked_blocker, true);
    w.movement.flags_mut()[blocker_order] = 1;

    let mut boarded = false;
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    for tick in 1..=1200u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
        if w.space_pilot_view(unit_ref)
            .is_some_and(|v| v.phase == PilotPhase::Hangar)
        {
            boarded = true;
            break;
        }
        step_pilot_mover(&mut w, unit_ref, mover, SPACE_FIXED_DT);
    }
    COUNTING.with(|on| on.set(false));
    let allocs = ALLOCATIONS.with(|n| n.get());

    assert!(boarded, "pilot must route down the bent ramp and board craft beyond node_reach");
    assert_eq!(w.space().unwrap().pilots.census.boardings, 1);
    assert_eq!(w.space().unwrap().pilots.census.walk_failures, 0);
    assert!(w.space().unwrap().pilots.census.blocked_seats >= 1);
    assert_eq!(allocs, 0, "routed bent-ramp walk and boarding must allocate 0 times");
    assert_eq!(
        w.nav_walk_step(unit_ref, SPACE_FIXED_DT),
        NavWalkStatus::Failed,
        "external walk must be released upon boarding"
    );
}

#[test]
fn a_free_bot_routes_up_a_ramp_to_a_barrier_ringed_chair_within_notify_radius_with_zero_allocs() {
    use crate::ai::owned::tests::phase::{ALLOCATIONS, COUNTING};
    use crate::movement::soldier::{Mover, V3};
    use crate::space::combat::SpaceDamageCategory;
    use crate::space::objects::{
        SpaceBuildingAttachment, SpaceTerminalState, SpaceTurretPilotType, SpaceTurretState,
        TurretTargetFilter,
    };
    use crate::space::tests::add_synthetic_building;
    use crate::world::owner::NavWalkStatus;
    use bfe_content::{
        bvh::{build, Positions},
        install::CollisionKernel,
    };

    // Lower deck (y = 0) separated by a bulkhead from an upper chair room (y = 4)
    // reached via a side ramp and doorway. The remote turret chair sits 40.3 m
    // from the soldier inside its own navigation barrier, so neither a 30 m
    // straight check nor an exact-center route can reach it.
    let (mut w, coll) = make_synthetic_f3_world();
    add_synthetic_box(&mut w, coll, [-6.0, -1.0, -10.0], [16.5, 0.0, 10.0], true);
    add_synthetic_box(&mut w, coll, [15.8, 0.0, -10.0], [16.2, 7.0, 2.0], true);
    add_synthetic_box(&mut w, coll, [27.5, 3.0, -10.0], [48.0, 4.0, 10.0], true);
    add_synthetic_box(&mut w, coll, [31.8, 4.0, -10.0], [32.2, 8.0, 4.0], true);
    {
        let verts: [f32; 12] = [
            16.0, 0.0, 2.5,
            28.0, 4.0, 2.5,
            28.0, 4.0, 9.5,
            16.0, 0.0, 9.5,
        ];
        let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];
        let bvh = build(Positions::F32(&verts), &indices);
        let pos_id = w.collision.positions(&verts);
        let tree = w.collision.tree(pos_id, &indices, &bvh);
        let polys = w.collision.tree_polygon_ids(tree, 2);
        polys[0] = 0;
        polys[1] = 0;
        let body = w.collision.body_new(tree, 0) as u32;
        w.collision
            .body_offset(body, (body as f64 + 1.0) * 16_777_216.0);
        let io = w.collision.io_mut();
        io[20..23].copy_from_slice(&[16.0, 0.0, 2.5]);
        io[23..26].copy_from_slice(&[28.0, 4.0, 9.5]);
        w.collision.body_set(body);
        w.collision.state.worlds[coll].order.push(body);
        w.collision.state.worlds[coll].version += 1;
        let order = w.collision.state.worlds[coll].order.len() - 1;
        w.movement.flags_mut()[order] = 1;
        w.collision.enable_body(body, true);
    }

    let make_building = |index: usize,
                         name: &str,
                         at: [f64; 3],
                         turret: Option<SpaceTurretState>,
                         terminal: Option<SpaceTerminalState>| {
        let mut m = three::Matrix4::default().elements;
        m[12..15].copy_from_slice(&at);
        SpaceBuildingAttachment {
            building_index: index,
            instance: index,
            name: name.into(),
            class: name.into(),
            base: if terminal.is_some() {
                "remoteterminal".into()
            } else {
                "armedbuilding".into()
            },
            bodies: [None; 3],
            animated_index: None,
            matrix: m,
            position: at,
            center: at,
            local_center_offset: [0.0; 3],
            radius: 2.0,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret,
            terminal,
        }
    };
    let remote_gun = SpaceTurretState {
        pilot_type: SpaceTurretPilotType::Remote,
        weapon: SpaceWeaponSpec {
            shot_delay: 0.25,
            velocity: 600.0,
            life_span: 1.0,
            max_damage: 10.0,
            max_range: 450.0,
            ..Default::default()
        },
        filter: TurretTargetFilter {
            target_person: false,
            target_vehicle: true,
            target_building: false,
            min_range: 0.0,
            max_range: 450.0,
        },
        yaw: 0.0,
        pitch: 0.0,
        prev_yaw: 0.0,
        prev_pitch: 0.0,
        yaw_min: -std::f64::consts::PI,
        yaw_max: std::f64::consts::PI,
        pitch_min: -1.0,
        pitch_max: 1.0,
        turn_rate: 3.5,
        pitch_rate: 3.5,
        clock: Default::default(),
        barrage_remaining: 0,
        barrage_timer: 0.0,
        trigger_was_held: false,
        warm_up: 0.0,
        heat: 0.0,
        overheated: false,
        shots_fired: 0,
        muzzle_offsets: Vec::new(),
        remote_operator: None,
        pending_remote_aim: None,
        pending_remote_fire: false,
        pending_remote_spread_scale: None,
        kick: Default::default(),
    };
    let chair = SpaceTerminalState {
        remote_object_name: String::new(),
        next_terminal_name: String::new(),
        prev_terminal_name: String::new(),
        remote_turret: Some(0),
        next_terminal: Some(1),
        prev_terminal: Some(1),
        occupant: None,
        seat: None,
        feet: None,
    };

    add_synthetic_building(&mut w, 0, 1.0, 300.0, [0.0, 50.0, 0.0], [None; 3], false);
    add_synthetic_building(&mut w, 1, 1.0, 300.0, [40.0, 5.0, 0.0], [None; 3], false);
    let buildings = vec![
        make_building(0, "gun", [0.0, 50.0, 0.0], Some(remote_gun), None),
        make_building(1, "chair", [40.0, 5.0, 0.0], None, Some(chair)),
    ];

    let mut space = SpaceBattlefield::from_parts(
        SpaceBounds::from_world_json(&parse("{}").unwrap()),
        SpaceWorldAnims::default(),
        vec![fighter_profile()],
        vec![],
        vec![],
        buildings,
        vec![],
        coll,
        w.collision.state.bodies.len(),
    );
    space.pilots = PilotBots::build(
        &space.bounds,
        &space.pads,
        &space.fighters,
        &space.profiles,
        &parse("{}").unwrap(),
        64,
        2,
    );
    space.pilots.routes.exits.push(super::routes::Exit {
        team: 1,
        center: [0.0, 0.0, -20.0],
        ..Default::default()
    });
    w.space = Some(Box::new(space));

    let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 117.0, 0.0, 0.0);
    soldier.position = [0.0, 0.0, 0.0];
    soldier.settled = soldier.position;
    soldier.grounded = true;
    soldier.max_speed = 6.5;
    soldier.max_strafe = 4.5;
    soldier.max_turn = 6.0;
    soldier.max_pitch = 4.0;
    soldier.accel = 70.0;
    let mut unit = UnitRow {
        alive: true,
        team: 1.0,
        nav_radius: Some(0.45),
        soldier: UnitBody::Soldier(soldier),
        controller: Some(Ref::new(0, 1)),
        ..Default::default()
    };
    unit.combat.health = 117.0;
    unit.combat.max_health = 117.0;
    let key = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit));
    let unit_ref = key.reference();
    prepare_synthetic_tail(&mut w);

    let mover = w.movement.new_mover(coll, |m| {
        *m = Mover::new(coll);
        m.max_speed = 6.5;
        m.max_strafe = 4.5;
        m.max_jump = 7.5;
        m.max_turn = 6.0;
        m.max_pitch = 4.0;
        m.accel = 70.0;
        m.position = V3::new(0.0, 0.0, 0.0);
        m.settled = m.position;
        m.grounded = true;
    });
    w.movement.reserve_mover(mover, 256, 16, 512);
    w.movement.prepare_world_gathers(&w.collision.state, coll);

    admit_test_swbf2_nav_with_barriers(
        &mut w,
        coll,
        &[
            ([0.0, 0.0, 0.0], 4.0),
            ([12.0, 0.0, 6.0], 4.0),
            ([30.0, 4.0, 6.0], 4.0),
            ([36.0, 4.0, 6.0], 4.0),
            ([36.0, 4.0, 0.0], 4.0),
        ],
        &[(0, 1), (1, 2), (2, 3), (3, 4)],
        &[],
        r#"{"sourceIndex":0,"name":"chair_box","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,40,4,0],"size":[1.4,3,1.4],"flags":1}"#,
    );

    // This scene has no bot controllers and so no targets: the mission keeps
    // bots in turrets, and a chair needs none.
    assert!(w.space_set_pilot_stay_in_turrets(true));
    // Beyond the mission's notify radius (35 m < 40.3 m): the bot stays Free.
    assert!(w.space_set_pilot_notify_radius(35.0));
    for tick in 1..=35u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
    }
    assert_eq!(
        w.space_pilot_view(unit_ref).map(|v| v.phase),
        Some(PilotPhase::Free),
        "chair outside notify radius must not lure a bot"
    );

    // Within notify radius (65 m >= 40.3 m): the bot starts a routed walk up the ramp,
    // cancels cleanly on hold, then resumes on unhold and boards the chair with 0 allocs.
    assert!(w.space_set_pilot_notify_radius(65.0));
    for tick in 36..=90u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
        if w.space_pilot_view(unit_ref)
            .is_some_and(|v| v.phase == PilotPhase::ToChair)
        {
            break;
        }
    }
    assert_eq!(
        w.space_pilot_view(unit_ref).map(|v| v.phase),
        Some(PilotPhase::ToChair),
        "chair within notify radius must begin a routed walk"
    );
    assert!(w.space_pilot_hold(unit_ref, true));
    assert_eq!(w.space_pilot_view(unit_ref).map(|v| v.phase), Some(PilotPhase::Free));
    assert_eq!(
        w.nav_walk_step(unit_ref, SPACE_FIXED_DT),
        NavWalkStatus::Failed,
        "hold must release the routed chair walk"
    );
    assert!(w.space_pilot_hold(unit_ref, false));

    let mut boarded = false;
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    for tick in 91..=1400u32 {
        w.ai.begin_nav_tick(tick);
        after_ai_in(&mut w, SPACE_FIXED_DT);
        if w.space_pilot_view(unit_ref)
            .is_some_and(|v| v.phase == PilotPhase::Chair)
        {
            boarded = true;
            break;
        }
        step_pilot_mover(&mut w, unit_ref, mover, SPACE_FIXED_DT);
    }
    COUNTING.with(|on| on.set(false));
    let allocs = ALLOCATIONS.with(|n| n.get());

    assert!(boarded, "bot must route up the ramp round the chair barrier and board the chair");
    assert_eq!(w.space().unwrap().pilots.census.chair_boardings, 1);
    assert_eq!(w.space().unwrap().pilots.census.walk_failures, 0);
    assert_eq!(allocs, 0, "routed chair walk and boarding must allocate 0 times");
    assert_eq!(
        w.nav_walk_step(unit_ref, SPACE_FIXED_DT),
        NavWalkStatus::Failed,
        "external walk must be released upon boarding the chair"
    );
}

// The bound of a battle tick and of a census count. The tests run with overflow checks on, so an
// unchecked sum or difference on either panics here.

#[test]
fn a_census_count_saturates_at_the_bound() {
    let mut count = u32::MAX - 2;
    bump(&mut count, 1);
    assert_eq!(count, u32::MAX - 1);
    bump(&mut count, 5);
    assert_eq!(count, u32::MAX);
    bump(&mut count, 1);
    assert_eq!(count, u32::MAX);
    assert_eq!(super::count(usize::MAX), u32::MAX, "a count of items clamps");
}

#[test]
fn a_bots_deadlines_stay_ahead_of_the_tick_at_the_bound() {
    let other = PilotTarget::Craft { index: 3, generation: 2 };
    let near = u32::MAX - 3;
    let mut bot = Bot { target: other, shot_tick: near - 100, ..Bot::fresh(1.0, near - 200) };
    bot.give_up(near);
    assert_eq!(bot.shun_until, u32::MAX, "the shun deadline saturates instead of wrapping to a small number");
    assert!(bot.shuns(other, near), "still shunned before the bound");
    assert!(!bot.shuns(other, u32::MAX), "and over at the bound, not shunned for ever");
    let chasing = Bot { target: other, shot_tick: near - 2_000, ..Bot::fresh(1.0, near - 3_000) };
    assert!(chasing.no_shot(u32::MAX), "an old shot is long past");
    assert!(!chasing.no_shot(u32::MIN), "a tick before the shot is not past it");
}

#[test]
fn pilots_walk_to_a_craft_and_search_across_the_tick_bound() {
    // One free bot and one craft, started 40 ticks below the bound and run across it.
    let (mut w, bots) = deck(&[[12.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]], &[]);
    let me = bots[0].reference();
    w.space_mut().unwrap().pilots.tick = u32::MAX - 40;
    run(&mut w, 120);
    assert_eq!(w.space().unwrap().pilots.tick, u32::MAX, "the pilot tick saturates");
    let view = w.space_pilot_view(me).expect("steered by the pilots");
    assert!(view.since <= u32::MAX && view.phase != PilotPhase::Fight);
    // Eight craft aloft over an empty field keep searching on their own think ticks at the bound.
    let craft: Vec<[f64; 3]> = (0..8).map(|i| [f64::from(i) * 40.0, 0.0, 0.0]).collect();
    let crew: Vec<[f64; 3]> = craft.iter().map(|c| [c[0] + 4.0, c[1], c[2]]).collect();
    let (mut w, bots) = deck(&craft, &crew, &[]);
    for (i, key) in bots.iter().enumerate() {
        aloft(&mut w, key, i, PilotPhase::Fight);
    }
    w.space_mut().unwrap().pilots.tick = u32::MAX - 20;
    w.space_mut().unwrap().pilots.census.target_searches = u32::MAX - 3;
    run(&mut w, 60);
    let pilots = &w.space().unwrap().pilots;
    assert_eq!(pilots.tick, u32::MAX);
    assert_eq!(pilots.census.target_searches, u32::MAX, "a census count at its bound stays there");
}

#[test]
fn the_space_tick_saturates_at_the_bound_and_the_battle_keeps_stepping() {
    let (mut w, _) = deck(&[[12.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]], &[]);
    w.space_mut().unwrap().tick = u32::MAX - 2;
    for _ in 0..6 {
        assert!(w.space_step_tick(SPACE_FIXED_DT));
    }
    let space = w.space().unwrap();
    assert_eq!(space.tick, u32::MAX, "the space tick saturates");
    assert!(space.elapsed_s.is_finite());
    let _ = space.state_digest(None);
}

/// A craft with a pilot's seat and two gunners' seats, a bot and two people
/// standing beside it.
fn three_seats() -> (Box<World>, Vec<F3UnitKey>) {
    let gun = |seat: u8| SpaceWeaponSpec { seat, channel: 0, ai_targets_vehicle: true, max_range: 500.0, velocity: 900.0, ..Default::default() };
    let profile = SpaceFlightProfile { seat_count: 3, weapons: vec![gun(0), gun(1), gun(2)], ..fighter_profile() };
    let (mut w, keys) = scene(vec![profile], &[(0, 1, [0.0; 3])], &[(1, [4.0, 0.0, 0.0]), (1, [-4.0, 0.0, 0.0]), (1, [0.0, 0.0, 4.0])], &[], |_, _| Vec::new());
    for key in &keys[1..] {
        let u = w.sim.store.unit.get_mut(key.reference()).unwrap();
        u.human = true;
        u.controller = None;
    }
    (w, keys)
}

fn seat_of(w: &World, key: &F3UnitKey) -> Option<u8> {
    w.space_seat_for_unit(key).map(|s| s.seat)
}

#[test]
fn a_person_boarding_takes_the_seat_a_bot_holds_and_the_bot_moves_on() {
    let (mut w, keys) = three_seats();
    let (bot, person, other) = (&keys[0], &keys[1], &keys[2]);
    let seat = w.space_board_seat(bot, 0, 0).expect("the bot boards the pilot's seat");
    let slot = bot.reference().slot as usize;
    let life = w.sim.store.unit.get(bot.reference()).unwrap().generation;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Hangar, receipt: Some(seat), craft: 0, ..Bot::fresh(life, 0) };
    let mine = w.space_board_person(person, 0).expect("the person boards");
    assert_eq!(mine.seat, 0, "the earliest seat a bot holds goes to the person");
    assert_eq!(seat_of(&w, bot), Some(1), "the bot moves to the next free seat after its own");
    let b = w.space().unwrap().pilots.bots[slot];
    assert_eq!((b.phase, b.receipt.map(|r| r.seat)), (PilotPhase::Gunner, Some(1)));
    // Another person takes the next seat that is free or a bot holds, never a person's.
    assert_eq!(w.space_board_person(other, 0).map(|s| s.seat), Some(1));
    assert_eq!(seat_of(&w, bot), Some(2));
    assert_eq!(seat_of(&w, person), Some(0));
}

#[test]
fn the_seat_keys_pass_a_persons_seat_and_trade_places_with_a_bot() {
    let (mut w, keys) = three_seats();
    let (bot, person, other) = (&keys[0], &keys[1], &keys[2]);
    let mine = w.space_board_person(person, 0).expect("the person boards the pilot's seat");
    let seat = w.space_board_seat(bot, 0, 1).expect("the bot takes a gunner's seat");
    let slot = bot.reference().slot as usize;
    let life = w.sim.store.unit.get(bot.reference()).unwrap().generation;
    w.space_mut().unwrap().pilots.bots[slot] = Bot { phase: PilotPhase::Gunner, receipt: Some(seat), craft: 0, seat: 1, ..Bot::fresh(life, 0) };
    w.space_mut().unwrap().fighters[0].pending_input.throttle = 1.0;
    // Next: the bot's seat; they trade places and the pilot's stick is let go.
    let mine = w.space_switch_seat(mine, 1).expect("a seat to move to");
    assert_eq!((mine.seat, seat_of(&w, bot)), (1, Some(0)));
    assert_eq!(w.space().unwrap().fighters[0].pending_input.throttle, 0.0);
    let b = w.space().unwrap().pilots.bots[slot];
    assert_eq!((b.phase, b.receipt.map(|r| r.seat)), (PilotPhase::Hangar, Some(0)), "a landed craft's new pilot starts in the hangar");
    // Next again: the free seat; then round the craft, past a person's seat.
    let mine = w.space_switch_seat(mine, 1).expect("the free seat");
    assert_eq!(mine.seat, 2);
    assert_eq!(w.space_board_person(other, 0).map(|s| s.seat), Some(0), "the bot's pilot seat goes to the second person");
    assert_eq!(seat_of(&w, bot), Some(1));
    let mine = w.space_switch_seat(mine, 1).expect("round past the person to the bot");
    assert_eq!((mine.seat, seat_of(&w, bot)), (1, Some(2)));
    let mine = w.space_switch_seat(mine, -1).expect("back the other way, past the person");
    assert_eq!((mine.seat, seat_of(&w, bot)), (2, Some(1)));
}
