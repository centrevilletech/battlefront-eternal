//! Weapon policy dispatches to the current native owner. A SeatUnit borrows
//! its holder's arsenal; it never acquires a second copy of weapon state.
use super::rows::{MountEditContext,ReadContext,WriteContext,context_store};
use super::{entities::{Entities,Entity},seats::{Controls,Source},weapons::{self,Definitions}};
use crate::gen::{F3UnitKey,Store,Weapon};
use crate::ai::combat::world::{Obj,WeaponSettings,WeaponView,index_of};
use crate::combat::defs::{Js,WeaponDefs};

#[derive(Clone,Debug)]
pub enum Owner {Unit(F3UnitKey),Mount(Source,usize)}
impl Owner {
    /// useClassicRandom runs at controller binding, before its next shot.
    /// The numeric stream was attached to F3 once during world construction.
    #[cfg(target_arch="wasm32")]
    pub fn bind_classic(&self,w:&mut crate::world::owner::World,binding:u32,stream:crate::ai::random::SharedRandom)->bool {
        use crate::vehicles::{module::f2::{firing,live_seats},f2_seats::HullLife};
        match self {
            Self::Unit(key)=>{
                if key.validate(&w.sim.store).is_err() {return false;}
                crate::units::f3::use_ai_stream_in(w,key,binding);true
            },
            Self::Mount(Source::Hull(vehicle,life),seat)=>firing::bind_ai_stream_in(w,HullLife{vehicle:*vehicle,life:*life},*seat,stream),
            Self::Mount(Source::Turret(r,life),_)=>live_seats::bind_ai_stream_in(w,*r,*life,binding),
        }
    }
    pub fn resolve(store:&Store,entities:&Entities,controls:&Controls,obj:Obj)->Option<Self> {
        match entities.get(obj)? {
            Entity::Unit(r)=>Some(Self::Unit(F3UnitKey::of(r,store.unit.get(r)?))),
            Entity::Control(r)=>{
                let birth=controls.read(r)?;let lease=store.seat_lease.get(birth.lease)?;
                if lease.released||lease.controllable!=Some(r)||lease.index!=birth.index as f64 {return None;}
                match birth.source {
                    Source::Hull(h,life) if lease.vehicle==Some(h)&&store.vehicle.get(h)?.target_generation==life=>{},
                    Source::Turret(t,_) if lease.turret==Some(t)=>{},
                    _=>return None,
                }
                Some(Self::Mount(birth.source,birth.index))
            },
            _=>None,
        }
    }
    pub fn count(&self,context:ReadContext<'_>,mounts:&impl Mounted)->usize {let store=context_store!(context);
        match self {Self::Unit(k)=>k.validate(store).map_or(0,|u|u.combat.weapons.len()),
            Self::Mount(source,seat)=>mounts.count(context,*source,*seat).unwrap_or(0)}
    }
    pub fn current(&self,context:ReadContext<'_>,mounts:&impl Mounted,defs:&Definitions,channel:f64)->Option<usize> {let store=context_store!(context);
        match self {Self::Unit(k)=>defs.current(store,k,channel),Self::Mount(..)=>{
            let c=index_of(channel)?;
            // seatArsenal has exactly two channel lists, even on mounts with
            // more weapons. Definition Channel does not choose these lists.
            (c<2&&c<self.count(context,mounts)).then_some(c)
        }}
    }
    pub fn view(&self,context:ReadContext<'_>,mounts:&impl Mounted,defs:&Definitions,index:usize)->Option<WeaponView> {let store=context_store!(context);
        match self {Self::Unit(k)=>defs.view(store,k,index),Self::Mount(source,seat)=>
            mounts.read(context,*source,*seat,index,|w|{
                let mut view=defs.view_weapon(w);
                if matches!(view.channel,Js::Undefined|Js::Null) {view.channel=Js::Num(index as f64);}
                view
            })}
    }
    pub fn inspect<R>(&self,context:ReadContext<'_>,mounts:&impl Mounted,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> {let store=context_store!(context);
        match self {Self::Unit(k)=>Some(read(k.validate(store).ok()?.combat.weapons.get(index)?)),
            Self::Mount(source,seat)=>mounts.read(context,*source,*seat,index,read)}
    }
    pub fn settings(&self,context:ReadContext<'_>,mounts:&impl Mounted,index:usize)->Option<WeaponSettings> {let store=context_store!(context);
        match self {Self::Unit(k)=>weapons::settings(store,k,index).ok(),
            Self::Mount(source,seat)=>mounts.read(context,*source,*seat,index,weapons::read)}
    }
    pub fn edit<R,C:weapons::Changes+?Sized>(&self,context:WriteContext<'_>,ownership:&mut C,mounts:&mut impl Mounted,index:usize,
        edit:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {
        match self {Self::Unit(k)=>weapons::edit(context,ownership,k,index,edit).ok(),
            Self::Mount(source,seat)=>mounts.edit(context,*source,*seat,index,edit)}
    }
    pub fn select<C:weapons::Changes+?Sized>(&self,context:WriteContext<'_>,ownership:&mut C,mounts:&impl Mounted,defs:&WeaponDefs,index:usize)->bool {
        match self {Self::Unit(k)=>weapons::select(context,ownership,k,index,defs).unwrap_or(false),
            Self::Mount(..)=>index<self.count(context,mounts)}
    }
}

/// Mounted callbacks use the caller's canonical Store, which the AI phase
/// has loaned out of Sim. Reading Sim's empty shell would erase every hull
/// arsenal from maxExpandedRange and dispatch. They observe or edit one weapon only. They cannot reenter the
/// world while the holder lends its canonical row.
pub trait Mounted {
    fn count(&self,context:ReadContext<'_>,source:Source,seat:usize)->Option<usize>;
    fn read<R>(&self,context:ReadContext<'_>,source:Source,seat:usize,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R>;
    fn edit<R>(&mut self,context:MountEditContext<'_>,source:Source,seat:usize,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->Option<R>;
}

#[cfg(target_arch="wasm32")]
pub struct Live;
#[cfg(target_arch="wasm32")]
impl Mounted for Live {
    fn count(&self,context:ReadContext<'_>,source:Source,seat:usize)->Option<usize> {
        use crate::vehicles::{module::f2::{firing,live_seats},f2_seats::HullLife};
        match source {Source::Hull(vehicle,life)=>firing::weapon_count_ref(context,HullLife{vehicle,life},seat),
            Source::Turret(r,life)=>live_seats::weapon_count_ref(&context.fleet,r,life)}
    }
    fn read<R>(&self,context:ReadContext<'_>,source:Source,seat:usize,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> {
        use crate::vehicles::{module::f2::{firing,live_seats},f2_seats::HullLife};
        match source {Source::Hull(vehicle,life)=>firing::with_weapon_ref(context,HullLife{vehicle,life},seat,index,read),
            Source::Turret(r,life)=>live_seats::with_weapon_ref(&context.fleet,r,life,index,read)}
    }
    fn edit<R>(&mut self,context:MountEditContext<'_>,source:Source,seat:usize,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {
        use crate::vehicles::{module::f2::{firing,live_seats},f2_seats::HullLife};
        match source {Source::Hull(vehicle,life)=>firing::edit_weapon_in(context,HullLife{vehicle,life},seat,index,edit),
            Source::Turret(r,life)=>live_seats::edit_weapon_in(context,r,life,index,edit)}
    }
}
