//! The battle's rules (src/battle.js): who fights for each team (its
//! Characters), their reinforcements, the bleed of reinforcements from
//! holding fewer posts, how a team loses, and each Character's scores.
//! Teams 1 and 2 are the armies; 3 to 7 the locals, who neither bleed nor
//! count down. Characters are named by their place in `characters`.

use super::posts::CommandPosts;
use super::stats::{BattleStats, Record, Seen};
use super::teams::TeamMap;
use super::world::BattleWorld;
use crate::store::Nullable;

/// GAME_OVER_DELAY: s from a team's last reinforcement to the game-over flag.
pub const GAME_OVER_DELAY: f64 = 2.0;
/// END_SEQUENCE: s from the game-over flag to the end-of-match screens.
pub const END_SEQUENCE: f64 = 7.0;
/// The no-post countdown, s.
pub const NO_POST: f64 = 20.0;

/// A Character (battle.js:52): a fighter for a team, who outlives its
/// bodies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Character {
    /// Within the team.
    pub index: f64,
    pub team: f64,
    pub human: bool,
    /// A bot's comes with its roster.
    pub name: Option<String>,
    /// The class of its current or next body.
    pub class_name: Option<String>,
    /// The living (or dying) body: its unit's registry id.
    pub unit: Option<u32>,
    /// A hero's (heroes do not count among the living).
    pub hero: bool,
    pub kills: f64,
    pub team_kills: f64,
    pub deaths: f64,
    /// Command post points (16 bits, as the original keeps them).
    pub captures: f64,
    /// Seconds before being outside the battlefield kills him; -1 inside.
    pub out_of_bounds: f64,
    /// The end-of-match tallies (src/battle-stats.js), made when needed.
    pub record: Option<Record>,
}

impl Character {
    pub fn new(index: f64, team: f64, human: bool) -> Character {
        Character { index, team, human, out_of_bounds: -1.0, ..Default::default() }
    }
}

/// A team as the mission's table gives it (map.json mission.teams).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamDesc {
    /// SetUnitCount.
    pub units: f64,
    /// Its reinforcements, if the mission gives them.
    pub reinforcements: Option<f64>,
    /// [threshold, rate] pairs, as the mission lists them.
    pub bleed: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battle {
    pub own: f64,
    pub foe: f64,
    pub teams: TeamMap<TeamDesc>,
    pub reinforcements: TeamMap<f64>,
    pub start: TeamMap<f64>,
    /// The teams, ascending.
    pub team_ids: Vec<f64>,
    pub no_post: TeamMap<f64>,
    pub bleed: TeamMap<f64>,
    pub victory_clock: Nullable<f64>,
    pub countdown: Nullable<f64>,
    pub losing: bool,
    /// A team is out of reinforcements.
    pub over: bool,
    pub loser: Nullable<f64>,
    /// s since.
    pub over_time: f64,
    /// The scores stop counting.
    pub game_over: bool,
    /// The end-of-match screens take over.
    pub finished: bool,
    pub stats: BattleStats,
    /// Each team's bleed rules, lowest threshold first.
    pub bleed_rules: TeamMap<Vec<[f64; 2]>>,
    pub bleed_clock: TeamMap<f64>,
    pub characters: Vec<Character>,
}

impl Battle {
    /// new Battle({ teams, posts, own, foe }) (battle.js:92).
    pub fn new(teams: TeamMap<TeamDesc>, own: f64, foe: f64) -> Battle {
        let reinforcements: TeamMap<f64> = teams
            .entries()
            .map(|(t, v)| (*t, v.reinforcements.unwrap_or(if *t >= 3.0 { 2147483647.0 } else { 0.0 })))
            .collect();
        let team_ids: Vec<f64> = teams.entries().map(|(t, _)| *t).collect();
        let no_post = team_ids.iter().map(|t| (*t, NO_POST)).collect();
        let bleed_rules = teams
            .entries()
            .map(|(t, v)| {
                let mut rules = v.bleed.clone();
                // [...rules].sort((a, b) => a[0] - b[0]): a stable sort.
                jsrt::sort::js_sort(&mut rules, &mut |a: &[f64; 2], b: &[f64; 2]| a[0] - b[0]);
                (*t, rules)
            })
            .collect();
        Battle {
            own,
            foe,
            start: reinforcements.clone(),
            reinforcements,
            team_ids,
            no_post,
            victory_clock: Nullable::Null,
            countdown: Nullable::Null,
            loser: Nullable::Null,
            bleed_rules,
            teams,
            ..Default::default()
        }
    }

    /// spend(team, count) (battle.js:159): a death costs the team a
    /// reinforcement.
    pub fn spend(&mut self, team: f64, count: f64) {
        let left = self.reinforcements.at(team).unwrap_or(count);
        self.reinforcements.set(team, v8math::max(left - count, 0.0));
    }

    /// credit(victim, killer, relation) (battle.js:170).
    pub fn credit(&mut self, victim: Option<u32>, killer: Option<u32>, relation: f64) {
        if self.game_over {
            return;
        }
        if let Some(k) = killer {
            if relation != 0.0 {
                let c = &mut self.characters[k as usize];
                c.kills += 1.0;
                if relation > 0.0 {
                    c.team_kills += 1.0;
                }
            }
        }
        self.stats.kill(&mut self.characters, killer, victim, relation);
        if let Some(v) = victim {
            self.characters[v as usize].deaths += 1.0;
        }
        self.stats.death(&mut self.characters, victim);
    }

    /// headshot(credited) (battle.js:181).
    pub fn headshot(&mut self, credited: Option<u32>) {
        if !self.game_over {
            self.stats.headshot(&mut self.characters, credited);
        }
    }

    /// vehicleDestroyed(destroyer) (battle.js:186).
    pub fn vehicle_destroyed(&mut self, destroyer: Option<u32>) {
        if !self.game_over {
            self.stats.vehicle_destroyed(&mut self.characters, destroyer);
        }
    }

    /// point(character) (battle.js:192): a command post point.
    pub fn point(&mut self, character: Option<u32>) {
        let Some(c) = character else { return };
        if self.game_over {
            return;
        }
        let c = &mut self.characters[c as usize];
        c.captures = ((jsrt::conv::to_int32(c.captures + 1.0)) & 0xffff) as f64;
    }

    /// postDestroyed(destroyer, relation) (battle.js:201).
    pub fn post_destroyed(&mut self, destroyer: Option<u32>, relation: f64) {
        if relation < 1.0 {
            self.point(destroyer);
        }
    }

    /// The living soldiers of a team, heroes excepted: the spawn manager's
    /// count, else the Characters' own (battle.js:111).
    pub fn living(&self, team: f64, world: &mut dyn BattleWorld) -> f64 {
        if let Some(n) = world.living(team) {
            return n;
        }
        let mut n = 0.0;
        for c in &self.characters {
            if c.team == team && !c.hero && c.unit.is_some_and(|u| world.unit_alive(u)) {
                n += 1.0;
            }
        }
        n
    }

    /// update(dt) (battle.js:205). `stats_active(character)`: whether his
    /// tallies run this tick (battle.statsActive).
    pub fn update(&mut self, dt: f64, posts: &mut CommandPosts, stats_active: impl Fn(&Character) -> bool, world: &mut dyn BattleWorld) {
        if !self.game_over {
            self.stats.tick(dt, &mut self.characters, stats_active, |i, c| -> Option<Seen> {
                world.observe(i, c.unit)
            });
        }
        if self.over {
            self.over_time += dt;
            if self.over_time >= GAME_OVER_DELAY {
                self.game_over = true;
            }
            if self.over_time >= GAME_OVER_DELAY + END_SEQUENCE {
                self.finished = true;
            }
            return;
        }
        if posts.posts.is_empty() {
            return;
        }
        for i in 0..self.team_ids.len() {
            let team = self.team_ids[i];
            self.update_team(team, dt, posts, world);
        }
        // What the HUD shows: the main team counting down with no posts.
        let mut stranded = None;
        for team in [self.own, self.foe] {
            let score = self.score(team, posts, world);
            if score == 0.0 || score.is_nan() {
                stranded = Some(team);
                break;
            }
        }
        self.victory_clock = match stranded {
            // `stranded ? this.noPost[stranded] : null` (no team is 0).
            Some(t) if t != 0.0 && !t.is_nan() => match self.no_post.at(t) {
                Some(v) => Nullable::Value(v),
                None => Nullable::Undefined,
            },
            _ => Nullable::Null,
        };
        self.countdown = self.victory_clock;
        self.losing = stranded == Some(self.own);
        let loser = [self.foe, self.own].into_iter().find(|t| self.reinforcements.at(*t).unwrap_or(1.0) <= 0.0);
        if let Some(loser) = loser.filter(|t| *t != 0.0 && !t.is_nan()) {
            self.over = true;
            self.loser = Nullable::Value(loser);
            self.countdown = Nullable::Null;
            world.battle_over(loser);
        }
    }

    /// score(team) (battle.js:237): the ValueBleed of each post it holds
    /// (its command vehicles' too), plus 1 per neutral post.
    pub fn score(&self, team: f64, posts: &mut CommandPosts, world: &mut dyn BattleWorld) -> f64 {
        let mut owned = 0.0;
        for p in posts.owned(team, world) {
            owned += posts.value_bleed(p).unwrap_or(10.0);
        }
        owned + posts.owned(0.0, world).len() as f64
    }

    /// updateTeam(team, dt) (battle.js:242).
    fn update_team(&mut self, team: f64, dt: f64, posts: &mut CommandPosts, world: &mut dyn BattleWorld) {
        if team > 2.0 {
            self.update_locals(team, world);
            return;
        }
        let owned = posts.owned(team, world).len();
        let score = self.score(team, posts, world);
        let rules = self.bleed_rules.get(team).filter(|r| !r.is_empty()).cloned();
        if let Some(rules) = rules {
            let rate = rules.iter().find(|r| score <= r[0]).map(|r| r[1]).unwrap_or(0.0);
            self.bleed.set(team, rate);
            // `(this.bleedClock[team] || 0) + this.bleed[team] * dt`
            let clock = self.bleed_clock.at(team).filter(|c| *c != 0.0 && !c.is_nan()).unwrap_or(0.0);
            self.bleed_clock.set(team, clock + rate * dt);
            if self.bleed_clock.at(team).unwrap() >= 4.0 {
                *self.bleed_clock.get_mut(team).unwrap() -= 4.0;
                if self.reinforcements.at(team).is_some_and(|r| r > 0.0) {
                    *self.reinforcements.get_mut(team).unwrap() -= 1.0;
                }
            }
        }
        if score != 0.0 && !score.is_nan() {
            if owned > 0 {
                self.no_post.set(team, NO_POST);
            }
        } else {
            let left = self.no_post.at(team).unwrap_or(f64::NAN) - dt;
            self.no_post.set(team, left);
            if left <= 0.0 {
                self.reinforcements.set(team, 0.0);
                self.start.set(team, 0.0);
            }
        }
    }

    /// updateLocals(team) (battle.js:268): a local team friendly with
    /// exactly one army follows it out of the battle.
    fn update_locals(&mut self, team: f64, world: &mut dyn BattleWorld) {
        let mut ally = 0.0;
        for army in [1.0, 2.0] {
            if world.relation(team, army) > 0.0 {
                ally = if ally != 0.0 { -1.0 } else { army };
            }
        }
        if ally > 0.0 && self.teams.get(ally).is_some() && self.reinforcements.at(ally).unwrap_or(0.0) <= self.living(ally, world) {
            self.reinforcements.set(team, 0.0);
        }
    }
}
