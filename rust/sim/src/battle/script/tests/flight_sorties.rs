//! Scripted human sorties in the prepared Space Assault mission, with the
//! mission's own bots and their pilots. A scripted Republic pilot walks to a
//! parked V-wing, takes off, destroys the CIS sensor, flies home and lands; a
//! scripted marine flies a gunship onto the CIS deck and walks for its door
//! while the battle runs to ten minutes. Every action goes through the
//! ordinary player receipts. A scripted route cannot evade, so the cases name
//! who flies: nobody, the human's own team only, or both teams.

use bfe_wide::Wide;
use super::*;
use crate::gen::{F4LiveInput, F4TailInput, WorldClockState, WorldTickInput};
use crate::space::{
    combat::EmittingBody,
    flight::{SpaceFighter, SpaceFlightState, SpacePilotInput},
    SpaceEvent, SpaceImpactCause, SpaceImpactTarget, SPACE_FIXED_DT,
};
use crate::world::admit::{self, mission::*, Bots, Options};
use crate::world::owner::World;

const PILOT: &str = "rep_inf_ep3_pilot";
const MARINE: &str = "rep_inf_ep3_marine";
const POPULATION: u32 = 32;
const TEN_MINUTES: u32 = 36_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flying {
    /// No bot pilots: the controls recorded before the pilots existed.
    Nobody,
    /// Every bot of the enemy team is held on foot through the public hold.
    OwnTeam,
    Both,
}

struct Battle {
    world: Box<World>,
    admitted: admit::Admitted,
    clock: WorldClockState,
    tick: u32,
    flying: Flying,
    /// Hits on the craft the human occupies: tick, hull lost, destroyed, emitter.
    human_hits: Vec<(u32, f64, bool, String)>,
}

impl Battle {
    fn start(class: &str, bots: u32, flying: Flying) -> Self {
        let files = bfe_content::files::DirFiles::from_resolved_root(prepared_root(), bfe_family::GameFamily::Swbf2);
        let options = Options {
            family: GameFamily::Swbf2, mode: "ass".into(), practice_target: None, map: "spa6".into(),
            side: "rep".into(), unit: Some(class.into()), seed: 81, heroes: false, enhanced: false,
            name: None, bots: Bots::default(),
        };
        let selection = MissionSelection {
            family: GameFamily::Swbf2, map: "spa6".into(), era: "cw".into(), mode: "ass".into(), seed: 81,
        };
        let mut world = World::new();
        let preferences = MissionPreferences { bots_per_team: Some(bots), direct_spawn: true, ..Default::default() };
        let (admitted, _, _) = admit_developer_mission(&mut world, &files, &options, &selection, preferences).unwrap();
        world.step_init(&admitted.setup);
        if flying == Flying::Nobody {
            assert!(world.space_set_pilots_disabled(true));
        }
        println!("SORTIE class={class} bots={bots} flying={flying:?} notify={:?}", world.space_pilot_notify_radius());
        Battle {
            world, admitted, tick: 0, flying, human_hits: Vec::new(),
            clock: WorldClockState { hz: 60., max_steps: 4., max_debt: 8., budget: f64::INFINITY, ..Default::default() },
        }
    }

    fn advance(&mut self) {
        self.tick += 1;
        if self.flying == Flying::OwnTeam {
            let enemy: Vec<_> = (0..self.world.sim.store.unit.slots())
                .filter_map(|slot| self.world.sim.store.unit.at(slot))
                .filter(|(_, u)| u.alive && !u.human && u.team == 2.0)
                .map(|(r, _)| r)
                .collect();
            for unit in enemy {
                self.world.space_pilot_hold(unit, true);
            }
        }
        stamp(&mut self.world, self.tick);
        let out = self.world.step(&WorldTickInput {
            dt: SPACE_FIXED_DT,
            tick: self.tick.into(),
            clock: self.clock.clone(),
            units: self.admitted.units,
            ai: true,
            live: true,
            at: f64::from(self.tick) * 1000. / 60.,
            mouse_scale: 1.,
            ..Default::default()
        });
        assert!(out.accepted, "world refused tick {}", self.tick);
        self.clock = out.clock;
        let space = self.world.space().unwrap();
        for i in &space.impacts {
            let SpaceImpactTarget::Fighter { index, .. } = i.target else { continue };
            if !space.fighters.get(index).is_some_and(|f| f.pilot.is_some_and(|p| p.is_human)) {
                continue;
            }
            let by = match (i.emitter, i.cause) {
                (Some(EmittingBody::Fighter { index: e, .. }), _) => space.fighters.get(e).map_or("craft".into(), |o| {
                    format!("{} team {}", space.profiles[o.profile_index].class_name, o.team)
                }),
                (Some(EmittingBody::Building { index: e, .. }), _) => space.buildings.iter()
                    .find(|a| a.building_index == e).map_or(format!("object {e}"), |a| a.name.clone()),
                (None, SpaceImpactCause::Contact { body }) => format!("contact with body {body}"),
                (None, cause) => format!("{cause:?}"),
            };
            self.human_hits.push((self.tick, i.hull_loss, i.destroyed, by));
        }
    }

    fn team_points(&self, team: usize) -> u32 {
        self.world.event_rules().unwrap().team_points()[team]
    }

    /// The last hits on the human's craft, oldest first.
    fn print_hits(&self, last: usize) {
        for (tick, hull, destroyed, by) in self.human_hits.iter().rev().take(last).rev() {
            println!("HUMAN_HIT tick={tick} hull_loss={hull:.0} destroyed={destroyed} by={by}");
        }
    }
}

/// Mirror the three input banks the local runtime writes before World::step.
fn stamp(world: &mut World, i: u32) {
    let dt = 1. / 60.;
    let t = f64::from(i);
    let mut heap = Vec::new();
    macro_rules! put {
        ($value:expr, $name:literal, $type:ty) => {{
            let mut words = [0.; <$type>::WORDS];
            $value.put(&mut words, &mut heap);
            let mut bytes = [0u8; <$type>::WORDS * 8];
            for (chunk, value) in bytes.chunks_exact_mut(8).zip(words) {
                chunk.copy_from_slice(&value.to_le_bytes());
            }
            assert!(admit::banks::write(world, $name, &bytes).is_some());
        }};
    }
    put!(F4LiveInput { dt, tick: t }, "F4_LIVE_INPUT", F4LiveInput);
    put!(F4TailInput { dt, tick: t, server: true, ..Default::default() }, "F4_TAIL_INPUT", F4TailInput);
    put!(
        AiPhaseHeader {
            facts: AiPhaseFacts { dt, tick: i, network_views: true, ..Default::default() },
            time: t / 60.,
            unit_phase_follows: true,
            ..Default::default()
        },
        "AI_HEADER",
        AiPhaseHeader
    );
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    v8math::hypot3(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}

fn aim(f: &SpaceFighter, p: [f64; 3], throttle: f64, shoot: bool) -> SpacePilotInput {
    let distance = v8math::max(distance(f.position, p), 0.00001);
    let v: [f64; 3] = std::array::from_fn(|i| (p[i] - f.position[i]) / distance);
    let dot = |a: [f64; 3]| a[0] * v[0] + a[1] * v[1] + a[2] * v[2];
    let z = dot(f.forward());
    SpacePilotInput {
        throttle,
        stick_x: (-v8math::atan2(dot(f.right()), z) * 2.5).clamp(-1., 1.),
        stick_y: (v8math::asin(dot(f.up()).clamp(-1., 1.)) * 2.5).clamp(-1., 1.),
        fire_primary: shoot && z > 0.985,
        fire_secondary: shoot && z > 0.985,
        lock_target: shoot,
        ..Default::default()
    }
}

fn walk_input(world: &mut World, player: Ref<Unit>, p: [f64; 3]) {
    let from = world.space_unit_position(player).unwrap();
    let yaw = v8math::atan2(p[0] - from[0], p[2] - from[2]);
    world.space_drive_unit(player, Some(yaw), 1., 0., false, None).unwrap();
}

/// A breadth-first walk over the ground rays, around anything a soldier's
/// knees or chest would strike.
fn foot_path(world: &mut World, start: [f64; 3], goal: [f64; 3], radius: f64) -> Vec<[f64; 3]> {
    use std::collections::VecDeque;
    let n = 101usize;
    let mid = n / 2;
    let cell = 1.45;
    let idx = |x: usize, z: usize| x + z * n;
    let mut prev = vec![usize::MAX; n * n];
    let mut pos = vec![[0.; 3]; n * n];
    let origin = idx(mid, mid);
    prev[origin] = origin;
    pos[origin] = start;
    let mut q = VecDeque::from([origin]);
    let mut best = origin;
    while let Some(i) = q.pop_front() {
        let p = pos[i];
        if distance(p, goal) < distance(pos[best], goal) {
            best = i
        }
        if distance(p, goal) < radius {
            best = i;
            break;
        }
        for (dx, dz) in [(-1isize, 0isize), (1, 0), (0, 1), (0, -1), (-1, -1), (-1, 1), (1, 1), (1, -1)] {
            let Some(x) = (i % n).checked_add_signed(dx).filter(|k| *k < n) else { continue };
            let Some(z) = (i / n).checked_add_signed(dz).filter(|k| *k < n) else { continue };
            let j = idx(x, z);
            if prev[j] != usize::MAX {
                continue;
            }
            let gx = start[0] + (x as f64 - mid as f64) * cell;
            let gz = start[2] + (z as f64 - mid as f64) * cell;
            let ray = crate::collision::Ray { ox: gx, oy: p[1] + 1.25, oz: gz, dx: 0., dy: -1., dz: 0. };
            let Some((code, h)) = world.space_ray_hit(ray, 2.6, -1) else { continue };
            if code == crate::collision::MISS || h.ny.abs() < 0.48 || (h.py - p[1]).abs() > 0.93 {
                continue;
            }
            let a = [gx, h.py, gz];
            let len = distance(a, p);
            let v: [f64; 3] = std::array::from_fn(|k| (a[k] - p[k]) / len);
            if [0.55, 1.38].into_iter().any(|hgt| {
                [-0.30, 0.30].into_iter().any(|r| {
                    let ray = crate::collision::Ray {
                        ox: p[0] + v[2] * r, oy: p[1] + hgt, oz: p[2] - v[0] * r, dx: v[0], dy: v[1], dz: v[2],
                    };
                    world.space_ray_hit(ray, len + 0.28, -1)
                        .is_some_and(|(code, hit)| code != crate::collision::MISS && hit.distance < len + 0.27)
                })
            }) {
                continue;
            }
            prev[j] = i;
            pos[j] = a;
            q.push_back(j);
        }
    }
    let mut route = Vec::new();
    let mut j = best;
    while j != origin {
        route.push(pos[j]);
        j = prev[j];
    }
    route.reverse();
    route
}

fn route_walk(world: &mut World, key: Ref<Unit>, route: &[[f64; 3]], node: &mut usize, goal: [f64; 3]) {
    let p = world.space_unit_position(key).unwrap();
    while *node < route.len() && v8math::hypot(p[0] - route[*node][0], p[2] - route[*node][2]) < 0.68 {
        *node += 1;
    }
    walk_input(world, key, route.get(*node).copied().unwrap_or(goal));
}

/// A point of the object the craft can see, sampled across its bounds.
fn visible_spot(world: &mut World, id: usize, target: usize) -> Option<[f64; 3]> {
    let sp = world.space()?;
    let f = &sp.fighters[id];
    let from = f.position;
    let ignore = f.kernel_body.map_or(-1, |b| b as i32);
    let a = sp.buildings.iter().find(|a| a.building_index == target)?;
    let body = a.bodies[0]?;
    let b = world.space_body_bounds(body)?;
    let mid = std::array::from_fn::<_, 3, _>(|i| (b[i] + b[i + 3]) * 0.5);
    for y in [0., 0.6, -0.6] {
        for x in [0., 0.6, -0.6] {
            for z in [0., 0.6, -0.6] {
                let p: [f64; 3] = std::array::from_fn(|i| mid[i] + [x, y, z][i] * (b[i + 3] - b[i]) * 0.5);
                let d = v8math::max(distance(p, from), 0.001);
                let v: [f64; 3] = std::array::from_fn(|i| (p[i] - from[i]) / d);
                let ray = crate::collision::Ray { ox: from[0], oy: from[1], oz: from[2], dx: v[0], dy: v[1], dz: v[2] };
                if let Some((code, h)) = world.space_ray_hit(ray, d + 45., ignore) {
                    if code == body as i32 {
                        return Some([h.px, h.py, h.pz]);
                    }
                }
            }
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum Ending {
    /// The scripted route finished: tick and the space state digest.
    Completed { tick: u32, digest: Wide },
    /// The human's craft was destroyed under them.
    CraftLost { tick: u32, phase: u32 },
    /// Bots had taken every craft the human could board.
    NoTransport { tick: u32 },
    /// The human rode as crew and its bot pilot never landed on the enemy deck.
    NotDelivered { tick: u32 },
}

struct PilotRun {
    ending: Ending,
    kill_tick: Option<u32>,
    award: Option<u32>,
    /// The human's share of the sensor's hull loss.
    own_hull: f64,
    sensor_hull: f64,
}

/// The pilot walks to the parked Republic V-wing in the lowest craft slot, takes off, attacks the
/// CIS sensor from a flank, breaks off and returns until it falls, then flies
/// home and lands on its own deck.
fn pilot(b: &mut Battle) -> PilotRun {
    b.advance();
    let key = b.world.space_player_unit(b.admitted.handles.player).unwrap();
    let id = {
        let space = b.world.space().unwrap();
        space.fighters.iter()
            .find(|f| f.alive && f.team == 1 && space.profiles[f.profile_index].class_name.contains("vwing"))
            .unwrap().id
    };
    let target = b.world.space_building_by_name("cis_cap1_sensor2").unwrap();
    let starting = b.world.space_building_snapshot(target).unwrap();
    assert!(starting.team == 2 && starting.health == starting.max_health);
    let home = b.world.space().unwrap().fighters[id].position;
    let point = b.world.space().unwrap().buildings.iter().find(|a| a.building_index == target).unwrap().center;
    let seat = loop {
        if let Some(s) = b.world.space_board_seat(&key, id, 0) {
            break s;
        }
        assert!(b.tick < 1800, "pilot could not reach the parked fighter on foot");
        walk_input(&mut b.world, key.reference(), home);
        b.advance();
    };
    let space = b.world.space().unwrap();
    let region = space.bounds.landing_zones.iter().find(|r| r.contains(home)).unwrap().clone();
    let home_portal = space.bounds.forcefields.iter()
        .filter(|f| f.authored_opening && f.team == 1)
        .min_by(|a, b| distance(a.center, region.center).total_cmp(&distance(b.center, region.center)))
        .unwrap().center;
    let deck = [region.center[0], home_portal[1], region.center[2]];
    let spawn_direction = space.fighters[id].forward();
    let exit = std::array::from_fn(|i| home[i] + spawn_direction[i] * 240.);
    let flank = [point[0] - 290., point[1] + 800., home[2] + 280.];
    let stage = [point[0] - 190., point[1] + 450., point[2] + 950.];
    println!("BOARDED tick={} craft={id} seat={seat:?}", b.tick);
    let abort = 330.0;
    let (mut phase, mut took_off, mut round_flank, mut spot) = (0, 0, false, None);
    let (mut life, mut pass_origin) = (starting.clone(), stage);
    let (mut own_hull, mut kill_tick, mut award, mut score_before) = (0., None, None, 0);
    while b.tick < 40_000 {
        if phase == 4 && b.tick.is_multiple_of(12) {
            spot = visible_spot(&mut b.world, id, target);
        }
        let f = &b.world.space().unwrap().fighters[id];
        if !(f.alive && f.generation == seat.generation) {
            return PilotRun { ending: Ending::CraftLost { tick: b.tick, phase }, kill_tick, award, own_hull, sensor_hull: starting.health };
        }
        let energy = b.world.space().unwrap().profiles[f.profile_index].energy_bar;
        let input = match phase {
            0 => {
                phase = 1;
                SpacePilotInput { takeoff_or_land: true, ..Default::default() }
            }
            1 => {
                if f.state == SpaceFlightState::Flying {
                    phase = 2;
                    took_off = b.tick;
                }
                SpacePilotInput::default()
            }
            2 => {
                if b.tick - took_off >= 110 {
                    phase = 3;
                }
                SpacePilotInput { throttle: 1.0, boost: true, ..Default::default() }
            }
            3 => {
                if !round_flank && distance(f.position, flank) < 115. {
                    round_flank = true;
                }
                if round_flank && distance(f.position, stage) < 110.0 {
                    phase = 4;
                }
                let mut v = aim(f, if round_flank { stage } else { flank }, 1., false);
                v.boost = f.energy > 0.2 * energy || f.boosting;
                v
            }
            4 => {
                if distance(f.position, point) < abort || f.health < 0.65 * f.max_health {
                    phase = 5;
                    let d = distance(f.position, point);
                    pass_origin = std::array::from_fn(|i| point[i] + (f.position[i] - point[i]) * (abort + 180.) / d);
                }
                if !life.alive {
                    phase = 6;
                }
                aim(f, spot.unwrap_or(point), -1., true)
            }
            5 => {
                if (distance(f.position, point) > abort + 170. || distance(f.position, pass_origin) < 100.)
                    && f.health > 0.72 * f.max_health
                {
                    phase = 4;
                }
                if !life.alive {
                    phase = 6;
                }
                let mut v = aim(f, pass_origin, 1., false);
                v.boost = f.energy > 35.;
                v
            }
            6 => {
                if distance(f.position, stage) < 110. {
                    phase = 7;
                }
                let mut v = aim(f, stage, 1., false);
                v.boost = f.energy > 25.;
                v
            }
            7 => {
                if distance(f.position, flank) < 110. {
                    phase = 8;
                }
                let mut v = aim(f, flank, 1., false);
                v.boost = f.energy > 25.;
                v
            }
            8 => {
                let outer = [exit[0] + spawn_direction[0] * 135., home[1] + 22., exit[2] + spawn_direction[2] * 135.];
                if distance(f.position, outer) < 55. {
                    phase = 9;
                }
                aim(f, outer, 0.4, false)
            }
            9 => {
                if distance(f.position, exit) < 45. {
                    phase = 10;
                }
                aim(f, exit, -1., false)
            }
            _ => {
                if f.state == SpaceFlightState::Landed {
                    break;
                }
                let mut input = aim(f, deck, -1., false);
                if b.world.space().unwrap().bounds.landing_zone_at(f.position).is_some() {
                    input.takeoff_or_land = f.state == SpaceFlightState::Flying && !f.action_held;
                }
                input
            }
        };
        if life.alive {
            score_before = b.team_points(1);
        }
        assert!(b.world.space_control_seat(seat, input), "pilot seat lost on tick {}", b.tick);
        b.advance();
        for event in b.world.space().unwrap().events() {
            match *event {
                SpaceEvent::BuildingDestroyed { building, attacker_unit: Some(o), .. } if building == target && o == seat.owner => {
                    kill_tick = Some(b.tick);
                }
                SpaceEvent::BuildingDamaged { building, attacker_unit: Some(o), hull_loss, .. } if building == target && o == seat.owner => {
                    own_hull += hull_loss;
                }
                _ => {}
            }
        }
        life = b.world.space_building_snapshot(target).unwrap();
        if let Some(killed) = kill_tick.filter(|k| b.tick <= k + 4 && award.is_none()) {
            let delta = b.team_points(1) - score_before;
            if delta != 0 {
                award = Some(delta);
                println!("SENSOR kill tick={killed} award={delta} human share of hull loss {own_hull:.0} of {:.0}", starting.health);
            }
        }
    }
    let f = &b.world.space().unwrap().fighters[id];
    assert_eq!(f.state, SpaceFlightState::Landed);
    assert!(region.contains(f.position), "landed outside the home deck at {:?}", f.position);
    assert!(b.world.space_leave_seat(seat).is_some());
    PilotRun {
        ending: Ending::Completed { tick: b.tick, digest: b.world.space_state_digest().unwrap() },
        kill_tick, award, own_hull, sensor_hull: starting.health,
    }
}

struct MarineRun {
    ending: Ending,
    boarded: u32,
    landed: Option<u32>,
    /// Peak count of Republic craft aloft with a bot aboard.
    peak_friendly_aloft: usize,
    bot_airborne_ticks: u32,
    rosters: [usize; 2],
}

/// The marine walks to the Republic gunship in the lowest craft slot, or to the nearest free one
/// when a bot has taken it, flies a wide approach onto the CIS deck, lands,
/// leaves the seat and walks for the nearest CIS door while the battle runs to
/// ten minutes. When bots fly every gunship, it boards one waiting on the deck
/// for crew and rides to wherever its bot pilot lands.
fn marine(b: &mut Battle) -> MarineRun {
    b.advance();
    let key = b.world.space_player_unit(b.admitted.handles.player).unwrap();
    let gunship = |w: &World, id: usize| {
        let space = w.space().unwrap();
        let f = &space.fighters[id];
        f.alive && f.team == 1 && f.state == SpaceFlightState::Landed
            && space.profiles[f.profile_index].class_name.contains("gunship")
    };
    let free = |w: &World, id: usize| gunship(w, id) && w.space().unwrap().fighters[id].pilot.is_none();
    // A gunship waiting for crew: a bot at the controls and a gunner seat open.
    let crew_seat = |w: &World, id: usize| {
        let space = w.space().unwrap();
        let f = &space.fighters[id];
        if !gunship(w, id) || !f.pilot.is_some_and(|p| !p.is_human) {
            return None;
        }
        (1..space.profiles[f.profile_index].seat_count).find(|&s| f.occupant(s).is_none())
    };
    let first = b.world.space().unwrap().fighters.iter().position(|f| free(&b.world, f.id)).unwrap();
    let (mut target, mut route, mut node) = ((usize::MAX, 0), Vec::new(), 0);
    let mut census = Census::default();
    let (receipt, id, home) = loop {
        let at = b.world.space_unit_position(key.reference()).unwrap();
        let nearest = |w: &World, wanted: &dyn Fn(&World, usize) -> Option<u8>| {
            let space = w.space().unwrap();
            space.fighters.iter().filter_map(|f| wanted(w, f.id).map(|seat| (f.id, seat, distance(f.position, at))))
                .min_by(|x, y| x.2.total_cmp(&y.2)).map(|(id, seat, _)| (id, seat))
        };
        let choice = if free(&b.world, first) { Some((first, 0)) } else {
            nearest(&b.world, &|w, id| free(w, id).then_some(0))
                .or_else(|| nearest(&b.world, &crew_seat))
        };
        let Some(choice) = choice else {
            if b.tick >= 3600 {
                println!("MARINE no gunship to board tick={}", b.tick);
                return census.finish(b, Ending::NoTransport { tick: b.tick }, b.tick, None);
            }
            b.world.space_drive_unit(key.reference(), None, 0., 0., false, None).unwrap();
            b.advance();
            census.observe(&b.world);
            continue;
        };
        let home = b.world.space().unwrap().fighters[choice.0].position;
        if choice != target {
            println!("MARINE walking to gunship {} seat {} tick={}", choice.0, choice.1, b.tick);
            target = choice;
            route = foot_path(&mut b.world, at, home, 8.0);
            node = 0;
        }
        if let Some(seat) = b.world.space_board_seat(&key, choice.0, choice.1) {
            break (seat, choice.0, home);
        }
        assert!(b.tick < 3600, "marine could not reach a gunship, now walking to {choice:?} at {:.1} m", distance(at, home));
        route_walk(&mut b.world, key.reference(), &route, &mut node, home);
        b.advance();
        census.observe(&b.world);
    };
    if receipt.seat != 0 {
        return crew(b, census, &key, receipt);
    }
    let boarded = b.tick;
    println!("MARINE boarded={boarded} craft={id}");
    let sp = b.world.space().unwrap();
    let zone = sp.bounds.landing_zones.iter().find(|r| r.name.starts_with("cis")).unwrap().clone();
    let portal = sp.bounds.forcefields.iter()
        .filter(|field| field.authored_opening && field.team == 2)
        .min_by(|a, b| distance(home, a.center).total_cmp(&distance(home, b.center)))
        .unwrap().center;
    let radius = v8math::hypot(portal[0] - zone.center[0], portal[2] - zone.center[2]);
    let facing = [(portal[0] - zone.center[0]) / radius, 0., (portal[2] - zone.center[2]) / radius];
    let deck = [zone.center[0], portal[1], zone.center[2]];
    let mouth = std::array::from_fn(|i| portal[i] + facing[i] * 52.);
    let wide = [portal[0] + facing[0] * 480., portal[1] + 15., portal[2] + facing[2] * 480.];
    let flank = [home[0] - 980., 690., home[2] + 320.];
    let overhead = [deck[0] + facing[0] * 650., 170., deck[2] + facing[2] * 650.];
    let (mut phase, mut launch, mut wing) = (0, 0, 0);
    let mut landed = None;
    while b.tick < 12_000 && landed.is_none() {
        let f = &b.world.space().unwrap().fighters[id];
        if !(f.alive && f.generation == receipt.generation) {
            return census.finish(b, Ending::CraftLost { tick: b.tick, phase }, boarded, None);
        }
        let energy = b.world.space().unwrap().profiles[f.profile_index].energy_bar;
        let input = match phase {
            0 => {
                phase = 1;
                SpacePilotInput { takeoff_or_land: true, ..Default::default() }
            }
            1 => {
                if f.state == SpaceFlightState::Flying {
                    launch = b.tick;
                    phase = 2;
                }
                SpacePilotInput::default()
            }
            2 => {
                if b.tick - launch > 100 {
                    phase = 3;
                }
                SpacePilotInput { throttle: 1., boost: true, ..Default::default() }
            }
            3 => {
                let wp = [flank, overhead, wide][wing];
                if distance(f.position, wp) < [145., 240., 100.][wing] {
                    if wing < 2 { wing += 1 } else { phase = 4 }
                }
                let mut v = aim(f, wp, 1., false);
                v.boost = f.energy > 0.2 * energy || f.boosting;
                v
            }
            4 => {
                if distance(f.position, mouth) < 55. {
                    phase = 5;
                }
                let mut v = aim(f, mouth, 1., false);
                v.boost = true;
                v
            }
            _ => {
                if f.state == SpaceFlightState::Landed && zone.contains(f.position) {
                    landed = Some(b.tick);
                    continue;
                }
                let mut v = aim(f, deck, if distance(f.position, deck) > 130. { 1. } else { -1. }, false);
                v.takeoff_or_land = zone.contains(f.position)
                    && !f.action_held
                    && f.state == SpaceFlightState::Flying;
                v
            }
        };
        assert!(b.world.space_control_seat(receipt, input), "marine seat lost on tick {}", b.tick);
        b.advance();
        census.observe(&b.world);
    }
    let Some(landed_at) = landed else {
        return census.finish(b, Ending::CraftLost { tick: b.tick, phase }, boarded, None);
    };
    println!("MARINE landed on the CIS deck tick={landed_at} hull={:.0}", b.world.space().unwrap().fighters[id].health);
    assert!(b.world.space_leave_seat(receipt).is_some());
    walk_to_door(b, census, &key, boarded, landed_at)
}

/// Ride a bot-flown gunship as crew. The bot pilot decides where it lands; on
/// the CIS deck the marine leaves its seat and walks for the nearest door.
fn crew(b: &mut Battle, mut census: Census, key: &crate::gen::F3UnitKey, seat: crate::space::SpaceSeat) -> MarineRun {
    let boarded = b.tick;
    println!("MARINE boarded={boarded} craft={} as crew seat {}", seat.fighter, seat.seat);
    let zone = b.world.space().unwrap().bounds.landing_zones.iter().find(|r| r.name.starts_with("cis")).unwrap().clone();
    let mut landed = None;
    while b.tick < 12_000 && landed.is_none() {
        let f = &b.world.space().unwrap().fighters[seat.fighter];
        if !(f.alive && f.generation == seat.generation) {
            return census.finish(b, Ending::CraftLost { tick: b.tick, phase: 0 }, boarded, None);
        }
        if f.state == SpaceFlightState::Landed && zone.contains(f.position) {
            landed = Some(b.tick);
            break;
        }
        b.advance();
        census.observe(&b.world);
    }
    let Some(landed_at) = landed else {
        return census.finish(b, Ending::NotDelivered { tick: b.tick }, boarded, None);
    };
    println!("MARINE delivered on the CIS deck tick={landed_at}");
    assert!(b.world.space_leave_seat(seat).is_some());
    walk_to_door(b, census, key, boarded, landed_at)
}

/// After landing on the CIS deck: walk for the nearest CIS door until the
/// battle reaches ten minutes.
fn walk_to_door(b: &mut Battle, mut census: Census, key: &crate::gen::F3UnitKey, boarded: u32, landed_at: u32) -> MarineRun {
    b.advance();
    census.observe(&b.world);
    let pos = b.world.space_unit_position(key.reference()).unwrap();
    let entrance = b.world.space().unwrap().doors.iter()
        .filter(|d| d.name.starts_with("cis"))
        .min_by(|a, d| distance(a.position, pos).total_cmp(&distance(d.position, pos)))
        .unwrap().trigger_position;
    let route = foot_path(&mut b.world, pos, entrance, 3.5);
    let (mut node, mut walking) = (0, true);
    while b.tick < TEN_MINUTES {
        if walking {
            let alive = key.validate(&b.world.sim.store).ok().is_some_and(|u| u.alive && !u.removed && !u.combat.dead);
            if alive {
                route_walk(&mut b.world, key.reference(), &route, &mut node, entrance);
            } else {
                println!("MARINE killed on the CIS deck tick={}", b.tick);
                walking = false;
            }
        }
        b.advance();
        census.observe(&b.world);
    }
    let digest = b.world.space_state_digest().unwrap();
    census.finish(b, Ending::Completed { tick: b.tick, digest }, boarded, Some(landed_at))
}

#[derive(Default)]
struct Census {
    peak_friendly_aloft: usize,
    bot_airborne_ticks: u32,
    rosters: [usize; 2],
}

impl Census {
    fn observe(&mut self, w: &World) {
        let space = w.space().unwrap();
        let mut friendly = 0;
        for f in space.fighters.iter().filter(|f| f.alive && f.state != SpaceFlightState::Landed) {
            let bots = std::iter::once(f.pilot).chain(f.gunners).flatten().filter(|p| !p.is_human).count();
            self.bot_airborne_ticks += bots as u32;
            friendly += usize::from(bots > 0 && f.team == 1);
        }
        self.peak_friendly_aloft = self.peak_friendly_aloft.max(friendly);
        for team in [1, 2] {
            let n = w.sim.store.spawning.teams[team].characters.iter()
                .filter(|r| w.sim.store.character.get(**r).is_some_and(|c| !c.human)).count();
            self.rosters[team - 1] = self.rosters[team - 1].max(n);
        }
    }

    fn finish(self, b: &Battle, ending: Ending, boarded: u32, landed: Option<u32>) -> MarineRun {
        println!("MARINE ending={ending:?} landed={landed:?} peak_friendly_aloft={} bot_airborne_ticks={} rosters={:?} scores={:?}",
            self.peak_friendly_aloft, self.bot_airborne_ticks, self.rosters, b.world.event_rules().unwrap().team_points());
        MarineRun {
            ending, boarded, landed, peak_friendly_aloft: self.peak_friendly_aloft,
            bot_airborne_ticks: self.bot_airborne_ticks, rosters: self.rosters,
        }
    }
}

fn prepared() -> bool {
    std::env::var_os("BFE_SWBF2_PREPARED").is_some()
}

fn run_pilot(bots: u32, flying: Flying) -> PilotRun {
    let mut b = Battle::start(PILOT, bots, flying);
    let run = pilot(&mut b);
    println!("PILOT ending={:?} kill={:?} award={:?} human_hull={:.0}/{:.0} hits_on_craft={}",
        run.ending, run.kill_tick, run.award, run.own_hull, run.sensor_hull, b.human_hits.len());
    b.print_hits(6);
    run
}

fn run_marine(bots: u32, flying: Flying) -> MarineRun {
    let mut b = Battle::start(MARINE, bots, flying);
    let run = marine(&mut b);
    b.print_hits(6);
    run
}

fn completed_pilot_sortie(run: &PilotRun, alone: bool) {
    assert!(matches!(run.ending, Ending::Completed { .. }), "{:?}", run.ending);
    assert!(run.kill_tick.is_some(), "the sensor kill was not credited to the human");
    assert_eq!(run.award, Some(18), "the mission's award for the sensor");
    if alone {
        assert!((run.own_hull - run.sensor_hull).abs() < 0.001, "the human did {} of {}", run.own_hull, run.sensor_hull);
    } else {
        // Friendly bots may share the sensor's damage. The kill and award stay the human's.
        assert!(run.own_hull > 0.);
    }
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_pilot_sortie_alone() {
    if !prepared() { return }
    completed_pilot_sortie(&run_pilot(0, Flying::Both), true);
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_pilot_sortie_among_mission_bots_without_pilots() {
    if !prepared() { return }
    completed_pilot_sortie(&run_pilot(POPULATION, Flying::Nobody), true);
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_pilot_sortie_while_its_own_team_flies() {
    if !prepared() { return }
    completed_pilot_sortie(&run_pilot(POPULATION, Flying::OwnTeam), false);
}

/// Both teams fly: the scripted route ends either at home or when enemy fire
/// destroys the craft. A bot never takes the craft the human is heading for.
#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_pilot_sortie_while_both_teams_fly() {
    if !prepared() { return }
    let run = run_pilot(POPULATION, Flying::Both);
    if let Ending::CraftLost { .. } = run.ending { return }
    completed_pilot_sortie(&run, false);
}

fn delivered_marine(run: &MarineRun, population: bool) {
    assert!(run.boarded < 3600 && run.landed.is_some(), "{:?}", run.ending);
    assert!(matches!(run.ending, Ending::Completed { tick: TEN_MINUTES, .. }), "{:?}", run.ending);
    if population {
        assert_eq!(run.rosters, [31, 32]);
    }
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_marine_delivery_alone() {
    if !prepared() { return }
    let run = run_marine(0, Flying::Both);
    delivered_marine(&run, false);
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_marine_delivery_among_mission_bots_without_pilots() {
    if !prepared() { return }
    let run = run_marine(POPULATION, Flying::Nobody);
    delivered_marine(&run, true);
    assert_eq!(run.bot_airborne_ticks, 0);
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_marine_delivery_while_its_own_team_flies() {
    if !prepared() { return }
    let run = run_marine(POPULATION, Flying::OwnTeam);
    delivered_marine(&run, true);
    assert!(run.peak_friendly_aloft > 0 && run.bot_airborne_ticks > 0);
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_marine_delivery_while_both_teams_fly() {
    if !prepared() { return }
    let run = run_marine(POPULATION, Flying::Both);
    if let Ending::CraftLost { .. } | Ending::NoTransport { .. } | Ending::NotDelivered { .. } = run.ending { return }
    delivered_marine(&run, true);
}
