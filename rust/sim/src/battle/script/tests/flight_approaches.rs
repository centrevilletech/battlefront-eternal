//! What a player's fighter meets on its way to the enemy capital ship, by
//! source, in the prepared Space Assault mission with the mission's bots and
//! their pilots. A scripted human boards a parked fighter of its side, waits
//! on its deck while the battle warms up, takes off and is set far out on one
//! of eight lines through the enemy ship's centre (both ends of its long axis,
//! both sides, above, below and two diagonals of the ship's own frame). From
//! there it flies at the centre at full throttle, straight or weaving around
//! the line, without firing. Every round any gun or craft fires at it and every
//! hit it takes is counted by where it came from.
//!
//! `BFE_SWBF2_PREPARED=<prepared root> cargo test --release -p sim --lib approaches -- --ignored --nocapture`
//!
//! Optional: `BFE_APPROACH_SEEDS` (how many seeds from 1, default 10),
//! `BFE_APPROACH_TEAMS` (default "1,2"), `BFE_APPROACH_LINES` (default
//! "0,1,2,3,4,5,6,7"), `BFE_APPROACH_MODES` (default "straight,weave"),
//! `BFE_APPROACH_THREADS` (default 8), `BFE_APPROACH_WARM` (ticks on the deck
//! before take-off, default 5400), `BFE_APPROACH_START` (metres from the ship's
//! centre at the start, default 1500, kept 150 m inside the battlefield).
//!
//! Lines printed: `APPROACH` (one a run) and `APPROACH_SOURCE` (one a source
//! that fired at the player or hit it in that run).
//!
//! A second test runs the same mission with nobody flying for the human,
//! `BFE_CENSUS_SEEDS` battles (default 24, seeds from 1001) of
//! `BFE_CENSUS_TICKS` (default 36000): the score at every minute, the craft
//! of each team aloft each minute, and every craft lost by what destroyed it
//! (`CENSUS`, `CENSUS_MINUTE`, `CENSUS_LOSS`, `CENSUS_LAUNCHES`, `CENSUS_SYSTEMS`,
//! `CENSUS_EARLY` lines).

use bfe_wide::Wide;
use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use crate::gen::{F4LiveInput, F4TailInput, WorldClockState, WorldTickInput};
use crate::space::{
    combat::EmittingBody,
    flight::{SpaceFighter, SpaceFlightState, SpaceOrdnanceKind, SpacePilotInput},
    objects::SpaceTurretPilotType,
    SpaceEvent, SpaceImpactCause, SpaceImpactTarget, SPACE_FIXED_DT,
};
use crate::world::admit::{self, mission::*, Bots, Options};
use crate::world::owner::World;

const POPULATION: u32 = 32;
const LINES: [&str; 8] = ["bow", "stern", "starboard", "port", "above", "below", "bow-starboard-above", "stern-port-below"];
/// A round counts as fired at the player when the player is the craft of its
/// team it passes nearest, within this.
const AT_PLAYER_M: f64 = 200.;
/// The run ends as arrived when the ship lies this close ahead.
const ARRIVED_M: f64 = 120.;
const FLIGHT_LIMIT_TICKS: u32 = 120 * 60;

fn env_list(name: &str, fallback: &str) -> Vec<String> {
    std::env::var(name).unwrap_or_else(|_| fallback.into()).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn env_num<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(fallback)
}

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

struct Sim {
    world: Box<World>,
    admitted: admit::Admitted,
    clock: WorldClockState,
    tick: u32,
}

impl Sim {
    fn start(side: &str, class: Option<&str>, seed: u32) -> Result<Self, String> {
        let files = bfe_content::files::DirFiles::from_resolved_root(prepared_root(), bfe_family::GameFamily::Swbf2);
        let options = Options {
            family: GameFamily::Swbf2, mode: "ass".into(), practice_target: None, map: "spa6".into(),
            side: side.into(), unit: class.map(Into::into), seed, heroes: false, enhanced: false,
            name: None, bots: Bots::default(),
        };
        let selection = MissionSelection {
            family: GameFamily::Swbf2, map: "spa6".into(), era: "cw".into(), mode: "ass".into(), seed,
        };
        let mut world = World::new();
        let preferences = MissionPreferences { bots_per_team: Some(POPULATION), direct_spawn: true, ..Default::default() };
        let (admitted, _, _) = admit_developer_mission(&mut world, &files, &options, &selection, preferences)
            .map_err(|e| format!("{e:?}"))?;
        world.step_init(&admitted.setup);
        Ok(Sim {
            world, admitted, tick: 0,
            clock: WorldClockState { hz: 60., max_steps: 4., max_debt: 8., budget: f64::INFINITY, ..Default::default() },
        })
    }

    fn advance(&mut self) {
        self.tick = self.tick.saturating_add(1);
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
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    v8math::hypot3(a[0], a[1], a[2])
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = length(a);
    if l > 1e-9 { scale(a, 1. / l) } else { [0., 0., 1.] }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The orientation whose nose points along `d`, level.
fn facing(d: [f64; 3]) -> [f64; 4] {
    let d = unit(d);
    let yaw = v8math::atan2(d[0], d[2]);
    let pitch = -v8math::asin(d[1].clamp(-1., 1.));
    let (sy, cy) = (v8math::sin(yaw * 0.5), v8math::cos(yaw * 0.5));
    let (sp, cp) = (v8math::sin(pitch * 0.5), v8math::cos(pitch * 0.5));
    [cy * sp, sy * cp, -sy * sp, cy * cp]
}

fn steer(f: &SpaceFighter, p: [f64; 3]) -> SpacePilotInput {
    let v = unit(sub(p, f.position));
    let z = dot(f.forward(), v);
    SpacePilotInput {
        throttle: 1.,
        stick_x: (-v8math::atan2(dot(f.right(), v), z) * 2.5).clamp(-1., 1.),
        stick_y: (v8math::asin(dot(f.up(), v).clamp(-1., 1.)) * 2.5).clamp(-1., 1.),
        ..Default::default()
    }
}

/// How near a round passes a craft flying on as it flies now: the closest
/// approach within the round's life, or for a beam its distance from the beam.
fn miss(origin: [f64; 3], direction: [f64; 3], speed: f64, life: f64, beam: bool, at: [f64; 3], velocity: [f64; 3]) -> f64 {
    let r0 = sub(origin, at);
    if beam || speed <= 0. {
        let s = (-dot(r0, direction)).clamp(0., 6000.);
        return length(add(r0, scale(direction, s)));
    }
    let rv = sub(scale(direction, speed), velocity);
    let vv = dot(rv, rv);
    let t = if vv > 1e-9 { (-dot(r0, rv) / vv).clamp(0., life) } else { 0. };
    length(add(r0, scale(rv, t)))
}

#[derive(Default, Clone)]
struct Source {
    kind: String,
    rounds: u32,
    rounds_elsewhere: u32,
    hits: u32,
    splash: u32,
    hull: f64,
    shield: f64,
    first_range: Option<f64>,
    first_tick: u32,
    last_tick: u32,
    speed: f64,
    miss_sum: f64,
}

/// What fired: a name, a kind, its round speed, life and whether it is a beam,
/// and the team it fired for.
fn shooter(world: &World, fighter: Option<usize>, building: Option<usize>, weapon: u8) -> Option<(String, String, f64, f64, bool, i32)> {
    let space = world.space()?;
    if let Some(i) = fighter {
        let f = space.fighters.get(i)?;
        let p = &space.profiles[f.profile_index];
        let w = p.weapons.get(usize::from(weapon))?;
        let kind = if w.seat == 0 { "craft" } else { "gunner" };
        let crew = f.occupant(w.seat).map_or("empty", |o| if o.is_human { "human" } else { "bot" });
        return Some((format!("{}:{}", p.class_name, w.name), format!("{kind}:{crew}"), w.velocity, w.life_span,
            w.ordnance_kind == SpaceOrdnanceKind::Beam, f.team));
    }
    let b = building?;
    let a = space.buildings.iter().find(|a| a.building_index == b)?;
    let t = a.turret.as_ref()?;
    let team = world.space_building_snapshot(b).map_or(0, |s| s.team);
    let (kind, team) = match t.pilot_type {
        SpaceTurretPilotType::SelfPiloted => (format!("auto:{}", a.class), team),
        SpaceTurretPilotType::Remote => match t.remote_operator {
            Some(o) => (format!("chair:{}:{}", if o.is_human { "human" } else { "bot" }, a.class), o.team),
            None => (format!("chair:empty:{}", a.class), team),
        },
        SpaceTurretPilotType::Player => (format!("seat:{}", a.class), team),
    };
    Some((a.name.clone(), kind, t.weapon.velocity, t.weapon.life_span, t.weapon.ordnance_kind == SpaceOrdnanceKind::Beam, team))
}

fn hitter(world: &World, emitter: Option<EmittingBody>, cause: SpaceImpactCause) -> (String, String) {
    let Some(space) = world.space() else { return ("?".into(), "?".into()) };
    match (emitter, cause) {
        (Some(EmittingBody::Fighter { index, .. }), SpaceImpactCause::Round { weapon }) => {
            shooter(world, Some(index), None, weapon).map_or(("craft".into(), "craft".into()), |s| (s.0, s.1))
        }
        (Some(EmittingBody::Fighter { index, .. }), _) => space.fighters.get(index).map_or(("craft".into(), "craft".into()), |f| {
            (format!("{}:blast", space.profiles[f.profile_index].class_name), "craft:blast".into())
        }),
        (Some(EmittingBody::Building { index, .. }), _) => {
            shooter(world, None, Some(index), 0).map_or_else(|| {
                let name = space.buildings.iter().find(|a| a.building_index == index).map_or(format!("object {index}"), |a| a.name.clone());
                (name, "object".into())
            }, |s| (s.0, s.1))
        }
        (None, SpaceImpactCause::Contact { .. }) => ("contact".into(), "collision".into()),
        (None, c) => (format!("{c:?}"), "other".into()),
    }
}

/// One approach. Returns the printed lines.
fn approach(team: i32, line: usize, weave: bool, seed: u32, warm: u32, start: f64) -> Vec<String> {
    let (side, class, craft, ship) = if team == 1 {
        ("rep", "rep_inf_ep3_pilot", "rep_fly_vwing", "cis_cap1_body1")
    } else {
        ("cis", "cis_inf_pilot", "cis_fly_droidfighter_sc", "rep_cap_body1")
    };
    let tag = format!("team={team} line={} mode={} seed={seed}", LINES[line], if weave { "weave" } else { "straight" });
    let mut s = match Sim::start(side, Some(class), seed) {
        Ok(s) => s,
        Err(e) => return vec![format!("APPROACH {tag} ended=no_battle error={e}")],
    };
    s.advance();
    let Some(key) = s.world.space_player_unit(s.admitted.handles.player) else {
        return vec![format!("APPROACH {tag} ended=no_player")];
    };
    // Board the nearest parked fighter of the class, walking to it, while the
    // bots of the human's team are held on foot so that one is left for it.
    let own_bots = |w: &World| -> Vec<Ref<Unit>> {
        (0..w.sim.store.unit.slots())
            .filter_map(|slot| w.sim.store.unit.at(slot))
            .filter(|(_, u)| !u.human && u.team == f64::from(team))
            .map(|(r, _)| r)
            .collect()
    };
    for unit in own_bots(&s.world) {
        s.world.space_pilot_hold(unit, true);
    }
    let mut seat = None;
    let mut tried = Vec::new();
    while seat.is_none() && s.tick < 4800 {
        let from = s.world.space_unit_position(key.reference()).unwrap_or_default();
        let pick = {
            let space = s.world.space().unwrap();
            space.fighters.iter()
                .filter(|f| f.alive && f.team == team && f.state == SpaceFlightState::Landed && f.pilot.is_none()
                    && !tried.contains(&f.id) && !space.profiles[f.profile_index].class_name.contains("gunship"))
                .min_by(|a, b| {
                    let rank = |f: &SpaceFighter| (u8::from(space.profiles[f.profile_index].class_name != craft), length(sub(f.position, from)));
                    let (ra, rb) = (rank(a), rank(b));
                    ra.0.cmp(&rb.0).then(ra.1.total_cmp(&rb.1))
                })
                .map(|f| (f.id, f.position))
        };
        let Some((id, at)) = pick else { break };
        let began = s.tick;
        while s.tick < began + 1500 {
            if let Some(got) = s.world.space_board_seat(&key, id, 0) {
                seat = Some(got);
                break;
            }
            for unit in own_bots(&s.world) {
                s.world.space_pilot_hold(unit, true);
            }
            let p = s.world.space_unit_position(key.reference()).unwrap_or(at);
            let yaw = v8math::atan2(at[0] - p[0], at[2] - p[2]);
            let _ = s.world.space_drive_unit(key.reference(), Some(yaw), 1., 0., false, None);
            s.advance();
            if s.world.space().unwrap().fighters[id].pilot.is_some_and(|o| !o.is_human) {
                break;
            }
        }
        tried.push(id);
    }
    for unit in own_bots(&s.world) {
        s.world.space_pilot_hold(unit, false);
    }
    let Some(seat) = seat else {
        return vec![format!("APPROACH {tag} ended=no_craft tick={}", s.tick)];
    };
    let id = seat.fighter;
    let flown_class = {
        let space = s.world.space().unwrap();
        space.profiles[space.fighters[id].profile_index].class_name.clone()
    };
    while s.tick < warm {
        s.world.space_control_seat(seat, SpacePilotInput::default());
        s.advance();
    }
    let mut asked = false;
    while s.world.space().unwrap().fighters[id].state != SpaceFlightState::Flying {
        let f = &s.world.space().unwrap().fighters[id];
        if !f.alive || s.tick > warm + 1200 {
            return vec![format!("APPROACH {tag} ended=no_takeoff tick={}", s.tick)];
        }
        s.world.space_control_seat(seat, SpacePilotInput { takeoff_or_land: !asked, ..Default::default() });
        asked = true;
        s.advance();
    }
    // The enemy ship's frame and the line.
    let (center, axes, matrix_ok) = {
        let space = s.world.space().unwrap();
        let Some(a) = space.buildings.iter().find(|a| a.name == ship) else {
            return vec![format!("APPROACH {tag} ended=no_ship")];
        };
        let m = a.matrix;
        let x = unit([m[0], m[1], m[2]]);
        let y = unit([m[4], m[5], m[6]]);
        let z = unit([m[8], m[9], m[10]]);
        (a.center, [x, y, z], (dot(x, y).abs() < 1e-3) as u8)
    };
    let [x, y, z] = axes;
    let dirs = [z, scale(z, -1.), x, scale(x, -1.), y, scale(y, -1.), unit(add(add(z, x), y)), unit(scale(add(add(z, x), y), -1.))];
    let out = dirs[line];
    let mut distance = start;
    {
        let bounds = &s.world.space().unwrap().bounds;
        while distance > 400. && bounds.check_boundary(add(center, scale(out, distance))).signed_margin < 150. {
            distance -= 25.;
        }
    }
    let begin = add(center, scale(out, distance));
    let nose = scale(out, -1.);
    let (profile_speed, max_health) = {
        let space = s.world.space_mut().unwrap();
        let f = &mut space.fighters[id];
        let p = &space.profiles[f.profile_index];
        let speed = p.max_speed;
        f.position = begin;
        f.previous_position = begin;
        f.quaternion = facing(nose);
        f.previous_quaternion = f.quaternion;
        f.airspeed = speed;
        f.velocity = scale(nose, speed);
        f.yaw_rate = 0.;
        f.pitch_rate = 0.;
        f.roll_rate = 0.;
        f.recompute_matrix();
        assert!(dot(f.forward(), nose) > 0.999, "facing {:?} for {:?}", f.forward(), nose);
        (speed, f.max_health)
    };
    let side_a = unit(cross(nose, if nose[1].abs() > 0.9 { x } else { [0., 1., 0.] }));
    let side_b = cross(nose, side_a);
    let started = s.tick;
    let trace = std::env::var_os("BFE_APPROACH_TRACE").is_some();
    let mut sources: BTreeMap<String, Source> = BTreeMap::new();
    let mut flown = 0.;
    let mut last = begin;
    let mut ending = "timeout";
    let mut hull_lost = 0.;
    let mut shield_lost = 0.;
    let mut ahead = f64::INFINITY;
    let mut contacts_seen = s.world.space().unwrap().fighters[id].contacts;
    loop {
        let (alive, position) = {
            let f = &s.world.space().unwrap().fighters[id];
            (f.alive && f.generation == seat.generation, f.position)
        };
        if !alive {
            ending = "destroyed";
            break;
        }
        let touching = {
            let space = s.world.space().unwrap();
            let f = &space.fighters[id];
            let fresh = f.contacts != contacts_seen;
            contacts_seen = f.contacts;
            match space.body_owner(f.contact_body) {
                crate::space::SpaceBodyOwner::Building(b) if fresh => {
                    s.world.space_building_snapshot(b).is_some_and(|o| o.team != team)
                }
                _ => false,
            }
        };
        if touching {
            ending = "arrived";
            break;
        }
        flown += length(sub(position, last));
        last = position;
        if s.tick.saturating_sub(started) >= FLIGHT_LIMIT_TICKS {
            break;
        }
        if s.tick.saturating_sub(started).is_multiple_of(6) {
            let toward = unit(sub(center, position));
            let ray = crate::collision::Ray { ox: position[0], oy: position[1], oz: position[2], dx: toward[0], dy: toward[1], dz: toward[2] };
            let own = s.world.space().unwrap().fighters[id].kernel_body.map_or(-1, |b| b as i32);
            ahead = s.world.space_ray_hit(ray, 4000., own)
                .filter(|(code, _)| *code != crate::collision::MISS)
                .map_or(f64::INFINITY, |(_, h)| h.distance);
            if ahead < ARRIVED_M {
                ending = "arrived";
                break;
            }
        }
        if trace && s.tick.saturating_sub(started).is_multiple_of(60) {
            let f = &s.world.space().unwrap().fighters[id];
            println!("APPROACH_TRACE {tag} t={} state={:?} airspeed={:.1} speed={:.1} pos={:?} fwd={:?} health={:.0} oob={} contacts={} body={} margin={:.0}",
                s.tick.saturating_sub(started), f.state, f.airspeed, length(f.velocity), position.map(|v| v.round()), f.forward().map(|v| (v * 100.).round() / 100.), f.health, f.oob_warning,
                f.contacts, format!("{}:{:?}", f.contact_body, s.world.space().unwrap().body_owner(f.contact_body)), s.world.space().unwrap().bounds.check_boundary(position).signed_margin);
        }
        let input = {
            let f = &s.world.space().unwrap().fighters[id];
            let goal = if weave {
                let t = f64::from(s.tick.saturating_sub(started)) * SPACE_FIXED_DT;
                let w1 = std::f64::consts::TAU / 6.;
                let w2 = std::f64::consts::TAU / 8.;
                add(center, add(scale(side_a, 140. * v8math::sin(w1 * t)), scale(side_b, 140. * v8math::sin(w2 * t + 1.))))
            } else {
                center
            };
            steer(f, goal)
        };
        s.world.space_control_seat(seat, input);
        let (before_at, before_velocity) = {
            let f = &s.world.space().unwrap().fighters[id];
            (f.position, f.velocity)
        };
        s.advance();
        let now = s.tick;
        let world = &*s.world;
        let space = world.space().unwrap();
        // Rounds fired this tick: at the player, or at another craft of its team.
        let friends: Vec<([f64; 3], [f64; 3], usize)> = space.fighters.iter()
            .filter(|f| f.alive && f.team == team && f.state == SpaceFlightState::Flying)
            .map(|f| (f.position, f.velocity, f.id))
            .collect();
        for event in space.events() {
            let SpaceEvent::WeaponFired { fighter, building, weapon_index, origin, direction, count, .. } = *event else { continue };
            let Some((name, kind, speed, life, beam, shooter_team)) = shooter(world, fighter, building, weapon_index) else { continue };
            if shooter_team == team || shooter_team == 0 {
                continue;
            }
            let mut best = (f64::INFINITY, usize::MAX);
            for &(at, velocity, fid) in &friends {
                let (at, velocity) = if fid == id { (before_at, before_velocity) } else { (at, velocity) };
                let m = miss(origin, direction, speed, life, beam, at, velocity);
                if m < best.0 {
                    best = (m, fid);
                }
            }
            let entry = sources.entry(name).or_default();
            entry.kind = kind;
            entry.speed = if beam { 0. } else { speed };
            if best.1 == id && best.0 < AT_PLAYER_M {
                entry.rounds = entry.rounds.saturating_add(count);
                entry.miss_sum += best.0 * f64::from(count);
                if entry.first_range.is_none() {
                    entry.first_range = Some(length(sub(origin, before_at)));
                    entry.first_tick = now;
                }
                entry.last_tick = now;
            } else if length(sub(origin, before_at)) < 1500. {
                entry.rounds_elsewhere = entry.rounds_elsewhere.saturating_add(count);
            }
        }
        for i in &space.impacts {
            let SpaceImpactTarget::Fighter { index, .. } = i.target else { continue };
            if index != id {
                continue;
            }
            let (name, kind) = hitter(world, i.emitter, i.cause);
            let entry = sources.entry(name).or_default();
            if entry.kind.is_empty() {
                entry.kind = kind;
            }
            match i.cause {
                SpaceImpactCause::Round { .. } => entry.hits = entry.hits.saturating_add(1),
                _ => entry.splash = entry.splash.saturating_add(1),
            }
            entry.hull += i.hull_loss;
            entry.shield += i.shield_loss;
            hull_lost += i.hull_loss;
            shield_lost += i.shield_loss;
        }
    }
    let seconds = f64::from(s.tick.saturating_sub(started)) * SPACE_FIXED_DT;
    let at = s.world.space().unwrap().fighters[id].position;
    let mut lines = vec![format!(
        "APPROACH {tag} craft={flown_class} ended={ending} seconds={seconds:.2} flown_m={flown:.0} start_m={distance:.0} end_center_m={:.0} end_ahead_m={:.0} hull_lost={hull_lost:.0} shield_lost={shield_lost:.0} max_health={max_health:.0} speed={profile_speed:.0} frame_ok={matrix_ok}",
        length(sub(at, center)), if ahead.is_finite() { ahead } else { -1. },
    )];
    for (name, src) in &sources {
        if src.rounds == 0 && src.hits == 0 && src.splash == 0 {
            continue;
        }
        let window = f64::from(src.last_tick.saturating_sub(src.first_tick) + 1) * SPACE_FIXED_DT;
        lines.push(format!(
            "APPROACH_SOURCE {tag} source={name} kind={} rounds={} rounds_elsewhere={} hits={} splash={} hull={:.0} shield={:.0} first_range_m={:.0} first_s={:.2} window_s={window:.2} speed={:.0} miss_m={:.1}",
            src.kind, src.rounds, src.rounds_elsewhere, src.hits, src.splash, src.hull, src.shield,
            src.first_range.unwrap_or(-1.), if src.rounds > 0 { f64::from(src.first_tick - started) * SPACE_FIXED_DT } else { -1. },
            src.speed, if src.rounds > 0 { src.miss_sum / f64::from(src.rounds) } else { -1. },
        ));
    }
    lines
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_player_approaches_to_the_enemy_ship() {
    if std::env::var_os("BFE_SWBF2_PREPARED").is_none() {
        return;
    }
    let seeds: u32 = env_num("BFE_APPROACH_SEEDS", 10);
    let warm: u32 = env_num("BFE_APPROACH_WARM", 5400);
    let start: f64 = env_num("BFE_APPROACH_START", 1500.);
    let threads: usize = env_num("BFE_APPROACH_THREADS", 8);
    let teams: Vec<i32> = env_list("BFE_APPROACH_TEAMS", "1,2").iter().filter_map(|s| s.parse().ok()).collect();
    let lines: Vec<usize> = env_list("BFE_APPROACH_LINES", "0,1,2,3,4,5,6,7").iter().filter_map(|s| s.parse().ok()).filter(|l| *l < LINES.len()).collect();
    let modes: Vec<bool> = env_list("BFE_APPROACH_MODES", "straight,weave").iter().map(|m| m == "weave").collect();
    let mut jobs = Vec::new();
    for seed in 1..=seeds {
        for &team in &teams {
            for &line in &lines {
                for &weave in &modes {
                    jobs.push((team, line, weave, seed));
                }
            }
        }
    }
    let queue = Mutex::new(jobs);
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| loop {
                let Some((team, line, weave, seed)) = queue.lock().unwrap().pop() else { break };
                let printed = std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn(move || approach(team, line, weave, seed, warm, start))
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| vec![format!("APPROACH team={team} line={} seed={seed} ended=panic", LINES[line])]);
                println!("{}", printed.join("\n"));
            });
        }
    });
}

/// What destroyed a craft, for the census.
fn loss_kind(world: &World, emitter: Option<EmittingBody>, cause: SpaceImpactCause) -> String {
    match (emitter, cause) {
        (_, SpaceImpactCause::Contact { .. }) => "collision".into(),
        (_, SpaceImpactCause::Flight) => "flight".into(),
        (Some(EmittingBody::Building { index, .. }), _) => {
            shooter(world, None, Some(index), 0).map_or("object".into(), |s| s.1.split(':').take(2).collect::<Vec<_>>().join(":"))
        }
        (Some(EmittingBody::Fighter { index, .. }), SpaceImpactCause::Round { weapon }) => {
            shooter(world, Some(index), None, weapon).map_or("craft".into(), |s| format!("{}:{}", s.1, s.0.split(':').nth(1).unwrap_or("?")))
        }
        (Some(EmittingBody::Fighter { .. }), _) => "craft:blast".into(),
        (None, c) => format!("{c:?}"),
    }
}

fn census(seed: u32, ticks: u32) -> Vec<String> {
    let mut s = match Sim::start("rep", None, seed) {
        Ok(s) => s,
        Err(e) => return vec![format!("CENSUS seed={seed} error={e}")],
    };
    let mut lines = Vec::new();
    let mut aloft = [0u32; 3];
    let mut chairs = [0u32; 3];
    let mut samples = 0u32;
    let mut losses: BTreeMap<(i32, String), u32> = BTreeMap::new();
    // A craft's opening 30 s after its launch: by team and source, hits, damage,
    // the sum of ranges at the hit, and craft lost.
    let mut early: BTreeMap<(i32, String), (u32, f64, f64, u32)> = BTreeMap::new();
    let mut launched: Vec<Option<(u32, u32)>> = Vec::new();
    let mut landed: Vec<bool> = Vec::new();
    let mut launches = [0u32; 3];
    // Ship systems (lock-on objects) each team lost.
    let mut systems_lost = [0u32; 3];
    let mut winner = None;
    while s.tick < ticks {
        s.advance();
        let world = &*s.world;
        let space = world.space().unwrap();
        if launched.len() < space.fighters.len() {
            launched.resize(space.fighters.len(), None);
            landed.resize(space.fighters.len(), true);
        }
        for f in &space.fighters {
            let up = f.alive && f.state != SpaceFlightState::Landed;
            if up && landed[f.id] {
                launched[f.id] = Some((f.generation, s.tick));
                launches[f.team.clamp(0, 2) as usize] += 1;
            }
            landed[f.id] = !up;
        }
        for event in space.events() {
            if let SpaceEvent::BuildingDestroyed { building, team, .. } = *event {
                if space.buildings.iter().any(|a| a.building_index == building && a.lock_on_enabled) {
                    systems_lost[team.clamp(0, 2) as usize] += 1;
                }
            }
        }
        for i in &space.impacts {
            let SpaceImpactTarget::Fighter { index, .. } = i.target else { continue };
            let Some(f) = space.fighters.get(index) else { continue };
            let young = launched.get(index).copied().flatten().is_some_and(|(g, t)| g == f.generation && s.tick.saturating_sub(t) <= 1800);
            if young {
                let (name, kind) = hitter(world, i.emitter, i.cause);
                let from = match i.emitter {
                    Some(EmittingBody::Building { index: b, .. }) => space.buildings.iter().find(|a| a.building_index == b).map(|a| a.center),
                    Some(EmittingBody::Fighter { index: e, .. }) => space.fighters.get(e).map(|o| o.position),
                    None => None,
                };
                let key = if kind.starts_with("auto") { format!("{kind}:{name}") } else { kind };
                let e = early.entry((f.team, key)).or_default();
                e.0 = e.0.saturating_add(1);
                e.1 += i.hull_loss + i.shield_loss;
                e.2 += from.map_or(0., |p| length(sub(p, f.position)));
                e.3 = e.3.saturating_add(u32::from(i.destroyed));
            }
            if !i.destroyed {
                continue;
            }
            let team = f.team;
            let lost = losses.entry((team, loss_kind(world, i.emitter, i.cause))).or_default();
            *lost = lost.saturating_add(1);
        }
        if s.tick.is_multiple_of(60) {
            samples += 1;
            for f in space.fighters.iter().filter(|f| f.alive && f.state != SpaceFlightState::Landed) {
                aloft[f.team.clamp(0, 2) as usize] += 1;
            }
            for t in space.buildings.iter().filter_map(|a| a.terminal.as_ref()).filter_map(|t| t.occupant) {
                chairs[t.team.clamp(0, 2) as usize] += 1;
            }
        }
        let points = world.event_rules().map_or([0; 3], |r| { let p = r.team_points(); [p[0], p[1], *p.get(2).unwrap_or(&0)] });
        if winner.is_none() && (points[1] >= 180 || points[2] >= 180) {
            winner = Some((if points[1] >= 180 { 1 } else { 2 }, s.tick));
        }
        if s.tick.is_multiple_of(3600) {
            let n = f64::from(samples.max(1));
            lines.push(format!("CENSUS_MINUTE seed={seed} minute={} score={},{} aloft_rep={:.2} aloft_cis={:.2} chairs_rep={:.2} chairs_cis={:.2}",
                s.tick / 3600, points[1], points[2], f64::from(aloft[1]) / n, f64::from(aloft[2]) / n, f64::from(chairs[1]) / n, f64::from(chairs[2]) / n));
            (aloft, chairs, samples) = ([0; 3], [0; 3], 0);
        }
    }
    let points = s.world.event_rules().map_or([0; 3], |r| { let p = r.team_points(); [p[0], p[1], *p.get(2).unwrap_or(&0)] });
    let (won, at) = winner.unwrap_or((if points[1] > points[2] { 1 } else if points[2] > points[1] { 2 } else { 0 }, s.tick));
    lines.insert(0, format!("CENSUS seed={seed} winner={won} at={at} score={},{} digest={:016x}", points[1], points[2],
        s.world.space_state_digest().unwrap_or(Wide::ZERO)));
    for ((team, kind), n) in losses {
        lines.push(format!("CENSUS_LOSS seed={seed} team={team} by={kind} craft={n}"));
    }
    lines.push(format!("CENSUS_LAUNCHES seed={seed} rep={} cis={}", launches[1], launches[2]));
    if let Some(space) = s.world.space() {
        for p in &space.profiles {
            lines.push(format!("CENSUS_PROFILE seed={seed} class={} aim_bound={:.2} sphere={:.2} max_speed={:.0} health={:.0}",
                p.class_name, crate::space::aim::craft_bound(p), p.sphere[3], p.max_speed, p.max_health));
        }
    }
    lines.push(format!("CENSUS_SYSTEMS seed={seed} rep_lost={} cis_lost={}", systems_lost[1], systems_lost[2]));
    for ((team, source), (hits, damage, range, lost)) in early {
        lines.push(format!("CENSUS_EARLY seed={seed} team={team} source={source} hits={hits} damage={damage:.0} range_sum={range:.0} lost={lost}"));
    }
    lines
}

#[test]
#[ignore = "requires the prepared Space Assault mission"]
fn prepared_space_battle_census() {
    if std::env::var_os("BFE_SWBF2_PREPARED").is_none() {
        return;
    }
    let seeds: u32 = env_num("BFE_CENSUS_SEEDS", 24);
    let ticks: u32 = env_num("BFE_CENSUS_TICKS", 36_000);
    let threads: usize = env_num("BFE_APPROACH_THREADS", 8);
    let queue = Mutex::new((1001..1001 + seeds).collect::<Vec<u32>>());
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| loop {
                let Some(seed) = queue.lock().unwrap().pop() else { break };
                let printed = std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn(move || census(seed, ticks))
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| vec![format!("CENSUS seed={seed} panic")]);
                println!("{}", printed.join("\n"));
            });
        }
    });
}
