//! Command posts (src/commandposts.js), following the 2004 game. Soldiers in
//! a post's capture region count for their team's capture team; N of a
//! team capture at 1 + 1/2 + ... (N terms) times the base rate; an owned
//! post that other teams stand in is neutralized over NeutralizeTime at the
//! sum of their rates, and that progress is lost when they leave; a neutral
//! post fills toward one team over CaptureTime, another team must first
//! drain it, two teams present freeze it, an empty one keeps its progress.
//! A post without a capture region never changes hands. A destructible
//! post (one with a building) goes inactive for the battle when its
//! building is destroyed. The command vehicles' posts follow the placed
//! ones in all() and owned() (src/mobile-posts.js).
//!
//! The state is the store's (rust/sim/schema/battle.mjs): the
//! `commandPosts` singleton and its posts, the `mobilePosts` singleton and
//! its command vehicles' posts.

use super::mobile::{self, MobileEvent};
use super::regions::in_region;
use super::world::PostsWorld;
use crate::gen::{Post, PostChange, PostEvent, PostRef, SpawnPath, SpawnPost, Store};
use crate::store::{NumMap, Ref};
use three::Vector3;

/// rate(count): 2 - 2^(1 - count).
fn rate(count: f64) -> f64 {
    2.0 - v8math::pow(2.0, 1.0 - count)
}

/// attackRate(counts, owner): the sum of the other teams' rates, in the
/// counts' order.
fn attack_rate(counts: &NumMap<f64>, owner: f64) -> f64 {
    let mut sum = 0.0;
    for (team, &n) in counts.iter() {
        if team != owner && n > 0.0 {
            sum += rate(n);
        }
    }
    sum
}

/// A post as the spawn manager sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnView {
    pub active: bool,
    /// ownerOf(post): post.team | 0.
    pub owner: f64,
    pub position: [f64; 3],
    pub spawn_path: Option<SpawnPath>,
    pub ally_path: Option<SpawnPath>,
    pub ally_count: f64,
    pub class_spawn_points: f64,
    pub yaw: f64,
}

/// A post changing: what Post.update and destroy() return.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Captured { by: f64 },
    Neutralized { from: f64 },
    /// Its building destroyed (`destroyed: true`).
    Destroyed { from: f64 },
}

impl Change {
    /// The event CommandPosts.update queues: `{ post, ...change }`.
    pub fn event(self, post: Ref<Post>) -> PostEvent {
        match self {
            Change::Captured { by } => PostEvent { post, kind: PostChange::Captured, from: None, by: Some(by), destroyed: None },
            Change::Neutralized { from } => PostEvent { post, kind: PostChange::Neutralized, from: Some(from), by: None, destroyed: None },
            Change::Destroyed { from } => PostEvent { post, kind: PostChange::Lost, from: Some(from), by: None, destroyed: Some(true) },
        }
    }
}

/// destroy() (commandposts.js:69): out of the battle, its owner kept.
pub fn destroy(post: &mut Post) -> Change {
    post.active = false;
    post.counts.clear();
    post.neutralize = 0.0;
    post.progress = 0.0;
    post.toward = 0.0;
    post.contested = false;
    Change::Destroyed { from: post.team }
}

/// contains(point) (commandposts.js:81).
pub fn contains(post: &Post, point: &Vector3) -> bool {
    post.capture.as_ref().is_some_and(|c| in_region(c, point))
}

/// The teams in a post's region, in the counts' order.
fn present(post: &Post) -> Vec<(f64, f64)> {
    post.counts.iter().filter(|(_, n)| **n > 0.0).map(|(t, n)| (t, *n)).collect()
}

/// remaining() (commandposts.js:86): seconds until the current capture or
/// neutralize lands, or Infinity.
pub fn remaining(post: &Post) -> f64 {
    let teams = present(post);
    if post.team != 0.0 && !post.team.is_nan() {
        let attack = attack_rate(&post.counts, post.team);
        return if attack != 0.0 && !attack.is_nan() { ((1.0 - post.neutralize) * post.neutralize_time) / attack } else { f64::INFINITY };
    }
    if teams.len() != 1 {
        return f64::INFINITY;
    }
    let (team, n) = teams[0];
    if team != post.toward && post.progress > 0.0 {
        return f64::INFINITY;
    }
    ((1.0 - post.progress) * post.capture_time) / rate(n)
}

/// update(dt) (commandposts.js:99).
pub fn update_post(post: &mut Post, dt: f64) -> Option<Change> {
    let teams = present(post);
    post.contested = teams.len() > 1;
    if post.team != 0.0 && !post.team.is_nan() {
        let attack = attack_rate(&post.counts, post.team);
        if attack == 0.0 || attack.is_nan() {
            post.neutralize = 0.0;
            return None;
        }
        post.neutralize += (attack * dt) / post.neutralize_time;
        if post.neutralize >= 1.0 {
            let lost = post.team;
            post.team = 0.0;
            post.neutralize = 0.0;
            post.progress = 0.0;
            post.toward = 0.0;
            return Some(Change::Neutralized { from: lost });
        }
        return None;
    }
    if teams.len() != 1 {
        return None;
    }
    let (team, n) = teams[0];
    let step = (rate(n) * dt) / post.capture_time;
    if post.progress > 0.0 && team != post.toward {
        post.progress = v8math::max(post.progress - step, 0.0);
        return None;
    }
    post.toward = team;
    post.progress += step;
    if post.progress >= 1.0 {
        post.team = team;
        post.progress = 0.0;
        post.toward = 0.0;
        return Some(Change::Captured { by: team });
    }
    None
}

/// A post in all()'s list as a Character's objective or spawn post.
pub fn post_ref(p: Option<SpawnPost>) -> PostRef {
    match p {
        Some(SpawnPost::Post(r)) => PostRef::Post(r),
        Some(SpawnPost::MobilePost(r)) => PostRef::MobilePost(r),
        None => PostRef::Null,
    }
}

/// A Character's post as one of all()'s (null and undefined: none).
pub fn spawn_post(p: PostRef) -> Option<SpawnPost> {
    match p {
        PostRef::Post(r) => Some(SpawnPost::Post(r)),
        PostRef::MobilePost(r) => Some(SpawnPost::MobilePost(r)),
        PostRef::Null | PostRef::Undefined => None,
    }
}

/// A soldier who can capture: his team and where he is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capturer {
    pub team: f64,
    pub position: Vector3,
}

/// all() (commandposts.js:349): every post the battle counts, the command
/// vehicles' as they are now.
pub fn all<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W) -> Vec<SpawnPost> {
    mobile::refresh_all(s, world);
    let mut out: Vec<SpawnPost> = s.command_posts.posts.iter().map(|&r| SpawnPost::Post(r)).collect();
    out.extend(s.mobile_posts.posts.iter().map(|&r| SpawnPost::MobilePost(r)));
    out
}

/// The team and whether it stands of a post in all()'s list.
pub fn team_of(s: &Store, p: SpawnPost) -> (f64, bool) {
    match p {
        SpawnPost::Post(r) => {
            let post = s.post.get(r).expect("a post");
            (post.team, post.active)
        }
        SpawnPost::MobilePost(r) => {
            let m = s.mobile_post.get(r).expect("a command vehicle's post");
            (m.team, m.active)
        }
    }
}

/// What the spawn manager reads of a post in all()'s list.
pub fn spawn_view<W: PostsWorld + ?Sized>(s: &Store, p: SpawnPost, world: &mut W) -> SpawnView {
    let owner = |team: f64| jsrt::conv::to_int32(team) as f64;
    match p {
        SpawnPost::Post(r) => {
            let post = s.post.get(r).expect("a post");
            let ai = post.ai.clone().unwrap_or_default();
            SpawnView {
                active: post.active,
                owner: owner(post.team),
                position: post.position,
                spawn_path: ai.spawn_path,
                ally_path: ai.ally_path,
                ally_count: ai.ally_count,
                class_spawn_points: ai.class_spawn_points,
                yaw: ai.yaw,
            }
        }
        SpawnPost::MobilePost(r) => {
            let post = s.mobile_post.get(r).expect("a command vehicle's post");
            let position = match post.vehicle {
                Some(v) if post.active => world.vehicle_position(v),
                _ => post.last_position,
            };
            SpawnView {
                active: post.active,
                owner: owner(post.team),
                position,
                spawn_path: None,
                ally_path: None,
                ally_count: 0.0,
                class_spawn_points: post.ai.class_spawn_points,
                yaw: post.yaw,
            }
        }
    }
}

/// A post's class spawn points where its hull is now (a command vehicle's:
/// classSpawnWorld); a placed post's are its record's.
pub fn class_spawn_world<W: PostsWorld + ?Sized>(s: &Store, p: SpawnPost, world: &mut W) -> Vec<[f64; 4]> {
    match p {
        SpawnPost::Post(r) => s.post.get(r).expect("a post").ai.as_ref().map(|ai| ai.class_spawn_world.clone()).unwrap_or_default(),
        SpawnPost::MobilePost(r) => {
            let post = s.mobile_post.get(r).expect("a command vehicle's post");
            match post.vehicle {
                Some(_) if post.active => world.class_spawn_world(r),
                _ => Vec::new(),
            }
        }
    }
}

/// A post's desc.valueBleed.
pub fn value_bleed(s: &Store, p: SpawnPost) -> Option<f64> {
    match p {
        SpawnPost::Post(r) => s.post.get(r).expect("a post").value_bleed,
        SpawnPost::MobilePost(r) => Some(s.mobile_post.get(r).expect("a command vehicle's post").desc.value_bleed),
    }
}

/// owned(team) (commandposts.js:414): the posts a team holds, its living
/// command vehicles' included.
pub fn owned<W: PostsWorld + ?Sized>(s: &mut Store, team: f64, world: &mut W) -> Vec<SpawnPost> {
    let list = all(s, world);
    list.into_iter()
        .filter(|p| {
            let (t, active) = team_of(s, *p);
            t == team && active
        })
        .collect()
}

/// The command vehicles' posts lost or changed hands since the last call
/// (mobileEvents).
pub fn mobile_events<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W) -> Vec<MobileEvent> {
    mobile::events(s, world)
}

/// at(point) (commandposts.js:409): the post whose capture region holds a
/// point.
pub fn at(s: &Store, point: &Vector3) -> Option<Ref<Post>> {
    s.command_posts.posts.iter().copied().find(|&r| {
        let p = s.post.get(r).expect("a post");
        p.active && contains(p, point)
    })
}

/// update(dt, soldiers) (commandposts.js:379) on a world (no camera). Each
/// soldier counts for his team's capture team (the registry's, when the
/// posts have its table: the world answers).
pub fn update<W: PostsWorld + ?Sized>(s: &mut Store, dt: f64, soldiers: &[Capturer], world: &mut W) {
    s.command_posts.time += dt;
    for i in 0..s.command_posts.posts.len() {
        let r = s.command_posts.posts[i];
        let post = s.post.get_mut(r).expect("a post");
        if !post.active {
            continue;
        }
        if let Some(b) = post.building {
            if !world.building_alive(b) {
                let change = destroy(post);
                s.command_posts.events.push(change.event(r));
                continue;
            }
        }
        post.counts.clear();
        for soldier in soldiers {
            let team = world.capture_team(soldier.team);
            if team == 0.0 || team.is_nan() || !contains(post, &soldier.position) {
                continue;
            }
            // counts.set(team, (counts.get(team) || 0) + 1)
            let n = post.counts.get(&team).copied().filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(0.0);
            post.counts.set(team, n + 1.0);
        }
        if let Some(change) = update_post(post, dt) {
            s.command_posts.events.push(change.event(r));
        }
    }
}
