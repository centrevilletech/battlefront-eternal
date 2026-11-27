//! NetworkInputs accounting and wire-input encoding stay in their Rust owners.
//! This adapter supplies generated device records and transport receipts; it
//! keeps only the bounded command/edge metadata needed to retry publication.
use super::receiver::Receiver;
use crate::generated_input::{ClientCommand, ClientFlightAction, ClientFlightCommand, ClientFlightHeld, ClientLook};
use bfe_presentation::{
    network::{input::{encode, encode_swbf2}, outbound::Outbound, Output, Words},
    network_layout::{net_edge as e, net_header as h, net_op as op, net_outbound as out, net_view as v},
};
use bfe_sim::{
    gen::{Air, Posture, Soldier},
    movement::soldier::swbf2::TumblePhase,
    prediction::reconcile::{target_lead, LeadTargetInput},
};
use crate::input_family::{SoldierInput, WireInputs};
use bfe_transport::{connection::Lane, endpoint::Endpoint, layout::INPUT_CODES};
use serde::Serialize;
#[derive(Clone, Copy, Default)]
struct State {
    sequence: f64,
    tick: f64,
    held: u32,
    look: [f64; 2],
    revision: f64,
    pilot: Option<bfe_sim::space::local::PilotControls>,
}
/// Per-class look constants used by [`Sender::preview`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassTurn {
    pub max_turn: f64,
    pub pitch_limits: [f64; 2],
    /// Present for 2005 soldier classes (`ControlSpeed` turn column).
    pub swbf2: Option<bfe_sim::movement::soldier::swbf2::TurnScales>,
}
impl ClassTurn {
    pub const fn classic(max_turn: f64, pitch_limits: [f64; 2]) -> Self {
        Self { max_turn, pitch_limits, swbf2: None }
    }
}
#[derive(Clone, Copy, Default)]
struct Pointer {
    sequence: f64,
    x: f64,
    y: f64,
    zoom: f64,
    scale: f64,
    invert: bool,
    turn_scale: Option<f64>,
}
pub struct Sender {
    pub aim_scene: Option<super::shot_aim::Shared>,
    pub fire_aim: Option<[f64;2]>,
    // BFE_HUD_TRACE: the unit the camera ray last held, logged when it changes.
    aim_target: Option<u32>,
    trace: bool,
    pub history: Box<super::replay::History>,
    owner: Box<Outbound>,
    life: [f64; 2],
    sequence: u32,
    edge_id: u32,
    edges: [[f64; e::STRIDE]; 32],
    edge_count: usize,
    latest: Option<State>,
    backup: Option<State>,
    use_backup: bool,
    held: [usize; KEYS.len()],
    // Wire action IDs by ClientAction bit, absent for the trigger slots and
    // for the actions this battle's game does not have.
    actions: [Option<usize>; ACTIONS.len()],
    flight_actions: [usize; ClientFlightAction::NAMES.len()],
    inputs: WireInputs,
    // Whether the last command held block on foot (a 2005 battle only).
    block_down: bool,
    /// What a 2005 soldier holds and taps beyond his next command.
    pub soldier: SoldierInput,
    /// Whether the player is holding Use (`E`), forwarded in Space `PILOT` controls.
    pub use_held: bool,
    // Reused scratch buffers for `PILOT` rev 1 section encoding (zero per-packet alloc).
    pilot_buf: Vec<u8>,
    records_buf: Vec<u8>,
    prev_space_state: Option<State>,
    last_sent_space_seq: f64,
    // The newest command's flight settings and pad, sent with its view.
    flight: Option<ClientFlightCommand>,
    pointers: [Pointer; 128],
    pointer_at: usize,
    pointer_count: usize,
    mouse_scale: f64,
    aspect: f64,
    invert: bool,
    first_person: bool,
    pub yaw: f64,
    pub pitch: f64,
    pub sent: u64,
    pub refused: u64,
    pub fire_edges: u64,
    pub spawned: bool,
    pub auto_spawn: bool,
    // A team, class and post picked before the server's first correction,
    // kept until the spawn can be sent (else the automatic spawn would ask
    // for the server's default class, on the team the server chose).
    pick: Option<(u8, usize, usize)>,
    last_tick: f64,
    ahead: bool,
    measured: bool,
    last_ack: f64,
    turns: Vec<ClassTurn>,
    swbf2_models: Vec<Option<bfe_sim::units::infantry::TurnModel>>,
    /// What the online replay's restored 2005 state says about the soldier's look rate, as of the replay's
    /// last frame (see [`Sender::note_replayed_state`]). None in 2004 battles and in Instant Action.
    replayed: Option<ReplayedLook>,
    // Instant Action's lead: one tick above the local world's next tick,
    // raised a tick when the world takes a command late.
    local: bool,
    extra: f64,
    cooldown: u32,
    calm: u32,
    late_seen: u64,
    local_next: Option<f64>,
    // Online: the tick each recent command was stamped for (by sequence), to read how late
    // the server ran it from its input-ack.
    stamps: Box<[(u32, f64); 256]>,
    acked: f64,
    acked_server_tick: f64,
    // Online lead feedback (the client's InputLead with recover): acknowledgements still to
    // pass before lateness may raise the margin again, and on-time ticks in a row.
    lead_cooldown: f64,
    lead_calm: f64,
    /// Commands the server ran after the tick they were stamped for, and by how many ticks in all.
    pub late: u64,
    pub late_ticks: f64,
    // The previewed body's turn per mouse count, and every count taken so far (scaled as it was applied).
    per_count: f64,
    counted: f64,
    walker: WalkerTurn,
    infantry_turn: InfantryTurn,
    infantry_velocity: CorrectedVelocity,
    infantry_pending: [PendingInfantry; 128],
    infantry_at: u32,
    infantry_count: u32,
    infantry_acked_mouse_yaw: f64,
    infantry_mouse_yaw: f64,
    infantry_predicted_mouse: bool,
    last_acked_infantry: HeldInfantry,
}
/// The yaw of a droideka the player controls arrives with the server's corrections (its body is not
/// predicted here). What a correction turns it by beyond the mouse's own motion (a turn made with the
/// keys) is spread over the time to the next correction instead of reaching the view in one step.
#[derive(Clone, Copy, Default)]
struct WalkerTurn {
    life: Option<[f64; 3]>,
    // The previewed yaw, the counts and the correction tick of the frame before.
    yaw: f64,
    counted: f64,
    tick: f64,
    // The turn not shown yet, and how fast it is being shown.
    left: f64,
    rate: f64,
}
/// Per-life yaw easing state for local 2005 infantry bodies.
#[derive(Clone, Copy, Default)]
struct InfantryTurn {
    life: Option<[f64; 3]>,
    yaw: f64,
    counted: f64,
    mouse_yaw: f64,
    tick: f64,
    left: f64,
    rate: f64,
}
/// The look-rate facts of the online replay's restored 2005 state.
#[derive(Clone, Copy)]
struct ReplayedLook {
    sprinting: bool,
    rolling: bool,
    tumbling: bool,
}

/// The local 2005 body's horizontal velocity at its last correction, and its
/// change per tick since the correction before (for the turn predictor).
#[derive(Clone, Copy, Default)]
struct CorrectedVelocity {
    life: Option<[f64; 3]>,
    tick: f64,
    velocity: [f64; 2],
    change: [f64; 2],
}
/// Unacknowledged 2005 infantry command retained for per-command turn-factor prediction.
#[derive(Clone, Copy, Default)]
struct PendingInfantry {
    sequence: f64,
    tick: f64,
    forward: f64,
    right: f64,
    jump: bool,
    crouch: bool,
    controls: bfe_sim::units::infantry::Controls,
    x: f64,
    y: f64,
    zoom: f64,
    scale: f64,
    invert: bool,
    mouse_yaw: f64,
}
/// Last acknowledged held inputs used when stepping an idle gap before a stamped command.
#[derive(Clone, Copy, Default)]
struct HeldInfantry {
    forward: f64,
    right: f64,
    controls: bfe_sim::units::infantry::Controls,
}
fn held_only(controls: bfe_sim::units::infantry::Controls) -> bfe_sim::units::infantry::Controls {
    bfe_sim::units::infantry::Controls {
        sprint: controls.sprint,
        block: controls.block,
        hold_to_block: controls.hold_to_block,
        melee_held: controls.melee_held,
        ..Default::default()
    }
}
fn wrap_delta(delta: f64) -> f64 {
    delta.sin().atan2(delta.cos())
}
/// The furthest ahead of the server an online command may be stamped (the client's 24 ticks).
const ONLINE_MAX_LEAD: f64 = 24.;
// ClientCommand.held bit order, then a pilot's boost and roll lock (the
// world's own keys for them), which only a craft's command carries. A 2005
// soldier's sprint on foot uses the boost code; his own levels
// (SOLDIER_CODES) follow.
const KEYS: [&str; 14] = ["KeyW", "KeyS", "KeyD", "KeyA", "NumpadMultiply", "NumpadDivide", "Mouse0", "Mouse2", "Space",
    "ShiftLeft", "KeyZ", "KeyC", "KeyR", "KeyB"];
const FLIGHT_KEYS: [(ClientFlightHeld, u32); 2] = [(ClientFlightHeld::Boost, 9), (ClientFlightHeld::RollLock, 10)];
// ClientCommand.actions bit order (ClientAction).
const ACTIONS: [&str; 16] = crate::generated_input::ClientAction::NAMES;
// ClientAction's roll bit: never passed through the shared adapter.
const ROLL: usize = crate::generated_input::ClientAction::Roll as usize;
// Sender's held bit for each wire code a 2005 hold on foot uses.
fn foot_bit(code: &str) -> u32 { KEYS.iter().position(|k| *k == code).expect("sender key") as u32 }
fn put(x: &mut [f64], at: &mut usize, n: f64) {
    x[*at] = n;
    *at += 1;
}
// Generated physical codes name the wire bitmap, while ClientCommand uses
// the desktop held/action schema. Resolve that correspondence once at birth.
impl Default for Sender {
    fn default() -> Self { Self::for_inputs(WireInputs::default()) }
}
impl Sender {
    /// A sender for a battle of the given game: its wire tables never name an
    /// input the battle's game does not have.
    pub fn for_inputs(inputs: WireInputs) -> Self {
        let codes = WireInputs::new(bfe_family::GameFamily::Swbf2).codes();
        let held = KEYS.map(|name| codes.iter().position(|n| *n == name).expect("generated input code"));
        let actions = ACTIONS.map(|name| inputs.action_id(name));
        let flight_actions =
            ClientFlightAction::NAMES.map(|name| inputs.action_id(name).expect("generated flight action"));
        Self {
            aim_scene: None,
            fire_aim: None,
            aim_target: None,
            trace: std::env::var_os("BFE_HUD_TRACE").is_some(),
            history: Box::new(Default::default()),
            owner: Box::new(Outbound::default()),
            life: [0.; 2],
            sequence: 0,
            edge_id: 0,
            edges: [[0.; e::STRIDE]; 32],
            edge_count: 0,
            latest: None,
            backup: None,
            use_backup: false,
            held,
            actions,
            flight_actions,
            inputs,
            block_down: false,
            soldier: SoldierInput::default(),
            use_held: false,
            pilot_buf: Vec::with_capacity(64),
            records_buf: Vec::with_capacity(96),
            prev_space_state: None,
            last_sent_space_seq: 0.,
            flight: None,
            pointers: [Pointer::default(); 128],
            pointer_at: 0,
            pointer_count: 0,
            mouse_scale: 1.,
            aspect: 1280. / 720.,
            invert: false,
            first_person: false,
            yaw: 0.,
            pitch: 0.,
            sent: 0,
            refused: 0,
            fire_edges: 0,
            spawned: false,
            pick: None,
            auto_spawn: true,
            last_tick: 0.,
            ahead: false,
            measured: false,
            last_ack: f64::NEG_INFINITY,
            local: false,
            extra: 0.,
            cooldown: 0,
            calm: 0,
            late_seen: 0,
            stamps: Box::new([(0, 0.); 256]),
            acked: 0.,
            acked_server_tick: 0.,
            lead_cooldown: 0.,
            lead_calm: 0.,
            late: 0,
            late_ticks: 0.,
            per_count: 0.,
            counted: 0.,
            walker: WalkerTurn::default(),
            infantry_turn: InfantryTurn::default(),
            infantry_velocity: CorrectedVelocity::default(),
            infantry_pending: [PendingInfantry::default(); 128],
            infantry_at: 0,
            infantry_count: 0,
            infantry_acked_mouse_yaw: 0.,
            infantry_mouse_yaw: 0.,
            infantry_predicted_mouse: false,
            last_acked_infantry: HeldInfantry::default(),
            local_next: None,
            turns: Vec::new(),
            swbf2_models: Vec::new(),
            replayed: None,
        }
    }
}
impl Sender {
    pub fn command_sequence(&self) -> f64 {
        self.sequence as f64
    }
    /// How far ahead of the newest world tick heard the newest command is stamped.
    pub fn lead(&self, receiver: &Receiver) -> f64 {
        self.last_tick - receiver.latest
    }
    /// The world tick the newest command is stamped for.
    pub fn command_tick(&self) -> f64 {
        self.last_tick
    }
    /// The rate the 60 Hz command clock should run at. Stamps never go back,
    /// so after the world has fallen behind them (a stall, a slow machine)
    /// the clock runs a tenth slow until they are within a tick of where
    /// they need to be. Otherwise every later command would reach the world
    /// that much early and wait, and every frame would replay that many ticks.
    pub fn pace(&self) -> f64 {
        if self.ahead { 0.9 } else { 1. }
    }
    /// A local world's count of commands it took after their tick. As the
    /// page's InputLead for its own world: a late command raises the extra
    /// margin a tick (once per 30 commands, those in flight were stamped
    /// before), 600 on-time commands in a row lower it a tick.
    pub fn local_lateness(&mut self, late_total: u64) {
        self.local = true;
        if late_total > self.late_seen {
            self.calm = 0;
            if self.cooldown == 0 {
                self.extra = (self.extra + 1.).min(12.);
                self.cooldown = 30;
            }
        }
        self.late_seen = late_total;
    }
    /// A local world's next tick (the one it has not run yet). Its commands
    /// are stamped one tick above it: the world may be running that next
    /// tick while the command is on its way.
    pub fn local_next_tick(&mut self, next: f64) {
        self.local_next = Some(next);
    }
    /// The server ran an acknowledged command `late` ticks after the tick it was stamped for
    /// (`elapsed` server ticks since the previous acknowledgement). As the client's online
    /// InputLead: a late one raises the extra margin by its lateness, at most once per 30
    /// acknowledged ticks (commands in flight were stamped before the raise); after 60 on-time
    /// ticks in a row the margin sheds a tenth of a tick per tick, so a stall does not leave
    /// minutes of extra input latency behind it.
    fn online_lateness(&mut self, late: f64, elapsed: f64) {
        let elapsed = elapsed.clamp(1., 60.);
        self.lead_cooldown = (self.lead_cooldown - elapsed).max(0.);
        if late > 0. {
            self.late += 1;
            self.late_ticks += late;
            self.lead_calm = 0.;
            if self.lead_cooldown == 0. {
                self.extra = (self.extra + late).min(ONLINE_MAX_LEAD);
                self.lead_cooldown = 30.;
            }
        } else {
            self.lead_calm += elapsed;
            if self.lead_calm >= 60. {
                self.lead_calm = 60.;
                self.extra = (self.extra - 0.1 * elapsed).max(0.);
            }
        }
    }
    /// The extra margin a local world's lateness has added, in ticks.
    pub fn extra_lead(&self) -> f64 {
        self.extra
    }
    pub fn new(turns: &[ClassTurn]) -> Self {
        Self { turns: turns.to_vec(), ..Self::default() }
    }
    /// Install per-class 2005 movement tuning and combo state machines for
    /// exact per-pending-command look-rate prediction. Empty in 2004 battles.
    pub fn set_swbf2_models(&mut self, models: Vec<Option<bfe_sim::units::infantry::TurnModel>>) {
        self.swbf2_models = models;
    }
    /// The online replay's restored 2005 movement state after its frame: the sender's look rate for the
    /// next pointer events reads it (sprint, roll, and thrown, tumbling or sliding) instead of the
    /// posture alone. `None` when there is no replayed 2005 body.
    pub fn note_replayed_state(&mut self, state: Option<&bfe_sim::movement::soldier::swbf2::State>) {
        self.replayed = state.map(|s| ReplayedLook {
            sprinting: s.sprint_time.is_some(),
            rolling: s.roll.is_some(),
            tumbling: s.is_tumble_look(),
        });
    }
    /// The sender of a battle of `inputs`' game (a 2004 battle by default).
    pub fn with_inputs(turns: &[ClassTurn], inputs: WireInputs) -> Self {
        Self { turns: turns.to_vec(), ..Self::for_inputs(inputs) }
    }
    pub fn inputs(&self) -> WireInputs { self.inputs }
    /// Puts another game's inputs on the wire from the next command on.
    pub fn set_inputs(&mut self, inputs: WireInputs) {
        self.actions = ACTIONS.map(|name| inputs.action_id(name));
        self.flight_actions = ClientFlightAction::NAMES.map(|name| inputs.action_id(name).expect("generated flight action"));
        self.inputs = inputs;
    }
    // EntityEdgeSender bounds outstanding retries. Preserve the newest tail
    // when a paused input burst exceeds the window, without growing a map.
    fn edge(&mut self, kind: f64, tick: f64) -> &mut [f64; e::STRIDE] {
        if self.edge_count == 32 {
            self.edges.copy_within(1..32, 0);
            self.edge_count -= 1;
            self.refused += 1;
        }
        self.edge_id = self.edge_id.wrapping_add(1).max(1);
        let at = self.edge_count;
        self.edge_count += 1;
        let row = &mut self.edges[at];
        row.fill(0.);
        row[e::LIFE_MATCHES] = 1.;
        row[e::ID] = self.edge_id as f64;
        row[e::TICK] = tick;
        row[e::KIND] = kind;
        row
    }
    // Receipt retires a retry, whereas an applied correction retires pointer
    // history. Those clocks must stay distinct during loss and server backlog.
    pub fn feedback(&mut self, receiver: &mut Receiver) {
        let life = [receiver.epoch, receiver.own.generation];
        if self.life != life {
            self.history.clear();
            self.life = life;
            self.edge_count = 0;
            self.edge_id = 0;
            // Each life starts with block released: a control still held
            // sends its press again in the new life.
            self.block_down = false;
            self.latest = None;
            self.backup = None;
            self.prev_space_state = None;
            self.last_sent_space_seq = 0.;
            self.pointer_count = 0;
            self.pointer_at = 0;
            self.infantry_count = 0;
            self.infantry_at = 0;
            self.infantry_acked_mouse_yaw = 0.;
            self.infantry_mouse_yaw = 0.;
            self.infantry_predicted_mouse = false;
            self.last_acked_infantry = HeldInfantry::default();
            self.last_tick = 0.;
            self.ahead = false;
        }
        let mut n = 0;
        for at in 0..self.edge_count {
            let row = self.edges[at];
            if !receiver.received.values[..receiver.received.count].contains(&(row[e::ID] as u32)) {
                self.edges[n] = row;
                n += 1;
            }
        }
        self.edge_count = n;
        self.measured = false;
        receiver.received.count = 0;
        match self.inputs.family {
            bfe_family::GameFamily::Swbf1 => self.released_lateness(receiver),
            bfe_family::GameFamily::Swbf2 => self.swbf2_lateness(receiver),
        }
        // The infantry commands the authority has applied, whatever the battle's
        // wire: a local world's soldier commands ride the released one.
        while self.infantry_count != 0
            && self.infantry_pending[self.infantry_at as usize].sequence <= receiver.corrected_sequence
        {
            let acked = self.infantry_pending[self.infantry_at as usize];
            self.infantry_acked_mouse_yaw += acked.mouse_yaw;
            self.last_acked_infantry = HeldInfantry {
                forward: acked.forward,
                right: acked.right,
                controls: held_only(acked.controls),
            };
            self.infantry_at = (self.infantry_at + 1) % 128;
            self.infantry_count -= 1;
        }
        let mut ignored = [0.; 2];
        let _ = self.owner.run(op::OUTBOUND_ACK, &mut Words::new(&[receiver.ack]), &mut Output { values: &mut ignored, at: 0 });
        self.history.acknowledge(receiver.corrected_sequence);
        while self.pointer_count != 0 && self.pointers[self.pointer_at].sequence <= receiver.corrected_sequence {
            self.pointer_at = (self.pointer_at + 1) % 128;
            self.pointer_count -= 1;
        }
    }
    // A 2004 battle: the newest command acknowledged by a snapshot or an
    // input-ack, whichever arrived first, with the timing the last input-ack
    // that advanced it carried.
    fn released_lateness(&mut self, receiver: &Receiver) {
        if receiver.ack > self.acked {
            self.acked = receiver.ack;
            let (sequence, stamped) = self.stamps[receiver.ack as usize % 256];
            if !self.local && sequence as f64 == receiver.ack {
                let late = (receiver.ack_tick - stamped).max(0.);
                let elapsed = if self.acked_server_tick > 0. { receiver.ack_server_tick - self.acked_server_tick } else { 1. };
                self.acked_server_tick = receiver.ack_server_tick;
                self.online_lateness(late, elapsed);
            }
        }
    }
    // A 2005 battle: a snapshot advances `ack` without any timing, so the
    // lateness follows the newest command an input-ack itself carried.
    fn swbf2_lateness(&mut self, receiver: &Receiver) {
        if receiver.ack_measured > self.acked {
            self.acked = receiver.ack_measured;
            let (sequence, stamped) = self.stamps[receiver.ack_measured as usize % 256];
            if !self.local && sequence as f64 == receiver.ack_measured {
                let late = (receiver.ack_tick - stamped).max(0.);
                let elapsed = if self.acked_server_tick > 0. { receiver.ack_server_tick - self.acked_server_tick } else { 1. };
                self.acked_server_tick = receiver.ack_server_tick;
                self.online_lateness(late, elapsed);
            }
        }
    }
    // NetworkInputs receives one generated command per fixed client step.
    // No entity facade or locally authoritative simulation participates.
    pub fn tick(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, receiver: &Receiver, rtt: f64, now: f64) {
        self.tick_with_infantry(c, look, flight, None, receiver, rtt, now);
    }
    /// One command, with the infantry controls a local world's 2005 soldier
    /// takes: a battle on the wire in the 2005 game sends its own records
    /// (`swbf2_tick`); every other command is the released one, extended for
    /// the local world's soldier.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_with_infantry(
        &mut self,
        c: &ClientCommand,
        look: &ClientLook,
        flight: &ClientFlightCommand,
        infantry: Option<bfe_sim::units::infantry::Controls>,
        receiver: &Receiver,
        rtt: f64,
        now: f64,
    ) {
        match self.inputs.family {
            bfe_family::GameFamily::Swbf1 => self.released_tick(c, look, flight, infantry, receiver, rtt, now),
            bfe_family::GameFamily::Swbf2 => self.swbf2_tick(c, look, flight, receiver, rtt, now),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn released_tick(
        &mut self,
        c: &ClientCommand,
        look: &ClientLook,
        flight: &ClientFlightCommand,
        infantry: Option<bfe_sim::units::infantry::Controls>,
        receiver: &Receiver,
        rtt: f64,
        now: f64,
    ) {
        if !receiver.welcomed || receiver.spectator || receiver.corrections == 0 {
            return;
        }
        // applyLook and NetworkSession's view context use the same
        // mouse settings. Advertising a fixed scale made the server turn
        // twice as far as the default desktop prediction requested.
        self.mouse_scale = if c.has_mouse_scale && c.mouse_scale.is_finite() && c.mouse_scale > 0. { c.mouse_scale.min(2.) } else { 1. };
        self.aspect = if c.has_aspect && c.aspect.is_finite() && c.aspect > 0. { c.aspect } else { 1280. / 720. };
        self.invert = c.invert.unwrap_or(false);
        self.first_person = c.first_person.unwrap_or(false);
        self.sequence = self.sequence.wrapping_add(1).max(1);
        // A pilot's command carries the craft's own keys as the Instant Action
        // command does (local/commands.rs): its boost and roll lock held, its
        // take-off, trick, loop and target lock presses. On foot they stay off
        // the wire, so a press is never left queued for a later boarding.
        let flying = receiver.uses_flight_controls();
        let mut held = c.held & 0x1ff;
        if flying {
            for (flag, bit) in FLIGHT_KEYS {
                if flight.held & (1 << flag as u8) != 0 {
                    held |= 1 << bit;
                }
            }
        }
        self.flight = flying.then(|| flight.clone());
        // TargetLead uses the same transit estimate and future bound as the
        // page. Sequences remain distinct even when that bound holds a tick.
        if self.local {
            self.cooldown = self.cooldown.saturating_sub(1);
            self.calm += 1;
            if self.calm >= 600 {
                self.calm = 0;
                self.extra = (self.extra - 1.).max(0.);
            }
        }
        let target = if let (true, Some(next)) = (self.local, self.local_next) { next + 1. + self.extra } else { target_lead(LeadTargetInput {
            hz: 60.,
            margin: 2.,
            max_lead: ONLINE_MAX_LEAD,
            worst_gap: 0.,
            extra: self.extra,
            extrapolate: true,
            estimate: receiver.estimator.estimate(now / 1000.),
            latest: receiver.latest,
            round_trip_seconds: rtt / 1000.,
        })
        .bounded };
        let tick = target.floor().max(self.last_tick + 1.).min(receiver.latest + 30.);
        // Measured on a frame's first command: a slow frame's later ones are
        // stamped for the ticks after it and are not ahead of anything.
        if !self.measured {
            self.ahead = tick > target + 1.;
            self.measured = true;
        }
        self.last_tick = tick;
        self.stamps[self.sequence as usize % 256] = (self.sequence, tick);
        self.history.push(c, look, flight, tick, receiver.own.generation, self.sequence as f64);
        let mut x = [0.; 512];
        let mut at = 0;
        for n in [receiver.epoch, receiver.own.generation, self.sequence as f64, tick] {
            put(&mut x, &mut at, n);
        }
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        for bit in 0..KEYS.len() {
            if held & (1 << bit) != 0 {
                let name = KEYS[bit];
                put(&mut x, &mut at, name.len() as f64);
                for b in name.bytes() {
                    put(&mut x, &mut at, b as f64);
                }
            }
        }
        x[count] = (at - start) as f64;
        put(&mut x, &mut at, 1.);
        put(&mut x, &mut at, look.x);
        put(&mut x, &mut at, look.y);
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        for bit in 0..ROLL {
            if c.actions & (1 << bit) != 0 && bit != 4 && bit != 5 {
                if let Some(action) = self.actions[bit] {
                    put(&mut x, &mut at, action as f64);
                }
            }
        }
        if flying {
            for (bit, &action) in self.flight_actions.iter().enumerate() {
                if flight.actions & (1 << bit) != 0 {
                    put(&mut x, &mut at, action as f64);
                }
            }
        }
        for (code, count) in [(21, c.primary_count.min(32)), (22, c.secondary_count.min(32))] {
            for _ in 0..count {
                put(&mut x, &mut at, code as f64);
            }
        }
        x[count] = (at - start) as f64;
        let mut values = [0.; out::STRIDE];
        if self.owner.run(op::OUTBOUND_COMMAND, &mut Words::new(&x[..at]), &mut Output { values: &mut values, at: 0 }).is_err() {
            self.refused += 1;
            return;
        }
        // OUTBOUND_COMMAND owns cumulative look and held revisions. These
        // small checkpoints retain only what the packed publication needs.
        let state = State {
            sequence: self.sequence as f64,
            tick,
            held,
            look: [values[out::LOOK_X], values[out::LOOK_Y]],
            revision: values[out::REVISION],
            pilot: None,
        };
        if values[out::RESET] != 0. {
            self.latest = None;
            self.backup = None;
        }
        if values[out::CHANGED] != 0. {
            self.backup = Some(state);
        }
        if values[out::TRANSITION] == 0. {
            self.backup = None;
        }
        self.use_backup = values[out::BACKUP] != 0.;
        self.latest = Some(state);
        // Copy callback scalars before mutating our retry queue. The outbound
        // owner retains its event bank until the next generated command.
        let mut actions = [0u8; 32];
        let ac = self.owner.actions().len().min(32);
        actions[..ac].copy_from_slice(&self.owner.actions()[..ac]);
        for &action in &actions[..ac] {
            self.edge(0., tick)[e::ACTION] = action as f64;
        }
        let mut fires = [bfe_presentation::network::outbound::Fire::default(); 128];
        let fc = self.owner.fires().len().min(128);
        fires[..fc].copy_from_slice(&self.owner.fires()[..fc]);
        self.counted += look.x * self.mouse_scale / receiver.own.zoom.max(1.);
        if look.x != 0. || look.y != 0. {
            if self.pointer_count == 128 {
                // The oldest look leaves the preview: its counts leave the count with it.
                let dropped = self.pointers[self.pointer_at];
                self.counted -= dropped.x * dropped.scale / dropped.zoom;
                self.pointer_at = (self.pointer_at + 1) % 128;
                self.pointer_count -= 1;
                self.refused += 1;
            }
            let to = (self.pointer_at + self.pointer_count) % 128;
            let turn_scale = self.turn_scale(receiver);
            self.pointers[to] = Pointer {
                sequence: state.sequence,
                x: look.x,
                y: look.y,
                zoom: receiver.own.zoom.max(1.),
                scale: self.mouse_scale,
                invert: self.invert,
                turn_scale,
            };
            self.pointer_count += 1;
        }
        if receiver.infantry_view.is_some() && !receiver.own.walker && receiver.own.remote.is_none() {
            if self.infantry_count == 128 {
                let dropped = self.infantry_pending[self.infantry_at as usize];
                self.infantry_acked_mouse_yaw += dropped.mouse_yaw;
                self.infantry_at = (self.infantry_at + 1) % 128;
                self.infantry_count -= 1;
            }
            let to = ((self.infantry_at + self.infantry_count) % 128) as usize;
            let forward = f64::from((c.held & (1 << 0) != 0) as u8) - f64::from((c.held & (1 << 1) != 0) as u8);
            let right = f64::from((c.held & (1 << 2) != 0) as u8) - f64::from((c.held & (1 << 3) != 0) as u8);
            self.infantry_pending[to] = PendingInfantry {
                sequence: state.sequence,
                tick: state.tick,
                forward,
                right,
                jump: c.actions & (1 << 0) != 0,
                crouch: c.actions & (1 << 1) != 0,
                controls: infantry.unwrap_or_default(),
                x: look.x,
                y: look.y,
                zoom: receiver.own.zoom.max(1.),
                scale: self.mouse_scale,
                invert: self.invert,
                mouse_yaw: 0.,
            };
            self.infantry_count += 1;
        }
        self.preview(receiver);
        self.fire_aim = Some([self.yaw,self.pitch]);
        // The camera ray's aim is read only by what fires: a trigger edge this
        // tick, or a trigger held (each predicted round and the walker's
        // repeater take it). Nothing else looks at it.
        const TRIGGERS: u32 = 1 << 6 | 1 << 7;
        let firing = fc > 0 || c.held & TRIGGERS != 0;
        if let (true, Some(scene)) = (firing || self.trace, &self.aim_scene) {
            let (aim,reach,target)=scene.borrow().sample(&receiver.own,self.yaw,self.pitch,self.first_person);
            self.fire_aim=Some([aim.yaw,aim.pitch]);
            if target != self.aim_target && self.trace {
                eprintln!("[combat-aim] {{\"tick\":{},\"target\":{}}}", receiver.latest, target.map_or(-1, i64::from));
            }
            self.aim_target = target;
            if fires[..fc].iter().any(|fire|fire.pressed && !fire.held) && self.trace {
                use bfe_presentation::network_layout::net_record as record;
                let velocity=target.and_then(|id|receiver.active().find(|row|row[record::ID]==id as f64).map(|row|[row[record::VELOCITY],row[record::VELOCITY+1],row[record::VELOCITY+2]]));
                let team=target.and_then(|id|receiver.active().find(|row|row[record::ID]==id as f64).map(|row|row[record::TEAM]));
                eprintln!("[combat-client] {}",serde_json::json!({"sequence":self.sequence,"player":receiver.own.id,
                    "displayedTick":super::clock::rendered_tick(receiver.clock.shown,receiver.latest),
                    "bodyAim":[self.yaw,self.pitch],"fireAim":self.fire_aim,"camera":scene.borrow().camera,
                    "viewProjection":scene.borrow().view_projection,"position":scene.borrow().position,"posture":receiver.own.posture,"zoom":receiver.own.zoom,
                    "rayFrom":[aim.from.x,aim.from.y,aim.from.z],"rayDirection":[aim.basis.direction.x,aim.basis.direction.y,aim.basis.direction.z],
                    "origin":[aim.basis.origin.x,aim.basis.origin.y,aim.basis.origin.z],"reach":reach,"target":target,"targetTeam":team,"targetVelocity":velocity,"team":receiver.own.team}));
            }
        }
        for fire in &fires[..fc] {
            if !fire.held {
                let [yaw,pitch] = self.fire_aim.unwrap_or([self.yaw,self.pitch]);
                let view = super::clock::rendered_tick(receiver.clock.shown, receiver.latest);
                let edge = self.edge(1., tick);
                edge[e::BUTTON] = fire.button as f64;
                edge[e::PRESSED] = fire.pressed as u8 as f64;
                edge[e::VIEW_TICK] = view;
                edge[e::AIM_YAW] = yaw;
                edge[e::AIM_PITCH] = pitch;
                self.fire_edges += 1;
            }
        }
    }
    fn turn_scale(&self, receiver: &Receiver) -> Option<f64> {
        let own = &receiver.own;
        if own.remote.is_some() {
            return None;
        }
        let scales = self.turns.get(own.class)?.swbf2?;
        let posture = Posture::from_u8(own.posture as u8).unwrap_or_default();
        let air = if own.air == 0 { None } else { Air::from_u8(own.air - 1) };
        let sample = receiver
            .infantry
            .filter(|s| s.id == own.id as u32 && s.generation == own.generation);
        // The simulation's rule (`look_turn_scale`), with sprint, roll and the tumble phase from the local
        // sample or, online, from the replay's restored state (`note_replayed_state`). The stand-up lock is
        // the body's look lock.
        let (sprinting, rolling, tumbling) = match sample {
            Some(s) => (
                s.sprint,
                s.roll.is_some(),
                matches!(s.tumble, Some(TumblePhase::Thrown | TumblePhase::Tumble | TumblePhase::Landed)),
            ),
            None => self.replayed.map_or((false, false, false), |r| (r.sprinting, r.rolling, r.tumbling)),
        };
        let tumbling = tumbling || own.air == Air::Thrown as u8 + 1;
        let combo_turn = sample.map_or(1.0, |s| if s.tumble.is_some() { 1.0 } else { s.turn_factor });
        Some(bfe_sim::units::infantry::look_turn_scale(
            scales, posture, air, sprinting, rolling, tumbling, combo_turn,
        ))
    }
    fn swbf2_tick(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, receiver: &Receiver, rtt: f64, now: f64) {
        if !receiver.welcomed || receiver.spectator || receiver.corrections == 0 {
            return;
        }
        self.mouse_scale = if c.has_mouse_scale && c.mouse_scale.is_finite() && c.mouse_scale > 0. { c.mouse_scale.min(2.) } else { 1. };
        self.aspect = if c.has_aspect && c.aspect.is_finite() && c.aspect > 0. { c.aspect } else { 1280. / 720. };
        self.invert = c.invert.unwrap_or(false);
        self.first_person = c.first_person.unwrap_or(false);
        self.sequence = self.sequence.wrapping_add(1).max(1);
        let flying = receiver.uses_flight_controls();
        let mut held = c.held & 0x1ff;
        if flying {
            for (flag, bit) in FLIGHT_KEYS {
                if flight.held & (1 << flag as u8) != 0 {
                    held |= 1 << bit;
                }
            }
        } else {
            // A 2005 soldier's sprint and block holds.
            for &(bit, code) in self.inputs.foot_holds() {
                if c.held & (1 << bit) != 0 {
                    held |= 1 << foot_bit(code);
                }
            }
            for (code, _) in self.inputs.soldier_codes().iter().zip(self.soldier.held()).filter(|(_, on)| *on) {
                held |= 1 << foot_bit(code);
            }
        }
        self.flight = flying.then(|| flight.clone());
        if self.local {
            self.cooldown = self.cooldown.saturating_sub(1);
            self.calm += 1;
            if self.calm >= 600 {
                self.calm = 0;
                self.extra = (self.extra - 1.).max(0.);
            }
        }
        let target = if let (true, Some(next)) = (self.local, self.local_next) { next + 1. + self.extra } else { target_lead(LeadTargetInput {
            hz: 60.,
            margin: 2.,
            max_lead: ONLINE_MAX_LEAD,
            worst_gap: 0.,
            extra: self.extra,
            extrapolate: true,
            estimate: receiver.estimator.estimate(now / 1000.),
            latest: receiver.latest,
            round_trip_seconds: rtt / 1000.,
        })
        .bounded };
        let tick = target.floor().max(self.last_tick + 1.).min(receiver.latest + 30.);
        if !self.measured {
            self.ahead = tick > target + 1.;
            self.measured = true;
        }
        self.last_tick = tick;
        self.stamps[self.sequence as usize % 256] = (self.sequence, tick);
        self.history.push_with_use(c, look, flight, self.use_held, tick, receiver.own.generation, self.sequence as f64);
        let mut x = [0.; 512];
        let mut at = 0;
        for n in [receiver.epoch, receiver.own.generation, self.sequence as f64, tick] {
            put(&mut x, &mut at, n);
        }
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        for bit in 0..KEYS.len() {
            if held & (1 << bit) != 0 {
                let name = KEYS[bit];
                put(&mut x, &mut at, name.len() as f64);
                for b in name.bytes() {
                    put(&mut x, &mut at, b as f64);
                }
            }
        }
        x[count] = (at - start) as f64;
        put(&mut x, &mut at, 1.);
        put(&mut x, &mut at, look.x);
        put(&mut x, &mut at, look.y);
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        // The shared numeric adapter takes the released actions only: a 2005
        // soldier's roll is added as its own edge after it (below).
        let allowed = self.inputs.action_bits(!flying);
        for bit in 0..ROLL {
            if c.actions & allowed & (1 << bit) != 0 && bit != 4 && bit != 5 {
                if let Some(action) = self.actions[bit] {
                    put(&mut x, &mut at, action as f64);
                }
            }
        }
        if flying {
            for (bit, &action) in self.flight_actions.iter().enumerate() {
                if flight.actions & (1 << bit) != 0 {
                    put(&mut x, &mut at, action as f64);
                }
            }
        }
        for (code, count) in [(21, c.primary_count.min(32)), (22, c.secondary_count.min(32))] {
            for _ in 0..count {
                put(&mut x, &mut at, code as f64);
            }
        }
        x[count] = (at - start) as f64;
        let mut values = [0.; out::STRIDE];
        if self.owner.run(op::OUTBOUND_COMMAND, &mut Words::new(&x[..at]), &mut Output { values: &mut values, at: 0 }).is_err() {
            self.refused += 1;
            return;
        }
        let pilot_controls = receiver
            .swbf2_plan
            .as_ref()
            .is_some_and(|p| {
                p.of(bfe_sim::wire::revisions::PILOT)
                    == bfe_sim::wire::revisions::Use::Revision(bfe_sim::wire::space::REVISION)
            })
            .then(|| crate::local::space::controls(c, look, flight, self.use_held));
        let state = State {
            sequence: self.sequence as f64,
            tick,
            held,
            look: [values[out::LOOK_X], values[out::LOOK_Y]],
            revision: values[out::REVISION],
            pilot: pilot_controls,
        };
        if values[out::RESET] != 0. {
            self.latest = None;
            self.backup = None;
        }
        if values[out::CHANGED] != 0. {
            self.backup = Some(state);
        }
        if values[out::TRANSITION] == 0. {
            self.backup = None;
        }
        self.use_backup = values[out::BACKUP] != 0.;
        self.latest = Some(state);
        let mut actions = [0u8; 32];
        let ac = self.owner.actions().len().min(32);
        actions[..ac].copy_from_slice(&self.owner.actions()[..ac]);
        for &action in &actions[..ac] {
            self.edge(0., tick)[e::ACTION] = action as f64;
        }
        // A 2005 soldier's roll press, after the actions the adapter returned,
        // then block's presses and releases since the last command: a tap the
        // held level does not show is sent whole, in its order.
        if c.actions & allowed & (1 << ROLL) != 0 {
            if let Some(action) = self.actions[ROLL] {
                self.edge(0., tick)[e::ACTION] = action as f64;
            }
        }
        // A hero's power lock press on foot: the server takes the lock (or lets
        // it go) and names its target in his own correction.
        if let Some(lock) = self.inputs.lock_action().filter(|_| self.soldier.lock_pressed && !flying && receiver.own.mount.is_none()) {
            self.edge(0., tick)[e::ACTION] = lock as f64;
        }
        if let Some((press, release)) = self.inputs.block_actions() {
            let on_foot = !flying && receiver.own.mount.is_none();
            let block = on_foot && c.held & (1 << crate::generated_input::ClientHeld::Block as u32) != 0;
            let tapped = on_foot && self.soldier.block_pressed;
            let edges: &[usize] = match (self.block_down, block, tapped) {
                (false, true, _) => &[press],
                (false, false, true) => &[press, release],
                (true, true, true) => &[release, press],
                (true, false, true) => &[release, press, release],
                (true, false, false) => &[release],
                (false, false, false) | (true, true, false) => &[],
            };
            for &edge in edges {
                self.edge(0., tick)[e::ACTION] = edge as f64;
            }
            self.block_down = block;
        }
        let mut fires = [bfe_presentation::network::outbound::Fire::default(); 128];
        let fc = self.owner.fires().len().min(128);
        fires[..fc].copy_from_slice(&self.owner.fires()[..fc]);
        self.counted += look.x * self.mouse_scale / receiver.own.zoom.max(1.);
        if look.x != 0. || look.y != 0. {
            if self.pointer_count == 128 {
                let dropped = self.pointers[self.pointer_at];
                self.counted -= dropped.x * dropped.scale / dropped.zoom;
                self.pointer_at = (self.pointer_at + 1) % 128;
                self.pointer_count -= 1;
                self.refused += 1;
            }
            let to = (self.pointer_at + self.pointer_count) % 128;
            let turn_scale = self.turn_scale(receiver);
            self.pointers[to] = Pointer { sequence: state.sequence, x: look.x, y: look.y, zoom: receiver.own.zoom.max(1.), scale: self.mouse_scale, invert: self.invert, turn_scale };
            self.pointer_count += 1;
        }
        self.preview(receiver);
        self.fire_aim = Some([self.yaw,self.pitch]);
        const TRIGGERS: u32 = 1 << 6 | 1 << 7;
        let firing = fc > 0 || c.held & TRIGGERS != 0;
        if let (true, Some(scene)) = (firing || self.trace, &self.aim_scene) {
            let (aim,reach,target)=scene.borrow().sample(&receiver.own,self.yaw,self.pitch,self.first_person);
            self.fire_aim=Some([aim.yaw,aim.pitch]);
            if target != self.aim_target && self.trace {
                eprintln!("[combat-aim] {{\"tick\":{},\"target\":{}}}", receiver.latest, target.map_or(-1, i64::from));
            }
            self.aim_target = target;
            if fires[..fc].iter().any(|fire|fire.pressed && !fire.held) && self.trace {
                use bfe_presentation::network_layout::net_record as record;
                let velocity=target.and_then(|id|receiver.active().find(|row|row[record::ID]==id as f64).map(|row|[row[record::VELOCITY],row[record::VELOCITY+1],row[record::VELOCITY+2]]));
                let team=target.and_then(|id|receiver.active().find(|row|row[record::ID]==id as f64).map(|row|row[record::TEAM]));
                eprintln!("[combat-client] {}",serde_json::json!({"sequence":self.sequence,"player":receiver.own.id,
                    "displayedTick":super::clock::rendered_tick(receiver.clock.shown,receiver.latest),
                    "bodyAim":[self.yaw,self.pitch],"fireAim":self.fire_aim,"camera":scene.borrow().camera,
                    "viewProjection":scene.borrow().view_projection,"position":scene.borrow().position,"posture":receiver.own.posture,"zoom":receiver.own.zoom,
                    "rayFrom":[aim.from.x,aim.from.y,aim.from.z],"rayDirection":[aim.basis.direction.x,aim.basis.direction.y,aim.basis.direction.z],
                    "origin":[aim.basis.origin.x,aim.basis.origin.y,aim.basis.origin.z],"reach":reach,"target":target,"targetTeam":team,"targetVelocity":velocity,"team":receiver.own.team}));
            }
        }
        if !receiver.space_mounted() {
            for fire in &fires[..fc] {
                if !fire.held {
                    let [yaw,pitch] = self.fire_aim.unwrap_or([self.yaw,self.pitch]);
                    let view = super::clock::rendered_tick(receiver.clock.shown, receiver.latest);
                    let edge = self.edge(1., tick);
                    edge[e::BUTTON] = fire.button as f64;
                    edge[e::PRESSED] = fire.pressed as u8 as f64;
                    edge[e::VIEW_TICK] = view;
                    edge[e::AIM_YAW] = yaw;
                    edge[e::AIM_PITCH] = pitch;
                    self.fire_edges += 1;
                }
            }
        }
    }
    // BodyPreview starts from the most recent applied correction, then
    // follows Soldier.lookRaw for each still-unapplied pointer event.
    pub fn preview(&mut self, receiver: &Receiver) {
        let own = &receiver.own;
        let class_turn = if own.remote.is_some() {
            ClassTurn::classic(2., [-50_f64.to_radians(), 50_f64.to_radians()])
        } else {
            self.turns.get(own.class).copied().unwrap_or(ClassTurn::classic(2., [-0.8, 1.1]))
        };
        let mut body = if own.remote.is_some() {
            Soldier {
                aim_yaw: own.remote_yaw,
                aim_pitch: own.remote_pitch,
                max_turn: class_turn.max_turn,
                pitch_limits: class_turn.pitch_limits,
                posture: Posture::Stand,
                air: None,
                dive: 0.,
                lock: 0.,
                ..Default::default()
            }
        } else {
            Soldier {
                aim_yaw: own.yaw,
                aim_pitch: own.pitch,
                max_turn: class_turn.max_turn,
                pitch_limits: class_turn.pitch_limits,
                posture: Posture::from_u8(own.posture as u8).unwrap_or_default(),
                air: if own.air == 0 { None } else { Air::from_u8(own.air - 1) },
                dive: own.dive,
                lock: own.lock,
                ..Default::default()
            }
        };
        if receiver.infantry_view.is_some() && !own.walker && own.remote.is_none() {
            if let (Some(model), Some(sample)) = (
                self.swbf2_models.get(own.class).and_then(Option::as_ref),
                receiver
                    .infantry
                    .filter(|s| s.id == own.id as u32 && s.generation == own.generation),
            ) {
                let mut pred = bfe_sim::units::infantry::TurnPredictor::from_sample(
                    body.posture,
                    body.air,
                    own.velocity,
                    own.lock,
                    &sample,
                );
                let life = [receiver.epoch, own.id, own.generation];
                let now = [own.velocity[0], own.velocity[2]];
                let v = &mut self.infantry_velocity;
                if v.life != Some(life) || own.tick < v.tick {
                    *v = CorrectedVelocity { life: Some(life), tick: own.tick, velocity: now, change: [0.; 2] };
                } else if own.tick > v.tick {
                    let ticks = own.tick - v.tick;
                    v.change = if ticks <= 2. {
                        [(now[0] - v.velocity[0]) / ticks, (now[1] - v.velocity[1]) / ticks]
                    } else {
                        [0.; 2]
                    };
                    v.tick = own.tick;
                    v.velocity = now;
                }
                pred.velocity_change_xz = v.change;
                let mut cur_tick = receiver.latest;
                let mut prev = self.last_acked_infantry;
                let mut pending_mouse_yaw = 0.;
                for i in 0..self.infantry_count {
                    let idx = ((self.infantry_at + i) % 128) as usize;
                    let p = self.infantry_pending[idx];
                    let gap = ((p.tick - cur_tick - 1.).round().max(0.) as usize).min(8);
                    for _ in 0..gap {
                        pred.step(model, prev.forward, prev.right, false, false, prev.controls, body.aim_yaw, 1. / 60.);
                    }
                    let scale = pred.scale(model);
                    body.lock = pred.lock;
                    let yaw_before = body.aim_yaw;
                    if p.x != 0. || p.y != 0. {
                        body.look_raw_scaled(p.x / p.zoom, p.y / p.zoom, p.scale, p.invert, scale);
                    }
                    let dy = wrap_delta(body.aim_yaw - yaw_before);
                    self.infantry_pending[idx].mouse_yaw = dy;
                    pending_mouse_yaw += dy;
                    pred.step(model, p.forward, p.right, p.jump, p.crouch, p.controls, body.aim_yaw, 1. / 60.);
                    prev = HeldInfantry {
                        forward: p.forward,
                        right: p.right,
                        controls: held_only(p.controls),
                    };
                    cur_tick = p.tick.max(cur_tick + 1.);
                }
                let current_scale = pred.scale(model);
                body.lock = pred.lock;
                let mut one = body.clone();
                one.look_raw_scaled(1., 0., 1., false, current_scale);
                self.per_count = one.aim_yaw - body.aim_yaw;
                let yaw_before = body.aim_yaw;
                self.history.loose.turn_scaled(&mut body, own.zoom, Some(current_scale));
                let loose_yaw = wrap_delta(body.aim_yaw - yaw_before);
                self.infantry_mouse_yaw = self.infantry_acked_mouse_yaw + pending_mouse_yaw + loose_yaw;
                self.infantry_predicted_mouse = true;
                self.yaw = body.aim_yaw;
                self.pitch = body.aim_pitch;
                return;
            }
        }
        self.infantry_predicted_mouse = false;
        let current_scale = self.turn_scale(receiver);
        // What one mouse count turns this body by (on a copy: the body itself is only touched by real looks).
        let mut one = body.clone();
        match current_scale {
            Some(scale) => one.look_raw_scaled(1., 0., 1., false, scale),
            None => one.look_raw(1., 0., 1., false),
        }
        self.per_count = one.aim_yaw - body.aim_yaw;
        for i in 0..self.pointer_count {
            let p = self.pointers[(self.pointer_at + i) % 128];
            match p.turn_scale {
                Some(scale) => body.look_raw_scaled(p.x / p.zoom, p.y / p.zoom, p.scale, p.invert, scale),
                None => body.look_raw(p.x / p.zoom, p.y / p.zoom, p.scale, p.invert),
            }
        }
        // And the counts no command has taken yet (none while a command is
        // being made: its own look is the newest pointer above).
        self.history.loose.turn_scaled(&mut body, own.zoom, current_scale);
        self.yaw = body.aim_yaw;
        self.pitch = body.aim_pitch;
    }
    /// Once a frame, after `preview`, for a droideka the player controls: the part of a correction's
    /// turn that the mouse does not account for is shown over the time to the next correction (see
    /// [`WalkerTurn`]). The mouse's own turn is never delayed. Anything else: nothing.
    pub fn ease_walker_turn(&mut self, receiver: &Receiver, dt: f64) {
        if !receiver.own.walker && receiver.infantry_view.is_some() {
            self.ease_swbf2_infantry_turn(receiver, dt);
            return;
        }
        let own = &receiver.own;
        let life = [receiver.epoch, own.id, own.generation];
        let counted = self.counted + self.history.loose.x * self.history.loose.scale / own.zoom.max(1.);
        let active = own.walker && own.alive && own.remote.is_none() && own.mount.is_none();
        let w = &mut self.walker;
        if !active || w.life != Some(life) {
            w.left = 0.;
            w.rate = 0.;
        } else {
            let turn = self.yaw - w.yaw - self.per_count * (counted - w.counted);
            let step = turn.sin().atan2(turn.cos());
            if step.abs() > 0.5 {
                // A new facing (a spawn, a teleport): at once.
                w.left = 0.;
            } else if step.abs() > 1e-9 {
                w.left -= step;
                w.rate = w.left.abs() / ((own.tick - w.tick).clamp(1., 6.) / 60.);
            }
            let shown = w.rate * dt.max(0.);
            w.left = if w.left.abs() <= shown { 0. } else { w.left - shown * w.left.signum() };
        }
        w.life = active.then_some(life);
        w.yaw = self.yaw;
        w.counted = counted;
        w.tick = own.tick;
        self.yaw += w.left;
    }
    fn ease_swbf2_infantry_turn(&mut self, receiver: &Receiver, dt: f64) {
        let own = &receiver.own;
        let life = [receiver.epoch, own.id, own.generation];
        let counted = self.counted + self.history.loose.x * self.history.loose.scale / own.zoom.max(1.);
        let mouse_yaw = self.infantry_mouse_yaw;
        let active = own.alive && own.remote.is_none() && own.mount.is_none();
        let w = &mut self.infantry_turn;
        if !active || w.life != Some(life) {
            w.left = 0.;
            w.rate = 0.;
        } else {
            let d_mouse = if self.infantry_predicted_mouse {
                mouse_yaw - w.mouse_yaw
            } else {
                self.per_count * (counted - w.counted)
            };
            let turn = self.yaw - w.yaw - d_mouse;
            let step = turn.sin().atan2(turn.cos());
            if step.abs() > 0.5 {
                w.left = 0.;
            } else if step.abs() > 1e-9 {
                w.left -= step;
                w.rate = w.left.abs() / ((own.tick - w.tick).clamp(1., 6.) / 60.);
            }
            let shown = w.rate * dt.max(0.);
            w.left = if w.left.abs() <= shown { 0. } else { w.left - shown * w.left.signum() };
        }
        w.life = active.then_some(life);
        w.yaw = self.yaw;
        w.counted = counted;
        w.mouse_yaw = mouse_yaw;
        w.tick = own.tick;
        self.yaw += w.left;
    }
    /// The frame's loose look: the mouse counts `look` holds that the input
    /// clock, `alpha` of the way into its tick, has not put in a command,
    /// with the mouse settings the next command will carry.
    pub fn set_loose(&mut self, look: &ClientLook, mouse_scale: f64, invert: bool, alpha: f64) {
        let scale = if mouse_scale.is_finite() && mouse_scale > 0. { mouse_scale.min(2.) } else { 1. };
        self.history.loose = super::replay::Loose { x: look.x, y: look.y, scale, invert, alpha };
    }
    pub fn spawn(&mut self, receiver: &Receiver) {
        if self.spawned || !receiver.complete || receiver.corrections == 0 || receiver.spectator {
            return;
        }
        // A pick made before this first correction is the player's choice:
        // the team he picked first, where the correction puts him on another.
        if let Some((team, class_id, post_id)) = self.pick.take() {
            if receiver.own.alive {
                self.spawned = true;
            } else {
                if receiver.own.team != 0 && receiver.own.team != team {
                    self.request_team(receiver, team);
                }
                self.send_spawn(receiver, class_id, post_id);
            }
            return;
        }
        if !self.auto_spawn {
            return;
        }
        // The correction names the server-selected roster class. Choosing a
        // model from the content table can select an unavailable special class.
        self.spawned = true;
        if receiver.own.alive {
            return;
        }
        let tick = self.last_tick.max(receiver.latest + 1.);
        let edge = self.edge(3., tick);
        edge[e::CLASS_ID] = receiver.own.class as f64;
        edge[e::POST_ID] = 0.;
    }
    pub fn request_team(&mut self, receiver: &Receiver, team: u8) {
        if !receiver.complete || receiver.corrections == 0 {
            return;
        }
        let tick = self.last_tick.max(receiver.latest + 1.);
        let edge = self.edge(2., tick);
        edge[e::TEAM] = team as f64;
        edge[e::TEAM_RANGE_VALID] = 1.;
    }
    /// Ask to spawn as `class_id` at `post_id`, on `team`: the team the
    /// player picked with them, which goes first only with a pick kept for
    /// the first correction (the team he is on is not known before it; once
    /// it is, the caller asks for the team itself).
    pub fn request_spawn(&mut self, receiver: &Receiver, team: u8, class_id: usize, post_id: usize) {
        if receiver.spectator || receiver.own.alive {
            return;
        }
        if !receiver.complete || receiver.corrections == 0 {
            // Nothing can be sent before the server's first correction: keep
            // the pick for `spawn`, which sends it once one has come.
            self.pick = Some((team, class_id, post_id));
            return;
        }
        self.send_spawn(receiver, class_id, post_id);
    }
    fn send_spawn(&mut self, receiver: &Receiver, class_id: usize, post_id: usize) {
        self.spawned = true;
        self.pick = None;
        let tick = self.last_tick.max(receiver.latest + 1.);
        let edge = self.edge(3., tick);
        edge[e::CLASS_ID] = if class_id != 0 { class_id } else { receiver.own.class.max(1) } as f64;
        edge[e::POST_ID] = post_id as f64;
    }
    pub fn request_suicide(&mut self, receiver: &Receiver) {
        if !receiver.complete || receiver.corrections == 0 || receiver.spectator || !receiver.own.alive {
            return;
        }
        let tick = self.last_tick.max(receiver.latest + 1.);
        let _ = self.edge(4., tick);
    }
    pub fn send(&mut self, endpoint: &mut Endpoint, receiver: &mut Receiver) -> Result<(), String> {
        match self.inputs.family {
            bfe_family::GameFamily::Swbf1 => self.released_send(endpoint, receiver),
            bfe_family::GameFamily::Swbf2 => self.swbf2_send(endpoint, receiver),
        }
    }
    fn released_send(&mut self, endpoint: &mut Endpoint, receiver: &mut Receiver) -> Result<(), String> {
        let network = receiver.network.borrow();
        let epoch = network.header[h::EPOCH];
        let ack = network.header[h::ACK_TICK];
        // Spawn screens and spectators still acknowledge complete snapshots.
        // A keepalive carries no synthetic movement command or new sequence.
        let Some(latest) = self.latest else {
            if receiver.welcomed && endpoint.now() - self.last_ack >= 100. {
                self.last_ack = endpoint.now();
                let mut bytes = [0; 1100];
                let n = encode(op::ACK, &mut Words::new(&[epoch, ack]), &mut bytes).map_err(str::to_string)?;
                endpoint.send(Lane::State, false, &bytes[..n]).map_err(|e| e.to_string())?;
            }
            return Ok(());
        };
        if !self.owner.allow(latest.tick, self.edge_count, 2) {
            return Ok(());
        }
        let mut count = 0;
        for at in 0..self.edge_count {
            if self.edges[at][e::TICK] >= latest.tick - 30. {
                self.edges[count] = self.edges[at];
                count += 1;
            }
        }
        self.edge_count = count;
        // A held transition retains one predecessor until acknowledged.
        // Both commands refer to a single validated view context on the wire.
        let backup = self.backup.filter(|s| self.use_backup && s.sequence < latest.sequence);
        let states = [backup.unwrap_or(latest), latest];
        let commands = if backup.is_some() { 2 } else { 1 };
        let mut edges = self.edge_count;
        loop {
            let mut x = [0.; 2048];
            let mut at = 0;
            for n in [self.life[0], self.life[1], 1., commands as f64, edges as f64, 1., epoch, ack] {
                put(&mut x, &mut at, n);
            }
            let start = at;
            at += v::STRIDE;
            x[start + v::FLAGS] = 32. + 64. + 128. + 2. * self.first_person as u8 as f64 + self.invert as u8 as f64;
            x[start + v::PROFILE_FOUND] = 1.;
            x[start + v::MOUSE_SCALE] = self.mouse_scale;
            x[start + v::ASPECT] = self.aspect;
            x[start + v::VIEW_TICK] = super::clock::rendered_tick(receiver.clock.shown, receiver.latest);
            // A pilot's flight settings and pad, as the Instant Action
            // mailbox gets them: the server's stick reads them as the
            // prediction does.
            if let Some(flight) = &self.flight {
                let mut flags = 4. + if flight.invert { 8. } else { 0. };
                // Settings outside the wire's ranges stay home: the server
                // then reads its own defaults rather than refusing the packet.
                if flight.scale > 0. && flight.scale <= 10. {
                    flags += 256.;
                    x[start + v::SENSITIVITY] = flight.scale;
                }
                if (0.1..=4.).contains(&flight.curve) {
                    flags += 512.;
                    x[start + v::CURVE] = flight.curve;
                }
                if flight.pad_buttons != 0 || flight.pad_axes.iter().any(|a| *a != 0.) {
                    flags += 16.;
                    for (out, axis) in x[start + v::AXES..start + v::AXES + 4].iter_mut().zip(flight.pad_axes) {
                        *out = axis.clamp(-1., 1.);
                    }
                    x[start + v::BUTTONS] = f64::from(flight.pad_buttons & 0xffff);
                }
                x[start + v::FLAGS] += flags;
            }
            for state in &states[..commands] {
                for n in [1., state.sequence, state.tick, 0.] {
                    put(&mut x, &mut at, n);
                }
                let count = at;
                put(&mut x, &mut at, 0.);
                let first = at;
                for bit in 0..KEYS.len() {
                    if state.held & (1 << bit) != 0 {
                        put(&mut x, &mut at, self.held[bit] as f64);
                    }
                }
                x[count] = (at - first) as f64;
                put(&mut x, &mut at, state.look[0]);
                put(&mut x, &mut at, state.look[1]);
            }
            for row in &self.edges[..edges] {
                x[at..at + e::STRIDE].copy_from_slice(row);
                at += e::STRIDE;
            }
            for code in 0..128 {
                put(&mut x, &mut at, if code < INPUT_CODES.len() { 1. } else { 0. });
            }
            let mut bytes = [0; 1100];
            match encode(op::INPUT, &mut Words::new(&x[..at]), &mut bytes) {
                Ok(length) => {
                    if endpoint.send(Lane::State, false, &bytes[..length]).map_err(|e| e.to_string())? {
                        self.sent += 1;
                        let mut result = [0.; 4];
                        let words = [latest.sequence, latest.look[0], latest.look[1], latest.tick, latest.revision];
                        self.owner
                            .run(op::OUTBOUND_SENT, &mut Words::new(&words), &mut Output { values: &mut result, at: 0 })
                            .map_err(str::to_string)?;
                    }
                    break;
                }
                Err("input packet exceeds bounds") if edges != 0 => edges -= 1,
                Err(error) => return Err(error.into()),
            }
        }
        // Result ACKs retire server-side edge outcomes, independently of
        // input receipt and correction execution acknowledgements.
        if receiver.outcomes.count != 0 {
            #[derive(Serialize)]
            struct Ack<'a> {
                #[serde(rename = "type")]
                kind: &'static str,
                epoch: u32,
                generation: u32,
                ids: &'a [u32],
            }
            let mut bytes = Bytes { values: [0; 1100], at: 0 };
            serde_json::to_writer(
                &mut bytes,
                &Ack {
                    kind: "entity-edge-ack",
                    epoch: self.life[0] as u32,
                    generation: self.life[1] as u32,
                    ids: &receiver.outcomes.values[..receiver.outcomes.count],
                },
            )
            .map_err(|e| e.to_string())?;
            if endpoint.send(Lane::State, true, &bytes.values[..bytes.at]).map_err(|e| e.to_string())? {
                receiver.outcomes.count = 0;
            }
        }
        Ok(())
    }
    fn swbf2_send(&mut self, endpoint: &mut Endpoint, receiver: &mut Receiver) -> Result<(), String> {
        let network = receiver.network.borrow();
        let epoch = network.header[h::EPOCH];
        let ack = network.header[h::ACK_TICK];
        let Some(latest) = self.latest else {
            if receiver.welcomed && endpoint.now() - self.last_ack >= 100. {
                self.last_ack = endpoint.now();
                let mut bytes = [0; 1100];
                let n = encode(op::ACK, &mut Words::new(&[epoch, ack]), &mut bytes).map_err(str::to_string)?;
                endpoint.send(Lane::State, false, &bytes[..n]).map_err(|e| e.to_string())?;
            }
            return Ok(());
        };
        let space_mounted = receiver.space_mounted();
        if !space_mounted {
            if !self.owner.allow(latest.tick, self.edge_count, 2) {
                return Ok(());
            }
        } else if latest.sequence <= self.last_sent_space_seq && self.edge_count == 0 {
            return Ok(());
        }
        let mut count = 0;
        for at in 0..self.edge_count {
            if self.edges[at][e::TICK] >= latest.tick - 30. {
                self.edges[count] = self.edges[at];
                count += 1;
            }
        }
        self.edge_count = count;
        // Mounted 2005 Space controls carry the immediately preceding
        // unacknowledged tick so one lost UDP datagram loses zero pilot ticks.
        let space_prev = space_mounted
            .then_some(self.prev_space_state)
            .flatten()
            .filter(|s| {
                s.sequence < latest.sequence
                    && s.tick <= latest.tick
                    && s.sequence > receiver.corrected_sequence
            });
        let backup = space_prev.or_else(|| {
            self.backup
                .filter(|s| self.use_backup && s.sequence < latest.sequence)
        });
        let states = [backup.unwrap_or(latest), latest];
        let commands = if backup.is_some() { 2 } else { 1 };
        let mut edges = self.edge_count;
        loop {
            let mut x = [0.; 2048];
            let mut at = 0;
            for n in [self.life[0], self.life[1], 1., commands as f64, edges as f64, 1., epoch, ack] {
                put(&mut x, &mut at, n);
            }
            let start = at;
            at += v::STRIDE;
            x[start + v::FLAGS] = 32. + 64. + 128. + 2. * self.first_person as u8 as f64 + self.invert as u8 as f64;
            x[start + v::PROFILE_FOUND] = 1.;
            x[start + v::MOUSE_SCALE] = self.mouse_scale;
            x[start + v::ASPECT] = self.aspect;
            x[start + v::VIEW_TICK] = super::clock::rendered_tick(receiver.clock.shown, receiver.latest);
            if let Some(flight) = &self.flight {
                let mut flags = 4. + if flight.invert { 8. } else { 0. };
                if flight.scale > 0. && flight.scale <= 10. {
                    flags += 256.;
                    x[start + v::SENSITIVITY] = flight.scale;
                }
                if (0.1..=4.).contains(&flight.curve) {
                    flags += 512.;
                    x[start + v::CURVE] = flight.curve;
                }
                if flight.pad_buttons != 0 || flight.pad_axes.iter().any(|a| *a != 0.) {
                    flags += 16.;
                    for (out, axis) in x[start + v::AXES..start + v::AXES + 4].iter_mut().zip(flight.pad_axes) {
                        *out = axis.clamp(-1., 1.);
                    }
                    x[start + v::BUTTONS] = f64::from(flight.pad_buttons & 0xffff);
                }
                x[start + v::FLAGS] += flags;
            }
            for state in &states[..commands] {
                for n in [1., state.sequence, state.tick, 0.] {
                    put(&mut x, &mut at, n);
                }
                let count = at;
                put(&mut x, &mut at, 0.);
                let first = at;
                for bit in 0..KEYS.len() {
                    if state.held & (1 << bit) != 0 {
                        put(&mut x, &mut at, self.held[bit] as f64);
                    }
                }
                x[count] = (at - first) as f64;
                put(&mut x, &mut at, state.look[0]);
                put(&mut x, &mut at, state.look[1]);
            }
            for row in &self.edges[..edges] {
                x[at..at + e::STRIDE].copy_from_slice(row);
                at += e::STRIDE;
            }
            let codes = self.inputs.codes().len();
            for code in 0..128 {
                put(&mut x, &mut at, if code < codes { 1. } else { 0. });
            }
            let mut bytes = [0; 1100];
            let mut pilot_entries = [bfe_sim::wire::space::PilotEntry::default(); 2];
            let mut pilot_count = 0usize;
            if receiver.swbf2_plan.as_ref().is_some_and(|p| {
                p.of(bfe_sim::wire::revisions::PILOT)
                    == bfe_sim::wire::revisions::Use::Revision(bfe_sim::wire::space::REVISION)
            }) {
                for st in &states[..commands] {
                    if let Some(controls) = st.pilot {
                        pilot_entries[pilot_count] = bfe_sim::wire::space::PilotEntry {
                            sequence: st.sequence as u32,
                            controls,
                        };
                        pilot_count += 1;
                    }
                }
            }
            self.pilot_buf.clear();
            self.records_buf.clear();
            if pilot_count > 0 {
                bfe_sim::wire::space::write_pilot_entries(
                    &pilot_entries[..pilot_count],
                    &mut self.pilot_buf,
                )
                .map_err(|e| e.to_string())?;
                let sec = [bfe_sim::wire::revisions::Section {
                    kind: bfe_sim::wire::revisions::PILOT,
                    revision: bfe_sim::wire::space::REVISION,
                    body: &self.pilot_buf,
                }];
                bfe_sim::wire::revisions::write_section(&sec, &mut self.records_buf)
                    .map_err(|e| e.to_string())?;
            }
            match encode_swbf2(op::INPUT, &mut Words::new(&x[..at]), &mut bytes, self.inputs.actions().len(), &self.records_buf) {
                Ok(length) => {
                    if endpoint.send(Lane::State, false, &bytes[..length]).map_err(|e| e.to_string())? {
                        self.sent += 1;
                        for st in &states[..commands] {
                            if st.pilot.is_some() {
                                self.history.mark_space_sent(st.sequence as u32);
                            }
                        }
                        if space_mounted {
                            self.last_sent_space_seq = latest.sequence;
                            self.prev_space_state = Some(latest);
                        }
                        let mut result = [0.; 4];
                        let words = [latest.sequence, latest.look[0], latest.look[1], latest.tick, latest.revision];
                        self.owner
                            .run(op::OUTBOUND_SENT, &mut Words::new(&words), &mut Output { values: &mut result, at: 0 })
                            .map_err(str::to_string)?;
                    }
                    break;
                }
                Err("input packet exceeds bounds") if edges != 0 => edges -= 1,
                Err(error) => return Err(error.into()),
            }
        }
        if receiver.outcomes.count != 0 {
            #[derive(Serialize)]
            struct Ack<'a> {
                #[serde(rename = "type")]
                kind: &'static str,
                epoch: u32,
                generation: u32,
                ids: &'a [u32],
            }
            let mut bytes = Bytes { values: [0; 1100], at: 0 };
            serde_json::to_writer(
                &mut bytes,
                &Ack {
                    kind: "entity-edge-ack",
                    epoch: self.life[0] as u32,
                    generation: self.life[1] as u32,
                    ids: &receiver.outcomes.values[..receiver.outcomes.count],
                },
            )
            .map_err(|e| e.to_string())?;
            if endpoint.send(Lane::State, true, &bytes.values[..bytes.at]).map_err(|e| e.to_string())? {
                receiver.outcomes.count = 0;
            }
        }
        Ok(())
    }
}
struct Bytes {
    values: [u8; 1100],
    at: usize,
}
impl std::io::Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self.at + bytes.len();
        let Some(dest) = self.values.get_mut(self.at..end) else {
            return Err(std::io::ErrorKind::WriteZero.into());
        };
        dest.copy_from_slice(bytes);
        self.at = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "sender_corpus.rs"]
mod sender_corpus;

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_transport::layout::INPUT_ACTIONS;
    #[test]
    fn pointer_preview_retains_the_command_mouse_settings() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let mut sender = Sender::default();
        let command = ClientCommand {
            mouse_scale: 0.5,
            has_mouse_scale: true,
            invert: Some(true),
            ..Default::default()
        };
        sender.tick(
            &command,
            &ClientLook { x: 12., y: -8. },
            &Default::default(),
            &receiver,
            1.,
            0.,
        );
        assert_eq!(sender.mouse_scale, 0.5);
        assert!(sender.invert);
        assert_eq!(sender.pointer_count, 1);
        let pointer = sender.pointers[sender.pointer_at];
        assert_eq!(pointer.scale, 0.5);
        assert!(pointer.invert);
        assert_eq!((pointer.x, pointer.y), (12., -8.));
    }

    // A class picked on the spawn screen before the server's first correction
    // (the moment after joining) used to be dropped, and the automatic spawn
    // then asked for the server's default class. The pick waits instead.
    #[test]
    fn a_class_picked_before_the_first_correction_is_the_one_asked_for() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.own.class = 1;
        receiver.latest = 100.;
        let mut sender = Sender::default();
        sender.auto_spawn = true;
        sender.request_spawn(&receiver, 0, 5, 3);
        sender.spawn(&receiver);
        assert!(!sender.spawned && sender.edge_count == 0, "nothing goes before the first correction");
        receiver.complete = true;
        receiver.corrections = 1;
        sender.spawn(&receiver);
        assert!(sender.spawned);
        assert_eq!(sender.edge_count, 1);
        let edge = sender.edges[0];
        assert_eq!((edge[e::KIND], edge[e::CLASS_ID], edge[e::POST_ID]), (3., 5., 3.));
        // Once sent, the pick is not sent again.
        sender.spawned = false;
        sender.spawn(&receiver);
        assert_eq!(sender.edges[1][e::CLASS_ID], 1., "a later automatic spawn takes the server's class");
        // A pick made once the correction is in goes at once, as before.
        let mut sender = Sender::default();
        sender.request_spawn(&receiver, 0, 4, 2);
        assert!(sender.spawned);
        assert_eq!((sender.edges[0][e::CLASS_ID], sender.edges[0][e::POST_ID]), (4., 2.));
    }

    // A team picked with the class before the first correction is kept with
    // it: once the correction puts the player on another team, the team edge
    // goes first, then the spawn. On the team he picked, only the spawn goes;
    // a second pick replaces the first; a spectator's or a living player's
    // pick is not kept (the automatic spawn takes the server's class).
    #[test]
    fn a_team_picked_with_the_class_before_the_first_correction_goes_first() {
        let waiting = |team: u8| {
            let mut receiver = Receiver::new(serde_json::Value::Null);
            receiver.welcomed = true;
            receiver.own.class = 1;
            receiver.own.team = team;
            receiver.latest = 100.;
            let mut sender = Sender::default();
            sender.auto_spawn = true;
            (receiver, sender)
        };
        let corrected = |receiver: &mut Receiver, team: u8| {
            receiver.complete = true;
            receiver.corrections = 1;
            receiver.own.team = team;
        };
        let (mut receiver, mut sender) = waiting(0);
        sender.request_spawn(&receiver, 2, 5, 3);
        assert_eq!(sender.edge_count, 0);
        corrected(&mut receiver, 1);
        sender.spawn(&receiver);
        assert_eq!(sender.edge_count, 2);
        assert_eq!((sender.edges[0][e::KIND], sender.edges[0][e::TEAM]), (2., 2.));
        assert_eq!((sender.edges[1][e::KIND], sender.edges[1][e::CLASS_ID], sender.edges[1][e::POST_ID]), (3., 5., 3.));
        let (mut receiver, mut sender) = waiting(0);
        sender.request_spawn(&receiver, 1, 5, 3);
        sender.request_spawn(&receiver, 1, 6, 4);
        corrected(&mut receiver, 1);
        sender.spawn(&receiver);
        assert_eq!(sender.edge_count, 1);
        assert_eq!((sender.edges[0][e::KIND], sender.edges[0][e::CLASS_ID], sender.edges[0][e::POST_ID]), (3., 6., 4.));
        for (spectator, alive) in [(true, false), (false, true)] {
            let (mut receiver, mut sender) = waiting(0);
            receiver.spectator = spectator;
            receiver.own.alive = alive;
            sender.request_spawn(&receiver, 2, 5, 3);
            receiver.spectator = false;
            receiver.own.alive = false;
            corrected(&mut receiver, 1);
            sender.spawn(&receiver);
            assert_eq!(sender.edge_count, 1, "spectator {spectator} alive {alive}");
            assert_eq!((sender.edges[0][e::KIND], sender.edges[0][e::CLASS_ID]), (3., 1.), "spectator {spectator} alive {alive}");
        }
    }

    // In a battle whose deployment is the player's choice on its screens
    // (auto_spawn off), a pick kept until a correction arrives is still the
    // one asked for, and without a pick nothing is ever asked for by
    // itself: a dead player's fire press alone must not deploy him.
    #[test]
    fn a_pick_is_sent_without_automatic_spawn_and_nothing_else_is() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.own.class = 1;
        receiver.latest = 100.;
        let mut sender = Sender::default();
        sender.auto_spawn = false;
        sender.request_spawn(&receiver, 0, 5, 3);
        sender.spawn(&receiver);
        assert_eq!(sender.edge_count, 0, "nothing goes before any correction");
        receiver.complete = true;
        receiver.corrections = 1;
        sender.spawn(&receiver);
        assert_eq!(sender.edge_count, 1, "the player's pick is asked for");
        assert_eq!((sender.edges[0][e::KIND], sender.edges[0][e::CLASS_ID], sender.edges[0][e::POST_ID]), (3., 5., 3.));
        // Dead again with no pick: the automatic spawn stays off.
        sender.spawned = false;
        sender.spawn(&receiver);
        assert_eq!(sender.edge_count, 1, "no automatic spawn follows the pick");
    }

    #[test]
    fn a_local_world_gets_commands_one_tick_above_its_next_tick() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        receiver.latest = 100.;
        let mut sender = Sender::default();
        let step = |sender: &mut Sender, next: f64| {
            sender.local_next_tick(next);
            sender.tick(&ClientCommand::default(), &ClientLook::default(), &Default::default(), &receiver, 0., 0.);
            sender.command_tick()
        };
        sender.local_lateness(0);
        assert_eq!(step(&mut sender, 101.), 102.);
        // A second command in the same frame goes to the tick after.
        assert_eq!(step(&mut sender, 101.), 103.);
        assert_eq!(step(&mut sender, 103.), 104.);
        // The world took one late: a tick more from now on.
        sender.local_lateness(1);
        assert_eq!(step(&mut sender, 104.), 106.);
    }

    #[test]
    fn online_lead_rises_with_late_commands_and_relaxes_when_on_time() {
        let mut sender = Sender::default();
        // Two ticks late: the margin rises by two at once, and not again within 30 ticks.
        sender.online_lateness(2., 1.);
        assert_eq!(sender.extra, 2.);
        sender.online_lateness(3., 10.);
        assert_eq!(sender.extra, 2.);
        sender.online_lateness(3., 25.);
        assert_eq!(sender.extra, 5.);
        // On time: nothing for 60 ticks, then a tenth of a tick per tick.
        sender.online_lateness(0., 50.);
        assert_eq!(sender.extra, 5.);
        sender.online_lateness(0., 10.);
        assert!((sender.extra - 4.).abs() < 1e-9);
        sender.online_lateness(0., 10.);
        assert!((sender.extra - 3.).abs() < 1e-9);
        // Never past the client's 24 ticks.
        sender.online_lateness(60., 60.);
        assert_eq!(sender.extra, ONLINE_MAX_LEAD);
        assert_eq!(sender.late, 4);
    }

    // The player's droideka turns with the keys: each correction (one a tick, four frames a tick)
    // brings 0.02 rad more yaw. The view must turn on every frame, by about a quarter of that, and
    // the mouse's own turn must not be delayed by it.
    #[test]
    fn a_droidekas_key_turn_is_shown_over_the_tick_and_the_mouse_stays_immediate() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.own.walker = true;
        receiver.own.alive = true;
        receiver.own.generation = 1.;
        receiver.own.zoom = 1.;
        let mut sender = Sender::default();
        sender.per_count = -0.001;
        let mut shown = Vec::new();
        for frame in 0..80 {
            if frame % 4 == 0 {
                receiver.own.tick += 1.;
                receiver.own.yaw += 0.02;
            }
            sender.yaw = receiver.own.yaw;
            sender.ease_walker_turn(&receiver, 1. / 240.);
            shown.push(sender.yaw);
        }
        for pair in shown[8..].windows(2) {
            assert!((pair[1] - pair[0] - 0.005).abs() < 1e-9, "{pair:?}");
        }
        // Three quarters of the turn are still to show when a correction arrives, none when the next is due.
        assert!((receiver.own.yaw - shown[76] - 0.015).abs() < 1e-9);
        assert!((receiver.own.yaw - shown[79]).abs() < 1e-9);
        // Then the mouse turns, 3 counts in a frame: all of it shows in the frame it arrives.
        let before = sender.yaw;
        sender.history.loose = crate::online::replay::Loose { x: 3., scale: 1., ..Default::default() };
        sender.yaw = receiver.own.yaw - 0.003;
        sender.ease_walker_turn(&receiver, 1. / 240.);
        assert!((sender.yaw - before + 0.003).abs() < 1e-9, "{}", sender.yaw - before);
        // A respawn facing another way is shown at once.
        receiver.own.generation = 2.;
        receiver.own.yaw = 2.;
        sender.yaw = 2.;
        sender.ease_walker_turn(&receiver, 1. / 240.);
        assert_eq!(sender.yaw, 2.);
    }

    #[test]
    fn droideka_correction_while_panning_shows_at_released_rate_and_between_polls() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        assert!(receiver.infantry_view.is_none());
        receiver.own.walker = true;
        receiver.own.alive = true;
        receiver.own.generation = 1.;
        receiver.own.zoom = 1.;
        receiver.own.tick = 1.;
        let mut sender = Sender::default();
        sender.per_count = -0.002;
        let dt = 1. / 144.;

        // Seed the walker state on tick 1.
        sender.yaw = 0.;
        sender.ease_walker_turn(&receiver, dt);
        assert_eq!(sender.yaw, 0.);

        // Tick 2: a -0.05 rad correction arrives while the player pans 5 counts
        // in the opposite direction (+0.01 rad mouse delta), so w.left = +0.05
        // and w.rate = 3.0 rad/s. Released easing shows 3.0 / 144 rad on this
        // frame without capping against d_mouse.
        receiver.own.tick = 2.;
        sender.history.loose = crate::online::replay::Loose { x: -5., scale: 1., ..Default::default() };
        let preview_yaw = 0.01 - 0.05;
        sender.yaw = preview_yaw;
        sender.ease_walker_turn(&receiver, dt);
        let expected_left_1 = 0.05 - 3.0 * dt;
        assert!(
            (sender.yaw - (preview_yaw + expected_left_1)).abs() < 1e-12,
            "panning frame: got yaw {} expected {}",
            sender.yaw,
            preview_yaw + expected_left_1
        );

        // Next display frame (< 50 ms later) with no new mouse counts: released
        // easing keeps advancing at w.rate * dt instead of stalling between polls.
        sender.yaw = preview_yaw;
        sender.ease_walker_turn(&receiver, dt);
        let expected_left_2 = expected_left_1 - 3.0 * dt;
        assert!(
            (sender.yaw - (preview_yaw + expected_left_2)).abs() < 1e-12,
            "inter-poll frame: got yaw {} expected {}",
            sender.yaw,
            preview_yaw + expected_left_2
        );
    }

    #[test]
    fn local_infantry_key_turn_eases_across_frames_while_mouse_stays_immediate() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.enable_local_infantry_view();
        receiver.own.walker = false;
        receiver.own.alive = true;
        receiver.own.generation = 1.;
        receiver.own.zoom = 1.;
        let mut sender = Sender::default();
        sender.per_count = -0.0015;
        let mut shown = Vec::new();
        for frame in 0..80 {
            if frame % 4 == 0 {
                receiver.own.tick += 1.;
                receiver.own.yaw += 0.024;
            }
            sender.yaw = receiver.own.yaw;
            sender.ease_walker_turn(&receiver, 1. / 240.);
            shown.push(sender.yaw);
        }
        for pair in shown[8..].windows(2) {
            assert!((pair[1] - pair[0] - 0.006).abs() < 1e-9, "{pair:?}");
        }
        let before = sender.yaw;
        sender.history.loose = crate::online::replay::Loose { x: 4., scale: 1., ..Default::default() };
        sender.yaw = receiver.own.yaw - 0.006;
        sender.ease_walker_turn(&receiver, 1. / 240.);
        assert!((sender.yaw - before + 0.006).abs() < 1e-9, "{}", sender.yaw - before);
    }

    #[test]
    fn ease_walker_turn_leaves_2004_trooper_yaw_immediate() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        assert!(receiver.infantry_view.is_none());
        receiver.own.walker = false;
        receiver.own.alive = true;
        receiver.own.generation = 1.;
        receiver.own.zoom = 1.;
        receiver.own.tick = 1.;
        receiver.own.yaw = 0.2;
        let mut sender = Sender::default();
        sender.per_count = -0.001;
        sender.yaw = receiver.own.yaw;
        sender.ease_walker_turn(&receiver, 1. / 240.);
        assert_eq!(sender.yaw, 0.2);

        receiver.own.tick = 2.;
        receiver.own.yaw = 0.24;
        sender.yaw = receiver.own.yaw;
        sender.ease_walker_turn(&receiver, 1. / 240.);
        assert_eq!(sender.yaw, 0.24);
    }

    fn test_swbf2_tuning(scales: bfe_sim::movement::soldier::swbf2::TurnScales) -> bfe_sim::movement::soldier::swbf2::Tuning {
        use bfe_sim::movement::soldier::swbf2::{ControlSpeed, Tuning};
        let row = |turn| ControlSpeed { forward: 1.0, strafe: 1.0, turn };
        Tuning {
            stand: row(scales.stand),
            crouch: row(scales.crouch),
            sprint: ControlSpeed { forward: 1.5, strafe: 0.7, turn: scales.sprint },
            jump: row(scales.jump),
            jet: row(scales.jet),
            jetpack: None,
            roll: row(scales.roll),
            tumble: row(scales.tumble),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 20.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            getup_front_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
            recover_from_tumble: true,
        }
    }

    #[test]
    fn swbf2_preview_uses_class_control_speed_turn_and_smooth_jump_transition() {
        use bfe_sim::movement::soldier::swbf2::{Roll, RollDirection, TurnScales};
        use bfe_sim::units::infantry::{Controls, Sample, TurnModel, TurnPredictor};
        let scales = TurnScales {
            stand: 1.0,
            crouch: 0.9,
            sprint: 0.5,
            jump: 0.6,
            jet: 1.25,
            roll: 0.35,
            tumble: 0.1,
        };
        let max_turn = 6.0;
        let rate = max_turn / 2048.;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.enable_local_infantry_view();
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.id = 9.;
        receiver.own.generation = 1.;
        receiver.own.alive = true;
        receiver.own.class = 1;
        receiver.own.zoom = 1.;
        let mut sender = Sender::new(&[
            ClassTurn::classic(2.0, [-0.8, 1.1]),
            ClassTurn { max_turn, pitch_limits: [-0.8, 1.1], swbf2: Some(scales) },
        ]);

        // Verify per-posture 2005 ControlSpeed turn scales in preview.
        let sample = |sprint: bool, roll: bool| Sample {
            jump_count: 0,
            land_cooldown: 0.0,
            tumble: None,
            tumble_state: None,
            id: 9,
            generation: 1.,
            energy: 100.,
            exhausted: false,
            sprint,
            sprint_time: sprint.then_some(0.1),
            roll: roll.then_some(Roll { direction: RollDirection::Forward, elapsed: 0.1 }),
            roll_seconds: 0.5,
            combo: None,
            state_time: 0.,
            anim_time: 0.,
            blades_off: 0,
            turn_factor: 1.0,
            swing_turn: 0.0,
            saber: None,
            combo_state: None,
            buttons_armed: 0,
            saber_locked: false,
        };
        for (posture, air, sprint, roll, expected) in [
            (Posture::Stand as usize, 0u8, false, false, 1.0),
            (Posture::Crouch as usize, 0, false, false, 0.9),
            (Posture::Stand as usize, 0, true, false, 0.5),
            (Posture::Stand as usize, Air::Jump as u8 + 1, false, false, 0.6),
            (Posture::Stand as usize, Air::Fall as u8 + 1, false, false, 0.6),
            (Posture::Stand as usize, Air::Jet as u8 + 1, false, false, 1.25),
            (Posture::Crouch as usize, 0, false, true, 0.35),
            (Posture::Stand as usize, Air::Thrown as u8 + 1, false, false, 0.1),
        ] {
            receiver.own.posture = posture;
            receiver.own.air = air;
            receiver.infantry = Some(sample(sprint, roll));
            sender.preview(&receiver);
            assert!(
                (-sender.per_count - rate * expected).abs() < 1e-12,
                "posture {posture} air {air} sprint {sprint} roll {roll}: {} != {}",
                -sender.per_count,
                rate * expected
            );
        }

        // Steady pan across a predicted Jump (1.0 -> 0.6) with 2-tick ack lag
        // and intermittent empty frames: exact 0.6x in air, zero reversals, zero stalls.
        let model = TurnModel {
            tuning: test_swbf2_tuning(scales),
            max_speed: 6.0,
            machine: None,
        };
        sender.set_swbf2_models(vec![None, Some(model.clone())]);
        receiver.own.posture = 0;
        receiver.own.air = 0;
        let mut auth_pred = TurnPredictor::from_sample(Posture::Stand, None, [0.; 3], 0., &sample(false, false));
        receiver.infantry = Some(sample(false, false));
        let mut auth = Soldier { max_turn, pitch_limits: [-0.8, 1.1], ..Default::default() };
        let mut queue: std::collections::VecDeque<(u32, f64, bool)> = Default::default();
        let mut loose_x = 0.;
        let mut world_tick = 0.;
        let mut prev_yaw = 0.;
        for frame in 0..80 {
            let dx = if frame % 2 == 0 { -8. } else { 0. };
            loose_x += dx;
            if frame % 4 == 0 {
                world_tick += 1.;
                if let Some((seq, px, pjump)) = queue.front().copied() {
                    if world_tick >= f64::from(seq) + 2. {
                        queue.pop_front();
                        let auth_scale = auth_pred.scale(&model);
                        auth.look_raw_scaled(px, 0., 1.0, false, auth_scale);
                        auth_pred.step(&model, 0., 0., pjump, false, Controls::default(), 0., 1. / 60.);
                        receiver.corrected_sequence = f64::from(seq);
                    }
                }
                receiver.latest = world_tick;
                receiver.own.tick = world_tick;
                receiver.own.air = auth_pred.air.map_or(0, |a| a as u8 + 1);
                receiver.own.velocity = [0., auth_pred.velocity_y, 0.];
                receiver.own.yaw = auth.aim_yaw;
                receiver.infantry = Some(Sample {
                    energy: auth_pred.movement.energy,
                    exhausted: auth_pred.movement.exhausted,
                    ..sample(false, false)
                });
                sender.feedback(&mut receiver);
                sender.local_next_tick(world_tick + 1.);
                let jump = world_tick as u32 == 4;
                let cmd = ClientCommand {
                    mouse_scale: 1.0,
                    has_mouse_scale: true,
                    invert: Some(false),
                    actions: if jump { 1 << 0 } else { 0 },
                    ..Default::default()
                };
                sender.tick_with_infantry(
                    &cmd,
                    &ClientLook { x: loose_x, y: 0. },
                    &ClientFlightCommand::default(),
                    Some(Controls::default()),
                    &receiver,
                    0.,
                    frame as f64,
                );
                queue.push_back((sender.command_sequence() as u32, loose_x, jump));
                loose_x = 0.;
            }
            sender.set_loose(&ClientLook { x: loose_x, y: 0. }, 1.0, false, (frame % 4) as f64 / 4.);
            sender.preview(&receiver);
            sender.ease_walker_turn(&receiver, 1. / 240.);
            if frame > 0 {
                let delta = sender.yaw - prev_yaw;
                assert!(delta >= -1e-12, "frame {frame} reversed by {delta} rad");
                if dx != 0. {
                    assert!(delta > 0., "frame {frame} stalled on active mouse pan");
                }
            }
            prev_yaw = sender.yaw;
        }
    }

    #[test]
    fn swbf2_pending_command_prediction_matches_authority_turn_factor_with_zero_catchup() {
        use bfe_sim::combat::combo::{ComboButton, ComboMachine, ComboState, WeaponMeleeSpec, COMBO_ANIM_FPS};
        use bfe_sim::movement::soldier::swbf2::TurnScales;
        use bfe_sim::units::infantry::{Controls, Sample, TurnModel, TurnPredictor};
        use serde_json::json;

        let machine = ComboMachine::from_json_value(
            &json!({
                "nameHash": "0000b33f",
                "states": [
                    { "name": "IDLE", "children": [
                        { "key": "Duration", "values": [0] },
                        { "key": "Transition", "values": ["SWING_A"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["Fire", "Press"] }] }
                        ]},
                        { "key": "Transition", "values": ["FROZEN"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["FireSecondary", "Press"] }] }
                        ]}
                    ]},
                    { "name": "SWING_A", "children": [
                        { "key": "Duration", "values": [0.25] },
                        { "key": "TurnFactor", "values": [0.5] },
                        { "key": "Transition", "values": ["SWING_B"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["Fire", "Press"] }] }
                        ]},
                        { "key": "Transition", "values": ["IDLE"] }
                    ]},
                    { "name": "SWING_B", "children": [
                        { "key": "Duration", "values": [0.25] },
                        { "key": "TurnFactor", "values": [0.3] },
                        { "key": "Transition", "values": ["FINISH"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["Fire", "Press"] }] }
                        ]},
                        { "key": "Transition", "values": ["IDLE"] }
                    ]},
                    { "name": "FINISH", "children": [
                        { "key": "Duration", "values": [0.25] },
                        { "key": "TurnFactor", "values": [1.0] },
                        { "key": "Transition", "values": ["COOLDOWN"] }
                    ]},
                    { "name": "COOLDOWN", "children": [
                        { "key": "Duration", "values": [0.50] },
                        { "key": "TurnFactor", "values": [0.1] },
                        { "key": "Transition", "values": ["STEP_OUT"] }
                    ]},
                    { "name": "STEP_OUT", "children": [
                        { "key": "Duration", "values": [0.35] },
                        { "key": "TurnFactor", "values": [1.0] },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]},
                    { "name": "FROZEN", "children": [
                        { "key": "Duration", "values": [0.25] },
                        { "key": "InputLock", "values": ["Turn"] },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]}
                ]
            }),
            &WeaponMeleeSpec::default(),
            COMBO_ANIM_FPS,
            |_| None,
        )
        .unwrap();

        let scales = TurnScales {
            stand: 1.0,
            crouch: 1.0,
            sprint: 0.5,
            jump: 0.6,
            jet: 1.25,
            roll: 0.35,
            tumble: 0.1,
        };
        let max_turn = 6.0;
        let rate = max_turn / 2048.0;
        let model = TurnModel {
            tuning: test_swbf2_tuning(scales),
            max_speed: 6.0,
            machine: Some(machine.clone()),
        };

        let make_sample = |pred: &TurnPredictor, m: &ComboMachine| Sample {
            jump_count: 0,
            land_cooldown: 0.0,
            tumble: None,
            tumble_state: None,
            id: 9,
            generation: 1.,
            energy: pred.movement.energy,
            exhausted: pred.movement.exhausted,
            sprint: pred.movement.sprint_time.is_some(),
            sprint_time: pred.movement.sprint_time,
            roll: pred.movement.roll,
            roll_seconds: model.tuning.roll_seconds(),
            combo: pred.combo.map(|c| c.state.0),
            state_time: pred.combo.map_or(0., |c| c.state_time),
            anim_time: pred.combo.map_or(0., |c| c.anim_time),
            blades_off: 0,
            turn_factor: pred.combo.map_or(1.0, |c| m.active_turn_factor(&c)),
            swing_turn: 0.0,
            saber: None,
            combo_state: pred.combo,
            buttons_armed: pred.buttons_armed,
            saber_locked: pred.saber_locked,
        };

        // Run at 60, 144, and 240 Hz with 2-tick ack lag:
        // Pass 1: continuous pan across 1.0 -> 0.5 -> 0.3 -> 1.0 -> 0.1 -> 1.0 (zero catch-up).
        // Pass 2: stop the mouse mid-COOLDOWN (0.1) (zero quiet frames turning).
        // Pass 3: enter FROZEN (0.0) while panning (zero drift).
        for fps in [60u32, 144, 240] {
            let dt = 1. / f64::from(fps);
            let mut receiver = Receiver::new(serde_json::Value::Null);
            receiver.enable_local_infantry_view();
            receiver.control_family = bfe_family::GameFamily::Swbf2;
            receiver.welcomed = true;
            receiver.complete = true;
            receiver.corrections = 1;
            receiver.own.id = 9.;
            receiver.own.generation = 1.;
            receiver.own.alive = true;
            receiver.own.class = 1;
            receiver.own.zoom = 1.;

            let mut sender = Sender::new(&[
                ClassTurn::classic(2.0, [-0.8, 1.1]),
                ClassTurn { max_turn, pitch_limits: [-0.8, 1.1], swbf2: Some(scales) },
            ]);
            sender.set_swbf2_models(vec![None, Some(model.clone())]);

            let init_combo = ComboState::new(&machine);
            let mut auth_pred = TurnPredictor {
                posture: Posture::Stand,
                air: None,
                grounded: true,
                lock: 0.,
                velocity_xz: [0., 0.],
                velocity_change_xz: [0., 0.],
                ramp: 0.,
                velocity_y: 0.,
                movement: bfe_sim::movement::soldier::swbf2::State::new(&model.tuning),
                combo: Some(init_combo),
                buttons_armed: 0,
                saber_locked: false,
            };
            receiver.infantry = Some(make_sample(&auth_pred, &machine));
            let mut auth = Soldier { max_turn, pitch_limits: [-0.8, 1.1], ..Default::default() };
            let mut queue: std::collections::VecDeque<(u32, f64, f64, Controls)> = Default::default();
            let mut loose_x = 0.;
            let mut world_tick = 0u32;
            let mut prev_yaw = 0.;
            let mut prev_tf = 1.0_f64;
            let mut by_state: [(f64, f64); 7] = [(0., 0.); 7];
            let mut quiet_moving = 0u32;
            let fire = ComboButton::Fire.bit();

            let total_frames = (fps as usize) * 4;
            for frame in 0..total_frames {
                let t = frame as f64 * dt;
                // Pan continuously through combo 1 (0..2.1s), pause inside combo 2 COOLDOWN (2.9..3.4s),
                // then pan through FROZEN (3.4..4.0s).
                let panning = t < 2.1 || t >= 3.4;
                let dx = if panning { 6.0 } else { 0.0 };
                loose_x += dx;

                let due = ((frame + 1) as f64 * dt * 60.).floor() as u32 > world_tick;
                if due {
                    world_tick += 1;
                    // Run any command stamped for `world_tick` (2-tick lead: acked after 2 ticks).
                    while let Some(&(seq, stamp, px, ctrls)) = queue.front() {
                        if f64::from(world_tick) >= stamp {
                            queue.pop_front();
                            let auth_scale = auth_pred.scale(&model);
                            auth.look_raw_scaled(px, 0., 1.0, false, auth_scale);
                            auth_pred.step(&model, 0., 0., false, false, ctrls, 0., 1. / 60.);
                            receiver.corrected_sequence = f64::from(seq);
                        } else {
                            break;
                        }
                    }
                    receiver.latest = f64::from(world_tick);
                    receiver.own.tick = f64::from(world_tick);
                    receiver.own.yaw = auth.aim_yaw;
                    receiver.infantry = Some(make_sample(&auth_pred, &machine));
                    sender.feedback(&mut receiver);
                    sender.local_next_tick(f64::from(world_tick + 1));

                    // Fire presses for combo 1 (ticks 5, 15, 28), combo 2 (ticks 110, 120, 133),
                    // and secondary press for FROZEN (tick 215, after combo 2 returns to IDLE).
                    let press_fire = matches!(world_tick, 5 | 15 | 28 | 110 | 120 | 133);
                    let press_block = world_tick == 215;
                    let ctrls = Controls {
                        melee_pressed: if press_fire { fire } else { 0 },
                        melee_held: if press_fire { fire } else { 0 },
                        block_pressed: press_block,
                        block: press_block,
                        ..Default::default()
                    };
                    let cmd = ClientCommand {
                        mouse_scale: 1.0,
                        has_mouse_scale: true,
                        invert: Some(false),
                        ..Default::default()
                    };
                    sender.tick_with_infantry(
                        &cmd,
                        &ClientLook { x: loose_x, y: 0. },
                        &ClientFlightCommand::default(),
                        Some(ctrls),
                        &receiver,
                        0.,
                        t * 1000.,
                    );
                    queue.push_back((
                        sender.command_sequence() as u32,
                        sender.command_tick(),
                        loose_x,
                        ctrls,
                    ));
                    loose_x = 0.;
                }
                sender.set_loose(&ClientLook { x: loose_x, y: 0. }, 1.0, false, 0.5);
                sender.preview(&receiver);
                sender.ease_walker_turn(&receiver, dt);

                if frame > 0 {
                    let delta = sender.yaw - prev_yaw;
                    if dx > 0. {
                        // A frame's mouse counts (whether loose or packed into this tick's
                        // command) use the TurnFactor active before this tick's step.
                        let tf = prev_tf;
                        let st = if (tf - 0.5).abs() < 1e-6 {
                            1
                        } else if (tf - 0.3).abs() < 1e-6 {
                            2
                        } else if (tf - 0.1).abs() < 1e-6 {
                            4
                        } else if tf.abs() < 1e-6 {
                            6
                        } else {
                            0
                        };
                        by_state[st].0 += dx;
                        by_state[st].1 += -delta;
                        assert!(delta <= 1e-12, "fps={fps} frame={frame} reversed: {delta}");
                        if tf > 0. {
                            let expected_step = dx * rate * tf;
                            assert!(
                                (-delta - expected_step).abs() < 1e-9,
                                "fps={fps} frame={frame} tf={tf}: got {} expected {}",
                                -delta,
                                expected_step
                            );
                        } else {
                            assert!(delta.abs() < 1e-12, "fps={fps} frame={frame} drifted in TurnFactor=0: {delta}");
                        }
                    } else if delta.abs() > 1e-12 {
                        quiet_moving += 1;
                    }
                }
                prev_yaw = sender.yaw;
                prev_tf = (-sender.per_count) / rate;
            }

            assert_eq!(quiet_moving, 0, "fps={fps}: quiet frames turned after mouse stop");
            for (idx, expected_tf) in [(0usize, 1.0), (1, 0.5), (2, 0.3), (4, 0.1), (6, 0.0)] {
                let (cnt, dy) = by_state[idx];
                assert!(cnt > 0., "fps={fps} state {idx} had no samples");
                let ratio = (dy / cnt) / rate;
                assert!(
                    (ratio - expected_tf).abs() < 1e-9,
                    "fps={fps} state {idx}: ratio {ratio:.6} != {expected_tf:.6}"
                );
            }
            // Drain remaining queue with zero mouse and confirm exact convergence.
            while let Some((seq, _stamp, px, ctrls)) = queue.pop_front() {
                let auth_scale = auth_pred.scale(&model);
                auth.look_raw_scaled(px, 0., 1.0, false, auth_scale);
                auth_pred.step(&model, 0., 0., false, false, ctrls, 0., 1. / 60.);
                receiver.corrected_sequence = f64::from(seq);
            }
            receiver.latest += 1.;
            receiver.own.tick = receiver.latest;
            receiver.own.yaw = auth.aim_yaw;
            receiver.infantry = Some(make_sample(&auth_pred, &machine));
            sender.feedback(&mut receiver);
            sender.set_loose(&ClientLook::default(), 1.0, false, 0.);
            sender.preview(&receiver);
            sender.ease_walker_turn(&receiver, dt);
            assert!(
                (sender.yaw - auth.aim_yaw).abs() < 1e-12,
                "fps={fps}: preview={} != auth={}",
                sender.yaw,
                auth.aim_yaw
            );
        }
    }

    #[test]
    fn changing_to_a_2005_gunner_seat_stops_flight_commands() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.control_family = bfe_family::GameFamily::Swbf2;
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        receiver.own.mount_kind = bfe_presentation::network_layout::net_kind::VEHICLE;
        receiver.own.flight = Some(0);
        let flight = ClientFlightCommand {
            held: 1 << ClientFlightHeld::Boost as u8,
            ..Default::default()
        };
        let mut sender = Sender::default();
        for seat in [0, 2, 0] {
            receiver.own.mount = Some((14, 1, seat));
            sender.tick(&ClientCommand::default(), &ClientLook::default(), &flight, &receiver, 0., 0.);
            assert_eq!(sender.flight.is_some(), seat == 0);
            assert_eq!(sender.latest.unwrap().held & (1 << 9) != 0, seat == 0);
        }
    }

    // A pilot's take-off press and boost reach the server as they reach an
    // Instant Action world; on foot the same flight bits send nothing.
    #[test]
    fn a_pilots_flight_keys_go_on_the_wire() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let flight = ClientFlightCommand {
            held: 1 << ClientFlightHeld::Boost as u8,
            actions: 1 << ClientFlightAction::Takeoff as u8,
            scale: 1.,
            curve: 1.35,
            ..Default::default()
        };
        let takeoff = INPUT_ACTIONS.iter().position(|a| *a == "takeoff").unwrap() as f64;
        let sent = |receiver: &Receiver| {
            let mut sender = Sender::default();
            sender.tick(&ClientCommand::default(), &ClientLook::default(), &flight, receiver, 0., 0.);
            let actions: Vec<f64> = sender.edges[..sender.edge_count].iter().map(|row| row[e::ACTION]).collect();
            (actions, sender.latest.unwrap().held, sender.flight.is_some())
        };
        assert_eq!(sent(&receiver), (vec![], 0, false));
        receiver.own.flight = Some(0);
        assert_eq!(sent(&receiver), (vec![takeoff], 1 << 9, true));
    }

    #[test]
    fn preview_locks_yaw_and_pitch_while_own_lock_is_positive() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.alive = true;
        receiver.own.generation = 1.;
        receiver.own.class = 0;
        receiver.own.zoom = 1.;
        receiver.own.yaw = 0.75;
        receiver.own.pitch = -0.15;
        receiver.own.lock = 0.8;
        let mut sender = Sender::default();
        sender.tick(
            &ClientCommand::default(),
            &ClientLook { x: 120., y: -80. },
            &ClientFlightCommand::default(),
            &receiver,
            0.,
            0.,
        );
        sender.set_loose(&ClientLook { x: 40., y: -30. }, 1., false, 0.5);
        sender.preview(&receiver);
        assert_eq!((sender.yaw, sender.pitch), (0.75, -0.15), "stand-up lock must freeze preview yaw and pitch");
        // Once own.lock clears (e.g. during Thrown/Tumble/Landed or after GetUp), preview turns freely again.
        receiver.own.lock = 0.0;
        sender.preview(&receiver);
        assert_ne!(sender.yaw, 0.75);
        assert_ne!(sender.pitch, -0.15);
    }

    #[test]
    fn preview_scales_yaw_by_class_tumble_turn_across_tumble_phases() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.id = 4.;
        receiver.own.alive = true;
        receiver.own.generation = 2.;
        receiver.own.class = 0;
        receiver.own.zoom = 1.;
        receiver.own.yaw = 0.;
        receiver.own.pitch = 0.;
        let turns = [ClassTurn {
            max_turn: 4.,
            pitch_limits: [-1., 1.],
            swbf2: Some(bfe_sim::movement::soldier::swbf2::TurnScales {
                stand: 1., crouch: 1., sprint: 1., jump: 0.5, jet: 1., roll: 1., tumble: 0.12,
            }),
        }];
        let mut sender = Sender::new(&turns);

        // Standing baseline via loose + pending pointer.
        sender.set_loose(&ClientLook { x: 100., y: -40. }, 1., false, 0.5);
        sender.preview(&receiver);
        let stand_dyaw = sender.yaw;
        let stand_dpitch = sender.pitch;
        assert!(stand_dyaw.abs() > 0.01 && stand_dpitch.abs() > 0.01);

        let sample_with = |tumble: Option<TumblePhase>| bfe_sim::units::infantry::Sample {
            jump_count: 0,
            land_cooldown: 0.0,
            turn_factor: 1.0,
            swing_turn: 0.0,
            id: 4,
            generation: 2.,
            energy: 100.,
            sprint: false,
            roll: None,
            roll_seconds: 0.,
            tumble,
            tumble_state: None,
            combo: None,
            state_time: 0.,
            anim_time: 0.,
            blades_off: 0,
            saber: None,
            exhausted: false,
            sprint_time: None,
            combo_state: None,
            buttons_armed: 0,
            saber_locked: false,
        };

        for phase in [TumblePhase::Thrown, TumblePhase::Tumble, TumblePhase::Landed] {
            receiver.own.air = if phase == TumblePhase::Thrown { Air::Thrown as u8 + 1 } else { 0 };
            receiver.infantry = Some(sample_with(Some(phase)));
            let mut s = Sender::new(&turns);
            s.tick(
                &ClientCommand::default(),
                &ClientLook { x: 60., y: -20. },
                &ClientFlightCommand::default(),
                &receiver,
                0.,
                0.,
            );
            s.set_loose(&ClientLook { x: 40., y: -20. }, 1., false, 0.5);
            s.preview(&receiver);
            assert!(
                (s.yaw - stand_dyaw * 0.12).abs() < 1e-12,
                "phase {phase:?} preview yaw {} must equal 0.12x standing {}",
                s.yaw,
                stand_dyaw * 0.12,
            );
            assert!((s.pitch - stand_dpitch).abs() < 1e-12);
        }

        // Recover does not apply tumble look scaling.
        receiver.own.air = Air::Fall as u8 + 1;
        receiver.infantry = Some(sample_with(Some(TumblePhase::Recover)));
        sender.set_loose(&ClientLook { x: 100., y: 0. }, 1., false, 0.5);
        sender.preview(&receiver);
        assert!((sender.yaw - stand_dyaw * 0.5).abs() < 1e-12);
    }

    // A 2005 soldier's sprint, block and roll reach the wire on foot as the
    // wire codes and IDs of the two-game contract; a 2004 battle given the
    // same command sends exactly what it sent before.
    #[test]
    fn only_a_2005_soldier_sends_its_own_inputs() {
        use crate::generated_input::{ClientAction, ClientHeld};
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let command = ClientCommand {
            held: 1 << ClientHeld::Forward as u8 | 1 << ClientHeld::Sprint as u8 | 1 << ClientHeld::Block as u8,
            actions: 1 << ClientAction::Jump as u8 | 1 << ClientAction::Roll as u8,
            ..Default::default()
        };
        let sent = |inputs: WireInputs, receiver: &Receiver| {
            let mut sender = Sender::for_inputs(inputs);
            sender.tick(&command, &ClientLook::default(), &ClientFlightCommand::default(), receiver, 0., 0.);
            let actions: Vec<f64> = sender.edges[..sender.edge_count].iter().map(|row| row[e::ACTION]).collect();
            let held = sender.latest.unwrap().held;
            let codes: Vec<&str> = (0..KEYS.len()).filter(|bit| held & (1 << bit) != 0)
                .map(|bit| inputs.codes()[sender.held[bit]]).collect();
            (actions, codes)
        };
        let old = WireInputs::new(bfe_family::GameFamily::Swbf1);
        let new = WireInputs::new(bfe_family::GameFamily::Swbf2);
        assert_eq!(sent(old, &receiver), (vec![0.], vec!["KeyW"]));
        assert_eq!(sent(new, &receiver), (vec![0., 21., 22.], vec!["KeyW", "ShiftLeft"]));
        // Flying, the 2005 soldier's foot inputs stay home too.
        receiver.own.flight = Some(0);
        assert_eq!(sent(new, &receiver), (vec![0.], vec!["KeyW"]));
    }

    // A 2005 soldier's crouch and reload holds and his hold-to-block setting
    // reach the wire as their own codes on foot, and a block tap between two
    // commands as its own edges, in order. A 2004 battle sends none of it.
    #[test]
    fn a_2005_soldiers_holds_setting_and_block_taps_go_on_the_wire() {
        use crate::generated_input::ClientHeld;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let (press, release) = (22., 23.);
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf2));
        let mut step = |sender: &mut Sender, receiver: &Receiver, block: bool, soldier: SoldierInput| {
            let command = ClientCommand { held: if block { 1 << ClientHeld::Block as u8 } else { 0 }, ..Default::default() };
            sender.soldier = soldier;
            sender.edge_count = 0;
            sender.tick(&command, &ClientLook::default(), &ClientFlightCommand::default(), receiver, 0., 0.);
            let held = sender.latest.unwrap().held;
            let codes: Vec<&str> = (0..KEYS.len()).filter(|bit| held & (1 << bit) != 0).map(|bit| sender.inputs.codes()[sender.held[bit]]).collect();
            (sender.edges[..sender.edge_count].iter().map(|row| row[e::ACTION]).collect::<Vec<_>>(), codes)
        };
        let tap = SoldierInput { block_pressed: true, ..Default::default() };
        let held = SoldierInput { crouch: true, reload: true, hold_to_block: true, ..Default::default() };
        assert_eq!(step(&mut sender, &receiver, false, held), (vec![], vec!["KeyC", "KeyR", "KeyB"]));
        assert_eq!(step(&mut sender, &receiver, false, tap), (vec![press, release], vec![]));
        assert_eq!(step(&mut sender, &receiver, true, tap), (vec![press], vec![]));
        assert_eq!(step(&mut sender, &receiver, true, Default::default()), (vec![], vec![]));
        assert_eq!(step(&mut sender, &receiver, true, tap), (vec![release, press], vec![]));
        assert_eq!(step(&mut sender, &receiver, false, tap), (vec![release, press, release], vec![]));
        assert_eq!(step(&mut sender, &receiver, false, Default::default()), (vec![], vec![]));
        // Flying they stay home, and a held block is let go.
        assert_eq!(step(&mut sender, &receiver, true, held).0, vec![press]);
        receiver.own.flight = Some(0);
        assert_eq!(step(&mut sender, &receiver, true, SoldierInput { block_pressed: true, ..held }), (vec![release], vec![]));
        receiver.own.flight = None;
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf1));
        assert_eq!(step(&mut sender, &receiver, true, SoldierInput { block_pressed: true, ..held }), (vec![], vec![]));
    }

    // A block still held when a new life starts is pressed again in it.
    #[test]
    fn a_block_held_into_a_new_life_is_pressed_again() {
        use crate::generated_input::ClientHeld;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let command = ClientCommand { held: 1 << ClientHeld::Block as u8, ..Default::default() };
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf2));
        let sent = |sender: &mut Sender, receiver: &mut Receiver| {
            sender.feedback(receiver);
            sender.tick(&command, &ClientLook::default(), &ClientFlightCommand::default(), receiver, 0., 0.);
            sender.edges[..sender.edge_count].iter().map(|row| (row[e::ID], row[e::ACTION])).collect::<Vec<_>>()
        };
        // The press, still unacknowledged while the control stays down.
        assert_eq!(sent(&mut sender, &mut receiver), vec![(1., 22.)]);
        assert_eq!(sent(&mut sender, &mut receiver), vec![(1., 22.)]);
        receiver.own.generation = 2.;
        assert_eq!(sent(&mut sender, &mut receiver), vec![(1., 22.)]);
    }

    // A 2005 battle's receiver and sender: its text handler is the 2005 one.
    fn pair_2005() -> (Sender, Receiver) {
        use bfe_sim::wire::revisions::{Plan, Use, INPUT, SOLDIER};
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.swbf2_plan = Some(Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1))] });
        (Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf2)), receiver)
    }

    // The commands of a run under jitter, one row each: the tick the command
    // was stamped for, how many the server ran late so far, by how many ticks
    // in all, and the extra lead the sender then holds. Every third command is
    // named by a snapshot (no timing) before its input-ack arrives.
    fn lead_rows(sender: &mut Sender, receiver: &mut Receiver) -> Vec<(f64, u64, f64, f64)> {
        const LATE: [f64; 24] = [0., 0., 3., 0., 0., 5., 0., 1., 0., 0., 4., 0., 0., 0., 2., 0., 0., 6., 0., 0., 0., 0., 3., 0.];
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.epoch = 1.;
        receiver.own.generation = 1.;
        receiver.latest = 100.;
        let mut rows = Vec::new();
        for (step, late) in LATE.into_iter().enumerate() {
            let now = 1000. + 200. * step as f64;
            sender.tick(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default(), receiver, 40., now);
            let (sequence, stamped) = (sender.command_sequence(), sender.command_tick());
            if step % 3 == 1 {
                receiver.ack = sequence;
                sender.feedback(receiver);
            }
            let server = 100. + 12. * (step + 1) as f64;
            let ack = format!(r#"{{"type":"input-ack","epoch":1,"generation":1,"ack":{sequence},"ackTick":{},"serverTick":{server}}}"#, stamped + late);
            receiver.message(true, ack.as_bytes(), now + 20.);
            sender.feedback(receiver);
            rows.push((stamped, sender.late, sender.late_ticks, sender.extra_lead()));
        }
        rows
    }

    // A 2004 sender keeps the released lead: the rows below are what the
    // released build's sender gave for this same run (its lateness is read on
    // the newest acknowledgement of any kind, so a command a snapshot named
    // first is never measured late).
    #[test]
    fn a_2004_sender_keeps_the_released_lead_under_jitter() {
        const RELEASED: [(f64, u64, f64, f64); 24] = [
            (104., 0, 0., 0.), (127., 0, 0., 0.), (139., 1, 3., 3.), (154., 1, 3., 3.),
            (166., 1, 3., 3.), (178., 2, 8., 8.), (195., 2, 8., 8.), (207., 2, 8., 8.),
            (219., 2, 8., 8.), (231., 2, 8., 8.), (243., 2, 8., 8.), (255., 2, 8., 5.6),
            (265., 2, 8., 4.3999999999999995), (276., 2, 8., 4.3), (288., 3, 10., 6.3), (302., 3, 10., 6.3),
            (314., 3, 10., 6.3), (326., 4, 16., 12.3), (344., 4, 16., 12.3), (356., 4, 16., 12.3),
            (368., 4, 16., 12.3), (380., 4, 16., 12.3), (392., 4, 16., 12.3), (404., 4, 16., 9.9),
        ];
        let mut receiver = Receiver::new(serde_json::Value::Null);
        let mut sender = Sender::default();
        assert_eq!(lead_rows(&mut sender, &mut receiver), RELEASED);
    }

    // A 2005 sender measures every command an input-ack names, whether or not
    // a snapshot named it first.
    #[test]
    fn a_2005_sender_measures_the_commands_a_snapshot_named_first() {
        let (mut sender, mut receiver) = pair_2005();
        let rows = lead_rows(&mut sender, &mut receiver);
        // Seven commands were run late, by 3, 5, 1, 4, 2, 6 and 3 ticks.
        assert_eq!(rows.last().map(|row| (row.1, row.2)), Some((7, 24.)));
    }

    // A snapshot carrying ACK_INPUT before the matching reliable input-ack must
    // not suppress lateness measurement when that input-ack arrives.
    #[test]
    fn a_snapshot_arriving_before_input_ack_still_measures_lateness() {
        let (mut sender, mut receiver) = pair_2005();
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.epoch = 1.;
        receiver.own.generation = 1.;
        receiver.latest = 100.;
        sender.tick(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default(), &receiver, 30., 1000.);
        let stamped = sender.command_tick();
        // Snapshot arrives first and advances receiver.ack without ack_tick.
        receiver.ack = 1.;
        sender.feedback(&mut receiver);
        assert_eq!(sender.late, 0);
        // The reliable input-ack arrives afterwards reporting the command ran 4 ticks late.
        let ack = format!(r#"{{"type":"input-ack","epoch":1,"generation":1,"ack":1,"ackTick":{},"serverTick":{}}}"#, stamped + 4., stamped + 5.);
        receiver.message(true, ack.as_bytes(), 1020.);
        sender.feedback(&mut receiver);
        assert_eq!((sender.late, sender.late_ticks, sender.extra_lead()), (1, 4., 4.));
    }

    // The same case in a 2004 battle reads as released: the snapshot's
    // acknowledgement came first, so the command is not measured.
    #[test]
    fn a_snapshot_arriving_before_input_ack_is_not_measured_in_a_2004_battle() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.epoch = 1.;
        receiver.own.generation = 1.;
        receiver.latest = 100.;
        let mut sender = Sender::default();
        sender.tick(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default(), &receiver, 30., 1000.);
        let stamped = sender.command_tick();
        receiver.ack = 1.;
        sender.feedback(&mut receiver);
        let ack = format!(r#"{{"type":"input-ack","epoch":1,"generation":1,"ack":1,"ackTick":{},"serverTick":{}}}"#, stamped + 4., stamped + 5.);
        receiver.message(true, ack.as_bytes(), 1020.);
        sender.feedback(&mut receiver);
        assert_eq!((sender.late, sender.late_ticks, sender.extra_lead()), (0, 0., 0.));
    }

    #[test]
    fn a_ground_seat_suppresses_block_edges_and_releases_held_block() {
        use crate::generated_input::ClientHeld;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let (press, release) = (22., 23.);
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf2));
        let step = |sender: &mut Sender, receiver: &Receiver, block: bool, tapped: bool| {
            let command = ClientCommand { held: if block { 1 << ClientHeld::Block as u8 } else { 0 }, ..Default::default() };
            sender.soldier = SoldierInput { block_pressed: tapped, ..Default::default() };
            sender.edge_count = 0;
            sender.tick(&command, &ClientLook::default(), &ClientFlightCommand::default(), receiver, 0., 0.);
            sender.edges[..sender.edge_count].iter().map(|row| row[e::ACTION]).collect::<Vec<_>>()
        };
        assert_eq!(step(&mut sender, &receiver, true, false), vec![press]);
        // Boarding a ground seat releases block and suppresses taps while seated.
        receiver.own.mount = Some((14, 1, 0));
        assert_eq!(step(&mut sender, &receiver, true, true), vec![release]);
        assert_eq!(step(&mut sender, &receiver, true, true), Vec::<f64>::new());
        // Disembarking while still holding block presses again on foot.
        receiver.own.mount = None;
        assert_eq!(step(&mut sender, &receiver, true, false), vec![press]);
    }

    #[test]
    fn a_2005_soldier_sends_his_power_lock_press_on_foot_and_a_2004_one_never() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1.;
        let step = |sender: &mut Sender, receiver: &Receiver, pressed: bool| {
            sender.soldier = SoldierInput { lock_pressed: pressed, ..Default::default() };
            sender.edge_count = 0;
            sender.tick(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default(), receiver, 0., 0.);
            sender.edges[..sender.edge_count].iter().map(|row| row[e::ACTION]).collect::<Vec<_>>()
        };
        let lock = bfe_transport::layout::SWBF2_INPUT_ACTIONS.iter().position(|a| *a == "targetLock").unwrap() as f64;
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf2));
        assert_eq!(step(&mut sender, &receiver, true), vec![lock]);
        assert_eq!(step(&mut sender, &receiver, false), Vec::<f64>::new());
        // In a ground seat the press is a vehicle's, never a soldier's lock.
        receiver.own.mount = Some((14, 1, 0));
        assert_eq!(step(&mut sender, &receiver, true), Vec::<f64>::new());
        receiver.own.mount = None;
        let mut sender = Sender::for_inputs(WireInputs::new(bfe_family::GameFamily::Swbf1));
        assert_eq!(step(&mut sender, &receiver, true), Vec::<f64>::new());
    }


    #[test]
    fn swbf2_drawn_turn_rate_matches_simulated_across_all_states_and_tumble_at_60_and_144_hz() {
        use bfe_content::{
            bvh::{build, Positions},
            collision::{triangle_bounds, MapCollision, TriangleGrid},
        };
        use bfe_sim::collision::CollisionOwner;
        use bfe_sim::movement::soldier::{
            swbf2::{Actions, State as Swbf2State, Step, Tumble, TumblePhase, TurnScales},
            Air as MoveAir, Input as MoveInput, Posture as MovePosture, V3,
        };
        use bfe_sim::movement::MovementOwner;
        use bfe_sim::units::infantry::{look_turn_scale, Controls, Sample, TurnModel};

        let scales = TurnScales {
            stand: 1.0,
            crouch: 0.85,
            sprint: 0.5,
            jump: 0.6,
            jet: 1.25,
            roll: 0.35,
            tumble: 0.12,
        };
        let max_turn = 6.0;
        let rate = max_turn / 2048.0;
        let sim_dt = 1.0 / 60.0;

        let make_flat_world = || {
            let floor = vec![
                -200.0f32, 0.0, -200.0, 200.0, 0.0, -200.0, 200.0, 0.0, 200.0, -200.0, 0.0, 200.0,
            ];
            let building = vec![
                -6.0f32, -100.0, -6.0, 6.0, -100.0, -6.0, 6.0, -100.0, 6.0, -6.0, -100.0, 6.0,
            ];
            let indices = vec![0u32, 2, 1, 0, 3, 2];
            let buildings = build(Positions::F32(&building), &indices);
            let terrain = build(Positions::F32(&floor), &indices);
            let map = MapCollision {
                tri_bounds: triangle_bounds(&building, &indices),
                grid: Some(TriangleGrid::new(&floor, &indices)),
                positions: building.into(),
                indices: indices.clone().into(),
                polygons: vec![0, 1].into(),
                masks: Vec::new().into(),
                has_terrain: true,
                terrain_positions: floor.into(),
                terrain_indices: indices.into(),
                kill_y: -200.0,
            };
            let mut col = CollisionOwner::new();
            let world = bfe_content::install::install(&mut col, &map, &buildings, Some(&terrain))
                .unwrap()
                .world as usize;
            bfe_sim::collision::broad::prepare(&mut col.state, world);
            (world, col.state)
        };

        let to_gen_posture = |p: MovePosture| match p {
            MovePosture::Stand => Posture::Stand,
            MovePosture::Crouch => Posture::Crouch,
            MovePosture::Prone => Posture::Prone,
        };
        let to_gen_air = |a: MoveAir| match a {
            MoveAir::None => None,
            MoveAir::Jump => Some(Air::Jump),
            MoveAir::Fall => Some(Air::Fall),
            MoveAir::Thrown => Some(Air::Thrown),
            MoveAir::Jet => Some(Air::Jet),
        };

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum StateTag {
            Stand = 0,
            Crouch = 1,
            Sprint = 2,
            Jump = 3,
            Roll = 4,
            Thrown = 5,
            Tumble = 6,
            Landed = 7,
            GetUp = 8,
            Recover = 9,
        }
        const NAMES: [&str; 10] = [
            "Stand", "Crouch", "Sprint", "Jump", "Roll", "Thrown", "Tumble", "Landed", "GetUp",
            "Recover",
        ];
        const EXPECTED_SCALES: [f64; 10] = [1.0, 0.85, 0.5, 0.6, 0.35, 0.12, 0.12, 0.12, 0.0, 0.6];

        let classify = |posture: Posture, air: Option<Air>, st: &Swbf2State, lock: f64| -> StateTag {
            if let Some(t) = st.tumble {
                match t.phase {
                    TumblePhase::Thrown => StateTag::Thrown,
                    TumblePhase::Tumble => StateTag::Tumble,
                    TumblePhase::Landed => StateTag::Landed,
                    TumblePhase::GetUp => StateTag::GetUp,
                    TumblePhase::Recover => StateTag::Recover,
                }
            } else if lock > 0.0 {
                StateTag::GetUp
            } else if air == Some(Air::Thrown) {
                StateTag::Thrown
            } else if st.roll.is_some() {
                StateTag::Roll
            } else if matches!(air, Some(Air::Jump | Air::Fall)) {
                StateTag::Jump
            } else if st.sprint_time.is_some() {
                StateTag::Sprint
            } else if posture == Posture::Crouch {
                StateTag::Crouch
            } else {
                StateTag::Stand
            }
        };

        let make_sample = |st: &Swbf2State, tuning: &bfe_sim::movement::soldier::swbf2::Tuning| Sample {
            id: 9,
            generation: 1.0,
            energy: st.energy,
            exhausted: st.exhausted,
            sprint: st.sprint_time.is_some(),
            sprint_time: st.sprint_time,
            roll: st.roll,
            roll_seconds: tuning.roll_seconds(),
            jump_count: st.jump_count,
            land_cooldown: st.land_cooldown,
            tumble: st.tumble.map(|t| t.phase),
            tumble_state: st.tumble,
            combo: None,
            state_time: 0.0,
            anim_time: 0.0,
            blades_off: 0,
            turn_factor: 1.0,
            swing_turn: 0.0,
            saber: None,
            combo_state: None,
            buttons_armed: 0,
            saber_locked: false,
        };

        for fps in [60u32, 144] {
            let dt = 1.0 / f64::from(fps);
            // Per-state accumulators across both runs:
            // (panning_counts, drawn_dyaw_rad, sim_counts, sim_dyaw_rad, max_frame_err, reversals, quiet_moves)
            let mut stats: [(f64, f64, f64, f64, f64, u32, u32); 10] = [(0., 0., 0., 0., 0., 0, 0); 10];

            for (run_idx, recover_from_tumble) in [(0u32, false), (1u32, true)] {
                let mut tuning = test_swbf2_tuning(scales);
                tuning.recover_from_tumble = recover_from_tumble;
                let model = TurnModel {
                    tuning,
                    max_speed: 6.0,
                    machine: None,
                };
                let (world, mut col) = make_flat_world();
                let mut movement = MovementOwner::new();
                movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
                let mover_id = movement.new_mover(world, |m| {
                    m.grounded = true;
                    m.max_speed = 6.0;
                    m.max_strafe = 5.0;
                    m.max_turn = max_turn;
                    m.accel = 70.0;
                    m.dive_time = 0.6;
                });
                let mut auth_state = Swbf2State::new(&tuning);
                let mut auth_soldier = Soldier {
                    max_turn,
                    pitch_limits: [-0.8, 1.1],
                    grounded: true,
                    ..Default::default()
                };

                let mut receiver = Receiver::new(serde_json::Value::Null);
                receiver.enable_local_infantry_view();
                receiver.control_family = bfe_family::GameFamily::Swbf2;
                receiver.welcomed = true;
                receiver.complete = true;
                receiver.corrections = 1;
                receiver.own.id = 9.0;
                receiver.own.generation = 1.0;
                receiver.own.alive = true;
                receiver.own.class = 1;
                receiver.own.zoom = 1.0;
                receiver.infantry = Some(make_sample(&auth_state, &tuning));

                let mut sender = Sender::new(&[
                    ClassTurn::classic(2.0, [-0.8, 1.1]),
                    ClassTurn { max_turn, pitch_limits: [-0.8, 1.1], swbf2: Some(scales) },
                ]);
                sender.set_swbf2_models(vec![None, Some(model.clone())]);

                struct Queued {
                    seq: u32,
                    stamp: f64,
                    px: f64,
                    py: f64,
                    forward: f64,
                    jump: bool,
                    crouch: bool,
                    controls: Controls,
                }
                let mut queue: std::collections::VecDeque<Queued> = Default::default();
                let mut loose_x = 0.0;
                let mut loose_y = 0.0;
                let mut world_tick = 0u32;
                let mut prev_yaw = 0.0;
                let mut prev_pitch = 0.0;
                let mut prev_tag = StateTag::Stand;
                let mut prev_expected = 1.0;
                let mut settle_frames = 0u32;
                let mut pushed = false;
                let mut quiet_start_tick: Option<u32> = None;

                let total_ticks = if recover_from_tumble { 140u32 } else { 470u32 };
                let total_frames = ((total_ticks as f64 / 60.0) * f64::from(fps)).ceil() as usize;

                for frame in 0..total_frames {
                    let t = frame as f64 * dt;
                    // Pause mouse briefly inside Tumble to verify zero quiet-frame turning while
                    // keeping active panning across Landed -> GetUp -> Stand transitions.
                    let in_quiet = quiet_start_tick
                        .is_some_and(|start| world_tick >= start && world_tick < start + 8);
                    let dx = if in_quiet { 0.0 } else { 6.0 };
                    let dy = if in_quiet { 0.0 } else { -1.5 };
                    loose_x += dx;
                    loose_y += dy;

                    let due = ((frame + 1) as f64 * dt * 60.0).floor() as u32 > world_tick;
                    if due {
                        world_tick += 1;
                        // Trigger an external Force Push at tick 210 (run 0: Thrown -> Tumble -> Landed -> GetUp -> Stand)
                        // or tick 15 (run 1: Thrown -> Recover -> Fall).
                        let push_tick = if recover_from_tumble { 15 } else { 210 };
                        if world_tick == push_tick && !pushed {
                            pushed = true;
                            let v = if recover_from_tumble {
                                V3::new(0.0, 14.0, 4.0)
                            } else {
                                V3::new(0.0, 5.5, 13.0)
                            };
                            let m = movement.mover(mover_id);
                            m.grounded = false;
                            m.air = MoveAir::Thrown;
                            m.velocity = v;
                            auth_state.roll = None;
                            auth_state.sprint_time = None;
                            auth_state.tumble = Some(Tumble::enter(v, auth_soldier.aim_yaw, 1));
                            // External impulse changes state for 2 in-flight ticks already sent before push;
                            // allow 3 ticks (5 frames at 60 Hz, 10 at 144 Hz) for those pre-push ticks to ack.
                            settle_frames = (fps / 15).max(4);
                        }

                        while let Some(q) = queue.front() {
                            if f64::from(world_tick) >= q.stamp {
                                let q = queue.pop_front().unwrap();
                                let m_before = movement.mover(mover_id);
                                let posture_before = to_gen_posture(m_before.posture);
                                let air_before = to_gen_air(m_before.air);
                                let lock_before = m_before.lock;
                                auth_soldier.posture = posture_before;
                                auth_soldier.air = air_before;
                                auth_soldier.lock = lock_before;
                                let auth_scale = look_turn_scale(
                                    tuning.turn_scales(),
                                    posture_before,
                                    air_before,
                                    auth_state.sprint_time.is_some(),
                                    auth_state.roll.is_some(),
                                    auth_state.is_tumble_look(),
                                    1.0,
                                );
                                let tag_before = classify(posture_before, air_before, &auth_state, lock_before);
                                let yaw_b = auth_soldier.aim_yaw;
                                auth_soldier.look_raw_scaled(q.px, q.py, 1.0, false, auth_scale);
                                let sim_dy = -(auth_soldier.aim_yaw - yaw_b);
                                if q.px > 0.0 && settle_frames == 0 {
                                    stats[tag_before as usize].2 += q.px;
                                    stats[tag_before as usize].3 += sim_dy;
                                }
                                let m_sync = movement.mover(mover_id);
                                m_sync.aim_yaw = auth_soldier.aim_yaw;
                                m_sync.aim_pitch = auth_soldier.aim_pitch;
                                let tumbling = auth_state.tumble.is_some()
                                    || m_sync.air == MoveAir::Thrown;
                                let mi = if tumbling {
                                    MoveInput::default()
                                } else {
                                    MoveInput {
                                        forward: q.forward,
                                        jump: q.jump,
                                        crouch: q.crouch,
                                        ..MoveInput::default()
                                    }
                                };
                                let acts = if tumbling {
                                    Actions::default()
                                } else {
                                    Actions {
                                        sprint: q.controls.sprint,
                                        roll: q.controls.roll,
                                        ai: false,
                                    }
                                };
                                let was_air_jump = matches!(air_before, Some(Air::Jump | Air::Fall));
                                movement
                                    .advance_profiled(
                                        &mut col,
                                        mover_id,
                                        &mi,
                                        sim_dt,
                                        Some(Step {
                                            actions: acts,
                                            tuning: &tuning,
                                            state: &mut auth_state,
                                            override_: None,
                                        }),
                                    )
                                    .unwrap();
                                // Unpredicted terrain landing from normal Jump/Recover (no terrain in TurnPredictor)
                                // triggers a 1-tick transition ease just like any collision boundary.
                                let m_after = movement.mover(mover_id);
                                if was_air_jump && m_after.grounded && auth_state.tumble.is_none() {
                                    settle_frames = (fps / 15).max(4);
                                }
                                if !recover_from_tumble
                                    && quiet_start_tick.is_none()
                                    && auth_state.tumble.is_some_and(|tb| tb.phase == TumblePhase::Tumble)
                                {
                                    quiet_start_tick = Some(world_tick + 4);
                                }
                                receiver.corrected_sequence = f64::from(q.seq);
                            } else {
                                break;
                            }
                        }

                        let m = movement.mover(mover_id);
                        receiver.latest = f64::from(world_tick);
                        receiver.own.tick = f64::from(world_tick);
                        receiver.own.posture = to_gen_posture(m.posture) as usize;
                        receiver.own.air = to_gen_air(m.air).map_or(0, |a| a as u8 + 1);
                        receiver.own.velocity = m.velocity.arr();
                        receiver.own.lock = m.lock;
                        receiver.own.yaw = auth_soldier.aim_yaw;
                        receiver.own.pitch = auth_soldier.aim_pitch;
                        receiver.infantry = Some(make_sample(&auth_state, &tuning));
                        sender.feedback(&mut receiver);
                        sender.local_next_tick(f64::from(world_tick + 1));

                        let (held, actions, ctrls) = if recover_from_tumble {
                            (0u32, 0u32, Controls::default())
                        } else {
                            let crouch_edge = world_tick == 15 || world_tick == 35;
                            let hold_w = (40..=90).contains(&world_tick)
                                || (165..=195).contains(&world_tick);
                            let sprint = (65..=90).contains(&world_tick);
                            let jump_edge = world_tick == 100;
                            // After the landing lock of the jump (about 0.25 s after tick 157).
                            let roll_edge = world_tick == 176;
                            (
                                if hold_w { 1 << 0 } else { 0 },
                                (if jump_edge { 1 << 0 } else { 0 })
                                    | (if crouch_edge { 1 << 1 } else { 0 }),
                                Controls {
                                    sprint,
                                    roll: roll_edge,
                                    ..Controls::default()
                                },
                            )
                        };
                        let cmd = ClientCommand {
                            held,
                            actions,
                            mouse_scale: 1.0,
                            has_mouse_scale: true,
                            invert: Some(false),
                            ..Default::default()
                        };
                        sender.tick_with_infantry(
                            &cmd,
                            &ClientLook { x: loose_x, y: loose_y },
                            &ClientFlightCommand::default(),
                            Some(ctrls),
                            &receiver,
                            0.0,
                            t * 1000.0,
                        );
                        queue.push_back(Queued {
                            seq: sender.command_sequence() as u32,
                            stamp: sender.command_tick(),
                            px: loose_x,
                            py: loose_y,
                            forward: if held & 1 != 0 { 1.0 } else { 0.0 },
                            jump: actions & 1 != 0,
                            crouch: actions & 2 != 0,
                            controls: ctrls,
                        });
                        loose_x = 0.0;
                        loose_y = 0.0;
                    }

                    sender.set_loose(&ClientLook { x: loose_x, y: loose_y }, 1.0, false, 0.5);
                    sender.preview(&receiver);
                    sender.ease_walker_turn(&receiver, dt);

                    if frame > 0 {
                        let dyaw = sender.yaw - prev_yaw;
                        let dpitch = sender.pitch - prev_pitch;
                        if settle_frames > 0 {
                            settle_frames -= 1;
                        } else if dx > 0.0 {
                            let s = &mut stats[prev_tag as usize];
                            s.0 += dx;
                            s.1 += -dyaw;
                            let expected_step = dx * rate * prev_expected;
                            let err = (-dyaw - expected_step).abs();
                            if err > s.4 {
                                s.4 = err;
                            }
                            if dyaw > 1e-12 {
                                s.5 += 1;
                            }
                            if prev_tag == StateTag::GetUp {
                                assert!(
                                    dyaw.abs() < 1e-12 && dpitch.abs() < 1e-12,
                                    "fps={fps} run={run_idx} frame={frame} GetUp moved: dyaw={dyaw} dpitch={dpitch}"
                                );
                            } else {
                                assert!(
                                    err < 1e-9,
                                    "fps={fps} run={run_idx} frame={frame} state={} tick={world_tick}: drawn={} expected={} err={err}",
                                    NAMES[prev_tag as usize],
                                    -dyaw,
                                    expected_step,
                                );
                            }
                        } else if dyaw.abs() > 1e-12 {
                            stats[prev_tag as usize].6 += 1;
                        }
                    }

                    // Active predicted state for the next frame's counts: matches the scale
                    // just computed by `TurnPredictor` in `sender.preview(&receiver)`.
                    prev_yaw = sender.yaw;
                    prev_pitch = sender.pitch;
                    let m = movement.mover(mover_id);
                    let active_scale = (-sender.per_count) / rate;
                    prev_expected = active_scale;
                    prev_tag = if active_scale.abs() < 1e-9 {
                        StateTag::GetUp
                    } else if (active_scale - scales.tumble).abs() < 1e-9 {
                        match auth_state.tumble.map(|tb| tb.phase) {
                            Some(TumblePhase::Tumble) => StateTag::Tumble,
                            Some(TumblePhase::Landed) => StateTag::Landed,
                            _ => StateTag::Thrown,
                        }
                    } else if (active_scale - scales.jump).abs() < 1e-9 {
                        if auth_state.tumble.is_some_and(|tb| tb.phase == TumblePhase::Recover) {
                            StateTag::Recover
                        } else {
                            StateTag::Jump
                        }
                    } else if (active_scale - scales.roll).abs() < 1e-9 {
                        StateTag::Roll
                    } else if (active_scale - scales.sprint).abs() < 1e-9 {
                        StateTag::Sprint
                    } else if (active_scale - scales.crouch).abs() < 1e-9 {
                        StateTag::Crouch
                    } else {
                        let _ = m;
                        StateTag::Stand
                    };
                }

                // Drain remaining queue with zero mouse and verify exact convergence.
                while let Some(q) = queue.pop_front() {
                    let m_before = movement.mover(mover_id);
                    let posture_before = to_gen_posture(m_before.posture);
                    let air_before = to_gen_air(m_before.air);
                    auth_soldier.posture = posture_before;
                    auth_soldier.air = air_before;
                    auth_soldier.lock = m_before.lock;
                    let auth_scale = look_turn_scale(
                        tuning.turn_scales(),
                        posture_before,
                        air_before,
                        auth_state.sprint_time.is_some(),
                        auth_state.roll.is_some(),
                        auth_state.is_tumble_look(),
                        1.0,
                    );
                    auth_soldier.look_raw_scaled(q.px, q.py, 1.0, false, auth_scale);
                    let m_sync = movement.mover(mover_id);
                    m_sync.aim_yaw = auth_soldier.aim_yaw;
                    m_sync.aim_pitch = auth_soldier.aim_pitch;
                    let tumbling = auth_state.tumble.is_some() || m_sync.air == MoveAir::Thrown;
                    let mi = if tumbling {
                        MoveInput::default()
                    } else {
                        MoveInput {
                            forward: q.forward,
                            jump: q.jump,
                            crouch: q.crouch,
                            ..MoveInput::default()
                        }
                    };
                    let acts = if tumbling {
                        Actions::default()
                    } else {
                        Actions {
                            sprint: q.controls.sprint,
                            roll: q.controls.roll,
                            ai: false,
                        }
                    };
                    movement
                        .advance_profiled(
                            &mut col,
                            mover_id,
                            &mi,
                            sim_dt,
                            Some(Step {
                                actions: acts,
                                tuning: &tuning,
                                state: &mut auth_state,
                                override_: None,
                            }),
                        )
                        .unwrap();
                    receiver.corrected_sequence = f64::from(q.seq);
                }
                let m = movement.mover(mover_id);
                receiver.latest += 1.0;
                receiver.own.tick = receiver.latest;
                receiver.own.posture = to_gen_posture(m.posture) as usize;
                receiver.own.air = to_gen_air(m.air).map_or(0, |a| a as u8 + 1);
                receiver.own.velocity = m.velocity.arr();
                receiver.own.lock = m.lock;
                receiver.own.yaw = auth_soldier.aim_yaw;
                receiver.own.pitch = auth_soldier.aim_pitch;
                receiver.infantry = Some(make_sample(&auth_state, &tuning));
                sender.feedback(&mut receiver);
                sender.set_loose(&ClientLook::default(), 1.0, false, 0.0);
                sender.preview(&receiver);
                sender.ease_walker_turn(&receiver, dt);
                assert!(
                    (sender.yaw - auth_soldier.aim_yaw).abs() < 1e-12,
                    "fps={fps} run={run_idx}: yaw preview={} != auth={}",
                    sender.yaw,
                    auth_soldier.aim_yaw
                );
                assert!(
                    (sender.pitch - auth_soldier.aim_pitch).abs() < 1e-12,
                    "fps={fps} run={run_idx}: pitch preview={} != auth={}",
                    sender.pitch,
                    auth_soldier.aim_pitch
                );
            }

            for idx in 0..10 {
                let (d_cnt, d_dy, s_cnt, s_dy, max_err, rev, quiet) = stats[idx];
                assert!(d_cnt > 0.0 && s_cnt > 0.0, "fps={fps} state {} had no samples", NAMES[idx]);
                let drawn_scale = (d_dy / d_cnt) / rate;
                let sim_scale = (s_dy / s_cnt) / rate;
                let expected = EXPECTED_SCALES[idx];
                eprintln!(
                    "LOOK_RATE fps={fps} state={} sim_scale={sim_scale:.6} drawn_scale={drawn_scale:.6} expected={expected:.6} max_err={max_err:.3e} reversals={rev} quiet_moves={quiet} d_cnt={d_cnt:.0} s_cnt={s_cnt:.0}",
                    NAMES[idx]
                );
                assert!(
                    (sim_scale - expected).abs() < 1e-9,
                    "fps={fps} state {}: sim_scale={sim_scale:.6} != expected={expected:.6}",
                    NAMES[idx]
                );
                assert!(
                    (drawn_scale - sim_scale).abs() < 1e-9,
                    "fps={fps} state {}: drawn_scale={drawn_scale:.6} != sim_scale={sim_scale:.6}",
                    NAMES[idx]
                );
                assert_eq!(rev, 0, "fps={fps} state {} had {rev} reversals", NAMES[idx]);
                assert_eq!(quiet, 0, "fps={fps} state {} had {quiet} quiet moves", NAMES[idx]);
            }
        }
    }

    #[test]
    fn online_preview_scales_yaw_across_tumble_phases_without_local_infantry_sample() {
        // Online clients never receive `receiver.infantry` (Instant Action only); the compact soldier
        // gives `air`, `posture` and `lock`, and the sender is told the replay's restored state
        // (`note_replayed_state`) for the tumble phase.
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.control_family = bfe_family::GameFamily::Swbf2;
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.id = 4.;
        receiver.own.alive = true;
        receiver.own.generation = 2.;
        receiver.own.class = 0;
        receiver.own.zoom = 1.;
        receiver.infantry = None;
        let turns = [ClassTurn {
            max_turn: 4.,
            pitch_limits: [-1., 1.],
            swbf2: Some(bfe_sim::movement::soldier::swbf2::TurnScales {
                stand: 1., crouch: 0.85, sprint: 0.5, jump: 0.5, jet: 1., roll: 0.4, tumble: 0.12,
            }),
        }];

        // Standing baseline.
        let mut base_sender = Sender::new(&turns);
        base_sender.set_loose(&ClientLook { x: 100., y: -40. }, 1., false, 0.5);
        base_sender.preview(&receiver);
        let stand_dyaw = base_sender.yaw;
        let stand_dpitch = base_sender.pitch;
        assert!(stand_dyaw.abs() > 0.01 && stand_dpitch.abs() > 0.01);

        // The replayed state of a soldier in the given tumble phase (or none).
        let tuning = test_swbf2_tuning(turns[0].swbf2.unwrap());
        let replayed = |phase: Option<TumblePhase>| {
            let mut state = bfe_sim::movement::soldier::swbf2::State::new(&tuning);
            state.tumble = phase.map(|phase| {
                let mut t = bfe_sim::movement::soldier::swbf2::Tumble::enter(bfe_sim::movement::soldier::V3::ZERO, 0., 1);
                t.phase = phase;
                t
            });
            state
        };
        // Thrown, Tumble (crouched on ground), and Landed (crouched slide) are the replay's tumble
        // phases and must scale yaw by 0.12 with `infantry == None`.
        for (name, posture, air, phase) in [
            ("Thrown", Posture::Stand as usize, Air::Thrown as u8 + 1, TumblePhase::Thrown),
            ("Tumble", Posture::Crouch as usize, 0, TumblePhase::Tumble),
            ("Landed", Posture::Crouch as usize, 0, TumblePhase::Landed),
        ] {
            assert!(receiver.infantry.is_none());
            receiver.own.posture = posture;
            receiver.own.air = air;
            receiver.own.lock = 0.0;
            receiver.own.yaw = 0.0;
            receiver.own.pitch = 0.0;
            let mut s = Sender::new(&turns);
            s.note_replayed_state(Some(&replayed(Some(phase))));
            s.tick(
                &ClientCommand::default(),
                &ClientLook { x: 60., y: -20. },
                &ClientFlightCommand::default(),
                &receiver,
                0.,
                0.,
            );
            s.set_loose(&ClientLook { x: 40., y: -20. }, 1., false, 0.5);
            s.preview(&receiver);
            assert!(
                (s.yaw - stand_dyaw * 0.12).abs() < 1e-12,
                "online {name} (infantry=None) preview yaw {} must equal 0.12x standing {}",
                s.yaw,
                stand_dyaw * 0.12,
            );
            assert!((s.pitch - stand_dpitch).abs() < 1e-12);
        }

        // GetUp locks both yaw and pitch via `own.lock`.
        receiver.own.posture = Posture::Stand as usize;
        receiver.own.air = 0;
        receiver.own.lock = 0.9;
        receiver.own.yaw = 0.4;
        receiver.own.pitch = -0.2;
        let mut getup = Sender::new(&turns);
        getup.note_replayed_state(Some(&replayed(Some(TumblePhase::GetUp))));
        getup.tick(
            &ClientCommand::default(),
            &ClientLook { x: 60., y: -20. },
            &ClientFlightCommand::default(),
            &receiver,
            0.,
            0.,
        );
        getup.set_loose(&ClientLook { x: 40., y: -20. }, 1., false, 0.5);
        getup.preview(&receiver);
        assert_eq!((getup.yaw, getup.pitch), (0.4, -0.2), "online GetUp must freeze yaw and pitch");

        // Recover uses Air::Fall (jump scale = 0.5x).
        receiver.own.posture = Posture::Stand as usize;
        receiver.own.air = Air::Fall as u8 + 1;
        receiver.own.lock = 0.0;
        receiver.own.yaw = 0.0;
        receiver.own.pitch = 0.0;
        let mut recover = Sender::new(&turns);
        recover.note_replayed_state(Some(&replayed(Some(TumblePhase::Recover))));
        recover.set_loose(&ClientLook { x: 100., y: 0. }, 1., false, 0.5);
        recover.preview(&receiver);
        assert!((recover.yaw - stand_dyaw * 0.5).abs() < 1e-12, "online Recover must use jump turn scale");

        // Ordinary crouch outside a tumble still uses crouch scale (0.85x).
        receiver.own.posture = Posture::Crouch as usize;
        receiver.own.air = 0;
        receiver.own.lock = 0.0;
        let mut crouch = Sender::new(&turns);
        crouch.note_replayed_state(Some(&replayed(None)));
        crouch.set_loose(&ClientLook { x: 100., y: 0. }, 1., false, 0.5);
        crouch.preview(&receiver);
        assert!((crouch.yaw - stand_dyaw * 0.85).abs() < 1e-12, "ordinary crouch must keep crouch turn scale");
    }
}
