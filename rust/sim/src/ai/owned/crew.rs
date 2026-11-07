//! objectiveCrew (ai/index.js): crew is observed from current leases, in seat
//! order. Raw hull identity is distinct from the registry's target wrapper.
use super::rows::{ReadContext,context_store};
use super::{entities::{Entities,Entity},roster::{CharacterOwner,Roster},scratch::{List,Pool},turrets::Turrets};
use crate::{ai::control::world::CrewSnap,gen::{SeatLease,Store},store::Ref,vehicles::f2_seats::Seats};

fn lease(store:&Store,reference:Ref<SeatLease>)->Option<&crate::gen::SeatLease> {
    let row=store.seat_lease.get(reference)?;
    crate::seats::lease::live(row,reference,&store.unit).then_some(row)
}
fn rider(context:ReadContext<'_>,roster:&Roster,owner:&dyn CharacterOwner,reference:Ref<SeatLease>)->Option<usize> {let store=context_store!(context);
    let character=store.unit.get(lease(store,reference)?.unit?)?.character?;
    roster.index(context,owner,character)
}
fn seated<'a>(context:ReadContext<'a>,roster:&Roster,owner:&dyn CharacterOwner,index:usize)->Option<&'a crate::gen::SeatLease> {let store=context_store!(context);
    let character=roster.reference(context,owner,index)?;
    let unit=store.character.get(character)?.unit?;
    lease(store,store.unit.get(unit)?.seat?)
}
pub fn read(context:ReadContext<'_>,entities:&Entities,roster:&Roster,owner:&dyn CharacterOwner,index:usize)->Option<CrewSnap> {let store=context_store!(context);
    let row=seated(context,roster,owner,index)?;
    let (vehicle,is_driver,driver)=if let Some(hull)=row.vehicle {
        let hull_row=store.vehicle.get(hull)?;
        (entities.find(Entity::Hull(hull))?,row.index==0.,hull_row.seats.first().and_then(|s|s.lease).and_then(|l|rider(context,roster,owner,l)))
    }else{(entities.find(Entity::Turret(row.turret?))?,false,None)};
    Some(CrewSnap {vehicle,is_driver,driver,occupants:Vec::new()})
}
pub fn occupants(context:ReadContext<'_>,seats:&Seats,turrets:&Turrets,roster:&Roster,owner:&dyn CharacterOwner,pool:&Pool<usize>,index:usize)->List<usize> {let store=context_store!(context);
    let mut out=pool.take();
    let Some(row)=seated(context,roster,owner,index) else{return out};
    if let Some(hull)=row.vehicle {
        if let Some(hull)=store.vehicle.get(hull) {for seat in &hull.seats {
            if let Some(index)=seat.lease.and_then(|l|rider(context,roster,owner,l)) {out.push_reserved(index);}
        }}
    }else if let Some(index)=row.turret.and_then(|t|turrets.crew_lease(seats,t)).and_then(|l|rider(context,roster,owner,l)) {
        out.push_reserved(index);
    }
    out
}
