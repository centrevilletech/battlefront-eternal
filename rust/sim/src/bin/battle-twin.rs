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
use bfe_sim::battle::mobile::Pad;
use bfe_sim::battle::objectives::kill_region;
use bfe_sim::battle::posts::{Capturer, Change, CommandPosts, Post, PostId, PostRules};
use bfe_sim::battle::regions::{build_region, in_region, Region, RegionType};
use bfe_sim::battle::rules::Character;
use bfe_sim::battle::stats::{BattleStats, Seen};
use bfe_sim::battle::world::{BattleWorld, Body};
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

fn region(v: &Value) -> Region {
    let matrix = v.get("matrix").filter(|m| m.as_array().is_some());
    let center = v.get("center").filter(|c| c.as_array().is_some());
    Region {
        kind: RegionType::named(v.get("type").and_then(Value::as_str).unwrap_or("")),
        matrix: matrix.map(nums::<16>),
        center: center.map(nums::<3>),
        size: nums::<3>(v.get("size").unwrap()),
    }
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

/// The world the twin's command posts ask: the capture team table.
struct Twin {
    capture: Vec<f64>,
}

impl BattleWorld for Twin {
    fn relation(&mut self, _: f64, _: f64) -> f64 {
        0.0
    }
    fn capture_team(&mut self, team: f64) -> f64 {
        self.capture.get(team as usize).copied().unwrap_or(team)
    }
    fn unit_alive(&mut self, _: u32) -> bool {
        unreachable!()
    }
    fn observe(&mut self, _: u32, _: Option<u32>) -> Option<Seen> {
        unreachable!()
    }
    fn body(&mut self, _: u32) -> Body {
        unreachable!()
    }
    fn combat_dead(&mut self, _: u32) -> bool {
        unreachable!()
    }
    fn kill(&mut self, _: u32) {
        unreachable!()
    }
    fn kill_droid(&mut self, _: u32) {
        unreachable!()
    }
    fn wreck(&mut self, _: u32) {
        unreachable!()
    }
    fn pad(&mut self, _: usize) -> Pad {
        unreachable!()
    }
    fn vehicle_position(&mut self, _: u32) -> [f64; 3] {
        unreachable!()
    }
    fn class_spawn_world(&mut self, _: usize) -> Vec<Vec<f64>> {
        unreachable!()
    }
    fn spawn_unit(&mut self, _: &mut Character, _: u32, _: &str, _: Option<PostId>, _: [f64; 3], _: Option<f64>) -> Option<u32> {
        unreachable!()
    }
    fn force_kill(&mut self, _: u32, _: u32) {
        unreachable!()
    }
    fn health(&mut self, _: u32) -> f64 {
        unreachable!()
    }
    fn request_objective(&mut self, _: &mut Character, _: u32) {
        unreachable!()
    }
    fn republish(&mut self, _: &mut Character, _: u32) {
        unreachable!()
    }
    fn hero_name(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
    fn living(&mut self, _: f64) -> Option<f64> {
        unreachable!()
    }
    fn building_alive(&mut self, _: u32) -> bool {
        unreachable!()
    }
    fn battle_over(&mut self, _: f64) {}
}

fn regions(ops: &Value, t: &mut Trace) {
    for (i, r) in arr(ops, "regions").iter().enumerate() {
        let built = build_region(&region(r.get("region").unwrap()));
        let kind = match built.kind {
            RegionType::Sphere => "sphere",
            RegionType::Box => "box",
            RegionType::Cylinder => "cylinder",
            RegionType::Other => r.get("region").unwrap().get("type").and_then(Value::as_str).unwrap(),
        };
        let mut parts = vec!["region".to_string(), bits(i as f64), kind.to_string()];
        parts.extend(built.half.iter().map(|x| bits(*x)));
        parts.extend([built.radius, built.center.x, built.center.y, built.center.z].iter().map(|x| bits(*x)));
        parts.extend(built.inverse.elements.iter().map(|x| bits(*x)));
        t.line(&parts);
        for (j, p) in arr(r, "points").iter().enumerate() {
            t.line(&["in".into(), bits(i as f64), bits(j as f64), b(in_region(&built, &vec3(p)))]);
        }
    }
}

fn kills(ops: &Value, t: &mut Trace) {
    for (k, r) in arr(ops, "kills").iter().enumerate() {
        let draws: Vec<f64> = arr(r, "draws").iter().map(num).collect();
        let victims: Vec<Vector3> = arr(r, "victims").iter().map(vec3).collect();
        let mut next = 0;
        let mut effects = Vec::new();
        let mut killed = Vec::new();
        let count = kill_region(
            Some(&region(r.get("region").unwrap())),
            || {
                next += 1;
                draws[next - 1]
            },
            |name, p| effects.push((name.to_string(), p)),
            &victims,
            |i| killed.push(i),
        );
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
        let rules = PostRules { neutralize_time: num(rules.get("neutralizeTime").unwrap()), capture_time: num(rules.get("captureTime").unwrap()) };
        let mut posts = CommandPosts { teams: true, ..Default::default() };
        for (i, p) in arr(run, "posts").iter().enumerate() {
            let capture = p.get("capture").filter(|c| c.as_array().is_none() && c.get("type").is_some()).map(|c| build_region(&region(c)));
            posts.posts.push(Post { name: format!("cp{i}"), team: num(p.get("team").unwrap()), capture, rules, active: true, ..Default::default() });
        }
        let mut world = Twin { capture: arr(run, "capture").iter().map(num).collect() };
        for (k, tick) in arr(run, "ticks").iter().enumerate() {
            let soldiers: Vec<Capturer> = arr(tick, "soldiers")
                .iter()
                .map(|s| Capturer { team: num(s.get("team").unwrap()), position: vec3(s.get("position").unwrap()) })
                .collect();
            posts.update(num(tick.get("dt").unwrap()), &soldiers, &mut world);
            for (i, post) in posts.posts.iter().enumerate() {
                let mut parts = vec!["post".to_string(), bits(r as f64), bits(k as f64), bits(i as f64)];
                parts.extend([post.team, post.neutralize, post.progress, post.toward].iter().map(|x| bits(*x)));
                parts.push(b(post.contested));
                parts.push(bits(post.remaining()));
                for (team, n) in &post.counts {
                    parts.push(bits(*team));
                    parts.push(bits(*n));
                }
                t.line(&parts);
            }
            for e in posts.events.drain(..) {
                let (kind, from, by) = match e.change {
                    Change::Captured { by } => ("captured", "-".to_string(), bits(by)),
                    Change::Neutralized { from } => ("neutralized", bits(from), "-".to_string()),
                    Change::Destroyed { from } => ("lost", bits(from), "-".to_string()),
                };
                t.line(&["event".into(), bits(r as f64), bits(k as f64), bits(e.post as f64), kind.into(), from, by]);
            }
        }
    }
}

fn seen(v: &Value) -> Option<Seen> {
    if v.as_object().is_none() {
        return None;
    }
    let pair = |key: &str| -> Option<(Option<String>, Option<String>)> {
        let p = v.get(key)?;
        p.as_object()?;
        Some((opt_str(p.get("key")), opt_str(p.get("label"))))
    };
    Some(Seen {
        position: v.get("position").filter(|p| p.as_array().is_some()).map(nums::<3>),
        health: v.get("health").filter(|h| h.as_str().is_some()).map(num),
        weapon: pair("weapon"),
        vehicle: pair("vehicle"),
        class_label: opt_str(v.get("classLabel")),
    })
}

fn stats(ops: &Value, t: &mut Trace) {
    for (r, run) in arr(ops, "stats").iter().enumerate() {
        let mut characters: Vec<Character> = arr(run, "classes")
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut ch = Character::new(i as f64, if i < 4 { 1.0 } else { 2.0 }, false);
                ch.class_name = opt_str(Some(c));
                ch
            })
            .collect();
        let mut stats = BattleStats::default();
        let place = |v: Option<&Value>| v.and_then(Value::as_f64).map(|x| x as u32);
        for (k, op) in arr(run, "ops").iter().enumerate() {
            match op.get("op").and_then(Value::as_str).unwrap() {
                "kill" => {
                    let relation = op.get("relation").and_then(Value::as_f64).unwrap();
                    stats.kill(&mut characters, place(op.get("killer")), place(op.get("victim")), relation);
                }
                "death" => stats.death(&mut characters, place(op.get("who"))),
                "head" => stats.headshot(&mut characters, place(op.get("who"))),
                "vehicle" => stats.vehicle_destroyed(&mut characters, place(op.get("who"))),
                "class" => {
                    let who = place(op.get("who")).unwrap() as usize;
                    characters[who].class_name = opt_str(op.get("name"));
                }
                "tick" => {
                    let active: Vec<bool> = arr(op, "active").iter().map(Value::truthy).collect();
                    let seen: Vec<Option<Seen>> = arr(op, "seen").iter().map(seen).collect();
                    stats.tick(num(op.get("dt").unwrap()), &mut characters, |i, _| active[i], |i, _| seen[i as usize].clone());
                }
                other => panic!("an op {other}"),
            }
            if k % 5 != 4 {
                continue;
            }
            for (ci, c) in characters.iter().enumerate() {
                let Some(rec) = &c.record else {
                    t.line(&["record".into(), bits(r as f64), bits(k as f64), bits(ci as f64), "-".into()]);
                    continue;
                };
                let mut parts = vec!["record".to_string(), bits(r as f64), bits(k as f64), bits(ci as f64), bits(rec.health), b(rec.low)];
                match rec.anchor {
                    Some(a) => parts.extend(a.iter().map(|x| bits(*x))),
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
                    for (v, m) in &s.victims {
                        parts.push(bits(*v as f64));
                        parts.push(bits(*m));
                    }
                    t.line(&parts);
                    for (name, table) in [("classes", &s.classes), ("weapons", &s.weapons), ("vehicles", &s.vehicles)] {
                        let mut parts = vec![name.to_string()];
                        parts.extend(head.clone());
                        for e in table.iter() {
                            parts.push(e.key.clone());
                            parts.push(e.label.clone().unwrap_or_else(|| "-".into()));
                            parts.push(bits(e.time));
                        }
                        t.line(&parts);
                    }
                }
            }
            let ring = &stats.ring;
            t.line(&["ring".into(), bits(r as f64), bits(k as f64), bits(stats.time), bits(ring.head as f64), bits(ring.tail as f64)]);
        }
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
    let mut out = t.0;
    if !out.ends_with('\n') {
        let _ = writeln!(out);
    }
    std::fs::write(&args[2], out).expect("the trace");
}
