//! The battle package's replays (`cargo test -p sim --features replay
//! --test replay_battle`, BFE_REPLAY_CORPUS a directory of captures made
//! with `tools/sim-capture.mjs --specs battle`): each frame starts from the
//! state the JavaScript started from (tools/sim-capture/battle.mjs writes
//! it as explicit values), runs the Rust port, serves what it asks of the
//! rest of the world from the capture, and compares the state it ends with,
//! bit for bit: the capture's end state with the port's fields laid over
//! it must be the capture's end state.
//!
//! Without a corpus the tests pass without replaying (CI has no game data).
#![cfg(feature = "replay")]

use bfe_sim::battle::battlefield::Battlefield;
use bfe_sim::battle::boundary::{Boundary, Edge};
use bfe_sim::battle::mobile::{ClassRecord, Hull, MobileAi, MobileDesc, MobilePost, MobilePosts, Pad, Reported};
use bfe_sim::battle::posts::{Capturer, Change, CommandPosts, Post, PostAi, PostId, PostRules, SpawnPath};
use bfe_sim::battle::regions::{build_region, Built, Region, RegionType};
use bfe_sim::battle::rules::{Battle, Character, Spawn, TeamDesc};
use bfe_sim::battle::spawning::{ClassicSpawning, Draws, SpawnClass, SpawnTeam, TEAM_COUNT};
use bfe_sim::battle::stats::{BattleStats, Record, Ring, Seen, Slot, TimeEntry, RING_SIZE};
use bfe_sim::battle::teams::TeamMap;
use bfe_sim::battle::world::{BattleWorld, Body};
use bfe_sim::store::Nullable;
use sim_replay::{check_value, replay_corpus, Arena, Capture, Corpus, Frame, Log, Mismatch, Outcome, Typed, Value, V};
use std::cell::RefCell;
use std::rc::Rc;
use three::{Matrix4, Vector3};

// ---- Reading the capture's values.

fn num(v: V) -> f64 {
    v.opt_num().unwrap_or(f64::NAN)
}

fn opt_num(v: V) -> Option<f64> {
    if v.is_nullish() {
        None
    } else {
        Some(num(v))
    }
}

fn nullable(v: V) -> Nullable<f64> {
    if v.is_undefined() {
        Nullable::Undefined
    } else if v.is_null() {
        Nullable::Null
    } else {
        Nullable::Value(num(v))
    }
}

fn opt_str(v: V) -> Option<String> {
    v.str().map(str::to_string)
}

fn nums<const N: usize>(v: V) -> [f64; N] {
    let mut out = [0.0; N];
    for (i, x) in out.iter_mut().enumerate() {
        *x = num(v.at(i));
    }
    out
}

fn vec3(v: V) -> [f64; 3] {
    v.vec3().unwrap_or_else(|| nums::<3>(v))
}

fn region(v: V) -> Option<Region> {
    if v.is_nullish() {
        return None;
    }
    let matrix = v.get("matrix");
    let center = v.get("center");
    Some(Region {
        kind: RegionType::named(v.get("type").str().unwrap_or("")),
        matrix: (matrix.len() >= 16).then(|| nums::<16>(matrix)),
        center: (!center.is_nullish()).then(|| nums::<3>(center)),
        size: nums::<3>(v.get("size")),
    })
}

fn built(v: V) -> Option<Built> {
    if v.is_nullish() {
        return None;
    }
    let c = vec3(v.get("center"));
    Some(Built {
        kind: RegionType::named(v.get("type").str().unwrap_or("")),
        half: nums::<3>(v.get("half")),
        radius: num(v.get("radius")),
        center: Vector3::new(c[0], c[1], c[2]),
        inverse: Matrix4 { elements: nums::<16>(v.get("inverse")) },
    })
}

fn post(v: V) -> Post {
    let p = vec3(v.get("position"));
    let rules = v.get("rules");
    Post {
        name: opt_str(v.get("name")).unwrap_or_default(),
        label: None,
        team: num(v.get("team")),
        position: Vector3::new(p[0], p[1], p[2]),
        capture: built(v.get("capture")),
        value_bleed: opt_num(v.get("valueBleed")),
        kill_region: region(v.get("killRegion")),
        rules: PostRules { neutralize_time: num(rules.get("neutralizeTime")), capture_time: num(rules.get("captureTime")) },
        neutralize: num(v.get("neutralize")),
        progress: num(v.get("progress")),
        toward: num(v.get("toward")),
        counts: v.get("counts").entries().map(|(k, n)| (num(k), num(n))).collect(),
        contested: v.get("contested").bool(),
        building: (!v.get("building").is_nullish()).then(|| num(v.get("building").get("index")) as u32),
        active: v.get("active").bool(),
        ai: post_ai(v.get("ai")),
    }
}

fn path(v: V) -> Option<SpawnPath> {
    (!v.is_nullish()).then(|| SpawnPath { present: v.get("present").bool(), points: v.get("points").items().map(|p| p.items().map(num).collect()).collect() })
}

fn post_ai(v: V) -> PostAi {
    if v.is_nullish() {
        return PostAi::default();
    }
    PostAi {
        spawn_path: path(v.get("spawnPath")),
        ally_path: path(v.get("allyPath")),
        ally_count: num(v.get("allyCount")),
        class_spawn_points: num(v.get("classSpawnPoints")),
        class_spawn_world: v.get("classSpawnWorld").items().map(|p| p.items().map(num).collect()).collect(),
        yaw: num(v.get("yaw")),
    }
}

/// A post's place in all()'s list (the placed ones, then the command
/// vehicles') as a PostId.
fn post_id(place: f64, placed: usize) -> PostId {
    let place = place as usize;
    if place < placed {
        PostId::Placed(place)
    } else {
        PostId::Mobile(place - placed)
    }
}

fn post_place(p: PostId, placed: usize) -> f64 {
    match p {
        PostId::Placed(i) => i as f64,
        PostId::Mobile(i) => (placed + i) as f64,
    }
}

/// A hull's handle as a number: its place in the battle's hulls, or a
/// hull no longer listed (`gone:N`) past them.
fn hull_id(v: V) -> Option<u32> {
    let (kind, key) = v.handle()?;
    if kind != "vehicle" {
        return None;
    }
    if let Some(n) = key.opt_num() {
        return Some(n as u32);
    }
    let s = key.str()?;
    s.strip_prefix("gone:").and_then(|n| n.parse::<u32>().ok()).map(|n| 1_000_000 + n)
}

fn class_record(v: V) -> Option<ClassRecord> {
    if v.is_nullish() {
        return None;
    }
    let values = v.get("values");
    let six = |x: V| -> Option<Vec<f64>> { (!x.is_nullish()).then(|| x.items().map(num).collect()) };
    Some(ClassRecord {
        atk: six(values.get("atk")),
        def: six(values.get("def")),
        value_bleed: opt_num(v.get("valueBleed")),
        spawn_points: v
            .get("spawnPoints")
            .items()
            .map(|p| {
                let at = |i: usize| opt_num(p.at(i)).unwrap_or(0.0);
                [at(0), at(1), at(2), at(3)]
            })
            .collect(),
    })
}

fn hull(v: V) -> Option<(Hull, [f64; 3])> {
    if v.is_nullish() {
        return None;
    }
    Some((
        Hull {
            id: hull_id(v.get("id")).expect("a hull's handle"),
            alive: v.get("alive").bool(),
            command: v.get("command").bool(),
            generation: num(v.get("generation")),
            team: num(v.get("team")),
            controlling_team: num(v.get("controllingTeam")),
            seated: v.get("seated").bool(),
            yaw: num(v.get("yaw")),
            name: opt_str(v.get("name")).unwrap_or_default(),
            label: opt_str(v.get("label")).unwrap_or_default(),
            radius: num(v.get("radius")),
            speed: num(v.get("speed")),
            value_bleed_setting: num(v.get("valueBleedSetting")),
            record: class_record(v.get("record")),
        },
        vec3(v.get("position")),
    ))
}

/// A command vehicle's post, the hulls on its pad, and where each is.
fn mobile_post(v: V) -> (MobilePost, Pad, Vec<(u32, [f64; 3])>) {
    let desc = v.get("desc");
    let ai = v.get("ai");
    let r = v.get("reported");
    let mut places = Vec::new();
    let pad_v = v.get("pad");
    let mut pad = Pad::default();
    for h in pad_v.get("variants").items() {
        let (h, at) = hull(h).expect("a variant");
        places.push((h.id, at));
        pad.variants.push(h);
    }
    if let Some((h, at)) = hull(pad_v.get("vehicle")) {
        places.push((h.id, at));
        pad.vehicle = Some(h);
    }
    let post = MobilePost {
        slot: num(v.get("slot")) as usize,
        label: opt_str(v.get("label")).unwrap_or_default(),
        team: num(v.get("team")),
        active: v.get("active").bool(),
        vehicle: hull_id(v.get("vehicle")),
        life: opt_num(v.get("life")),
        yaw: num(v.get("yaw")),
        radius: num(v.get("radius")),
        speed: num(v.get("speed")),
        last_position: vec3(v.get("lastPosition")),
        desc: MobileDesc {
            name: opt_str(desc.get("name")).unwrap_or_default(),
            display_name: opt_str(desc.get("displayName")).unwrap_or_default(),
            value_bleed: num(desc.get("valueBleed")),
        },
        ai: MobileAi {
            atk: ai.get("atk").items().map(num).collect(),
            def: ai.get("def").items().map(num).collect(),
            value_bleed: num(ai.get("valueBleed")),
            points: ai.get("points").items().map(nums::<4>).collect(),
            class_spawn_points: num(ai.get("classSpawnPoints")),
        },
        reported: if r.is_nullish() {
            Reported::default()
        } else {
            Reported { active: r.get("active").bool(), team: num(r.get("team")), vehicle: hull_id(r.get("vehicle")), life: opt_num(r.get("life")) }
        },
    };
    (post, pad, places)
}

/// Each command vehicle's post's class spawn points where its hull is.
fn class_spawn_worlds(v: V) -> Vec<Vec<Vec<f64>>> {
    v.get("mobile").items().map(|m| m.get("classSpawnWorld").items().map(|p| p.items().map(num).collect()).collect()).collect()
}

/// The command posts, the pads' hulls by post, and where every hull is.
fn posts(v: V) -> (CommandPosts, Vec<Pad>, Vec<(u32, [f64; 3])>) {
    let mut pads = Vec::new();
    let mut places = Vec::new();
    let mobile = v.get("mobile");
    let mobile = (!mobile.is_nullish()).then(|| {
        let mut m = MobilePosts::default();
        for p in mobile.items() {
            let (post, pad, at) = mobile_post(p);
            m.posts.push(post);
            pads.push(pad);
            places.extend(at);
        }
        m
    });
    let posts = CommandPosts { posts: v.get("posts").items().map(post).collect(), time: num(v.get("time")), events: Vec::new(), mobile, teams: v.get("teams").bool() };
    (posts, pads, places)
}

fn time_entry(v: V) -> TimeEntry {
    TimeEntry { key: opt_str(v.get("key")).unwrap_or_default(), label: opt_str(v.get("label")), time: num(v.get("time")) }
}

fn slot(v: V) -> Slot {
    Slot {
        team_kills: num(v.get("teamKills")),
        vehicles_destroyed: num(v.get("vehiclesDestroyed")),
        spree: num(v.get("spree")),
        survivalist: num(v.get("survivalist")),
        life_kills: num(v.get("lifeKills")),
        life_kills_best: num(v.get("lifeKillsBest")),
        headshots: num(v.get("headshots")),
        deaths: num(v.get("deaths")),
        life: num(v.get("life")),
        life_best: num(v.get("lifeBest")),
        camp: num(v.get("camp")),
        camp_best: num(v.get("campBest")),
        escapes: num(v.get("escapes")),
        victims: v.get("victims").items().map(|p| (num(p.at(0)) as u32, num(p.at(1)))).collect(),
        classes: v.get("classes").items().map(time_entry).collect(),
        weapons: v.get("weapons").items().map(time_entry).collect(),
        vehicles: v.get("vehicles").items().map(time_entry).collect(),
    }
}

fn record(v: V) -> Option<Record> {
    if v.is_nullish() {
        return None;
    }
    let anchor = v.get("anchor");
    Some(Record {
        slots: [slot(v.get("slots").at(0)), slot(v.get("slots").at(1))],
        anchor: (!anchor.is_nullish()).then(|| nums::<3>(anchor)),
        health: num(v.get("health")),
        low: v.get("low").bool(),
    })
}

fn character(v: V) -> Character {
    Character {
        index: num(v.get("index")),
        team: num(v.get("team")),
        human: v.get("human").bool(),
        name: opt_str(v.get("name")),
        class_name: opt_str(v.get("className")),
        unit: opt_num(v.get("unit")).map(|u| u as u32),
        hero: v.get("hero").bool(),
        kills: num(v.get("kills")),
        team_kills: num(v.get("teamKills")),
        deaths: num(v.get("deaths")),
        captures: num(v.get("captures")),
        out_of_bounds: num(v.get("outOfBounds")),
        record: record(v.get("record")),
        ..Default::default()
    }
}

/// The spawn manager's view of a Character (spawnCharacterValue) laid on
/// one.
fn spawn_character(v: V, c: &mut Character, placed: usize) {
    c.team = num(v.get("team"));
    c.human = v.get("human").bool();
    c.hero = v.get("hero").bool();
    c.name = opt_str(v.get("name"));
    c.class_name = opt_str(v.get("className"));
    c.class_index = opt_num(v.get("classIndex"));
    c.slot = num(v.get("slot"));
    c.manager_index = num(v.get("managerIndex"));
    c.unit = opt_num(v.get("unit")).map(|u| u as u32);
    c.objective = opt_num(v.get("objective")).map(|p| post_id(p, placed));
    let sp = v.get("spawn");
    if !sp.is_nullish() {
        c.spawn = Spawn {
            timer: num(sp.get("timer")),
            wave: num(sp.get("wave")),
            ordinal: num(sp.get("ordinal")),
            post: opt_num(sp.get("post")).map(|p| post_id(p, placed)),
            ready: sp.get("ready").bool(),
        };
    }
}

fn spawning(v: V, placed: usize) -> ClassicSpawning {
    ClassicSpawning {
        teams: v
            .get("teams")
            .items()
            .map(|t| {
                let mut relations = [0.0; TEAM_COUNT];
                for (i, r) in t.get("relations").items().enumerate().take(TEAM_COUNT) {
                    relations[i] = num(r);
                }
                SpawnTeam {
                    index: num(t.get("index")),
                    faction: num(t.get("faction")),
                    unit_count: num(t.get("unitCount")),
                    tickets: num(t.get("tickets")),
                    classes: t.get("classes").items().map(|c| SpawnClass { name: opt_str(c.get("name")).unwrap_or_default(), target: num(c.get("target")), hero: c.get("hero").bool() }).collect(),
                    hero_count: num(t.get("heroCount")),
                    relations,
                    characters: t.get("characters").items().map(|c| num(c) as u32).collect(),
                    living: num(t.get("living")),
                    capacity: num(t.get("capacity")),
                }
            })
            .collect(),
        a: num(v.get("A")),
        b: num(v.get("B")),
        q: num(v.get("Q")),
        wave_countdown: num(v.get("waveCountdown")),
        wave: num(v.get("wave")),
        stagger: num(v.get("stagger")),
        released: num(v.get("released")),
        locals_cap: num(v.get("localsCap")),
        queues: v.get("queues").items().map(|q| (post_id(num(q.at(0)), placed), nums::<4>(q.at(1)))).collect(),
        demo: v.get("demo").bool(),
    }
}

fn team_map<T>(v: V, f: impl Fn(V) -> T) -> TeamMap<T> {
    v.items().map(|p| (num(p.at(0)), f(p.at(1)))).collect()
}

fn typed_f64(v: V) -> Vec<f64> {
    match v.typed() {
        Some(t) => (0..t.len()).map(|i| t.get(i).unwrap()).collect(),
        None => v.items().map(num).collect(),
    }
}

fn battle(v: V) -> Battle {
    let stats = v.get("stats");
    let ring = stats.get("ring");
    let mut r = Ring::default();
    for (i, k) in ring.get("killers").items().enumerate().take(RING_SIZE) {
        r.killers[i] = opt_num(k).map(|c| c as u32);
    }
    for (i, t) in typed_f64(ring.get("times")).into_iter().enumerate().take(RING_SIZE) {
        r.times[i] = t;
    }
    for (i, s) in typed_f64(ring.get("slots")).into_iter().enumerate().take(RING_SIZE) {
        r.slots[i] = s as i32;
    }
    r.head = num(ring.get("head")) as usize;
    r.tail = num(ring.get("tail")) as usize;
    Battle {
        own: num(v.get("own")),
        foe: num(v.get("foe")),
        teams: team_map(v.get("teams"), |t| TeamDesc {
            units: num(t.get("units")),
            reinforcements: opt_num(t.get("reinforcements")),
            bleed: t.get("bleed").items().map(nums::<2>).collect(),
        }),
        reinforcements: team_map(v.get("reinforcements"), num),
        start: team_map(v.get("start"), num),
        team_ids: v.get("teamIds").items().map(num).collect(),
        no_post: team_map(v.get("noPost"), num),
        bleed: team_map(v.get("bleed"), num),
        victory_clock: nullable(v.get("victoryClock")),
        countdown: nullable(v.get("countdown")),
        losing: v.get("losing").bool(),
        over: v.get("over").bool(),
        loser: nullable(v.get("loser")),
        over_time: num(v.get("overTime")),
        game_over: v.get("gameOver").bool(),
        finished: v.get("finished").bool(),
        stats: BattleStats { time: num(stats.get("time")), ring: r },
        bleed_rules: team_map(v.get("bleedRules"), |rules| rules.items().map(nums::<2>).collect()),
        bleed_clock: team_map(v.get("bleedClock"), num),
        characters: v.get("characters").items().map(character).collect(),
    }
}

fn seen(v: V) -> Option<Seen> {
    if v.is_nullish() {
        return None;
    }
    let pair = |x: V| (!x.is_nullish()).then(|| (opt_str(x.at(0)), opt_str(x.at(1))));
    let position = v.get("position");
    Some(Seen {
        position: (!position.is_nullish()).then(|| nums::<3>(position)),
        health: opt_num(v.get("health")),
        weapon: pair(v.get("weapon")),
        vehicle: pair(v.get("vehicle")),
        class_label: opt_str(v.get("classLabel")),
    })
}

fn body(v: V) -> Option<Body> {
    if v.is_nullish() {
        return None;
    }
    let droid = v.get("droid");
    Some(Body {
        removed: v.get("removed").bool(),
        alive: v.get("alive").bool(),
        combat_dead: v.get("combatDead").bool(),
        place: nums::<3>(v.get("place")),
        vehicle: hull_id(v.get("vehicle")),
        droid: (!droid.is_nullish()).then(|| nums::<3>(droid)),
    })
}

// ---- Writing the port's state over the capture's.

fn n(x: f64) -> Value {
    Value::Num(x)
}

fn s(x: &Option<String>) -> Value {
    x.as_deref().map(Value::str).unwrap_or(Value::Null)
}

fn opt(x: Option<f64>) -> Value {
    x.map(Value::Num).unwrap_or(Value::Null)
}

fn nullable_out(x: Nullable<f64>) -> Value {
    match x {
        Nullable::Value(v) => Value::Num(v),
        Nullable::Null => Value::Null,
        Nullable::Undefined => Value::Undefined,
    }
}

fn arr(arena: &mut Arena, xs: &[f64]) -> Value {
    arena.array(xs.iter().map(|x| Value::Num(*x)).collect())
}

/// The node at `path` (object keys and array places) under `root`.
fn node_at(arena: &Arena, root: &Value, path: &[&str]) -> Value {
    let mut v = arena.view(root);
    for key in path {
        v = match key.parse::<usize>() {
            Ok(i) => v.at(i),
            Err(_) => v.get(key),
        };
    }
    v.value.clone()
}

fn team_map_out<T>(arena: &mut Arena, m: &TeamMap<T>, f: impl Fn(&mut Arena, &T) -> Value) -> Value {
    let pairs: Vec<Value> = m
        .entries()
        .map(|(t, v)| {
            let v = f(arena, v);
            arena.array(vec![Value::Num(*t), v])
        })
        .collect();
    arena.array(pairs)
}

fn time_entries_out(arena: &mut Arena, entries: &[TimeEntry]) -> Value {
    let items = entries
        .iter()
        .map(|e| arena.object("", vec![("key", Value::str(&e.key)), ("label", s(&e.label)), ("time", n(e.time))]))
        .collect();
    arena.array(items)
}

fn record_out(arena: &mut Arena, r: &Option<Record>) -> Value {
    let Some(r) = r else { return Value::Null };
    let mut slots = Vec::new();
    for sl in &r.slots {
        let victims = sl.victims.iter().map(|(c, k)| arena.array(vec![n(*c as f64), n(*k)])).collect();
        let victims = arena.array(victims);
        let classes = time_entries_out(arena, &sl.classes);
        let weapons = time_entries_out(arena, &sl.weapons);
        let vehicles = time_entries_out(arena, &sl.vehicles);
        slots.push(arena.object(
            "",
            vec![
                ("teamKills", n(sl.team_kills)),
                ("vehiclesDestroyed", n(sl.vehicles_destroyed)),
                ("spree", n(sl.spree)),
                ("survivalist", n(sl.survivalist)),
                ("lifeKills", n(sl.life_kills)),
                ("lifeKillsBest", n(sl.life_kills_best)),
                ("headshots", n(sl.headshots)),
                ("deaths", n(sl.deaths)),
                ("life", n(sl.life)),
                ("lifeBest", n(sl.life_best)),
                ("camp", n(sl.camp)),
                ("campBest", n(sl.camp_best)),
                ("escapes", n(sl.escapes)),
                ("victims", victims),
                ("classes", classes),
                ("weapons", weapons),
                ("vehicles", vehicles),
            ],
        ));
    }
    let anchor = match r.anchor {
        Some(a) => arr(arena, &a),
        None => Value::Null,
    };
    let slots = arena.array(slots);
    arena.object("", vec![("anchor", anchor), ("health", n(r.health)), ("low", Value::Bool(r.low)), ("slots", slots)])
}

/// The port's battle over the capture's (every field the rules write).
fn battle_over(arena: &mut Arena, at: &Value, b: &Battle) {
    let reinforcements = team_map_out(arena, &b.reinforcements, |_, x| n(*x));
    arena.assign(at, "reinforcements", reinforcements);
    let start = team_map_out(arena, &b.start, |_, x| n(*x));
    arena.assign(at, "start", start);
    let no_post = team_map_out(arena, &b.no_post, |_, x| n(*x));
    arena.assign(at, "noPost", no_post);
    let bleed = team_map_out(arena, &b.bleed, |_, x| n(*x));
    arena.assign(at, "bleed", bleed);
    let clock = team_map_out(arena, &b.bleed_clock, |_, x| n(*x));
    arena.assign(at, "bleedClock", clock);
    arena.assign(at, "victoryClock", nullable_out(b.victory_clock));
    arena.assign(at, "countdown", nullable_out(b.countdown));
    arena.assign(at, "losing", Value::Bool(b.losing));
    arena.assign(at, "over", Value::Bool(b.over));
    arena.assign(at, "loser", nullable_out(b.loser));
    arena.assign(at, "overTime", n(b.over_time));
    arena.assign(at, "gameOver", Value::Bool(b.game_over));
    arena.assign(at, "finished", Value::Bool(b.finished));
    let stats = node_at(arena, at, &["stats"]);
    arena.assign(&stats, "time", n(b.stats.time));
    let ring = node_at(arena, &stats, &["ring"]);
    let killers = b.stats.ring.killers.iter().map(|k| opt(k.map(|c| c as f64))).collect();
    let killers = arena.array(killers);
    arena.assign(&ring, "killers", killers);
    let times = arena.typed(Typed::F64(b.stats.ring.times.to_vec()));
    arena.assign(&ring, "times", times);
    let slots = arena.typed(Typed::I32(b.stats.ring.slots.to_vec()));
    arena.assign(&ring, "slots", slots);
    arena.assign(&ring, "head", n(b.stats.ring.head as f64));
    arena.assign(&ring, "tail", n(b.stats.ring.tail as f64));
    for (i, c) in b.characters.iter().enumerate() {
        let at_c = node_at(arena, at, &["characters", &i.to_string()]);
        arena.assign(&at_c, "kills", n(c.kills));
        arena.assign(&at_c, "teamKills", n(c.team_kills));
        arena.assign(&at_c, "deaths", n(c.deaths));
        arena.assign(&at_c, "captures", n(c.captures));
        arena.assign(&at_c, "outOfBounds", n(c.out_of_bounds));
        let r = record_out(arena, &c.record);
        arena.assign(&at_c, "record", r);
    }
}

/// The port's posts over the capture's (what update() and refresh() write).
fn posts_over(arena: &mut Arena, at: &Value, p: &CommandPosts) {
    arena.assign(at, "time", n(p.time));
    for (i, post) in p.posts.iter().enumerate() {
        let at_p = node_at(arena, at, &["posts", &i.to_string()]);
        arena.assign(&at_p, "team", n(post.team));
        arena.assign(&at_p, "active", Value::Bool(post.active));
        arena.assign(&at_p, "neutralize", n(post.neutralize));
        arena.assign(&at_p, "progress", n(post.progress));
        arena.assign(&at_p, "toward", n(post.toward));
        let counts = post.counts.iter().map(|(t, k)| (n(*t), n(*k))).collect();
        let counts = arena.map(counts);
        arena.assign(&at_p, "counts", counts);
        arena.assign(&at_p, "contested", Value::Bool(post.contested));
    }
    if let Some(m) = &p.mobile {
        for (i, post) in m.posts.iter().enumerate() {
            let at_m = node_at(arena, at, &["mobile", &i.to_string()]);
            arena.assign(&at_m, "label", Value::str(&post.label));
            arena.assign(&at_m, "team", n(post.team));
            arena.assign(&at_m, "active", Value::Bool(post.active));
            let vehicle = node_at(arena, &at_m, &["vehicle"]);
            let ok = match post.vehicle {
                None => matches!(vehicle, Value::Null),
                Some(id) => hull_id(arena.view(&vehicle)) == Some(id),
            };
            if !ok {
                arena.assign(&at_m, "vehicle", Value::str(&format!("the port's hull {:?}", post.vehicle)));
            }
            arena.assign(&at_m, "life", opt(post.life));
            arena.assign(&at_m, "yaw", n(post.yaw));
            arena.assign(&at_m, "radius", n(post.radius));
            arena.assign(&at_m, "speed", n(post.speed));
            let lp = arena.vec3(post.last_position);
            arena.assign(&at_m, "lastPosition", lp);
            let desc = node_at(arena, &at_m, &["desc"]);
            arena.assign(&desc, "name", Value::str(&post.desc.name));
            arena.assign(&desc, "displayName", Value::str(&post.desc.display_name));
            arena.assign(&desc, "valueBleed", n(post.desc.value_bleed));
            let ai = node_at(arena, &at_m, &["ai"]);
            let atk = arr(arena, &post.ai.atk);
            arena.assign(&ai, "atk", atk);
            let def = arr(arena, &post.ai.def);
            arena.assign(&ai, "def", def);
            arena.assign(&ai, "valueBleed", n(post.ai.value_bleed));
            let points = post.ai.points.iter().map(|q| arr(arena, q)).collect();
            let points = arena.array(points);
            arena.assign(&ai, "points", points);
            arena.assign(&ai, "classSpawnPoints", n(post.ai.class_spawn_points));
        }
    }
    let events = p
        .events
        .iter()
        .map(|e| {
            let (kind, from, by, destroyed) = match e.change {
                Change::Captured { by } => ("captured", Value::Null, n(by), false),
                Change::Neutralized { from } => ("neutralized", n(from), Value::Null, false),
                Change::Destroyed { from } => ("lost", n(from), Value::Null, true),
            };
            arena.object("", vec![("post", n(e.post as f64)), ("kind", Value::str(kind)), ("from", from), ("by", by), ("destroyed", Value::Bool(destroyed))])
        })
        .collect();
    let events = arena.array(events);
    arena.assign(at, "events", events);
}

// ---- The world, served from a frame.

/// What a frame's state says of the rest of the world, and its calls.
struct Served<'f> {
    log: Rc<RefCell<Log<'f>>>,
    placed: usize,
    class_spawn_worlds: Vec<Vec<Vec<f64>>>,
    hero_names: Vec<(String, Option<String>)>,
    health: Vec<(u32, f64)>,

    capture_teams: Vec<f64>,
    relations: Vec<Vec<f64>>,
    living: Vec<Option<f64>>,
    seen: Vec<Option<Seen>>,
    alive: Vec<(u32, bool)>,
    pads: Vec<Pad>,
    places: Vec<(u32, [f64; 3])>,
    buildings: Vec<(u32, bool)>,
    bodies: Vec<(u32, Body)>,
    failure: Option<String>,
}

impl<'f> Served<'f> {
    fn new(frame: &'f Frame) -> Served<'f> {
        Served {
            log: Rc::new(RefCell::new(Log::new(frame))),
            placed: 0,
            class_spawn_worlds: Vec::new(),
            hero_names: Vec::new(),
            health: Vec::new(),
            capture_teams: Vec::new(),
            relations: Vec::new(),
            living: Vec::new(),
            seen: Vec::new(),
            alive: Vec::new(),
            pads: Vec::new(),
            places: Vec::new(),
            buildings: Vec::new(),
            bodies: Vec::new(),
            failure: None,
        }
    }

    fn fail(&mut self, what: String) {
        self.failure.get_or_insert(what);
    }

    fn team(table: &[f64], team: f64) -> Option<f64> {
        (team >= 0.0 && team.fract() == 0.0).then(|| table.get(team as usize).copied()).flatten()
    }

    /// A served kill: the call, then the bodies as it left them.
    fn serve(&mut self, label: &str) {
        let log = self.log.clone();
        let mut log = log.borrow_mut();
        match log.call(label) {
            Ok(call) => {
                let after = call.extra().get("bodies");
                self.bodies = after.items().filter_map(|b| Some((num(b.get("id")) as u32, body(b)?))).collect();
            }
            Err(Mismatch(m)) => self.fail(m),
        }
    }
}

/// The shared generator A, served from the frame's draws.
struct LogDraws<'f> {
    log: Rc<RefCell<Log<'f>>>,
    failure: Option<String>,
}

impl Draws for LogDraws<'_> {
    fn next(&mut self) -> f64 {
        match self.log.borrow_mut().draw("A") {
            Ok(v) => v,
            Err(Mismatch(m)) => {
                self.failure.get_or_insert(m);
                0.0
            }
        }
    }
}

impl Served<'_> {
    /// A served call of the spawn manager's: checked, then the Character it
    /// was made for as it left him (the fields `fields` names), and the
    /// manager's own state it changed, kept for the caller to apply.
    fn serve_spawn(&mut self, label: &str, c: &mut Character, place: u32, fields: &[&str]) {
        let log = self.log.clone();
        let mut log = log.borrow_mut();
        let call = match log.call(label) {
            Ok(call) => call,
            Err(Mismatch(m)) => {
                self.fail(m);
                return;
            }
        };
        let mut theirs = c.clone();
        spawn_character(call.extra().get("characters").at(place as usize), &mut theirs, self.placed);
        for f in fields {
            match *f {
                "unit" => c.unit = theirs.unit,
                "className" => c.class_name = theirs.class_name.clone(),
                "objective" => c.objective = theirs.objective,
                _ => unreachable!(),
            }
        }
    }
}

impl BattleWorld for Served<'_> {
    fn spawn_unit(&mut self, character: &mut Character, place: u32, class_name: &str, post: Option<PostId>, at: [f64; 3], yaw: Option<f64>) -> Option<u32> {
        let log = self.log.clone();
        let call = {
            let peek = log.borrow();
            match peek.peek() {
                Some(sim_replay::Event::Call(c)) if c.name == "spawning.spawnUnit" => Some(c),
                _ => None,
            }
        };
        let Some(call) = call else {
            self.fail(format!("spawnUnit({class_name}): the capture has no such call next"));
            return None;
        };
        // Its arguments: (character, className, post, [x, y, z], yaw).
        let args = call.args();
        let mut why = None;
        if args.at(1).str() != Some(class_name) {
            why = Some(format!("spawnUnit's class: the capture has {}, the port {class_name}", args.at(1).show()));
        } else if nums::<3>(args.at(3)).iter().zip(at).any(|(a, b)| !sim_replay::same_number(*a, b)) {
            why = Some(format!("spawnUnit's point: the capture has {}, the port {at:?}", args.at(3).show()));
        } else if !sim_replay::same_number(opt_num(args.at(4)).unwrap_or(f64::NAN), yaw.unwrap_or(f64::NAN)) || args.at(4).is_undefined() != yaw.is_none() {
            why = Some(format!("spawnUnit's yaw: the capture has {}, the port {yaw:?}", args.at(4).show()));
        }
        let _ = post;
        if let Some(why) = why {
            self.fail(why);
        }
        let unit = call.result().handle_num("unit").map(|u| u as u32);
        self.serve_spawn("spawning.spawnUnit", character, place, &["unit", "className", "objective"]);
        unit
    }
    fn force_kill(&mut self, place: u32, _unit: u32) {
        self.fail(format!("forceKill({place}): a forced kill is not served yet"));
    }
    fn health(&mut self, unit: u32) -> f64 {
        match self.health.iter().find(|(u, _)| *u == unit) {
            Some((_, h)) => *h,
            None => {
                self.fail(format!("unit {unit}'s health: not captured"));
                0.0
            }
        }
    }
    fn request_objective(&mut self, character: &mut Character, place: u32) {
        self.serve_spawn("objectives.request", character, place, &["objective"]);
    }
    fn republish(&mut self, character: &mut Character, place: u32) {
        self.serve_spawn("objectives.republish", character, place, &["objective"]);
    }
    fn hero_name(&mut self, class_name: &str) -> Option<String> {
        self.hero_names.iter().find(|(n, _)| n == class_name).and_then(|(_, h)| h.clone())
    }
    fn class_spawn_world(&mut self, post: usize) -> Vec<Vec<f64>> {
        self.class_spawn_worlds.get(post).cloned().unwrap_or_default()
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        match (a >= 0.0 && a < 8.0 && a.fract() == 0.0, b >= 0.0 && b < 8.0 && b.fract() == 0.0) {
            (true, true) => self.relations[a as usize][b as usize],
            _ => {
                self.fail(format!("relation({a}, {b}): not captured"));
                0.0
            }
        }
    }
    fn capture_team(&mut self, team: f64) -> f64 {
        Self::team(&self.capture_teams, team).unwrap_or_else(|| {
            self.fail(format!("captureTeam({team}): not captured"));
            team
        })
    }
    fn unit_alive(&mut self, unit: u32) -> bool {
        match self.alive.iter().find(|(u, _)| *u == unit) {
            Some((_, a)) => *a,
            None => {
                self.fail(format!("unit {unit}'s alive: not captured"));
                false
            }
        }
    }
    fn observe(&mut self, character: u32, _unit: Option<u32>) -> Option<Seen> {
        self.seen.get(character as usize).cloned().flatten()
    }
    fn body(&mut self, unit: u32) -> Body {
        match self.bodies.iter().find(|(u, _)| *u == unit) {
            Some((_, b)) => *b,
            None => {
                self.fail(format!("unit {unit}'s body: not captured"));
                Body::default()
            }
        }
    }
    fn combat_dead(&mut self, unit: u32) -> bool {
        self.body(unit).combat_dead
    }
    fn kill(&mut self, _unit: u32) {
        self.serve("combat.damage");
    }
    fn kill_droid(&mut self, _unit: u32) {
        self.serve("remotes.kill");
    }
    fn wreck(&mut self, _vehicle: u32) {
        self.serve("vehicle.damage");
    }
    fn pad(&mut self, slot: usize) -> Pad {
        self.pads.get(slot).cloned().unwrap_or_default()
    }
    fn vehicle_position(&mut self, vehicle: u32) -> [f64; 3] {
        match self.places.iter().find(|(v, _)| *v == vehicle) {
            Some((_, at)) => *at,
            None => {
                self.fail(format!("hull {vehicle}'s position: not captured"));
                [0.0; 3]
            }
        }
    }
    fn living(&mut self, team: f64) -> Option<f64> {
        if team >= 0.0 && team.fract() == 0.0 {
            self.living.get(team as usize).copied().flatten()
        } else {
            None
        }
    }
    fn building_alive(&mut self, building: u32) -> bool {
        match self.buildings.iter().find(|(b, _)| *b == building) {
            Some((_, a)) => *a,
            None => {
                self.fail(format!("building {building}'s alive: not captured"));
                true
            }
        }
    }
    fn battle_over(&mut self, _loser: f64) {}
}

/// Each command vehicle's pad's hulls, by the pad's slot.
fn pads_by_slot(posts: &CommandPosts, pads: Vec<Pad>) -> Vec<Pad> {
    let mut by_slot = Vec::new();
    if let Some(m) = &posts.mobile {
        for (i, p) in m.posts.iter().enumerate() {
            if by_slot.len() <= p.slot {
                by_slot.resize(p.slot + 1, Pad::default());
            }
            by_slot[p.slot] = pads[i].clone();
        }
    }
    by_slot
}

/// The port's spawn manager over the capture's.
fn spawning_over(arena: &mut Arena, at: &Value, sp: &ClassicSpawning, placed: usize) {
    arena.assign(at, "waveCountdown", n(sp.wave_countdown));
    arena.assign(at, "wave", n(sp.wave));
    arena.assign(at, "stagger", n(sp.stagger));
    arena.assign(at, "released", n(sp.released));
    arena.assign(at, "localsCap", n(sp.locals_cap));
    for (i, t) in sp.teams.iter().enumerate() {
        let at_t = node_at(arena, at, &["teams", &i.to_string()]);
        arena.assign(&at_t, "tickets", n(t.tickets));
        arena.assign(&at_t, "living", n(t.living));
        let chars: Vec<f64> = t.characters.iter().map(|c| *c as f64).collect();
        let chars = arr(arena, &chars);
        arena.assign(&at_t, "characters", chars);
        let classes = t
            .classes
            .iter()
            .map(|c| arena.object("", vec![("name", Value::str(&c.name)), ("target", n(c.target)), ("hero", Value::Bool(c.hero))]))
            .collect();
        let classes = arena.array(classes);
        arena.assign(&at_t, "classes", classes);
    }
    let queues = sp
        .queues
        .iter()
        .map(|(p, q)| {
            let q = arr(arena, q);
            arena.array(vec![n(post_place(*p, placed)), q])
        })
        .collect();
    let queues = arena.array(queues);
    arena.assign(at, "queues", queues);
}

/// The port's Characters over the capture's (spawnCharacterValue).
fn characters_over(arena: &mut Arena, at: &Value, chars: &[Character], placed: usize) {
    for (i, c) in chars.iter().enumerate() {
        let at_c = node_at(arena, at, &[&i.to_string()]);
        arena.assign(&at_c, "team", n(c.team));
        arena.assign(&at_c, "human", Value::Bool(c.human));
        arena.assign(&at_c, "hero", Value::Bool(c.hero));
        arena.assign(&at_c, "name", s(&c.name));
        arena.assign(&at_c, "className", s(&c.class_name));
        arena.assign(&at_c, "classIndex", opt(c.class_index));
        arena.assign(&at_c, "unit", opt(c.unit.map(|u| u as f64)));
        arena.assign(&at_c, "objective", opt(c.objective.map(|p| post_place(p, placed))));
        let sp = node_at(arena, &at_c, &["spawn"]);
        if !matches!(sp, Value::Null | Value::Undefined) {
            arena.assign(&sp, "timer", n(c.spawn.timer));
            arena.assign(&sp, "wave", n(c.spawn.wave));
            arena.assign(&sp, "ordinal", n(c.spawn.ordinal));
            arena.assign(&sp, "post", opt(c.spawn.post.map(|p| post_place(p, placed))));
            arena.assign(&sp, "ready", Value::Bool(c.spawn.ready));
        }
    }
}

/// The spawn manager, its Characters and the battle's reinforcements as a
/// frame's state has them, and a world serving the rest.
fn spawn_frame(frame: &Frame) -> (ClassicSpawning, Battle, CommandPosts, Served<'_>) {
    let before = frame.before();
    let (posts, pads, places) = posts(before.get("posts"));
    let placed = posts.posts.len();
    let sp = spawning(before.get("spawning"), placed);
    let mut battle = Battle { reinforcements: team_map(before.get("reinforcements"), num), ..Default::default() };
    battle.characters = before
        .get("characters")
        .items()
        .map(|v| {
            let mut c = Character::default();
            spawn_character(v, &mut c, placed);
            c
        })
        .collect();
    let mut world = Served::new(frame);
    world.placed = placed;
    world.pads = pads_by_slot(&posts, pads);
    world.places = places;
    world.class_spawn_worlds = class_spawn_worlds(before.get("posts"));
    world.hero_names = before.get("heroNames").items().map(|p| (opt_str(p.at(0)).unwrap_or_default(), opt_str(p.at(1)))).collect();
    world.health = battle.characters.iter().zip(before.get("health").items()).filter_map(|(c, h)| c.unit.map(|u| (u, num(h)))).collect();
    (sp, battle, posts, world)
}

/// The spawn manager's frame's end state against the port's.
fn compare_spawn(frame: &Frame, sp: &ClassicSpawning, battle: &Battle, placed: usize) -> Result<(), Mismatch> {
    compare(frame, "spawning", |arena, at| spawning_over(arena, at, sp, placed))
        .and_then(|_| compare(frame, "characters", |arena, at| characters_over(arena, at, &battle.characters, placed)))
        .and_then(|_| {
            let mut arena = Arena::default();
            let mine = team_map_out(&mut arena, &battle.reinforcements, |_, x| n(*x));
            check_value("reinforcements", frame.after().get("reinforcements"), arena.view(&mine))
        })
}

/// Whether the port's state, laid over the capture's end state, is it.
fn compare(frame: &Frame, root: &str, mine: impl FnOnce(&mut Arena, &Value)) -> Result<(), Mismatch> {
    let after = frame.after().get(root);
    let mut arena = Arena::default();
    let copy = arena.copy(after);
    mine(&mut arena, &copy);
    check_value(root, after, arena.view(&copy))
}

fn outcome(r: Result<(), Mismatch>, served: Option<&Served>) -> Outcome {
    if let Some(f) = served.and_then(|s| s.failure.clone()) {
        return Outcome::Failed(f);
    }
    match r {
        Ok(()) => Outcome::Same,
        Err(Mismatch(m)) => Outcome::Failed(m),
    }
}

fn corpus() -> Option<Corpus> {
    Corpus::from_env().expect("BFE_REPLAY_CORPUS")
}

fn run(name: &str, port: impl FnMut(&Capture, &Frame) -> Outcome) {
    let Some(corpus) = corpus() else { return };
    let report = replay_corpus(&corpus, name, port).expect("the corpus reads");
    println!("{}", report.summary());
    report.assert_clean();
}

// ---- The replays.

/// CommandPosts.update: the posts' counts, captures, neutralizations and
/// destroyed buildings; and each capture region as buildRegion makes it
/// from the world file's.
#[test]
fn command_posts_replay_as_captured() {
    run("CommandPosts.update", |_, frame| {
        let before = frame.before();
        let (mut posts, _, _) = posts(before.get("posts"));
        for (i, p) in before.get("posts").get("posts").items().enumerate() {
            if let Some(desc) = region(p.get("captureDesc")) {
                if Some(build_region(&desc)) != posts.posts[i].capture {
                    return Outcome::Failed(format!("post {i}'s capture region: buildRegion differs from the capture's"));
                }
            }
        }
        let soldiers: Vec<Capturer> = before
            .get("soldiers")
            .items()
            .map(|s| {
                let p = vec3(s.get("position"));
                Capturer { team: num(s.get("team")), position: Vector3::new(p[0], p[1], p[2]) }
            })
            .collect();
        let mut world = Served::new(frame);
        world.capture_teams = before.get("captureTeams").items().map(num).collect();
        world.buildings = before
            .get("posts")
            .get("posts")
            .items()
            .filter(|p| !p.get("building").is_nullish())
            .map(|p| (num(p.get("building").get("index")) as u32, p.get("building").get("alive").bool()))
            .collect();
        posts.update(num(frame.arg(0)), &soldiers, &mut world);
        let r = compare(frame, "posts", |arena, at| posts_over(arena, at, &posts)).and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}

/// Battle.update: the scores' tick, each team's bleed and countdown, the
/// end; the command vehicles' posts refreshed on the way.
#[test]
fn battle_rules_replay_as_captured() {
    run("Battle.update", |_, frame| {
        let before = frame.before();
        let mut b = battle(before.get("battle"));
        let (mut posts, pads, places) = posts(before.get("posts"));
        let active: Vec<bool> = before.get("active").items().map(|a| a.bool()).collect();
        let mut world = Served::new(frame);
        world.relations = before.get("relations").items().map(|row| row.items().map(num).collect()).collect();
        world.living = before.get("living").items().map(opt_num).collect();
        world.seen = before.get("seen").items().map(seen).collect();
        world.alive = b.characters.iter().zip(before.get("alive").items()).filter_map(|(c, a)| c.unit.map(|u| (u, a.bool()))).collect();
        world.pads = pads_by_slot(&posts, pads);
        world.places = places;
        // (BattleStats.tick asks in the Characters' order, by place.)
        b.update(num(frame.arg(0)), &mut posts, |i, _| active.get(i).copied().unwrap_or(false), &mut world);
        let r = compare(frame, "battle", |arena, at| battle_over(arena, at, &b))
            .and_then(|_| compare(frame, "posts", |arena, at| posts_over(arena, at, &posts)))
            .and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}

/// ClassicSpawning.clock: the wave and release clocks, the locals' cap.
#[test]
fn spawn_clock_replays_as_captured() {
    run("ClassicSpawning.clock", |_, frame| {
        let before = frame.before();
        let (mut posts, pads, places) = posts(before.get("posts"));
        let placed = posts.posts.len();
        let mut sp = spawning(before.get("spawning"), placed);
        let mut world = Served::new(frame);
        world.placed = placed;
        world.pads = pads_by_slot(&posts, pads);
        world.places = places;
        sp.clock(num(frame.arg(0)), &mut posts, &mut world);
        let r = compare(frame, "spawning", |arena, at| spawning_over(arena, at, &sp, placed)).and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}

/// ClassicSpawning.updateCharacters: each Character's respawn (served:
/// spawnUnit and the AI's objectives; the draws checked in order).
#[test]
fn spawn_characters_replay_as_captured() {
    run("ClassicSpawning.updateCharacters", |_, frame| {
        if !frame.before().get("battleCharacters").bool() {
            return Outcome::Skipped("the battle's Characters are not the manager's".into());
        }
        let (mut sp, mut battle, mut posts, mut world) = spawn_frame(frame);
        let placed = posts.posts.len();
        let mut draws = LogDraws { log: world.log.clone(), failure: None };
        sp.update_characters(num(frame.arg(0)), &mut battle, &mut posts, &mut world, &mut draws);
        if let Some(f) = draws.failure {
            return Outcome::Failed(f);
        }
        let r = compare_spawn(frame, &sp, &battle, placed).and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}

/// ClassicSpawning.onDeath: a Character's body died.
#[test]
fn spawn_deaths_replay_as_captured() {
    run("ClassicSpawning.onDeath", |_, frame| {
        let place = num(frame.before().get("place"));
        if !(place >= 0.0) {
            return Outcome::Skipped("a Character not in the manager's list".into());
        }
        let (mut sp, mut battle, posts, mut world) = spawn_frame(frame);
        let placed = posts.posts.len();
        sp.on_death(place as usize, &mut battle, &mut world);
        let r = compare_spawn(frame, &sp, &battle, placed).and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}

/// Battlefield.update: each Character's countdown outside, and the kills
/// when it runs out (served).
#[test]
fn battlefield_replays_as_captured() {
    run("Battlefield.update", |_, frame| {
        let before = frame.before();
        let boundary = Boundary {
            polygons: before.get("boundary").items().map(|edges| edges.items().map(|e| Edge { x: num(e.at(0)), z: num(e.at(1)), nx: num(e.at(2)), nz: num(e.at(3)) }).collect()).collect(),
        };
        let field = Battlefield { boundary };
        let mut characters: Vec<Character> = before
            .get("characters")
            .items()
            .map(|c| Character { out_of_bounds: num(c.get("outOfBounds")), unit: opt_num(c.get("unit")).map(|u| u as u32), ..Default::default() })
            .collect();
        let mut world = Served::new(frame);
        world.bodies = before.get("bodies").items().filter_map(|b| Some((num(b.get("id")) as u32, body(b)?))).collect();
        field.update(num(frame.arg(0)), &mut characters, &mut world);
        let r = compare(frame, "characters", |arena, at| {
            for (i, c) in characters.iter().enumerate() {
                let at_c = node_at(arena, at, &[&i.to_string()]);
                arena.assign(&at_c, "outOfBounds", n(c.out_of_bounds));
            }
        })
        .and_then(|_| world.log.borrow().finish());
        outcome(r, Some(&world))
    });
}
