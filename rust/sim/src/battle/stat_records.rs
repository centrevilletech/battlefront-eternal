//! Prepared storage for battle-stats' lazy per-Character records. The spare
//! lives outside the replicated Character, preserving null/undefined until
//! recordOf is first needed and retaining its tables through roster reuse.
use super::stats::{self, CLASSES, VEHICLES, WEAPONS};
use crate::gen::{Character, Record, StatSlot, Store};
use crate::store::{Nullable, Ref};
struct Entry { generation: u32, spare: Option<Record> }
pub struct Records { entries: Vec<Option<Entry>> }
fn reserve(record: &mut Record, victims: usize) {
    for slot in &mut record.slots {
        slot.classes.reserve(CLASSES.saturating_sub(slot.classes.len()));
        slot.weapons.reserve(WEAPONS.saturating_sub(slot.weapons.len()));
        slot.vehicles.reserve(VEHICLES.saturating_sub(slot.vehicles.len()));
        slot.victims.reserve(victims);
    }
}
fn clear(record: &mut Record) {
    for slot in &mut record.slots {
        let mut victims = std::mem::take(&mut slot.victims);
        let mut classes = std::mem::take(&mut slot.classes);
        let mut weapons = std::mem::take(&mut slot.weapons);
        let mut vehicles = std::mem::take(&mut slot.vehicles);
        victims.clear(); classes.clear(); weapons.clear(); vehicles.clear();
        *slot = StatSlot { victims, classes, weapons, vehicles, ..StatSlot::default() };
    }
    record.anchor = None; record.health = 255.0; record.low = false;
}
impl Records {
    pub fn new(characters: usize) -> Self { Self { entries: Vec::with_capacity(characters) } }
    /// Constructor/roster admission supplies the prepared maximum victim set.
    /// Growing the roster reserves existing records here, outside the phase.
    pub fn prepare(&mut self, s: &mut Store, character: Ref<Character>, victims: usize) {
        let c = s.character.get_mut(character).expect("prepared statistics Character");
        self.entries.resize_with(self.entries.len().max(character.slot as usize + 1), || None);
        let entry = &mut self.entries[character.slot as usize];
        if !entry.as_ref().is_some_and(|e| e.generation == character.gen) {
            *entry = Some(Entry { generation: character.gen, spare: None });
        }
        let entry = entry.as_mut().unwrap();
        if let Nullable::Value(record) = &mut c.record { reserve(record, victims); }
        else {
            let record = entry.spare.get_or_insert_with(stats::new_record);
            reserve(record, victims);
        }
    }
    fn entry(&mut self, character: Ref<Character>) -> &mut Entry {
        self.entries.get_mut(character.slot as usize).and_then(Option::as_mut)
            .filter(|e| e.generation == character.gen).expect("prepared statistics life")
    }
    /// Run immediately before native credit, death or active-stat observation.
    /// Moving a prepared record changes no observable lazy-record ordering.
    pub fn ensure(&mut self, s: &mut Store, character: Ref<Character>) {
        let entry = self.entry(character);
        let c = s.character.get_mut(character).expect("statistics Character");
        if !matches!(c.record, Nullable::Value(_)) {
            c.record = Nullable::Value(entry.spare.take().expect("reserved lazy statistics record"));
        }
    }
    /// Roster reset keeps a null public record and recycles its backing tables.
    pub fn reset(&mut self, s: &mut Store, character: Ref<Character>) {
        let entry = self.entry(character);
        let c = s.character.get_mut(character).expect("reset statistics Character");
        if let Nullable::Value(mut record) = std::mem::replace(&mut c.record, Nullable::Null) {
            clear(&mut record); entry.spare = Some(record);
        }
    }
}
