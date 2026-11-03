//! What the end-of-match screens report for each Character
//! (src/battle-stats.js; docs/scoreboard.md), kept as the 2004 game keeps
//! its per-Character stats record: per team slot (team 2's slot 1, every
//! other team's slot 0), a ring of the last hundred kills shared by
//! everyone, and time tables of 5 classes, 10 weapons and 5 vehicles a slot
//! where a new entry takes the place of the one with the least time.
//!
//! Characters are named by their place in the battle's list (`u32`).

use super::rules::Character;

pub const SPREE_WINDOW: f64 = 5.0;
pub const RING_SIZE: usize = 100;
pub const CAMP_RADIUS: f64 = 10.0;
pub const SURVIVAL_HEALTH: f64 = 64.0;
pub const ESCAPE_LOW: f64 = 0.1;
const CLASSES: usize = 5;
const WEAPONS: usize = 10;
const VEHICLES: usize = 5;

/// slotOf(team): team 2's slot 1, every other team's slot 0.
pub fn slot_of(team: f64) -> usize {
    if team == 2.0 {
        1
    } else {
        0
    }
}

/// A string field's truthiness (null and "" are false).
pub fn truthy_str(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.is_empty())
}

/// `value & 0xff` on a count (counts stay small integers).
fn u8_of(value: f64) -> f64 {
    (jsrt::conv::to_int32(value) & 0xff) as f64
}

/// A time table's entry: { key, label, time }.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeEntry {
    pub key: String,
    pub label: Option<String>,
    pub time: f64,
}

/// One team slot's tallies (slotRecord).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub team_kills: f64,
    pub vehicles_destroyed: f64,
    pub spree: f64,
    pub survivalist: f64,
    pub life_kills: f64,
    pub life_kills_best: f64,
    pub headshots: f64,
    pub deaths: f64,
    pub life: f64,
    pub life_best: f64,
    pub camp: f64,
    pub camp_best: f64,
    pub escapes: f64,
    /// Character -> kills of it, in the Map's insertion order.
    pub victims: Vec<(u32, f64)>,
    pub classes: Vec<TimeEntry>,
    pub weapons: Vec<TimeEntry>,
    pub vehicles: Vec<TimeEntry>,
}

/// A Character's record (recordOf), made when first needed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub slots: [Slot; 2],
    /// The camping spot.
    pub anchor: Option<[f64; 3]>,
    /// His last health, 0..255.
    pub health: f64,
    /// Under 10% since his last full health.
    pub low: bool,
}

impl Record {
    pub fn new() -> Record {
        Record { slots: [Slot::default(), Slot::default()], anchor: None, health: 255.0, low: false }
    }
}

/// recordOf(character).
pub fn record_of(character: &mut Character) -> &mut Record {
    character.record.get_or_insert_with(Record::new)
}

/// What a Character's tallies see of him this tick (observe): where what
/// he controls is, his soldier's health (0..1, while it lives), the weapon
/// in his hand or his seat's first (its key may be missing), the vehicle he
/// sits in and his class's label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub position: Option<[f64; 3]>,
    pub health: Option<f64>,
    pub weapon: Option<(Option<String>, Option<String>)>,
    pub vehicle: Option<(Option<String>, Option<String>)>,
    pub class_label: Option<String>,
}

/// addTime(table, size, key, label, dt) (battle-stats.js:96).
fn add_time(table: &mut Vec<TimeEntry>, size: usize, key: Option<&str>, label: Option<&str>, dt: f64) {
    let Some(key) = key.filter(|k| !k.is_empty()) else { return };
    let label = label.filter(|l| !l.is_empty());
    if let Some(entry) = table.iter_mut().find(|e| e.key == key) {
        entry.time += dt;
        if let Some(label) = label {
            entry.label = Some(label.to_string());
        }
        return;
    }
    if table.len() < size {
        table.push(TimeEntry { key: key.to_string(), label: label.map(str::to_string), time: dt });
        return;
    }
    let mut least = 0;
    for (i, e) in table.iter().enumerate() {
        if e.time < table[least].time {
            least = i;
        }
    }
    let e = &mut table[least];
    e.key = key.to_string();
    e.label = label.map(str::to_string);
    e.time = dt;
}

/// The ring of recent kills.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub killers: [Option<u32>; RING_SIZE],
    pub times: [f64; RING_SIZE],
    pub slots: [i32; RING_SIZE],
    pub head: usize,
    pub tail: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Ring { killers: [None; RING_SIZE], times: [0.0; RING_SIZE], slots: [0; RING_SIZE], head: 0, tail: 0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BattleStats {
    pub time: f64,
    pub ring: Ring,
}

impl BattleStats {
    /// kill(killer, victim, relation) (battle-stats.js:150).
    pub fn kill(&mut self, characters: &mut [Character], killer: Option<u32>, victim: Option<u32>, relation: f64) {
        let Some(k) = killer else { return };
        if relation == 0.0 {
            return;
        }
        let team = characters[k as usize].team;
        let health = record_of(&mut characters[k as usize]).health;
        let record = &mut record_of(&mut characters[k as usize]).slots[slot_of(team)];
        if let Some(v) = victim {
            match record.victims.iter_mut().find(|(c, _)| *c == v) {
                Some(entry) => entry.1 = u8_of(entry.1 + 1.0),
                None => record.victims.push((v, u8_of(1.0))),
            }
        }
        if relation > 0.0 {
            record.team_kills = u8_of(record.team_kills + 1.0);
            return;
        }
        if health < SURVIVAL_HEALTH {
            record.survivalist = u8_of(record.survivalist + 1.0);
        }
        record.life_kills = u8_of(record.life_kills + 1.0);
        record.life_kills_best = v8math::max(record.life_kills_best, record.life_kills);
        self.spree(characters, k);
    }

    /// spree(killer) (battle-stats.js:167): the killer's kills in the last
    /// 5 s, counted only when the slot at the ring's next free entry is his
    /// (the original game's).
    fn spree(&mut self, characters: &mut [Character], killer: u32) {
        let ring = &mut self.ring;
        let next = |i: usize| (i + 1) % RING_SIZE;
        let now = self.time;
        while ring.tail != ring.head && !(now - SPREE_WINDOW < ring.times[ring.tail]) {
            ring.tail = next(ring.tail);
        }
        let head = next(ring.head);
        if head == ring.tail {
            return; // full
        }
        let own = slot_of(characters[killer as usize].team) as i32;
        ring.killers[ring.head] = Some(killer);
        ring.times[ring.head] = now;
        ring.slots[ring.head] = own;
        ring.head = head;
        let mut count = 0.0;
        let mut i = ring.tail;
        while i != head {
            if ring.killers[i] == Some(killer) && ring.slots[head] == own {
                count += 1.0;
            }
            i = next(i);
        }
        let team = characters[killer as usize].team;
        let record = &mut record_of(&mut characters[killer as usize]).slots[slot_of(team)];
        record.spree = v8math::max(record.spree, u8_of(count));
    }

    /// death(victim) (battle-stats.js:187).
    pub fn death(&mut self, characters: &mut [Character], victim: Option<u32>) {
        let Some(v) = victim else { return };
        let team = characters[v as usize].team;
        let r = record_of(&mut characters[v as usize]);
        let record = &mut r.slots[slot_of(team)];
        record.deaths = u8_of(record.deaths + 1.0);
        record.life_kills = 0.0;
        record.life = 0.0;
        r.low = false;
    }

    /// headshot(credited) (battle-stats.js:196).
    pub fn headshot(&mut self, characters: &mut [Character], credited: Option<u32>) {
        let Some(c) = credited else { return };
        let team = characters[c as usize].team;
        let record = &mut record_of(&mut characters[c as usize]).slots[slot_of(team)];
        record.headshots = ((jsrt::conv::to_int32(record.headshots + 1.0)) & 0xffff) as f64;
    }

    /// vehicleDestroyed(destroyer) (battle-stats.js:202).
    pub fn vehicle_destroyed(&mut self, characters: &mut [Character], destroyer: Option<u32>) {
        let Some(c) = destroyer else { return };
        let team = characters[c as usize].team;
        let record = &mut record_of(&mut characters[c as usize]).slots[slot_of(team)];
        record.vehicles_destroyed = u8_of(record.vehicles_destroyed + 1.0);
    }

    /// tick(dt, characters, active, see) (battle-stats.js:210): one world
    /// tick for each Character whose tallies run (`active(place, him)`).
    /// `see(place, him)` is observe(him), asked only for one with a class or
    /// a unit.
    pub fn tick(&mut self, dt: f64, characters: &mut [Character], active: impl Fn(usize, &Character) -> bool, mut see: impl FnMut(u32, &Character) -> Option<Seen>) {
        self.time += dt;
        for i in 0..characters.len() {
            if !active(i, &characters[i]) {
                continue;
            }
            let character = &characters[i];
            let seen = if truthy_str(&character.class_name) || character.unit.is_some() { see(i as u32, character) } else { None };
            let team = characters[i].team;
            let class_name = characters[i].class_name.clone();
            let record = record_of(&mut characters[i]);
            let own = slot_of(team);
            if let Some(class_name) = class_name.as_ref().filter(|c| !c.is_empty()) {
                let s = &mut record.slots[own];
                add_time(&mut s.classes, CLASSES, Some(class_name), seen.as_ref().and_then(|s| s.class_label.as_deref()), dt);
                s.life += dt;
                s.life_best = v8math::max(s.life_best, s.life);
            }
            let Some(seen) = seen else { continue };
            if let Some(health) = seen.health {
                record.health = v8math::min(v8math::max(v8math::trunc(health * 255.0), 0.0), 255.0);
                if health < ESCAPE_LOW {
                    record.low = true;
                } else if record.low && health >= 1.0 {
                    record.low = false;
                    record.slots[own].escapes = u8_of(record.slots[own].escapes + 1.0);
                }
            }
            if let Some(p) = seen.position {
                let far = match record.anchor {
                    None => true,
                    Some(a) => v8math::pow(p[0] - a[0], 2.0) + v8math::pow(p[1] - a[1], 2.0) + v8math::pow(p[2] - a[2], 2.0) > CAMP_RADIUS * CAMP_RADIUS,
                };
                if far {
                    record.anchor = Some(p);
                    record.slots[own].camp = 0.0;
                } else {
                    let s = &mut record.slots[own];
                    s.camp += dt;
                    s.camp_best = v8math::max(s.camp_best, s.camp);
                }
            }
            if let Some((key, label)) = &seen.weapon {
                add_time(&mut record.slots[own].weapons, WEAPONS, key.as_deref(), label.as_deref(), dt);
            }
            if let Some((key, label)) = &seen.vehicle {
                add_time(&mut record.slots[own].vehicles, VEHICLES, key.as_deref(), label.as_deref(), dt);
            }
        }
    }
}
