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

use super::mobile::MobilePosts;
use super::regions::{in_region, Built, Region};
use super::world::BattleWorld;
use three::Vector3;

/// rate(count): 2 - 2^(1 - count).
fn rate(count: f64) -> f64 {
    2.0 - v8math::pow(2.0, 1.0 - count)
}

/// attackRate(counts, owner): the sum of the other teams' rates, in the
/// counts' order.
fn attack_rate(counts: &[(f64, f64)], owner: f64) -> f64 {
    let mut sum = 0.0;
    for &(team, n) in counts {
        if team != owner && n > 0.0 {
            sum += rate(n);
        }
    }
    sum
}

/// The map's command post rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostRules {
    pub neutralize_time: f64,
    pub capture_time: f64,
}

impl Default for PostRules {
    fn default() -> Self {
        PostRules { neutralize_time: 12.0, capture_time: 10.0 }
    }
}

/// A post changing: what Post.update and destroy() return.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Captured { by: f64 },
    Neutralized { from: f64 },
    /// Its building destroyed (`destroyed: true`).
    Destroyed { from: f64 },
}

/// A placed post (commandposts.js Post).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Post {
    pub name: String,
    pub label: Option<String>,
    pub team: f64,
    pub position: Vector3,
    pub capture: Option<Built>,
    /// desc.valueBleed.
    pub value_bleed: Option<f64>,
    /// desc.killRegion (src/objectives.js).
    pub kill_region: Option<Region>,
    pub rules: PostRules,
    /// Neutralize progress (0..1) against the owner.
    pub neutralize: f64,
    /// Capture progress (0..1) toward `toward` while neutral.
    pub progress: f64,
    pub toward: f64,
    /// Team -> soldiers in the region, in the Map's insertion order.
    pub counts: Vec<(f64, f64)>,
    pub contested: bool,
    /// A destructible one's building (its place in the live objects'
    /// `objectives`).
    pub building: Option<u32>,
    pub active: bool,
}

impl Post {
    /// destroy() (commandposts.js:69): out of the battle, its owner kept.
    pub fn destroy(&mut self) -> Change {
        self.active = false;
        self.counts.clear();
        self.neutralize = 0.0;
        self.progress = 0.0;
        self.toward = 0.0;
        self.contested = false;
        Change::Destroyed { from: self.team }
    }

    /// contains(point) (commandposts.js:81).
    pub fn contains(&self, point: &Vector3) -> bool {
        self.capture.as_ref().is_some_and(|c| in_region(c, point))
    }

    /// remaining() (commandposts.js:86): seconds until the current capture
    /// or neutralize lands, or Infinity.
    pub fn remaining(&self) -> f64 {
        let teams: Vec<(f64, f64)> = self.counts.iter().copied().filter(|(_, n)| *n > 0.0).collect();
        if self.team != 0.0 && !self.team.is_nan() {
            let attack = attack_rate(&self.counts, self.team);
            return if attack != 0.0 && !attack.is_nan() { ((1.0 - self.neutralize) * self.rules.neutralize_time) / attack } else { f64::INFINITY };
        }
        if teams.len() != 1 {
            return f64::INFINITY;
        }
        let (team, n) = teams[0];
        if team != self.toward && self.progress > 0.0 {
            return f64::INFINITY;
        }
        ((1.0 - self.progress) * self.rules.capture_time) / rate(n)
    }

    /// update(dt) (commandposts.js:99).
    pub fn update(&mut self, dt: f64) -> Option<Change> {
        let teams: Vec<(f64, f64)> = self.counts.iter().copied().filter(|(_, n)| *n > 0.0).collect();
        self.contested = teams.len() > 1;
        if self.team != 0.0 && !self.team.is_nan() {
            let attack = attack_rate(&self.counts, self.team);
            if attack == 0.0 || attack.is_nan() {
                self.neutralize = 0.0;
                return None;
            }
            self.neutralize += (attack * dt) / self.rules.neutralize_time;
            if self.neutralize >= 1.0 {
                let lost = self.team;
                self.team = 0.0;
                self.neutralize = 0.0;
                self.progress = 0.0;
                self.toward = 0.0;
                return Some(Change::Neutralized { from: lost });
            }
            return None;
        }
        if teams.len() != 1 {
            return None;
        }
        let (team, n) = teams[0];
        let step = (rate(n) * dt) / self.rules.capture_time;
        if self.progress > 0.0 && team != self.toward {
            self.progress = v8math::max(self.progress - step, 0.0);
            return None;
        }
        self.toward = team;
        self.progress += step;
        if self.progress >= 1.0 {
            self.team = team;
            self.progress = 0.0;
            self.toward = 0.0;
            return Some(Change::Captured { by: team });
        }
        None
    }
}

/// A post in all()'s list: a placed one, then the command vehicles'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostId {
    Placed(usize),
    Mobile(usize),
}

/// One of the events update() queues (CommandPosts.events).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostEvent {
    pub post: usize,
    pub change: Change,
}

/// A soldier who can capture: his team and where he is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capturer {
    pub team: f64,
    pub position: Vector3,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandPosts {
    pub posts: Vec<Post>,
    pub time: f64,
    pub events: Vec<PostEvent>,
    pub mobile: Option<MobilePosts>,
    /// Whether the registry's team table is set (who is whose friend):
    /// without it every soldier counts for his own team.
    pub teams: bool,
}

impl CommandPosts {
    /// all() (commandposts.js:349): every post the battle counts, the
    /// command vehicles' as they are now.
    pub fn all(&mut self, world: &mut dyn BattleWorld) -> Vec<PostId> {
        let mut out: Vec<PostId> = (0..self.posts.len()).map(PostId::Placed).collect();
        if let Some(m) = self.mobile.as_mut() {
            m.refresh(world);
            // `listed` holds them only when there are any.
            out.extend((0..m.posts.len()).map(PostId::Mobile));
        }
        out
    }

    /// The team and whether it stands of a post in all()'s list.
    pub fn team_of(&self, p: PostId) -> (f64, bool) {
        match p {
            PostId::Placed(i) => (self.posts[i].team, self.posts[i].active),
            PostId::Mobile(i) => {
                let m = &self.mobile.as_ref().unwrap().posts[i];
                (m.team, m.active)
            }
        }
    }

    /// A post's desc.valueBleed.
    pub fn value_bleed(&self, p: PostId) -> Option<f64> {
        match p {
            PostId::Placed(i) => self.posts[i].value_bleed,
            PostId::Mobile(i) => Some(self.mobile.as_ref().unwrap().posts[i].desc.value_bleed),
        }
    }

    /// owned(team) (commandposts.js:414): the posts a team holds, its living
    /// command vehicles' included.
    pub fn owned(&mut self, team: f64, world: &mut dyn BattleWorld) -> Vec<PostId> {
        let all = self.all(world);
        all.into_iter()
            .filter(|p| {
                let (t, active) = self.team_of(*p);
                t == team && active
            })
            .collect()
    }

    /// The command vehicles' posts lost or changed hands since the last
    /// call (mobileEvents).
    pub fn mobile_events(&mut self, world: &mut dyn BattleWorld) -> Vec<super::mobile::MobileEvent> {
        match self.mobile.as_mut() {
            Some(m) => m.events(world),
            None => Vec::new(),
        }
    }

    /// at(point) (commandposts.js:409): the post whose capture region holds
    /// a point.
    pub fn at(&self, point: &Vector3) -> Option<usize> {
        self.posts.iter().position(|p| p.active && p.contains(point))
    }

    /// update(dt, soldiers) (commandposts.js:379) on a world (no camera).
    pub fn update(&mut self, dt: f64, soldiers: &[Capturer], world: &mut dyn BattleWorld) {
        self.time += dt;
        for i in 0..self.posts.len() {
            let post = &mut self.posts[i];
            if !post.active {
                continue;
            }
            if let Some(b) = post.building {
                if !world.building_alive(b) {
                    let change = post.destroy();
                    self.events.push(PostEvent { post: i, change });
                    continue;
                }
            }
            post.counts.clear();
            for soldier in soldiers {
                let team = if self.teams { world.capture_team(soldier.team) } else { soldier.team };
                if team == 0.0 || team.is_nan() || !post.contains(&soldier.position) {
                    continue;
                }
                match post.counts.iter_mut().find(|(t, _)| *t == team) {
                    Some(entry) => entry.1 += 1.0,
                    None => post.counts.push((team, 1.0)),
                }
            }
            if let Some(change) = post.update(dt) {
                self.events.push(PostEvent { post: i, change });
            }
        }
    }
}
