//! One admission export call against its native body: the export's name
//! selects the `_in` body the module's export wraps, the call's scalar
//! arguments are passed as the page passed them (pool keys where the module
//! passes addresses), its scratch bytes fill the buffer the body reads or
//! the buffer the export hands the page to write, and what the body answers
//! is recorded for the owners after it. A body answering null where the
//! page would have thrown refuses the call. Exports that only hand the page
//! a view of module memory do nothing here; the per-tick exports (steps,
//! casts, publication) belong to the tick and publication entries.
use super::rows::{Call, Cx};
use super::{AdmitError, Stage};
use crate::world::owner::World;

/// The scalar argument at `at`, 0 when the call has fewer.
fn a(call: &Call, at: usize) -> f64 { call.args.get(at).copied().unwrap_or(0.0) }
fn u(call: &Call, at: usize) -> u32 { a(call, at) as u32 }
fn i(call: &Call, at: usize) -> i32 { a(call, at) as i32 }

/// What a body answered: nothing (unit), a scalar, a row of words, bytes,
/// or null where the page would have thrown.
#[derive(Clone, Debug, PartialEq)]
pub enum Returned {
    None,
    Scalar(f64),
    Row(Vec<f64>),
    Bytes(Vec<u8>),
    Refused,
}
use Returned::{Bytes as B, None as N, Refused as X, Row as R, Scalar as S};
fn row(words: Option<&[f64]>) -> Returned { words.map_or(X, |w| R(w.to_vec())) }
fn block(block: Option<&crate::store::block::Block>) -> Returned { block.map_or(X, |b| R(b.words.clone())) }
fn bytes(bytes: Option<&[u8]>) -> Returned { bytes.map_or(X, |b| B(b.to_vec())) }

// The page writes into the buffer an export hands it, or reads it back. A
// call with scratch bytes fills the buffer (little-endian, by the buffer's
// element type); a call without reads it: the owned bank comes back as f64
// words (a Float64Array for the host) or little-endian bytes (a Uint8Array
// the host views by the export's element type). No host sees a pointer.
fn fill_u8(into: Option<&mut [u8]>, call: &Call) -> Returned {
    match into {
        Some(b) if call.scratch.is_empty() => B(b.to_vec()),
        Some(b) if b.len() >= call.scratch.len() => { b[..call.scratch.len()].copy_from_slice(&call.scratch); N }
        _ => X,
    }
}
fn fill_u32(into: Option<&mut [u32]>, call: &Call) -> Returned {
    match into {
        Some(b) if call.scratch.is_empty() => B(b.iter().flat_map(|v| v.to_le_bytes()).collect()),
        Some(b) if b.len() * 4 >= call.scratch.len() => { for (w, x) in b.iter_mut().zip(call.scratch.chunks_exact(4)) { *w = u32::from_le_bytes(x.try_into().unwrap()); } N }
        _ => X,
    }
}
fn fill_i32(into: Option<&mut [i32]>, call: &Call) -> Returned {
    match into {
        Some(b) if call.scratch.is_empty() => B(b.iter().flat_map(|v| v.to_le_bytes()).collect()),
        Some(b) if b.len() * 4 >= call.scratch.len() => { for (w, x) in b.iter_mut().zip(call.scratch.chunks_exact(4)) { *w = i32::from_le_bytes(x.try_into().unwrap()); } N }
        _ => X,
    }
}
fn fill_f32(into: Option<&mut [f32]>, call: &Call) -> Returned {
    match into {
        Some(b) if call.scratch.is_empty() => B(b.iter().flat_map(|v| v.to_le_bytes()).collect()),
        Some(b) if b.len() * 4 >= call.scratch.len() => { for (w, x) in b.iter_mut().zip(call.scratch.chunks_exact(4)) { *w = f32::from_le_bytes(x.try_into().unwrap()); } N }
        _ => X,
    }
}
fn fill_f64(into: Option<&mut [f64]>, call: &Call) -> Returned {
    match into {
        Some(b) if call.scratch.is_empty() => R(b.to_vec()),
        Some(b) if b.len() * 8 >= call.scratch.len() => { for (w, x) in b.iter_mut().zip(call.scratch.chunks_exact(8)) { *w = f64::from_le_bytes(x.try_into().unwrap()); } N }
        _ => X,
    }
}
/// The collision kernel's exchange slots, enabled flags and places the page
/// writes before a query (grid_new's bounds, body_set's bounds and matrices,
/// a cast's ray and candidates' flags): the scratch carries the 64 io words,
/// then the enabled flags, then the places, each as far as it goes.
fn io(w: &mut World, call: &Call) {
    let io = w.collision.io_mut();
    let io_len = io.len();
    for (slot, x) in io.iter_mut().zip(call.scratch.chunks_exact(8).take(io_len)) { *slot = f64::from_le_bytes(x.try_into().unwrap()); }
    let rest = call.scratch.get(io_len * 8..).unwrap_or(&[]);
    let enabled = w.collision.enabled_mut();
    let enabled_len = enabled.len();
    let n = rest.len().min(enabled_len);
    enabled[..n].copy_from_slice(&rest[..n]);
    let rest = rest.get(enabled_len..).unwrap_or(&[]);
    let places = &mut w.collision.gathers.places;
    let n = rest.len().min(places.len());
    places[..n].copy_from_slice(&rest[..n]);
}
/// The scratch bytes a body reads by pointer and length (camera and body
/// definitions): the call's bytes, placed in the exchange's scratch buffer.
fn scratch(w: &mut World, call: &Call) -> (*const u8, usize) {
    let buffer = w.sim.scratch_room(call.scratch.len());
    buffer.copy_from_slice(&call.scratch);
    (buffer.as_ptr(), call.scratch.len())
}
/// The F3 input bank the page writes rows straight into (f3-sim.js
/// `layout.input`: F3BodyName rows before f3_body_names): the call's row.
fn input(w: &mut World, call: &Call) {
    let Some(bank) = crate::units::f3::input_bank_mut_in(w) else { return; };
    for (slot, word) in bank.iter_mut().zip(call.row.iter()) { *slot = *word; }
}
/// The F3 command bank the page writes a direct row into (f3-sim.js
/// `layout.command`: F3Change, F3Adoption, F3PoseCommand) before f3_command,
/// f3_adopt and f3_pose: the call's row and heap, in that order.
fn direct(w: &mut World, call: &Call) {
    if crate::units::f3::input_bank_mut_in(w).is_none() { return; }
    let bank = crate::units::f3::command_row_in(&mut w.units);
    for (slot, word) in bank.iter_mut().zip(call.row.iter().chain(call.heap.iter())) { *slot = *word; }
}

/// Runs one call. `args(world, call)` has already interned its strings,
/// written its scratch and placed its argument block.
pub(crate) fn call(world: &mut World, stage: Stage, name: &str, call: &Call) -> Result<Returned, AdmitError> {
    let (w, c) = (world, call);
    // `bank:NAME` reads an owner's bank by name, or writes the call's scratch into it.
    if let Some(bank) = name.strip_prefix("bank:") {
        let answer = if c.scratch.is_empty() { super::banks::read(w, bank) } else { super::banks::write(w, bank, &c.scratch) };
        return answer.ok_or_else(|| AdmitError::NoBody { stage, call: name.to_string() });
    }
    let answer = match name {
        "ai_mission_teams" => R(crate::ai::owned::entry::ai_mission_teams_in(w, u(c,0)).words.clone()),
        "ai_weapon_policies" => block(crate::ai::owned::policies::ai_weapon_policies_in(w)),
        "ai_world_activate" => S(crate::ai::owned::entry::ai_world_activate_in(w) as f64),
        "ai_world_begin" => S(crate::ai::owned::entry::ai_world_begin_in(w) as f64),
        "ai_world_boundary" => row(crate::ai::owned::entry::ai_world_boundary_in(w)),
        "ai_world_command" => S(crate::ai::owned::entry::ai_world_command_in(w) as f64),
        "ai_world_content" => S(crate::ai::owned::entry::ai_world_content_in(w) as f64),
        "ai_world_prop_owner" => row(crate::ai::owned::entry::ai_world_prop_owner_in(w, u(c,0))),
        "ai_world_roster" => S(crate::ai::owned::entry::ai_world_roster_in(w) as f64),
        "ai_world_roster_character" => row(crate::ai::owned::entry::ai_world_roster_character_in(w, u(c,0))),
        "ai_world_weapon_policies" => block(crate::ai::owned::policies::ai_world_weapon_policies_in(w)),
        "body_new" => S(w.collision.body_new(u(c,0), u(c,1)) as f64),
        "body_offset" => { w.collision.body_offset(u(c,0), a(c,1)); N },
        "body_set" => { io(w, c); w.collision.body_set(u(c,0)); N },
        "cache_new" => S(w.collision.cache_new(a(c,0)) as f64),
        "camera_add_definition" => { let (p, n) = scratch(w, c); S(crate::cameras::camera_add_definition_in(w, u(c,0), p, n) as f64) },
        "camera_commands_ptr" => fill_f64(crate::cameras::camera_commands_ptr_in(w, u(c,0)), c),
        "camera_init" => { let (p, n) = scratch(w, c); S(crate::cameras::camera_init_in(w, p, n) as f64) },
        "camera_input_ptr" => fill_f64(crate::cameras::camera_input_ptr_in(w, u(c,0)), c),
        "camera_output_ptr" => row(crate::cameras::camera_output_ptr_in(w, u(c,0))),
        "clip_data" => fill_f32(Some(crate::pose::clip_data_in(&mut w.pose, u(c,0))), c),
        "clip_new" => S(w.pose.clip_new(u(c,0), u(c,1), a(c,2), u(c,3)) as f64),
        "clip_tracks" => fill_i32(Some(crate::pose::clip_tracks_in(&mut w.pose, u(c,0))), c),
        "cmb_class" => { crate::combat::module::cmb_class_in(w, u(c,0)); N },
        "cmb_explosion" => { crate::combat::module::cmb_explosion_in(w, u(c,0)); N },
        "cmb_new" => row(crate::combat::module::cmb_new_in(w, u(c,0), u(c,1), u(c,2), a(c,3), a(c,4), u(c,5))),
        "cmb_set" => S(crate::combat::module::cmb_set_in(w, u(c,0), u(c,1), a(c,2)) as f64),
        "cmb_weapon" => { crate::combat::module::cmb_weapon_in(w, u(c,0)); N },
        "cmb_weapon_set" => S(crate::combat::module::cmb_weapon_set_in(w, u(c,0), u(c,1), u(c,2), a(c,3)) as f64),
        "f2_bodies" => { crate::vehicles::module::f2::live_seats::f2_bodies_in(w); N },
        "f2_body_words" => S(crate::vehicles::module::f2::live_seats::f2_body_words() as f64),
        "f2_command" => S(crate::vehicles::module::f2::f2_command_in(w, u(c,0), u(c,1), a(c,2), u(c,3), a(c,4), u(c,5), u(c,6), u(c,7)) as f64),
        "f2_firing_adopt" => { crate::vehicles::module::f2::firing::live::f2_firing_adopt_in(w); N },
        "f2_firing_supply" => row(crate::vehicles::module::f2::firing::live::f2_firing_supply_in(w)),
        "f2_firing_words" => S(crate::vehicles::module::f2::firing::live::f2_firing_words(u(c,0)) as f64),
        "f2_init" => row(crate::vehicles::module::f2::f2_init_in(w, u(c,0), a(c,1), a(c,2))),
        "f2_prepare_body" => { crate::vehicles::module::f2::f2_prepare_body_in(w, u(c,0)); N },
        "f2_register" => { crate::vehicles::module::f2::f2_register_in(w, u(c,0), u(c,1), u(c,2)); N },
        "f2_retire" => S(crate::vehicles::module::f2::f2_retire_in(w, u(c,0), u(c,1)) as f64),
        "f2_rider_birth" => { crate::vehicles::module::f2::riders::f2_rider_birth_in(w); N },
        "f2_rider_birth_words" => S(crate::vehicles::module::f2::riders::f2_rider_birth_words() as f64),
        "f2_seats_adopt" => { crate::vehicles::module::f2::live_seats::f2_seats_adopt_in(w, u(c,0)); N },
        "f2_seats_adopt_words" => S(crate::vehicles::module::f2::live_seats::f2_seats_adopt_words() as f64),
        "f2_seats_init" => R(crate::vehicles::module::f2::live_seats::f2_seats_init_in(w).to_vec()),
        "f2_turret_adopt" => { crate::vehicles::module::f2::live_seats::f2_turret_adopt_in(w); N },
        "f2_turret_life" => { crate::vehicles::module::f2::live_seats::f2_turret_life_in(w); N },
        "f2_turret_words" => S(crate::vehicles::module::f2::live_seats::f2_turret_words(u(c,0)) as f64),
        "f2_turrets_idle" => { crate::vehicles::module::f2::live_seats::f2_turrets_idle_in(w, a(c,0)); N },
        "f3_adopt" => { direct(w, c); S(crate::units::f3::f3_adopt_in(w) as f64) },
        "f3_blast_binding" => { crate::units::f3::f3_blast_binding_in(w); N },
        // F3.bodiesIn writes the F3BodyName rows straight into the F3 input
        // bank (f3-sim.js putF3BodyName at layout.input) before the call.
        "f3_body_names" => { input(w, c); crate::units::f3::f3_body_names_in(w, u(c,0)); N },
        "f3_body_skeleton" => S(crate::units::live_pose::f3_body_skeleton_in(w) as f64),
        "f3_building" => { crate::units::f3_props::f3_building_in(w); N },
        "f3_command" => { direct(w, c); S(crate::units::f3::f3_command_in(w) as f64) },
        "f3_dispenser_binding" => S(crate::units::f3_items::f3_dispenser_binding_in(w) as f64),
        "f3_factory_finish" => { crate::units::f3_construction::f3_factory_finish_in(&mut w.units); N },
        "f3_registry_remove" => { crate::units::f3::f3_registry_remove_in(&mut w.sim, u(c,0)); N },
        "f3_factory_acquire" => S(crate::units::f3_construction::f3_factory_acquire_in(w, u(c,0)) as f64),
        "f3_factory_player_bind" => { crate::units::f3_construction::f3_factory_player_bind_in(w); N },
        "f3_factory_player_spawn" => S(crate::units::f3_construction::f3_factory_player_spawn_in(w) as f64),
        "f3_factory_reserve" => { crate::units::f3_construction::f3_factory_reserve_in(w, u(c,0)); N },
        "f3_factory_resident" => { crate::units::f3_construction::f3_factory_resident_in(w); N },
        "f3_factory_stage" => { crate::units::f3_construction::f3_factory_stage_in(w); N },
        "f3_foreign" => { crate::units::f3::f3_foreign_in(w, u(c,0)); N },
        "f3_hit_policy" => { crate::units::f3::f3_hit_policy_in(w); N },
        "f3_hull" => { crate::units::f3_hulls::f3_hull_in(w); N },
        "f3_impact_rules" => { crate::units::f3::f3_impact_rules_in(w); N },
        "f3_init" => row(crate::units::f3::f3_init_in(w, u(c,0), a(c,1), u(c,2))),
        "f3_item_class" => { crate::units::f3_items::f3_item_class_in(w); N },
        "f3_missile_classes" => { crate::units::f3::f3_missile_classes_in(w, u(c,0)); N },
        "f3_pose" => { direct(w, c); S(crate::units::f3::f3_pose_in(w) as f64) },
        "f3_recon_class" => { crate::recon::module::f3_recon_class_in(w); N },
        "f3_recon_command" => S(crate::recon::module::f3_recon_command_in(w) as f64),
        "f3_recon_level" => { crate::recon::module::f3_recon_level_in(w); N },
        "f3_recon_owners" => { crate::recon::module::f3_recon_owners_in(w, u(c,0)); N },
        "f3_recon_strike" => { crate::recon::module::f3_recon_strike_in(w); N },
        "f3_recon_weapon" => { crate::recon::module::f3_recon_weapon_in(w); N },
        "f3_registry_add" => S(crate::units::f3::f3_registry_add_in(w, u(c,0), u(c,1)) as f64),
        "f3_registry_prepare" => { crate::units::f3::f3_registry_prepare_in(w); N },
        "f3_relations" => S(crate::units::f3::f3_relations_in(w, u(c,0)) as f64),
        "f3_sensor" => row(crate::units::f3::f3_sensor_in(w)),
        "f3_unit_items" => { crate::units::f3_items::f3_unit_items_in(w); N },
        "f3_weapon_services" => { crate::units::f3::f3_weapon_services_in(w, u(c,0), u(c,1)); N },
        "f3_world_blast" => { crate::units::f3::f3_world_blast_in(w); N },
        "f4_battle_capacity" => S(crate::battle::f4_core::f4_battle_capacity_in(w, u(c,0)) as f64),
        "f4_battle_command" => S(crate::battle::f4_core::f4_battle_command_in(w) as f64),
        "f4_battle_init" => { crate::battle::f4_core::f4_battle_init_in(w); N },
        "f4_battle_publish" => row(crate::battle::f4_core::f4_battle_publish_in(w)),
        "f4_building_handle" => row(crate::battle::f4_live::f4_building_handle_in(w, u(c,0))),
        "f4_character_handle" => R(crate::battle::f4_core::f4_character_handle_in(w, u(c,0)).to_vec()),
        "f4_connection_command" => row(crate::battle::f4_connections::f4_connection_command_in(w)),
        "f4_connections_init" => { crate::battle::f4_connections::f4_connections_init_in(w, u(c,0)); N },
        "f4_human_ready" => S(crate::battle::f4_core::f4_human_ready_in(w) as f64),
        "f4_live" => row(crate::battle::f4_live::f4_live_in(w)),
        "f4_live_command" => S(crate::battle::f4_live::f4_live_command_in(w) as f64),
        "f4_live_init" => row(crate::battle::f4_live::f4_live_init_in(w)),
        "f4_live_publish" => row(crate::battle::f4_live::f4_live_publish_in(w)),
        "f4_roster_command" => row(crate::battle::f4_roster::f4_roster_command_in(w)),
        "f4_roster_init" => { crate::battle::f4_roster::f4_roster_init_in(w); N },
        "f4_startup_class" => S(crate::battle::startup::f4_startup_class_in(w) as f64),
        "f4_tail" => { crate::battle::f4_tail::f4_tail_in(w); N },
        "f4_tail_capacity" => S(crate::battle::f4_core::f4_tail_capacity_in(w, u(c,0), u(c,1)) as f64),
        "f4_tail_commands" => fill_f64(crate::battle::f4_tail::f4_tail_commands_in(w), c),
        "f4_tail_handle" => R(crate::battle::f4_core::f4_tail_handle_in(w, u(c,0), u(c,1)).to_vec()),
        "f4_tail_init" => row(crate::battle::f4_tail::f4_tail_init_in(w)),
        "f4_tail_ready" => row(crate::battle::f4_tail::f4_tail_ready_in(w)),
        "f4_unit_spawn" => S(crate::battle::f4_tail::f4_unit_spawn_in(w) as f64),
        "grid_cells" => { let id = u(c,0) as usize; fill_u32(w.collision.state.grids.get_mut(id).map(|g| g.cells.as_mut_slice()), c) },
        "grid_index" => { let id = u(c,0) as usize; fill_u32(w.collision.state.grids.get_mut(id).map(|g| g.index.as_mut_slice()), c) },
        "grid_new" => { io(w, c); S(w.collision.grid_new(u(c,0), u(c,1), u(c,2), u(c,3), u(c,4)) as f64) },
        "grid_start" => { let id = u(c,0) as usize; fill_u32(w.collision.state.grids.get_mut(id).map(|g| g.start.as_mut_slice()), c) },
        "input_mailbox_create" => S(crate::input::module::input_mailbox_create_in(w) as f64),
        "input_mailbox_drop" => { crate::input::module::input_mailbox_drop_in(w, u(c,0)); N },
        "input_mailbox_inspect" => row(crate::input::module::input_mailbox_inspect_in(w, u(c,0))),
        "input_mailbox_owned_run" => row(crate::input::module::input_mailbox_owned_run_in(w, u(c,0))),
        "input_mailbox_reject" => { crate::input::module::input_mailbox_reject_in(w, u(c,0)); N },
        "input_mailbox_rejected" => S(crate::input::module::input_mailbox_rejected_in(w, u(c,0)) as f64),
        "input_mailbox_spawn" => { crate::input::module::input_mailbox_spawn_in(w, u(c,0)); N },
        "input_mailbox_view" => { crate::input::module::input_mailbox_view_in(w, u(c,0)); N },
        "items_args_words" => S(crate::items::module::items_args_words(u(c,0)) as f64),
        "items_def" => { crate::items::module::items_def_in(w, u(c,0)); N },
        "items_pools" => { crate::items::module::items_pools_in(w, u(c,0)); N },
        "items_spawn" => row(crate::items::module::items_spawn_in(w)),
        "mover_new" => S(w.movement.mover_new(u(c,0)) as f64),
        "mover_tick" => S(w.movement.tick(&mut w.collision.state, u(c,0)) as f64),
        "ord_args_words" => S(crate::ordnance::module::ord_args_words(u(c,0)) as f64),
        "ord_def" => { crate::ordnance::module::ord_def_in(w, u(c,0)); N },
        "ord_explosion" => { crate::ordnance::module::ord_explosion_in(w, u(c,0)); N },
        "ord_fire" => row(crate::ordnance::module::ord_fire_in(w)),
        "ord_flight_missile" => row(crate::ordnance::module::ord_flight_missile_in(w)),
        "ord_water" => { crate::ordnance::module::ord_water_in(w, u(c,0), u(c,1)); N },
        "pose_add" => { w.pose.pose_add(u(c,0), u(c,1), a(c,2), a(c,3), u(c,4)); N },
        "pose_clear" => { w.pose.pose_clear(u(c,0)); N },
        "pose_construct_batches" => S(w.pose.pose_construct_batches() as f64),
        "pose_construct_vehicle" => S(w.pose.pose_construct_vehicle() as f64),
        "pose_construction_input" => fill_u8(Some(crate::pose::pose_construction_input_in(&mut w.pose, u(c,0) as usize)), c),
        "pose_construction_length" => S(w.pose.pose_construction_length() as f64),
        "pose_construction_output" => B(crate::pose::pose_construction_output_in(&w.pose).to_vec()),
        "pose_new" => S(w.pose.pose_new(u(c,0), i(c,1)) as f64),
        "pose_out" => R(crate::pose::pose_out_in(&mut w.pose, u(c,0)).to_vec()),
        "pose_q" => fill_f32(Some(crate::pose::pose_q_in(&mut w.pose, u(c,0))), c),
        "pose_t" => fill_f32(Some(crate::pose::pose_t_in(&mut w.pose, u(c,0))), c),
        "pose_w" => fill_f32(Some(crate::pose::pose_w_in(&mut w.pose, u(c,0))), c),
        "positions_at" => { let id = u(c,0) as usize; match w.collision.state.positions.get_mut(id) { Some(crate::collision::Corners::F32(v)) => fill_f32(Some(v.as_mut_slice()), c), Some(crate::collision::Corners::F64(v)) => fill_f64(Some(v.as_mut_slice()), c), None => X } },
        "positions_new" => S(w.collision.positions_new(u(c,0), u(c,1)) as f64),
        "push_walls_ray" => { w.collision.push_walls_ray(u(c,0)); N },
        "set_bounds" => { let id = u(c,0) as usize; fill_f32(w.collision.state.sets.get_mut(id).map(|s| s.bounds.as_mut_slice()), c) },
        "set_new" => S(w.collision.set_new(u(c,0), u(c,1), a(c,2)) as f64),
        "set_polygons" => { let id = u(c,0) as usize; fill_u32(w.collision.state.sets.get_mut(id).map(|s| s.polygons.as_mut_slice()), c) },
        "sim_call_room" => N,
        "tree_build" => { w.collision.state.build_tree(u(c,0)); N },
        "tree_index" => { let id = u(c,0) as usize; fill_u32(w.collision.state.trees.get_mut(id).map(|t| t.index.as_mut_slice()), c) },
        "tree_indirect" => { let id = u(c,0) as usize; fill_u32(w.collision.state.trees.get_mut(id).map(|t| t.indirect.as_mut_slice()), c) },
        "tree_new" => S(w.collision.tree_new(u(c,0), u(c,1), u(c,2)) as f64),
        "tree_polygons" => fill_u32(Some(w.collision.tree_polygon_ids(u(c,0), u(c,1))), c),
        "tree_root" => fill_u32(Some(w.collision.tree_root_words(u(c,0), u(c,1))), c),
        "tree_root_at" => R(crate::collision::build::tree_root_at_in(&w.collision.state, u(c,0), u(c,1)).iter().map(|v| *v as f64).collect()),
        "tree_root_len" => S(crate::collision::build::tree_root_len_in(&w.collision.state, u(c,0), u(c,1)) as f64),
        "tree_roots" => S(crate::collision::build::tree_roots_in(&w.collision.state, u(c,0)) as f64),
        "units_droideka_attach" => { crate::units::module::units_droideka_attach_in(w, u(c,0), u(c,1)); N },
        "units_droideka_class" => S(crate::units::module::units_droideka_class_in(w) as f64),
        "units_muzzle_attach" => { crate::units::module::units_muzzle_attach_in(w, u(c,0), u(c,1)); N },
        "veh_bytes" => fill_u8(crate::vehicles::module::veh_bytes_in(w, u(c,0)), c),
        "veh_class" => { crate::vehicles::module::veh_class_in(w, u(c,0), u(c,1), a(c,2)); N },
        // A cold call carries the collider's words in its scratch; the host
        // wrote them into the loan veh_input handed out just before.
        "veh_collider" => { if !c.scratch.is_empty() { fill_f64(crate::vehicles::module::veh_input_in(w, (c.scratch.len() / 8) as u32), c); } crate::vehicles::module::veh_collider_in(w, u(c,0), u(c,1)); N },
        "veh_collision" => { crate::vehicles::module::veh_collision_in(w, i(c,0), i(c,1)); N },
        "veh_flight_env" => { crate::vehicles::module::veh_flight_env_in(w, u(c,0), a(c,1)); N },
        "veh_flight_tune" => S(crate::vehicles::module::veh_flight_tune_in(w, u(c,0), u(c,1), a(c,2)) as f64),
        "veh_input" => fill_f64(crate::vehicles::module::veh_input_in(w, u(c,0)), c),
        "veh_map" => { crate::vehicles::module::veh_map_in(w, u(c,0), a(c,1)); N },
        // A cold call carries the pack's bytes in its scratch (the host wrote
        // them into veh_bytes' loan).
        "veh_pack" => { if !c.scratch.is_empty() { fill_u8(crate::vehicles::module::veh_bytes_in(w, c.scratch.len() as u32), c); } S(if u(c,2) == 0 { 0 } else { crate::vehicles::module::veh_pack_in(w, u(c,0), u(c,1), u(c,2), u(c,3)) } as f64) },
        // A cold call carries the pulled pose in its scratch (the host wrote
        // it into veh_input's loan).
        "veh_pull" => { if !c.scratch.is_empty() { fill_f64(crate::vehicles::module::veh_input_in(w, (c.scratch.len() / 8) as u32), c); } S(crate::vehicles::module::veh_pull_in(w, u(c,0), a(c,1), u(c,2), u(c,3)) as f64) },
        "wire_baselines" => row(crate::wire::host::wire_baselines_in(w)),
        "wire_bind_store" => { let capacity = w.wire.as_deref().and_then(|o| o.sources.get(u(c,1) as usize)).map(|s| s.capture.seat_capacity()); S(crate::wire::module::wire_bind_store_in(w, u(c,0) as usize, capacity) as f64) },
        "wire_budget_create" => S(crate::wire::host::wire_budget_create_in(w, a(c,0), a(c,1), a(c,2)) as f64),
        "wire_budget_drop" => { crate::wire::host::wire_budget_drop_in(w, u(c,0) as usize); N },
        "wire_budget_get" => S(crate::wire::host::wire_budget_get_in(w, u(c,0) as usize, u(c,1)) as f64),
        "wire_budget_run" => S(crate::wire::host::wire_budget_run_in(w, u(c,0) as usize, u(c,1), a(c,2), a(c,3)) as f64),
        "wire_budget_set" => { crate::wire::host::wire_budget_set_in(w, u(c,0) as usize, u(c,1), a(c,2)); N },
        "wire_capture" => row(crate::wire::module::wire_capture_in(w, u(c,0) as usize)),
        "wire_combat_commit" => S(crate::wire::combat::wire_combat_commit_in(w, u(c,0) as usize) as f64),
        "wire_combat_create" => S(crate::wire::combat::wire_combat_create_in(w) as f64),
        "wire_combat_drop" => { crate::wire::combat::wire_combat_drop_in(w, u(c,0) as usize); N },
        "wire_combat_dynamic" => S(crate::wire::combat::wire_combat_dynamic_in(w, u(c,0) as usize) as f64),
        "wire_combat_encode" => row(crate::wire::combat::wire_combat_encode_in(w)),
        "wire_combat_next_id" => S(crate::wire::combat::wire_combat_next_id_in(w, u(c,0) as usize) as f64),
        "wire_combat_run" => row(crate::wire::combat::wire_combat_run_in(w, u(c,0) as usize)),
        "wire_combat_saved" => row(crate::wire::combat::wire_combat_saved_in(w, u(c,0) as usize, u(c,1))),
        "wire_combat_set_next_id" => { crate::wire::combat::wire_combat_set_next_id_in(w, u(c,0) as usize, a(c,1)); N },
        "wire_combat_words" => S(crate::wire::combat::wire_combat_words(u(c,0)) as f64),
        "wire_cosmetics_byte_count" => S(crate::wire::cosmetics::wire_cosmetics_byte_count_in(w, u(c,0) as usize) as f64),
        "wire_cosmetics_bytes" => bytes(crate::wire::cosmetics::wire_cosmetics_bytes_in(w, u(c,0) as usize)),
        "wire_cosmetics_create" => S(crate::wire::cosmetics::wire_cosmetics_create_in(w) as f64),
        "wire_cosmetics_drop" => { crate::wire::cosmetics::wire_cosmetics_drop_in(w, u(c,0) as usize); N },
        "wire_cosmetics_error" => bytes(crate::wire::cosmetics::wire_cosmetics_error_in(w, u(c,0) as usize)),
        "wire_cosmetics_error_len" => S(crate::wire::cosmetics::wire_cosmetics_error_len_in(w, u(c,0) as usize) as f64),
        "wire_cosmetics_metric" => S(crate::wire::cosmetics::wire_cosmetics_metric_in(w, u(c,0) as usize, u(c,1)) as f64),
        "wire_cosmetics_payload" => fill_u8(crate::wire::cosmetics::wire_cosmetics_payload_in(w, u(c,0) as usize, u(c,1)), c),
        "wire_cosmetics_room" => fill_f64(crate::wire::cosmetics::wire_cosmetics_room_in(w, u(c,0) as usize, u(c,1)), c),
        "wire_cosmetics_run" => S(crate::wire::cosmetics::wire_cosmetics_run_in(w, u(c,0) as usize, u(c,1)) as f64),
        "wire_cosmetics_word_count" => S(crate::wire::cosmetics::wire_cosmetics_word_count_in(w, u(c,0) as usize) as f64),
        "wire_cosmetics_words" => row(crate::wire::cosmetics::wire_cosmetics_words_in(w, u(c,0) as usize)),
        "wire_create" => S(crate::wire::module::wire_create_in(w) as f64),
        "wire_drop" => { crate::wire::module::wire_drop_in(w, u(c,0) as usize); N },
        "wire_edges_create" => S(crate::wire::edges::wire_edges_create_in(w) as f64),
        "wire_edges_drop" => { crate::wire::edges::wire_edges_drop_in(w, u(c,0) as usize); N },
        "wire_edges_run" => row(crate::wire::edges::wire_edges_run_in(w, u(c,0) as usize)),
        "wire_edges_words" => S(crate::wire::edges::wire_edges_words() as f64),
        "wire_encode" => row(crate::wire::module::wire_encode_in(w, u(c,0))),
        "wire_held" => row(crate::wire::module::wire_held_in(w, u(c,0) as usize, a(c,1), a(c,2), a(c,3), a(c,4))),
        "wire_host_create" => S(crate::wire::host::wire_host_create_in(w, u(c,0), a(c,1), a(c,2), a(c,3)) as f64),
        "wire_host_drop" => { crate::wire::host::wire_host_drop_in(w, u(c,0) as usize); N },
        "wire_host_get" => S(crate::wire::host::wire_host_get_in(w, u(c,0) as usize, u(c,1), u(c,2)) as f64),
        "wire_host_held" => row(crate::wire::host::wire_host_held_in(w, u(c,0) as usize, u(c,1), a(c,2))),
        "wire_host_op" => S(crate::wire::host::wire_host_op_in(w, u(c,0) as usize, u(c,1), u(c,2), a(c,3), a(c,4)) as f64),
        "wire_host_own" => row(crate::wire::host::wire_host_own_in(w, u(c,0) as usize, u(c,1))),
        "wire_host_set" => { crate::wire::host::wire_host_set_in(w, u(c,0) as usize, u(c,1), u(c,2), a(c,3)); N },
        "wire_host_stale" => row(crate::wire::host::wire_host_stale_in(w, u(c,0) as usize, a(c,1), a(c,2))),
        "wire_host_words" => S(crate::wire::host::wire_host_words(u(c,0)) as f64),
        "wire_presentation_create" => S(crate::wire::presentation::wire_presentation_create_in(w) as f64),
        "wire_presentation_drop" => { crate::wire::presentation::wire_presentation_drop_in(w, u(c,0) as usize); N },
        "wire_presentation_error_field" => S(crate::wire::presentation::wire_presentation_error_field_in(w, u(c,0) as usize) as f64),
        "wire_presentation_len" => S(crate::wire::presentation::wire_presentation_len_in(w, u(c,0) as usize) as f64),
        "wire_presentation_ptr" => bytes(crate::wire::presentation::wire_presentation_ptr_in(w, u(c,0) as usize)),
        "wire_presentation_room" => fill_f64(crate::wire::presentation::wire_presentation_room_in(w, u(c,0) as usize, u(c,1)), c),
        "wire_presentation_run" => S(crate::wire::presentation::wire_presentation_run_in(w, u(c,0) as usize, u(c,1)) as f64),
        // The export's publication reads its facts from the source's input
        // bank (wire_store_world_input) and answers the result block; a host
        // without the module's memory takes the bytes through world_publish.
        "wire_publish" => row(crate::wire::publish_module::wire_publish_in(w, u(c,0) as usize)),
        "wire_registry_run" => row(crate::wire::registry::wire_registry_run_in(w, u(c,0))),
        "wire_registry_words" => S(crate::wire::registry::wire_registry_words(u(c,0)) as f64),
        "wire_semantic_create" => S(crate::wire::semantic::wire_semantic_create_in(w) as f64),
        "wire_semantic_drop" => { crate::wire::semantic::wire_semantic_drop_in(w, u(c,0) as usize); N },
        "wire_semantic_len" => S(crate::wire::semantic::wire_semantic_len_in(w, u(c,0) as usize) as f64),
        "wire_semantic_ptr" => bytes(crate::wire::semantic::wire_semantic_ptr_in(w, u(c,0) as usize)),
        "wire_semantic_room" => fill_f64(crate::wire::semantic::wire_semantic_room_in(w, u(c,0) as usize, u(c,1)), c),
        "wire_semantic_run" => S(crate::wire::semantic::wire_semantic_run_in(w, u(c,0) as usize, u(c,1)) as f64),
        "wire_shots_attach" => S(crate::wire::shots::wire_shots_attach_in(w, u(c,0) as usize) as f64),
        "wire_shots_counter" => S(crate::wire::shots::wire_shots_counter_in(w, u(c,0) as usize, a(c,1), a(c,2)) as f64),
        "wire_shots_create" => S(crate::wire::shots::wire_shots_create_in(w, a(c,0)) as f64),
        "wire_shots_drop" => { crate::wire::shots::wire_shots_drop_in(w, u(c,0) as usize); N },
        "wire_shots_observe" => { crate::wire::shots::wire_shots_observe_in(w, u(c,0) as usize, a(c,1), a(c,2), a(c,3)); N },
        "wire_shots_overflow" => S(crate::wire::shots::wire_shots_overflow_in(w, u(c,0) as usize) as f64),
        "wire_shots_reset" => { crate::wire::shots::wire_shots_reset_in(w, u(c,0) as usize); N },
        "wire_stagger_cadence" => { crate::wire::stagger::wire_stagger_cadence_in(w, u(c,0) as usize); N },
        "wire_stagger_create" => S(crate::wire::stagger::wire_stagger_create_in(w) as f64),
        "wire_stagger_drop" => { crate::wire::stagger::wire_stagger_drop_in(w, u(c,0) as usize); N },
        "wire_stagger_due" => S(crate::wire::stagger::wire_stagger_due_in(w, u(c,0) as usize, a(c,1), a(c,2), a(c,3)) as f64),
        "wire_stagger_held" => S(crate::wire::stagger::wire_stagger_held_in(w, u(c,0) as usize, a(c,1), a(c,2), a(c,3), i(c,4)) as f64),
        "wire_stagger_schemas" => S(crate::wire::stagger::wire_stagger_schemas_in(w, u(c,0) as usize, a(c,1), u(c,2)) as f64),
        "wire_stagger_take" => row(crate::wire::stagger::wire_stagger_take_in(w, u(c,0) as usize)),
        "wire_stagger_words" => S(crate::wire::stagger::wire_stagger_words(u(c,0)) as f64),
        "wire_store_source_configure" => S(crate::wire::store_source::wire_store_source_configure_in(w, u(c,0) as usize) as f64),
        "wire_store_source_create" => S(crate::wire::store_source::wire_store_source_create_in(w) as f64),
        "wire_store_source_drop" => { crate::wire::store_source::wire_store_source_drop_in(w, u(c,0) as usize); N },
        "wire_store_source_probe" => row(crate::wire::store_source::wire_store_source_probe_in(w, u(c,0) as usize, u(c,1) as usize)),
        "wire_store_source_words" => S(crate::wire::store_source::wire_store_source_words() as f64),
        "wire_store_world_input" => fill_f64(crate::wire::store_source::wire_store_world_input_in(w, u(c,0) as usize), c),
        "wire_words" => S(crate::wire::module::wire_words(u(c,0)) as f64),
        "world_clock_run" => row(crate::world::module::world_clock_run_in(w)),
        "world_clock_time" => S(crate::world::module::world_clock_time(a(c,0), a(c,1)) as f64),
        "world_clock_words" => S(crate::world::module::world_clock_words(u(c,0)) as f64),
        // The events port delivers through the host's `worldEvents` import,
        // which a native host does not serve: it reads the events bank
        // (read_bank EVENTS) instead.
        "world_events_run" => return Err(AdmitError::NoBody { stage, call: "world_events_run (read the events bank)".to_string() }),
        "world_events_words" => S(crate::world::module::world_events_words(u(c,0)) as f64),
        "world_local_run" => row(crate::world::module::world_local_run_in(w)),
        "world_local_words" => S(crate::world::module::world_local_words(u(c,0)) as f64),
        "world_new" => S(w.collision.world_new(u(c,0), i(c,1), i(c,2)) as f64),
        "world_order" => fill_u32(Some(w.collision.world_order_slots(u(c,0), u(c,1))), c),
        "world_statics" => { w.collision.world_statics(u(c,0), u(c,1)); N },
        "ai_world_dangers" => S(crate::ai::owned::entry::ai_world_dangers_in(w) as f64),
        "ai_world_phase" => S(crate::ai::owned::entry::ai_world_phase_in(w) as f64),
        "body_aim_batch" => { crate::units::page_body::module::body_aim_batch_in(w, u(c,0), u(c,1)); N },
        "body_aim_capacity" => S(crate::units::page_body::module::body_aim_capacity_in(w) as f64),
        "body_aim_ptr" => fill_f64(crate::units::page_body::module::body_aim_ptr_in(w), c),
        "body_construct" => { let (p, n) = scratch(w, c); match w.sim.scratch_bytes(p, n).and_then(|b| bfe_content::json::parse_bytes(b).ok()) { Some(def) => row(crate::units::page_body::module::body_construct_in(w, def)), None => X } },
        "body_preview" => { crate::units::page_body::module::body_preview_in(w, u(c,0)); N },
        "body_preview_capacity" => S(crate::units::page_body::module::body_preview_capacity_in(w) as f64),
        "body_preview_frame_ptr" => fill_f64(crate::units::page_body::module::body_preview_frame_ptr_in(w), c),
        "body_preview_looks_ptr" => fill_f64(crate::units::page_body::module::body_preview_looks_ptr_in(w), c),
        "body_preview_resolve" => { crate::units::page_body::module::body_preview_resolve_in(w); N },
        "body_spawn" => row(crate::units::page_body::module::body_spawn_in(w, u(c,0))),
        "body_spawn_droideka" => row(crate::units::page_body::module::body_spawn_droideka_in(w, u(c,0))),
        "camera_director_best" => S(crate::cameras::camera_director_best_in(w, u(c,0), a(c,1), a(c,2), a(c,3)) as f64),
        "camera_director_show" => S(crate::cameras::camera_director_show_in(w, u(c,0), a(c,1)) as f64),
        "camera_flags_ptr" => fill_u8(crate::cameras::camera_flags_ptr_in(w), c),
        "camera_reset" => { crate::cameras::camera_reset_in(w, u(c,0)); N },
        "cmd_echo" => block(crate::gen::cmd_echo_in(w, a(c,0), a(c,1), a(c,2), a(c,3), a(c,4), a(c,5), a(c,6), a(c,7))),
        "cmd_fill" => block(crate::gen::cmd_fill_in(w, a(c,0), a(c,1))),
        "cmd_relay" => block(crate::gen::cmd_relay_in(w, a(c,0), a(c,1), a(c,2), a(c,3), a(c,4), a(c,5), a(c,6), a(c,7))),
        "f2_allocations" => S(crate::vehicles::module::f2::allocation::f2_allocations() as f64),
        "f2_core_allocations" => S(crate::vehicles::module::f2::allocation::f2_core_allocations() as f64),
        "f2_lifecycle_allocations" => S(crate::vehicles::module::f2::allocation::f2_lifecycle_allocations() as f64),
        "f2_ordnance_allocations" => S(crate::vehicles::module::f2::allocation::f2_ordnance_allocations() as f64),
        "f2_stage_allocations" => S(crate::vehicles::module::f2::allocation::f2_stage_allocations(u(c,0) as usize) as f64),
        "f2_step" => { crate::vehicles::module::f2::f2_step_in(w, a(c,0), a(c,1)); N },
        "f3_acquire" => S(crate::units::f3::f3_acquire_in(w, u(c,0)) as f64),
        "f3_factory_realize" => S(crate::units::f3_construction::f3_factory_realize_in(w) as f64),
        "f3_step" => S(crate::units::f3::f3_step_in(w, u(c,0), a(c,1), a(c,2), a(c,3)) as f64),
        "f4_animated_birth" => row(crate::battle::f4_live::f4_animated_birth_in(w)),
        "f4_grip_class" => { crate::battle::f4_live::f4_grip_class_in(w); N },
        "f4_hull_class" => { crate::battle::f4_live::f4_hull_class_in(w); N },
        "f4_tail_capture_team" => { crate::battle::f4_tail::f4_tail_capture_team_in(w); N },
        "f4_tail_hero" => { crate::battle::f4_tail::f4_tail_hero_in(w); N },
        "f4_tail_hull_class" => { crate::battle::f4_tail::f4_tail_hull_class_in(w); N },
        "f4_tail_pad" => { crate::battle::f4_tail::f4_tail_pad_in(w); N },
        "f4_tail_post" => { crate::battle::f4_tail::f4_tail_post_in(w); N },
        "f4_tail_unit_class" => { crate::battle::f4_tail::f4_tail_unit_class_in(w); N },
        "f4_tail_weapon_class" => { crate::battle::f4_tail::f4_tail_weapon_class_in(w); N },
        "f4_unit_class" => { crate::battle::f4_live::f4_unit_class_in(w); N },
        "f4_viewpoint_clock" => { crate::battle::f4_live::f4_viewpoint_clock_in(w); N },
        "f4_weapon_class" => { crate::battle::f4_live::f4_weapon_class_in(w); N },
        "gather_begin" => { io(w, c); S(w.collision.gather_begin(u(c,0), i(c,1), i(c,2), i(c,3)) as f64) },
        "gather_end" => { io(w, c); S(w.collision.gather_end(u(c,0), u(c,1)) as f64) },
        "grid_ray" => { io(w, c); S(w.collision.grid_ray(u(c,0)) as f64) },
        "input_actions_run" => row(crate::input::module::input_actions_run_in(w)),
        "input_actions_words" => S(crate::input::module::input_actions_words(u(c,0)) as f64),
        "input_controls_axis" => S(crate::input::module::input_controls_axis_in(w) as f64),
        "input_controls_run" => row(crate::input::module::input_controls_run_in(w)),
        "input_controls_words" => S(crate::input::module::input_controls_words(u(c,0)) as f64),
        "input_fire_run" => row(crate::input::module::input_fire_run_in(w)),
        "input_fire_words" => S(crate::input::module::input_fire_words(u(c,0)) as f64),
        "input_queue_run" => row(crate::input::module::input_queue_run_in(w)),
        "input_queue_words" => S(crate::input::module::input_queue_words(u(c,0)) as f64),
        "items_blast" => { crate::items::module::items_blast_in(w); N },
        "items_dispense" => row(crate::items::module::items_dispense_in(w)),
        "items_drop_for" => row(crate::items::module::items_drop_for_in(w)),
        "items_hurt" => { crate::items::module::items_hurt_in(w); N },
        "items_spawn" => row(crate::items::module::items_spawn_in(w)),
        "items_update" => S(crate::items::module::items_update_in(w, a(c,0), u(c,1)) as f64),
        "network_seats_words" => S(crate::seats::network_module::network_seats_words(u(c,0)) as f64),
        "ord_update" => S(crate::ordnance::module::ord_update_in(w, a(c,0)) as f64),
        "overlap_begin" => { io(w, c); S(w.collision.overlap_begin(u(c,0), i(c,1)) as f64) },
        "overlap_end" => { io(w, c); S(w.collision.overlap_end(u(c,0), u(c,1)) as f64) },
        "sim_abi" => S(crate::abi::sim_abi() as f64),
        "sim_calls" => R(crate::abi::sim_calls_in(w).words.clone()),
        "sim_command" => block(crate::abi::sim_command_in(w, u(c,0))),
        "sim_events" => R(crate::abi::sim_events_in(w).to_vec()),
        "sim_events_clear" => { crate::abi::sim_events_clear_in(w); N },
        "sim_export" => block(crate::abi::sim_export_in(w, u(c,0), u(c,1))),
        "sim_import" => S(crate::abi::sim_import_in(w, u(c,0), u(c,1)) as f64),
        "sim_import_args" => N,
        "sim_import_capacity" => S(crate::abi::sim_import_capacity() as f64),
        "sim_import_result_room" => N,
        "sim_import_room" => { let block = crate::abi::sim_import_room_in(w, u(c,0), u(c,1), u(c,2), u(c,3)); let header = crate::store::block::HEADER; if c.scratch.is_empty() { R(block.words.clone()) } else if (block.words.len() - header) * 8 >= c.scratch.len() { for (word, x) in block.words[header..].iter_mut().zip(c.scratch.chunks_exact(8)) { *word = f64::from_le_bytes(x.try_into().unwrap()); } N } else { X } },
        "sim_kind_hash" => S(crate::abi::sim_kind_hash(u(c,0)) as i32 as f64),
        "sim_kinds" => S(crate::abi::sim_kinds() as f64),
        "sim_memory_pages" => N,
        "sim_reserve" => S(crate::abi::sim_reserve(u(c,0)) as f64),
        "sim_reset" => { crate::abi::sim_reset_in(w); N },
        "sim_retire" => S(crate::abi::sim_retire_in(w, u(c,0), u(c,1), u(c,2)) as f64),
        "sim_scratch" => fill_u8(Some(w.sim.scratch_room(u(c,0) as usize)), c),
        "sim_str_count" => S(crate::abi::sim_str_count_in(w) as f64),
        "sim_str_intern" => S(w.sim.intern_scratch(u(c,0) as usize).0 as f64),
        "sim_str_len" => S(crate::abi::sim_str_len_in(w, u(c,0)) as f64),
        "sim_str_ptr" => R(crate::abi::sim_str_ptr_in(w, u(c,0)).iter().map(|v| *v as f64).collect()),
        "sound_commands" => row(crate::audio_control::sound_commands_in(w, u(c,0))),
        "sound_debug" => row(crate::audio_control::sound_debug_in(w, u(c,0))),
        "sound_free" => { crate::audio_control::sound_free_in(w, u(c,0)); N },
        "sound_import" => { crate::audio_control::sound_import_in(w, u(c,0), u(c,1), u(c,2)); N },
        "sound_info" => fill_f64(crate::audio_control::sound_info_in(w, u(c,0)), c),
        "sound_input" => fill_f64(crate::audio_control::sound_input_in(w, u(c,0)), c),
        "sound_new" => S(crate::audio_control::sound_new_in(w, u(c,0)) as f64),
        "sound_placements" => row(crate::audio_control::sound_placements_in(w, u(c,0))),
        "sound_ready" => { crate::audio_control::sound_ready_in(w, u(c,0)); N },
        "sound_requests" => row(crate::audio_control::sound_requests_in(w, u(c,0))),
        "sound_step" => { crate::audio_control::sound_step_in(w, u(c,0), u(c,1)); N },
        "terrain_near_ray" => { io(w, c); S(w.collision.terrain_near_ray(u(c,0)) as f64) },
        "tree_ray" => { io(w, c); S(w.collision.tree_ray(u(c,0)) as f64) },
        "veh_bodies" => { crate::vehicles::module::veh_bodies_in(w, u(c,0)); N },
        "veh_changes" => row(crate::vehicles::module::veh_changes_in(w)),
        "veh_flags" => fill_u8(crate::vehicles::module::veh_flags_in(w), c),
        "veh_flight_acc" => S(crate::vehicles::module::veh_flight_acc_in(w) as f64),
        "veh_flight_inputs" => { crate::vehicles::module::veh_flight_inputs_in(w, u(c,0)); N },
        "veh_ground" => S(crate::vehicles::module::veh_ground_in(w, a(c,0), a(c,1), a(c,2), a(c,3), a(c,4), u(c,5)) as f64),
        "veh_locks" => row(crate::vehicles::module::veh_locks_in(w)),
        "veh_members" => { crate::vehicles::module::veh_members_in(w, u(c,0)); N },
        "veh_relations" => { crate::vehicles::module::veh_relations_in(w, u(c,0)); N },
        "wire_input_encode" => row(crate::wire::input::wire_input_encode_in(w, u(c,0))),
        "wire_input_words" => S(crate::wire::input::wire_input_words(u(c,0)) as f64),
        "wire_publish_bind" => S(crate::wire::publish_module::wire_publish_bind_in(w, u(c,0) as usize, u(c,1), u(c,2), u(c,3) as usize) as f64),
        "wire_publish_create" => S(crate::wire::publish_module::wire_publish_create_in(w, u(c,0) as usize, u(c,1) as usize, u(c,2) as usize, u(c,3) as usize, u(c,4) as usize, u(c,5) as usize) as f64),
        "wire_publish_drop" => { crate::wire::publish_module::wire_publish_drop_in(w, u(c,0) as usize); N },
        "wire_publish_load" => row(crate::wire::publish_module::wire_publish_load_in(w, u(c,0) as usize)),
        "world_candidates" => { io(w, c); S(w.collision.world_candidates(u(c,0), i(c,1), u(c,2), u(c,3)) as f64) },
        "world_cast" => { io(w, c); S(w.collision.world_cast(u(c,0), u(c,1)) as f64) },
        "world_cast_moving" => { io(w, c); S(w.collision.world_cast_moving(u(c,0), u(c,1)) as f64) },
        "world_cast_rays" => S(w.collision.world_cast_rays(u(c,0), u(c,1)) as f64),
        "world_step_init" => { crate::world::step::world_step_init_in(w); N },
        "world_step_run" => row(crate::world::step::world_step_run_in(w)),
        // Pointer accessors hand the page a view of module memory; a host
        // without that memory gets the owned bank instead (words for f64
        // banks, little-endian bytes otherwise), or fills it with its scratch.
        "io_ptr" => fill_f64(Some(w.collision.io_mut()), c),
        "enabled_ptr" => fill_u8(Some(w.collision.enabled_mut()), c),
        "candidates_ptr" => fill_u32(Some(&mut w.collision.candidates), c),
        "places_ptr" => fill_u8(Some(&mut w.collision.gathers.places), c),
        "input_reserve" => fill_f64(Some(w.collision.reserve_input(u(c,0))), c),
        "movement_io_ptr" => fill_f64(Some(w.movement.io_mut()), c),
        "movement_flags_ptr" => fill_u8(Some(w.movement.flags_mut()), c),
        "camera_frame" => { crate::cameras::camera_frame_in::<bfe_presentation::cameras::math::V8>(w, u(c,0)); N },
        "camera_director_follow" => S(crate::cameras::camera_director_follow_in::<bfe_presentation::cameras::math::V8>(w, u(c,0), a(c,1), a(c,2), a(c,3)) as f64),
        "nothing" => S(0.0),
        "players_actions" => row(crate::players::module::players_actions_in(w, u(c,0))),
        "players_auto" => { crate::players::module::players_auto_in(w, u(c,0), u(c,1)); N },
        "players_create" => S(crate::players::module::players_create_in(w) as f64),
        "players_disconnect" => { crate::players::module::players_disconnect_in(w, u(c,0), a(c,1)); N },
        "players_drop" => { crate::players::module::players_drop_in(w, u(c,0)); N },
        "players_fire" => S(crate::players::module::players_fire_in(w, u(c,0)) as f64),
        "players_init" => { crate::players::module::players_init_in(w); N },
        "players_input" => { crate::players::module::players_input_in(w, u(c,0), a(c,1), a(c,2), u(c,3)); N },
        "players_lanes" => row(crate::players::module::players_lanes_in(w, u(c,0), u(c,1), u(c,2))),
        "players_link" => S(crate::players::module::players_link_in(w, u(c,0)) as f64),
        "players_outcomes_enable" => { crate::players::module::players_outcomes_enable_in(w, u(c,0)); N },
        "players_shots" => row(crate::players::module::players_shots_in(w)),
        "players_stop" => { crate::players::module::players_stop_in(w, u(c,0)); N },
        "players_views" => row(crate::players::module::players_views_in(w)),
        "veh_flight_keys" => S(crate::vehicles::module::veh_flight_keys_in(w) as f64),
        _ => return Err(AdmitError::NoBody { stage, call: name.to_string() }),
    };
    Ok(answer)
}

/// Records what a body answered for the owners after it.
pub(crate) fn record(cx: &mut Cx, stage: Stage, name: &'static str, answer: Returned) -> Result<(), AdmitError> {
    match answer {
        Returned::None => cx.returned.push((name, 1.0)),
        Returned::Scalar(value) => cx.returned.push((name, value)),
        Returned::Row(words) => { cx.returned.push((name, 1.0)); cx.returned_rows.push((name, words)); }
        Returned::Bytes(bytes) => { cx.returned.push((name, bytes.len() as f64)); }
        Returned::Refused => return Err(AdmitError::Refused { stage, call: name.to_string() }),
    }
    Ok(())
}
