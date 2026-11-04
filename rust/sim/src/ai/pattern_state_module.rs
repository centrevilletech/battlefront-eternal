//! Attack-pattern scalar state. Host identities are retained by its facade.
use super::module::state;

#[derive(Default)]
pub(super) struct PatternState { fields: [(u32,f64);25] }

#[no_mangle]
pub extern "C" fn ai_pattern_new() -> u32 {
    let s=state();s.patterns.push(PatternState::default());(s.patterns.len()-1) as u32
}
#[no_mangle]
pub extern "C" fn ai_pattern_field(id: u32,field: u32) -> f64 {state().patterns[id as usize].fields[field as usize].1}
#[no_mangle]
pub extern "C" fn ai_pattern_kind(id: u32,field: u32) -> u32 {state().patterns[id as usize].fields[field as usize].0}
#[no_mangle]
pub extern "C" fn ai_pattern_write(id: u32,field: u32,kind: u32,value: f64) {state().patterns[id as usize].fields[field as usize]=(kind,value);}
