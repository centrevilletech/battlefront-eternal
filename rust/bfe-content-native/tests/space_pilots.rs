//! A 2005 space battle with bot pilots, headless, on the player's prepared
//! 2005 content. Returns at once unless `BFE_SWBF2_CONTENT` (the prepared
//! content), `BFE_PILOT_MAP` and `BFE_PILOT_MISSION` (a space map and one of
//! its missions) are set.
//!
//! `BFE_SWBF2_CONTENT=... BFE_PILOT_MAP=... BFE_PILOT_MISSION=... cargo test --release -p bfe-content-native --test space_pilots -- --ignored --nocapture`
//!
//! Optional settings, read at run time:
//! - `BFE_PILOT_TICKS` (default 3600), `BFE_PILOT_SEED` (default 81) and
//!   `BFE_PILOT_BOTS` ("classic,medium" and so on).
//! - `BFE_PILOT_TEAMS`: `UNITS:Q1,Q2,...:REINFORCEMENTS` replaces the stand-in
//!   rosters with a mission's (units a team, then a quota for each loaded
//!   soldier class in roster order, then the reinforcement count).
//! - `BFE_PILOT_SETUP`: a JSON file with what a mission script sets up at load
//!   (`landing` regions, `extents`, `lockOn` objects, `shields` as
//!   `[name, current, maximum, recharge]`, `aiFloor` as `[name, share]`,
//!   `shieldLinks` as `{controller, objects}`), standing in for the mission
//!   host.
//! - `BFE_PILOT_TRACE`: a CSV file of fixed-tick rows for plotting.
//! - `BFE_PILOT_OFF`: the same battle with the pilots switched off, as a baseline.
//! - `BFE_PILOT_TIMES`: a file of per-tick milliseconds (whole tick, pilot
//!   phase; little-endian f32 pairs) for comparing replays.
//!
//! Prints `PILOT` lines counted from the battle's events and its impact log
//! (losses by cause, weapon hits, fatal contacts, returns, deliveries, trigger
//! lines, soldiers left on foot near a free seat), the worst ticks and the
//! allocations made in the pilot phase and in whole ticks after a 60-tick
//! warm-up.

use bfe_content::files::{DirFiles, Files};
use bfe_content::{json, Value};
use bfe_content_native::setup::{
    admission::{CLOCK_START_MS, CLOCK_UNIX_MS},
    TickRows,
};
use bfe_content_native::GameFamily;
use bfe_sim::abi::context::Context;
use bfe_wide::{Fnv, Wide};
use bfe_sim::space::flight::SpaceFlightState;
use bfe_sim::space::pilots::{PilotPhase, PilotRow, PilotTarget};
use bfe_sim::space::combat::EmittingBody;
use bfe_sim::space::objects::SpaceTurretPilotType;
use bfe_sim::space::{SpaceEvent, SpaceImpact, SpaceImpactCause, SpaceImpactTarget};
use bfe_sim::world::admit::{Bots, Options};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static PHASE_START: Cell<(u32, Option<Instant>)> = const { Cell::new((0, None)) };
    static PHASE_ALLOCS: Cell<u32> = const { Cell::new(0) };
    static PHASE_WORST: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    static PHASE_COUNT: Cell<bool> = const { Cell::new(false) };
    static PHASE_LAST: Cell<f64> = const { Cell::new(0.0) };
}
#[global_allocator]
static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();
fn allocations() -> u32 {
    bfe_test_alloc::thread_allocations()
}

/// The pilot phase's hook: time and allocations between its two calls.
fn probe(start: bool) {
    if start {
        PHASE_START.with(|s| s.set((allocations(), Some(Instant::now()))));
        return;
    }
    let (before, at) = PHASE_START.with(Cell::get);
    if PHASE_COUNT.with(Cell::get) {
        PHASE_ALLOCS.with(|a| a.set(a.get() + allocations().wrapping_sub(before)));
        if let Some(at) = at {
            let spent = at.elapsed();
            PHASE_WORST.with(|w| w.set(w.get().max(spent)));
            PHASE_LAST.with(|l| l.set(spent.as_secs_f64() * 1000.0));
        }
    }
}

fn env<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(fallback)
}

/// Replaces both stand-in teams with `UNITS:Q1,Q2,...:REINFORCEMENTS`.
fn with_teams(world: &Value, spec: &str) -> Value {
    let mut fields = spec.split(':');
    let units: f64 = fields.next().and_then(|s| s.parse().ok()).expect("BFE_PILOT_TEAMS units");
    let quotas: Vec<f64> = fields.next().map(|q| q.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let reinforcements: Option<f64> = fields.next().and_then(|s| s.parse().ok());
    let mut root = world.as_object().cloned().expect("world object");
    let mut mission = root.get("mission").and_then(Value::as_object).cloned().expect("mission");
    let mut teams = mission.get("teams").and_then(Value::as_object).cloned().expect("teams");
    for key in ["1", "2"] {
        let Some(team) = teams.get(key).and_then(Value::as_object).cloned() else { continue };
        let mut team = team;
        team.set(Rc::from("units"), Value::Number(units));
        if let Some(r) = reinforcements {
            team.set(Rc::from("reinforcements"), Value::Number(r));
        }
        let classes: Vec<Value> = team
            .get("classes")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, row)| match (row.at(0), quotas.get(i)) {
                (Some(name), Some(q)) => Value::Array(Rc::new(vec![name.clone(), Value::Number(*q)])),
                _ => row.clone(),
            })
            .collect();
        team.set(Rc::from("classes"), Value::Array(Rc::new(classes)));
        teams.set(Rc::from(key), Value::Object(Rc::new(team)));
    }
    mission.set(Rc::from("teams"), Value::Object(Rc::new(teams)));
    root.set(Rc::from("mission"), Value::Object(Rc::new(mission)));
    Value::Object(Rc::new(root))
}

/// What a mission script sets up at load, applied through the public world calls.
fn apply_setup(context: &Context, path: &str) -> String {
    let text = std::fs::read(path).expect("BFE_PILOT_SETUP file");
    let setup = json::parse(std::str::from_utf8(&text).expect("utf-8")).expect("setup json");
    let names = |key: &str| -> Vec<String> {
        setup.get(key).and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
    };
    let rows = |key: &str| -> Vec<Vec<Value>> {
        setup.get(key).and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(|v| v.as_array().map(|a| a.to_vec())).collect()
    };
    let landing = names("landing");
    let lock = names("lockOn");
    let shields = rows("shields");
    let floors = rows("aiFloor");
    let extents = setup.get("extents").and_then(Value::as_f64);
    let notify = setup.get("notifyRadius").and_then(Value::as_f64);
    context
        .owned(|w| {
            use bfe_sim::battle::rules::objects::ObjectProperty as P;
            let mut report = Vec::new();
            if let Some(e) = extents {
                report.push(format!("extents={}", w.space_set_world_extents(e)));
            }
            let zones = landing.iter().filter(|n| w.space_add_landing_region(n).is_some()).count();
            report.push(format!("landing={zones}/{}", landing.len()));
            let mut locked = 0;
            for n in &lock {
                if let Some(i) = w.space_building_by_name(n) {
                    locked += usize::from(w.space_set_building_property(i, P::LockOn(true)).is_ok());
                }
            }
            report.push(format!("lockOn={locked}/{}", lock.len()));
            let mut shielded = 0;
            for row in &shields {
                let (Some(name), Some(cur), Some(max), Some(add)) =
                    (row.first().and_then(Value::as_str), row.get(1).and_then(Value::as_f64), row.get(2).and_then(Value::as_f64), row.get(3).and_then(Value::as_f64))
                else {
                    continue;
                };
                if let Some(i) = w.space_building_by_name(name) {
                    let ok = w.space_set_building_property(i, P::MaxShield(max)).is_ok()
                        && w.space_set_building_property(i, P::CurrentShield(cur)).is_ok()
                        && w.space_set_building_property(i, P::AddShield(add)).is_ok();
                    shielded += usize::from(ok);
                }
            }
            report.push(format!("shields={shielded}/{}", shields.len()));
            let mut floored = 0;
            for row in &floors {
                if let (Some(name), Some(v)) = (row.first().and_then(Value::as_str), row.get(1).and_then(Value::as_f64)) {
                    if let Some(i) = w.space_building_by_name(name) {
                        floored += usize::from(w.space_set_building_property(i, P::AiDamageThreshold(v)).is_ok());
                    }
                }
            }
            report.push(format!("aiFloor={floored}/{}", floors.len()));
            // The mission's AI vehicle notify radius, as its host passes it.
            if let Some(r) = notify {
                report.push(format!("notifyRadius={}", w.space_set_pilot_notify_radius(r)));
            }
            report.join(" ")
        })
        .expect("setup")
}

#[derive(Default)]
struct Life {
    team: i32,
    born: u32,
    boarded: Option<u32>,
    launched: Option<u32>,
    first_kill: Option<u32>,
    /// On foot with a free seat in reach and a clear walk to it: when the
    /// current stretch began, and the longest stretch in ticks.
    seat_wait: Option<u32>,
    longest_wait: u32,
}

/// Rounds a craft weapon or ship gun fired and what its direct hits struck.
#[derive(Default, Debug)]
struct Shots {
    fired: u32,
    impacts: u32,
    enemy_craft: u32,
    friendly_craft: u32,
    enemy_object: u32,
    friendly_object: u32,
    neutral_object: u32,
    surface: u32,
    soldier: u32,
    /// Hull and shield damage done to enemy ship objects.
    object_hull: u32,
    object_shield: u32,
}

/// A pilot's nose at a ship object: the craft's centre, its nose, the run's
/// aim point and the weapons whose trigger is held (primary, secondary).
struct Nose {
    from: [f64; 3],
    forward: [f64; 3],
    aim: [f64; 3],
    weapons: [Option<bfe_sim::space::flight::SpaceWeaponSpec>; 2],
}

/// A trigger line to test: seat kind, from, to, stop short by, own body,
/// target bodies.
type FireLine = (&'static str, [f64; 3], [f64; 3], f64, i32, [i32; 3]);
/// A free seat in reach of a soldier on foot: life, soldier, craft position,
/// hull reach, craft body, craft.
type SeatLine = ((u32, Wide), [f64; 3], [f64; 3], f64, i32, usize);

/// What became of returns home, from their start to their end.
#[derive(Default)]
struct Returns {
    started: u32,
    landed: u32,
    died: u32,
    resumed: u32,
    open: u32,
    seconds_to_death: Vec<f64>,
    seconds_to_land: Vec<f64>,
}

/// One bout of hull contact: ticks with a contact no more than 15 apart.
#[derive(Clone, Copy, Default)]
struct Bout {
    first: u32,
    last: u32,
    ticks: u32,
    from: [f64; 3],
    to: [f64; 3],
    phase: PilotPhase,
}

#[derive(Default)]
struct Record {
    lives: BTreeMap<(u32, Wide), Life>,
    shots: BTreeMap<String, Shots>,
    /// The same for ship guns and chair guns, by kind and object class.
    guns: BTreeMap<String, Shots>,
    /// Losses by victim team and true cause.
    causes: BTreeMap<(i32, String), u32>,
    /// Ship-gun kills by victim team and gun class.
    gun_kills: BTreeMap<(i32, String), u32>,
    fatal_contacts: Vec<String>,
    contact_ticks: BTreeMap<String, u32>,
    bouts: Vec<(u32, usize, Bout)>,
    open_bouts: BTreeMap<usize, Bout>,
    systems_down: [Vec<(u32, String)>; 3],
    landings: [u32; 3],
    deliveries: Vec<String>,
    takeoffs: u32,
    damage: [(f64, f64); 3],
    /// Hull and shield damage by object name.
    object_damage: BTreeMap<String, (f64, f64)>,
    takeoffs_by_class: BTreeMap<String, u32>,
    /// Crew aboard each craft at the end of the previous tick (crew leave a
    /// transport on the tick it sets down on the enemy deck).
    last_crew: Vec<usize>,
    aloft_peak: [usize; 3],
    aloft_sum: [u32; 3],
    aloft_minutes: Vec<[f64; 2]>,
    samples: u32,
    returns: Returns,
    /// Returns in progress: (unit slot, life) to (craft, generation, start tick).
    episodes: BTreeMap<(u32, Wide), (usize, u32, u32)>,
    /// Trigger samples by seat kind: held, held with the line to the target
    /// blocked, held with no target.
    triggers: BTreeMap<&'static str, [u32; 3]>,
    blocked_by: BTreeMap<String, u32>,
    impacts_dropped: u32,
    /// The latest pilot phase seen for each craft.
    phases: BTreeMap<usize, PilotPhase>,
    /// Attacks on the other side's ship objects, by attacking team and the
    /// kind of object: a lockable system, a hull section carrying the shared
    /// shield, a gun, a frigate.
    attacks: BTreeMap<(i32, &'static str), Attacks>,
    /// The enemy object each craft's pilot is on a run against.
    on_run: BTreeMap<usize, RunNow>,
    /// The latest tick each craft's pilot was on a run against an object.
    latest_run: BTreeMap<(usize, usize), u32>,
    /// Self-piloted guns lost, by side.
    guns_down: [u32; 3],
    /// Each side's shared shield: last tick's charge, its most, every rise, the
    /// rises while no enemy craft was on a run against its systems, those
    /// ticks, and the pauses between runs.
    pools: [Pool; 3],
    /// Each craft life (craft, generation), from spawn to loss.
    craft_lives: BTreeMap<(usize, u32), CraftLife>,
}

/// How long after leaving a run a craft's hit on that object still counts as
/// the run's (its bombs may be on their way down).
const RUN_TAIL_TICKS: u32 = 600;

/// A craft's run in progress: the object and its kind, its team, and whether
/// the craft is diving on it and has released a round in this dive.
struct RunNow {
    object: usize,
    kind: &'static str,
    team: i32,
    diving: bool,
    fired: bool,
}

/// Runs on enemy ship objects of one kind and the rounds released on them.
#[derive(Default, Debug)]
struct Attacks {
    /// A craft's pilot taking an enemy ship object as its target starts a run;
    /// each time it reaches the run's start it begins a dive.
    runs: u32,
    dives: u32,
    /// Dives in which a round was released.
    dives_fired: u32,
    /// The target side's shared shield summed over run starts.
    shield_at_start: f64,
    /// Runs that ended with the craft destroyed.
    runs_lost: u32,
    run_ticks: u32,
    dive_ticks: u32,
    /// Rounds a craft released while on a run, by weapon.
    rounds: BTreeMap<String, u32>,
    /// Hits on objects of the kind, by what fired them: a pilot on a run against
    /// that object, a pilot otherwise, a gunner, a chair, a ship gun.
    hits: BTreeMap<&'static str, Hits>,
}

/// Direct hits and blasts that the shield took whole, that reached the hull,
/// and that did nothing (a damage floor, a zero scale).
#[derive(Default, Debug)]
struct Hits {
    shield: u32,
    hull: u32,
    none: u32,
    shield_damage: f64,
    hull_damage: f64,
}

#[derive(Default, Debug)]
struct Pool {
    last: f64,
    most: f64,
    rises: f64,
    rises_idle: f64,
    idle_ticks: u32,
    pauses: u32,
    busy: bool,
}

#[derive(Default, Debug)]
struct CraftLife {
    class: String,
    team: i32,
    takeoff: Option<u32>,
    lost: Option<u32>,
    cause: Option<String>,
    /// Runs flown by object kind, and the kind of the run it died on.
    ran: BTreeMap<&'static str, u32>,
    died_on: Option<&'static str>,
    rounds_on_objects: u32,
}

fn weapon_name(space: &bfe_sim::space::SpaceBattlefield, fighter: usize, weapon: u8) -> (String, u8) {
    space
        .fighters
        .get(fighter)
        .and_then(|f| space.profiles[f.profile_index].weapons.get(usize::from(weapon)))
        .map_or((format!("craft{fighter} weapon{weapon}"), 0), |w| (w.name.clone(), w.seat))
}

/// What fired: a craft's pilot or gunner weapon, a self-piloted ship gun, a
/// remote gun worked from a chair, another ship object or a soldier; and its
/// name.
fn emitter_kind(space: &bfe_sim::space::SpaceBattlefield, emitter: Option<EmittingBody>, weapon: u8) -> (&'static str, String) {
    match emitter {
        Some(EmittingBody::Fighter { index, .. }) => {
            let (name, seat) = weapon_name(space, index, weapon);
            (if seat == 0 { "craft" } else { "gunner" }, name)
        }
        Some(EmittingBody::Building { index, .. }) => match space.buildings.iter().find(|a| a.building_index == index) {
            Some(a) => match a.turret.as_ref().map(|t| t.pilot_type) {
                Some(SpaceTurretPilotType::SelfPiloted) => ("ship gun", a.class.clone()),
                Some(_) => ("chair", a.class.clone()),
                None => ("ship object", a.class.clone()),
            },
            None => ("ship object", format!("object{index}")),
        },
        None => ("soldier", String::new()),
    }
}

fn body_name(space: &bfe_sim::space::SpaceBattlefield, body: i32) -> (bool, String) {
    use bfe_sim::space::SpaceBodyOwner;
    match space.body_owner(body) {
        SpaceBodyOwner::Fighter(j) => (true, format!("craft{j}")),
        SpaceBodyOwner::Building(i) => (false, space.buildings.iter().find(|a| a.building_index == i).map_or(format!("object{i}"), |a| a.name.clone())),
        SpaceBodyOwner::Other => (false, format!("static{body}")),
    }
}

fn team_of(w: &bfe_sim::world::owner::World, target: SpaceImpactTarget) -> i32 {
    let space = w.space().expect("space");
    match target {
        SpaceImpactTarget::Fighter { index, .. } => space.fighters.get(index).map_or(0, |f| f.team),
        SpaceImpactTarget::Building { index } => w.space_building_snapshot(index).map_or(0, |s| s.team),
        _ => 0,
    }
}

/// Whether the line from `from` to within `reach` of `to` meets a body other
/// than those in `own` and `target`.
fn blocked(w: &mut bfe_sim::world::owner::World, from: [f64; 3], to: [f64; 3], reach: f64, own: i32, target: &[i32]) -> Option<String> {
    let span = d(from, to) - reach;
    if span <= 1.0 {
        return None;
    }
    let dir = [(to[0] - from[0]) / d(from, to), (to[1] - from[1]) / d(from, to), (to[2] - from[2]) / d(from, to)];
    let (body, _) = w.space_ray(from, dir, span, own)?;
    if target.contains(&body) {
        return None;
    }
    Some(body_name(w.space().expect("space"), body).1)
}

fn hull_reach(p: &bfe_sim::space::flight::SpaceFlightProfile) -> f64 {
    p.contact_bounds_max.iter().chain(p.contact_bounds_min.iter()).fold(0.0f64, |m, v| m.max(v.abs()))
}

#[test]
#[ignore = "requires prepared 2005 content"]
fn space_assault_with_bot_pilots() {
    let (Some(root), Ok(map), Ok(mission)) =
        (std::env::var_os("BFE_SWBF2_CONTENT"), std::env::var("BFE_PILOT_MAP"), std::env::var("BFE_PILOT_MISSION"))
    else {
        return;
    };
    let files = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let ticks: u32 = env("BFE_PILOT_TICKS", 3600);
    let seed: u32 = env("BFE_PILOT_SEED", 81);
    let bots: String = env("BFE_PILOT_BOTS", "classic,medium".to_string());
    let (mode, difficulty) = bots.split_once(',').unwrap_or((bots.as_str(), "medium"));
    let options = Options {
        family: GameFamily::Swbf2,
        map: map.clone(),
        mode: "ass".into(),
        practice_target: None,
        side: "rep".into(),
        enhanced: mode == "enhanced",
        seed,
        heroes: false,
        unit: None,
        name: None,
        bots: Bots::read(mode, difficulty, "full"),
    };

    let loader = bfe_content::battle::Loader::new(&files as &dyn Files);
    let query = format!("{}&mission={mission}&spawnscreen=1", options.query());
    let mut battle = bfe_content::battle::Battle::load_for_family(&loader, GameFamily::Swbf2, &map, &query).expect("load");
    if let Ok(spec) = std::env::var("BFE_PILOT_TEAMS") {
        battle.world = with_teams(&battle.world, &spec);
    }
    if let Some(teams) = battle.world.get("mission").and_then(|m| m.get("teams")) {
        for key in ["1", "2"] {
            let t = teams.get(key);
            let units = t.and_then(|t| t.get("units")).and_then(Value::as_f64).unwrap_or(0.0);
            let classes: Vec<String> = t.and_then(|t| t.get("classes")).and_then(Value::as_array).unwrap_or(&[]).iter()
                .map(|r| format!("{}x{}", r.at(0).and_then(Value::as_str).unwrap_or("?"), r.at(1).and_then(Value::as_f64).unwrap_or(0.0)))
                .collect();
            println!("PILOT team {key} units={units} classes={classes:?}");
        }
    }
    let content = bfe_content::content::Content::build(battle).expect("content");
    let context = Context::new();
    let admitted = context
        .owned(|w| bfe_sim::world::admit::sequence::run_into(w, &content, &files, &options, &mut Default::default()))
        .expect("context")
        .expect("admit");
    context.world_host_random(seed, None).expect("random");
    context.world_step_init(admitted.setup.clone()).expect("init");
    let setup = std::env::var("BFE_PILOT_SETUP").map(|p| apply_setup(&context, &p)).unwrap_or_else(|_| "none".into());
    context.owned(|w| w.space_set_pilot_probe(Some(probe))).expect("probe");
    if std::env::var_os("BFE_PILOT_OFF").is_some() {
        context.owned(|w| w.space_set_pilots_disabled(true)).expect("pilots off");
        println!("PILOT pilots switched off for a baseline");
    }
    let (exits, paths) = context
        .owned(|w| w.space_pilot_routes().map(|r| (r.exits.len(), r.exits.iter().filter(|e| e.path.1 > e.path.0).count())).unwrap_or((0, 0)))
        .expect("routes");
    println!("PILOT setup seed={seed} bots={bots} ticks={ticks} {setup} exits={exits} launch_paths={paths}");
    context
        .owned(|w| {
            let Some(r) = w.space_pilot_routes() else { return };
            for e in &r.exits {
                let first = (e.path.1 > e.path.0).then(|| r.nodes[e.path.0 as usize]);
                println!("PILOT exit team={} center={:?} outward={:?} half={:?} path_nodes={} first={first:?}", e.team, e.center, e.outward, e.half, e.path.1 - e.path.0);
            }
            if let Some(space) = w.space() {
                for f in space.fighters.iter().filter(|f| f.alive) {
                    println!("PILOT parked craft={} team={} class={} pos={:?} fwd={:?}", f.id, f.team, space.profiles[f.profile_index].class_name, f.position, f.forward());
                }
                for p in &space.profiles {
                    println!(
                        "PILOT profile craft={} speed={:.0}/{:.0}/{:.0}/{:.0} accel={:.1} turn={:.2} pitch={:.2} roll={:.2} bounds={:?}/{:?}",
                        p.class_name, p.min_speed, p.mid_speed, p.max_speed, p.boost_speed, p.acceleration, p.turn_rate, p.pitch_rate, p.roll_rate, p.contact_bounds_min, p.contact_bounds_max
                    );
                    for x in &p.weapons {
                        println!(
                            "PILOT weapon_spec craft={} weapon={} seat={} channel={} kind={:?} range={:.0}/{:.0}/{:.0} targets={}{}{} speed={:.0} life={:.2} gravity={:.1} lock={:.0}/{:.1}deg/{:.1}s guided={} damage={:.0} scales_vbs={:.2}/{:.2}/{:.2} blast={:.0}r{:.0} blast_vbs={:.2}/{:.2}/{:.2} delay={:.3} salvo={}",
                            p.class_name, x.name, x.seat, x.channel, x.ordnance_kind, x.ai_min_range, x.ai_optimal_range, x.max_range,
                            u8::from(x.ai_targets_person), u8::from(x.ai_targets_vehicle), u8::from(x.ai_targets_building), x.velocity, x.life_span, x.gravity,
                            x.lock_on_range, x.lock_on_angle_deg, x.lock_time, x.guided_missile, x.max_damage, x.vehicle_scale, x.building_scale, x.shield_scale,
                            x.blast.damage, x.blast.outer_radius, x.blast.scales[3], x.blast.scales[4], x.blast.shield_scale, x.shot_delay, x.salvo_count
                        );
                    }
                }
                let mut pads: BTreeMap<(i32, String), (u32, u32, f64)> = BTreeMap::new();
                for p in &space.pads {
                    let e = pads.entry((p.team, space.profiles[p.profile_index].class_name.clone())).or_default();
                    e.0 += 1;
                    e.1 += p.spawn_count;
                    e.2 = p.spawn_time;
                }
                for ((team, class), (n, craft, spawn)) in &pads {
                    println!("PILOT pads team={team} class={class} pads={n} craft={craft} spawn_time={spawn:.0}");
                }
                for a in space.buildings.iter().filter(|a| a.lock_on_enabled || w.space_building_snapshot(a.building_index).is_some_and(|s| s.max_shield > 0.0)) {
                    let Some(s) = w.space_building_snapshot(a.building_index) else { continue };
                    println!(
                        "PILOT object name={} team={} health={:.0}/{:.0} shield={:.0}/{:.0}+{:.0} floor={:.2} radius={:.0} center={:?} gun={} bodies={:?}",
                        a.name, s.team, s.health, s.max_health, s.cur_shield, s.max_shield, s.add_shield, a.ai_damage_threshold, a.radius, a.center, a.turret.is_some(), a.bodies
                    );
                }
            }
        })
        .expect("routes");
    let watch: Option<usize> = std::env::var("BFE_PILOT_WATCH").ok().and_then(|s| s.parse().ok());

    let mut trace = std::env::var("BFE_PILOT_TRACE").ok().map(|p| std::io::BufWriter::new(std::fs::File::create(p).expect("trace file")));
    if let Some(t) = trace.as_mut() {
        writeln!(t, "kind,tick,a,b,c,d,e,f,g,h,i").unwrap();
    }
    if let Some(t) = trace.as_mut() {
        context
            .owned(|w| {
                let space = w.space().expect("space battlefield");
                for a in &space.buildings {
                    let Some(snap) = w.space_building_snapshot(a.building_index) else { continue };
                    let gun = u8::from(a.turret.is_some());
                    writeln!(t, "G,0,b,{},{},{},{},{},{},{},{},{}", a.building_index, snap.team, f3(a.center[0]), f3(a.center[1]), f3(a.center[2]), f3(a.radius), u8::from(a.lock_on_enabled), gun, a.name).unwrap();
                }
                for f in &space.fighters {
                    writeln!(t, "G,0,f,{},{},{}", f.id, f.team, space.profiles[f.profile_index].class_name).unwrap();
                }
                if let Some(r) = w.space_pilot_routes() {
                    for e in &r.exits {
                        writeln!(t, "G,0,e,{},{},{},{},{},{},{}", e.team, f3(e.center[0]), f3(e.center[1]), f3(e.center[2]), f3(e.outward[0]), f3(e.outward[2]), f3(e.half[0])).unwrap();
                    }
                }
                for p in &space.pads {
                    writeln!(t, "G,0,p,{},{},{},{}", p.team, f3(p.position[0]), f3(p.position[1]), f3(p.position[2])).unwrap();
                }
            })
            .expect("geometry");
    }
    let mut clock = TickRows::new();
    let mut record = Record::default();
    let mut times: Vec<f64> = Vec::with_capacity(ticks as usize);
    let mut phase_times: Vec<f64> = Vec::with_capacity(ticks as usize);
    let mut links = std::env::var("BFE_PILOT_SETUP").ok().map(|p| shield_links(&p)).unwrap_or_default();
    // Seats count as in reach as far as a waiting craft lures a bot.
    let foot_reach = context.owned(|w| w.space_pilot_notify_radius()).ok().flatten().unwrap_or(bfe_sim::space::pilots::RELEASED_NOTIFY_RADIUS);
    let mut steady_allocs = 0u32;
    let mut rows: Vec<PilotRow> = Vec::new();
    let mut last_contacts: Vec<u32> = Vec::new();
    let mut digest = Fnv::new();
    for tick in 1..=ticks {
        let units = context.world_prepare_units().expect("prepare");
        let at = CLOCK_START_MS + f64::from(tick) * (1000.0 / 60.0);
        let input = clock.input(units, true, true, 0, true, 0, at, 1.0);
        PHASE_COUNT.with(|c| c.set(tick > 60));
        let before = allocations();
        let start = Instant::now();
        let output = context.world_tick(input).expect("tick");
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        phase_times.push(PHASE_LAST.with(|l| l.replace(0.0)));
        if tick > 60 {
            steady_allocs += allocations().wrapping_sub(before);
        }
        assert!(output.accepted, "tick {tick} refused");
        clock.complete(&output);
        let _ = CLOCK_UNIX_MS;
        if !links.is_empty() {
            context.owned(|w| link_shields(w, &mut links, tick)).expect("links");
        }
        context
            .owned(|w| observe(w, tick, &mut record, &mut rows, &mut last_contacts, &mut trace, foot_reach, &mut digest, watch))
            .expect("observe");
    }
    let census = context.owned(|w| w.space_pilot_census()).expect("census").expect("pilots");
    let phase_allocs = PHASE_ALLOCS.with(Cell::get);
    let phase_worst = PHASE_WORST.with(Cell::get).as_secs_f64() * 1000.0;
    let mut order: Vec<usize> = (0..times.len()).collect();
    order.sort_by(|a, b| times[*b].total_cmp(&times[*a]));
    for &i in order.iter().take(5) {
        println!("PILOT worst tick {} {:.3} ms (pilot phase {:.3} ms)", i + 1, times[i], phase_times[i]);
    }
    let mut porder: Vec<usize> = (0..phase_times.len()).collect();
    porder.sort_by(|a, b| phase_times[*b].total_cmp(&phase_times[*a]));
    for &i in porder.iter().take(5) {
        println!("PILOT worst pilot phase tick {} {:.3} ms (whole tick {:.3} ms)", i + 1, phase_times[i], times[i]);
    }
    if let Some(cost) = times.get(15_955) {
        println!("PILOT tick 15956 {cost:.3} ms (pilot phase {:.3} ms)", phase_times[15_955]);
    }
    if let Ok(path) = std::env::var("BFE_PILOT_TIMES") {
        let mut bytes = Vec::with_capacity(times.len() * 8);
        for (t, p) in times.iter().zip(&phase_times) {
            bytes.extend_from_slice(&(*t as f32).to_le_bytes());
            bytes.extend_from_slice(&(*p as f32).to_le_bytes());
        }
        std::fs::write(path, bytes).expect("times file");
    }
    // Returns still under way at the end.
    record.returns.open += u32::try_from(record.episodes.len()).unwrap_or(u32::MAX);
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let p = |q: usize| sorted[(sorted.len() - 1) * q / 100];
    let lives = record.lives.values().filter(|l| l.team > 0).count();
    let boarded: Vec<f64> = record.lives.values().filter_map(|l| l.boarded.map(|b| f64::from(b - l.born) / 60.0)).collect();
    let launched: Vec<f64> = record.lives.values().filter_map(|l| l.launched.map(|b| f64::from(b - l.born) / 60.0)).collect();
    let median = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
    };
    let never = record.lives.values().filter(|l| l.boarded.is_none()).count();
    let contact_kind = |kind: &str| record.contact_ticks.iter().filter(|(k, _)| k.starts_with(kind)).map(|(_, n)| *n).sum::<u32>();
    println!(
        "PILOT result ticks={ticks} lives={lives} boarded={} launched={} never_boarded={never} spawn_to_board_median_s={:.1} spawn_to_launch_median_s={:.1} takeoffs={} landings_home={} landings_enemy={} aloft_peak={:?} aloft_mean=[{:.1},{:.1}] ship_contact_ticks={} craft_contact_ticks={} digest={:016x}",
        boarded.len(), launched.len(), median(&boarded), median(&launched), record.takeoffs, record.landings[0], record.landings[1],
        &record.aloft_peak[1..], record.aloft_sum[1] as f64 / record.samples as f64, record.aloft_sum[2] as f64 / record.samples as f64,
        contact_kind("ship"), contact_kind("craft"), digest.finish()
    );
    for team in 1..3 {
        let causes: BTreeMap<&str, u32> = record.causes.iter().filter(|((t, _), _)| *t == team).map(|((_, c), n)| (c.as_str(), *n)).collect();
        println!("PILOT losses team={team} {causes:?}");
        let guns: BTreeMap<&str, u32> = record.gun_kills.iter().filter(|((t, _), _)| *t == team).map(|((_, c), n)| (c.as_str(), *n)).collect();
        println!("PILOT ship_gun_kills victims_team={team} {guns:?}");
    }
    println!("PILOT fatal_contacts {} {:?}", record.fatal_contacts.len(), record.fatal_contacts);
    println!("PILOT contact_ticks {:?}", record.contact_ticks);
    let mut bouts = record.bouts.clone();
    bouts.extend(record.open_bouts.iter().map(|(k, b)| (b.ticks, *k, *b)));
    bouts.sort_by_key(|b| std::cmp::Reverse(b.0));
    let longest: Vec<String> = bouts
        .iter()
        .take(3)
        .map(|(n, k, b)| format!("craft{k} {:?} ticks {}-{} contacts={n} moved={:.1}m", b.phase, b.first, b.last, d(b.from, b.to)))
        .collect();
    println!("PILOT longest_contact_bouts {longest:?}");
    let r = &record.returns;
    println!(
        "PILOT returns started={} landed={} died={} resumed={} open={} median_s_to_death={:.2} median_s_to_land={:.1}",
        r.started, r.landed, r.died, r.resumed, r.open, median(&r.seconds_to_death), median(&r.seconds_to_land)
    );
    println!("PILOT enemy_deck_deliveries {} {:?}", record.deliveries.len(), record.deliveries);
    println!("PILOT triggers held/blocked/no_target {:?} blocked_by {:?}", record.triggers, record.blocked_by);
    let mut waits: Vec<(u32, (u32, Wide))> = record.lives.iter().map(|(k, l)| (l.longest_wait, *k)).filter(|(w, _)| *w > 0).collect();
    waits.sort_by_key(|w| std::cmp::Reverse(w.0));
    let over: usize = waits.iter().filter(|(w, _)| *w > 3600).count();
    println!(
        "PILOT on_foot_with_free_reachable_seat lives_over_60s={over} longest={:?}",
        waits.iter().take(3).map(|(w, k)| format!("slot{} {:.1}s", k.0, f64::from(*w) / 60.0)).collect::<Vec<_>>()
    );
    for (name, shot) in &record.shots {
        println!("PILOT weapon {name} {shot:?}");
    }
    for (name, shot) in &record.guns {
        println!("PILOT gun {name} {shot:?}");
    }
    println!("PILOT impacts_dropped {}", record.impacts_dropped);
    println!("PILOT aloft_minutes {:?}", record.aloft_minutes.iter().map(|m| format!("{:.1}/{:.1}", m[0], m[1])).collect::<Vec<_>>());
    for ((team, kind), a) in &record.attacks {
        println!(
            "PILOT attacks team={team} kind={kind} runs={} dives={} dives_with_rounds={} runs_lost={} run_seconds={:.0} dive_seconds={:.0} shield_at_start={:.0} rounds={:?}",
            a.runs,
            a.dives,
            a.dives_fired,
            a.runs_lost,
            a.run_ticks as f64 / 60.0,
            a.dive_ticks as f64 / 60.0,
            if a.runs > 0 { a.shield_at_start / a.runs as f64 } else { 0.0 },
            a.rounds
        );
        for (source, h) in &a.hits {
            println!(
                "PILOT hits team={team} kind={kind} from={} shield={} hull={} none={} shield_damage={:.0} hull_damage={:.0}",
                source.replace(' ', "_"),
                h.shield,
                h.hull,
                h.none,
                h.shield_damage,
                h.hull_damage
            );
        }
    }
    for team in 1..3 {
        let p = &record.pools[team];
        println!(
            "PILOT shield_pool team={team} most={:.0} risen={:.0} risen_between_runs={:.0} seconds_without_a_run={:.0} pauses_between_runs={}",
            p.most, p.rises, p.rises_idle, p.idle_ticks as f64 / 60.0, p.pauses
        );
    }
    let mut classes: BTreeMap<(i32, String), Vec<&CraftLife>> = BTreeMap::new();
    for life in record.craft_lives.values().filter(|l| l.takeoff.is_some()) {
        classes.entry((life.team, life.class.clone())).or_default().push(life);
    }
    for ((team, class), lives) in &classes {
        let mut aloft: Vec<f64> = lives.iter().filter_map(|l| Some(f64::from(l.lost? - l.takeoff?) / 60.0)).collect();
        aloft.sort_by(f64::total_cmp);
        let mut causes: BTreeMap<&str, u32> = BTreeMap::new();
        for l in lives.iter().filter_map(|l| l.cause.as_deref()) {
            *causes.entry(l).or_default() += 1;
        }
        let mut ran: BTreeMap<&str, u32> = BTreeMap::new();
        let mut died_on: BTreeMap<&str, u32> = BTreeMap::new();
        for l in lives.iter() {
            for (&kind, &n) in &l.ran {
                *ran.entry(kind).or_default() += n;
            }
            *died_on.entry(l.died_on.unwrap_or(if l.ran.is_empty() { "before any run" } else { "between runs" })).or_default() += u32::from(l.lost.is_some());
        }
        println!(
            "PILOT craft_lives team={team} class={class} launched={} lost={} median_s_aloft_to_loss={:.1} lives_with_a_run={} runs={ran:?} rounds_on_objects={} lost_where={died_on:?} causes={causes:?}",
            lives.len(),
            aloft.len(),
            median(&aloft),
            lives.iter().filter(|l| !l.ran.is_empty()).count(),
            lives.iter().map(|l| l.rounds_on_objects).sum::<u32>()
        );
    }
    for team in 1..3 {
        println!("PILOT systems_lost team={team} {:?}", record.systems_down[team]);
        println!("PILOT guns_lost team={team} {}", record.guns_down[team]);
        println!("PILOT ship_damage team={team} hull={:.0} shield={:.0}", record.damage[team].0, record.damage[team].1);
        let by_object: Vec<String> = record
            .object_damage
            .iter()
            .filter(|(name, _)| name.starts_with(if team == 1 { "rep" } else { "cis" }))
            .map(|(name, (h, sh))| format!("{name}:{h:.0}/{sh:.0}"))
            .collect();
        println!("PILOT object_damage team={team} hull/shield {}", by_object.join(" "));
        let launches: Vec<String> = record
            .takeoffs_by_class
            .iter()
            .filter(|(name, _)| name.starts_with(if team == 1 { "rep" } else { "cis" }))
            .map(|(name, n)| format!("{name}:{n}"))
            .collect();
        println!("PILOT takeoffs team={team} {}", launches.join(" "));
    }
    println!("PILOT census {census:?}");
    println!(
        "PILOT cost tick_p50={:.3} tick_p99={:.3} tick_max={:.3} pilot_phase_max={phase_worst:.3} ms; allocations after tick 60: pilot_phase={phase_allocs} whole_ticks={steady_allocs} ({:.1}/tick)",
        p(50), p(99), p(100), steady_allocs as f64 / f64::from(ticks.saturating_sub(60).max(1))
    );
    if let Some(mut t) = trace {
        t.flush().unwrap();
    }
    assert_eq!(phase_allocs, 0, "the pilot phase allocated");
}

type Trace = Option<std::io::BufWriter<std::fs::File>>;

/// A target's point, the reach of its hull from that point, and its bodies.
type Aim = ([f64; 3], f64, [i32; 3]);

/// One tick's evidence: events, impacts, seats, contacts, returns, trigger
/// lines and soldiers left on foot.
#[allow(clippy::too_many_arguments)]
fn observe(
    w: &mut bfe_sim::world::owner::World,
    tick: u32,
    record: &mut Record,
    rows: &mut Vec<PilotRow>,
    last_contacts: &mut Vec<u32>,
    trace: &mut Trace,
    foot_reach: f64,
    digest: &mut Fnv,
    watch: Option<usize>,
) {
    w.space_pilot_rows(rows);
    // Lines to test once the battlefield borrow ends: seat kind, from, to,
    // stop short by, own body, target bodies.
    let mut lines: Vec<FireLine> = Vec::new();
    // Free seats in reach of soldiers on foot: life, from, to, stop short by, craft body.
    let mut seats: Vec<SeatLine> = Vec::new();
    let mut on_foot: Vec<(u32, Wide)> = Vec::new();
    {
        let space = w.space().expect("space battlefield");
        record.impacts_dropped += space.impacts_dropped;
        for r in rows.iter() {
            if let Some(v) = r.view.filter(|v| v.seat == 0 && v.phase != PilotPhase::Walk) {
                if let Some(c) = v.craft {
                    record.phases.insert(c, v.phase);
                }
            }
        }
        let known = record.phases.clone();
        let phase_of = |craft: usize| known.get(&craft).copied().unwrap_or(PilotPhase::Free);
        for row in rows.iter() {
            let life = record.lives.entry((row.slot, Wide::from_f64_bits(row.life))).or_insert_with(|| Life { team: row.team, born: tick, ..Default::default() });
            if let Some(v) = row.view {
                if v.craft.is_some() && v.phase != PilotPhase::Walk && life.boarded.is_none() {
                    life.boarded = Some(tick);
                }
                if matches!(v.phase, PilotPhase::Launch | PilotPhase::Fight | PilotPhase::Raid) && life.launched.is_none() {
                    life.launched = Some(tick);
                }
            }
        }
        // Craft lives, and runs on enemy systems from each pilot's target.
        for f in &space.fighters {
            let life = record.craft_lives.entry((f.id, f.generation)).or_insert_with(|| CraftLife {
                class: space.profiles[f.profile_index].class_name.clone(),
                team: f.team,
                ..Default::default()
            });
            if f.alive && f.state != SpaceFlightState::Landed && life.takeoff.is_none() {
                life.takeoff = Some(tick);
            }
        }
        let object_of = |index: usize| -> Option<(i32, &'static str)> {
            let a = space.buildings.iter().find(|a| a.building_index == index)?;
            let s = w.space_building_snapshot(index).filter(|s| s.alive)?;
            let kind = if a.ai_damage_threshold > 0.0 {
                "frigate"
            } else if a.lock_on_enabled {
                "system"
            } else if a.turret.is_some() {
                "gun"
            } else if s.max_shield > 0.0 {
                "hull"
            } else {
                "other"
            };
            Some((s.team, kind))
        };
        let shield_of = |team: i32| {
            space
                .buildings
                .iter()
                .filter_map(|a| w.space_building_snapshot(a.building_index))
                .filter(|s| s.team == team && s.max_shield > 0.0)
                .fold(0.0f64, |m, s| m.max(s.cur_shield))
        };
        for row in rows.iter() {
            let Some(v) = row.view.filter(|v| v.seat == 0 && v.phase != PilotPhase::Walk) else { continue };
            let Some(craft) = v.craft else { continue };
            let Some(f) = space.fighters.get(craft).filter(|f| f.alive) else { continue };
            let target = match v.target {
                PilotTarget::Building { index, .. } => object_of(index).filter(|&(t, _)| t != f.team).map(|(t, k)| (index, k, t)),
                _ => None,
            };
            let diving = v.phase == PilotPhase::Fight && v.run == 0 && v.step == 1;
            let Some((object, kind, team)) = target else {
                record.on_run.remove(&craft);
                continue;
            };
            record.latest_run.insert((craft, object), tick);
            let a = record.attacks.entry((f.team, kind)).or_default();
            if record.on_run.get(&craft).is_none_or(|r| r.object != object) {
                a.runs += 1;
                a.shield_at_start += shield_of(team);
                record.on_run.insert(craft, RunNow { object, kind, team, diving: false, fired: false });
                if let Some(life) = record.craft_lives.get_mut(&(craft, f.generation)) {
                    *life.ran.entry(kind).or_default() += 1;
                }
            }
            if let Some(r) = record.on_run.get_mut(&craft) {
                if diving && !r.diving {
                    a.dives += 1;
                    r.fired = false;
                }
                r.diving = diving;
            }
        }
        for (&craft, r) in record.on_run.iter() {
            if let Some(f) = space.fighters.get(craft) {
                let a = record.attacks.entry((f.team, r.kind)).or_default();
                a.run_ticks += 1;
                a.dive_ticks += u32::from(r.diving);
            }
        }
        let exit_team = |p: [f64; 3]| {
            w.space_pilot_routes()
                .and_then(|r| r.exits.iter().min_by(|a, b| d(a.center, p).total_cmp(&d(b.center, p))).map(|x| x.team))
                .unwrap_or(0)
        };
        let nearest_exit = |p: [f64; 3]| w.space_pilot_routes().map_or(f64::NAN, |r| r.exits.iter().map(|x| d(x.center, p)).fold(f64::INFINITY, f64::min));
        for e in space.events() {
            match *e {
                SpaceEvent::FighterTakeoff { fighter, .. } => {
                    record.takeoffs += 1;
                    if let Some(f) = space.fighters.get(fighter) {
                        *record.takeoffs_by_class.entry(space.profiles[f.profile_index].class_name.clone()).or_default() += 1;
                    }
                }
                SpaceEvent::FighterLanded { fighter, zone, .. } => {
                    let f = &space.fighters[fighter];
                    let home = space.bounds.landing_zones.get(zone).is_some_and(|z| exit_team(z.center) == f.team);
                    record.landings[usize::from(!home)] += 1;
                    if !home && space.profiles[f.profile_index].spawn_point_count > 0 {
                        let crew = record.last_crew.get(fighter).copied().unwrap_or(0).max(f.gunners.iter().flatten().count());
                        record.deliveries.push(format!("tick={tick} craft={fighter} team={} crew={crew}", f.team));
                    }
                    if let Some(t) = trace.as_mut() {
                        writeln!(t, "L,{tick},{fighter},{},{},{},{}", f.team, f3(f.position[0]), f3(f.position[2]), u8::from(home)).unwrap();
                    }
                }
                SpaceEvent::FighterDestroyed { fighter, team, attacker_unit, .. } => {
                    let f = &space.fighters[fighter];
                    let impact = space.impacts.iter().find(|i| i.destroyed && matches!(i.target, SpaceImpactTarget::Fighter { index, .. } if index == fighter));
                    let cause = match impact {
                        Some(i) => match i.cause {
                            SpaceImpactCause::Contact { body } => {
                                let (craft, name) = body_name(space, body);
                                record.fatal_contacts.push(format!(
                                    "tick={tick} craft={fighter} team={team} struck={name} opening_m={:.0} phase={:?}",
                                    nearest_exit(f.position),
                                    phase_of(fighter)
                                ));
                                (if craft { "contact with a craft" } else { "contact with a ship" }).to_string()
                            }
                            SpaceImpactCause::Flight => "flight loss".into(),
                            SpaceImpactCause::Explosion => "object explosion".into(),
                            SpaceImpactCause::Round { weapon } => cause_of(space, i, weapon, team, record),
                            SpaceImpactCause::Splash => cause_of(space, i, 0, team, record),
                        },
                        None => "unattributed".into(),
                    };
                    if let Some(o) = attacker_unit.and_then(|o| w.space_owner_facts(o)) {
                        if o.team != team {
                            if let Some(life) = record.lives.get_mut(&(o.slot, Wide::from_f64_bits(o.life))) {
                                if life.first_kill.is_none() {
                                    life.first_kill = Some(tick);
                                    if let Some(t) = trace.as_mut() {
                                        writeln!(t, "F,{tick},{},{},{},{fighter}", o.slot, Wide::from_f64_bits(o.life), o.team).unwrap();
                                    }
                                }
                            }
                        }
                    }
                    if let Some(t) = trace.as_mut() {
                        writeln!(t, "K,{tick},{fighter},{team},{},{},{},{cause}", f3(f.position[0]), f3(f.position[1]), f3(f.position[2])).unwrap();
                    }
                    let died_on = record.on_run.remove(&fighter).map(|r| r.kind);
                    if let Some(kind) = died_on {
                        record.attacks.entry((team, kind)).or_default().runs_lost += 1;
                    }
                    if let Some(life) = record.craft_lives.get_mut(&(fighter, f.generation)) {
                        life.lost = Some(tick);
                        life.cause = Some(cause.clone());
                        life.died_on = died_on;
                    }
                    *record.causes.entry((team, cause)).or_default() += 1;
                }
                SpaceEvent::BuildingDestroyed { building, team, .. } => {
                    let Some(att) = space.buildings.iter().find(|a| a.building_index == building) else { continue };
                    let side = team.clamp(0, 2) as usize;
                    if att.turret.as_ref().is_some_and(|t| t.pilot_type == SpaceTurretPilotType::SelfPiloted) {
                        record.guns_down[side] += 1;
                    }
                    if att.lock_on_enabled || att.ai_damage_threshold > 0.0 {
                        record.systems_down[side].push((tick, att.name.clone()));
                        if let Some(t) = trace.as_mut() {
                            writeln!(t, "S,{tick},{building},{team},{},{},{},{}", f3(att.center[0]), f3(att.center[1]), f3(att.center[2]), att.name).unwrap();
                        }
                    }
                }
                SpaceEvent::BuildingDamaged { building, hull_loss, shield_loss, .. } => {
                    if let Some(snap) = w.space_building_snapshot(building) {
                        let side = snap.team.clamp(0, 2) as usize;
                        record.damage[side].0 += hull_loss;
                        record.damage[side].1 += shield_loss;
                        if let Some(att) = space.buildings.iter().find(|a| a.building_index == building) {
                            let e = record.object_damage.entry(att.name.clone()).or_default();
                            e.0 += hull_loss;
                            e.1 += shield_loss;
                        }
                    }
                }
                SpaceEvent::WeaponFired { fighter: Some(fi), weapon_index, count, .. } => {
                    let name = weapon_name(space, fi, weapon_index).0;
                    if let Some(r) = record.on_run.get_mut(&fi) {
                        let f = &space.fighters[fi];
                        let a = record.attacks.entry((f.team, r.kind)).or_default();
                        *a.rounds.entry(name.clone()).or_default() += count;
                        if r.diving && !r.fired {
                            r.fired = true;
                            a.dives_fired += 1;
                        }
                        if let Some(life) = record.craft_lives.get_mut(&(fi, f.generation)) {
                            life.rounds_on_objects += count;
                        }
                    }
                    record.shots.entry(name).or_default().fired += count;
                }
                SpaceEvent::WeaponFired { fighter: None, building: Some(index), weapon_index, count, .. } => {
                    let (kind, class) = emitter_kind(space, Some(EmittingBody::Building { index, life: 0.0 }), weapon_index);
                    record.guns.entry(format!("{kind} {class}")).or_default().fired += count;
                }
                _ => {}
            }
        }
        // Hits on ship objects by the other side, by what fired them and what
        // they reached (a blast does not name its weapon: a craft's counts as
        // its pilot's).
        for i in &space.impacts {
            let weapon = match i.cause {
                SpaceImpactCause::Round { weapon } => weapon,
                SpaceImpactCause::Splash => 0,
                _ => continue,
            };
            let SpaceImpactTarget::Building { index } = i.target else { continue };
            let Some((team, kind)) = object_of(index) else { continue };
            if team == i.team || !(1..=2).contains(&i.team) {
                continue;
            }
            let source = match (emitter_kind(space, i.emitter, weapon).0, i.emitter) {
                ("craft", Some(EmittingBody::Fighter { index: craft, .. })) if record.latest_run.get(&(craft, index)).is_some_and(|t| t + RUN_TAIL_TICKS >= tick) => "run",
                (kind, _) => kind,
            };
            let h = record.attacks.entry((i.team, kind)).or_default().hits.entry(source).or_default();
            if i.hull_loss > 0.0 {
                h.hull += 1;
            } else if i.shield_loss > 0.0 {
                h.shield += 1;
            } else {
                h.none += 1;
            }
            h.shield_damage += i.shield_loss;
            h.hull_damage += i.hull_loss;
        }
        // Each side's shared shield: how far it rises, and how far while no
        // enemy craft is on a run against the side's systems.
        for team in 1..3i32 {
            let (now, most) = space
                .buildings
                .iter()
                .filter_map(|a| w.space_building_snapshot(a.building_index))
                .filter(|s| s.team == team && s.max_shield > 0.0)
                .fold((0.0f64, 0.0f64), |(n, m), s| (n.max(s.cur_shield), m.max(s.max_shield)));
            let busy = record.on_run.values().any(|r| r.team == team);
            let pool = &mut record.pools[team as usize];
            let rise = if tick > 1 { (now - pool.last).max(0.0) } else { 0.0 };
            pool.rises += rise;
            if !busy {
                pool.rises_idle += rise;
                pool.idle_ticks += 1;
                if pool.busy {
                    pool.pauses += 1;
                }
            }
            pool.busy = busy;
            pool.last = now;
            pool.most = pool.most.max(most);
        }
        // Direct hits of craft weapons and ship guns, and on whom.
        for i in &space.impacts {
            let SpaceImpactCause::Round { weapon } = i.cause else { continue };
            let s = match i.emitter {
                Some(EmittingBody::Fighter { index, .. }) => record.shots.entry(weapon_name(space, index, weapon).0).or_default(),
                Some(EmittingBody::Building { .. }) => {
                    let (kind, class) = emitter_kind(space, i.emitter, weapon);
                    record.guns.entry(format!("{kind} {class}")).or_default()
                }
                None => continue,
            };
            s.impacts += 1;
            match i.target {
                SpaceImpactTarget::Fighter { .. } => {
                    if team_of(w, i.target) == i.team {
                        s.friendly_craft += 1;
                    } else {
                        s.enemy_craft += 1;
                    }
                }
                SpaceImpactTarget::Building { .. } => match team_of(w, i.target) {
                    0 => s.neutral_object += 1,
                    t if t == i.team => s.friendly_object += 1,
                    _ => {
                        s.enemy_object += 1;
                        s.object_hull += i.hull_loss as u32;
                        s.object_shield += i.shield_loss as u32;
                    }
                },
                SpaceImpactTarget::Soldier => s.soldier += 1,
                SpaceImpactTarget::Surface { .. } => s.surface += 1,
            }
        }
        // Crew aboard each craft now, for a landing next tick.
        record.last_crew.resize(space.fighters.len(), 0);
        for f in &space.fighters {
            record.last_crew[f.id] = if f.alive { f.gunners.iter().flatten().count() } else { 0 };
        }
        // Hull contacts by the body struck, and bouts of sustained contact.
        last_contacts.resize(space.fighters.len(), 0);
        for f in &space.fighters {
            if f.alive && f.contacts != last_contacts[f.id] {
                let (craft, name) = body_name(space, f.contact_body);
                let phase = phase_of(f.id);
                *record.contact_ticks.entry(format!("{}/{phase:?}", if craft { "craft" } else { "ship" })).or_default() += 1;
                let b = record.open_bouts.entry(f.id).or_insert(Bout { first: tick, last: tick, ticks: 0, from: f.position, to: f.position, phase });
                b.last = tick;
                b.ticks += 1;
                b.to = f.position;
                if let Some(t) = trace.as_mut() {
                    writeln!(t, "X,{tick},{},{},{},{},{},{},{name},{phase:?},{:.1}", f.id, f.team, f3(f.position[0]), f3(f.position[1]), f3(f.position[2]), u8::from(craft), f.airspeed).unwrap();
                }
            }
            last_contacts[f.id] = f.contacts;
        }
        let closed: Vec<usize> = record.open_bouts.iter().filter(|(_, b)| tick > b.last + 15).map(|(k, _)| *k).collect();
        for k in closed {
            if let Some(b) = record.open_bouts.remove(&k) {
                record.bouts.push((b.ticks, k, b));
            }
        }
        // Returns home and how each ends.
        let mut ended: Vec<((u32, Wide), &'static str, u32)> = Vec::new();
        for (key, &(craft, generation, start)) in &record.episodes {
            let died = space.events().iter().any(|e| matches!(*e, SpaceEvent::FighterDestroyed { fighter, generation: g, .. } if fighter == craft && g == generation));
            let landed = space.events().iter().any(|e| matches!(*e, SpaceEvent::FighterLanded { fighter, generation: g, .. } if fighter == craft && g == generation));
            let phase = rows.iter().find(|r| (r.slot, Wide::from_f64_bits(r.life)) == *key).and_then(|r| r.view).map(|v| v.phase);
            let outcome = if died {
                Some("died")
            } else if landed || phase == Some(PilotPhase::Parked) {
                Some("landed")
            } else if phase == Some(PilotPhase::Fight) {
                Some("resumed")
            } else if phase != Some(PilotPhase::Return) {
                Some("other")
            } else {
                None
            };
            if let Some(o) = outcome {
                ended.push((*key, o, tick - start));
            }
        }
        for (key, outcome, spent) in ended {
            record.episodes.remove(&key);
            let r = &mut record.returns;
            let s = f64::from(spent) / 60.0;
            match outcome {
                "died" => {
                    r.died += 1;
                    r.seconds_to_death.push(s);
                }
                "landed" => {
                    r.landed += 1;
                    r.seconds_to_land.push(s);
                }
                "resumed" => r.resumed += 1,
                _ => r.open += 1,
            }
        }
        for row in rows.iter() {
            let Some(v) = row.view else { continue };
            let (PilotPhase::Return, Some(craft), 0) = (v.phase, v.craft, v.seat) else { continue };
            let key = (row.slot, Wide::from_f64_bits(row.life));
            if let std::collections::btree_map::Entry::Vacant(e) = record.episodes.entry(key) {
                e.insert((craft, space.fighters[craft].generation, tick));
                record.returns.started += 1;
            }
        }
        // Every quarter second: trigger lines, and soldiers on foot near a free seat.
        if tick.is_multiple_of(15) {
            let target = |t: PilotTarget| -> Option<Aim> {
                match t {
                    PilotTarget::Craft { index, .. } => space
                        .fighters
                        .get(index)
                        .filter(|f| f.alive)
                        .map(|f| (f.position, hull_reach(&space.profiles[f.profile_index]), [f.kernel_body.map_or(-1, |b| b as i32), -1, -1])),
                    PilotTarget::Building { index, .. } => space
                        .buildings
                        .iter()
                        .find(|a| a.building_index == index)
                        .map(|a| (a.center, a.radius, a.bodies.map(|b| b.map_or(-1, |b| b as i32)))),
                    PilotTarget::None => None,
                }
            };
            for r in rows.iter() {
                let Some(v) = r.view.filter(|v| v.phase != PilotPhase::Free) else {
                    on_foot.push((r.slot, Wide::from_f64_bits(r.life)));
                    continue;
                };
                // A pilot's rounds at a ship object leave along its nose and each
                // falls by its own weapon's gravity on the way to the run's aim
                // point: each held weapon's path is tested (a guided missile's
                // is the line to the aim point).
                let mut nose: Option<Nose> = None;
                let (kind, held, muzzle, own): (&'static str, bool, [f64; 3], i32) = match (v.phase, v.craft) {
                    (PilotPhase::Chair, _) => {
                        let gun = space.buildings.iter().find(|a| {
                            a.turret
                                .as_ref()
                                .and_then(|t| t.remote_operator)
                                .and_then(|op| w.space_owner_facts(op.owner))
                                .is_some_and(|o| o.slot == r.slot && o.life.to_le_bytes() == r.life.to_le_bytes())
                        });
                        let Some(gun) = gun else { continue };
                        let held = gun.turret.as_ref().is_some_and(|t| t.pending_remote_fire);
                        let toward = target(v.target).map_or([0.0, 0.0, 1.0], |(p, _, _)| unit3(sub3(p, gun.center)));
                        ("chair", held, add3(gun.center, scale3(toward, gun.radius + 1.0)), gun.bodies[0].map_or(-1, |b| b as i32))
                    }
                    (PilotPhase::Gunner, Some(c)) => {
                        let f = &space.fighters[c];
                        let input = f.seat_input(v.seat);
                        let aim = input.aim.unwrap_or_else(|| f.forward());
                        let reach = hull_reach(&space.profiles[f.profile_index]);
                        ("gunner", input.fire_primary || input.fire_secondary, add3(f.position, scale3(aim, reach + 1.0)), f.kernel_body.map_or(-1, |b| b as i32))
                    }
                    (PilotPhase::Fight | PilotPhase::Launch | PilotPhase::Return | PilotPhase::Raid, Some(c)) => {
                        let f = &space.fighters[c];
                        let input = f.seat_input(0);
                        let reach = hull_reach(&space.profiles[f.profile_index]);
                        let kind = match v.target {
                            PilotTarget::Building { .. } => {
                                let profile = &space.profiles[f.profile_index];
                                let held = |on: bool, channel: u8| profile.weapons.iter().find(|w| on && w.seat == 0 && w.channel == channel).cloned();
                                nose = Some(Nose {
                                    from: f.position,
                                    forward: f.forward(),
                                    aim: v.aim_point,
                                    weapons: [held(input.fire_primary, 0), held(input.fire_secondary, 1)],
                                });
                                "pilot on a ship object"
                            }
                            _ => "pilot on a craft",
                        };
                        (kind, input.fire_primary || input.fire_secondary, add3(f.position, scale3(f.forward(), reach + 1.0)), f.kernel_body.map_or(-1, |b| b as i32))
                    }
                    _ => continue,
                };
                if !held {
                    continue;
                }
                let counts = record.triggers.entry(kind).or_default();
                counts[0] += 1;
                match (target(v.target), nose) {
                    (None, _) => counts[2] += 1,
                    (Some((_, _, bodies)), Some(n)) => {
                        let range = d(n.from, n.aim);
                        for w in n.weapons.iter().flatten() {
                            if w.lock_on_range > 0.0 {
                                lines.push((kind, muzzle, n.aim, 10.0, own, bodies));
                            } else {
                                let fall = if w.round_speed() > 1.0 { 0.5 * w.gravity * (range / w.round_speed()).powi(2) } else { 0.0 };
                                let chord = sub3(scale3(n.forward, range), [0.0, fall, 0.0]);
                                let len = d(chord, [0.0; 3]).max(1e-9);
                                lines.push((kind, muzzle, add3(muzzle, scale3(chord, (range + 40.0) / len)), 0.0, own, bodies));
                            }
                        }
                    }
                    (Some((to, reach, bodies)), None) => lines.push((kind, muzzle, to, reach, own, bodies)),
                }
            }
            let walking: Vec<usize> = rows.iter().filter_map(|r| r.view.filter(|v| v.phase == PilotPhase::Walk && v.seat == 0).and_then(|v| v.craft)).collect();
            for key in &on_foot {
                let Some(r) = rows.iter().find(|r| (r.slot, Wide::from_f64_bits(r.life)) == *key) else { continue };
                for f in space.fighters.iter() {
                    if !f.alive || f.team != r.team || f.state != SpaceFlightState::Landed || f.pilot.is_some() || walking.contains(&f.id) {
                        continue;
                    }
                    if d(f.position, r.position) > foot_reach {
                        continue;
                    }
                    let reach = hull_reach(&space.profiles[f.profile_index]) + 1.0;
                    seats.push((*key, r.position, f.position, reach, f.kernel_body.map_or(-1, |b| b as i32), f.id));
                }
            }
        }
        // Watched craft: one by slot, or every craft of one class.
        let watch_class = std::env::var("BFE_PILOT_WATCH_CLASS").ok();
        if watch.is_some() || watch_class.is_some() {
            for f in space.fighters.iter().filter(|f| f.alive && tick.is_multiple_of(6) && f.pilot.is_some()) {
                let class = &space.profiles[f.profile_index].class_name;
                if watch != Some(f.id) && watch_class.as_deref() != Some(class.as_str()) {
                    continue;
                }
                let id = f.id;
                let i = f.pending_input;
                let fwd = f.forward();
                let pilot = rows.iter().find(|r| r.view.is_some_and(|v| v.craft == Some(id) && v.seat == 0));
                println!(
                    "PILOT watch t={tick} id={id} cls={class} gen={} st={:?} pos=({:.1},{:.1},{:.1}) fwd=({:.2},{:.2},{:.2}) v={:.1} hp={:.0} thr={:.2} sx={:.2} sy={:.2} act={} fire={}{}{} view={:?}",
                    f.generation, f.state, f.position[0], f.position[1], f.position[2], fwd[0], fwd[1], fwd[2], f.airspeed, f.health, i.throttle, i.stick_x, i.stick_y, i.takeoff_or_land,
                    u8::from(i.fire_primary), u8::from(i.fire_secondary), u8::from(i.lock_target), pilot.and_then(|r| r.view)
                );
            }
        }
        // Craft aloft and the trace rows.
        let mut aloft = [0usize; 3];
        for f in space.fighters.iter().filter(|f| f.alive && f.state != SpaceFlightState::Landed) {
            aloft[f.team.clamp(0, 2) as usize] += 1;
        }
        for (team, &n) in aloft.iter().enumerate().skip(1) {
            record.aloft_peak[team] = record.aloft_peak[team].max(n);
            record.aloft_sum[team] += u32::try_from(n).unwrap_or(u32::MAX);
        }
        record.samples += 1;
        let minute = ((tick - 1) / 3600) as usize;
        if record.aloft_minutes.len() <= minute {
            record.aloft_minutes.push([0.0; 2]);
        }
        record.aloft_minutes[minute][0] += aloft[1] as f64 / 3600.0;
        record.aloft_minutes[minute][1] += aloft[2] as f64 / 3600.0;
        if tick.is_multiple_of(600) {
            println!("PILOT digest tick={tick} space={:016x} craft={:016x}", w.space_state_digest().unwrap_or(Wide::ZERO), digest.finish());
        }
        if tick.is_multiple_of(15) {
            if let Some(t) = trace.as_mut() {
                for r in rows.iter() {
                    let (phase, craft) = r.view.map_or((String::from("Free"), -1i32), |v| (format!("{:?}", v.phase), v.craft.map_or(-1, |c| c as i32)));
                    writeln!(t, "U,{tick},{},{},{},{},{},{},{phase},{craft}", r.slot, Wide::from_f64_bits(r.life), r.team, f3(r.position[0]), f3(r.position[1]), f3(r.position[2])).unwrap();
                }
            }
        }
        if tick.is_multiple_of(30) {
            for f in space.fighters.iter().filter(|f| f.alive) {
                *digest = mix(*digest, f.position.iter().chain(f.velocity.iter()).copied());
                if let Some(t) = trace.as_mut() {
                    let pilot = rows.iter().find(|r| r.view.is_some_and(|v| v.craft == Some(f.id) && v.seat == 0 && v.phase != PilotPhase::Walk));
                    let (slot, phase, target) = pilot.map_or((-1i32, String::from("-"), String::from("-")), |r| {
                        let v = r.view.unwrap();
                        let target = match v.target {
                            PilotTarget::None => "-".into(),
                            PilotTarget::Craft { index, .. } => format!("c{index}"),
                            PilotTarget::Building { index, .. } => format!("b{index}"),
                        };
                        (i32::try_from(r.slot).unwrap_or(i32::MAX), format!("{:?}", v.phase), target)
                    });
                    writeln!(t, "C,{tick},{},{},{},{},{},{},{},{slot},{phase}:{target}", f.id, f.team, f.state as u8, f3(f.position[0]), f3(f.position[1]), f3(f.position[2]), f3(f.health)).unwrap();
                }
            }
            let alive = |team: i32| {
                space
                    .buildings
                    .iter()
                    .filter(|a| (a.lock_on_enabled || a.ai_damage_threshold > 0.0) && w.space_building_snapshot(a.building_index).is_some_and(|s| s.alive && s.team == team))
                    .count()
            };
            // Each side's shared shield charge: the most any of its shielded
            // objects holds (a linked pool holds one value).
            let shield = |team: i32| {
                space
                    .buildings
                    .iter()
                    .filter_map(|a| w.space_building_snapshot(a.building_index))
                    .filter(|s| s.team == team && s.max_shield > 0.0)
                    .fold(0.0f64, |m, s| m.max(s.cur_shield))
            };
            if let Some(t) = trace.as_mut() {
                writeln!(t, "A,{tick},{},{},{},{},{:.0},{:.0}", aloft[1], aloft[2], alive(1), alive(2), shield(1), shield(2)).unwrap();
            }
        }
    }
    for (kind, from, to, reach, own, bodies) in lines {
        if let Some(name) = blocked(w, from, to, reach, own, &bodies) {
            record.triggers.entry(kind).or_default()[1] += 1;
            *record.blocked_by.entry(name).or_default() += 1;
        }
    }
    if tick.is_multiple_of(15) {
        let mut waiting: Vec<((u32, Wide), [f64; 3], usize)> = Vec::new();
        // A seat is reachable when a soldier's waist-high walk line to the
        // craft meets nothing but the craft, as the pilots test a walk.
        for (key, from, to, _, body, craft) in seats {
            if !waiting.iter().any(|w| w.0 == key) && w.space_walk_blocker(from, to, body).is_none() {
                waiting.push((key, from, craft));
            }
        }
        let report = std::env::var_os("BFE_PILOT_WAITS").is_some();
        for (key, life) in record.lives.iter_mut() {
            if let Some(&(_, at, craft)) = waiting.iter().find(|w| w.0 == *key).filter(|_| on_foot.contains(key)) {
                let start = *life.seat_wait.get_or_insert(tick);
                let before = life.longest_wait;
                life.longest_wait = life.longest_wait.max(tick - start);
                if report && before < 3600 && life.longest_wait >= 3600 {
                    let space = w.space().expect("space");
                    let f = &space.fighters[craft];
                    let view = rows.iter().find(|r| r.slot == key.0).map(|r| (r.team, r.pilot_class, r.view));
                    println!(
                        "PILOT wait slot={} at={at:?} craft={craft} class={} team={} pos={:?} landed_zone={:?} claims_view={view:?}",
                        key.0, space.profiles[f.profile_index].class_name, f.team, f.position, f.landed_zone
                    );
                }
            } else {
                life.seat_wait = None;
            }
        }
    }
}

/// The cause of a loss to a round or its splash: friendly or enemy, and what fired.
fn cause_of(space: &bfe_sim::space::SpaceBattlefield, i: &SpaceImpact, weapon: u8, victim_team: i32, record: &mut Record) -> String {
    let (kind, name) = emitter_kind(space, i.emitter, weapon);
    let side = if i.team == victim_team { "friendly" } else { "enemy" };
    if kind == "ship gun" && side == "enemy" {
        *record.gun_kills.entry((victim_team, name)).or_default() += 1;
    }
    format!("{side} {kind}")
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn unit3(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if n > 1e-9 {
        scale3(a, 1.0 / n)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// One shield pool a mission links over several objects.
struct Link {
    controller: String,
    objects: Vec<String>,
    ids: Vec<usize>,
    pool: f64,
    max: f64,
    add: f64,
    down: bool,
}

fn shield_links(path: &str) -> Vec<Link> {
    let text = std::fs::read(path).expect("BFE_PILOT_SETUP file");
    let setup = json::parse(std::str::from_utf8(&text).expect("utf-8")).expect("setup json");
    setup
        .get("shieldLinks")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|l| {
            Some(Link {
                controller: l.get("controller")?.as_str()?.to_string(),
                objects: l.get("objects")?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                ids: Vec::new(),
                pool: 0.0,
                max: 0.0,
                add: 0.0,
                down: false,
            })
        })
        .collect()
}

/// The shared pool after one tick: every hit on any linked object comes off
/// it (two hits in one tick take both), and a linked object whose
/// shield reached zero drops it, as the mission's damage callback does.
/// Returns the pool after the damage, after the tick's recharge, and whether
/// it is down.
fn linked_pool(pool: f64, add_per_tick: f64, max: f64, losses: f64, broke: bool) -> (f64, f64, bool) {
    let after = pool - losses;
    if broke || after <= 0.0 {
        return (0.0, 0.0, true);
    }
    (after, (after + add_per_tick).min(max), false)
}

/// After each tick, as the mission's shield link script does from its damage
/// callbacks: the tick's shield losses on any linked object come off the
/// shared pool, every object takes the pool's charge, and at zero the
/// recharge stops and the generator is killed.
fn link_shields(w: &mut bfe_sim::world::owner::World, links: &mut [Link], tick: u32) {
    use bfe_sim::battle::rules::objects::ObjectProperty as P;
    for link in links.iter_mut().filter(|l| !l.down) {
        if link.ids.is_empty() {
            link.ids = link.objects.iter().filter_map(|n| w.space_building_by_name(n)).collect();
            let Some((cur, max, add)) = link.ids.first().and_then(|i| w.space_building_shields(*i)) else { continue };
            (link.pool, link.max, link.add) = (cur, max, add);
        }
        let (mut losses, mut broke) = (0.0, false);
        for e in w.space().expect("space").events() {
            match *e {
                SpaceEvent::BuildingDamaged { building, shield_loss, .. } if link.ids.contains(&building) => losses += shield_loss,
                SpaceEvent::ShieldChanged { building, cur_shield, .. } if link.ids.contains(&building) && cur_shield <= 0.0 => broke = true,
                _ => {}
            }
        }
        let (_, next, down) = linked_pool(link.pool, link.add / 60.0, link.max, losses, broke);
        link.pool = next;
        if tick.is_multiple_of(3600) {
            println!("PILOT shield tick={tick} controller={} charge={next:.0}", link.controller);
        }
        if down {
            for i in &link.ids {
                let _ = w.space_set_building_property(*i, P::AddShield(0.0));
                let _ = w.space_set_building_property(*i, P::CurrentShield(0.0));
            }
            if let Some(g) = w.space_building_by_name(&link.controller) {
                w.space_kill_building(g, None);
            }
            link.down = true;
            println!("PILOT shields_down tick={tick} controller={}", link.controller);
        } else {
            for i in &link.ids {
                if w.space_building_shields(*i).is_some_and(|s| (s.0 - next).abs() > 1e-9) {
                    let _ = w.space_set_building_property(*i, P::CurrentShield(next));
                }
            }
        }
    }
}

#[test]
fn a_linked_shield_pool_loses_every_simultaneous_hit() {
    // An invented pool: two 23-point hits on different linked objects in one
    // tick take 46, then the tick's recharge is added.
    let (after, next, down) = linked_pool(187_300.0, 11.0 / 60.0, 187_300.0, 46.0, false);
    assert_eq!(after, 187_254.0);
    assert!(!down);
    assert!((next - (187_254.0 + 11.0 / 60.0)).abs() < 1e-9);
    // Recharge stops at the pool's maximum.
    assert_eq!(linked_pool(187_300.0, 21.0, 187_300.0, 0.0, false).1, 187_300.0);
    // Hits that empty the pool drop it; so does a linked object reaching zero.
    assert!(linked_pool(19.0, 17.0, 187_300.0, 23.0, false).2);
    assert!(linked_pool(530.0, 17.0, 187_300.0, 0.0, true).2);
}

fn d(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn f3(x: f64) -> String {
    format!("{x:.2}")
}

fn mix(mut h: Fnv, values: impl Iterator<Item = f64>) -> Fnv {
    for v in values {
        h.bytes(&v.to_le_bytes());
    }
    h
}
