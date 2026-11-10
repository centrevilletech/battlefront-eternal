//! The local-server.js step/publication boundary. Context implementations own
//! gameplay; this host sequences generated input, one enclosing tick and its
//! publication. The same receiver and display phases used online consume the
//! result, with no transport, authoritative mirror or second simulation.
use super::{clock::Clock, publication::Publication, setup::Admitted, thread::PublicationSlot};
use bfe_client::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    online::receiver::Receiver,
};
use bfe_sim::gen::{
    HsCommandKind, WireWorldFacts, WorldTickInput, WorldTickOutput, WorldTickSetup,
};
use bfe_sim::store::block::Block;

/// Allocation-free counters proving that `Context::world_tick` and the presentation
/// capture ran on the local engine (`Context::world_host_journal` record counts by kind,
/// AI hearing records, published presentation/combat records, and `Context` poison status).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostCounters {
    pub journal_seq: u64,
    pub hearing_shot: u64,
    pub hearing_damage: u64,
    pub kind_spawn: u64,
    pub kind_vo: u64,
    pub kind_shot: u64,
    pub kind_damage: u64,
    pub kind_board: u64,
    pub kind_unboard: u64,
    pub kind_other: u64,
    pub presentation_records_published: u64,
    pub combat_records_published: u64,
    pub context_poisoned: bool,
}

// A device batch is borrowed only during the native ingress call. The world
// retains accepted commands in its own mailbox, as it does for remote peers.
// Neither a renderer nor a window event can retain a pointer into this batch.
pub struct Input<'a> {
    pub command: &'a ClientCommand,
    pub look: &'a ClientLook,
    pub flight: &'a ClientFlightCommand,
    pub tick: f64,
    pub at_ms: f64,
    pub epoch: f64,
    pub generation: f64,
    pub view_tick: Option<f64>,
    pub flight_mode: bool,
}

// This trait fixes the runtime boundary while cold setup and Context bodies
// develop independently. Production must use Context; observers in tests
// check host ordering and errors, never replace a gameplay solver.
pub trait Engine {
    fn initialize(&mut self, setup: WorldTickSetup) -> Result<(), String>;
    fn input(&mut self, input: Input<'_>) -> Result<(), String>;
    fn human_command(
        &mut self,
        _kind: HsCommandKind,
        _team: u8,
        _class_name: &str,
        _post: &str,
    ) -> Result<(), String> {
        Ok(())
    }
    fn step(&mut self, input: WorldTickInput) -> Result<WorldTickOutput, String>;
    fn publish(&mut self, facts: &[f64]) -> Result<Publication<'_>, String>;
    fn acknowledge(
        &mut self,
        snapshot: [f64; 2],
        generation: f64,
        outcomes: &[u32],
    ) -> Result<(), String>;
    fn close(&mut self);
    fn refused(&self) -> u64 {
        0
    }
    fn host_counters(&self) -> HostCounters {
        HostCounters::default()
    }
}

pub struct Runtime<E: Engine> {
    engine: E,
    clock: Clock,
    pub receiver: Receiver,
    facts: [f64; WireWorldFacts::WORDS],
    pub ticks: u64,
    pub refused: u64,
    client: u32,
    now_ms: f64,
    unix_ms: f64,
    closed: bool,
}

impl<E: Engine> Runtime<E> {
    // createLocalServer finishes admission before the first step. Resource
    // identities and the local player's welcome are known here, so no JSON
    // registry message or asynchronous transport bootstrap is necessary.
    pub fn new(
        mut engine: E,
        admission: Admitted,
        start_ms: f64,
        unix_ms: f64,
    ) -> Result<Self, String> {
        if let Err(error) = engine.initialize(admission.setup) {
            engine.close();
            return Err(error);
        }
        let mut receiver = Receiver::new(admission.registry);
        receiver.registry = true;
        receiver.welcomed = true;
        receiver.own.id = admission.player_id;
        let welcome_bytes = admission.publication;
        let welcome_output = admission.publication_output;
        let mut runtime = Self {
            engine,
            clock: Clock::new(admission.tick),
            receiver,
            facts: [0.; WireWorldFacts::WORDS],
            ticks: 0,
            refused: 0,
            client: admission.handles.client,
            now_ms: start_ms,
            unix_ms,
            closed: false,
        };
        // createLocalServer.welcome publishes during Context::world_admit before
        // accepting the first command. Deliver Admitted.publication directly when
        // present so we do not run a second tick-zero wire_publish call.
        if !welcome_bytes.is_empty() {
            runtime.deliver_welcome(&welcome_output, &welcome_bytes)?;
        } else {
            runtime.publish()?;
        }
        Ok(runtime)
    }

    /// Initializes the runtime on the 60 Hz world thread and copies its initial
    /// tick-0 baseline publication into `slot` so the render-thread `Receiver`
    /// receives the baseline `[1, 0]` snapshot before tick 1 deltas arrive.
    pub fn new_into_slot(
        mut engine: E,
        admission: Admitted,
        start_ms: f64,
        unix_ms: f64,
        slot: &mut PublicationSlot,
    ) -> Result<Self, String> {
        if let Err(error) = engine.initialize(admission.setup) {
            engine.close();
            return Err(error);
        }
        let mut receiver = Receiver::new(admission.registry);
        receiver.registry = true;
        receiver.welcomed = true;
        receiver.own.id = admission.player_id;
        let welcome_bytes = admission.publication;
        let welcome_output = admission.publication_output;
        let mut runtime = Self {
            engine,
            clock: Clock::new(admission.tick),
            receiver,
            facts: [0.; WireWorldFacts::WORDS],
            ticks: 0,
            refused: 0,
            client: admission.handles.client,
            now_ms: start_ms,
            unix_ms,
            closed: false,
        };
        if !welcome_bytes.is_empty() {
            runtime.deliver_welcome_into_slot(&welcome_output, &welcome_bytes, slot)?;
        } else {
            runtime.publish_into_slot(slot)?;
        }
        Ok(runtime)
    }

    fn deliver_welcome(
        &mut self,
        output: &bfe_sim::gen::WirePublicationOutput,
        welcome_bytes: &[u8],
    ) -> Result<(), String> {
        let mut block = Block::new();
        let published =
            Publication::from_welcome(output, welcome_bytes, &mut block).map_err(String::from)?;
        published.deliver(self.client, &mut self.receiver, self.now_ms)?;
        let ack = Publication::acknowledgement(&self.receiver);
        self.engine.acknowledge(
            ack,
            self.receiver.own.generation,
            &self.receiver.outcomes.values[..self.receiver.outcomes.count],
        )?;
        self.receiver.outcomes.count = 0;
        self.receiver.received.count = 0;
        Ok(())
    }

    fn deliver_welcome_into_slot(
        &mut self,
        output: &bfe_sim::gen::WirePublicationOutput,
        welcome_bytes: &[u8],
        slot: &mut PublicationSlot,
    ) -> Result<(), String> {
        let client_id = self.client;
        let now_ms = self.now_ms;
        let mut block = Block::new();
        let published =
            Publication::from_welcome(output, welcome_bytes, &mut block).map_err(String::from)?;
        published.copy_into(&mut slot.block, &mut slot.bytes);
        slot.tick = 0.0;
        slot.client = client_id;
        published.deliver(client_id, &mut self.receiver, now_ms)?;
        let ack = Publication::acknowledgement(&self.receiver);
        self.engine.acknowledge(
            ack,
            self.receiver.own.generation,
            &self.receiver.outcomes.values[..self.receiver.outcomes.count],
        )?;
        self.receiver.outcomes.count = 0;
        self.receiver.received.count = 0;
        self.receiver.publications.clear();
        Ok(())
    }

    pub fn tick(&self) -> f64 {
        self.clock.tick()
    }
    pub fn refusals(&self) -> u64 {
        self.refused.saturating_add(self.engine.refused())
    }
    pub fn host_counters(&self) -> HostCounters {
        self.engine.host_counters()
    }

    /// Queues a spawn-screen team/class/post selection on the reliable player actions lane.
    pub fn spawn_human(
        &mut self,
        team: u8,
        class_name: &str,
        post_name: &str,
    ) -> Result<(), String> {
        let team = team.clamp(1, 2);
        if self.receiver.own.team != 0 && self.receiver.own.team != team {
            self.engine
                .human_command(HsCommandKind::Team, team, "", "")?;
        }
        self.engine
            .human_command(HsCommandKind::Spawn, team, class_name, post_name)
    }

    /// Queues a suicide (`Respawn`) command on the reliable player actions lane.
    pub fn respawn_human(&mut self) -> Result<(), String> {
        self.engine.human_command(HsCommandKind::Suicide, 0, "", "")
    }

    // Native admission publishes tick zero to establish the controlled
    // generation before desktop input is sampled. Later publications preserve
    // every tick's event order until the display phase clears its journal.
    pub fn publish(&mut self) -> Result<(), String> {
        self.facts[WireWorldFacts::TICK] = self.tick();
        self.facts[WireWorldFacts::AT] = self.now_ms;
        self.facts[WireWorldFacts::UNIX_MS] = self.unix_ms + self.now_ms.floor();
        let published = self.engine.publish(&self.facts)?;
        published.deliver(self.client, &mut self.receiver, self.now_ms)?;
        let ack = Publication::acknowledgement(&self.receiver);
        // Snapshot ACKs retire baselines; edge ACKs independently retire the
        // world's reliable outcomes. Omitting the latter fills a long local
        // battle's fire queue even though no network packet was ever lost.
        self.engine.acknowledge(
            ack,
            self.receiver.own.generation,
            &self.receiver.outcomes.values[..self.receiver.outcomes.count],
        )?;
        self.receiver.outcomes.count = 0;
        self.receiver.received.count = 0;
        Ok(())
    }

    fn publish_into_slot(&mut self, slot: &mut PublicationSlot) -> Result<(), String> {
        let current_tick = self.tick();
        let client_id = self.client;
        let now_ms = self.now_ms;
        self.facts[WireWorldFacts::TICK] = current_tick;
        self.facts[WireWorldFacts::AT] = now_ms;
        self.facts[WireWorldFacts::UNIX_MS] = self.unix_ms + now_ms.floor();
        let published = self.engine.publish(&self.facts)?;
        published.copy_into(&mut slot.block, &mut slot.bytes);
        slot.tick = current_tick;
        slot.client = client_id;
        published.deliver(client_id, &mut self.receiver, now_ms)?;
        let ack = Publication::acknowledgement(&self.receiver);
        self.engine.acknowledge(
            ack,
            self.receiver.own.generation,
            &self.receiver.outcomes.values[..self.receiver.outcomes.count],
        )?;
        self.receiver.outcomes.count = 0;
        self.receiver.received.count = 0;
        self.receiver.publications.clear();
        Ok(())
    }

    // Game.step accepts one already stamped command, then WorldTick owns all
    // gameplay work. A rejected tick never completes its clock or publishes
    // a fabricated state. The window reports the failure and closes the owner.
    pub fn step(
        &mut self,
        command: &ClientCommand,
        look: &ClientLook,
        flight: &ClientFlightCommand,
    ) -> Result<(), String> {
        if self.closed {
            return Err("local battle is closed".into());
        }
        let mut input = self.clock.input();
        // The recorded local host adds the period, rather than multiplying
        // tick by period. Preserve that rounding and Date.now's integer slot.
        self.now_ms += 1000. / 60.;
        input.at = self.now_ms;
        input.mouse_scale = 1.;
        self.engine.input(Input {
            command,
            look,
            flight,
            tick: input.tick,
            at_ms: self.now_ms,
            epoch: self.receiver.epoch,
            generation: self.receiver.own.generation,
            view_tick: Some(self.receiver.clock.shown),
            flight_mode: self.receiver.own.body_schema == 7,
        })?;
        let output = self.engine.step(input)?;
        if let Err(error) = self.clock.complete(&output) {
            self.refused = self.refused.saturating_add(1);
            return Err(error.into());
        }
        self.ticks += 1;
        self.publish()
    }

    /// Steps one 60 Hz tick on the dedicated world thread and copies the
    /// resulting publication into `slot` for non-blocking render-thread delivery.
    pub fn step_into_slot(
        &mut self,
        command: &ClientCommand,
        look: &ClientLook,
        flight: &ClientFlightCommand,
        _render_ack: [f64; 2],
        slot: &mut PublicationSlot,
    ) -> Result<f64, String> {
        if self.closed {
            return Err("local battle is closed".into());
        }
        let mut input = self.clock.input();
        self.now_ms += 1000. / 60.;
        input.at = self.now_ms;
        input.mouse_scale = 1.;
        self.engine.input(Input {
            command,
            look,
            flight,
            tick: input.tick,
            at_ms: self.now_ms,
            epoch: self.receiver.epoch,
            generation: self.receiver.own.generation,
            view_tick: Some(self.receiver.clock.shown),
            flight_mode: self.receiver.own.body_schema == 7,
        })?;
        let output = self.engine.step(input)?;
        if let Err(error) = self.clock.complete(&output) {
            self.refused = self.refused.saturating_add(1);
            return Err(error.into());
        }
        self.ticks += 1;
        self.publish_into_slot(slot)?;
        Ok(self.tick())
    }

    // Close converges with failure and ordinary window teardown. Context is
    // thread-affine and is disposed on this same thread, after the renderer
    // has finished borrowing this frame's publication and audio observations.
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.engine.close();
        }
    }
}
