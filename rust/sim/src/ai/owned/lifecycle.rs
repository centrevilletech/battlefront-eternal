//! F4's synchronous AI hooks (src/ai/index.js). Resolve the canonical
//! Character or post before calling Core; no mirror or deferred callback can
//! move an objective draw past the spawn/death operation that requested it.
use super::runtime::{Runtime,World};
use crate::ai::control::world::UKey;
use crate::gen::{F4Handle,F4AIHook,F4AIHookOp,PostChange,PsMobileEventKind,SpawnPost};

pub trait Bindings:World {
    fn character_index(&self,character:&F4Handle)->Option<usize>;
    fn post_index(&self,post:SpawnPost)->Option<usize>;
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stale {Character,Post,Payload}
impl Runtime {
    pub fn request_objective(&mut self,world:&mut impl World,character:usize)->Option<usize> {
        self.core.objectives_request(world,&mut self.random,character,false,None)
    }
    pub fn republish(&mut self,world:&mut impl World,character:usize) {
        self.core.objectives_republish(world,character);
    }
    pub fn boarded(&mut self,world:&mut impl World,character:usize,previous:Option<UKey>,vehicle:Option<UKey>) {
        self.core.objectives_on_boarded(world,&mut self.random,character,previous,vehicle);
    }
    pub fn detach(&mut self,world:&mut impl World,character:usize) {
        self.core.detach(world,&mut self.random,character);
    }
    pub fn present_human_team(&mut self,world:&mut impl World,team:f64) {
        Callback {core:&mut self.core,random:&mut self.random}.present_human_team(world,team);
    }
    pub fn battle_hook(&mut self,world:&mut impl Bindings,hook:&F4AIHook)->Result<(),Stale> {
        Callback {core:&mut self.core,random:&mut self.random}.battle_hook(world,hook)
    }
}

/// Recursive owners borrow the already active Core and shared generators.
/// A death, spawn or aimer never needs to acquire the enclosing Runtime or
/// replace it with an empty owner while a suspended agent stack still exists.
pub struct Callback<'a> {
    pub core:&'a mut crate::ai::control::Core,
    pub random:&'a mut crate::ai::phase::RandomState,
}
impl Callback<'_> {
    /// presentHumanTeam/refreshCurves updates living controllers in place.
    /// Their perception, filtered aim and private Enhanced generator survive.
    pub fn present_human_team(&mut self,world:&mut impl World,team:f64) {
        self.core.difficulty.player_team=Some(team);
        for at in 0..self.core.live.len() {
            let index=self.core.live[at];
            if let Some(unit)=self.core.controllers[index].unit {
                self.core.controllers[index].difficulty=self.core.curves_for(world.unit(unit).team);
            }
        }
        world.difficulty_changed(&self.core.difficulty);
    }
    /// Presentation still receives its ordered post/mobile notification.
    /// This call performs only the synchronous authoritative AI action.
    pub fn battle_hook(&mut self,world:&mut impl Bindings,hook:&F4AIHook)->Result<(),Stale> {
        match hook.op {
            F4AIHookOp::RequestObjective|F4AIHookOp::Republish|F4AIHookOp::Spawned|F4AIHookOp::Died|F4AIHookOp::Attach|F4AIHookOp::Detach=>{
                let character=world.character_index(hook.character_handle.as_ref().ok_or(Stale::Payload)?).ok_or(Stale::Character)?;
                match hook.op {
                    F4AIHookOp::RequestObjective=>{self.core.objectives_request(world,self.random,character,false,None);},
                    F4AIHookOp::Republish=>self.core.objectives_republish(world,character),
                    F4AIHookOp::Spawned=>{self.core.spawned(world,self.random,character);},
                    F4AIHookOp::Died=>self.core.died(world,self.random,character),
                    F4AIHookOp::Attach=>{self.core.attach(world,self.random,character);},
                    F4AIHookOp::Detach=>self.core.detach(world,self.random,character),
                    _=>unreachable!(),
                }
            },
            F4AIHookOp::Post=>{
                let event=hook.post.as_ref().ok_or(Stale::Payload)?;
                let post=world.post_index(SpawnPost::Post(event.post)).ok_or(Stale::Post)?;
                match event.kind {
                    PostChange::Captured=>self.core.objectives_on_owner_changed(world,self.random,post,0.,event.by.unwrap_or(f64::NAN)),
                    PostChange::Neutralized=>self.core.objectives_on_owner_changed(world,self.random,post,event.from.unwrap_or(f64::NAN),0.),
                    PostChange::Lost=>self.core.objectives_on_deactivated(world,self.random,post),
                }
            },
            F4AIHookOp::Mobile=>{
                let event=hook.mobile.as_ref().ok_or(Stale::Payload)?;
                let post=world.post_index(SpawnPost::MobilePost(event.post)).ok_or(Stale::Post)?;
                match event.kind {
                    PsMobileEventKind::Lost=>self.core.objectives_on_deactivated(world,self.random,post),
                    PsMobileEventKind::Owner=>self.core.objectives_on_owner_changed(world,self.random,post,event.from,event.to.unwrap_or(f64::NAN)),
                }
            },
            F4AIHookOp::Retarget=>self.present_human_team(world,hook.number),
            // Battle results and rejected class choices notify presentation.
            F4AIHookOp::Over|F4AIHookOp::Rejected=>{},
        }
        Ok(())
    }
}
