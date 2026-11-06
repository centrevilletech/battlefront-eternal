//! Generation-qualified external Battle/Character writes. A roster reuse is
//! a new Character life even when the scoreboard keeps the same facade slot.
use super::rules;
use crate::gen::{Character, F4Command, F4CommandOp, F4Handle, F4ObjectKind, Store};
use crate::store::Ref;

#[derive(Clone, Copy, Default)]
struct Life { generation: u32, life: f64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Result { Applied, Stale, Unsupported }
pub struct Commands {
    lives: Vec<Life>,
    pub dirty: Vec<Ref<Character>>,
    pub battle_dirty: bool,
}
impl Commands {
    pub fn new(capacity: usize) -> Self {
        Self { lives: Vec::with_capacity(capacity), dirty: Vec::with_capacity(capacity), battle_dirty: false }
    }
    /// Called at Character creation and roster reassignment, never inferred
    /// from a Unit respawn: rounds fired in that Character's last body can
    /// still earn him credit while his next body is alive.
    pub fn adopt(&mut self, s: &Store, r: Ref<Character>) -> F4Handle {
        assert!(s.character.contains(r), "a live Character");
        self.lives.resize(self.lives.len().max(r.slot as usize + 1), Life::default());
        let life = &mut self.lives[r.slot as usize];
        life.life = if life.generation == r.gen { life.life + 1.0 } else { 1.0 };
        life.generation = r.gen;
        F4Handle { kind: F4ObjectKind::Character, slot: r.slot, generation: r.gen, life: life.life }
    }
    pub fn resolve(&self, s: &Store, h: &F4Handle) -> Option<Ref<Character>> {
        if h.kind != F4ObjectKind::Character { return None; }
        let life = self.lives.get(h.slot as usize)?;
        let r = Ref::new(h.slot, h.generation);
        (life.generation == h.generation && life.life == h.life && s.character.contains(r)).then_some(r)
    }
    fn mark(&mut self, r: Option<Ref<Character>>) {
        if let Some(r) = r { if !self.dirty.contains(&r) { self.dirty.push(r); } }
    }
    /// The caller publishes this touched set before returning to JS. A stale
    /// secondary Character is rejected before any score or ticket changes.
    pub fn apply(&mut self, s: &mut Store, command: &F4Command) -> Result {
        self.apply_with(s, command, |_, _| {})
    }
    /// Resolve both lives before preparing lazy records. Rejected commands
    /// cannot create a record, and capture points do not create tallies.
    pub fn apply_with(&mut self, s: &mut Store, command: &F4Command,
        mut prepare: impl FnMut(&mut Store, Ref<Character>)) -> Result {
        use F4CommandOp::*;
        match command.op {
            Objective => {
                let Some(r)=self.resolve(s,&command.target) else{return Result::Stale};
                let post=match &command.other {
                    None=>crate::gen::PostRef::Null,
                    Some(h)=>{
                        if h.life!=1.0{return Result::Stale;}
                        match h.kind {
                            F4ObjectKind::Post=>{let p=Ref::new(h.slot,h.generation);if !s.post.contains(p){return Result::Stale;}crate::gen::PostRef::Post(p)},
                            F4ObjectKind::MobilePost=>{let p=Ref::new(h.slot,h.generation);if !s.mobile_post.contains(p){return Result::Stale;}crate::gen::PostRef::MobilePost(p)},
                            _=>return Result::Stale,
                        }
                    }
                };
                s.character.get_mut(r).unwrap().objective=post;self.mark(Some(r));Result::Applied
            }
            Cancel => {
                let Some(r)=self.resolve(s,&command.target) else{return Result::Stale};
                let c=s.character.get_mut(r).unwrap();if !c.human {return Result::Unsupported;}
                if c.unit.is_none() {if let Some(spawn)=&mut c.spawn {spawn.ready=false;}}
                self.mark(Some(r));Result::Applied
            }
            CharacterController => {
                let Some(r)=self.resolve(s,&command.target) else{return Result::Stale};
                s.character.get_mut(r).unwrap().controller=command.controller;self.mark(Some(r));Result::Applied
            }
            Controller => {
                let Some(r)=self.resolve(s,&command.target) else{return Result::Stale};
                let Some(key)=command.unit_key.as_ref() else{return Result::Stale};
                let Ok(unit)=key.validate_mut(s) else{return Result::Stale};
                if unit.character!=Some(r){return Result::Stale;}
                unit.controller=command.controller;Result::Applied
            }
            Spend => {
                let h = &command.target;
                if h.kind != F4ObjectKind::Battle || h.slot != 0 || h.generation != 1 || h.life != 1.0 { return Result::Stale; }
                rules::spend(&mut s.battle, command.team, command.value);
                self.battle_dirty = true; Result::Applied
            }
            Credit | Headshot | VehicleDestroyed | Point | PostDestroyed => {
                let victim = if command.target.kind == F4ObjectKind::Battle && command.op == Credit {
                    let h = &command.target;
                    if h.slot != 0 || h.generation != 1 || h.life != 1.0 { return Result::Stale; }
                    None
                } else {
                    let Some(r) = self.resolve(s, &command.target) else { return Result::Stale };
                    Some(r)
                };
                let other = if let Some(h) = &command.other {
                    let Some(r) = self.resolve(s, h) else { return Result::Stale };
                    Some(r)
                } else { None };
                if !s.battle.game_over {
                    match command.op {
                        Credit => {
                            if command.value != 0.0 { if let Some(r) = other { prepare(s, r); } }
                            if let Some(r) = victim { prepare(s, r); }
                        }
                        Headshot | VehicleDestroyed => { if let Some(r) = victim { prepare(s, r); } }
                        _ => {},
                    }
                }
                match command.op {
                    Credit => rules::credit(s, victim, other, command.value),
                    Headshot => rules::headshot(s, victim),
                    VehicleDestroyed => rules::vehicle_destroyed(s, victim),
                    Point => rules::point(s, victim),
                    PostDestroyed => rules::post_destroyed(s, victim, command.value),
                    _ => unreachable!(),
                }
                self.mark(victim); self.mark(other); self.battle_dirty = true;
                Result::Applied
            }
            _ => Result::Unsupported,
        }
    }
    /// A consuming publication clears marks, while nested commands can begin
    /// another touched set without forcing a whole roster copy.
    pub fn published(&mut self) { self.dirty.clear(); self.battle_dirty = false; }
}
