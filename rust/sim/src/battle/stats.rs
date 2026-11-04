//! What the end-of-match screens report for each Character
//! (src/battle-stats.js; docs/scoreboard.md), kept as the 2004 game keeps
//! its per-Character stats record: per team slot (team 2's slot 1, every
//! other team's slot 0), a ring of the last hundred kills shared by
//! everyone, and time tables of 5 classes, 10 weapons and 5 vehicles a slot
//! where a new entry takes the place of the one with the least time.
//!
//! The state is the store's (rust/sim/schema/battle.mjs): the battle's
//! `BattleStats` and each Character's `Record`.

use crate::gen::{Anchor, BattleStats, Character, Record, StatSlot, TimeEntry};
use crate::store::{Nullable, Ref, Slab, StrId, Strings};

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

/// The tallies only test whether a name is empty. A live module reads its
/// string table for that instant, without borrowing it across a callback
/// which may intern a new weapon or class label.
pub trait Text {
    fn nonempty(&self, id: StrId) -> bool;
}

impl Text for Strings {
    fn nonempty(&self, id: StrId) -> bool { !self.units(id).is_empty() }
}

/// A string field's truthiness (null and "" are false).
pub fn truthy(strings: &dyn Text, s: Option<StrId>) -> bool {
    s.is_some_and(|id| strings.nonempty(id))
}

/// `value & 0xff` on a count (counts stay small integers).
fn u8_of(value: f64) -> f64 {
    (jsrt::conv::to_int32(value) & 0xff) as f64
}

/// A Character's record as recordOf makes it.
pub fn new_record() -> Record {
    Record { slots: vec![StatSlot::default(), StatSlot::default()], anchor: None, health: 255.0, low: false }
}

/// recordOf(character): made when first needed (none, or null).
pub fn record_of(character: &mut Character) -> &mut Record {
    if !matches!(character.record, Nullable::Value(_)) {
        character.record = Nullable::Value(new_record());
    }
    match &mut character.record {
        Nullable::Value(r) => r,
        _ => unreachable!(),
    }
}

/// slot(character): his record's slot for his team now.
fn slot_mut(character: &mut Character) -> &mut StatSlot {
    let own = slot_of(character.team);
    &mut record_of(character).slots[own]
}

/// What a Character's tallies see of him this tick (observe): where what
/// he controls is, his soldier's health (0..1, while it lives), the weapon
/// in his hand or his seat's first (its key may be missing), the vehicle he
/// sits in and his class's label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub position: Option<[f64; 3]>,
    pub health: Option<f64>,
    pub weapon: Option<(Option<StrId>, Option<StrId>)>,
    pub vehicle: Option<(Option<StrId>, Option<StrId>)>,
    pub class_label: Option<StrId>,
}

/// addTime(table, size, key, label, dt) (battle-stats.js:96).
fn add_time(table: &mut Vec<TimeEntry>, size: usize, key: Option<StrId>, label: Option<StrId>, dt: f64, strings: &dyn Text) {
    if !truthy(strings, key) {
        return;
    }
    let key = key.unwrap();
    let label = label.filter(|l| truthy(strings, Some(*l)));
    if let Some(entry) = table.iter_mut().find(|e| e.key == key) {
        entry.time += dt;
        if let Some(label) = label {
            entry.label = Some(label);
        }
        return;
    }
    if table.len() < size {
        table.push(TimeEntry { key, label, time: dt });
        return;
    }
    let mut least = 0;
    for (i, e) in table.iter().enumerate() {
        if e.time < table[least].time {
            least = i;
        }
    }
    let e = &mut table[least];
    e.key = key;
    e.label = label;
    e.time = dt;
}

/// kill(killer, victim, relation) (battle-stats.js:150).
pub fn kill(stats: &mut BattleStats, characters: &mut Slab<Character>, killer: Option<Ref<Character>>, victim: Option<Ref<Character>>, relation: f64) {
    let Some(k) = killer else { return };
    if relation == 0.0 {
        return;
    }
    let c = characters.get_mut(k).expect("a killer");
    let health = record_of(c).health;
    let record = slot_mut(c);
    if let Some(v) = victim {
        // (get(victim) || 0) + 1
        let n = record.victims.get(&v).copied().filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(0.0);
        record.victims.set(v, u8_of(n + 1.0));
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
    spree(stats, characters, k);
}

/// spree(killer) (battle-stats.js:167): the killer's kills in the last 5 s,
/// counted only when the slot at the ring's next free entry is his (the
/// original game's).
fn spree(stats: &mut BattleStats, characters: &mut Slab<Character>, killer: Ref<Character>) {
    let now = stats.time;
    let ring = &mut stats.ring;
    let next = |i: f64| (i + 1.0) % RING_SIZE as f64;
    while ring.tail != ring.head && !(now - SPREE_WINDOW < ring.times[ring.tail as usize]) {
        ring.tail = next(ring.tail);
    }
    let head = next(ring.head);
    if head == ring.tail {
        return; // full
    }
    let c = characters.get_mut(killer).expect("a killer");
    let own = slot_of(c.team) as i32;
    let at = ring.head as usize;
    ring.killers[at] = Some(killer);
    ring.times[at] = now;
    ring.slots[at] = own;
    ring.head = head;
    let mut count = 0.0;
    let mut i = ring.tail;
    while i != head {
        if ring.killers[i as usize] == Some(killer) && ring.slots[head as usize] == own {
            count += 1.0;
        }
        i = next(i);
    }
    let record = slot_mut(c);
    record.spree = v8math::max(record.spree, u8_of(count));
}

/// death(victim) (battle-stats.js:187).
pub fn death(characters: &mut Slab<Character>, victim: Option<Ref<Character>>) {
    let Some(v) = victim else { return };
    let c = characters.get_mut(v).expect("a victim");
    let record = slot_mut(c);
    record.deaths = u8_of(record.deaths + 1.0);
    record.life_kills = 0.0;
    record.life = 0.0;
    record_of(c).low = false;
}

/// headshot(credited) (battle-stats.js:196).
pub fn headshot(characters: &mut Slab<Character>, credited: Option<Ref<Character>>) {
    let Some(r) = credited else { return };
    let record = slot_mut(characters.get_mut(r).expect("a Character"));
    record.headshots = ((jsrt::conv::to_int32(record.headshots + 1.0)) & 0xffff) as f64;
}

/// vehicleDestroyed(destroyer) (battle-stats.js:202).
pub fn vehicle_destroyed(characters: &mut Slab<Character>, destroyer: Option<Ref<Character>>) {
    let Some(r) = destroyer else { return };
    let record = slot_mut(characters.get_mut(r).expect("a Character"));
    record.vehicles_destroyed = u8_of(record.vehicles_destroyed + 1.0);
}

/// tick(dt, characters, active, see) (battle-stats.js:210): one world tick
/// for each Character of `list` whose tallies run (`active(him)`).
/// `see(him)` is observe(him), asked only for one with a class or a unit.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    stats: &mut BattleStats,
    dt: f64,
    list: &[Ref<Character>],
    characters: &mut Slab<Character>,
    strings: &dyn Text,
    active: impl Fn(Ref<Character>, &Character) -> bool,
    mut see: impl FnMut(Ref<Character>, &Character) -> Option<Seen>,
) {
    stats.time += dt;
    for &r in list {
        let character = characters.get_mut(r).expect("a Character of the battle");
        if !active(r, character) {
            continue;
        }
        record_of(character);
        let seen = if truthy(strings, character.class_name) || character.unit.is_some() { see(r, character) } else { None };
        let class_name = character.class_name;
        let own = slot_of(character.team);
        let record = record_of(character);
        if truthy(strings, class_name) {
            let s = &mut record.slots[own];
            add_time(&mut s.classes, CLASSES, class_name, seen.as_ref().and_then(|s| s.class_label), dt, strings);
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
            let far = match &record.anchor {
                None => true,
                Some(a) => v8math::pow(p[0] - a.x, 2.0) + v8math::pow(p[1] - a.y, 2.0) + v8math::pow(p[2] - a.z, 2.0) > CAMP_RADIUS * CAMP_RADIUS,
            };
            if far {
                record.anchor = Some(Anchor { x: p[0], y: p[1], z: p[2] });
                record.slots[own].camp = 0.0;
            } else {
                let s = &mut record.slots[own];
                s.camp += dt;
                s.camp_best = v8math::max(s.camp_best, s.camp);
            }
        }
        if let Some((key, label)) = seen.weapon {
            add_time(&mut record.slots[own].weapons, WEAPONS, key, label, dt, strings);
        }
        if let Some((key, label)) = seen.vehicle {
            add_time(&mut record.slots[own].vehicles, VEHICLES, key, label, dt, strings);
        }
    }
}
