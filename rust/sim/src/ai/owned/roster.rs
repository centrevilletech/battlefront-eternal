//! The AI keeps roster order as F4 handles. Character facts and objectives
//! remain in the canonical battle slab, including across body respawns.
use super::rows::{ReadContext,WriteContext,context_store,context_store_mut};
use crate::{gen::{F4Handle,PostRef},battle::{commands::Commands,posts::post_ref}};
use crate::ai::control::world::CharacterSnap;
use super::{entities::{Entities,Entity},population::Population};

/// The roster resolves the owning F4 Character life. Commands is useful for
/// isolated fixtures; the live battle supplies Core and its sparse publisher.
pub trait CharacterOwner {
    fn resolve(&self,context:ReadContext<'_>,handle:&F4Handle)->Option<crate::store::Ref<crate::gen::Character>>;
    #[cfg(not(target_arch="wasm32"))]
    fn changed(&mut self,reference:crate::store::Ref<crate::gen::Character>);
    #[cfg(target_arch="wasm32")]
    fn changed_in(&mut self,w:&mut crate::world::owner::World,reference:crate::store::Ref<crate::gen::Character>);
}
impl CharacterOwner for Commands {
    fn resolve(&self,context:ReadContext<'_>,handle:&F4Handle)->Option<crate::store::Ref<crate::gen::Character>> {let store=context_store!(context);Commands::resolve(self,store,handle)}
    #[cfg(not(target_arch="wasm32"))]
    fn changed(&mut self,r:crate::store::Ref<crate::gen::Character>) {if !self.dirty.contains(&r) {self.dirty.push(r);}}
    #[cfg(target_arch="wasm32")]
    fn changed_in(&mut self,_w:&mut crate::world::owner::World,r:crate::store::Ref<crate::gen::Character>) {if !self.dirty.contains(&r) {self.dirty.push(r);}}
}
impl CharacterOwner for crate::battle::core::Core {
    fn resolve(&self,context:ReadContext<'_>,handle:&F4Handle)->Option<crate::store::Ref<crate::gen::Character>> {let store=context_store!(context);
        let r=crate::store::Ref::new(handle.slot,handle.generation);
        (store.character.contains(r)&&self.character(r)==Some(handle)).then_some(r)
    }
    #[cfg(not(target_arch="wasm32"))]
    fn changed(&mut self,r:crate::store::Ref<crate::gen::Character>) {self.mark(r);}
    #[cfg(target_arch="wasm32")]
    fn changed_in(&mut self,_w:&mut crate::world::owner::World,r:crate::store::Ref<crate::gen::Character>) {self.mark(r);}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stale {Character,Post}
pub struct Roster {handles:Vec<F4Handle>,by_slot:Vec<Option<(u32,usize)>>}
impl Roster {
    pub fn new(capacity:usize)->Self {Self {handles:Vec::with_capacity(capacity),by_slot:vec![None;capacity]}}
    pub fn add(&mut self,handle:F4Handle)->usize {
        let index=self.handles.len();self.bind(index,&handle);self.handles.push(handle);index
    }
    fn bind(&mut self,index:usize,handle:&F4Handle) {
        assert_eq!(handle.kind,crate::gen::F4ObjectKind::Character);
        let slot=handle.slot as usize;if self.by_slot.len()<=slot {self.by_slot.resize(slot+1,None);}
        self.by_slot[slot]=Some((handle.generation,index));
    }
    /// A reassignment changes the F4 command life of an existing object.
    /// Use storage identity only to replace that binding, never to validate
    /// an incoming lifecycle command against its old gameplay life.
    pub fn storage_index(&self,reference:crate::store::Ref<crate::gen::Character>)->Option<usize> {
        let (generation,index)=self.by_slot.get(reference.slot as usize).copied().flatten()?;
        (generation==reference.gen).then_some(index)
    }
    pub fn replace(&mut self,index:usize,handle:F4Handle) {
        let old=&self.handles[index];
        if self.by_slot[old.slot as usize]==Some((old.generation,index)) {self.by_slot[old.slot as usize]=None;}
        self.bind(index,&handle);self.handles[index]=handle;
    }
    pub fn reference(&self,context:ReadContext<'_>,commands:&dyn CharacterOwner,index:usize)->Option<crate::store::Ref<crate::gen::Character>> {
        commands.resolve(context,self.handles.get(index)?)
    }
    pub fn index(&self,context:ReadContext<'_>,commands:&dyn CharacterOwner,reference:crate::store::Ref<crate::gen::Character>)->Option<usize> {
        let (generation,index)=self.by_slot.get(reference.slot as usize).copied().flatten()?;
        (generation==reference.gen&&self.reference(context,commands,index)==Some(reference)).then_some(index)
    }
    pub fn handles(&self)->&[F4Handle] {&self.handles}
    /// Combat feed reads Character.objective without observing its unrelated
    /// body binding. The same qualified Character and post lives still gate it.
    pub fn objective(&self,context:ReadContext<'_>,commands:&dyn CharacterOwner,posts:&Population,index:usize)->Result<Option<usize>,Stale> {let store=context_store!(context);
        let reference=self.reference(context,commands,index).ok_or(Stale::Character)?;
        match store.character.get(reference).unwrap().objective {
            PostRef::Null|PostRef::Undefined=>Ok(None),
            post=>posts.index(post).map(Some).ok_or(Stale::Post),
        }
    }
    pub fn read(&self,context:ReadContext<'_>,commands:&dyn CharacterOwner,entities:&Entities,posts:&Population,index:usize)->Result<CharacterSnap,Stale> {let store=context_store!(context);
        let handle=self.handles.get(index).ok_or(Stale::Character)?;
        let reference=commands.resolve(context,handle).ok_or(Stale::Character)?;
        let c=store.character.get(reference).unwrap();
        let objective=match c.objective {
            PostRef::Null|PostRef::Undefined=>None,
            post=>Some(posts.index(post).ok_or(Stale::Post)?),
        };
        Ok(CharacterSnap {index,team:c.team,human:c.human,
            unit:c.unit.filter(|r|store.unit.contains(*r)).and_then(|r|entities.find(Entity::Unit(r))),objective})
    }
    pub fn assign(&self,context:WriteContext<'_>,commands:&mut dyn CharacterOwner,posts:&Population,index:usize,post:Option<usize>)->Result<(),Stale> {
        let handle=self.handles.get(index).ok_or(Stale::Character)?;
        let reference=commands.resolve(context,handle).ok_or(Stale::Character)?;
        let post=match post {Some(i)=>Some(posts.post(i).ok_or(Stale::Post)?),None=>None};
        let after=post_ref(post);let c=context_store_mut!(context).character.get_mut(reference).unwrap();
        if c.objective!=after {
            c.objective=after;
            #[cfg(target_arch="wasm32")]
            commands.changed_in(context,reference);
            #[cfg(not(target_arch="wasm32"))]
            commands.changed(reference);
        }
        Ok(())
    }
}
