//! Desktop input becomes the generated mailbox and reliable-lane records.
//! NetworkInputs' existing Outbound owner retains cumulative look and trigger
//! transitions; this adapter resolves strings once and packs its outputs.
use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientFlightAction, ClientFlightHeld, ClientLook};
use bfe_presentation::{network::{outbound::Outbound, Output, Words}, network_layout::{net_op as op, net_outbound as out}};
use bfe_sim::{abi::context::Context, gen::{IaEntry, IfEdge, ImEntry, ImField, InputMailboxCall, ImOperation, WiView}, store::StrId};
use bfe_transport::layout::INPUT_ACTIONS;

const HELD: [&str; 9] = ["KeyW", "KeyS", "KeyD", "KeyA", "NumpadMultiply", "NumpadDivide", "Mouse0", "Mouse2", "Space"];
// ClientHeld and ClientAction use desktop bit positions. InputActions uses
// wire action ids instead, so the cold name table resolves that difference;
// casting a desktop action bit to a protocol id would bind the wrong action.
const ACTIONS: [&str; 11] = ["jump", "crouch", "prone", "use", "firePrimary", "fireSecondary", "reload", "nextPrimary", "previousPrimary", "nextSecondary", "zoom"];
const FIELDS: [&str; 7] = ["mouseScale", "invertMouse", "firstPerson", "aspect", "viewTick", "flightSettings", "gamepad"];
// WiView's established presence bits keep false values separate from missing
// properties. InputMailbox consumes this existing protocol record directly.
const FIRST_PERSON: u16 = 2;
const FLIGHT: u16 = 4;
const FLIGHT_INVERT: u16 = 8;
const GAMEPAD: u16 = 16;
const MOUSE_SCALE: u16 = 32;
const ASPECT: u16 = 64;
const VIEW_TICK: u16 = 128;
const SENSITIVITY: u16 = 256;
const CURVE: u16 = 512;

pub struct Names {
    pub held: [StrId; 9],
    pub actions: [StrId; INPUT_ACTIONS.len()],
    pub fields: [StrId; 7],
    pub extra_held: [StrId; 2],
    pub outbound_actions: [u8; 11],
    pub flight_actions: [u8; 4],
}
impl Names {
    // InputMailbox's names must belong to the same Context as the player.
    // The returned ids remain stable for this battle; frames never ask a
    // string table to compare or intern a key, action or view property.
    pub fn new(context: &Context) -> Result<Self, String> {
        let intern = |name: &str| context.intern(&name.encode_utf16().collect::<Vec<_>>()).map_err(|e| e.to_string());
        fn names<const N: usize>(source: &[&str; N], mut intern: impl FnMut(&str) -> Result<StrId, String>) -> Result<[StrId; N], String> {
            let mut ids = [StrId(0); N];
            for (id, name) in ids.iter_mut().zip(source) { *id = intern(name)?; }
            Ok(ids)
        }
        let mut outbound_actions = [0; 11]; let mut flight_actions = [0; 4];
        for (bit, (out, name)) in outbound_actions.iter_mut().zip(ACTIONS).enumerate() {
            // Desktop fire slots have no InputActions name: capture routes
            // their counts and transitions through the shared fire owner.
            if bit == 4 || bit == 5 { continue; }
            *out = INPUT_ACTIONS.iter().position(|action| *action == name).ok_or("unknown desktop action")? as u8;
        }
        for (out, name) in flight_actions.iter_mut().zip(ClientFlightAction::NAMES) {
            *out = INPUT_ACTIONS.iter().position(|action| *action == name).ok_or("unknown flight action")? as u8;
        }
        Ok(Self { held: names(&HELD, intern)?, actions: names(&INPUT_ACTIONS, intern)?, fields: names(&FIELDS, intern)?,
            extra_held: names(&["ShiftLeft", "KeyZ"], intern)?, outbound_actions, flight_actions })
    }
}

pub struct Commands {
    owner: Outbound,
    names: Names,
    pub mailbox: InputMailboxCall,
    pub actions: IaEntry,
    pub fires: Vec<IfEdge>,
    fire_id: u32,
    pub refused: u64,
}
impl Commands {
    pub fn new(names: Names) -> Self {
        // One desktop update contributes one mailbox entry. Its held keys and
        // seven view fields fit these admitted vectors even in flight; it
        // never carries the mailbox's queue back into a separate host store.
        let entry = ImEntry { codes: Vec::with_capacity(16), look: Vec::with_capacity(1), view: Vec::with_capacity(7),
            actions: Vec::new(), human: Vec::new(), fire: Vec::new(), ..Default::default() };
        Self { owner: Outbound::default(), names, mailbox: InputMailboxCall { op: ImOperation::Push,
            entry: Some(entry), epoch_matches: true, ..Default::default() },
            actions: IaEntry { actions: Vec::with_capacity(32), ..Default::default() },
            fires: Vec::with_capacity(128), fire_id: 0, refused: 0 }
    }

    // Client::tick writes this same outbound command. Local authority already
    // knows its next tick, so it does not run a network lead estimator or keep
    // retransmission metadata. Edges still follow the one shared input owner.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(&mut self, command: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand,
        epoch: f64, generation: f64, tick: f64, first_person: bool, aspect: f64, view_tick: Option<f64>, flight_mode: bool) -> Result<(), &'static str> {
        // Match Sender::tick's bounded command arena. The longest batch is
        // eleven held names, thirteen semantic actions and sixty-four wheel
        // presses, which fits before any write without growing a buffer.
        let mut words = [0.; 512]; let mut at = 0;
        let put = |words: &mut [f64], at: &mut usize, value: f64| { words[*at] = value; *at += 1; };
        for value in [epoch, generation, command.sequence, tick] { put(&mut words, &mut at, value); }
        let length_at = at; at += 1; let begin = at;
        for (bit, name) in HELD.iter().enumerate() {
            if command.held & (1 << bit) == 0 { continue; }
            put(&mut words, &mut at, name.len() as f64);
            for byte in name.bytes() { put(&mut words, &mut at, byte as f64); }
        }
        if flight_mode {
            for (flag, name) in [(ClientFlightHeld::Boost, "ShiftLeft"), (ClientFlightHeld::RollLock, "KeyZ")] {
                if flight.held & (1 << flag as u8) == 0 { continue; }
                put(&mut words, &mut at, name.len() as f64);
                for byte in name.bytes() { put(&mut words, &mut at, byte as f64); }
            }
        }
        words[length_at] = (at - begin) as f64;
        for value in [1., look.x, look.y] { put(&mut words, &mut at, value); }
        let length_at = at; at += 1; let begin = at;
        for bit in 0..ACTIONS.len() {
            // Fire press/release edges have their own reliable lane. Packing
            // those bits as ordinary actions would apply one trigger twice.
            if command.actions & (1 << bit) == 0 || bit == 4 || bit == 5 { continue; }
            put(&mut words, &mut at, self.names.outbound_actions[bit] as f64);
        }
        // Controls binds flight presses only in a pilot context. Admitting a
        // takeoff while on foot would leave an unconsumed action queued until
        // a later boarding, so the native host preserves that modal boundary.
        if flight_mode { for bit in 0..ClientFlightAction::NAMES.len() {
            if flight.actions & (1 << bit) != 0 { put(&mut words, &mut at, self.names.flight_actions[bit] as f64); }
        } }
        for (code, count) in [(21., command.primary_count.min(32)), (22., command.secondary_count.min(32))] {
            // NetworkInputs caps coalesced wheel events at thirty-two per
            // direction. Preserve that same pause/burst degradation locally.
            for _ in 0..count { put(&mut words, &mut at, code); }
        }
        words[length_at] = (at - begin) as f64;
        let mut values = [0.; out::STRIDE];
        self.owner.run(op::OUTBOUND_COMMAND, &mut Words::new(&words[..at]), &mut Output { values: &mut values, at: 0 })?;

        let entry = self.mailbox.entry.as_mut().ok_or("local mailbox entry missing")?;
        entry.identity = command.sequence; entry.sequence = command.sequence;
        entry.tick = tick; entry.generation = generation; entry.down = command.held as f64;
        entry.codes.clear();
        for (bit, id) in self.names.held.iter().enumerate() { if command.held & (1 << bit) != 0 { entry.codes.push(*id); } }
        if flight_mode { for (index, flag) in [ClientFlightHeld::Boost, ClientFlightHeld::RollLock].iter().enumerate() {
            if flight.held & (1 << *flag as u8) != 0 { entry.codes.push(self.names.extra_held[index]); }
        } }
        // Outbound owns the cumulative pointer total across commands. The
        // mailbox differences it when consuming commands, so neither display
        // interpolation nor multiple ticks per draw can apply look twice.
        entry.look_total = Some([values[out::LOOK_X], values[out::LOOK_Y]]);
        let mut flags = MOUSE_SCALE | ASPECT | FLIGHT | SENSITIVITY | CURVE;
        if first_person { flags |= FIRST_PERSON; }
        if flight.invert { flags |= FLIGHT_INVERT; }
        if view_tick.is_some() { flags |= VIEW_TICK; }
        if flight.pad_buttons != 0 || flight.pad_axes.iter().any(|v| *v != 0.) { flags |= GAMEPAD; }
        entry.publication = WiView { flags: flags as f64, profile_found: true, mouse_scale: command.mouse_scale,
            aspect, view_tick: view_tick.unwrap_or(0.), sensitivity: flight.scale, curve: flight.curve,
            axes: flight.pad_axes, buttons: flight.pad_buttons as f64, ..Default::default() };
        entry.view.clear();
        // Include false and zero values too: removing the last gamepad axis
        // or leaving flight must clear yesterday's input publication fields.
        for &key in &self.names.fields { entry.view.push(ImField { key, value: 0. }); }
        self.mailbox.sequence = command.sequence; self.mailbox.tick = tick; self.mailbox.generation = generation;

        self.actions.id = command.sequence; self.actions.tick = tick; self.actions.generation = generation;
        self.actions.actions.clear();
        // The shared owner consumes the whole command before this adapter
        // bounds the wire-shaped output. Excess actions are counted and
        // discarded, with no deferred allocation or unbounded retry queue.
        self.refused = self.refused.saturating_add(self.owner.actions().len().saturating_sub(32) as u64);
        for &action in self.owner.actions().iter().take(32) {
            self.actions.actions.push(*self.names.actions.get(action as usize).ok_or("invalid native action code")?);
        }
        self.fires.clear();
        let mut used = 0;
        for fire in self.owner.fires() {
            // Held publications describe the state already in ImEntry.codes;
            // only transitions need their own reliable fire identity.
            if fire.held { continue; }
            if used == 128 { self.refused = self.refused.saturating_add(1); continue; }
            // Zero is the protocol's absent edge. Wrapping preserves a live
            // nonzero identity without overflowing a long-running session.
            self.fire_id = self.fire_id.wrapping_add(1).max(1);
            self.fires.push(IfEdge { id: self.fire_id as f64, tick, generation, button: fire.button as f64,
                pressed: fire.pressed, view_tick, ..Default::default() });
            used += 1;
        }
        Ok(())
    }
}
