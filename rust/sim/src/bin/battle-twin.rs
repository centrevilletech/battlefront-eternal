//! The Rust side of the battle package's twin gate (V1): replays the
//! operations tools/battle-twin.mjs wrote (world regions and points, kill
//! regions, command posts' ticks, end-of-match records) through the battle
//! port and writes the same trace, every f64 as its bit pattern:
//!
//!   battle-twin OPS.json TRACE.out
//!
//! The JavaScript's trace and this one must diff empty.

use bfe_content::json;
use bfe_content::Value;
use bfe_sim::battle::mobile::{MobileEvent, Pad};
use bfe_sim::battle::names::NameLists;
use bfe_sim::battle::objectives::kill_region;
use bfe_sim::battle::phase::change_of;
use bfe_sim::battle::posts::{self, remaining, Capturer, Change};
use bfe_sim::battle::regions::{build_region, in_region, region_desc};
use bfe_sim::battle::roster;
use bfe_sim::battle::rules::{new_character, new_stats};
use bfe_sim::battle::spawning::{self, Arg, Call, HumanSpec, TEAM_CAPACITY};
use bfe_sim::battle::stats::{self, Seen};
use bfe_sim::battle::world::{BattleWorld, Body, Member, Victim};
use bfe_sim::gen::{Building, Character, MobilePost, PlayerId, Post, PostAi, PostEvent, PostRef, RegionDesc, RegionType, SpawnPath, SpawnPost, Store, Unit, Vehicle};
use bfe_sim::store::{Nullable, Ref, StrId, Strings};
use std::fmt::Write as _;
use three::Vector3;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// A number written as its bit pattern.
fn num(v: &Value) -> f64 {
    match v.as_str() {
        Some(s) => f64::from_bits(u64::from_str_radix(s, 16).expect("a bit pattern")),
        None => v.as_f64().unwrap_or(f64::NAN),
    }
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).unwrap_or(&[])
}

fn nums<const N: usize>(v: &Value) -> [f64; N] {
    let a = v.as_array().expect("an array");
    let mut out = [0.0; N];
    for (i, x) in out.iter_mut().enumerate() {
        *x = num(&a[i]);
    }
    out
}

fn vec3(v: &Value) -> Vector3 {
    let [x, y, z] = nums::<3>(v);
    Vector3::new(x, y, z)
}

/// A region as the twin wrote it, and whether its type is one the world
/// files never name ("other": built as a box is, holding nothing, as the
/// JavaScript builds and tests it).
fn region(v: &Value) -> (RegionDesc, bool) {
    let matrix = v.get("matrix").filter(|m| m.as_array().is_some()).map(nums::<16>);
    let center = v.get("center").filter(|c| c.as_array().is_some()).map(nums::<3>);
    let (kind, other) = match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "sphere" => (RegionType::Sphere, false),
        "box" => (RegionType::Box, false),
        "cylinder" => (RegionType::Cylinder, false),
        _ => (RegionType::Box, true),
    };
    (region_desc(kind, matrix.as_ref().map(|m| &m[..]), center.as_ref().map(|c| &c[..]), nums::<3>(v.get("size").unwrap())), other)
}

/// A string as the trace writes it.
fn text(strings: &Strings, id: StrId) -> String {
    String::from_utf16_lossy(strings.units(id))
}

struct Trace(String);

impl Trace {
    fn line(&mut self, parts: &[String]) {
        self.0.push_str(&parts.join(" "));
        self.0.push('\n');
    }
}

fn b(x: bool) -> String {
    (if x { "1" } else { "0" }).into()
}

fn opt_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

/// The world the twin's command posts and rosters ask: the capture team
/// table, hero names (upper case), no AI objectives.
struct Twin {
    capture: Vec<f64>,
    /// Each class's hero name (its name in upper case), in the battle's
    /// strings.
    heroes: Vec<(StrId, StrId)>,
    /// The roster run's bodies (unit slot, alive), and the lines its world
    /// calls write (the run's number first), in order.
    units: Vec<(u32, bool)>,
    run: f64,
    lines: Vec<Vec<String>>,
}

impl Twin {
    fn place(s: &Store, c: Ref<Character>) -> f64 {
        s.battle.characters.iter().position(|x| *x == c).unwrap() as f64
    }
}

impl BattleWorld for Twin {
    fn relation(&mut self, _: f64, _: f64) -> f64 {
        0.0
    }
    fn capture_team(&mut self, team: f64) -> f64 {
        self.capture.get(team as usize).copied().unwrap_or(team)
    }
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool {
        self.units.iter().find(|(u, _)| *u == unit.slot).map(|(_, a)| *a).expect("a roster run's body")
    }
    fn observe(&mut self, _: Ref<Character>, _: Option<Ref<Unit>>) -> Option<Seen> {
        unreachable!()
    }
    fn body(&mut self, _: Ref<Unit>) -> Body {
        unreachable!()
    }
    fn combat_dead(&mut self, _: Ref<Unit>) -> bool {
        unreachable!()
    }
    fn kill(&mut self, _: &mut Store, _: Ref<Unit>) {
        unreachable!()
    }
    fn kill_droid(&mut self, _: &mut Store, _: Ref<Unit>) {
        unreachable!()
    }
    fn wreck(&mut self, _: &mut Store, _: Ref<Vehicle>) {
        unreachable!()
    }
    fn pad(&mut self, _: f64) -> Pad {
        unreachable!()
    }
    fn vehicle_position(&mut self, _: Ref<Vehicle>) -> [f64; 3] {
        unreachable!()
    }
    fn class_spawn_world(&mut self, _: Ref<MobilePost>) -> Vec<[f64; 4]> {
        unreachable!()
    }
    fn spawn_unit(&mut self, _: &mut Store, _: Ref<Character>, _: Option<StrId>, _: SpawnPost, _: [f64; 3], _: Option<f64>) -> Option<Ref<Unit>> {
        unreachable!()
    }
    fn force_kill(&mut self, _: &mut Store, _: Ref<Character>, _: Ref<Unit>) {
        unreachable!()
    }
    fn health(&mut self, _: Ref<Unit>) -> f64 {
        unreachable!()
    }
    fn request_objective(&mut self, _: &mut Store, _: Ref<Character>) {}
    fn republish(&mut self, _: &mut Store, _: Ref<Character>) {
        unreachable!()
    }
    fn hero_name(&mut self, name: StrId) -> Option<StrId> {
        self.heroes.iter().find(|(c, _)| *c == name).map(|(_, h)| *h)
    }
    fn living(&mut self, _: f64) -> Option<f64> {
        unreachable!()
    }
    fn building_alive(&mut self, _: Ref<Building>) -> bool {
        unreachable!()
    }
    fn members(&mut self) -> Vec<Member> {
        unreachable!("not in these frames")
    }
    fn building_attacker(&mut self, _: Ref<Building>) -> Option<Ref<Character>> {
        unreachable!("not in these frames")
    }
    fn vehicle_attacker(&mut self, _: Ref<Vehicle>) -> Option<Ref<Character>> {
        unreachable!("not in these frames")
    }
    fn kill_region_victims(&mut self) -> Vec<Victim> {
        unreachable!("not in these frames")
    }
    fn kill_victim(&mut self, _: &mut Store, _: Victim) {
        unreachable!("not in these frames")
    }
    fn effect(&mut self, _: &str, _: [f64; 3]) {
        unreachable!("not in these frames")
    }
    fn post_event(&mut self, _: &mut Store, _: &PostEvent) {
        unreachable!("not in these frames")
    }
    fn mobile_event(&mut self, _: &mut Store, _: &MobileEvent) {
        unreachable!("not in these frames")
    }
    fn battle_over(&mut self, _: f64) {}
    fn ai_detach(&mut self, s: &mut Store, c: Ref<Character>) {
        self.lines.push(vec!["detach".into(), bits(self.run), bits(Self::place(s, c))]);
    }
    fn ai_attach(&mut self, s: &mut Store, c: Ref<Character>) {
        self.lines.push(vec!["attach".into(), bits(self.run), bits(Self::place(s, c))]);
    }
    fn retire_unit(&mut self, s: &mut Store, c: Ref<Character>, _: Ref<Unit>) {
        self.lines.push(vec!["remove".into(), bits(self.run), bits(Self::place(s, c))]);
    }
}

fn regions(ops: &Value, t: &mut Trace) {
    for (i, r) in arr(ops, "regions").iter().enumerate() {
        let (desc, other) = region(r.get("region").unwrap());
        let built = build_region(&desc);
        let kind = if other { r.get("region").unwrap().get("type").and_then(Value::as_str).unwrap() } else { RegionType::NAMES[built.r#type as usize] };
        let mut parts = vec!["region".to_string(), bits(i as f64), kind.to_string()];
        parts.extend(built.half.iter().map(|x| bits(*x)));
        parts.extend([built.radius, built.center[0], built.center[1], built.center[2]].iter().map(|x| bits(*x)));
        parts.extend(built.inverse.iter().map(|x| bits(*x)));
        t.line(&parts);
        for (j, p) in arr(r, "points").iter().enumerate() {
            t.line(&["in".into(), bits(i as f64), bits(j as f64), b(!other && in_region(&built, &vec3(p)))]);
        }
    }
}

fn kills(ops: &Value, t: &mut Trace) {
    for (k, r) in arr(ops, "kills").iter().enumerate() {
        let draws: Vec<f64> = arr(r, "draws").iter().map(num).collect();
        let victims: Vec<Vector3> = arr(r, "victims").iter().map(vec3).collect();
        struct Host {
            draws: Vec<f64>,
            next: usize,
            effects: Vec<(String, Vector3)>,
            victims: Vec<Vector3>,
            killed: Vec<usize>,
        }
        impl bfe_sim::battle::objectives::KillRegionHost for Host {
            fn random(&mut self) -> f64 {
                self.next += 1;
                self.draws[self.next - 1]
            }
            fn play(&mut self, name: &str, point: Vector3) {
                self.effects.push((name.to_string(), point));
            }
            fn victims(&mut self) -> Vec<Vector3> {
                self.victims.clone()
            }
            fn kill(&mut self, victim: usize) {
                self.killed.push(victim);
            }
        }
        let mut host = Host { draws, next: 0, effects: Vec::new(), victims, killed: Vec::new() };
        let count = kill_region(Some(&region(r.get("region").unwrap()).0), &mut host);
        let (next, draws, effects, killed) = (host.next, host.draws, host.effects, host.killed);
        assert_eq!(next, draws.len(), "kill region {k}: the port drew {next} of the {} draws", draws.len());
        for (e, (name, p)) in effects.iter().enumerate() {
            t.line(&["blast".into(), bits(k as f64), bits(e as f64), name.clone(), bits(p.x), bits(p.y), bits(p.z)]);
        }
        let mut parts = vec!["killed".to_string(), bits(k as f64), bits(count as f64)];
        parts.extend(killed.iter().map(|i| bits(*i as f64)));
        t.line(&parts);
    }
}

fn posts(ops: &Value, t: &mut Trace) {
    for (r, run) in arr(ops, "posts").iter().enumerate() {
        let rules = run.get("rules").unwrap();
        let (neutralize_time, capture_time) = (num(rules.get("neutralizeTime").unwrap()), num(rules.get("captureTime").unwrap()));
        let mut s = Store::default();
        let mut strings = Strings::default();
        for (i, p) in arr(run, "posts").iter().enumerate() {
            let capture = p.get("capture").filter(|c| c.as_array().is_none() && c.get("type").is_some()).map(|c| build_region(&region(c).0));
            let post = Post { name: strings.intern_str(&format!("cp{i}")), team: num(p.get("team").unwrap()), capture, neutralize_time, capture_time, active: true, ..Default::default() };
            let r = s.post.insert(post);
            s.command_posts.posts.push(r);
        }
        let mut world = Twin { capture: arr(run, "capture").iter().map(num).collect(), heroes: Vec::new(), units: Vec::new(), run: 0.0, lines: Vec::new() };
        for (k, tick) in arr(run, "ticks").iter().enumerate() {
            let soldiers: Vec<Capturer> = arr(tick, "soldiers")
                .iter()
                .map(|s| Capturer { team: num(s.get("team").unwrap()), position: vec3(s.get("position").unwrap()) })
                .collect();
            posts::update(&mut s, num(tick.get("dt").unwrap()), &soldiers, &mut world);
            for (i, &pr) in s.command_posts.posts.iter().enumerate() {
                let post = s.post.get(pr).unwrap();
                let mut parts = vec!["post".to_string(), bits(r as f64), bits(k as f64), bits(i as f64)];
                parts.extend([post.team, post.neutralize, post.progress, post.toward].iter().map(|x| bits(*x)));
                parts.push(b(post.contested));
                parts.push(bits(remaining(post)));
                for (team, n) in post.counts.iter() {
                    parts.push(bits(team));
                    parts.push(bits(*n));
                }
                t.line(&parts);
            }
            for e in std::mem::take(&mut s.command_posts.events) {
                let (kind, from, by) = match change_of(&e) {
                    Change::Captured { by } => ("captured", "-".to_string(), bits(by)),
                    Change::Neutralized { from } => ("neutralized", bits(from), "-".to_string()),
                    Change::Destroyed { from } => ("lost", bits(from), "-".to_string()),
                };
                let place = s.command_posts.posts.iter().position(|p| *p == e.post).unwrap();
                t.line(&["event".into(), bits(r as f64), bits(k as f64), bits(place as f64), kind.into(), from, by]);
            }
        }
    }
}

fn seen(v: &Value, strings: &mut Strings) -> Option<Seen> {
    if v.as_object().is_none() {
        return None;
    }
    let mut str_of = |x: Option<&Value>| opt_str(x).map(|s| strings.intern_str(&s));
    let mut pair = |key: &str| -> Option<(Option<StrId>, Option<StrId>)> {
        let p = v.get(key)?;
        p.as_object()?;
        Some((str_of(p.get("key")), str_of(p.get("label"))))
    };
    let weapon = pair("weapon");
    let vehicle = pair("vehicle");
    let class_label = opt_str(v.get("classLabel")).map(|s| strings.intern_str(&s));
    Some(Seen { position: v.get("position").filter(|p| p.as_array().is_some()).map(nums::<3>), health: v.get("health").filter(|h| h.as_str().is_some()).map(num), weapon, vehicle, class_label })
}

fn stats(ops: &Value, t: &mut Trace) {
    for (r, run) in arr(ops, "stats").iter().enumerate() {
        let mut strings = Strings::default();
        let mut characters = bfe_sim::store::Slab::<Character>::default();
        let list: Vec<Ref<Character>> = arr(run, "classes")
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut ch = new_character(i as f64, if i < 4 { 1.0 } else { 2.0 }, false, &mut strings);
                ch.class_name = opt_str(Some(c)).map(|s| strings.intern_str(&s));
                characters.insert(ch)
            })
            .collect();
        let place_of = |c: Ref<Character>| list.iter().position(|x| *x == c).unwrap() as f64;
        let mut stats = new_stats();
        let place = |v: Option<&Value>| v.and_then(Value::as_f64).map(|x| list[x as usize]);
        for (k, op) in arr(run, "ops").iter().enumerate() {
            match op.get("op").and_then(Value::as_str).unwrap() {
                "kill" => {
                    let relation = op.get("relation").and_then(Value::as_f64).unwrap();
                    stats::kill(&mut stats, &mut characters, place(op.get("killer")), place(op.get("victim")), relation);
                }
                "death" => stats::death(&mut characters, place(op.get("who"))),
                "head" => stats::headshot(&mut characters, place(op.get("who"))),
                "vehicle" => stats::vehicle_destroyed(&mut characters, place(op.get("who"))),
                "class" => {
                    let who = place(op.get("who")).unwrap();
                    let name = opt_str(op.get("name")).map(|s| strings.intern_str(&s));
                    characters.get_mut(who).unwrap().class_name = name;
                }
                "tick" => {
                    let active: Vec<bool> = arr(op, "active").iter().map(Value::truthy).collect();
                    let seen: Vec<Option<Seen>> = arr(op, "seen").iter().map(|v| seen(v, &mut strings)).collect();
                    stats::tick(&mut stats, num(op.get("dt").unwrap()), &list, &mut characters, &strings, |c, _| active[place_of(c) as usize], |c, _| seen[place_of(c) as usize].clone());
                }
                other => panic!("an op {other}"),
            }
            if k % 5 != 4 {
                continue;
            }
            for (ci, &cr) in list.iter().enumerate() {
                let c = characters.get(cr).unwrap();
                let Nullable::Value(rec) = &c.record else {
                    t.line(&["record".into(), bits(r as f64), bits(k as f64), bits(ci as f64), "-".into()]);
                    continue;
                };
                let mut parts = vec!["record".to_string(), bits(r as f64), bits(k as f64), bits(ci as f64), bits(rec.health), b(rec.low)];
                match &rec.anchor {
                    Some(a) => parts.extend([a.x, a.y, a.z].iter().map(|x| bits(*x))),
                    None => parts.push("-".into()),
                }
                t.line(&parts);
                for (si, s) in rec.slots.iter().enumerate() {
                    let head = vec![bits(r as f64), bits(k as f64), bits(ci as f64), bits(si as f64)];
                    let mut parts = vec!["slot".to_string()];
                    parts.extend(head.clone());
                    parts.extend(
                        [s.team_kills, s.vehicles_destroyed, s.spree, s.survivalist, s.life_kills, s.life_kills_best, s.headshots, s.deaths, s.life, s.life_best, s.camp, s.camp_best, s.escapes]
                            .iter()
                            .map(|x| bits(*x)),
                    );
                    t.line(&parts);
                    let mut parts = vec!["victims".to_string()];
                    parts.extend(head.clone());
                    for (v, m) in s.victims.iter() {
                        parts.push(bits(place_of(v)));
                        parts.push(bits(*m));
                    }
                    t.line(&parts);
                    for (name, table) in [("classes", &s.classes), ("weapons", &s.weapons), ("vehicles", &s.vehicles)] {
                        let mut parts = vec![name.to_string()];
                        parts.extend(head.clone());
                        for e in table.iter() {
                            parts.push(text(&strings, e.key));
                            parts.push(e.label.map(|l| text(&strings, l)).unwrap_or_else(|| "-".into()));
                            parts.push(bits(e.time));
                        }
                        t.line(&parts);
                    }
                }
            }
            let ring = &stats.ring;
            t.line(&["ring".into(), bits(r as f64), bits(k as f64), bits(stats.time), bits(ring.head), bits(ring.tail)]);
        }
    }
}

fn path(v: Option<&Value>) -> Option<SpawnPath> {
    let v = v?;
    v.as_object()?;
    Some(SpawnPath { present: v.get("present").map(Value::truthy).unwrap_or(false), points: arr(v, "points").iter().map(nums::<4>).collect() })
}

fn opt_bits(x: Option<f64>) -> String {
    x.map(bits).unwrap_or_else(|| "-".into())
}

fn rosters(ops: &Value, t: &mut Trace) {
    for (r, run) in arr(ops, "rosters").iter().enumerate() {
        let calls: Vec<Call> = arr(run, "calls")
            .iter()
            .map(|c| Call {
                name: c.get("name").and_then(Value::as_str).unwrap().to_string(),
                args: arr(c, "args")
                    .iter()
                    .map(|a| match (a.get("n"), a.get("s").and_then(Value::as_str)) {
                        (Some(x), _) => Arg::Num(num(x)),
                        (None, Some(s)) => Arg::Str(s.to_string()),
                        _ => Arg::Other,
                    })
                    .collect(),
                ..Default::default()
            })
            .collect();
        let names = run.get("names").filter(|n| n.as_object().is_some()).map(|n| NameLists {
            sides: (0..5)
                .map(|i| {
                    let side = n.get(&i.to_string());
                    let list = |key: &str| side.map(|s| arr(s, key).iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    (list("first"), list("last"))
                })
                .collect(),
            local: opt_str(n.get("local")),
        });
        let heroes = run.get("heroes").map(Value::truthy).unwrap_or(false);
        let mut strings = Strings::default();
        let mut s = Store::default();
        let (spawning, mut naming) = spawning::new_spawning(&calls, num(run.get("unitScale").unwrap()), heroes, false, names, TEAM_CAPACITY, &|_| true, &mut strings);
        let classes: Vec<StrId> = spawning.teams.iter().flat_map(|t| t.classes.iter().map(|c| c.name)).collect();
        let heroes_named = classes.into_iter().map(|c| (c, strings.intern_str(&text(&strings, c).to_uppercase()))).collect();
        let mut world = Twin { capture: Vec::new(), heroes: heroes_named, units: Vec::new(), run: r as f64, lines: Vec::new() };
        s.spawning = spawning;
        s.battle.stats = new_stats();
        for p in arr(run, "posts") {
            let ai = p.get("ai").unwrap();
            let post = Post {
                team: num(p.get("team").unwrap()),
                active: p.get("active").map(Value::truthy).unwrap_or(false),
                position: nums::<3>(p.get("position").unwrap()),
                ai: Some(PostAi {
                    spawn_path: path(ai.get("spawnPath")),
                    ally_path: path(ai.get("allyPath")),
                    ally_count: num(ai.get("allyCount").unwrap()),
                    class_spawn_points: num(ai.get("classSpawnPoints").unwrap()),
                    class_spawn_world: Vec::new(),
                    yaw: num(ai.get("yaw").unwrap()),
                }),
                ..Default::default()
            };
            let pr = s.post.insert(post);
            s.command_posts.posts.push(pr);
        }
        let humans: Vec<HumanSpec> = arr(run, "humans")
            .iter()
            .map(|h| HumanSpec { team: num(h.get("team").unwrap()), id: h.get("id").and_then(Value::as_f64), name: opt_str(h.get("name")) })
            .collect();
        let mut random = jsrt::rng::ClassicRandom::new(run.get("seed").and_then(Value::as_f64).unwrap());
        spawning::begin(&mut s, &humans, &mut naming, &mut strings, &mut world, &mut random);
        let place_of_post = |p: SpawnPost| match p {
            SpawnPost::Post(pr) => s.command_posts.posts.iter().position(|x| *x == pr).unwrap() as f64,
            SpawnPost::MobilePost(_) => unreachable!(),
        };
        let place_of = |c: Ref<Character>| s.battle.characters.iter().position(|x| *x == c).unwrap() as f64;
        let rb = bits(r as f64);
        let sp = &s.spawning;
        t.line(&["delays".into(), rb.clone(), bits(sp.a), bits(sp.b), bits(sp.q), bits(sp.wave_countdown), bits(random.state as f64)]);
        for team in &sp.teams {
            let mut parts = vec!["team".to_string(), rb.clone(), bits(team.index), team.name.map(|n| text(&strings, n)).unwrap_or_else(|| "-".into())];
            parts.extend([team.faction, team.unit_count, team.tickets, team.hero_count].iter().map(|x| bits(*x)));
            parts.push(team.hero_class.map(|n| text(&strings, n)).unwrap_or_else(|| "-".into()));
            parts.extend([team.bonus, team.aggressiveness].iter().map(|x| bits(*x)));
            parts.extend(team.relations.iter().map(|x| bits(*x as f64)));
            parts.push(bits(team.capacity));
            for c in &team.classes {
                parts.push(text(&strings, c.name));
                parts.push(bits(c.target));
                parts.push(b(c.hero));
            }
            parts.push("|".into());
            parts.extend(team.characters.iter().map(|c| bits(place_of(*c))));
            t.line(&parts);
        }
        for (i, &cr) in s.battle.characters.iter().enumerate() {
            let c = s.character.get(cr).unwrap();
            let spawn = c.spawn.as_ref().unwrap();
            let post = match spawn.post {
                PostRef::Post(pr) => Some(place_of_post(SpawnPost::Post(pr))),
                PostRef::MobilePost(_) => unreachable!(),
                PostRef::Null | PostRef::Undefined => None,
            };
            let player = match c.player_id {
                bfe_sim::gen::PlayerId::F64(x) => x,
                _ => f64::NAN,
            };
            let mut parts = vec!["character".to_string(), rb.clone(), bits(i as f64), bits(c.team), b(c.human), bits(c.slot.unwrap()), bits(c.manager_index.unwrap()), bits(player)];
            parts.push(c.name.map(|n| text(&strings, n)).unwrap_or_else(|| "-".into()));
            parts.push(opt_bits(match c.class_index {
                Nullable::Value(x) => Some(x),
                _ => None,
            }));
            parts.push(c.class_name.map(|n| text(&strings, n)).unwrap_or_else(|| "-".into()));
            parts.push(b(c.hero == Some(true)));
            parts.extend([spawn.timer, spawn.wave.unwrap(), spawn.ordinal.unwrap()].iter().map(|x| bits(*x)));
            parts.push(opt_bits(post));
            parts.push(b(spawn.ready));
            t.line(&parts);
        }
        for (p, q) in s.spawning.queues.iter() {
            let mut parts = vec!["queue".to_string(), rb.clone(), bits(place_of_post(p))];
            parts.extend(q.iter().map(|x| bits(*x)));
            t.line(&parts);
        }
        multiplayer(run.get("multiplayer").unwrap(), r, &mut s, &mut naming, &mut strings, &mut world, &mut random, t);
    }
}

/// A Character's id as the trace writes it.
fn player_text(id: &PlayerId, strings: &Strings) -> String {
    match id {
        PlayerId::F64(x) => bits(*x),
        PlayerId::Str(s) => text(strings, *s),
        PlayerId::Undefined => "-".into(),
    }
}

/// The roster run on a battle after its match start: the bodies, the roster
/// made, then its operations, the state after each.
#[allow(clippy::too_many_arguments)]
fn multiplayer(mp: &Value, r: usize, s: &mut Store, naming: &mut spawning::Naming, strings: &mut Strings, world: &mut Twin, random: &mut jsrt::rng::ClassicRandom, t: &mut Trace) {
    let rb = bits(r as f64);
    let list = s.battle.characters.clone();
    for u in arr(mp, "units") {
        let (i, alive) = (num(&u.as_array().unwrap()[0]) as usize, u.as_array().unwrap()[1].truthy());
        let unit = Ref::new(1000 + i as u32, 0);
        world.units.push((unit.slot, alive));
        s.character.get_mut(list[i]).unwrap().unit = Some(unit);
    }
    let flush = |world: &mut Twin, t: &mut Trace| {
        for parts in world.lines.drain(..) {
            t.line(&parts);
        }
    };
    let state = |s: &Store, strings: &Strings, k: f64, t: &mut Trace| {
        let kb = bits(k);
        let place = |c: Ref<Character>| bits(s.battle.characters.iter().position(|x| *x == c).unwrap() as f64);
        for team in [1usize, 2] {
            let tm = &s.spawning.teams[team];
            let mut parts = vec!["rteam".to_string(), rb.clone(), kb.clone(), bits(team as f64), bits(tm.capacity), bits(tm.unit_count), bits(tm.living)];
            parts.extend(tm.characters.iter().map(|c| place(*c)));
            t.line(&parts);
        }
        for (i, &cr) in s.battle.characters.iter().enumerate() {
            let c = s.character.get(cr).unwrap();
            let sp = c.spawn.as_ref().unwrap();
            let post = match sp.post {
                PostRef::Post(pr) => bits(s.command_posts.posts.iter().position(|x| *x == pr).unwrap() as f64),
                _ => "-".into(),
            };
            let class_index = match c.class_index {
                Nullable::Value(x) => bits(x),
                _ => "-".into(),
            };
            let parts = vec![
                "rchar".to_string(), rb.clone(), kb.clone(), bits(i as f64), bits(c.team), b(c.human), player_text(&c.player_id, strings),
                c.name.map(|n| text(strings, n)).unwrap_or_else(|| "-".into()), bits(c.index), c.slot.map(bits).unwrap_or_else(|| "-".into()), class_index,
                c.class_name.map(|n| text(strings, n)).unwrap_or_else(|| "-".into()), c.hero.map(b).unwrap_or_else(|| "-".into()),
                bits(sp.timer), sp.wave.map(bits).unwrap_or_else(|| "-".into()), sp.ordinal.map(bits).unwrap_or_else(|| "-".into()), post, b(sp.ready),
                c.network_dormant.map(b).unwrap_or_else(|| "-".into()), if c.unit.is_some() { bits(i as f64) } else { "-".into() }, bits(c.kills),
                if c.record == Nullable::Null { "null".into() } else { "-".into() },
            ];
            t.line(&parts);
        }
        for (p, q) in s.spawning.queues.iter() {
            let SpawnPost::Post(pr) = p else { unreachable!() };
            let mut parts = vec!["rqueue".to_string(), rb.clone(), kb.clone(), bits(s.command_posts.posts.iter().position(|x| *x == pr).unwrap() as f64)];
            parts.extend(q.iter().map(|x| bits(*x)));
            t.line(&parts);
        }
    };
    let army = mp.get("armySize").filter(|a| a.as_str().is_some()).map(num);
    let local = list.iter().copied().find(|c| s.character.get(*c).unwrap().human);
    roster::create(s, army, local, naming, strings, world, random);
    flush(world, t);
    state(s, strings, -1.0, t);
    t.line(&["rrandom".into(), rb.clone(), bits(-1.0), bits(random.state as f64)]);
    for (k, op) in arr(mp, "ops").iter().enumerate() {
        let kb = bits(k as f64);
        let now = s.battle.characters.clone();
        let who = |v: Option<&Value>| now[num(v.unwrap()) as usize];
        let result = match op.get("op").and_then(Value::as_str).unwrap() {
            "reserve" => {
                let id = op.get("id").unwrap();
                let id = match id.get("s").and_then(Value::as_str) {
                    Some(text) => PlayerId::Str(strings.intern_str(text)),
                    None => PlayerId::F64(num(id.get("n").unwrap())),
                };
                let name: Vec<u16> = op.get("name").and_then(Value::as_str).unwrap().encode_utf16().collect();
                match roster::reserve(s, num(op.get("team").unwrap()), id, &name, strings, world) {
                    Ok(c) => bits(s.battle.characters.iter().position(|x| *x == c).unwrap() as f64),
                    Err(e) => e.to_string(),
                }
            }
            "release" => {
                let c = who(op.get("who"));
                roster::release(s, c, naming, strings, world, random);
                bits(num(op.get("who").unwrap()))
            }
            _ => {
                let c = who(op.get("who"));
                b(roster::switch_team(s, c, num(op.get("target").unwrap()), naming, strings, world, random))
            }
        };
        flush(world, t);
        t.line(&["rop".into(), rb.clone(), kb.clone(), op.get("op").and_then(Value::as_str).unwrap().into(), result]);
        state(s, strings, k as f64, t);
        t.line(&["rrandom".into(), rb.clone(), kb.clone(), bits(random.state as f64)]);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: battle-twin OPS.json TRACE.out");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&args[1]).expect("the operations");
    let ops = json::parse(&text).expect("the operations' JSON");
    let mut t = Trace(String::new());
    regions(&ops, &mut t);
    kills(&ops, &mut t);
    posts(&ops, &mut t);
    stats(&ops, &mut t);
    rosters(&ops, &mut t);
    let mut out = t.0;
    if !out.ends_with('\n') {
        let _ = writeln!(out);
    }
    std::fs::write(&args[2], out).expect("the trace");
}
