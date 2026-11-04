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
