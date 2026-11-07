//! What the connections' bodies hand the unit and hull phases each tick:
//! their intent rows and network shot facts (src/f3-sim.js preparePhase's
//! rows for network humans) and the network pilots' flight keys
//! (src/f2-sim.js prepareStep's flight rows). The host still writes rows
//! for bodies it drives itself; these are written beside them.
use super::owner::{Flight, Players, Shot, UnitShadow};
use crate::flight::model::FlightInput;
use crate::gen::*;
use crate::world::owner::World;

fn edge(shot: &Shot, button: f64) -> CbEdge {
    CbEdge { button, id: Some(shot.id), late: Some(shot.late), aim_yaw: shot.aim_yaw, aim_pitch: shot.aim_pitch }
}

/// preparePhase's row loop for the bodies the connections own: every body
/// that is a network human's or still carries a player's network facts,
/// except a bot whose intent is native and has nothing network to say.
pub fn write(w: &mut World, players: &mut Players) {
    players.scratch.clear();
    players.scratch.extend_from_slice(crate::units::f3::built_in(&w.units));
    players.rows.clear();
    players.tails.clear();
    let mut networks = 0u32;
    for i in 0..players.scratch.len() {
        let r = players.scratch[i];
        let slot = r.slot as usize;
        players.reserve_shadows(slot + 1);
        let context = crate::recon::module::context_in(w, r);
        let remote_first_person = context.and_then(|c| w.sim.store.recon_context.get(c)).is_some_and(|c| c.first_person);
        let native = crate::units::f3::native_intent_owned_in(&w.units, r);
        let Some(u) = w.sim.store.unit.get(r) else { continue };
        if u.removed { continue; }
        let shadow = &mut players.shadows[slot];
        // The connection left this body (its life ended, or a bot took the
        // slot): what the controls kept on it goes with it, so the bot gets
        // no connection row and the row list stays within its 65.
        if u.network_human != Some(true) { if shadow.network.is_some() || shadow.intent.is_some() || !matches!(shadow.flight, Flight::Absent) { *shadow = UnitShadow::default(); } continue; }
        let has_network = shadow.network.is_some();
        let fresh = std::mem::replace(&mut shadow.fresh, false);
        if native && !has_network && !remote_first_person { continue; }
        // The host's unit object kept what the controls wrote until the
        // next publication; the row carries that, else the published intent.
        let intent = if native { None } else { Some(if fresh { shadow.intent.clone().unwrap_or_else(|| u.intent.clone()) } else { u.intent.clone() }) };
        let network_index = if has_network { networks += 1; networks - 1 } else { 0 };
        players.rows.push(F3PhaseInput { key: F3UnitKey::of(r, u), bot_target: None, bot_target_set: false, intent, remote_first_person, has_network, network_index });
    }
    // networkShotFor consumes this sparse tail after all intent rows, in
    // the bodies' order, first press a button.
    for i in 0..players.scratch.len() {
        let r = players.scratch[i];
        let Some(u) = w.sim.store.unit.get(r) else { continue };
        if u.removed || u.network_human != Some(true) { continue; }
        let Some(facts) = players.shadows.get(r.slot as usize).and_then(|s| s.network) else { continue };
        let [first, second] = facts.shots;
        players.tails.push(F3PhaseNetwork {
            network: F3NetworkView { view: facts.view, lead: facts.lead, playout: facts.playout,
                fire0: first.map_or(0.0, |s| s.id), fire1: second.map_or(0.0, |s| s.id),
                view0: first.and_then(|s| s.view_tick), view1: second.and_then(|s| s.view_tick) },
            edge_list: true, edge0: first.as_ref().map(|s| edge(s, 0.0)), edge1: second.as_ref().map(|s| edge(s, 1.0)), edge: None,
        });
    }
    let dropped = crate::units::f3::connection_rows_in(&mut w.units, &players.rows, &players.tails);
    if dropped > 0 { players.faults = players.faults.wrapping_add(1); }
}

/// vehicle-phase-data.js flightKeys of `{}`: every key undefined.
fn no_keys() -> FlightInput {
    let nan = f64::NAN;
    FlightInput { forward: nan, right: nan, turn: nan, pitch: nan, up: nan, down: nan, takeoff: nan, stick_x: None, stick_y: None,
        boost: nan, trick: nan, immelmann: nan, roll_lock: nan, target_lock: nan, bot: nan, response_curve: nan, no_assist: false }
}

/// prepareStep's flight rows: only network pilot keys remain foreign. A
/// flying hull whose pilot seat holds a network human with flight keys
/// gets them; any other flying hull gets none.
pub fn flights(w: &mut World, players: &Players) {
    for i in 0..w.sim.store.vehicles.vehicles.len() {
        let hull = w.sim.store.vehicles.vehicles[i];
        let Some(v) = w.sim.store.vehicle.get(hull) else { continue };
        if !v.flying { continue; }
        let pilot = v.seats.first().and_then(|s| s.lease).and_then(|l| w.sim.store.seat_lease.get(l)).and_then(|l| l.unit);
        let keys = pilot.and_then(|u| {
            if w.sim.store.unit.get(u)?.network_human != Some(true) { return None; }
            match players.shadows.get(u.slot as usize).map(|s| s.flight) {
                Some(Flight::Keys(keys)) => Some(keys),
                Some(Flight::Empty) => Some(no_keys()),
                _ => None,
            }
        });
        crate::vehicles::module::network_flight_in(w, hull, keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::owner::NetworkFacts;

    // A network human's body gets a connection row with its facts; when the
    // connection leaves and a bot's life reuses the slot, the facts go and
    // the bot gets no row.
    #[test]
    fn a_bot_reusing_a_connections_slot_gets_no_connection_row() {
        let mut w = World::new();
        crate::units::f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        let r = w.sim.store.unit.insert(Unit { alive: true, generation: 1.0, network_human: Some(true), ..Default::default() });
        crate::units::f3::adopt_for_test_in(&mut w, r);
        let mut players = Players::new();
        players.shadow(r.slot).network = Some(NetworkFacts { shots: [None, None], view: Some(3.0), lead: Some(1.0), playout: Some(10.0) });
        write(&mut w, &mut players);
        assert_eq!(crate::units::f3::connection_rows_written_in(&w.units), (1, 1));
        assert_eq!(players.faults, 0);
        // His life ends and a bot takes the slot.
        { let u = w.sim.store.unit.get_mut(r).unwrap(); u.generation = 2.0; u.network_human = Some(false); }
        write(&mut w, &mut players);
        assert_eq!(crate::units::f3::connection_rows_written_in(&w.units), (0, 0));
        assert!(players.shadows[r.slot as usize].network.is_none());
        // Another connection's life in the slot starts from blank facts.
        { let u = w.sim.store.unit.get_mut(r).unwrap(); u.generation = 3.0; u.network_human = Some(true); }
        write(&mut w, &mut players);
        assert_eq!(crate::units::f3::connection_rows_written_in(&w.units), (1, 0));
    }
}
