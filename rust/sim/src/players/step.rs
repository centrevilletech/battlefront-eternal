//! The players' phase of a dedicated tick (src/runtime/multiplayer-world.js
//! step): the hit history's capture, then each connection in joining order:
//! its grace after a disconnect, the half second without input that stills
//! it, the command its mailbox and reliable lanes yield, the spawn screen's
//! choices, its dead body's clock and, alive, its controls. The AI's camera
//! facts, the unit phase's rows and the pilots' flight keys follow.
use super::{commands::{self, Command}, owner::{Player, Players}};
use crate::gen::*;
use crate::store::{block::HEADER, Ref, StrId};
use crate::world::owner::World;

/// A disconnected player's body and roster place are held this long in
/// simulated seconds (60 s at the 60 Hz world clock), for a reconnect.
const DISCONNECT_GRACE_SECONDS: f64 = 60.0;
/// Half a second of wall time without input stills a player.
const STALE_INPUT_MS: f64 = 500.0;

/// The connection's retained spawn state (F4Connections): his current
/// body, class, post and spawn screen phase.
pub fn view(w: &mut World, p: &Player) -> Option<F4ConnectionView> {
    let handle = p.character.as_ref()?;
    crate::battle::f4_connections::wire_view_in(&mut w.battle, &w.sim.store, handle).cloned()
}
/// `p.unit`.
pub fn body(w: &mut World, p: &Player) -> Option<Ref<Unit>> { view(w, p)?.unit }

// `u.alive` on the canonical row (the host's Unit mirror followed it).
fn alive(w: &World, unit: Ref<Unit>) -> bool { w.sim.store.unit.get(unit).is_some_and(|u| u.alive) }

/// connections.command(op, p, className, post): the connection's native
/// lifecycle (src/f4-connections.js), qualified by his Character.
fn connection(w: &mut World, p: &Player, op: F4ConnectionOp, class_name: Option<StrId>, post: Option<StrId>) {
    let Some(target) = p.character.clone() else { return };
    let command = F4ConnectionCommand { op, target, class_name, post, ordinal: p.ordinal, auto_respawn: p.auto_respawn };
    w.sim.calls.result(F4ConnectionCommand::WORDS, |row, heap| command.put(row, heap));
    crate::battle::f4_connections::f4_connection_command_in(w);
}

/// roster.release / roster.switchTeam (src/f4-roster.js command): true
/// when the roster accepted the change.
fn roster(w: &mut World, p: &Player, op: RoOperation, team: f64, empty: StrId) -> bool {
    let Some(target) = p.character.clone() else { return false };
    let command = F4RosterCommand { op, target: Some(target), team, player_id: PlayerId::Undefined, name: empty };
    w.sim.calls.result(F4RosterCommand::WORDS, |row, heap| command.put(row, heap));
    let Some(words) = crate::battle::f4_roster::f4_roster_command_in(w) else { return false };
    // The result block is read in place: its header names the row and heap.
    let stride = words[BlockHeader::STRIDE] as usize;
    let (heap_at, heap_words) = (words[BlockHeader::HEAP] as usize, words[BlockHeader::HEAP_WORDS] as usize);
    F4RosterResult::take(&words[HEADER..HEADER + stride], &words[heap_at..heap_at + heap_words]).flag
}

/// remoteWorld.retire(p.id): his recon context retires with his place; a
/// droid still flying keeps flying (src/f3-recon.js retire).
fn retire_recon(w: &mut World, key: u32) {
    let Some(context) = crate::units::f3::recon_context_by_key_in(w, key) else { return };
    let Some(c) = w.sim.store.recon_context.get(context) else { return };
    let binding = F3ReconBinding { action: F3ReconAction::Retire, key: c.key, owner: c.owner.clone(), seed: 0, droid: None, dx: 0.0, dy: 0.0, sensitivity: 1.0, flag: false };
    w.sim.calls.result(F3ReconBinding::WORDS, |row, heap| binding.put(row, heap));
    crate::recon::module::f3_recon_command_in(w);
}

/// His command this tick, read from the mailbox the host filled: the view
/// facts it carries (InputMailbox.take's `...this.values`), his mouse
/// samples, and the actions and spawn choices the controls and the choice
/// loop consume. False when the take itself was refused (an overflowing
/// burst), which the source treated as a fault of the tick.
fn take(w: &mut World, p: &mut Player, tick: f64, generation: f64, command: &mut Command, faults: &mut u32) -> bool {
    use crate::input::owner::view_flags as flags;
    let Some(mailbox) = w.input.mailboxes.get_mut(p.mailbox) else { return false };
    let ok = mailbox.take_command(tick, generation, &mut w.sim.strings);
    let epoch = mailbox.state.epoch;
    let view = &mailbox.publication.view;
    let bits = view.flags as u16;
    *command = Command {
        state_tick: mailbox.state.state_tick,
        mouse_scale: (bits & flags::MOUSE_SCALE != 0).then_some(view.mouse_scale),
        invert_mouse: bits & flags::INVERT_MOUSE != 0,
        first_person: mailbox.present(2).then_some(bits & flags::FIRST_PERSON != 0),
        view_tick: (bits & flags::VIEW_TICK != 0).then_some(view.view_tick),
        flight: bits & flags::FLIGHT != 0,
        sensitivity: (bits & flags::SENSITIVITY != 0).then_some(view.sensitivity),
        flight_invert: bits & flags::FLIGHT_INVERT != 0,
        curve: (bits & flags::CURVE != 0).then_some(view.curve),
        gamepad: (bits & flags::GAMEPAD != 0).then_some((view.axes, view.buttons as u32)),
    };
    let taken = mailbox.taken();
    p.looks.clear();
    p.looks.extend(taken.look.iter().map(|look| look.point));
    if p.separate_actions {
        // The reliable lane's presses and choices replace the mailbox's.
        p.actions_taken.clear();
        p.human_taken.clear();
        let _ = crate::input::actions::take_into_reusing(&mut p.actions, tick, generation, &mut p.actions_taken, &mut p.human_taken, &mut w.sim.strings, &mut p.action_entries);
        let mailbox = w.input.mailboxes.get(p.mailbox).unwrap();
        p.controls.accept(epoch, tick, generation, mailbox.down(), &p.actions_taken);
    } else {
        p.human_taken.clear();
        // The mailbox bounds a take to 16 choices a queued packet; the list
        // was reserved for that, so the tick path never grows it.
        let room = p.human_taken.capacity();
        p.human_taken.extend(taken.human.iter().take(room).cloned());
        if taken.human.len() > room { *faults = faults.wrapping_add(1); }
        p.controls.accept(epoch, tick, generation, mailbox.down(), &taken.actions);
    }
    ok
}

/// One tick of the players' phase; `input.units` rows precede the foreign
/// facts the host wrote for the hit history.
pub fn run(w: &mut World, input: &WorldTickInput) {
    let mut players = std::mem::take(&mut w.input.players);
    if !players.active { w.input.players = players; return; }
    let tick = input.tick;
    let at = input.at;
    let grace = v8math::round(DISCONNECT_GRACE_SECONDS * if input.clock.hz.is_nan() { 60.0 } else { input.clock.hz });
    // game.ai.worldData.time is the time of the tick before this one.
    let time = input.clock.tick / 60.0;
    let empty = players.names().empty;
    players.changed.clear();
    crate::units::f3::f3_history_capture_at_in(w, input.clock.tick as u32, input.units);
    let mut command = Command::default();
    let mut i = 0;
    while i < players.order.len() {
        let key = players.order[i];
        let Some(p) = players.pool.get_mut(key) else { players.order.remove(i); continue };
        if let Some(since) = p.disconnected_at {
            // His place is released at the end of his grace; a spectator held
            // none. The mailbox goes with him, and the host hears of it.
            if tick - since >= grace {
                if !p.spectator { roster(w, p, RoOperation::Release, 0.0, empty); }
                if let Some(recon) = p.recon_key { retire_recon(w, recon); }
                let mailbox = p.mailbox;
                w.input.mailboxes.remove(mailbox);
                players.changed.push(PlayerView { handle: key as u32, deleted: true, ..Default::default() });
                players.remove(key);
                continue;
            }
            i += 1;
            continue;
        }
        if p.spectator { i += 1; continue; }
        if !p.inputs_stale && at - p.last_input_at > STALE_INPUT_MS {
            p.inputs_stale = true;
            commands::stop(w, &mut players, key);
        }
        let p = players.pool.get_mut(key).unwrap();
        let Some(unit) = body(w, p) else { i += 1; continue };
        let generation = w.sim.store.unit.get(unit).map_or(0.0, |u| u.generation);
        if !take(w, p, tick, generation, &mut command, &mut players.faults) { players.faults = players.faults.wrapping_add(1); }
        // The choices judge the life the tick began with (`u.alive`), while a
        // spawn reads the current body: a class change replaces it mid-list.
        let was_alive = alive(w, unit);
        let human = std::mem::take(&mut p.human_taken);
        for choice in &human {
            match choice.kind {
                HsCommandKind::Suicide if was_alive => commands::suicide(w, unit),
                HsCommandKind::Team if !was_alive && p.team_select && (choice.team == 1.0 || choice.team == 2.0) => {
                    if roster(w, p, RoOperation::SwitchTeam, choice.team, empty) {
                        p.team = choice.team;
                        p.changed = true;
                        connection(w, p, F4ConnectionOp::Team, None, None);
                    }
                }
                HsCommandKind::Spawn => {
                    // A class change may replace the body; later choices
                    // see the new life (`!p.unit.alive` reads it fresh).
                    if !body(w, p).is_some_and(|current| alive(w, current)) {
                        p.changed = true;
                        connection(w, p, F4ConnectionOp::Spawn, choice.class_name, choice.post);
                    }
                }
                _ => {}
            }
        }
        p.human_taken = human;
        // Native removed-body clocks run before controls and the world.
        let Some(current) = body(w, p) else { i += 1; continue };
        if !alive(w, current) {
            p.changed = true;
            connection(w, p, F4ConnectionOp::State, None, None);
        }
        let Some(current) = body(w, p) else { i += 1; continue };
        if !alive(w, current) { i += 1; continue; }
        let looks = std::mem::take(&mut p.looks);
        commands::apply(w, &mut players, key, current, &command, &looks, tick, input.mouse_scale, time);
        if let Some(p) = players.pool.get_mut(key) { p.looks = looks; }
        i += 1;
    }
    super::views::cameras(w, &mut players);
    super::rows::write(w, &mut players);
    super::rows::flights(w, &mut players);
    views_block(w, &mut players, tick);
    w.input.players = players;
}

/// What the host mirrors after the tick: the players whose connection
/// changed, with their retired handles first.
fn views_block(w: &mut World, players: &mut Players, _tick: f64) {
    let ended = w.sim.store.battle.game_over;
    players.shots.clear();
    for &key in &players.order {
        let Some(p) = players.pool.get_mut(key) else { continue };
        if !p.changed && !p.applied && !p.stopped { continue; }
        let (applied, stopped) = (p.applied, p.stopped);
        p.changed = false;
        p.applied = false;
        p.stopped = false;
        let view = view(w, p);
        let (unit, class_name, phase, post, spawned_once, dead_time) = match &view {
            Some(v) => (v.unit, v.class_name, v.phase, v.post, v.spawned_once, v.dead_time),
            None => (None, None, HumanPhase::Unspawned, None, false, 0.0),
        };
        let shadow = unit.and_then(|u| players.shadows.get(u.slot as usize));
        let intent = if applied || stopped { shadow.and_then(|s| s.intent.clone()) } else { None };
        if applied || stopped {
            let shots = shadow.and_then(|s| s.network).map_or([None, None], |n| n.shots);
            players.shots.push(PlayerShots { handle: key as u32, fire0: shots[0].map(|s| s.id), fire1: shots[1].map(|s| s.id) });
        }
        players.changed.push(PlayerView { handle: key as u32, deleted: false, unit, class_name, team: p.team, phase, post,
            spawned_once, dead_time, ended, disconnected: p.disconnected_at.is_some(), intent, stopped });
    }
}

/// players_shots: the presses the bodies fired this tick (Unit.networkShots).
pub fn shots_in(w: &mut World) -> Option<&[f64]> {
    let players = &mut w.input.players;
    let block = &mut players.shots_block;
    block.begin(1, PlayerShotList::WORDS);
    let list = PlayerShotList { players: std::mem::take(&mut players.shots) };
    {
        let (row, heap) = block.row(0);
        list.put(row, heap);
    }
    block.finish();
    players.shots = list.players;
    Some(&block.words[..])
}

/// players_views: the changed players since the host last read them.
pub fn views_in(w: &mut World) -> Option<&[f64]> {
    let players = &mut w.input.players;
    let block = &mut players.views;
    block.begin(1, PlayerViews::WORDS);
    // The retained list is lent to the record only while it is encoded.
    let views = PlayerViews { players: std::mem::take(&mut players.changed) };
    {
        let (row, heap) = block.row(0);
        views.put(row, heap);
    }
    block.finish();
    players.changed = views.players;
    players.changed.clear();
    Some(&block.words[..])
}
