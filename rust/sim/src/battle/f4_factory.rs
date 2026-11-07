//! Character and HumanSpawn realization call the canonical unit factory.
//! Cold admission prepares storage and streams before a tick; spawning never
//! returns through JavaScript construction or copies body rows across owners.
use crate::{gen::*,store::Ref,store::StrId,units::f3::{self,WorldServices}};

// UnitFactory.acquire preserves the warm LIFO stack before activating a
// prepared cold body. Activation publishes its structural birth first.
pub(crate) fn acquire(w:&mut crate::world::owner::World,class:StrId, services:WorldServices<'_>)->Option<Ref<Unit>> {
    f3::acquire_native_in(w,class).or_else(||f3::construct_native_in(w,class,services))
}
// Game.spawnUnit and HumanSpawn retain the local viewpoint's identity. A
// connection resolves its own admitted body, even when no previous body was
// supplied by the Character's delayed spawn queue.
pub(crate) fn player(w:&mut crate::world::owner::World,request:&F4PlayerRequest,mut services:WorldServices<'_>)->Option<Ref<Unit>> {
    let character=request.character?;
    if request.local && request.body.is_none() {return None;}
    if let Some(body)=request.body {
        crate::vehicles::module::f2::live_seats::leave_voluntary_in(w,body,services.reborrow());
    }
    let unit=f3::realize_player_native_in(w,character,request.body,
        request.class_name,request.position,request.yaw,request.local,services)?;
    // Exhausted prepared class stock is a normal failed spawn, preserving
    // the caller's waiting state rather than trapping the whole frame.
    if request.local && Some(unit)!=request.body {return None;}
    Some(unit)
}
