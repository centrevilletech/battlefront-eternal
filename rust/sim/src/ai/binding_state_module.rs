//! Mutable binding fields and grenade notification clocks share the AI instance.
use super::module::state;
#[derive(Default)]
pub(super) struct BindingState { fields: [(u32,f64);2] }
#[no_mangle]
pub extern "C" fn ai_binding_new() -> u32 {
    let s=state();s.bindings.push(BindingState::default());(s.bindings.len()-1) as u32
}
#[no_mangle]
pub extern "C" fn ai_binding_field(id: u32,field: u32) -> f64 {state().bindings[id as usize].fields[field as usize].1}
#[no_mangle]
pub extern "C" fn ai_binding_kind(id: u32,field: u32) -> u32 {state().bindings[id as usize].fields[field as usize].0}
#[no_mangle]
pub extern "C" fn ai_binding_write(id: u32,field: u32,kind: u32,value: f64) {state().bindings[id as usize].fields[field as usize]=(kind,value);}
