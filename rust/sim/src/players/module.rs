//! The host's entries for the connected players: admission and linking
//! (multiplayer-world.js addPlayer), the inputs it admits after their
//! shape checks (input, fire, actions), the stops and disconnects, and
//! what it reads back after a tick.
use super::owner::{Player, MAX_INPUT_LEAD_TICKS};
use crate::gen::*;
use crate::world::owner::World;

// Handles are pool keys: a stale or foreign handle finds no player.
fn player(w: &mut World, handle: u32) -> Option<&mut Player> { w.input.players.pool.get_mut(handle as usize) }

/// createMultiplayerWorld: this world runs the players' phase.
pub fn players_init_in(w: &mut World) {
    let units = w.sim.store.unit.slots() as usize;
    w.input.players.activate(&mut w.sim.strings, units.max(256));
}

/// addPlayer's record of a connection, after its Character and body exist.
pub fn players_create_in(w: &mut World) -> u32 {
    let (row, heap) = w.sim.calls.args();
    if row.len() < PlayerSetup::WORDS { return 0; }
    let setup = PlayerSetup::take(row, heap);
    // His mailbox must already be admitted: the phase takes from it by key.
    if w.input.mailboxes.get(setup.mailbox as usize).is_none() { return 0; }
    let capacity = w.input.mailboxes.get(setup.mailbox as usize).map_or(0.0, |m| m.state.capacity).clamp(0.0, 4096.0) as usize;
    let mut p = Player::new(setup.mailbox as usize, setup.at, capacity);
    p.character = setup.character;
    p.spectator = setup.spectator;
    p.reliable_fire = setup.reliable_fire;
    p.separate_actions = setup.separate_actions;
    p.team_select = setup.team_select;
    p.auto_respawn = setup.auto_respawn;
    p.ordinal = setup.ordinal;
    p.team = setup.team;
    p.recon_key = setup.recon_key;
    w.input.players.admit(p).map_or(0, |key| key as u32)
}

/// addPlayer's reconnect: a fresh mailbox and fresh reliable lanes; his
/// body and roster place are kept.
pub fn players_link_in(w: &mut World, handle: u32) -> u32 {
    let (row, heap) = w.sim.calls.args();
    if row.len() < PlayerLink::WORDS { return 0; }
    let link = PlayerLink::take(row, heap);
    if w.input.mailboxes.get(link.mailbox as usize).is_none() { return 0; }
    let Some(p) = player(w, handle) else { return 0 };
    p.mailbox = link.mailbox as usize;
    p.reliable_fire = link.reliable_fire;
    p.separate_actions = link.separate_actions;
    p.disconnected_at = None;
    p.inputs_stale = false;
    p.last_input_at = link.at;
    p.controls = super::controls::Controls::new();
    p.fire = Player::fire_lane();
    p.actions = Player::actions_lane();
    p.changed = true;
    1
}

/// addPlayer's failed spawn: the admitted record goes with it.
pub fn players_drop_in(w: &mut World, handle: u32) { w.input.players.remove(handle as usize); }

/// stopInput (removePlayer, clearInput).
pub fn players_stop_in(w: &mut World, handle: u32) {
    let mut players = std::mem::take(&mut w.input.players);
    super::commands::stop(w, &mut players, handle as usize);
    w.input.players = players;
}

/// removePlayer: his grace starts at the tick after this one.
pub fn players_disconnect_in(w: &mut World, handle: u32, at: f64) {
    if let Some(p) = player(w, handle) { p.disconnected_at = Some(at); p.changed = true; }
}

/// `actor.autoRespawn`: the local world's policy.
pub fn players_auto_in(w: &mut World, handle: u32, flag: u32) {
    if let Some(p) = player(w, handle) { p.auto_respawn = flag != 0; }
}

/// world.input: his input lead is a running max that never decays (one
/// lag spike widens his rewind window for the rest of the connection; a
/// shrinking lead would change simulation results), and accepted input
/// marks him awake.
pub fn players_input_in(w: &mut World, handle: u32, at: f64, lead: f64, accepted: u32) {
    let Some(p) = player(w, handle) else { return };
    if lead.is_finite() {
        let bounded = v8math::max(0.0, v8math::min(MAX_INPUT_LEAD_TICKS, lead));
        p.input_lead_ticks = Some(v8math::max(p.input_lead_ticks.unwrap_or(0.0), bounded));
    }
    if accepted != 0 { p.heard(at); }
}

/// `p.unit.generation` and `p.unit.alive`, through his connection.
fn alive_body(w: &mut World, handle: u32) -> Option<(Option<f64>, bool)> {
    let character = w.input.players.pool.get(handle as usize)?.character.clone()?;
    let unit = crate::battle::f4_connections::wire_view_in(&mut w.battle, &w.sim.store, &character)?.unit?;
    let u = w.sim.store.unit.get(unit)?;
    Some((Some(u.generation), u.alive))
}

/// world.fire: a reliable trigger edge for a living, connected player.
pub fn players_fire_in(w: &mut World, handle: u32) -> u32 {
    let (row, heap) = w.sim.calls.args();
    if row.len() < PlayerFire::WORDS { return 0; }
    let input = PlayerFire::take(row, heap);
    let Some(p) = w.input.players.pool.get(handle as usize) else { return 0 };
    if !p.reliable_fire || p.spectator || p.disconnected_at.is_some() { return 0; }
    let Some((Some(generation), true)) = alive_body(w, handle) else { return 0 };
    let same_generation = input.edge.generation == generation;
    let p = player(w, handle).unwrap();
    let accepted = crate::input::fire::push(&mut p.fire.state, Some(input.edge), input.epoch_one, input.pressed_valid, same_generation, input.tick);
    if accepted { p.heard(input.at); }
    u32::from(accepted)
}

/// world.actions: a reliable action message for a connected player with
/// separate lanes. The admission's refusal or fault is read back.
pub fn players_actions_in(w: &mut World, handle: u32) -> Option<&[f64]> {
    let (row, heap) = w.sim.calls.args();
    if row.len() < PlayerActions::WORDS { return None; }
    let input = PlayerActions::take(row, heap);
    // Action names arrive as string ids the host interned; refuse any the
    // table does not hold before the lane compares their text.
    let strings = w.sim.strings.list.len();
    if input.entry.actions.iter().any(|id| id.0 as usize >= strings) { return None; }
    let mut answer = PlayerAdmission::default();
    let generation = alive_body(w, handle).and_then(|(generation, _)| generation);
    if let Some(p) = w.input.players.pool.get(handle as usize) {
        if p.separate_actions && !p.spectator && p.disconnected_at.is_none() {
            let same_generation = generation == Some(input.entry.generation);
            let strings = &w.sim.strings;
            let p = w.input.players.pool.get_mut(handle as usize).unwrap();
            let outcome = crate::input::actions::push(&mut p.actions, Some(input.entry), input.same_epoch, same_generation,
                input.payload_valid, input.human_valid, input.human, input.tick, strings);
            match outcome {
                Ok(accepted) => { answer.accepted = accepted; if accepted { p.heard(input.at); } }
                Err(error) => answer.error = Some(w.sim.strings.intern_str(error)),
            }
        }
    }
    w.sim.calls.result(PlayerAdmission::WORDS, |row, heap| answer.put(row, heap));
    Some(&w.sim.calls.block.words[..])
}

/// The reliable lanes as the host reads them; `drain` takes the lane's
/// pending receipts (InputActions/FireCommands drainOutcomes).
pub fn players_lanes_in(w: &mut World, handle: u32, lane: u32, drain: u32) -> Option<&[f64]> {
    let players = &mut w.input.players;
    let p = players.pool.get_mut(handle as usize)?;
    let mut lanes = PlayerLanes { actions_received: p.actions.received, actions_ack: p.actions.ack, actions_discarded: p.actions.discarded,
        fire_received: p.fire.state.received, fire_buttons: p.fire.state.buttons, stale: p.inputs_stale, faults: f64::from(players.faults), outcomes: Vec::new() };
    // The receipts are lent to the record while it is encoded, then the
    // emptied list goes back so the next receipts allocate nothing.
    let source = if lane == 0 { p.actions.outcomes.as_mut() } else { p.fire.state.outcomes.as_mut() };
    if drain != 0 { if let Some(outcomes) = source { lanes.outcomes = std::mem::take(outcomes); } }
    let block = &mut players.lanes;
    block.begin(1, PlayerLanes::WORDS);
    { let (row, heap) = block.row(0); lanes.put(row, heap); }
    block.finish();
    // The drained list keeps its capacity for the next receipts.
    if drain != 0 {
        let p = players.pool.get_mut(handle as usize)?;
        let source = if lane == 0 { p.actions.outcomes.as_mut() } else { p.fire.state.outcomes.as_mut() };
        if let Some(outcomes) = source { lanes.outcomes.clear(); *outcomes = lanes.outcomes; }
    }
    Some(&players.lanes.words[..])
}

/// enableOutcomes on both lanes (the wire's edge receipts).
pub fn players_outcomes_enable_in(w: &mut World, handle: u32) {
    if let Some(p) = player(w, handle) {
        p.actions.outcomes.get_or_insert_with(|| Vec::with_capacity(256));
        p.fire.state.outcomes.get_or_insert_with(|| Vec::with_capacity(64));
    }
}

pub fn players_views_in(w: &mut World) -> Option<&[f64]> { super::step::views_in(w) }
pub fn players_shots_in(w: &mut World) -> Option<&[f64]> { super::step::shots_in(w) }

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn players_init() { players_init_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn players_create() -> u32 { players_create_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn players_link(handle: u32) -> u32 { players_link_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn players_drop(handle: u32) { players_drop_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn players_stop(handle: u32) { players_stop_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn players_disconnect(handle: u32, at: f64) { players_disconnect_in(crate::abi::facade::world(), handle, at) }
    #[no_mangle]
    pub extern "C" fn players_auto(handle: u32, flag: u32) { players_auto_in(crate::abi::facade::world(), handle, flag) }
    #[no_mangle]
    pub extern "C" fn players_input(handle: u32, at: f64, lead: f64, accepted: u32) { players_input_in(crate::abi::facade::world(), handle, at, lead, accepted) }
    #[no_mangle]
    pub extern "C" fn players_fire(handle: u32) -> u32 { players_fire_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn players_actions(handle: u32) -> *const f64 { players_actions_in(crate::abi::facade::world(), handle).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn players_lanes(handle: u32, lane: u32, drain: u32) -> *const f64 { players_lanes_in(crate::abi::facade::world(), handle, lane, drain).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn players_outcomes_enable(handle: u32) { players_outcomes_enable_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn players_views() -> *const f64 { players_views_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn players_shots() -> *const f64 { players_shots_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
