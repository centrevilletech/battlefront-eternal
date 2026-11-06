//! Battle/Character commands and retained publication share one owner. Native
//! death services use the same entry as outside score commands, without an
//! export or a round trip through scoreboard facades.
use super::{commands::{Commands, Result}, publication::{Publication, Source}, stat_records::Records};
use crate::gen::{Character, F4Command, F4CommandOp, F4Handle, Record, StatSlot, Store, TimeEntry};
use crate::store::Ref;

pub struct Core {
    commands: Commands,
    pub records: Records,
    pub publication: Publication,
    characters: Vec<Option<F4Handle>>,
    battle: F4Handle,
    victims: usize,
}
impl Core {
    /// Battle construction knows its maximum roster. The two lazy stat slots
    /// can each use the full class, weapon, vehicle and victim capacities.
    pub fn new(s: &Store, characters: usize) -> Self {
        let mut publication = Publication::new(characters + 1);
        let mut heap = Vec::new(); let mut row = vec![0.0; crate::gen::Battle::EXPORT_STRIDE];
        s.battle.put_export(&mut row, &mut heap);
        let battle = publication.register(Source::Battle, 1.0, heap.len() + characters * 2 + s.battle.team_ids.len()*4);
        Self { commands: Commands::new(characters), records: Records::new(characters), publication,
            characters: Vec::with_capacity(characters), battle, victims: characters }
    }
    pub fn adopt(&mut self, s: &mut Store, r: Ref<Character>) -> F4Handle {
        self.records.prepare(s, r, self.victims);
        self.reassign(s,r)
    }
    pub fn resolve(&self,s:&Store,h:&F4Handle)->Option<Ref<Character>>{self.commands.resolve(s,h)}
    /// Roster reuse advances the command life without rebuilding stat storage.
    pub fn reassign(&mut self,s:&Store,r:Ref<Character>)->F4Handle{
        let handle = self.commands.adopt(s, r);
        let heap = Self::character_heap(self.victims);
        let published = self.publication.register(Source::Character(r), handle.life, heap);
        assert_eq!(published, handle);
        self.characters.resize(self.characters.len().max(r.slot as usize + 1), None);
        self.characters[r.slot as usize] = Some(handle.clone()); handle
    }
    pub fn character_heap(victims: usize) -> usize {
        2 * (StatSlot::WORDS + 20 * TimeEntry::WORDS + victims * 3) + Record::WORDS
    }
    pub fn character(&self, r: Ref<Character>) -> Option<&F4Handle> {
        self.characters.get(r.slot as usize)?.as_ref().filter(|h| h.generation == r.gen)
    }
    pub fn battle(&self) -> &F4Handle { &self.battle }
    pub fn mark_battle(&mut self) { self.publication.mark(&self.battle); }
    pub fn mark(&mut self, r: Ref<Character>) {
        if let Some(h) = self.characters.get(r.slot as usize).and_then(Option::as_ref).filter(|h| h.generation == r.gen) {
            self.publication.mark(h);
        }
    }
    pub fn apply(&mut self, s: &mut Store, command: &F4Command) -> Result {
        let records = &mut self.records;
        let result = self.commands.apply_with(s, command, |s, r| records.ensure(s, r));
        if result == Result::Applied {
            for &r in &self.commands.dirty {
                let h = self.characters[r.slot as usize].as_ref().expect("prepared Character publication");
                self.publication.mark(h);
            }
            if self.commands.battle_dirty { self.publication.mark(&self.battle); }
        }
        self.commands.published(); result
    }
    /// Combat has already resolved its captured Character references. A Unit
    /// respawn retains that Character life; roster reassignment invalidates the
    /// external command handle without erasing a shot's captured attribution.
    pub fn credit(&mut self, s: &mut Store, victim: Option<Ref<Character>>, killer: Option<Ref<Character>>, relation: f64) {
        if s.battle.game_over { return; }
        if relation != 0.0 { if let Some(r) = killer { self.records.ensure(s, r); } }
        if let Some(r) = victim { self.records.ensure(s, r); }
        super::rules::credit(s, victim, killer, relation);
        if let Some(r) = victim { self.mark(r); } if let Some(r) = killer { self.mark(r); }
        self.publication.mark(&self.battle);
    }
    pub fn achievement(&mut self, s: &mut Store, r: Option<Ref<Character>>, op: F4CommandOp) {
        let Some(r) = r else { return };
        if s.battle.game_over { return; }
        self.records.ensure(s, r);
        match op {
            F4CommandOp::Headshot => super::rules::headshot(s, Some(r)),
            F4CommandOp::VehicleDestroyed => super::rules::vehicle_destroyed(s, Some(r)),
            _ => panic!("an achievement command"),
        }
        self.mark(r);
    }
    pub fn reset_record(&mut self, s: &mut Store, r: Ref<Character>) { self.records.reset(s, r); self.mark(r); }
    pub fn flush(&mut self, s: &Store) -> usize { self.publication.flush(s) }
}
