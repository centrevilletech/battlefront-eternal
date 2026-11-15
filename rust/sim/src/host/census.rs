//! A once-in-a-while line about the battle's bots, for comparing hosts with
//! each other and with the browser build: per team the bots alive, those
//! that moved more than a metre in the last second and those seated, the
//! team's kills and deaths, its reinforcements and posts, and the shots and
//! damage the journal heard since the last line. A host calls `sample`
//! after every tick; it reads the world only once a second.
use crate::abi::context::{Context, Error};
use std::collections::HashMap;
use std::fmt::Write;

const TEAMS: usize = 8;

#[derive(Default)]
pub struct Census {
    /// Each unit's horizontal position and life a second ago, by store slot.
    last: HashMap<u32, (f64, f64, f64)>,
    journal_seen: usize,
}

#[derive(Clone, Copy, Default)]
struct Team {
    present: bool,
    alive: u32,
    moving: u32,
    seated: u32,
    kills: f64,
    deaths: f64,
    posts: u32,
}

impl Census {
    /// Answers a line every `every` ticks (and reads positions the second
    /// before it), else None.
    pub fn sample(&mut self, context: &Context, tick: u64, every: u64) -> Result<Option<String>, Error> {
        let every = every.max(120);
        let phase = tick % every;
        if phase != 0 && phase != every - 60 {
            return Ok(None);
        }
        let report = phase == 0;
        let last = std::mem::take(&mut self.last);
        let (teams, reinforcements) = context.owned(|w| {
            let store = &w.sim.store;
            let mut teams = [Team::default(); TEAMS];
            let mut next = HashMap::with_capacity(last.len());
            for (r, unit) in store.unit.iter() {
                let p = unit.position();
                next.insert(r.slot, (p.x, p.z, unit.generation));
                if unit.human || !(0.0..TEAMS as f64).contains(&unit.team) {
                    continue;
                }
                let t = &mut teams[unit.team as usize];
                t.present = true;
                if !unit.alive || unit.dead {
                    continue;
                }
                t.alive += 1;
                t.seated += u32::from(unit.seat.is_some());
                if let Some(&(x, z, life)) = last.get(&r.slot) {
                    if life == unit.generation && (p.x - x).hypot(p.z - z) > 1.0 {
                        t.moving += 1;
                    }
                }
            }
            for (_, c) in store.character.iter() {
                if (0.0..TEAMS as f64).contains(&c.team) {
                    let t = &mut teams[c.team as usize];
                    t.kills += c.kills;
                    t.deaths += c.deaths;
                }
            }
            for (_, post) in store.post.iter() {
                if (0.0..TEAMS as f64).contains(&post.team) {
                    teams[post.team as usize].posts += 1;
                }
            }
            let reinforcements: Vec<(f64, f64)> = store.battle.reinforcements.iter().filter(|(t, _)| *t < 3.0).map(|(t, v)| (t, *v)).collect();
            self.last = next;
            (teams, reinforcements)
        })?;
        if !report {
            return Ok(None);
        }
        let seen = self.journal_seen;
        let (shots, damage, head) = context.world_host_journal(|journal| {
            let head = journal.seq;
            let start = seen.max(head.saturating_sub(super::journal::CAPACITY));
            let (mut shots, mut damage) = (0u32, 0u32);
            for record in journal.records().filter(|r| r.seq >= start && r.seq < head) {
                match record.event.kind() {
                    "shot" => shots += 1,
                    "damage" => damage += 1,
                    _ => {}
                }
            }
            (shots, damage, head)
        })?;
        self.journal_seen = head;
        let mut line = format!("t={}s", tick / 60);
        for (index, t) in teams.iter().enumerate().filter(|(_, t)| t.present) {
            let _ = write!(line, " t{index}:{}/{}/{} kd{}-{}", t.alive, t.moving, t.seated, t.kills, t.deaths);
        }
        let _ = write!(line, " reinf=");
        for (team, count) in &reinforcements {
            let _ = write!(line, "{team}:{count},");
        }
        let _ = write!(line, " posts=");
        for (index, t) in teams.iter().enumerate().filter(|(_, t)| t.posts > 0) {
            let _ = write!(line, "{index}:{},", t.posts);
        }
        let _ = write!(line, " shots={shots} dmg={damage}");
        Ok(Some(line))
    }
}
