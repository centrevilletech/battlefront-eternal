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
//! Characters are named by their place in the battle's list, which is the
//! manager's (`managerIndex`).

use super::posts::{CommandPosts, PostId, SpawnView};
use super::rules::{Battle, Character};
use super::world::BattleWorld;
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

impl Draws for jsrt::rng::ClassicRandom {
    fn next(&mut self) -> f64 {
        self.next_f64()
    }
}

/// A class in a team's roster: AddUnitClass's target, or a hero's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnClass {
    pub name: String,
    pub target: f64,
    pub hero: bool,
}

/// A team as the manager keeps it (makeTeams, then the mission's calls).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnTeam {
    pub index: f64,
    pub faction: f64,
    pub unit_count: f64,
    pub tickets: f64,
    pub classes: Vec<SpawnClass>,
    pub hero_count: f64,
    /// Team a's relation to team b (Int8Array).
    pub relations: [f64; TEAM_COUNT],
    /// Its Characters by place, in roster order.
    pub characters: Vec<u32>,
    /// Living ordinary (non-hero) Characters, the human included.
    pub living: f64,
    pub capacity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassicSpawning {
    pub teams: Vec<SpawnTeam>,
    pub a: f64,
    pub b: f64,
    pub q: f64,
    pub wave_countdown: f64,
    pub wave: f64,
    pub stagger: f64,
    pub released: f64,
    pub locals_cap: f64,
    /// Each post's queue (AI at an ally's post, humans there, AI at its own
    /// post, humans there), in the Map's insertion order.
    pub queues: Vec<(PostId, [f64; 4])>,
    /// Attract mode: no humans, quick first waves.
    pub demo: bool,
}

/// ownerOf(post): post.team | 0.
fn owner_of(team: f64) -> f64 {
    to_int32(team) as f64
}

/// The places posts are in all()'s list, and how the manager sees each.
struct Listed {
    ids: Vec<PostId>,
    views: Vec<SpawnView>,
}

fn listed(posts: &mut CommandPosts, world: &mut dyn BattleWorld) -> Listed {
    let ids = posts.all(world);
    let views = ids.iter().map(|p| posts.spawn_view(*p, world)).collect();
    Listed { ids, views }
}

impl ClassicSpawning {
    /// relation(a, b): team a's relation to team b (1 friend, 0 neutral, -1
    /// enemy).
    pub fn relation(&self, a: f64, b: f64) -> f64 {
        let ok = |t: f64| t >= 0.0 && t < TEAM_COUNT as f64 && t.fract() == 0.0;
        if ok(a) && ok(b) {
            self.teams[a as usize].relations[b as usize]
        } else {
            0.0
        }
    }

    fn team(&self, t: f64) -> &SpawnTeam {
        &self.teams[t as usize]
    }

    fn team_mut(&mut self, t: f64) -> &mut SpawnTeam {
        &mut self.teams[t as usize]
    }

    // ---- Spawn posts (src/ai/classic/goals.js).

    /// friendlySpawn(post, team, relation) (goals.js:163): its own post with
    /// a spawn path or class spawn points, or a post held by a friend that
    /// has an ally path; never a neutral or inactive one.
    fn friendly_spawn(&self, post: &SpawnView, team: f64) -> bool {
        if !post.active {
            return false;
        }
        let owner = post.owner;
        if owner == 0.0 {
            return false;
        }
        if owner == team {
            return post.spawn_path.as_ref().is_some_and(|p| p.present) || post.class_spawn_points > 0.0;
        }
        self.relation(owner, team) > 0.0 && post.ally_path.as_ref().is_some_and(|p| p.present)
    }

    /// selectSpawnPost (goals.js:278): uniformly among the posts its team can
    /// spawn at (no draw for just one), or nowhere.
    fn select_spawn_post(&self, list: &Listed, team: f64, random: &mut dyn Draws) -> Option<PostId> {
        let eligible: Vec<usize> = (0..list.ids.len()).filter(|&i| list.views[i].active && self.friendly_spawn(&list.views[i], team)).collect();
        if eligible.is_empty() {
            return None;
        }
        let i = random.int(0.0, eligible.len() as f64 - 1.0);
        Some(list.ids[eligible[i as usize]])
    }

    /// retainSpawnPost (goals.js:285): a bot keeps its post while it stays a
    /// valid spawn point for its team.
    fn retain_spawn_post(&self, post: Option<PostId>, list: &Listed, team: f64, random: &mut dyn Draws) -> Option<PostId> {
        if let Some(p) = post {
            if let Some(i) = list.ids.iter().position(|x| *x == p) {
                let view = &list.views[i];
                if view.active && self.friendly_spawn(view, team) {
                    return Some(p);
                }
            }
        }
        self.select_spawn_post(list, team, random)
    }

    /// humanSpawnPost (goals.js:300): the eligible post nearest the old one;
    /// with none yet, the eligible post nearest any post an enemy holds.
    fn human_spawn_post(&self, post: Option<PostId>, list: &Listed, team: f64) -> Option<PostId> {
        let distance = |a: &[f64; 3], b: &[f64; 3]| {
            let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
            fround(x * x + y * y + z * z)
        };
        let active: Vec<usize> = (0..list.ids.len()).filter(|&i| list.views[i].active).collect();
        if let Some(p) = post {
            let at = list.ids.iter().position(|x| *x == p).expect("a post in the list");
            let view = &list.views[at];
            if view.active {
                if self.friendly_spawn(view, team) {
                    return Some(p);
                }
                let mut best = FLT_MAX;
                let mut pick = None;
                for &o in &active {
                    if !self.friendly_spawn(&list.views[o], team) {
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
        for &o in &active {
            if !self.friendly_spawn(&list.views[o], team) {
                continue;
            }
            for &e in &active {
                if !(self.relation(team, list.views[e].owner) < 0.0) {
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
    fn choose_post(&self, c: &Character, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> Option<PostId> {
        let list = listed(posts, world);
        if c.human {
            self.human_spawn_post(c.spawn.post, &list, c.team)
        } else {
            self.retain_spawn_post(c.spawn.post, &list, c.team, random)
        }
    }

    /// The owner of a post, as the queues' categories ask it.
    fn own(c: &Character, post: PostId, posts: &CommandPosts) -> bool {
        c.team == owner_of(posts.team_of(post).0)
    }

    // ---- Post queues.

    fn queue_of(&mut self, post: PostId) -> &mut [f64; 4] {
        let at = match self.queues.iter().position(|(p, _)| *p == post) {
            Some(i) => i,
            None => {
                self.queues.push((post, [0.0; 4]));
                self.queues.len() - 1
            }
        };
        &mut self.queues[at].1
    }

    /// reserve(post, human, own) (spawning.js:513).
    fn reserve(&mut self, post: PostId, human: bool, own: bool) -> f64 {
        let queue = self.queue_of(post);
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
    fn release(&mut self, post: PostId, human: bool, own: bool) -> f64 {
        let queue = self.queue_of(post);
        let i = (if human { 1 } else { 0 }) + (if own { 2 } else { 0 });
        queue[i] -= 1.0;
        if human {
            queue[i]
        } else {
            queue[i] + queue[1 + if own { 2 } else { 0 }]
        }
    }

    /// changePost(character, post) (spawning.js:529): a held reservation
    /// moves with him.
    fn change_post(&mut self, c: &mut Character, post: Option<PostId>, posts: &CommandPosts) {
        if c.spawn.ordinal >= 0.0 && c.spawn.post != post {
            if let Some(old) = c.spawn.post {
                let own = Self::own(c, old, posts);
                self.release(old, c.human, own);
            }
            c.spawn.ordinal = -1.0;
            if let Some(p) = post {
                let own = Self::own(c, p, posts);
                c.spawn.ordinal = self.reserve(p, c.human, own);
            }
        }
        c.spawn.post = post;
    }

    // ---- Tickets.

    /// tickets(team) (spawning.js:541): the battle's reinforcements.
    pub fn tickets(&self, team: f64, battle: &Battle) -> f64 {
        match battle.reinforcements.at(team) {
            Some(r) => r,
            None => self.team(team).tickets,
        }
    }

    /// spendTicket(team) (spawning.js:546).
    fn spend_ticket(&mut self, team: f64, battle: &mut Battle) {
        if battle.reinforcements.at(team).is_some() {
            battle.spend(team, 1.0);
        } else {
            self.team_mut(team).tickets -= 1.0;
        }
    }

    // ---- Classes.

    /// setClass(character, index) (spawning.js:433): a hero class takes the
    /// hero's name too.
    fn set_class(&self, c: &mut Character, index: f64, world: &mut dyn BattleWorld) {
        let team = self.team(c.team);
        let cls = (index >= 0.0 && index.fract() == 0.0).then(|| team.classes.get(index as usize)).flatten();
        c.class_index = cls.map(|_| index);
        let Some(cls) = cls else { return };
        c.class_name = Some(cls.name.clone());
        c.hero = cls.hero;
        if cls.hero {
            if let Some(name) = world.hero_name(&cls.name).filter(|n| !n.is_empty()) {
                c.name = Some(name);
            }
        }
    }

    /// pickClass(character) (spawning.js:461): the roster-deficit draw. A
    /// hero keeps his class; no deficit anywhere keeps the current one.
    fn pick_class(&self, place: usize, characters: &[Character], random: &mut dyn Draws) -> f64 {
        let c = &characters[place];
        let team = self.team(c.team);
        let class_of = |i: Option<f64>| i.filter(|x| *x >= 0.0 && x.fract() == 0.0).and_then(|x| team.classes.get(x as usize));
        if c.class_index.is_some() && class_of(c.class_index).is_some_and(|k| k.hero) {
            return c.class_index.unwrap();
        }
        let mut counts = vec![0.0; team.classes.len()];
        for &other in &team.characters {
            let o = &characters[other as usize];
            if let (Some(i), Some(_)) = (o.class_index, class_of(o.class_index)) {
                counts[i as usize] += 1.0;
            }
        }
        let n = team.classes.len() as f64 - team.hero_count;
        let mut deficits = Vec::new();
        let mut total = 0.0;
        let mut i = 0.0;
        while i < n {
            let d = v8math::max(team.classes[i as usize].target - counts[i as usize], 0.0);
            deficits.push(d);
            total += d;
            i += 1.0;
        }
        if total > 0.0 {
            let r = random.next() % total;
            let mut sum = 0.0;
            for (i, d) in deficits.iter().enumerate() {
                sum += d;
                if r < sum {
                    return i as f64;
                }
            }
            return -1.0;
        }
        c.class_index.unwrap_or(0.0)
    }

    /// prepare(character) (spawning.js:483): a bot (unless a hero) picks its
    /// class and is ready, a human waits; it keeps or picks a post, joins
    /// its queue, and is scheduled for the next wave (two ahead if the wave
    /// clock is overdue).
    fn prepare(&mut self, place: usize, battle: &mut Battle, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
        if !battle.characters[place].human {
            if !battle.characters[place].hero {
                let index = self.pick_class(place, &battle.characters, random);
                self.set_class(&mut battle.characters[place], index, world);
            }
            battle.characters[place].spawn.ready = true;
        } else {
            battle.characters[place].spawn.ready = false;
        }
        let post = self.choose_post(&battle.characters[place], posts, world, random);
        self.change_post(&mut battle.characters[place], post, posts);
        let c = &battle.characters[place];
        if let Some(post) = c.spawn.post {
            let (human, own) = (c.human, Self::own(c, post, posts));
            battle.characters[place].spawn.ordinal = self.reserve(post, human, own);
        }
        battle.characters[place].spawn.wave = self.wave + 1.0 + if self.wave_countdown < 0.0 { 1.0 } else { 0.0 };
    }

    // ---- The update.

    /// awaitingHuman(character) (spawning.js:567).
    pub fn awaiting_human(c: &Character) -> bool {
        c.human && c.unit.is_none() && !c.spawn.ready && c.spawn.timer <= 0.0
    }

    /// clock(dt) (spawning.js:574): the manager's two clocks, one step
    /// each, and the locals' living cap.
    pub fn clock(&mut self, dt: f64, posts: &mut CommandPosts, world: &mut dyn BattleWorld) {
        let dt = fround(dt);
        let mut cap = 0.0;
        let list = listed(posts, world);
        for view in list.views.iter().filter(|v| v.active) {
            if self.relation(view.owner, 3.0) > 0.0 {
                cap += to_int32(view.ally_count) as f64;
            }
        }
        self.locals_cap = v8math::min(cap, self.teams[3].unit_count);
        let countdown = fround(self.wave_countdown - dt);
        self.wave_countdown = countdown;
        let mut stagger = self.stagger;
        if !(countdown > 0.0) {
            self.wave_countdown = fround(self.a + countdown);
            if self.demo && self.wave < 3.0 {
                self.wave_countdown = 1.0;
            }
            self.wave += 1.0;
            stagger = 0.0;
            self.released = 0.0;
        }
        stagger = fround(stagger - dt);
        self.stagger = stagger;
        if stagger <= 0.0 {
            self.released += 1.0;
            self.stagger = fround(self.b + stagger);
        }
    }

    /// waiting(character) (spawning.js:601): whether the wave and stagger
    /// gate holds him back.
    fn waiting(&self, c: &Character) -> bool {
        (c.spawn.wave - self.wave) * self.q - self.released + c.spawn.ordinal >= 0.0
    }

    /// updateCharacters(dt) (spawning.js:561): every Character in roster
    /// order.
    pub fn update_characters(&mut self, dt: f64, battle: &mut Battle, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
        for place in 0..battle.characters.len() {
            self.update_character(place, dt, battle, posts, world, random);
        }
    }

    /// updateCharacter(character, dt) (spawning.js:605).
    fn update_character(&mut self, place: usize, dt: f64, battle: &mut Battle, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
        let dt = fround(dt);
        let mut t = battle.characters[place].team;
        if let Some(unit) = battle.characters[place].unit {
            // A hero dies when its team's tickets fall to its living count.
            let c = &battle.characters[place];
            if c.hero && self.tickets(c.team, battle) <= self.team(t).living && world.health(unit) > 0.0 {
                world.force_kill(place as u32, unit);
            }
            return;
        }
        let timer = fround(battle.characters[place].spawn.timer - dt);
        battle.characters[place].spawn.timer = timer;
        if timer > 0.0 {
            return;
        }
        if timer > -dt {
            self.prepare(place, battle, posts, world, random);
        }
        let human = battle.characters[place].human;
        if !human && self.tickets(t, battle) <= self.team(t).living {
            return;
        }
        if !human && self.team(t).characters.len() as f64 > self.team(t).capacity {
            self.change_post(&mut battle.characters[place], None, posts);
            self.switch_team(place, 3.0 - t, battle, posts, world, random);
            world.request_objective(&mut battle.characters[place], place as u32);
            t = battle.characters[place].team;
        }
        let post = self.choose_post(&battle.characters[place], posts, world, random);
        self.change_post(&mut battle.characters[place], post, posts);
        let c = &battle.characters[place];
        if !c.spawn.ready || c.spawn.post.is_none() {
            battle.characters[place].spawn.wave = self.wave + 1.0;
            return;
        }
        if !c.human {
            if self.waiting(c) {
                return;
            }
            if !c.hero && t > 2.0 {
                let mut living = 0.0;
                for &other in &self.team(t).characters {
                    let o = &battle.characters[other as usize];
                    if o.unit.is_some() && !o.human {
                        living += 1.0;
                    }
                }
                if self.locals_cap <= living {
                    return;
                }
            }
        }
        self.spawn(place, battle, posts, world, random);
    }

    /// spawnPoint(post, allied) (spawning.js:651): a random point of the
    /// post's spawn path (its ally path when the post is an ally's), else a
    /// random class spawn point (own posts only), else the post itself.
    fn spawn_point(&self, post: PostId, allied: bool, posts: &CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> Vec<f64> {
        let view = posts.spawn_view(post, world);
        let path = if allied { view.ally_path.as_ref() } else { view.spawn_path.as_ref() };
        if let Some(points) = path.filter(|p| p.present).map(|p| &p.points) {
            if !points.is_empty() {
                let i = random.next() % points.len() as f64;
                return points[i as usize].clone();
            }
        }
        if !allied && view.class_spawn_points > 0.0 {
            let world_points = posts.class_spawn_world(post, world);
            if !world_points.is_empty() {
                let i = random.next() % view.class_spawn_points;
                return world_points.get(i as usize).cloned().unwrap_or_else(|| world_points[0].clone());
            }
        }
        let yaw = if view.yaw != 0.0 && !view.yaw.is_nan() { view.yaw } else { 0.0 };
        vec![view.position[0], view.position[1], view.position[2], yaw]
    }

    /// spawn(character) (spawning.js:664): into the world; if the class
    /// can't be made, class 0 is tried once.
    fn spawn(&mut self, place: usize, battle: &mut Battle, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> Option<u32> {
        let post = battle.characters[place].spawn.post.expect("a post");
        let t = battle.characters[place].team;
        let allied = t != owner_of(posts.team_of(post).0);
        let point = self.spawn_point(post, allied, posts, world, random);
        let at = |i: usize| point.get(i).copied().unwrap_or(f64::NAN);
        let (xyz, yaw) = ([at(0), at(1), at(2)], point.get(3).copied());
        let class_name = battle.characters[place].class_name.clone().unwrap_or_default();
        let mut unit = world.spawn_unit(&mut battle.characters[place], place as u32, &class_name, Some(post), xyz, yaw);
        if unit.is_none() {
            let c = &battle.characters[place];
            if c.class_index.is_none_or(|i| i == 0.0 || i.is_nan()) {
                return None;
            }
            self.set_class(&mut battle.characters[place], 0.0, world);
            let class_name = battle.characters[place].class_name.clone().unwrap_or_default();
            unit = world.spawn_unit(&mut battle.characters[place], place as u32, &class_name, Some(post), xyz, yaw);
            unit?;
        }
        let c = &mut battle.characters[place];
        c.unit = unit;
        let team = &self.teams[t as usize];
        let cls = c.class_index.filter(|i| *i >= 0.0 && i.fract() == 0.0).and_then(|i| team.classes.get(i as usize));
        c.hero = cls.is_some_and(|k| k.hero);
        if !c.hero {
            self.teams[t as usize].living += 1.0;
        }
        if !battle.characters[place].human {
            world.republish(&mut battle.characters[place], place as u32);
        }
        let c = &battle.characters[place];
        if c.spawn.ordinal >= 0.0 {
            let (human, own) = (c.human, Self::own(c, post, posts));
            self.release(post, human, own);
            battle.characters[place].spawn.ordinal = -1.0;
        }
        unit
    }

    // ---- Events.

    /// onDeath(character) (spawning.js:690): he prepares again in 4 s; a bot
    /// draws a fresh objective and forgets its post; the team's living count
    /// drops (heroes aren't counted) and it spends a reinforcement.
    pub fn on_death(&mut self, place: usize, battle: &mut Battle, world: &mut dyn BattleWorld) {
        let t = battle.characters[place].team;
        battle.characters[place].spawn.timer = RESPAWN_DELAY;
        if !battle.characters[place].human {
            world.request_objective(&mut battle.characters[place], place as u32);
            battle.characters[place].spawn.post = None;
        }
        battle.characters[place].unit = None;
        if !battle.characters[place].hero {
            self.team_mut(t).living -= 1.0;
        }
        if self.tickets(t, battle) > 0.0 {
            self.spend_ticket(t, battle);
        }
    }

    /// switchTeam(character, to) (spawning.js:723): a non-hero Character to
    /// another team with room; a bot picks a class there.
    pub fn switch_team(&mut self, place: usize, to: f64, battle: &mut Battle, posts: &mut CommandPosts, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> bool {
        let c = &battle.characters[place];
        if c.hero || c.team == to || to < 1.0 || to >= TEAM_COUNT as f64 {
            return false;
        }
        if self.team(to).characters.len() as f64 >= self.team(to).capacity {
            return false;
        }
        let from = c.team;
        let held = c.spawn.ordinal >= 0.0;
        self.change_post(&mut battle.characters[place], None, posts);
        // splice(indexOf(character), 1): not found (-1) takes the last.
        let list = &mut self.team_mut(from).characters;
        match list.iter().position(|x| *x == place as u32) {
            Some(i) => {
                list.remove(i);
            }
            None => {
                list.pop();
            }
        }
        self.team_mut(to).characters.push(place as u32);
        battle.characters[place].team = to;
        if !battle.characters[place].human {
            let index = self.pick_class(place, &battle.characters, random);
            self.set_class(&mut battle.characters[place], index, world);
        } else {
            let last = self.team(to).classes.len() as f64 - 1.0;
            let index = v8math::min(v8math::max(battle.characters[place].class_index.unwrap_or(0.0), 0.0), last);
            self.set_class(&mut battle.characters[place], index, world);
        }
        let post = self.choose_post(&battle.characters[place], posts, world, random);
        self.change_post(&mut battle.characters[place], post, posts);
        if let (true, Some(p)) = (held, post) {
            let c = &battle.characters[place];
            let (human, own) = (c.human, to == owner_of(posts.team_of(p).0));
            battle.characters[place].spawn.ordinal = self.reserve(p, human, own);
        }
        true
    }
}
