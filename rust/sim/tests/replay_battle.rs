//! The battle package's native replays (V2): each captured frame's state
//! (tools/sim-capture/battle.mjs: the store's battle, command posts,
//! command vehicles' posts and spawn manager, as the generated exporters
//! wrote them) is read into the store (`gen::Replay`), the port runs the
//! frame, and the store's state is written back and compared with the
//! capture's end state, every field. What the frame reads of the rest of
//! the world comes from the capture's plain values; its calls across the
//! boundary are served from the frame's log (checked in order), with the
//! state they left where the frame goes on reading it; its draws come from
//! the log in order.
//!
//!   BFE_REPLAY_CORPUS=<dir> cargo test --release -p sim --features replay --test replay_battle -- --nocapture
#![cfg(feature = "replay")]

use bfe_sim::ai::nav::boundary::Boundary;
use bfe_sim::ai::nav::math::Vec3;
use bfe_sim::battle::battlefield::Battlefield;
use bfe_sim::battle::mobile::{ClassRecord, Hull, MobileEvent, Pad};
use bfe_sim::battle::phase::update_battle;
use bfe_sim::battle::posts::{self, Capturer};
use bfe_sim::battle::rules;
use bfe_sim::battle::spawning::{self, Draws};
use bfe_sim::battle::stats::Seen;
use bfe_sim::battle::world::{BattleWorld, Body, Member, Victim};
use bfe_sim::gen::{BattleStats, Building, Character, MobilePost, Post, PostEvent, Replay, SpawnPost, Spawning, Store, Unit, Vehicle, KIND_BATTLE, KIND_COMMAND_POSTS, KIND_MOBILE_POSTS, KIND_SPAWNING};
use bfe_sim::store::replay::{object_entries, index_key};
use bfe_sim::store::{IntObj, Ref, StrId, Strings};
use sim_replay::{check_value, replay_corpus, Arena, Call, Capture, Corpus, Frame, Log, Mismatch, Outcome, V};
use std::cell::RefCell;
use std::rc::Rc;
use three::Vector3;

// ---- Plain values.

fn num(v: V) -> f64 {
    v.opt_num().unwrap_or(f64::NAN)
}

fn opt_num(v: V) -> Option<f64> {
    v.opt_num()
}

fn nums<const N: usize>(v: V) -> [f64; N] {
    let mut out = [f64::NAN; N];
    for (i, x) in v.items().take(N).enumerate() {
        out[i] = num(x);
    }
    out
}

fn opt_str(v: V, strings: &mut Strings) -> Option<StrId> {
    v.str().map(|s| strings.intern_str(s))
}

fn place(v: V) -> Option<usize> {
    opt_num(v).filter(|x| *x >= 0.0).map(|x| x as usize)
}

// ---- The frame's state in a store.

/// The frame's state read into a replay context: the battle (its
/// Characters), the command posts, the command vehicles' posts, and the
/// spawn manager when the battle has one.
fn read_state(state: V) -> Result<(Replay, bool), String> {
    let mut cx = Replay::default();
    cx.read_value(KIND_BATTLE, state.get("battle"))?;
    cx.read_value(KIND_COMMAND_POSTS, state.get("posts"))?;
    cx.read_value(KIND_MOBILE_POSTS, state.get("mobile"))?;
    let ai = !state.get("spawning").is_nullish();
    if ai {
        cx.read_value(KIND_SPAWNING, state.get("spawning"))?;
    }
    Ok((cx, ai))
}

/// The store's state against the capture's end state, root by root.
fn compare(cx: &mut Replay, after: V, ai: bool) -> Result<(), Mismatch> {
    let mut roots = vec![("battle", KIND_BATTLE), ("posts", KIND_COMMAND_POSTS), ("mobile", KIND_MOBILE_POSTS)];
    if ai {
        roots.push(("spawning", KIND_SPAWNING));
    }
    for (root, kind) in roots {
        let mut arena = Arena::default();
        let mine = cx.write_value(kind, &mut arena);
        check_value(root, after.get(root), arena.view(&mine))?;
    }
    Ok(())
}

/// The Character at a place in the battle's list.
fn character_at(s: &Store, place: Option<usize>) -> Option<Ref<Character>> {
    place.and_then(|i| s.battle.characters.get(i).copied())
}

// ---- What a served call left.

/// A served call's state after it: every Character, the scores' ring and
/// the reinforcements (the death path's credit), the spawn manager (its
/// onDeath), the mobile posts refreshed by the AI, and the bodies a
/// battlefield kill left.
struct After {
    characters: Vec<(Ref<Character>, Character)>,
    mobile: Vec<(Ref<MobilePost>, MobilePost)>,
    stats: BattleStats,
    reinforcements: IntObj<f64>,
    spawning: Option<Spawning>,
    bodies: Option<Vec<(f64, Body)>>,
}

fn int_obj(v: V, path: &str) -> Result<IntObj<f64>, String> {
    let mut out = IntObj::default();
    for (key, x) in object_entries(v, path)? {
        out.set(index_key(key, path)?, num(x));
    }
    Ok(out)
}

fn body(v: V, cx: &mut Replay) -> Result<Option<(f64, Body)>, String> {
    if v.is_nullish() {
        return Ok(None);
    }
    let vehicle = if v.get("vehicle").is_nullish() { None } else { Some(Vehicle::ref_in(v.get("vehicle"), cx, "body.vehicle")?) };
    let droid = if v.get("droid").is_nullish() { None } else { Some(nums::<3>(v.get("droid"))) };
    Ok(Some((num(v.get("id")), Body { removed: v.get("removed").bool(), alive: v.get("alive").bool(), combat_dead: v.get("combatDead").bool(), place: nums::<3>(v.get("place")), vehicle, droid })))
}

/// A call's state after it, read through the context's handles (none of it
/// written into the store yet).
fn after_of(call: &Call, cx: &mut Replay) -> Result<After, String> {
    let x = call.extra();
    let mut characters = Vec::new();
    for (i, c) in x.get("battle").get("characters").items().enumerate() {
        let path = format!("after.characters[{i}]");
        let r = Character::ref_in(c.get("$id"), cx, &path)?;
        characters.push((r, Character::from_value(c, cx, &path)?));
    }
    let mut mobile = Vec::new();
    for (i, p) in x.get("mobile").get("posts").items().enumerate() {
        let path = format!("after.mobile.posts[{i}]");
        let r = MobilePost::ref_in(p.get("$id"), cx, &path)?;
        mobile.push((r, MobilePost::from_value(p, cx, &path)?));
    }
    let stats = BattleStats::from_value(x.get("battle").get("stats"), cx, "after.stats")?;
    let reinforcements = int_obj(x.get("battle").get("reinforcements"), "after.reinforcements")?;
    let spawning = if x.get("spawning").is_nullish() { None } else { Some(Spawning::from_value(x.get("spawning"), cx, "after.spawning")?) };
    let bodies = if x.has("bodies") {
        let mut out = Vec::new();
        for b in x.get("bodies").items() {
            out.extend(body(b, cx)?);
        }
        Some(out)
    } else {
        None
    };
    Ok(After { characters, mobile, stats, reinforcements, spawning, bodies })
}

// ---- The world, served from a frame.

/// What a frame's state says of the rest of the world, and its calls.
#[derive(Default)]
struct Tables {
    capture_teams: Vec<f64>,
    relations: Vec<Vec<f64>>,
    living: Vec<Option<f64>>,
    seen: Vec<(Ref<Character>, Option<Seen>)>,
    alive: Vec<(f64, bool)>,
    health: Vec<(f64, f64)>,
    hero_names: Vec<(StrId, Option<StrId>)>,
    pads: Vec<(f64, Pad)>,
    positions: Vec<(Ref<Vehicle>, [f64; 3])>,
    class_spawn_worlds: Vec<(Ref<MobilePost>, Vec<[f64; 4]>)>,
    buildings: Vec<(Ref<Building>, bool)>,
    bodies: Vec<(f64, Body)>,
    members: Vec<Member>,
    building_attackers: Vec<(Ref<Building>, Option<Ref<Character>>)>,
    hull_attackers: Vec<(Ref<Vehicle>, Option<Ref<Character>>)>,
    victims: Vec<Victim>,
    /// The class names spawnUnit was called with, interned.
    spawn_classes: Vec<(String, StrId)>,
}

fn hull(v: V, cx: &mut Replay) -> Result<Option<(Hull, [f64; 3])>, String> {
    if v.is_nullish() {
        return Ok(None);
    }
    let r = v.get("record");
    let list = |x: V| (!x.is_nullish()).then(|| x.items().map(num).collect::<Vec<f64>>());
    let record = (!r.is_nullish()).then(|| ClassRecord {
        atk: list(r.get("values").get("atk")),
        def: list(r.get("values").get("def")),
        value_bleed: opt_num(r.get("valueBleed")),
        spawn_points: r.get("spawnPoints").items().map(|p| [0, 1, 2, 3].map(|i| opt_num(p.at(i)).unwrap_or(0.0))).collect(),
    });
    let id = Vehicle::ref_in(v.get("id"), cx, "hull.id")?;
    let label = opt_str(v.get("label"), &mut cx.strings).unwrap_or_default();
    let name = opt_str(v.get("name"), &mut cx.strings).unwrap_or_default();
    let p = v.get("position");
    let position = [num(p.get("x")), num(p.get("y")), num(p.get("z"))];
    let position = if p.vec3().is_some() { p.vec3().unwrap() } else { position };
    Ok(Some((
        Hull {
            id,
            alive: v.get("alive").bool(),
            command: v.get("command").bool(),
            generation: num(v.get("generation")),
            team: num(v.get("team")),
            controlling_team: num(v.get("controllingTeam")),
            seated: v.get("seated").bool(),
            yaw: num(v.get("yaw")),
            name,
            label,
            radius: num(v.get("radius")),
            speed: num(v.get("speed")),
            value_bleed_setting: num(v.get("valueBleedSetting")),
            record,
        },
        position,
    )))
}

fn seen(v: V, strings: &mut Strings) -> Option<Seen> {
    if v.is_nullish() {
        return None;
    }
    let mut pair = |p: V| (!p.is_nullish()).then(|| (opt_str(p.at(0), strings), opt_str(p.at(1), strings)));
    let weapon = pair(v.get("weapon"));
    let vehicle = pair(v.get("vehicle"));
    Some(Seen {
        position: (!v.get("position").is_nullish()).then(|| nums::<3>(v.get("position"))),
        health: opt_num(v.get("health")),
        weapon,
        vehicle,
        class_label: opt_str(v.get("classLabel"), strings),
    })
}

/// The world tables from a frame's state (whatever of them it holds).
fn tables(state: V, frame: &Frame, cx: &mut Replay) -> Result<Tables, String> {
    let mut t = Tables { capture_teams: state.get("captureTeams").items().map(num).collect(), ..Default::default() };
    t.relations = state.get("relations").items().map(|row| row.items().map(num).collect()).collect();
    t.living = state.get("living").items().map(opt_num).collect();
    let characters = cx.store.battle.characters.clone();
    for (i, v) in state.get("seen").items().enumerate() {
        let s = seen(v, &mut cx.strings);
        t.seen.push((characters[i], s));
    }
    t.alive = state.get("alive").items().filter(|a| !a.is_nullish()).map(|a| (num(a.at(0)), a.at(1).bool())).collect();
    t.health = state.get("health").items().filter(|h| !h.is_nullish()).map(|h| (num(h.at(0)), num(h.at(1)))).collect();
    for p in state.get("heroNames").items() {
        let class = opt_str(p.at(0), &mut cx.strings).unwrap_or_default();
        let name = opt_str(p.at(1), &mut cx.strings);
        t.hero_names.push((class, name));
    }
    let mobile = cx.store.mobile_posts.posts.clone();
    for (i, pad) in state.get("pads").items().enumerate() {
        let post = mobile[i];
        let slot = cx.store.mobile_post.get(post).unwrap().index;
        let mut variants = Vec::new();
        for v in pad.get("variants").items() {
            if let Some((h, at)) = hull(v, cx)? {
                t.positions.push((h.id, at));
                variants.push(h);
            }
        }
        let vehicle = match hull(pad.get("vehicle"), cx)? {
            Some((h, at)) => {
                t.positions.push((h.id, at));
                Some(h)
            }
            None => None,
        };
        if !pad.get("last").is_nullish() {
            let id = Vehicle::ref_in(pad.get("last").get("id"), cx, "pad.last")?;
            let at = pad.get("last").get("position").vec3().unwrap_or([f64::NAN; 3]);
            t.positions.push((id, at));
        }
        t.pads.push((slot, Pad { variants, vehicle }));
        t.class_spawn_worlds.push((post, pad.get("classSpawnWorld").items().map(nums::<4>).collect()));
    }
    let placed = cx.store.command_posts.posts.clone();
    for (i, a) in state.get("buildings").items().enumerate() {
        if let (Some(b), false) = (cx.store.post.get(placed[i]).unwrap().building, a.is_nullish()) {
            t.buildings.push((b, a.bool()));
        }
    }
    for b in state.get("bodies").items() {
        t.bodies.extend(body(b, cx)?);
    }
    for m in state.get("members").items() {
        t.members.push(Member {
            soldier: m.get("soldier").bool(),
            alive: m.get("alive").bool(),
            removed: m.get("removed").bool(),
            seated: m.get("seated").bool(),
            held: m.get("held").bool(),
            captures: m.get("captures").bool(),
            player_mounted: m.get("playerMounted").bool(),
            team: num(m.get("team")),
            position: nums::<3>(m.get("position")),
            character: character_at(&cx.store, place(m.get("character"))),
        });
    }
    for (i, a) in state.get("attackers").items().enumerate() {
        if let Some(b) = cx.store.post.get(placed[i]).unwrap().building {
            t.building_attackers.push((b, character_at(&cx.store, place(a))));
        }
    }
    for (i, a) in state.get("hulls").items().enumerate() {
        if let Some(v) = cx.store.mobile_post.get(mobile[i]).unwrap().reported.vehicle {
            t.hull_attackers.push((v, character_at(&cx.store, place(a))));
        }
    }
    for v in state.get("victims").items() {
        let at = nums::<3>(v.get("position"));
        t.victims.push(match opt_num(v.get("unit")) {
            Some(id) => Victim::Unit { id: Ref::new(id as u32, 0), position: at },
            None => Victim::Hull { id: Vehicle::ref_in(v.get("hull"), cx, "victim.hull")?, position: at },
        });
    }
    for call in frame.calls().filter(|c| c.name == "spawning.spawnUnit") {
        if let Some(name) = call.arg(1).str() {
            let id = cx.strings.intern_str(name);
            t.spawn_classes.push((name.to_string(), id));
        }
    }
    Ok(t)
}

/// The frame's world: its tables, and its calls served in order with the
/// state each left (read before the frame ran).
struct Served<'f> {
    log: Rc<RefCell<Log<'f>>>,
    t: Tables,
    afters: Vec<After>,
    next: usize,
    /// Whether the game has an AI (ai.postEvent), and its objectives (the
    /// spawn manager's requests).
    ai: bool,
    serves_objectives: bool,
    failure: Option<String>,
}

impl<'f> Served<'f> {
    fn new(frame: &'f Frame, t: Tables, afters: Vec<After>) -> Served<'f> {
        // The calls the capture can hold: the objectives' only when the
        // manager had them (spawning.js `this.objectives?.request`).
        let serves_objectives = frame.before().get("objectives").truthy();
        Served { log: Rc::new(RefCell::new(Log::new(frame))), t, afters, next: 0, ai: frame.before().get("ai").truthy(), serves_objectives, failure: None }
    }

    fn fail(&mut self, what: String) {
        self.failure.get_or_insert(what);
    }

    /// The next call, which must be `label`, and the state it left.
    fn call(&mut self, label: &str) -> Option<(&'f Call, usize)> {
        let log = self.log.clone();
        let result = log.borrow_mut().call(label);
        match result {
            Ok(call) => {
                self.next += 1;
                Some((call, self.next - 1))
            }
            Err(Mismatch(m)) => {
                self.fail(m);
                None
            }
        }
    }

    /// A kill's death path, as the call left it: every Character (but his
    /// countdown outside the battlefield, the frame's own), the scores'
    /// ring, the reinforcements and the spawn manager; and the bodies.
    fn apply_death(&mut self, s: &mut Store, at: usize) {
        let after = &self.afters[at];
        for (r, c) in &after.characters {
            let keep = s.character.get(*r).map(|x| x.out_of_bounds);
            if let Some(slot) = s.character.get_mut(*r) {
                *slot = c.clone();
                slot.out_of_bounds = keep.unwrap();
            }
        }
        s.battle.stats = after.stats.clone();
        s.battle.reinforcements = after.reinforcements.clone();
        if let Some(sp) = &after.spawning {
            s.spawning = sp.clone();
        }
        if let Some(bodies) = &after.bodies {
            self.t.bodies = bodies.clone();
        }
        self.apply_mobile_refresh(s, at);
    }

    /// Foreign objective and death callbacks can list the posts, which
    /// refreshes command vehicles' cached state. Serve only refresh's
    /// fields; the post identities and event history remain the port's.
    fn apply_mobile_refresh(&mut self, s: &mut Store, at: usize) {
        for (r, theirs) in &self.afters[at].mobile {
            let mine = s.mobile_post.get_mut(*r).expect("the call's mobile post");
            mine.label = theirs.label;
            mine.team = theirs.team;
            mine.active = theirs.active;
            mine.vehicle = theirs.vehicle;
            mine.life = theirs.life;
            mine.yaw = theirs.yaw;
            mine.radius = theirs.radius;
            mine.speed = theirs.speed;
            mine.last_position = theirs.last_position;
            mine.desc = theirs.desc.clone();
            mine.ai = theirs.ai.clone();
        }
    }

    /// A served call's fields of the Character it was for, as it left them.
    fn apply_fields(&mut self, s: &mut Store, at: usize, who: Ref<Character>, fields: &[&str]) {
        let after = &self.afters[at];
        let Some((_, theirs)) = after.characters.iter().find(|(r, _)| *r == who) else {
            self.fail(format!("a served call: Character {who:?} is not in the state it left"));
            return;
        };
        let mine = s.character.get_mut(who).expect("the call's Character");
        for f in fields {
            match *f {
                "unit" => mine.unit = theirs.unit,
                "className" => mine.class_name = theirs.class_name,
                "objective" => mine.objective = theirs.objective,
                _ => unreachable!(),
            }
        }
    }

    fn team(table: &[f64], team: f64) -> Option<f64> {
        (team >= 0.0 && team.fract() == 0.0).then(|| table.get(team as usize).copied()).flatten()
    }
}

/// The shared generator A (or the gameplay stream), served from the frame's
/// draws.
struct LogDraws<'f> {
    log: Rc<RefCell<Log<'f>>>,
    stream: &'static str,
    failure: Option<String>,
}

impl Draws for LogDraws<'_> {
    fn next(&mut self) -> f64 {
        match self.log.borrow_mut().draw(self.stream) {
            Ok(v) => v,
            Err(Mismatch(m)) => {
                self.failure.get_or_insert(m);
                0.0
            }
        }
    }
}

impl BattleWorld for Served<'_> {
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        match (a >= 0.0 && a < 8.0 && a.fract() == 0.0, b >= 0.0 && b < 8.0 && b.fract() == 0.0) {
            (true, true) if !self.t.relations.is_empty() => self.t.relations[a as usize][b as usize],
            _ => {
                self.fail(format!("relation({a}, {b}): not captured"));
                0.0
            }
        }
    }
    fn capture_team(&mut self, team: f64) -> f64 {
        Self::team(&self.t.capture_teams, team).unwrap_or_else(|| {
            self.fail(format!("captureTeam({team}): not captured"));
            team
        })
    }
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool {
        match self.t.alive.iter().find(|(u, _)| *u == unit.slot as f64) {
            Some((_, a)) => *a,
            None => {
                self.fail(format!("unit {}'s alive: not captured", unit.slot));
                false
            }
        }
    }
    fn observe(&mut self, character: Ref<Character>, _: Option<Ref<Unit>>) -> Option<Seen> {
        self.t.seen.iter().find(|(c, _)| *c == character).and_then(|(_, s)| s.clone())
    }
    fn body(&mut self, unit: Ref<Unit>) -> Body {
        match self.t.bodies.iter().find(|(u, _)| *u == unit.slot as f64) {
            Some((_, b)) => *b,
            None => {
                self.fail(format!("unit {}'s body: not captured", unit.slot));
                Body::default()
            }
        }
    }
    fn combat_dead(&mut self, unit: Ref<Unit>) -> bool {
        self.body(unit).combat_dead
    }
    fn kill(&mut self, s: &mut Store, _: Ref<Unit>) {
        if let Some((_, at)) = self.call("combat.damage") {
            self.apply_death(s, at);
        }
    }
    fn kill_droid(&mut self, s: &mut Store, _: Ref<Unit>) {
        if let Some((_, at)) = self.call("remotes.kill") {
            self.apply_death(s, at);
        }
    }
    fn wreck(&mut self, s: &mut Store, _: Ref<Vehicle>) {
        if let Some((_, at)) = self.call("vehicle.damage") {
            self.apply_death(s, at);
        }
    }
    fn pad(&mut self, slot: f64) -> Pad {
        self.t.pads.iter().find(|(s, _)| *s == slot).map(|(_, p)| p.clone()).unwrap_or_default()
    }
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3] {
        match self.t.positions.iter().find(|(v, _)| *v == vehicle) {
            Some((_, at)) => *at,
            None => {
                self.fail(format!("hull {vehicle:?}'s position: not captured"));
                [0.0; 3]
            }
        }
    }
    fn class_spawn_world(&mut self, post: Ref<MobilePost>) -> Vec<[f64; 4]> {
        self.t.class_spawn_worlds.iter().find(|(p, _)| *p == post).map(|(_, w)| w.clone()).unwrap_or_default()
    }
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, _post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        let (call, after) = self.call("spawning.spawnUnit")?;
        // Its arguments: (character, className, post, [x, y, z], yaw).
        let args = call.args();
        let theirs = args.at(1).str().and_then(|n| self.t.spawn_classes.iter().find(|(t, _)| t == n).map(|(_, id)| *id));
        if theirs != class_name {
            self.fail(format!("spawnUnit's class: the capture has {}, the port {class_name:?}", args.at(1).show()));
        } else if nums::<3>(args.at(3)).iter().zip(at).any(|(a, b)| !sim_replay::same_number(*a, b)) {
            self.fail(format!("spawnUnit's point: the capture has {}, the port {at:?}", args.at(3).show()));
        } else if !sim_replay::same_number(opt_num(args.at(4)).unwrap_or(f64::NAN), yaw.unwrap_or(f64::NAN)) || args.at(4).is_undefined() != yaw.is_none() {
            self.fail(format!("spawnUnit's yaw: the capture has {}, the port {yaw:?}", args.at(4).show()));
        }
        let unit = call.result().handle_num("unit").map(|u| Ref::new(u as u32, 0));
        self.apply_fields(s, after, character, &["unit", "className", "objective"]);
        self.apply_mobile_refresh(s, after);
        unit
    }
    fn force_kill(&mut self, s: &mut Store, _: Ref<Character>, _: Ref<Unit>) {
        if let Some((_, at)) = self.call("spawning.forceKill") {
            self.apply_death(s, at);
        }
    }
    fn health(&mut self, unit: Ref<Unit>) -> f64 {
        match self.t.health.iter().find(|(u, _)| *u == unit.slot as f64) {
            Some((_, h)) => *h,
            None => {
                self.fail(format!("unit {}'s health: not captured", unit.slot));
                0.0
            }
        }
    }
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>) {
        if !self.serves_objectives {
            return;
        }
        if let Some((_, at)) = self.call("objectives.request") {
            self.apply_fields(s, at, character, &["objective"]);
            self.apply_mobile_refresh(s, at);
        }
    }
    fn republish(&mut self, s: &mut Store, character: Ref<Character>) {
        if !self.serves_objectives {
            return;
        }
        if let Some((_, at)) = self.call("objectives.republish") {
            self.apply_fields(s, at, character, &["objective"]);
            self.apply_mobile_refresh(s, at);
        }
    }
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId> {
        self.t.hero_names.iter().find(|(n, _)| *n == class_name).and_then(|(_, h)| *h)
    }
    fn living(&mut self, team: f64) -> Option<f64> {
        if team >= 0.0 && team.fract() == 0.0 {
            self.t.living.get(team as usize).copied().flatten()
        } else {
            None
        }
    }
    fn building_alive(&mut self, building: Ref<Building>) -> bool {
        match self.t.buildings.iter().find(|(b, _)| *b == building) {
            Some((_, a)) => *a,
            None => {
                self.fail(format!("building {building:?}'s alive: not captured"));
                true
            }
        }
    }
    fn members(&mut self) -> Vec<Member> {
        self.t.members.clone()
    }
    fn building_attacker(&mut self, building: Ref<Building>) -> Option<Ref<Character>> {
        self.t.building_attackers.iter().find(|(b, _)| *b == building).and_then(|(_, a)| *a)
    }
    fn vehicle_attacker(&mut self, vehicle: Ref<Vehicle>) -> Option<Ref<Character>> {
        self.t.hull_attackers.iter().find(|(v, _)| *v == vehicle).and_then(|(_, a)| *a)
    }
    fn kill_region_victims(&mut self) -> Vec<Victim> {
        self.t.victims.clone()
    }
    fn kill_victim(&mut self, s: &mut Store, victim: Victim) {
        let label = match victim {
            Victim::Unit { .. } => "combat.damage",
            Victim::Hull { .. } => "vehicle.setHealth",
        };
        if let Some((_, at)) = self.call(label) {
            self.apply_death(s, at);
        }
    }
    fn effect(&mut self, _: &str, _: [f64; 3]) {}
    fn post_event(&mut self, s: &mut Store, _: &PostEvent) {
        self.serve_ai(s);
    }
    fn mobile_event(&mut self, s: &mut Store, _: &MobileEvent) {
        self.serve_ai(s);
    }
    fn battle_over(&mut self, _: f64) {}
    fn ai_detach(&mut self, _: &mut Store, _: Ref<Character>) {
        self.fail("the roster's calls are not in these frames".into());
    }
    fn ai_attach(&mut self, _: &mut Store, _: Ref<Character>) {
        self.fail("the roster's calls are not in these frames".into());
    }
    fn retire_unit(&mut self, _: &mut Store, _: Ref<Character>, _: Ref<Unit>) {
        self.fail("the roster's calls are not in these frames".into());
    }
}

impl Served<'_> {
    /// ai.postEvent: served, the Characters' objectives as it left them.
    fn serve_ai(&mut self, s: &mut Store) {
        if !self.ai {
            return;
        }
        let Some((_, at)) = self.call("ai.postEvent") else { return };
        for (r, c) in &self.afters[at].characters {
            if let Some(mine) = s.character.get_mut(*r) {
                mine.objective = c.objective;
            }
        }
        self.apply_mobile_refresh(s, at);
    }
}

// ---- Running a frame.

/// A frame made ready: its state in a store, the world it reads and the
/// calls it makes, served.
struct Ready<'f> {
    cx: Replay,
    ai: bool,
    world: Served<'f>,
}

fn ready(frame: &Frame) -> Result<Ready<'_>, String> {
    let before = frame.before();
    let (mut cx, ai) = read_state(before)?;
    let t = tables(before, frame, &mut cx)?;
    let mut afters = Vec::new();
    for call in frame.calls() {
        afters.push(if call.extra().has("battle") { after_of(call, &mut cx)? } else { After { characters: Vec::new(), mobile: Vec::new(), stats: BattleStats::default(), reinforcements: IntObj::default(), spawning: None, bodies: None } });
    }
    let world = Served::new(frame, t, afters);
    Ok(Ready { cx, ai, world })
}

/// Runs the port on the frame's store (taken out of the context while it
/// runs) and compares the end state.
fn finish(frame: &Frame, mut r: Ready, port: impl FnOnce(&mut Store, &mut Strings, &mut Served) -> Option<String>) -> Outcome {
    let mut store = std::mem::take(&mut r.cx.store);
    let mut strings = std::mem::take(&mut r.cx.strings);
    let failure = port(&mut store, &mut strings, &mut r.world);
    r.cx.store = store;
    r.cx.strings = strings;
    if let Some(f) = r.world.failure.clone().or(failure) {
        return Outcome::Failed(f);
    }
    let result = compare(&mut r.cx, frame.after(), r.ai).and_then(|_| r.world.log.borrow().finish());
    match result {
        Ok(()) => Outcome::Same,
        Err(Mismatch(m)) => Outcome::Failed(m),
    }
}

fn run(name: &str, mut port: impl FnMut(&Capture, &Frame) -> Outcome) {
    let Some(corpus) = Corpus::from_env().expect("BFE_REPLAY_CORPUS") else { return };
    let report = replay_corpus(&corpus, name, |c, f| port(c, f)).expect("the corpus reads");
    println!("{}", report.summary());
    report.assert_clean();
}

fn or_fail(r: Result<Ready<'_>, String>) -> Result<Ready<'_>, Outcome> {
    r.map_err(|e| Outcome::Failed(format!("the frame's state: {e}")))
}

// ---- The replays.

/// CommandPosts.update: the posts' counts, captures, neutralizations and
/// destroyed buildings.
#[test]
fn command_posts_replay_as_captured() {
    run("CommandPosts.update", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        let soldiers: Vec<Capturer> = frame
            .before()
            .get("soldiers")
            .items()
            .map(|s| {
                let p = s.get("position").vec3().unwrap_or([f64::NAN; 3]);
                Capturer { team: num(s.get("team")), position: Vector3::new(p[0], p[1], p[2]) }
            })
            .collect();
        let dt = num(frame.arg(0));
        finish(frame, r, |s, _, world| {
            posts::update(s, dt, &soldiers, world);
            None
        })
    });
}

/// Battle.update: the scores' tick, each team's bleed and countdown, the
/// end; the command vehicles' posts refreshed on the way.
#[test]
fn battle_rules_replay_as_captured() {
    run("Battle.update", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        let characters = r.cx.store.battle.characters.clone();
        let active: Vec<(Ref<Character>, bool)> = characters.iter().copied().zip(frame.before().get("active").items().map(|a| a.bool())).collect();
        let dt = num(frame.arg(0));
        finish(frame, r, |s, strings, world| {
            rules::update(s, dt, strings, |c, _| active.iter().find(|(r, _)| *r == c).is_some_and(|(_, a)| *a), world);
            None
        })
    });
}

/// Battlefield.update: each Character's countdown outside, and the kills
/// when it runs out (served).
#[test]
fn battlefield_replays_as_captured() {
    run("Battlefield.update", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        // The boundary's points again from its edges' starts (an edge from
        // the last point to the first, then one from each point to the
        // next): they make the same edges.
        let polygons: Vec<Vec<Vec3>> = frame
            .before()
            .get("boundary")
            .items()
            .map(|edges| {
                let starts: Vec<Vec3> = edges.items().map(|e| Vec3 { x: num(e.at(0)), y: 0.0, z: num(e.at(1)) }).collect();
                let mut points = starts.get(1..).map(<[Vec3]>::to_vec).unwrap_or_default();
                points.extend(starts.first().copied());
                points
            })
            .collect();
        let field = Battlefield { boundary: Boundary::new(&polygons) };
        let dt = num(frame.arg(0));
        finish(frame, r, |s, _, world| {
            field.update(dt, s, world);
            None
        })
    });
}

/// ClassicSpawning.clock: the wave and release clocks, the locals' cap.
#[test]
fn spawn_clock_replays_as_captured() {
    run("ClassicSpawning.clock", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        let dt = num(frame.arg(0));
        finish(frame, r, |s, _, world| {
            spawning::clock(s, dt, world);
            None
        })
    });
}

/// ClassicSpawning.updateCharacters: each Character's respawn (served:
/// spawnUnit and the AI's objectives; the draws checked in order).
#[test]
fn spawn_characters_replay_as_captured() {
    run("ClassicSpawning.updateCharacters", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        if r.cx.store.spawning.characters != r.cx.store.battle.characters {
            return Outcome::Skipped("the battle's Characters are not the manager's".into());
        }
        let dt = num(frame.arg(0));
        let log = r.world.log.clone();
        finish(frame, r, |s, strings, world| {
            let mut draws = LogDraws { log, stream: "A", failure: None };
            spawning::update_characters(s, dt, strings, world, &mut draws);
            draws.failure
        })
    });
}

/// ClassicSpawning.onDeath: a Character's body died.
#[test]
fn spawn_deaths_replay_as_captured() {
    run("ClassicSpawning.onDeath", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        let Some(who) = character_at(&r.cx.store, place(frame.before().get("place"))) else {
            return Outcome::Skipped("a Character not in the manager's list".into());
        };
        finish(frame, r, |s, _, world| {
            spawning::on_death(s, who, world);
            None
        })
    });
}

/// The battle phase (game.js updateBattle) whole: the capturers, the posts'
/// tick and events (capture points, a destroyed post's kill region), the
/// command vehicles' posts, the manager's clock, the rules and the respawns.
#[test]
fn battle_phases_replay_as_captured() {
    run("battle", |_, frame| {
        let r = match or_fail(ready(frame)) {
            Ok(r) => r,
            Err(o) => return o,
        };
        let before = frame.before();
        let characters = r.cx.store.battle.characters.clone();
        let active: Vec<(Ref<Character>, bool)> = characters.iter().copied().zip(before.get("active").items().map(|a| a.bool())).collect();
        let (dt, ai) = (num(before.get("dt")), r.ai);
        let log = r.world.log.clone();
        finish(frame, r, |s, strings, world| {
            let mut a = LogDraws { log: log.clone(), stream: "A", failure: None };
            let mut gameplay = LogDraws { log, stream: "stream:gameplay", failure: None };
            update_battle(dt, s, strings, ai, |c, _| active.iter().find(|(r, _)| *r == c).is_some_and(|(_, a)| *a), world, &mut a, &mut gameplay);
            a.failure.or(gameplay.failure)
        })
    });
}

/// The posts' and command vehicles' kinds a frame names exist (a compile
/// check of the store's types the replays use).
#[allow(dead_code)]
fn kinds(_: Ref<Post>, _: Ref<MobilePost>) {}
