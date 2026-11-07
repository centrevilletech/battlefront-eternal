//! squads.emitVo and Items.ownedBy publish their first-use facts in source
//! order. Body and control identities remain distinct, even when a mounted
//! proxy borrows its physical rider's weapons or both share a slab index.
#[cfg(target_arch="wasm32")]
use super::rows::row_store;
use super::{arsenals::Mounted,entities::Entity,rows::Rows,weapons::Changes};
use crate::{ai::combat::world::Obj,gen::{AiActor,AiItemOwnerNotice,AiVoiceNotice},store::{StrId,Strings}};
use std::cell::Cell;
fn actor<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,unit:Obj)->Option<AiActor> {
    match rows.state.entities.get(unit)? {
        Entity::Unit(reference)=>Some(AiActor::Unit(reference)),
        Entity::Control(reference)=>Some(AiActor::SeatUnit(reference)),
        _=>None,
    }
}
pub struct Voices {text:Vec<(&'static str,StrId)>}
impl Voices {
    /// Source command/event names form a finite authored bank. Intern them
    /// at construction; no command or danger notification builds text hot.
    pub fn new(strings:&mut Strings)->Self {
        let mut out=Self {text:Vec::with_capacity(64)};
        for text in ["heardEnemy","HeardEnemySound","tac_yes","AISCResponseYessirSound","charge","ChargeSound","flee","FleeSound"] {out.add(strings,text);}
        for (_,_,suffix) in crate::ai::control::squads::VO_BANK {out.add(strings,suffix);}
        for properties in crate::ai::control::squads::VO_PROPERTIES {for property in properties {out.add(strings,property);}}
        out
    }
    fn add(&mut self,strings:&mut Strings,text:&'static str) {
        if !self.text.iter().any(|(value,_)|*value==text) {self.text.push((text,strings.intern_str(text)));}
    }
    fn id(&self,text:&str)->StrId {self.text.iter().find(|(value,_)|*value==text).expect("prepared native AI voice text").1}
    pub fn record<C:Changes+?Sized,M:Mounted>(&self,time:f64,rows:&Rows<'_,C,M>,unit:Obj,event:&str,property:&str,command:f64,context:Option<f64>,positional:Option<bool>,notice:Option<bool>)->Option<AiVoiceNotice> {
        let actor=actor(rows,unit)?;let unit=rows.unit(unit);
        Some(AiVoiceNotice {actor,generation:unit.generation,time,team:unit.team,position:[unit.position.x,unit.position.y,unit.position.z],human:unit.human,
            event:self.id(event),property:self.id(property),command:(command>=0.).then_some(command),context,positional,notice})
    }
}
pub struct ItemOwners {seen:Vec<Cell<Option<Obj>>>}
impl ItemOwners {
    pub fn new(identities:usize)->Self {Self {seen:(0..identities).map(|_|Cell::new(None)).collect()}}
    /// ownerKey numbers an object even when its first ownedBy result is an
    /// empty list. Gameplay life changes its item key, not its owner number.
    pub fn observe<C:Changes+?Sized,M:Mounted>(&self,rows:&Rows<'_,C,M>,unit:Obj)->Option<AiItemOwnerNotice> {
        let actor=actor(rows,unit)?;let cell=self.seen.get(unit as u32 as usize).expect("prepared native AI owner identity");
        if cell.get()==Some(unit) {return None;}
        cell.set(Some(unit));Some(AiItemOwnerNotice {actor})
    }
}
#[cfg(target_arch="wasm32")]
impl super::host::Voice for &Voices {
    fn emit(&self,time:f64,rows:&mut Rows<'_,super::weapons::F3,super::arsenals::Live>,unit:Obj,event:&str,property:&str,command:f64,context:Option<f64>,positional:Option<bool>,notice:Option<bool>) {
        if let Some(record)=self.record(time,rows,unit,event,property,command,context,positional,notice) {record.emit(&mut rows.w.sim.events);}
    }
}
#[cfg(target_arch="wasm32")]
impl super::host::OwnedItems for &ItemOwners {
    fn count(&self,rows:&mut Rows<'_,super::weapons::F3,super::arsenals::Live>,unit:Obj,weapon:(crate::store::DefId,u32))->usize {
        if let Some(record)=self.observe(rows,unit) {record.emit(&mut rows.w.sim.events);}
        let Some(Entity::Unit(reference))=rows.state.entities.get(unit) else {
            // ItemOwner admits physical bodies, player and null. A SeatUnit
            // has its own ownerKey; borrowing a rider's arsenal cannot make
            // that rider's item lists belong to the proxy.
            return 0;
        };
        let life=row_store!(rows).unit.get(reference).expect("current native dispenser body").generation;
        let key=crate::units::f3::items_read(&rows.w.units).owned_key(reference.slot,life,weapon.0,weapon.1).expect("prepared native dispenser owner key");
        row_store!(rows).items.owned.get(key).map_or(0,Vec::len)
    }
}
