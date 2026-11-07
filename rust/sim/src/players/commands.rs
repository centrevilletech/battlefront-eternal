//! A network player's controls applied to his body each tick
//! (src/runtime/multiplayer-world.js applyNetworkControls and stopInput):
//! squad orders, the movement and press keys into his intent, his trigger
//! edges, his mouse into his soldier or his flown droid, his seat's keys
//! and switches, and what his use key reaches. Every seat, body and
//! roster change goes through the same native commands the host called.
use super::owner::{Flight, NetworkFacts, Player, Players, Shot, ONLINE_PLAYOUT_MAX_TICKS};
use crate::flight::{controls::{read_input, ActionValue, InputReader}, model::FlightInput};
use crate::gen::*;
use crate::seats::boarding::{seat_control, LookScales, Mount, SeatControl};
use crate::store::{Ref, StrId};
use crate::vehicles::module::f2::live_seats as seats;
use crate::world::owner::World;

/// The command his mailbox and reliable lanes produced for this tick, as
/// applyNetworkControls reads it: value copies only, so the mailbox is
/// not borrowed while the body, seats and roster are commanded.
#[derive(Clone, Debug, Default)]
pub struct Command {
    pub state_tick: f64,
    pub mouse_scale: Option<f64>,
    pub invert_mouse: bool,
    pub first_person: Option<bool>,
    pub view_tick: Option<f64>,
    pub flight: bool,
    pub sensitivity: Option<f64>,
    pub flight_invert: bool,
    pub curve: Option<f64>,
    pub gamepad: Option<([f64; 4], u32)>,
}

/// `(game.ai?.playerCommand)`: squad orders need an AI battle at all.
fn ai_present(w: &World) -> bool { w.ai.battle().is_some() }

/// `x || 0`: JavaScript's falsy zero (and -0) becomes +0.
fn or_zero(x: f64) -> f64 { if x != 0.0 && !x.is_nan() { x } else { 0.0 } }

/// Game.ai.playerCommand then the squad notice, for the order's kind
/// (0 move out, 2 hold, 3 follow, 1 at ease, as the key order lists them).
fn squad(w: &mut World, p: &Player, unit: Ref<Unit>, index: usize, kind: f64, time: f64) {
    let Some(character) = p.character.clone() else { return };
    let command = AiPlayerCommand { character, kind, time };
    w.sim.calls.result(AiPlayerCommand::WORDS, |row, heap| command.put(row, heap));
    crate::ai::owned::entry::ai_world_command_in(w);
    let life = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
    PlayerSquad { unit: unit.slot, life, label: index as u32 }.emit(&mut w.sim.events);
}

/// `u.combat.remotes.look(x, y, scale, invert)`: the recon context's own
/// look command (f3-recon.js context.look).
fn remote_look(w: &mut World, context: Ref<ReconContext>, x: f64, y: f64, scale: f64, invert: bool) {
    let Some(c) = w.sim.store.recon_context.get(context) else { return };
    let binding = F3ReconBinding { action: F3ReconAction::Look, key: c.key, owner: c.owner.clone(), seed: 0, droid: None, dx: x, dy: y, sensitivity: scale, flag: invert };
    w.sim.calls.result(F3ReconBinding::WORDS, |row, heap| binding.put(row, heap));
    crate::recon::module::f3_recon_command_in(w);
}

/// `(u.soldier.lookRaw || u.soldier.look)(x / zoom, y / zoom, scale, invert)`:
/// a soldier's raw pointer counts, a droideka's look (f3-sim.js look).
fn body_look(w: &mut World, unit: Ref<Unit>, x: f64, y: f64, scale: f64, invert: bool) {
    let defs = crate::combat::module::defs_in(&mut w.combat);
    let Some(u) = w.sim.store.unit.get_mut(unit) else { return };
    let zoom = crate::combat::combat::zoom_factor(&mut u.combat, &defs);
    let raw = matches!(u.soldier, UnitBody::Soldier(_));
    let change = F3Change { key: F3UnitKey::of(unit, u), action: if raw { F3Action::LookRaw } else { F3Action::Look },
        vector: [x / zoom, y / zoom, scale], flag: invert, attacker: Attacker::Null, ..Default::default() };
    change.put(crate::units::f3::command_row_in(&mut w.units), &mut Vec::new());
    crate::units::f3::f3_command_in(w);
}

/// `u.combat.damage(Infinity)`: a death nobody is credited for.
pub fn suicide(w: &mut World, unit: Ref<Unit>) {
    let Some(u) = w.sim.store.unit.get(unit) else { return };
    if u.combat.dead { return; }
    let change = F3Change { key: F3UnitKey::of(unit, u), action: F3Action::Damage, value: f64::INFINITY, on_shield: f64::INFINITY,
        attacker: Attacker::Null, ..Default::default() };
    change.put(crate::units::f3::command_row_in(&mut w.units), &mut Vec::new());
    crate::units::f3::f3_command_in(w);
}

// invoke's `hull.aiTarget.generation`: the hull life a seat command is
// qualified by, so old input never acts on a reused hull.
fn hull_life(w: &World, hull: Ref<Vehicle>) -> f64 { w.sim.store.vehicle.get(hull).map_or(0.0, |v| v.target_generation) }

/// The seat he holds: its lease, hull or turret, and his place in it.
fn seat_of(w: &World, unit: Ref<Unit>) -> Option<(Ref<SeatLease>, Option<Ref<Vehicle>>, Option<Ref<Turret>>, usize)> {
    let lease = w.sim.store.unit.get(unit)?.seat?;
    let l = w.sim.store.seat_lease.get(lease)?;
    Some((lease, l.vehicle, l.turret, l.index as usize))
}

// F2Seats.invoke's command record: placement on, no post, an ordinary exit
// reason; only the fields a network player's operations set vary.
fn seat_command(w: &mut World, unit: Ref<Unit>, op: F2SeatOp, hull: Option<Ref<Vehicle>>, lease: Option<Ref<SeatLease>>, index: f64, hull_life: f64, direction: f64) -> F2SeatResult {
    let unit_life = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
    seats::command_in(w, F2SeatCommand { op, unit: Some(unit), unit_life, hull, hull_life, lease, index, placement: true,
        reason: SeatReason::Exit, safe: false, force: false, promote: None, post: None, direction })
}

/// The seat's key reader for flightInput: his held codes and queued presses
/// through the default flight bindings, his game pad from the command.
struct FlightReader<'a> { player: &'a mut Player, names: &'a super::owner::Names, pad: Option<([f64; 4], u32)> }
impl InputReader for FlightReader<'_> {
    type Error = std::convert::Infallible;
    type Binding = usize;
    type Code = StrId;
    type Value = bool;
    fn binding(&mut self, name: &str) -> Result<(usize, usize), Self::Error> {
        let index = match name {
            "throttle" => 0, "brake" => 1, "rollLeft" => 2, "rollRight" => 3, "boost" => 4,
            "trick" => 5, "immelmann" => 6, "rollLock" => 7, "takeoff" => 8, _ => 9,
        };
        Ok((index, self.names.flight[index].1.iter().filter(|c| c.is_some()).count()))
    }
    fn code(&mut self, binding: usize, index: usize) -> Result<Option<StrId>, Self::Error> { Ok(self.names.flight[binding].1[index]) }
    fn down(&mut self, code: StrId) -> Result<bool, Self::Error> { Ok(self.player.controls.has(code)) }
    fn take(&mut self, action: &str) -> Result<bool, Self::Error> {
        let (name, _) = self.names.flight_codes(action);
        Ok(self.player.controls.take(name))
    }
    fn axis(&mut self, index: usize) -> Result<Option<f64>, Self::Error> { Ok(self.pad.and_then(|(axes, _)| axes.get(index).copied())) }
    fn button(&mut self, index: usize) -> Result<bool, Self::Error> { Ok(self.pad.is_some_and(|(_, buttons)| index < 32 && buttons & (1 << index) != 0)) }
}

/// boarding.js seatInput for a network pilot (no steering assistance), as
/// vehicle-phase-data.js flightKeys encodes it: an absent key is NaN.
fn seat_keys(w: &World, p: &mut Player, names: &super::owner::Names, hull: Ref<Vehicle>, index: usize, command: &Command) -> FlightInput {
    let nan = f64::NAN;
    let blank = FlightInput { forward: 0.0, right: 0.0, turn: 0.0, pitch: nan, up: nan, down: nan, takeoff: nan, stick_x: None, stick_y: None,
        boost: nan, trick: nan, immelmann: nan, roll_lock: nan, target_lock: nan, bot: nan, response_curve: nan, no_assist: false };
    let Some(v) = w.sim.store.vehicle.get(hull) else { return blank };
    match seat_control(v, index) {
        Some(SeatControl::Flight) => {
            let keys = read_input(&mut FlightReader { player: p, names, pad: command.gamepad }).unwrap();
            let b = |x: bool| if x { 1.0 } else { 0.0 };
            FlightInput { forward: keys.forward, right: keys.right, stick_x: keys.stick.map(|s| s.0), stick_y: keys.stick.map(|s| s.1),
                boost: b(keys.boost), roll_lock: b(keys.roll_lock), takeoff: b(keys.takeoff.truthy()), trick: b(keys.trick.truthy()),
                immelmann: b(keys.immelmann.truthy()), target_lock: b(keys.target_lock.truthy()),
                response_curve: command.curve.unwrap_or(nan), no_assist: true, ..blank }
        }
        Some(SeatControl::Drive) => FlightInput { forward: p.controls.axis(&names.forward[0], &names.forward[1]),
            right: p.controls.axis(&names.right[0], &names.right[1]), turn: p.controls.axis(&names.turn[0], &names.turn[1]), ..blank },
        _ => blank,
    }
}

/// boarding.js seatInput's `brake`: the driving keys' Space, else nothing.
fn seat_brake(w: &World, p: &Player, names: &super::owner::Names, hull: Ref<Vehicle>, index: usize) -> bool {
    w.sim.store.vehicle.get(hull).and_then(|v| seat_control(v, index)) == Some(SeatControl::Drive) && p.controls.has(names.space)
}

/// applyNetworkControls(p, u, command, tick). `mouse_scale`: the world's
/// setting when the command names none; `time`: the AI's battle time.
pub fn apply(w: &mut World, players: &mut Players, key: usize, unit: Ref<Unit>, command: &Command, looks: &[[f64; 2]], tick: f64, mouse_scale: f64, time: f64) {
    players.reserve_shadows(unit.slot as usize + 1);
    let names = players.names.take().expect("activated players");
    let Some(p) = players.pool.get_mut(key) else { players.names = Some(names); return };
    // Squad commands first, in the key table's order (F3, F4, F2, F1 as the
    // original PC controls list them); the order's kind is the AI's.
    for (index, kind) in [0.0, 2.0, 3.0, 1.0].into_iter().enumerate() {
        if p.controls.take(names.squads[index]) && ai_present(w) { squad(w, p, unit, index, kind, time); }
    }
    // Older clients still send N (nextSpawn): dropped, so it never piles up.
    while p.controls.take(names.next_spawn) {}
    // combat.firstPerson only chooses the page's own view; a flown droid's
    // first-person flag reaches its camera through the unit phase's row.
    if let (Some(first_person), Some(context)) = (command.first_person, crate::recon::module::context_in(w, unit)) {
        if w.sim.store.unit.get(unit).is_some_and(|u| u.combat.has_remotes) {
            w.sim.store.recon_context.get_mut(context).unwrap().first_person = first_person;
        }
    }
    let shadow = &mut players.shadows[unit.slot as usize];
    let mut intent = if shadow.fresh { shadow.intent.clone().unwrap_or_default() } else { w.sim.store.unit.get(unit).map(|u| u.intent.clone()).unwrap_or_default() };
    intent.forward = p.controls.axis(&names.forward[0], &names.forward[1]);
    intent.right = p.controls.axis(&names.right[0], &names.right[1]);
    intent.turn = p.controls.axis(&names.turn[0], &names.turn[1]);
    intent.pitch = 0.0;
    // One press each of the world's actions, taken in applyNetworkControls'
    // order; the seat section below may consume the weapon selections.
    let presses: [bool; 9] = std::array::from_fn(|i| p.controls.take(names.actions[i]));
    intent.jump = presses[0]; intent.crouch = presses[1]; intent.prone = presses[2]; intent.r#use = presses[3]; intent.reload = presses[4];
    intent.next_primary = presses[5]; intent.previous_primary = presses[6]; intent.next_secondary = presses[7]; intent.zoom = presses[8];
    // The body's network facts for this tick: shots come below.
    let mut facts = NetworkFacts { shots: [None, None],
        view: if p.inputs_stale { None } else { command.view_tick },
        lead: if p.inputs_stale { None } else { p.input_lead_ticks },
        playout: Some(ONLINE_PLAYOUT_MAX_TICKS) };
    if p.reliable_fire {
        // FireCommands.take with the mailbox's consumed state: a release that
        // stalled behind retransmissions is recovered from the movement lane.
        let generation = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
        let movement = IfMovement { tick: command.state_tick, buttons: [f64::from(p.controls.has(names.mouse0)), f64::from(p.controls.has(names.mouse2))] };
        p.fire.shots.clear();
        if crate::input::fire::take(&mut p.fire, tick, generation, Some(movement), &mut w.sim.strings).is_err() { players.faults = players.faults.wrapping_add(1); }
        intent.buttons = p.fire.state.buttons;
        // How late each press ran: its lockout counts from its own tick.
        for shot in &p.fire.shots {
            let button = shot.button as usize;
            if button < 2 && facts.shots[button].is_none() {
                facts.shots[button] = Some(Shot { id: shot.id, tick: shot.tick, late: v8math::max(0.0, tick - shot.tick),
                    aim_yaw: shot.aim_yaw, aim_pitch: shot.aim_pitch, view_tick: shot.view_tick });
            }
        }
    } else {
        // Fire and seat keys as his seat reads them now (src/boarding.js seatScheme).
        let flying = seat_of(w, unit).and_then(|(_, hull, _, _)| hull).and_then(|hull| w.sim.store.vehicle.get(hull)).is_some_and(|v| v.flying);
        let (primary, secondary): (&[StrId], &[StrId]) = if flying { (&names.flight_fire_primary, &names.flight_fire_secondary) } else { (&names.fire_primary, &names.fire_secondary) };
        intent.buttons = [f64::from(p.controls.held(primary)), f64::from(p.controls.held(secondary))];
    }
    shadow.network = Some(facts);
    // Space brakes and lifts, Control descends: the hull and jet keys.
    intent.brake = Some(p.controls.has(names.space));
    intent.up = Some(f64::from(p.controls.has(names.space)));
    intent.down = Some(f64::from(p.controls.has(names.control_left)));
    // Mouse counts are clamped a sample at a time; a flown droid takes them
    // whole, a body divides them by its zoom (soldier.js lookRaw).
    let scale = v8math::min(command.mouse_scale.unwrap_or(mouse_scale), 2.0);
    let remotes_active = crate::recon::module::active_in(w, unit) == Some(true);
    let context = crate::recon::module::context_in(w, unit);
    for look in looks {
        let x = v8math::max(-10000.0, v8math::min(10000.0, look[0]));
        let y = v8math::max(-10000.0, v8math::min(10000.0, look[1]));
        match (remotes_active, context) {
            (true, Some(context)) => remote_look(w, context, x, y, scale, command.invert_mouse),
            _ => body_look(w, unit, x, y, scale, command.invert_mouse),
        }
    }
    if let Some((lease, hull, _, _)) = seat_of(w, unit) {
        if let Some(hull) = hull {
            if intent.next_primary { seat_command(w, unit, F2SeatOp::Cycle, Some(hull), Some(lease), 0.0, hull_life(w, hull), 1.0); }
            if intent.previous_primary { seat_command(w, unit, F2SeatOp::Cycle, Some(hull), Some(lease), 0.0, hull_life(w, hull), -1.0); }
            intent.next_primary = false;
            intent.previous_primary = false;
            // The keys of the seat he holds after any switch, as the hull takes them.
            if let Some((lease, Some(hull), _, index)) = seat_of(w, unit) {
                let scales = LookScales { scale: v8math::min(2.0, command.mouse_scale.unwrap_or(1.0)), invert: command.invert_mouse,
                    flight_scale: v8math::max(0.05, v8math::min(10.0, command.sensitivity.unwrap_or(1.0))), flight_invert: command.flight_invert };
                seats::seat_samples_in(w, &scales, looks);
                seat_command(w, unit, F2SeatOp::Network, Some(hull), Some(lease), index as f64, hull_life(w, hull), 1.0);
                let keys = seat_keys(w, p, &names, hull, index, command);
                intent.forward = or_zero(keys.forward);
                intent.right = or_zero(keys.right);
                intent.turn = or_zero(keys.turn);
                intent.brake = Some(seat_brake(w, p, &names, hull, index));
                shadow.flight = Flight::Keys(keys);
            }
        } else {
            // A turret's gunner turns and pitches by his summed mouse counts,
            // at the original's 60 counts a 2048th of a turn.
            let horizontal: f64 = looks.iter().fold(0.0, |n, v| n + v[0]);
            let vertical: f64 = looks.iter().fold(0.0, |n, v| n + v[1]);
            intent.turn += horizontal * scale * 60.0 / 2048.0;
            intent.pitch = -vertical * scale * 60.0 / 2048.0 * if command.invert_mouse { -1.0 } else { 1.0 };
        }
    } else if intent.r#use && !remotes_active {
        // What his E reaches, by the player's rules (src/boarding.js mountInReach).
        match seats::mount_in_reach_in(w, unit, true) {
            Some(Mount::Hull(index)) => {
                if let Some(&hull) = w.sim.store.vehicles.vehicles.get(index) {
                    seat_command(w, unit, F2SeatOp::Board, Some(hull), None, -1.0, hull_life(w, hull), 1.0);
                }
            }
            Some(Mount::Turret(index)) => {
                let life = seats::turret_generation_in(w, index);
                seat_command(w, unit, F2SeatOp::Man, None, None, index as f64, life, 1.0);
            }
            None => {}
        }
        if seat_of(w, unit).is_some() { intent.r#use = false; }
    }
    shadow.intent = Some(intent);
    shadow.fresh = true;
    p.applied = true;
    players.names = Some(names);
}

/// stopInput(p): his queues emptied, whatever he controls stilled (a hull's
/// sticks and a flown droid's keys alike). Leaving also hands the droid
/// back (the host's removePlayer), but a pause or a gap in his input does not.
pub fn stop(w: &mut World, players: &mut Players, key: usize) {
    let Some(p) = players.pool.get_mut(key) else { return };
    if let Some(mailbox) = w.input.mailboxes.get_mut(p.mailbox) { mailbox.clear_command(&mut w.sim.strings); }
    p.controls.clear();
    if crate::input::fire::clear(&mut p.fire.state, &mut w.sim.strings).is_err() { players.faults = players.faults.wrapping_add(1); }
    if crate::input::actions::clear(&mut p.actions, &mut w.sim.strings).is_err() { players.faults = players.faults.wrapping_add(1); }
    p.stopped = true;
    let Some(unit) = super::step::body(w, p) else { return };
    let shadow = players.shadow(unit.slot);
    // Object.assign(unit.intent, blankSeatIntent(), {flight: {}}): the jet
    // direction is the one intent field the blank leaves alone.
    let mut intent = if shadow.fresh { shadow.intent.clone().unwrap_or_default() } else { w.sim.store.unit.get(unit).map(|u| u.intent.clone()).unwrap_or_default() };
    intent.forward = 0.0; intent.right = 0.0; intent.turn = 0.0; intent.pitch = 0.0;
    intent.jump = false; intent.crouch = false; intent.prone = false; intent.r#use = false; intent.exit = false;
    intent.buttons = [0.0, 0.0];
    intent.reload = false; intent.next_primary = false; intent.previous_primary = false; intent.next_secondary = false; intent.zoom = false;
    intent.aim = None;
    intent.brake = Some(false); intent.up = Some(0.0); intent.down = Some(0.0);
    shadow.intent = Some(intent);
    shadow.fresh = true;
    shadow.flight = Flight::Empty;
    shadow.network = Some(NetworkFacts { shots: [None, None], view: None, lead: None, playout: None });
    if let Some((lease, Some(hull), _, 0)) = seat_of(w, unit) {
        seat_command(w, unit, F2SeatOp::ResetSticks, Some(hull), Some(lease), 0.0, hull_life(w, hull), 1.0);
    }
}
