//! A network player's controls applied to his body each tick
//! (applyNetworkControls and stopInput):
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
/// look command (context.look).
fn remote_look(w: &mut World, context: Ref<ReconContext>, x: f64, y: f64, scale: f64, invert: bool) {
    let Some(c) = w.sim.store.recon_context.get(context) else { return };
    let binding = F3ReconBinding { action: F3ReconAction::Look, key: c.key, owner: c.owner.clone(), seed: 0, droid: None, dx: x, dy: y, sensitivity: scale, flag: invert };
    w.sim.calls.result(F3ReconBinding::WORDS, |row, heap| binding.put(row, heap));
    crate::recon::module::f3_recon_command_in(w);
}

/// `(u.soldier.lookRaw || u.soldier.look)(x / zoom, y / zoom, scale, invert)`:
/// a soldier's raw pointer counts, a droideka's look (look).
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

/// seatInput for a network pilot (no steering assistance), as
/// flightKeys encodes it: an absent key is NaN.
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

/// seatInput's `brake`: the driving keys' Space, else nothing.
fn seat_brake(w: &World, p: &Player, names: &super::owner::Names, hull: Ref<Vehicle>, index: usize) -> bool {
    w.sim.store.vehicle.get(hull).and_then(|v| seat_control(v, index)) == Some(SeatControl::Drive) && p.controls.has(names.space)
}

/// applyNetworkControls(p, u, command, tick). `mouse_scale`: the world's
/// setting when the command names none; `time`: the AI's battle time.
pub fn apply(w: &mut World, players: &mut Players, key: usize, unit: Ref<Unit>, command: &Command, looks: &[[f64; 2]], tick: f64, mouse_scale: f64, time: f64) {
    if players.names.as_ref().is_none_or(|names| names.swbf2.is_none()) {
        released_apply(w, players, key, unit, command, looks, tick, mouse_scale, time);
    } else {
        swbf2_apply(w, players, key, unit, command, looks, tick, mouse_scale, time);
    }
}

fn released_apply(w: &mut World, players: &mut Players, key: usize, unit: Ref<Unit>, command: &Command, looks: &[[f64; 2]], tick: f64, mouse_scale: f64, time: f64) {
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
    // Mounted guns aim from the selected view's origin as well as drawing it.
    if let (Some(first_person), Some(u)) = (command.first_person, w.sim.store.unit.get_mut(unit)) {
        u.combat.first_person = first_person;
    }
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
        // Fire and seat keys as his seat reads them now (seatScheme).
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
    // whole, a body divides them by its zoom (lookRaw).
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
        // What his E reaches, by the player's rules (mountInReach).
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

fn swbf2_apply(w: &mut World, players: &mut Players, key: usize, unit: Ref<Unit>, command: &Command, looks: &[[f64; 2]], tick: f64, mouse_scale: f64, time: f64) {
    players.reserve_shadows(unit.slot as usize + 1);
    let names = players.names.take().expect("activated players");
    let Some(p) = players.pool.get_mut(key) else { players.names = Some(names); return };
    for (index, kind) in [0.0, 2.0, 3.0, 1.0].into_iter().enumerate() {
        if p.controls.take(names.squads[index]) && ai_present(w) { squad(w, p, unit, index, kind, time); }
    }
    while p.controls.take(names.next_spawn) {}
    if let (Some(first_person), Some(u)) = (command.first_person, w.sim.store.unit.get_mut(unit)) {
        u.combat.first_person = first_person;
    }
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
    let presses: [bool; 9] = std::array::from_fn(|i| p.controls.take(names.actions[i]));
    intent.jump = presses[0]; intent.crouch = presses[1]; intent.prone = presses[2]; intent.r#use = presses[3]; intent.reload = presses[4];
    intent.next_primary = presses[5]; intent.previous_primary = presses[6]; intent.next_secondary = presses[7]; intent.zoom = presses[8];
    // The 2005 soldier's own inputs. Prone has no 2005 posture: its press is
    // consumed without effect, as is a roll press that arrives while seated.
    // Block is held from its press edge to its release edge, both durable,
    // so a short press is never lost to a replaced held bitmap. The edges
    // apply one an application tick in the order they were sent, so a
    // release then a press ends pressed and a tap is a press then a release.
    // Each life starts released: a control still held from the previous life
    // takes a press in this one.
    let mut triggers = None;
    if let Some(extra) = names.swbf2 {
        intent.prone = false;
        let on_foot = seat_of(w, unit).is_none();
        let life = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
        let roll = p.controls.take(extra.roll);
        let edge = p.controls.take_first(&[extra.block_press, extra.block_release]);
        let (pressed, released) = (edge == Some(extra.block_press), edge == Some(extra.block_release));
        let held = shadow.swbf2.block && shadow.swbf2_life == life;
        let block = on_foot && !released && (pressed || held);
        // The combo buttons of a network soldier: jump, crouch, sprint and
        // reload from their held codes and press edges, the triggers from his
        // fire lane below. A press is a rising held level in this life or the
        // action press itself.
        let before = if shadow.swbf2_life == life { (shadow.swbf2.melee_held, shadow.swbf2.offhand_held) } else { (0, false) };
        let mut melee_held = 0u8;
        let mut melee_pressed = 0u8;
        if on_foot {
            use crate::combat::combo::ComboButton::{Crouch, Jump, Reload, Sprint};
            for (code, button) in [(names.space, Jump), (extra.crouch, Crouch), (extra.sprint, Sprint), (extra.reload, Reload)] {
                if p.controls.has(code) { melee_held |= button.bit(); }
            }
            melee_pressed = melee_held & !before.0;
            for (press, button) in [(intent.jump, Jump), (intent.crouch, Crouch), (intent.reload, Reload)] {
                if press { melee_pressed |= button.bit(); }
            }
            triggers = Some(before);
        }
        shadow.swbf2 = super::owner::Swbf2Intent { sprint: on_foot && p.controls.has(extra.sprint), block,
            block_pressed: on_foot && pressed, block_released: released, roll: on_foot && roll,
            melee_held, melee_pressed, offhand_held: false, offhand_pressed: false,
            hold_to_block: on_foot && p.controls.has(extra.hold_to_block),
            lock_pressed: on_foot && p.controls.take(extra.target_lock) };
        shadow.swbf2_life = life;
    }
    let mut facts = NetworkFacts { shots: [None, None],
        view: if p.inputs_stale { None } else { command.view_tick },
        lead: if p.inputs_stale { None } else { p.input_lead_ticks },
        playout: Some(ONLINE_PLAYOUT_MAX_TICKS) };
    if p.reliable_fire {
        let generation = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
        let movement = IfMovement { tick: command.state_tick, buttons: [f64::from(p.controls.has(names.mouse0)), f64::from(p.controls.has(names.mouse2))] };
        p.fire.shots.clear();
        if crate::input::fire::take(&mut p.fire, tick, generation, Some(movement), &mut w.sim.strings).is_err() { players.faults = players.faults.wrapping_add(1); }
        intent.buttons = p.fire.state.buttons;
        for shot in &p.fire.shots {
            let button = shot.button as usize;
            if button < 2 && facts.shots[button].is_none() {
                facts.shots[button] = Some(Shot { id: shot.id, tick: shot.tick, late: v8math::max(0.0, tick - shot.tick),
                    aim_yaw: shot.aim_yaw, aim_pitch: shot.aim_pitch, view_tick: shot.view_tick });
            }
        }
    } else {
        let flying = seat_of(w, unit).and_then(|(_, hull, _, _)| hull).and_then(|hull| w.sim.store.vehicle.get(hull)).is_some_and(|v| v.flying);
        let (primary, secondary): (&[StrId], &[StrId]) = if flying { (&names.flight_fire_primary, &names.flight_fire_secondary) } else { (&names.fire_primary, &names.fire_secondary) };
        intent.buttons = [f64::from(p.controls.held(primary)), f64::from(p.controls.held(secondary))];
    }
    shadow.network = Some(facts);
    // A 2005 soldier's triggers for his combo and his offhand weapon: the
    // fire lane's levels and presses, so a tap between two commands counts.
    if let Some((held, offhand)) = triggers {
        let fire = crate::combat::combo::ComboButton::Fire.bit();
        let down = intent.buttons.map(|level| level != 0.0);
        let pressed = |button: usize, before: bool| match p.reliable_fire {
            true => p.fire.shots.iter().any(|shot| shot.button as usize == button),
            false => down[button] && !before,
        };
        let soldier = &mut shadow.swbf2;
        if down[0] { soldier.melee_held |= fire; }
        if pressed(0, held & fire != 0) { soldier.melee_pressed |= fire; }
        soldier.offhand_held = down[1];
        soldier.offhand_pressed = pressed(1, offhand);
    }
    intent.brake = Some(p.controls.has(names.space));
    intent.up = Some(f64::from(p.controls.has(names.space)));
    intent.down = Some(f64::from(p.controls.has(names.control_left)));
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
            let horizontal: f64 = looks.iter().fold(0.0, |n, v| n + v[0]);
            let vertical: f64 = looks.iter().fold(0.0, |n, v| n + v[1]);
            intent.turn += horizontal * scale * 60.0 / 2048.0;
            intent.pitch = -vertical * scale * 60.0 / 2048.0 * if command.invert_mouse { -1.0 } else { 1.0 };
        }
    } else if intent.r#use && !remotes_active {
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
    if players.names.as_ref().is_none_or(|names| names.swbf2.is_none()) {
        return released_stop(w, players, key);
    }
    released_stop(w, players, key);
    let Some(p) = players.pool.get(key) else { return };
    if let Some(unit) = super::step::body(w, p) {
        players.shadow(unit.slot).swbf2 = Default::default();
    }
}

fn released_stop(w: &mut World, players: &mut Players, key: usize) {
    let Some(p) = players.pool.get_mut(key) else { return };
    if let Some(mailbox) = w.input.mailboxes.get_mut(p.mailbox) { mailbox.clear_command(&mut w.sim.strings); }
    p.controls.clear();
    if crate::input::fire::clear(&mut p.fire.state, &mut w.sim.strings).is_err() { players.faults = players.faults.wrapping_add(1); }
    if crate::input::actions::clear_reusing(&mut p.actions, &mut w.sim.strings, &mut p.action_entries).is_err() { players.faults = players.faults.wrapping_add(1); }
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

#[cfg(test)]
mod tests {
    #[test]
    fn player_view_commands_reach_the_combat_state_used_by_mounted_guns() {
        use crate::players::{commands::{apply, Command}, owner::{Player, Players}};
        let mut world = crate::world::owner::World::new();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let unit = world.sim.store.unit.insert(crate::gen::Unit::default());
        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 1);
        let key = players.admit(Player::new(0, 0., 4)).unwrap();
        for (view, expected) in [(Some(true), true), (None, true), (Some(false), false)] {
            apply(&mut world, &mut players, key, unit, &Command { first_person: view, ..Default::default() }, &[], 1., 1., 0.);
            assert_eq!(world.sim.store.unit.get(unit).unwrap().combat.first_person, expected);
        }
    }

    // The same held codes and presses: a 2005 battle reads sprint, block and
    // the roll press into the unit's shadow and drops the prone press; a 2004
    // battle reads prone as always and has no 2005 state at all.
    fn s2(world: &mut crate::world::owner::World, text: &str) -> crate::store::StrId { world.sim.strings.intern_str(text) }

    #[test]
    fn only_a_2005_battle_reads_sprint_block_and_roll() {
        use crate::players::{commands::{apply, Command}, owner::{Player, Players, Swbf2Intent, Swbf2Names}};
        use bfe_family::GameFamily;
        for family in GameFamily::ALL {
            let mut world = crate::world::owner::World::new();
            world.movement.select_family(family).unwrap();
            crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
            let unit = world.sim.store.unit.insert(crate::gen::Unit::default());
            let mut players = Players::new();
            players.activate(&mut world.sim.strings, 1);
            if family == GameFamily::Swbf2 {
                players.names.as_mut().unwrap().swbf2 = Some(Swbf2Names::new(&mut world.sim.strings));
            }
            let key = players.admit(Player::new(0, 0., 4)).unwrap();
            let mut s = |text: &str| world.sim.strings.intern_str(text);
            let down = [s("KeyW"), s("ShiftLeft")];
            let presses = [s("prone"), s("roll"), s("blockPress"), s("targetLock")];
            let p = players.pool.get_mut(key).unwrap();
            assert!(p.controls.accept(1., 1., 0., &down, &presses));
            apply(&mut world, &mut players, key, unit, &Command::default(), &[], 1., 1., 0.);
            let shadow = &players.shadows[unit.slot as usize];
            let intent = shadow.intent.clone().unwrap();
            if family == GameFamily::Swbf2 {
                assert_eq!(shadow.swbf2, Swbf2Intent { sprint: true, block: true, block_pressed: true, block_released: false, roll: true, melee_held: crate::combat::combo::ComboButton::Sprint.bit(), melee_pressed: crate::combat::combo::ComboButton::Sprint.bit(), offhand_held: false, offhand_pressed: false, hold_to_block: false, lock_pressed: true });
                assert!(!intent.prone);
                // Block stays held without a held code until its release edge.
                let release = [s2(&mut world, "blockRelease")];
                let p = players.pool.get_mut(key).unwrap();
                assert!(p.controls.accept(1., 2., 0., &[], &[]));
                apply(&mut world, &mut players, key, unit, &Command::default(), &[], 2., 1., 0.);
                assert!(players.shadows[unit.slot as usize].swbf2.block);
                let p = players.pool.get_mut(key).unwrap();
                assert!(p.controls.accept(1., 3., 0., &[], &release));
                apply(&mut world, &mut players, key, unit, &Command::default(), &[], 3., 1., 0.);
                let shadow = &players.shadows[unit.slot as usize].swbf2;
                assert!(!shadow.block && shadow.block_released);
            } else {
                assert_eq!(shadow.swbf2, Swbf2Intent::default());
                assert!(intent.prone);
                assert!(players.names.as_ref().unwrap().swbf2.is_none());
            }
            assert_eq!(intent.forward, 1.0);
        }
    }

    // A 2005 soldier's combo buttons and setting: levels from the semantic
    // held codes, presses from rising levels and durable actions, triggers
    // from the fire lane, so a tap between two commands is one tick's press.
    // His soldier step takes each press once.
    #[test]
    fn a_2005_soldiers_buttons_are_levels_and_durable_presses() {
        use crate::combat::combo::ComboButton;
        use crate::gen::IfEdge;
        use crate::players::{commands::{apply, Command}, owner::{Player, Players, Swbf2Names}};
        let mut world = crate::world::owner::World::new();
        world.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let unit = world.sim.store.unit.insert(crate::gen::Unit { generation: 1., ..Default::default() });
        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 1);
        players.names.as_mut().unwrap().swbf2 = Some(Swbf2Names::new(&mut world.sim.strings));
        let key = players.admit(Player::new(0, 0., 4)).unwrap();
        players.pool.get_mut(key).unwrap().reliable_fire = true;
        let [space, crouch, reload, setting, mouse0] = ["Space", "KeyC", "KeyR", "KeyB", "Mouse0"].map(|code| s2(&mut world, code));
        let [crouch_press, reload_press] = ["crouch", "reload"].map(|action| s2(&mut world, action));
        let bits = |buttons: &[ComboButton]| buttons.iter().fold(0, |bits, button| bits | button.bit());
        use ComboButton::{Crouch, Fire, Jump, Reload};
        let mut edges = 0.;
        let mut step = |world: &mut crate::world::owner::World, players: &mut Players, tick: f64, down: &[crate::store::StrId],
            presses: &[crate::store::StrId], triggers: &[(f64, bool)]| {
            let p = players.pool.get_mut(key).unwrap();
            assert!(p.controls.accept(1., tick, 1., down, presses));
            for &(button, pressed) in triggers {
                edges += 1.;
                let edge = IfEdge { id: edges, tick, button, pressed, generation: 1., ..Default::default() };
                assert!(crate::input::fire::push(&mut p.fire.state, Some(edge), true, true, true, tick));
            }
            apply(world, players, key, unit, &Command::default(), &[], tick, 1., 0.);
            players.shadows[unit.slot as usize].swbf2
        };
        let got = step(&mut world, &mut players, 1., &[space, crouch, reload, setting], &[], &[]);
        assert_eq!((got.melee_held, got.melee_pressed, got.hold_to_block), (bits(&[Jump, Crouch, Reload]), bits(&[Jump, Crouch, Reload]), true));
        let got = step(&mut world, &mut players, 2., &[crouch, setting], &[], &[]);
        assert_eq!((got.melee_held, got.melee_pressed), (bits(&[Crouch]), 0));
        // Presses whose level never showed, and a tap of both triggers.
        let got = step(&mut world, &mut players, 3., &[], &[crouch_press, reload_press], &[(0., true), (0., false), (1., true), (1., false)]);
        assert_eq!((got.melee_held, got.melee_pressed, got.hold_to_block), (bits(&[Fire]), bits(&[Fire, Crouch, Reload]), false));
        assert_eq!((got.offhand_held, got.offhand_pressed), (true, true));
        let got = step(&mut world, &mut players, 4., &[], &[], &[]);
        assert_eq!((got.melee_held, got.melee_pressed, got.offhand_held, got.offhand_pressed), (0, 0, false, false));
        // A trigger held: one press, then the level.
        let got = step(&mut world, &mut players, 5., &[mouse0], &[], &[(0., true)]);
        assert_eq!((got.melee_held, got.melee_pressed), (bits(&[Fire]), bits(&[Fire])));
        let got = step(&mut world, &mut players, 6., &[mouse0, setting], &[], &[]);
        assert_eq!((got.melee_held, got.melee_pressed), (bits(&[Fire]), 0));

        // The soldier's controls take the setting as a level and each press once.
        let shadow = &mut players.shadows[unit.slot as usize].swbf2;
        shadow.block = true;
        shadow.block_pressed = true;
        for (setting, block_pressed) in [(true, true), (true, false), (false, false)] {
            let mut controls = crate::units::infantry::Controls::default();
            shadow.hold_to_block = setting;
            shadow.take_into(&mut controls);
            assert_eq!((controls.hold_to_block, controls.block, controls.block_pressed), (setting, true, block_pressed));
        }
    }

    // A release then a press that reach one application tick end pressed, a
    // press then a release is a tap of one tick, and a block still held when
    // a new life starts is released until a press in that life.
    #[test]
    fn block_edges_apply_in_order_and_each_life_starts_released() {
        use crate::players::{commands::{apply, Command}, owner::{Player, Players, Swbf2Names}};
        let mut world = crate::world::owner::World::new();
        world.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let unit = world.sim.store.unit.insert(crate::gen::Unit::default());
        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 1);
        players.names.as_mut().unwrap().swbf2 = Some(Swbf2Names::new(&mut world.sim.strings));
        let key = players.admit(Player::new(0, 0., 4)).unwrap();
        let (press, release) = (s2(&mut world, "blockPress"), s2(&mut world, "blockRelease"));
        for (sequence, new_life, edges, expected) in [
            (1., false, vec![press], (true, true, false)),
            // One command carrying a release, then a press.
            (2., false, vec![release, press], (false, false, true)),
            (3., false, vec![], (true, true, false)),
            (4., false, vec![release], (false, false, true)),
            // A tap: a press, then a release.
            (5., false, vec![press, release], (true, true, false)),
            (6., false, vec![], (false, false, true)),
            // Held into a new life.
            (7., false, vec![press], (true, true, false)),
            (8., true, vec![], (false, false, false)),
            (9., false, vec![press], (true, true, false)),
        ] {
            if new_life { world.sim.store.unit.get_mut(unit).unwrap().generation += 1.; }
            let life = world.sim.store.unit.get(unit).unwrap().generation;
            assert!(players.pool.get_mut(key).unwrap().controls.accept(1., sequence, life, &[], &edges));
            apply(&mut world, &mut players, key, unit, &Command::default(), &[], sequence, 1., 0.);
            let shadow = players.shadows[unit.slot as usize].swbf2;
            assert_eq!((shadow.block, shadow.block_pressed, shadow.block_released), expected, "step {sequence}");
        }
    }

    // Pausing, stalling or disconnecting stills a 2005 soldier's shadow too:
    // sprint, block, hold-to-block and combo button holds clear with his intent.
    #[test]
    fn stop_clears_2005_soldier_holds_and_flags() {
        use crate::battle::{f4_connections, f4_core};
        use crate::gen::{Character, F4BattleSetup, F4ConnectionCommand, F4ConnectionOp, F4Handle, IfEdge, Unit};
        use crate::players::{commands::{apply, stop, Command}, owner::{Player, Players, Swbf2Intent, Swbf2Names}};
        let mut world = crate::world::owner::World::new();
        world.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let character = world.sim.store.character.insert(Character { human: true, team: 1., ..Default::default() });
        world.sim.store.battle.characters.push(character);
        world.sim.calls.result(F4BattleSetup::WORDS, |row, heap| {
            F4BattleSetup { capacity: 4, ..Default::default() }.put(row, heap);
        });
        f4_core::f4_battle_init_in(&mut world);
        let handle = F4Handle::take(f4_core::f4_character_handle_in(&mut world, 0), &[]);
        f4_connections::f4_connections_init_in(&mut world, 4);
        let admit = F4ConnectionCommand { op: F4ConnectionOp::Admit, target: handle.clone(), ordinal: 1., ..Default::default() };
        world.sim.calls.result(F4ConnectionCommand::WORDS, |row, heap| admit.put(row, heap));
        f4_connections::f4_connection_command_in(&mut world);
        let unit = world.sim.store.unit.insert(Unit { generation: 1., alive: true, ..Default::default() });
        f4_connections::set_test_unit(&mut world, &handle, unit);

        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 4);
        players.names.as_mut().unwrap().swbf2 = Some(Swbf2Names::new(&mut world.sim.strings));
        let mut player = Player::new(0, 0., 4);
        player.character = Some(handle);
        player.reliable_fire = true;
        let key = players.admit(player).unwrap();
        let down = [s2(&mut world, "ShiftLeft"), s2(&mut world, "KeyB"), s2(&mut world, "KeyC"), s2(&mut world, "Mouse0"), s2(&mut world, "Mouse2")];
        let presses = [s2(&mut world, "roll"), s2(&mut world, "blockPress")];
        let p = players.pool.get_mut(key).unwrap();
        assert!(p.controls.accept(1., 1., 1., &down, &presses));
        for (id, button) in [(1., 0.), (2., 1.)] {
            let edge = IfEdge { id, tick: 1., button, pressed: true, generation: 1., ..Default::default() };
            assert!(crate::input::fire::push(&mut p.fire.state, Some(edge), true, true, true, 1.));
        }
        apply(&mut world, &mut players, key, unit, &Command::default(), &[], 1., 1., 0.);
        let held = players.shadows[unit.slot as usize].swbf2;
        assert!(held.sprint && held.block && held.hold_to_block && held.melee_held != 0 && held.offhand_held);

        stop(&mut world, &mut players, key);
        assert_eq!(players.shadows[unit.slot as usize].swbf2, Swbf2Intent::default());
    }
}
