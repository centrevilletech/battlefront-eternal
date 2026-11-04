//! Live combat-agent fields and retained vector views.
use super::module::state;
#[derive(Default)]
pub(super) struct CombatAgentState { fields: [(u32,f64);10] }
#[no_mangle]
pub extern "C" fn ai_combat_agent_new() -> u32 {let s=state();s.combat_agents.push(CombatAgentState::default());(s.combat_agents.len()-1) as u32}
#[no_mangle]
pub extern "C" fn ai_combat_agent_field(id: u32,field: u32) -> f64 {state().combat_agents[id as usize].fields[field as usize].1}
#[no_mangle]
pub extern "C" fn ai_combat_agent_kind(id: u32,field: u32) -> u32 {state().combat_agents[id as usize].fields[field as usize].0}
#[no_mangle]
pub extern "C" fn ai_combat_agent_write(id: u32,field: u32,kind: u32,value: f64) {state().combat_agents[id as usize].fields[field as usize]=(kind,value);}
#[no_mangle]
pub extern "C" fn ai_combat_agent_vector_new() -> u32 {let s=state();s.combat_agent_vectors.push([(0,0.0);3]);(s.combat_agent_vectors.len()-1) as u32}
#[no_mangle]
pub extern "C" fn ai_combat_agent_vector(id: u32,index: u32) -> f64 {state().combat_agent_vectors[id as usize][index as usize].1}
#[no_mangle]
pub extern "C" fn ai_combat_agent_vector_kind(id: u32,index: u32) -> u32 {state().combat_agent_vectors[id as usize][index as usize].0}
#[no_mangle]
pub extern "C" fn ai_combat_agent_vector_write(id: u32,index: u32,kind: u32,value: f64) {state().combat_agent_vectors[id as usize][index as usize]=(kind,value);}
