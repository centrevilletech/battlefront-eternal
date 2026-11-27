//! Desktop input becomes the generated mailbox and reliable-lane records.
//! NetworkInputs' existing Outbound owner retains cumulative look and trigger
//! transitions; this adapter resolves strings once and packs its outputs.
use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientFlightAction, ClientFlightHeld, ClientLook};
use bfe_presentation::{network::{outbound::Outbound, Output, Words}, network_layout::{net_op as op, net_outbound as out}};
use bfe_sim::{abi::context::Context, gen::{IaEntry, IfEdge, ImEntry, ImField, InputMailboxCall, ImOperation, WiView}, store::StrId};
use bfe_client::input_family::WireInputs;

const HELD: [&str; 9] = ["KeyW", "KeyS", "KeyD", "KeyA", "NumpadMultiply", "NumpadDivide", "Mouse0", "Mouse2", "Space"];
// ClientHeld and ClientAction use desktop bit positions. InputActions uses
// wire action ids instead, so the cold name table resolves that difference;
// casting a desktop action bit to a protocol id would bind the wrong action.
// ClientCommand.actions bit order (ClientAction).
const ACTIONS: [&str; 16] = bfe_client::generated_input::ClientAction::NAMES;
// ClientAction's roll bit: never passed through the shared adapter.
const ROLL: usize = bfe_client::generated_input::ClientAction::Roll as usize;
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
    /// The battle's wire actions, by wire action ID.
    pub actions: Vec<StrId>,
    pub fields: [StrId; 7],
    pub extra_held: [StrId; 2],
    /// The battle's on-foot holds beyond the shared nine, by ClientHeld bit.
    pub foot_held: Vec<(u32, StrId)>,
    /// Wire action IDs by ClientAction bit; absent for the trigger slots and
    /// for actions the battle's game does not have.
    pub outbound_actions: [Option<u8>; ACTIONS.len()],
    pub flight_actions: [u8; 4],
    pub inputs: WireInputs,
    /// Block's press and release actions, for a battle that has block.
    pub block_actions: Option<(StrId, StrId)>,
}
impl Names {
    // InputMailbox's names must belong to the same Context as the player.
    // The returned ids remain stable for this battle; frames never ask a
    // string table to compare or intern a key, action or view property.
    pub fn new(context: &Context, inputs: WireInputs) -> Result<Self, String> {
        let intern = |name: &str| context.intern(&name.encode_utf16().collect::<Vec<_>>()).map_err(|e| e.to_string());
        fn names<const N: usize>(source: &[&str; N], mut intern: impl FnMut(&str) -> Result<StrId, String>) -> Result<[StrId; N], String> {
            let mut ids = [StrId(0); N];
            for (id, name) in ids.iter_mut().zip(source) { *id = intern(name)?; }
            Ok(ids)
        }
        let mut outbound_actions = [None; ACTIONS.len()]; let mut flight_actions = [0; 4];
        let released = inputs.action_bits(true);
        for (bit, (out, name)) in outbound_actions.iter_mut().zip(ACTIONS).enumerate() {
            // Desktop fire slots have no InputActions name: capture routes
            // their counts and transitions through the shared fire owner.
            if bit == 4 || bit == 5 || released & (1 << bit) == 0 { continue; }
            *out = Some(inputs.action_id(name).ok_or("unknown desktop action")? as u8);
        }
        for (out, name) in flight_actions.iter_mut().zip(ClientFlightAction::NAMES) {
            *out = inputs.action_id(name).ok_or("unknown flight action")? as u8;
        }
        let actions = inputs.actions().iter().map(|name| intern(name)).collect::<Result<Vec<_>, _>>()?;
        let foot_held = inputs.foot_holds().iter().map(|&(bit, code)| Ok((bit, intern(code)?))).collect::<Result<Vec<_>, String>>()?;
        let block_actions = inputs.block_actions().map(|(press, release)| (actions[press], actions[release]));
        Ok(Self { held: names(&HELD, intern)?, actions, fields: names(&FIELDS, intern)?,
            extra_held: names(&["ShiftLeft", "KeyZ"], intern)?, foot_held, outbound_actions, flight_actions, inputs, block_actions })
    }
}

pub struct Commands {
    owner: Box<Outbound>,
    names: Names,
    pub mailbox: InputMailboxCall,
    pub actions: IaEntry,
    pub fires: Vec<IfEdge>,
    fire_id: u32,
    pub refused: u64,
    // Whether the last command held block on foot (a 2005 battle only), and
    // the epoch and life it held it in: each life starts released.
    block_down: bool,
    block_life: [f64; 2],
}
impl Commands {
    pub fn new(names: Names) -> Self {
        // One desktop update contributes one mailbox entry. Its held keys and
        // seven view fields fit these admitted vectors even in flight; it
        // never carries the mailbox's queue back into a separate host store.
        let entry = ImEntry { codes: Vec::with_capacity(16), look: Vec::with_capacity(1), view: Vec::with_capacity(7),
            actions: Vec::new(), human: Vec::new(), fire: Vec::new(), ..Default::default() };
        Self { owner: Box::new(Outbound::default()), names, mailbox: InputMailboxCall { op: ImOperation::Push,
            entry: Some(entry), epoch_matches: true, ..Default::default() },
            actions: IaEntry { actions: Vec::with_capacity(32), ..Default::default() },
            fires: Vec::with_capacity(128), fire_id: 0, refused: 0, block_down: false, block_life: [0.; 2] }
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
        } else {
            // A 2005 soldier's sprint and block holds; a 2004 battle has none.
            for &(bit, code) in self.names.inputs.foot_holds() {
                if command.held & (1 << bit) == 0 { continue; }
                put(&mut words, &mut at, code.len() as f64);
                for byte in code.bytes() { put(&mut words, &mut at, byte as f64); }
            }
        }
        words[length_at] = (at - begin) as f64;
        for value in [1., look.x, look.y] { put(&mut words, &mut at, value); }
        let length_at = at; at += 1; let begin = at;
        let allowed = self.names.inputs.action_bits(!flight_mode);
        // The shared numeric adapter takes the released actions only: a 2005
        // soldier's roll joins the reliable actions after it (below).
        for bit in 0..ROLL {
            // Fire press/release edges have their own reliable lane. Packing
            // those bits as ordinary actions would apply one trigger twice.
            if command.actions & allowed & (1 << bit) == 0 || bit == 4 || bit == 5 { continue; }
            if let Some(action) = self.names.outbound_actions[bit] { put(&mut words, &mut at, action as f64); }
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
        } } else {
            for &(bit, id) in &self.names.foot_held { if command.held & (1 << bit) != 0 { entry.codes.push(id); } }
        }
        // Outbound owns the cumulative pointer total across commands. The
        // mailbox differences it when consuming commands, so neither display
        // interpolation nor multiple ticks per draw can apply look twice.
        entry.look_total = Some([values[out::LOOK_X], values[out::LOOK_Y]]);
        let mut flags = MOUSE_SCALE | ASPECT | FLIGHT | SENSITIVITY | CURVE;
        if first_person { flags |= FIRST_PERSON; }
        if command.invert == Some(true) { flags |= bfe_sim::input::owner::view_flags::INVERT_MOUSE; }
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
        if command.actions & allowed & (1 << ROLL) != 0 {
            if let Some(action) = self.names.outbound_actions[ROLL] {
                if self.actions.actions.len() < 32 {
                    self.actions.actions.push(*self.names.actions.get(action as usize).ok_or("invalid native action code")?);
                } else { self.refused = self.refused.saturating_add(1); }
            }
        }
        // Block's press or release, when the held control changed on foot. A
        // new life starts released, so a control still held is pressed again.
        if self.block_life != [epoch, generation] {
            self.block_life = [epoch, generation];
            self.block_down = false;
        }
        if let Some((press, release)) = self.names.block_actions {
            let block = !flight_mode && command.held & (1 << bfe_client::generated_input::ClientHeld::Block as u32) != 0;
            if block != self.block_down {
                if self.actions.actions.len() < 32 {
                    self.block_down = block;
                    self.actions.actions.push(if block { press } else { release });
                } else { self.refused = self.refused.saturating_add(1); }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_client::generated_input::ClientHeld;

    // A block still held when a new life starts is pressed again in it.
    #[test]
    fn a_block_held_into_a_new_life_is_pressed_again() {
        let context = Context::new();
        let names = Names::new(&context, WireInputs::new(bfe_family::GameFamily::Swbf2)).unwrap();
        let (press, release) = names.block_actions.unwrap();
        let mut commands = Commands::new(names);
        let mut sent = |held: u32, sequence: f64, generation: f64| {
            let command = ClientCommand { held, sequence, ..Default::default() };
            commands.capture(&command, &ClientLook::default(), &ClientFlightCommand::default(), 1., generation, sequence, false, 1., None,
                false).unwrap();
            commands.actions.actions.clone()
        };
        let block = 1 << ClientHeld::Block as u32;
        assert_eq!(sent(block, 1., 1.), vec![press]);
        assert_eq!(sent(block, 2., 1.), vec![]);
        assert_eq!(sent(block, 3., 2.), vec![press]);
        assert_eq!(sent(0, 4., 2.), vec![release]);
    }
}
