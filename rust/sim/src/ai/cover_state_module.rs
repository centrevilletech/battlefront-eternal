//! Cover-helper fields. Nonnumeric values retain their host identities;
//! ordinary posture and timing values are read and written here directly.

use super::module::state;

#[derive(Default)]
pub(super) struct CoverState {
    fields: [(u32,f64);5],
}

#[no_mangle]
pub extern "C" fn ai_cover_new() -> u32 {
    let s=state();s.covers.push(CoverState::default());(s.covers.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_cover_field(id: u32,field: u32) -> f64 {state().covers[id as usize].fields[field as usize].1}

#[no_mangle]
pub extern "C" fn ai_cover_kind(id: u32,field: u32) -> u32 {state().covers[id as usize].fields[field as usize].0}

#[no_mangle]
pub extern "C" fn ai_cover_write(id: u32,field: u32,kind: u32,value: f64) {state().covers[id as usize].fields[field as usize]=(kind,value);}

#[no_mangle]
pub extern "C" fn ai_cover_reserve(hint: u32,owner: u32,at: f64) {
    state().combat.reservations.by_hint.insert(hint as u64,super::combat::agent::Reservation {owner:if owner==0 {None} else {Some(owner as u64)},at});
}

#[no_mangle]
pub extern "C" fn ai_cover_reservation_owner(hint: u32) -> u32 {
    state().combat.reservations.by_hint[&(hint as u64)].owner.unwrap_or(0) as u32
}

#[no_mangle]
pub extern "C" fn ai_cover_reservation_at(hint: u32) -> f64 {state().combat.reservations.by_hint[&(hint as u64)].at}

#[no_mangle]
pub extern "C" fn ai_cover_reservation_write(hint: u32,at: f64) {state().combat.reservations.by_hint.get_mut(&(hint as u64)).unwrap().at=at;}

#[no_mangle]
pub extern "C" fn ai_cover_available(hint: u32,owner: u32,at: f64) -> u32 {
    let owner=if owner==0 {None} else {Some(owner as u64)};
    let map=&state().combat.reservations.by_hint;
    map.get(&(hint as u64)).is_none_or(|r|r.owner==owner || !(v8math::fround(at-r.at)<3.0)) as u32
}
