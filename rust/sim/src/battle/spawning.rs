//! Classic population and spawning (src/ai/classic/spawning.js;
//! docs/ai-population.md), and the spawn post choices it makes through
//! src/ai/classic/goals.js (friendlySpawn, selectSpawnPost,
//! retainSpawnPost, humanSpawnPost).
//!
//! Each team has a fixed roster of Characters. Each life a Character picks
//! a class by its team's roster deficit, a post it can spawn at and a place
//! in that post's queue, then waits for its wave: a wave every A seconds and
//! a release every B seconds within it (Q = trunc(A / B)); a bot scheduled
//! for wave S with queue ordinal J waits while (S - W) * Q - R + J >= 0. A
//! post's queue is four counters (AI or human, at its own team's post or an
//! ally's); joining returns the count before, plus, for a bot, the humans
//! waiting in its category. Every draw goes through the shared generator A
//! in the order the 2004 game makes it.
//!
//! The state is the store's (rust/sim/schema/battle.mjs): the `spawning`
//! singleton (its teams, clocks and queues) and the battle's Characters.

use super::names::{human_name, BotNames, NameLists};
use super::posts::{self, post_ref, spawn_post, SpawnFacts};
use super::rules::{new_character, spend};
use super::world::SpawningWorld;
use super::stats::{Text, ContextText};
use crate::gen::{Character, PlayerId, PostRef, Spawn, SpawnClass, SpawnPost, SpawnTeam, Spawning, Store, Unit};
use crate::store::{Nullable, Ref, StrId, Strings, TypedArray};
use jsrt::conv::{fround, to_int32};

pub const TEAM_COUNT: usize = 8;
/// Characters per team, offline.
pub const TEAM_CAPACITY: f64 = 200.0;
pub const CLASS_CAPACITY: usize = 12;
/// Seconds from death to the next preparation.
pub const RESPAWN_DELAY: f64 = 4.0;
/// Teams 3-7 without SetReinforcementCount.
pub const LOCALS_TICKETS: f64 = 2147483647.0;
const FLT_MAX: f64 = 3.4028234663852886e38;

/// The shared generator (the AI's A, src/ai/random.js ClassicRandom) as the
/// spawn manager draws from it.
pub trait Draws {
    /// next(): a raw 32-bit draw, as the double JavaScript returns.
    fn next(&mut self) -> f64;

    /// uniform(): [0, 1) in float32 steps.
    fn uniform(&mut self) -> f64 {
        ((self.next() as u32) >> 9) as f64 / 8388608.0
    }

    /// int(lo, hi): an inclusive integer, with the game's float32 rounding.
    fn int(&mut self, lo: f64, hi: f64) -> f64 {
        if !(hi > lo) {
            return lo;
        }
        let u = self.uniform();
        v8math::trunc(fround(fround(u * fround(hi - lo + 1.0)) + lo))
    }
}

impl<D:Draws+?Sized> Draws for &mut D {
fn next(&mut self)->f64 { (**self).next() }
fn uniform(&mut self)->f64 { (**self).uniform() }
fn int(&mut self,lo:f64,hi:f64)->f64 { (**self).int(lo,hi) }
}
impl Draws for jsrt::rng::ClassicRandom {
    fn next(&mut self) -> f64 {
        self.next_f64()
    }
}

/// Random callbacks borrow the current phase host only for the draw.
pub trait ContextDraws<W: ?Sized> {
    fn next_in(&mut self, world: &mut W) -> f64;
    fn uniform_in(&mut self, world: &mut W) -> f64 { ((self.next_in(world) as u32) >> 9) as f64 / 8388608.0 }
    fn int_in(&mut self, world: &mut W, lo: f64, hi: f64) -> f64 {
        if !(hi > lo) { return lo; }
        let u = self.uniform_in(world);
        v8math::trunc(fround(fround(u * fround(hi - lo + 1.0)) + lo))
    }
}
impl<W: ?Sized, D: Draws + ?Sized> ContextDraws<W> for D {
    fn next_in(&mut self, _: &mut W) -> f64 { self.next() }
    fn uniform_in(&mut self, _: &mut W) -> f64 { self.uniform() }
    fn int_in(&mut self, _: &mut W, lo: f64, hi: f64) -> f64 { self.int(lo, hi) }
}
pub struct BoundDraws<'a, W: ?Sized>(pub &'a mut dyn ContextDraws<W>, pub &'a mut W);
impl<W: ?Sized> Draws for BoundDraws<'_, W> {
    fn next(&mut self) -> f64 { self.0.next_in(self.1) }
    fn uniform(&mut self) -> f64 { self.0.uniform_in(self.1) }
    fn int(&mut self, lo: f64, hi: f64) -> f64 { self.0.int_in(self.1,lo,hi) }
}
pub trait RandomWorld { fn random_next(&mut self, stream: u32) -> f64; }

/// An argument of a mission's AI call (ai-mission.json), as the script gave
/// it.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Other,
}

/// A mission's AI call: its name and arguments, and what the extraction
/// decoded of it (a team name's faction, a bonus's mask).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Call {
    pub name: String,
    pub args: Vec<Arg>,
    pub faction: Option<f64>,
    pub mask: Option<f64>,
}

pub const HERO_BONUS: f64 = 8.0;
pub const DEFAULT_SPAWN_DELAY: [f64; 2] = [15.0, 0.5];

/// The hero a team fields when its mission named none, by faction.
fn default_hero(faction: f64) -> Option<&'static str> {
    match faction as i64 {
        1 => Some("all_inf_lukeskywalker"),
        2 => Some("imp_inf_darthvader"),
        3 => Some("rep_inf_macewindu"),
        4 => Some("cis_inf_countdooku"),
        _ => None,
    }
}

/// factionOf(name) (goals.js:50): SetTeamName's parser.
pub fn faction_of(name: Option<&str>) -> f64 {
    let Some(name) = name.filter(|n| !n.is_empty()) else { return super::names::LOCALS };
    match name.to_lowercase().as_str() {
        "all" | "alliance" => 1.0,
        "imp" | "empire" => 2.0,
        "rep" | "republic" => 3.0,
        "cis" => 4.0,
        "neutral" => 0.0,
        _ => super::names::LOCALS,
    }
}

/// bonusMask(name) (spawning.js:53).
pub fn bonus_mask(name: Option<&str>) -> f64 {
    match name.map(str::to_lowercase).as_deref() {
        Some("sabotage") => 32.0,
        Some("hero") => HERO_BONUS,
        Some("training") => 128.0,
        _ => 0.0,
    }
}

/// Number(value) of an argument, as luaInt and SetSpawnDelay read it.
fn number(arg: Option<&Arg>) -> f64 {
    match arg {
        Some(Arg::Num(x)) => *x,
        Some(Arg::Null) => 0.0,
        Some(Arg::Bool(value)) => if *value {1.0}else{0.0},
        Some(Arg::Str(s)) => bfe_content::js::string_to_number(s),
        Some(Arg::Other) => f64::NAN,
        None => f64::NAN,
    }
}

/// luaInt(value): Math.trunc(f32(Number(value) || 0)).
fn lua_int(arg: Option<&Arg>) -> f64 {
    let x = number(arg);
    let x = if x == 0.0 || x.is_nan() { 0.0 } else { x };
    v8math::trunc(fround(x))
}

/// makeTeams(capacity) (spawning.js:72): team 0 holds nobody; each team its
/// own friend and every other team's enemy, neutral to team 0; teams 3-7
/// with unlimited reinforcements.
pub fn make_teams(capacity: f64) -> Vec<SpawnTeam> {
    (0..TEAM_COUNT)
        .map(|index| {
            let mut relations = TypedArray::<i8, TEAM_COUNT>::default();
            if index != 0 {
                for other in 1..TEAM_COUNT {
                    relations[other] = if other == index { 1 } else { -1 };
                }
            }
            SpawnTeam {
                index: index as f64,
                tickets: if index <= 2 { 0.0 } else { LOCALS_TICKETS },
                aggressiveness: 1.0,
                relations,
                capacity: if index != 0 { capacity } else { 0.0 },
                ..Default::default()
            }
        })
        .collect()
}

/// applyMission(teams, calls, { hasClass }) (spawning.js:97): the mission's
/// calls in source order; returns the spawn delay the last SetSpawnDelay
/// left.
pub fn apply_mission(teams: &mut [SpawnTeam], calls: &[Call], has_class: &dyn Fn(&str) -> bool, strings: &mut Strings) -> [f64; 2] {
    let mut spawn_delay = DEFAULT_SPAWN_DELAY;
    for call in calls {
        let args = &call.args;
        let t = lua_int(args.first());
        let team = (t >= 0.0 && t < TEAM_COUNT as f64).then_some(t as usize);
        let s = |i: usize| match args.get(i) {
            Some(Arg::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        match call.name.as_str() {
            "SetTeamName" => {
                if let Some(t) = team {
                    teams[t].name = s(1).map(|n| strings.intern_str(n));
                    teams[t].faction = call.faction.unwrap_or_else(|| faction_of(s(1)));
                }
            }
            "AddUnitClass" => {
                if let (Some(t), Some(name)) = (team, s(1)) {
                    if has_class(name) && teams[t].classes.len() < CLASS_CAPACITY {
                        let target = match args.get(2) {
                            Some(Arg::Num(_)) => lua_int(args.get(2)),
                            _ => 0.0,
                        };
                        teams[t].classes.push(SpawnClass { name: strings.intern_str(name), target, hero: false });
                    }
                }
            }
            "SetUnitCount" => {
                if let Some(t) = team {
                    teams[t].unit_count = lua_int(args.get(1));
                }
            }
            "SetReinforcementCount" => {
                if let Some(t) = team {
                    teams[t].tickets = lua_int(args.get(1));
                }
            }
            "SetHeroClass" => {
                if let (Some(t), Some(name)) = (team, s(1)) {
                    teams[t].hero_class = Some(strings.intern_str(name));
                }
            }
            "ActivateBonus" => {
                if let Some(t) = team {
                    let mask = call.mask.unwrap_or_else(|| bonus_mask(s(1)));
                    teams[t].bonus = (to_int32(teams[t].bonus) | to_int32(mask)) as f64;
                }
            }
            "SetTeamAggressiveness" => {
                if let Some(t) = team {
                    teams[t].aggressiveness = fround(number(args.get(1)));
                }
            }
            "SetTeamAsFriend" | "SetTeamAsEnemy" | "SetTeamAsNeutral" => {
                let other = lua_int(args.get(1));
                if let Some(t) = team {
                    if other >= 0.0 && other < TEAM_COUNT as f64 {
                        teams[t].relations[other as usize] = match call.name.as_str() {
                            "SetTeamAsFriend" => 1,
                            "SetTeamAsEnemy" => -1,
                            _ => 0,
                        };
                    }
                }
            }
            "SetSpawnDelay" => {
                if args.len() >= 2 {
                    spawn_delay = [fround(number(args.first())), fround(number(args.get(1)))];
                }
            }
            _ => {}
        }
    }
    spawn_delay
}

/// appendHeroes(teams, { heroes, hasClass }) (spawning.js:185): teams 1 and
/// 2 field their hero when heroes are on (or the mission's bonus asks): the
/// hero class appended with a target of 1, the first class's target one
/// less.
pub fn append_heroes(teams: &mut [SpawnTeam], heroes: bool, has_class: &dyn Fn(&str) -> bool, strings: &mut Strings) {
    for t in [1, 2] {
        let team = &mut teams[t];
        if !heroes && (to_int32(team.bonus) & HERO_BONUS as i32) == 0 {
            continue;
        }
        let name = team.hero_class.map(|n| String::from_utf16_lossy(strings.units(n))).filter(|n| !n.is_empty()).or_else(|| default_hero(team.faction).map(str::to_string));
        let Some(name) = name else { continue };
        if !has_class(&name) || team.classes.len() >= CLASS_CAPACITY {
            continue;
        }
        team.classes.push(SpawnClass { name: strings.intern_str(&name), target: 1.0, hero: true });
        team.hero_count += 1.0;
        team.classes[0].target -= 1.0;
    }
}

/// A local human as the manager takes him: his team, id and name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HumanSpec {
    pub team: f64,
    pub id: Option<f64>,
    pub name: Option<String>,
}

/// The bots' names (src/bot-names.js): the mission's lists and each team's
/// generator, made with its first bot. The JavaScript keeps these in
/// closures (the manager's `names`, each team's `botNames`), so the store
/// does not hold them; they live beside it.
#[derive(Clone, Debug, Default)]
pub struct Naming {
    pub lists: Option<NameLists>,
    pub teams: Vec<Option<BotNames>>,
}

/// ownerOf(post): post.team | 0.
fn owner_of(team: f64) -> f64 {
    to_int32(team) as f64
}

fn ch(s: &Store, r: Ref<Character>) -> &Character {
    s.character.get(r).expect("a Character of the battle")
}

fn ch_mut(s: &mut Store, r: Ref<Character>) -> &mut Character {
    s.character.get_mut(r).expect("a Character of the battle")
}

/// character.spawn: every Character the manager made has one.
fn sp(c: &Character) -> &Spawn {
    c.spawn.as_ref().expect("a Character the spawn manager made")
}

fn sp_mut(c: &mut Character) -> &mut Spawn {
    c.spawn.as_mut().expect("a Character the spawn manager made")
}

/// A number field the roster may leave undefined, read as JavaScript reads
/// it in arithmetic and comparisons (undefined: NaN).
fn num(x: Option<f64>) -> f64 {
    x.unwrap_or(f64::NAN)
}

/// character.classIndex (null and undefined: none).
fn class_index(c: &Character) -> Option<f64> {
    match c.class_index {
        Nullable::Value(x) => Some(x),
        _ => None,
    }
}

/// Whether a Character is a hero (character.hero, undefined false).
fn is_hero(c: &Character) -> bool {
    c.hero == Some(true)
}

/// Retained post choices and transformed mobile class points. Reserve from
/// prepared mission geometry; paths stay on their canonical post rows.
#[derive(Default)]
pub struct Scratch {
    ids: Vec<SpawnPost>,
    views: Vec<SpawnFacts>,
    points: Vec<[f64; 4]>,
}
impl Scratch {
    pub fn reserve_points(&mut self,points:usize){self.points.reserve(points.saturating_sub(self.points.len()));}
    pub fn new(posts: usize, class_points: usize) -> Self {
        Self { ids: Vec::with_capacity(posts), views: Vec::with_capacity(posts), points: Vec::with_capacity(class_points) }
    }
}
fn listed<W: SpawningWorld + ?Sized>(s: &mut Store, world: &mut W, into: &mut Scratch) {
    super::mobile::refresh_all(s, world);
    into.ids.clear(); into.views.clear();
    into.ids.extend(s.command_posts.posts.iter().copied().map(SpawnPost::Post));
    into.ids.extend(s.mobile_posts.posts.iter().copied().map(SpawnPost::MobilePost));
    for &p in &into.ids { into.views.push(posts::spawn_facts(s, p, world)); }
}

/// relation(a, b): team a's relation to team b (1 friend, 0 neutral, -1
/// enemy).
pub fn relation(sp: &Spawning, a: f64, b: f64) -> f64 {
    let ok = |t: f64| t >= 0.0 && t < TEAM_COUNT as f64 && t.fract() == 0.0;
    if ok(a) && ok(b) {
        sp.teams[a as usize].relations[b as usize] as f64
    } else {
        0.0
    }
}

fn team(sp: &Spawning, t: f64) -> &SpawnTeam {
    &sp.teams[t as usize]
}

fn team_mut(sp: &mut Spawning, t: f64) -> &mut SpawnTeam {
    &mut sp.teams[t as usize]
}

// ---- The roster.

/// The manager after the mission's calls (the constructor, spawning.js
/// 238, without begin()): its teams, spawn delays and clocks, and the bots'
/// names beside it.
#[allow(clippy::too_many_arguments)]
pub fn new_spawning(calls: &[Call], unit_scale: f64, heroes: bool, demo: bool, names: Option<NameLists>, capacity: f64, has_class: &dyn Fn(&str) -> bool, strings: &mut Strings) -> (Spawning, Naming) {
    let mut teams = make_teams(capacity);
    let [a, b] = apply_mission(&mut teams, calls, has_class, strings);
    if unit_scale != 1.0 {
        for team in teams.iter_mut() {
            if team.unit_count > 0.0 {
                team.unit_count = v8math::max(1.0, v8math::round(team.unit_count * unit_scale));
            }
        }
    }
    append_heroes(&mut teams, heroes, has_class, strings);
    let (a, b) = (fround(a), fround(b));
    let spawning = Spawning { teams, a, b, q: v8math::trunc(fround(a / b)), wave_countdown: a, demo, heroes, ..Default::default() };
    (spawning, Naming { lists: names, teams: vec![None; TEAM_COUNT] })
}

/// begin() (spawning.js:339): humans join their teams, bots fill each team
/// to its unit count (named by their team), then each team pads its class
/// targets and activates its Characters in order.
pub fn begin<W: SpawningWorld + ?Sized>(s: &mut Store, humans: &[HumanSpec], naming: &mut Naming, strings: &mut Strings, world: &mut W, random: &mut dyn ContextDraws<W>) {
    if s.spawning.started {
        return;
    }
    let sp = &mut s.spawning;
    sp.started = true;
    sp.wave_countdown = if sp.demo { fround(0.1) } else { sp.a };
    sp.wave = 0.0;
    sp.stagger = 0.0;
    sp.released = 0.0;
    let mut registered = [0.0; TEAM_COUNT];
    if !s.spawning.demo {
        for (i, spec) in humans.iter().enumerate() {
            let team = to_int32(spec.team);
            if team > 0 && team < TEAM_COUNT as i32 {
                let name = spec.name.clone().unwrap_or_else(|| human_name(i as f64));
                let name = strings.intern_str(&name);
                insert(s, team as f64, true, spec.id.unwrap_or(i as f64), name, strings);
                registered[team as usize] += 1.0;
            }
        }
    }
    for t in 1..TEAM_COUNT {
        let mut i = registered[t];
        while i < s.spawning.teams[t].unit_count {
            let name = bot_name(&s.spawning, t, naming, strings);
            insert(s, t as f64, false, -1.0, name, strings);
            i += 1.0;
        }
    }
    for t in 0..TEAM_COUNT {
        setup_team(s, t, strings, world, random);
    }
}

/// insert(t, human, playerId, name) (spawning.js:362): a new Character at
/// the end of its team's roster and of the manager's list (which is the
/// battle's).
pub fn insert(s: &mut Store, t: f64, human: bool, player_id: f64, name: StrId, strings: &mut Strings) -> Option<Ref<Character>> {
    let team = &s.spawning.teams[t as usize];
    if team.characters.len() as f64 >= team.capacity {
        return None;
    }
    let slot = team.characters.len() as f64;
    let mut c = new_character(slot, t, human, strings);
    c.slot = Some(slot);
    c.manager_index = Some(s.spawning.characters.len() as f64);
    c.player_id = PlayerId::F64(player_id);
    c.name = Some(name);
    c.class_index = Nullable::Null;
    c.hero = Some(false);
    c.spawn = Some(Spawn { timer: 0.0, wave: Some(0.0), ordinal: Some(-1.0), post: PostRef::Null, ready: true });
    let r = s.character.insert(c);
    s.spawning.teams[t as usize].characters.push(r);
    s.spawning.characters.push(r);
    s.battle.characters.push(r);
    Some(r)
}

/// botName(team) (spawning.js:387): from the team's own generator, made with
/// its first bot.
pub fn bot_name(sp: &Spawning, t: usize, naming: &mut Naming, strings: &mut Strings) -> StrId {
    let faction = sp.teams[t].faction;
    let lists = naming.lists.clone();
    if naming.teams.len() < TEAM_COUNT {
        naming.teams.resize(TEAM_COUNT, None);
    }
    let name = naming.teams[t].get_or_insert_with(|| BotNames::new(lists.as_ref(), faction)).next();
    strings.intern_str(&name)
}

/// setupTeam(team) (spawning.js:392).
fn setup_team<W: SpawningWorld + ?Sized>(s: &mut Store, t: usize, strings: &mut Strings, world: &mut W, random: &mut dyn ContextDraws<W>) {
    pad_quotas(&mut s.spawning, t, &mut BoundDraws(random, world));
    let list = s.spawning.teams[t].characters.clone();
    for r in list {
        activate(s, r, strings, world, random);
    }
}

/// padQuotas(team) (spawning.js:400): class targets short of the team's
/// Characters gain one at a time at class max(RNG % 4n - 3n, 0).
pub fn pad_quotas(sp: &mut Spawning, t: usize, random: &mut dyn Draws) {
    let team = &mut sp.teams[t];
    let n = team.classes.len() as f64 - team.hero_count;
    if n <= 0.0 {
        return; // the 2004 game would divide by zero here
    }
    let mut sum: f64 = team.classes.iter().map(|c| c.target).sum();
    while sum < team.characters.len() as f64 {
        let index = v8math::max((random.next() % (4.0 * n)) - 3.0 * n, 0.0);
        team.classes[index as usize].target += 1.0;
        sum += 1.0;
    }
}

/// activate(character) (spawning.js:422): class 0, the final heroCount slots
/// take the hero classes, a bot draws its objective, then it prepares.
pub fn activate<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) {
    let t = ch(s, r).team;
    set_class(s, r, 0.0, strings, world);
    if !ch(s, r).human {
        let team = &s.spawning.teams[t as usize];
        let mut k = team.hero_count;
        while k > 0.0 {
            let team = &s.spawning.teams[t as usize];
            let at = team.characters.len() as f64 - k;
            if at >= 0.0 && team.characters.get(at as usize) == Some(&r) {
                let index = team.classes.len() as f64 - k;
                set_class(s, r, index, strings, world);
                break;
            }
            k -= 1.0;
        }
        world.request_objective(s, r);
    }
    prepare(s, r, strings, world, random);
}

// ---- Spawn posts (src/ai/classic/goals.js).

/// friendlySpawn(post, team, relation) (goals.js:163): its own post with a
/// spawn path or class spawn points, or a post held by a friend that has an
/// ally path; never a neutral or inactive one.
fn friendly_spawn(sp: &Spawning, post: &SpawnFacts, team: f64) -> bool {
    if !post.active {
        return false;
    }
    let owner = post.owner;
    if owner == 0.0 {
        return false;
    }
    if owner == team {
        return post.spawn_path || post.class_spawn_points > 0.0;
    }
    relation(sp, owner, team) > 0.0 && post.ally_path
}

/// selectSpawnPost (goals.js:278): uniformly among the posts its team can
/// spawn at (no draw for just one), or nowhere.
fn select_spawn_post(sp: &Spawning, list: &Scratch, team: f64, random: &mut dyn Draws) -> Option<SpawnPost> {
    let eligible = (0..list.ids.len()).filter(|&i| list.views[i].active && friendly_spawn(sp, &list.views[i], team)).count();
    if eligible == 0 { return None; }
    let pick = random.int(0.0, eligible as f64 - 1.0) as usize;
    let index = (0..list.ids.len()).filter(|&i| list.views[i].active && friendly_spawn(sp, &list.views[i], team))
        .nth(pick).expect("a selected eligible post");
    Some(list.ids[index])
}

/// retainSpawnPost (goals.js:285): a bot keeps its post while it stays a
/// valid spawn point for its team.
fn retain_spawn_post(sp: &Spawning, post: Option<SpawnPost>, list: &Scratch, team: f64, random: &mut dyn Draws) -> Option<SpawnPost> {
    if let Some(p) = post {
        if let Some(i) = list.ids.iter().position(|x| *x == p) {
            let view = &list.views[i];
            if view.active && friendly_spawn(sp, view, team) {
                return Some(p);
            }
        }
    }
    select_spawn_post(sp, list, team, random)
}

/// humanSpawnPost (goals.js:300): the eligible post nearest the old one;
/// with none yet, the eligible post nearest any post an enemy holds.
fn human_spawn_post(sp: &Spawning, post: Option<SpawnPost>, list: &Scratch, team: f64) -> Option<SpawnPost> {
    let distance = |a: &[f64; 3], b: &[f64; 3]| {
        let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        fround(x * x + y * y + z * z)
    };
    if let Some(p) = post {
        let at = list.ids.iter().position(|x| *x == p).expect("a post in the list");
        let view = &list.views[at];
        if view.active {
            if friendly_spawn(sp, view, team) {
                return Some(p);
            }
            let mut best = FLT_MAX;
            let mut pick = None;
            for o in 0..list.ids.len() {
                if !list.views[o].active { continue; }
                if !friendly_spawn(sp, &list.views[o], team) {
                    continue;
                }
                let d = distance(&list.views[o].position, &view.position);
                if best > d {
                    best = d;
                    pick = Some(list.ids[o]);
                }
            }
            return pick;
        }
    }
    let mut best = FLT_MAX;
    let mut pick = None;
    for o in 0..list.ids.len() {
        if !list.views[o].active { continue; }
        if !friendly_spawn(sp, &list.views[o], team) {
            continue;
        }
        for e in 0..list.ids.len() {
            if !list.views[e].active { continue; }
            if !(relation(sp, team, list.views[e].owner) < 0.0) {
                continue;
            }
            let d = distance(&list.views[o].position, &list.views[e].position);
            if best > d {
                best = d;
                pick = Some(list.ids[o]);
            }
        }
    }
    pick
}

/// choosePost(character) (spawning.js:496).
pub fn choose_post<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W, random: &mut dyn ContextDraws<W>) -> Option<SpawnPost> {
    choose_post_with_scratch(s, r, world, random, &mut Scratch::default())
}

pub fn choose_post_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) -> Option<SpawnPost> {
    let goal = if ch(s, r).human { None } else { world.spawn_goal(s, r) };
    listed(s, world, scratch);
    let list = &*scratch;
    let c = ch(s, r);
    if let Some(goal) = goal {
        let mut best = None;
        let mut best_distance = f64::INFINITY;
        for (i, p) in list.views.iter().enumerate() {
            if !friendly_spawn(&s.spawning, p, c.team) { continue; }
            let d = v8math::hypot(p.position[0] - goal[0], p.position[2] - goal[2]);
            if d < best_distance { best_distance = d; best = Some(list.ids[i]); }
        }
        if best.is_some() { return best; }
    }
    let old = spawn_post(sp(c).post);
    if c.human {
        human_spawn_post(&s.spawning, old, list, c.team)
    } else {
        retain_spawn_post(&s.spawning, old, list, c.team, &mut BoundDraws(random, world))
    }
}

/// The owner of a post, as the queues' categories ask it.
fn own(s: &Store, c: &Character, post: SpawnPost) -> bool {
    c.team == owner_of(posts::team_of(s, post).0)
}

/// Construction reserves room without creating queue entries or changing
/// Map insertion order. Team switches and first post reservations then reuse
/// storage, including a team which begins with no Characters of its own.
pub fn prepare_runtime(s: &mut Store, posts: usize, characters: usize) {
    s.spawning.queues.reserve(posts);
    s.spawning.characters.reserve(characters.saturating_sub(s.spawning.characters.len()));
    for team in &mut s.spawning.teams {
        team.characters.reserve(characters.saturating_sub(team.characters.len()));
    }
}

// ---- Post queues.

pub fn queue_of(sp: &mut Spawning, post: SpawnPost) -> &mut [f64; 4] {
    if !sp.queues.has(&post) {
        // [AI at an ally's post, humans there, AI at own post, humans there]
        sp.queues.set(post, [0.0; 4]);
    }
    sp.queues.get_mut(&post).unwrap()
}

/// reserve(post, human, own) (spawning.js:513).
pub fn reserve(sp: &mut Spawning, post: SpawnPost, human: bool, own: bool) -> f64 {
    let queue = queue_of(sp, post);
    let i = (if human { 1 } else { 0 }) + (if own { 2 } else { 0 });
    let before = queue[i];
    queue[i] += 1.0;
    if human {
        before
    } else {
        before + queue[1 + if own { 2 } else { 0 }]
    }
}

/// release(post, human, own) (spawning.js:520).
pub fn release(sp: &mut Spawning, post: SpawnPost, human: bool, own: bool) -> f64 {
    let queue = queue_of(sp, post);
    let i = (if human { 1 } else { 0 }) + (if own { 2 } else { 0 });
    queue[i] -= 1.0;
    if human {
        queue[i]
    } else {
        queue[i] + queue[1 + if own { 2 } else { 0 }]
    }
}

/// changePost(character, post) (spawning.js:529): a held reservation moves
/// with him.
pub fn change_post(s: &mut Store, r: Ref<Character>, post: Option<SpawnPost>) {
    let c = ch(s, r);
    let (human, held, old) = (c.human, num(sp(c).ordinal) >= 0.0, spawn_post(sp(c).post));
    if held && old != post {
        if let Some(old) = old {
            let own = own(s, ch(s, r), old);
            release(&mut s.spawning, old, human, own);
        }
        sp_mut(ch_mut(s, r)).ordinal = Some(-1.0);
        if let Some(p) = post {
            let own = own(s, ch(s, r), p);
            let ordinal = reserve(&mut s.spawning, p, human, own);
            sp_mut(ch_mut(s, r)).ordinal = Some(ordinal);
        }
    }
    sp_mut(ch_mut(s, r)).post = post_ref(post);
}

// ---- Tickets.

/// tickets(team) (spawning.js:541): the battle's reinforcements.
pub fn tickets(s: &Store, team: f64) -> f64 {
    match s.battle.reinforcements.at(team) {
        Some(r) => r,
        None => self::team(&s.spawning, team).tickets,
    }
}

/// spendTicket(team) (spawning.js:546).
pub fn spend_ticket(s: &mut Store, team: f64) {
    if s.battle.reinforcements.at(team).is_some() {
        spend(&mut s.battle, team, 1.0);
    } else {
        team_mut(&mut s.spawning, team).tickets -= 1.0;
    }
}

// ---- Classes.

/// setClass(character, index) (spawning.js:433): a hero class takes the
/// hero's name too.
pub fn set_class<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, index: f64, strings: &dyn ContextText<W>, world: &mut W) {
    set_class_named_with(s,r,index,strings,world,|world,name|world.hero_name(name));
}

/// Class assignment reads only a prepared hero name; no body service runs.
pub fn set_class_named(s:&mut Store,r:Ref<Character>,index:f64,strings:&dyn Text,mut hero_name:impl FnMut(StrId)->Option<StrId>) {
    set_class_named_with(s,r,index,&strings,&mut (),|_,name|hero_name(name));
}
fn set_class_named_with<W:?Sized>(s:&mut Store,r:Ref<Character>,index:f64,strings:&dyn ContextText<W>,world:&mut W,mut hero_name:impl FnMut(&mut W,StrId)->Option<StrId>) {
    let t = ch(s, r).team;
    let team = team(&s.spawning, t);
    let cls = (index >= 0.0 && index.fract() == 0.0).then(|| team.classes.get(index as usize)).flatten().cloned();
    let c = ch_mut(s, r);
    c.class_index = if cls.is_some() { Nullable::Value(index) } else { Nullable::Null };
    let Some(cls) = cls else { return };
    c.class_name = Some(cls.name);
    c.hero = Some(cls.hero);
    if cls.hero {
        if let Some(name) = hero_name(world,cls.name).filter(|n| strings.nonempty_in(world,*n)) {
            ch_mut(s, r).name = Some(name);
        }
    }
}

/// pickClass(character) (spawning.js:461): the roster-deficit draw. A hero
/// keeps his class; no deficit anywhere keeps the current one.
pub fn pick_class(s: &Store, r: Ref<Character>, random: &mut dyn Draws) -> f64 {
    let c = ch(s, r);
    let team = team(&s.spawning, c.team);
    let class_of = |i: Option<f64>| i.filter(|x| *x >= 0.0 && x.fract() == 0.0).and_then(|x| team.classes.get(x as usize));
    if class_index(c).is_some() && class_of(class_index(c)).is_some_and(|k| k.hero) {
        return class_index(c).unwrap();
    }
    // Mission loading admits at most CLASS_CAPACITY. Clamp the scratch view
    // too: an oversized imported roster cannot index past the fixed counts.
    let classes = team.classes.len().min(CLASS_CAPACITY);
    let mut counts = [0.0; CLASS_CAPACITY];
    for &other in &team.characters {
        let o = ch(s, other);
        if let (Some(i), Some(_)) = (class_index(o), class_of(class_index(o))) {
            if let Some(count) = counts.get_mut(i as usize) { *count += 1.0; }
        }
    }
    let n = v8math::max(0.0, v8math::min(classes as f64, team.classes.len() as f64 - team.hero_count));
    let mut deficits = [0.0; CLASS_CAPACITY];
    let mut used = 0;
    let mut total = 0.0;
    let mut i = 0.0;
    while i < n {
        let d = v8math::max(team.classes[i as usize].target - counts[i as usize], 0.0);
        deficits[used] = d; used += 1;
        total += d;
        i += 1.0;
    }
    if total > 0.0 {
        let r = random.next() % total;
        let mut sum = 0.0;
        for (i, d) in deficits[..used].iter().enumerate() {
            sum += d;
            if r < sum {
                return i as f64;
            }
        }
        return -1.0;
    }
    class_index(c).unwrap_or(0.0)
}

/// prepare(character) (spawning.js:483): a bot (unless a hero) picks its
/// class and is ready, a human waits; it keeps or picks a post, joins its
/// queue, and is scheduled for the next wave (two ahead if the wave clock is
/// overdue).
pub fn prepare<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) {
    prepare_with_scratch(s, r, strings, world, random, &mut Scratch::default())
}

pub fn prepare_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) {
    if !ch(s, r).human {
        if !is_hero(ch(s, r)) {
            let index = pick_class(s, r, &mut BoundDraws(random, world));
            set_class(s, r, index, strings, world);
        }
        sp_mut(ch_mut(s, r)).ready = true;
    } else {
        sp_mut(ch_mut(s, r)).ready = false;
    }
    let post = choose_post_with_scratch(s, r, world, random, scratch);
    change_post(s, r, post);
    if let Some(post) = spawn_post(sp(ch(s, r)).post) {
        let (human, own) = (ch(s, r).human, own(s, ch(s, r), post));
        let ordinal = reserve(&mut s.spawning, post, human, own);
        sp_mut(ch_mut(s, r)).ordinal = Some(ordinal);
    }
    let wave = s.spawning.wave + 1.0 + if s.spawning.wave_countdown < 0.0 { 1.0 } else { 0.0 };
    sp_mut(ch_mut(s, r)).wave = Some(wave);
}

// ---- The update.

/// awaitingHuman(character) (spawning.js:567).
pub fn awaiting_human(c: &Character) -> bool {
    c.human && c.unit.is_none() && !sp(c).ready && sp(c).timer <= 0.0
}

/// clock(dt) (spawning.js:574): the manager's two clocks, one step each, and
/// the locals' living cap.
pub fn clock<W: SpawningWorld + ?Sized>(s: &mut Store, dt: f64, world: &mut W) {
    let dt = fround(dt);
    let mut cap = 0.0;
    super::mobile::refresh_all(s, world);
    for i in 0..s.command_posts.posts.len() {
        let (active, owner, allies) = world.clock_post(s, SpawnPost::Post(s.command_posts.posts[i]));
        if active && relation(&s.spawning, owner, 3.0) > 0.0 { cap += to_int32(allies) as f64; }
    }
    for i in 0..s.mobile_posts.posts.len() {
        let (active, owner, allies) = world.clock_post(s, SpawnPost::MobilePost(s.mobile_posts.posts[i]));
        if active && relation(&s.spawning, owner, 3.0) > 0.0 { cap += to_int32(allies) as f64; }
    }
    let sp = &mut s.spawning;
    sp.locals_cap = v8math::min(cap, sp.teams[3].unit_count);
    let countdown = fround(sp.wave_countdown - dt);
    sp.wave_countdown = countdown;
    let mut stagger = sp.stagger;
    if !(countdown > 0.0) {
        sp.wave_countdown = fround(sp.a + countdown);
        if sp.demo && sp.wave < 3.0 {
            sp.wave_countdown = 1.0;
        }
        sp.wave += 1.0;
        stagger = 0.0;
        sp.released = 0.0;
    }
    stagger = fround(stagger - dt);
    sp.stagger = stagger;
    if stagger <= 0.0 {
        sp.released += 1.0;
        sp.stagger = fround(sp.b + stagger);
    }
}

/// waiting(character) (spawning.js:601): whether the wave and stagger gate
/// holds him back.
pub fn waiting(sp: &Spawning, c: &Character) -> bool {
    let spawn = self::sp(c);
    (num(spawn.wave) - sp.wave) * sp.q - sp.released + num(spawn.ordinal) >= 0.0
}

/// updateCharacters(dt) (spawning.js:561): every Character in roster order.
pub fn update_characters<W: SpawningWorld + ?Sized>(s: &mut Store, dt: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) {
    update_characters_with_scratch(s, dt, strings, world, random, &mut Scratch::default())
}

pub fn update_characters_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, dt: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) {
    let mut i = 0;
    while i < s.spawning.characters.len() {
        let r = s.spawning.characters[i];
        update_character_with_scratch(s, r, dt, strings, world, random, scratch);
        i += 1;
    }
}

/// updateCharacter(character, dt) (spawning.js:605).
pub fn update_character<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, dt: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) {
    update_character_with_scratch(s, r, dt, strings, world, random, &mut Scratch::default())
}

pub fn update_character_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, dt: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) {
    let dt = fround(dt);
    let mut t = ch(s, r).team;
    if let Some(unit) = ch(s, r).unit {
        // A hero dies when its team's tickets fall to its living count.
        let c = ch(s, r);
        if is_hero(c) && tickets(s, c.team) <= team(&s.spawning, t).living && world.health(s, unit) > 0.0 {
            world.force_kill(s, r, unit);
        }
        return;
    }
    let timer = fround(sp(ch(s, r)).timer - dt);
    sp_mut(ch_mut(s, r)).timer = timer;
    if timer > 0.0 {
        return;
    }
    if timer > -dt {
        prepare_with_scratch(s, r, strings, world, random, scratch);
    }
    let human = ch(s, r).human;
    if !human && tickets(s, t) <= team(&s.spawning, t).living {
        return;
    }
    if !human && team(&s.spawning, t).characters.len() as f64 > team(&s.spawning, t).capacity {
        change_post(s, r, None);
        switch_team_with_scratch(s, r, 3.0 - t, strings, world, random, scratch);
        world.request_objective(s, r);
        t = ch(s, r).team;
    }
    let post = choose_post_with_scratch(s, r, world, random, scratch);
    change_post(s, r, post);
    let c = ch(s, r);
    if !sp(c).ready || spawn_post(sp(c).post).is_none() {
        let wave = s.spawning.wave + 1.0;
        sp_mut(ch_mut(s, r)).wave = Some(wave);
        return;
    }
    if !c.human {
        if waiting(&s.spawning, c) {
            return;
        }
        if !is_hero(c) && t > 2.0 {
            let mut living = 0.0;
            for &other in &team(&s.spawning, t).characters {
                let o = ch(s, other);
                if o.unit.is_some() && !o.human {
                    living += 1.0;
                }
            }
            if s.spawning.locals_cap <= living {
                return;
            }
        }
    }
    spawn_with_scratch(s, r, strings, world, random, scratch);
}

/// spawnPoint(post, allied) (spawning.js:651): a random point of the post's
/// spawn path (its ally path when the post is an ally's), else a random
/// class spawn point (own posts only), else the post itself.
fn spawn_point<W: SpawningWorld + ?Sized>(s: &Store, post: SpawnPost, allied: bool, world: &mut W,
    random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) -> [f64; 4] {
    let view = posts::spawn_facts(s, post, world);
    if let SpawnPost::Post(r) = post {
        let ai = s.post.get(r).expect("a post").ai.as_ref();
        let path = ai.and_then(|a| if allied { a.ally_path.as_ref() } else { a.spawn_path.as_ref() });
        if let Some(path) = path.filter(|p| p.present && !p.points.is_empty()) {
            return path.points[(random.next_in(world) % path.points.len() as f64) as usize];
        }
    }
    if !allied && view.class_spawn_points > 0.0 {
        let points = match post {
            SpawnPost::Post(r) => s.post.get(r).expect("a post").ai.as_ref().map_or(&[][..], |a| a.class_spawn_world.as_slice()),
            SpawnPost::MobilePost(r) => {
                scratch.points.clear();
                if s.mobile_post.get(r).expect("a mobile post").active {
                    world.class_spawn_world_into(s, r, &mut scratch.points);
                }
                &scratch.points
            }
        };
        if !points.is_empty() {
            let i = random.next_in(world) % view.class_spawn_points;
            return points.get(i as usize).copied().unwrap_or(points[0]);
        }
    }
    let yaw = if view.yaw != 0.0 && !view.yaw.is_nan() { view.yaw } else { 0.0 };
    [view.position[0], view.position[1], view.position[2], yaw]
}

/// spawn(character) (spawning.js:664): into the world; if the class can't be
/// made, class 0 is tried once.
pub fn spawn<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) -> Option<Ref<Unit>> {
    spawn_with_scratch(s, r, strings, world, random, &mut Scratch::default())
}

pub fn spawn_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) -> Option<Ref<Unit>> {
    let post = spawn_post(sp(ch(s, r)).post).expect("a post");
    let t = ch(s, r).team;
    let allied = t != owner_of(posts::team_of(s, post).0);
    let point = spawn_point(s, post, allied, world, random, scratch);
    let (xyz, yaw) = ([point[0], point[1], point[2]], Some(point[3]));
    let class_name = ch(s, r).class_name;
    let mut unit = world.spawn_unit(s, r, class_name, post, xyz, yaw);
    if unit.is_none() {
        if class_index(ch(s, r)).is_none_or(|i| i == 0.0 || i.is_nan()) {
            return None;
        }
        set_class(s, r, 0.0, strings, world);
        let class_name = ch(s, r).class_name;
        unit = world.spawn_unit(s, r, class_name, post, xyz, yaw);
        unit?;
    }
    let c = ch_mut(s, r);
    c.unit = unit;
    let index = class_index(c);
    let team = &s.spawning.teams[t as usize];
    let cls = index.filter(|i| *i >= 0.0 && i.fract() == 0.0).and_then(|i| team.classes.get(i as usize));
    let hero = cls.is_some_and(|k| k.hero);
    ch_mut(s, r).hero = Some(hero);
    if !hero {
        s.spawning.teams[t as usize].living += 1.0;
    }
    if !ch(s, r).human {
        world.republish(s, r);
    }
    let c = ch(s, r);
    if num(sp(c).ordinal) >= 0.0 {
        let (human, own) = (c.human, own(s, c, post));
        release(&mut s.spawning, post, human, own);
        sp_mut(ch_mut(s, r)).ordinal = Some(-1.0);
    }
    unit
}

// ---- Events.

/// onDeath(character) (spawning.js:690): he prepares again in 4 s; a bot
/// draws a fresh objective and forgets its post; the team's living count
/// drops (heroes aren't counted) and it spends a reinforcement.
pub fn on_death<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W) {
    let t = ch(s, r).team;
    sp_mut(ch_mut(s, r)).timer = RESPAWN_DELAY;
    if !ch(s, r).human {
        world.request_objective(s, r);
        sp_mut(ch_mut(s, r)).post = PostRef::Null;
    }
    ch_mut(s, r).unit = None;
    if !is_hero(ch(s, r)) {
        team_mut(&mut s.spawning, t).living -= 1.0;
    }
    if tickets(s, t) > 0.0 {
        spend_ticket(s, t);
    }
}

/// A human's choice on his spawn screen (onHumanReady's options): a class
/// by name or by index, and a post (None: not given; Some(None): none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HumanChoice {
    pub class_name: Option<StrId>,
    pub class_index: Option<f64>,
    pub post: Option<Option<SpawnPost>>,
}

/// onHumanReady(character, choice) (spawning.js:705): a class from his
/// team's roster (by name or index; a name outside it is his class, not
/// counted) and a post if he chose one; he spawns on the next update.
pub fn on_human_ready<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, choice: HumanChoice, strings: &dyn ContextText<W>, world: &mut W) {
    on_human_ready_named_with(s,r,choice,strings,world,|world,name|world.hero_name(name));
}

/// A waiting human has no body to change. Readiness updates only canonical
/// Character/class/queue rows, including inside a synchronous AI hook.
pub fn on_human_ready_named(s:&mut Store,r:Ref<Character>,choice:HumanChoice,strings:&dyn Text,mut hero_name:impl FnMut(StrId)->Option<StrId>) {
    on_human_ready_named_with(s,r,choice,&strings,&mut (),|_,name|hero_name(name));
}
fn on_human_ready_named_with<W:?Sized>(s:&mut Store,r:Ref<Character>,choice:HumanChoice,strings:&dyn ContextText<W>,world:&mut W,mut hero_name:impl FnMut(&mut W,StrId)->Option<StrId>) {
    let t = ch(s, r).team;
    if let Some(name) = choice.class_name {
        match team(&s.spawning, t).classes.iter().position(|c| c.name == name) {
            Some(index) => set_class_named_with(s, r, index as f64, strings, world, &mut hero_name),
            None => ch_mut(s, r).class_name = Some(name),
        }
    } else if let Some(index) = choice.class_index {
        let last = team(&s.spawning, t).classes.len() as f64 - 1.0;
        let index = v8math::min(v8math::max(to_int32(index) as f64, 0.0), last);
        set_class_named_with(s, r, index, strings, world, &mut hero_name);
    }
    if let Some(post) = choice.post {
        change_post(s, r, post);
    }
    sp_mut(ch_mut(s, r)).ready = true;
}

/// switchTeam(character, to) (spawning.js:723): a non-hero Character to
/// another team with room; a bot picks a class there.
pub fn switch_team<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, to: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>) -> bool {
    switch_team_with_scratch(s, r, to, strings, world, random, &mut Scratch::default())
}

pub fn switch_team_with_scratch<W: SpawningWorld + ?Sized>(s: &mut Store, r: Ref<Character>, to: f64, strings: &dyn ContextText<W>, world: &mut W, random: &mut dyn ContextDraws<W>, scratch: &mut Scratch) -> bool {
    let c = ch(s, r);
    if is_hero(c) || c.team == to || to < 1.0 || to >= TEAM_COUNT as f64 {
        return false;
    }
    if team(&s.spawning, to).characters.len() as f64 >= team(&s.spawning, to).capacity {
        return false;
    }
    let from = c.team;
    let held = num(sp(c).ordinal) >= 0.0;
    change_post(s, r, None);
    // splice(indexOf(character), 1): not found (-1) takes the last.
    let list = &mut team_mut(&mut s.spawning, from).characters;
    match list.iter().position(|x| *x == r) {
        Some(i) => {
            list.remove(i);
        }
        None => {
            list.pop();
        }
    }
    team_mut(&mut s.spawning, to).characters.push(r);
    ch_mut(s, r).team = to;
    if !ch(s, r).human {
        let index = pick_class(s, r, &mut BoundDraws(random, world));
        set_class(s, r, index, strings, world);
    } else {
        let last = team(&s.spawning, to).classes.len() as f64 - 1.0;
        let index = v8math::min(v8math::max(class_index(ch(s, r)).unwrap_or(0.0), 0.0), last);
        set_class(s, r, index, strings, world);
    }
    let post = choose_post_with_scratch(s, r, world, random, scratch);
    change_post(s, r, post);
    if let (true, Some(p)) = (held, post) {
        let human = ch(s, r).human;
        let own = to == owner_of(posts::team_of(s, p).0);
        let ordinal = reserve(&mut s.spawning, p, human, own);
        sp_mut(ch_mut(s, r)).ordinal = Some(ordinal);
    }
    true
}

/// EnhancedSpawning.choosePost reads its retained objective before refreshing
/// the post list. Canonical placed/mobile posts supply that position directly.
pub fn objective_position<W: super::world::PostsWorld + ?Sized>(s: &Store, r: Ref<Character>, world: &mut W) -> Option<[f64; 3]> {
    let post = spawn_post(ch(s, r).objective)?;
    Some(posts::spawn_facts(s, post, world).position)
}
