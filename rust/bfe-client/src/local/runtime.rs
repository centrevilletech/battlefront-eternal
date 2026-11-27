//! The local-server.js step/publication boundary. Context implementations own
//! gameplay; this host sequences generated input, one enclosing tick and its
//! publication. The same receiver and display phases used online consume the
//! result, with no transport, authoritative mirror or second simulation.
use super::{clock::Clock, publication::{Faults, Publication}, setup::Admitted, thread::PublicationSlot};
use bfe_client::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    online::{feedback::Ids, receiver::Receiver},
};
use bfe_sim::wire::publisher::OwnFacts;
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
    /// The tick this command is the input of.
    pub tick: f64,
    /// The last tick the world has completed.
    pub now: f64,
    pub at_ms: f64,
    pub epoch: f64,
    pub generation: f64,
    pub view_tick: Option<f64>,
    pub fire_aim: Option<[f64;2]>,
    pub flight_mode: bool,
}

// This trait fixes the runtime boundary while cold setup and Context bodies
// develop independently. Production must use Context; observers in tests
// check host ordering and errors, never replace a gameplay solver.
pub trait Engine {
    fn mission_view(&mut self, _tick: u32, _dest: &mut Option<Box<super::mission_view::Observation>>) {}
    fn mission_score(&self) -> Option<bfe_sim::battle::script::MissionScore> { None }
    fn infantry_input(&mut self, _sequence: f64, _input: bfe_sim::units::infantry::Controls) -> Result<(), String> { Ok(()) }
    fn pilot_input(&mut self, _sequence: f64, _input: bfe_sim::space::local::PilotControls) -> Result<(), String> { Ok(()) }
    fn space_sample(&mut self, _frame: &mut super::space::Frame) -> Result<(), String> { Ok(()) }
    fn infantry_contacts(&self, _dest: &mut Option<Box<bfe_sim::units::infantry::contact::Contacts>>) {}
    fn infantry_powers(&self, _dest: &mut Option<Box<bfe_sim::combat::powers::PowerFrame>>) {}
    fn hero_audio(&self, _dest: &mut Option<Box<bfe_sim::units::infantry::hero_audio::HeroAudioSnapshot>>) {}
    fn infantry_sample(&self) -> Option<bfe_sim::units::infantry::Sample> { None }
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
    /// A world setting changed during the battle: whether a hit from the
    /// victim's own team hurts.
    fn friendly_fire(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn door_frame(&self, _out: &mut Vec<bfe_sim::space::SpaceDoorSnapshot>) -> Result<(), String> { Ok(()) }
    fn publish(&mut self, facts: &[f64]) -> Result<Publication<'_>, String>;
    fn acknowledge(
        &mut self,
        snapshot: [f64; 2],
        generation: f64,
        outcomes: &[u32],
    ) -> Result<(), String>;
    fn close(&mut self);
    /// What the last publication told the local client about itself.
    fn published_own(&self) -> Result<Option<OwnFacts>, String> {
        Ok(None)
    }
    fn refused(&self) -> u64 {
        0
    }
    fn host_counters(&self) -> HostCounters {
        HostCounters::default()
    }
}

/// What the world told its local client about itself, kept as that client's
/// receiver would: the snapshot epoch, the newest complete snapshot (the
/// acknowledgement), and the own correction's life and team.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Own {
    pub epoch: f64,
    pub ack_tick: f64,
    pub generation: f64,
    pub team: u8,
}

impl Own {
    fn apply(&mut self, facts: OwnFacts) {
        // A newer epoch starts the acknowledgements over; an older one is not read.
        if facts.epoch < self.epoch {
            return;
        }
        if facts.epoch > self.epoch {
            self.epoch = facts.epoch;
            self.ack_tick = 0.;
        }
        if let Some((generation, team)) = facts.correction {
            self.generation = generation;
            self.team = team;
        }
        if facts.complete {
            self.ack_tick = self.ack_tick.max(facts.tick);
        }
    }
}

pub struct Runtime<E: Engine> {
    engine: E,
    clock: Clock,
    /// The display receiver of a single-thread runtime (`new`, which tests
    /// drive directly). The world
    /// thread's runtime (`new_into_slot`) has none: it reads `own` from the
    /// world instead of decoding its own publications.
    receiver: Option<Receiver>,
    /// The world thread's view of its client (`new_into_slot`).
    pub own: Own,
    outcomes: Ids,
    facts: [f64; WireWorldFacts::WORDS],
    pub ticks: u64,
    pub refused: u64,
    client: u32,
    now_ms: f64,
    unix_ms: f64,
    closed: bool,
    faults: Faults,
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
        let mut receiver = Receiver::with_bounds(admission.registry, admission.wire_bounds);
        receiver.control_family = admission.family;
        receiver.registry = true;
        receiver.welcomed = true;
        receiver.own.id = admission.player_id;
        let welcome_bytes = admission.publication;
        let welcome_output = admission.publication_output;
        let mut runtime = Self {
            engine,
            clock: Clock::new(admission.tick),
            receiver: Some(receiver),
            own: Own::default(),
            outcomes: Ids::default(),
            facts: [0.; WireWorldFacts::WORDS],
            ticks: 0,
            refused: 0,
            client: admission.handles.client,
            now_ms: start_ms,
            unix_ms,
            closed: false,
            faults: Faults::default(),
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
        let welcome_bytes = admission.publication;
        let welcome_output = admission.publication_output;
        let mut runtime = Self {
            engine,
            clock: Clock::new(admission.tick),
            receiver: None,
            own: Own::default(),
            outcomes: Ids::default(),
            facts: [0.; WireWorldFacts::WORDS],
            ticks: 0,
            refused: 0,
            client: admission.handles.client,
            now_ms: start_ms,
            unix_ms,
            closed: false,
            faults: Faults::default(),
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
        if let Some(fault) = published.fault(self.client)? {
            return Err(fault);
        }
        let receiver = self.receiver.as_mut().ok_or("no display receiver")?;
        published.deliver(self.client, receiver, self.now_ms)?;
        self.engine.infantry_powers(&mut receiver.powers);
        let ack = Publication::acknowledgement(receiver);
        self.engine.acknowledge(
            ack,
            receiver.own.generation,
            &receiver.outcomes.values[..receiver.outcomes.count],
        )?;
        receiver.outcomes.count = 0;
        receiver.received.count = 0;
        Ok(())
    }

    fn deliver_welcome_into_slot(
        &mut self,
        output: &bfe_sim::gen::WirePublicationOutput,
        welcome_bytes: &[u8],
        slot: &mut PublicationSlot,
    ) -> Result<(), String> {
        let mut block = Block::new();
        let published =
            Publication::from_welcome(output, welcome_bytes, &mut block).map_err(String::from)?;
        published.copy_into(&mut slot.block, &mut slot.bytes);
        self.engine.space_sample(&mut slot.space)?;
        self.engine.door_frame(&mut slot.doors)?;
        slot.mission_score = self.engine.mission_score();
        self.engine.mission_view(0, &mut slot.mission_view);
        slot.tick = 0.0;
        slot.client = self.client;
        if let Some(fault) = published.fault(self.client)? {
            return Err(fault);
        }
        self.outcomes.count = 0;
        published.outcomes(self.client, self.own.epoch, self.own.generation, &mut self.outcomes)?;
        self.acknowledge_own()
    }

    // The world thread's client after one of its publications, as its
    // receiver would have read it: faults and the outcome ids its edges
    // return (matched against the life and epoch known before this
    // publication) come from the publication, then the publisher's own facts;
    // then the acknowledgement goes back.
    fn acknowledge_own(&mut self) -> Result<(), String> {
        if let Some(facts) = self.engine.published_own()? {
            self.own.apply(facts);
        }
        self.engine.acknowledge(
            [self.own.epoch, self.own.ack_tick],
            self.own.generation,
            &self.outcomes.values[..self.outcomes.count],
        )
    }

    /// The display receiver of a single-thread runtime (`new` only).
    pub fn receiver(&self) -> &Receiver {
        self.receiver.as_ref().expect("display receiver of a single-thread runtime")
    }
    pub fn receiver_mut(&mut self) -> &mut Receiver {
        self.receiver.as_mut().expect("display receiver of a single-thread runtime")
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
        let own_team = self.receiver.as_ref().map_or(self.own.team, |r| r.own.team);
        if own_team != 0 && own_team != team {
            self.engine
                .human_command(HsCommandKind::Team, team, "", "")?;
        }
        self.engine
            .human_command(HsCommandKind::Spawn, team, class_name, post_name)
    }

    /// Queues a suicide (`Respawn`) command on the reliable player actions lane.
    pub fn set_friendly_fire(&mut self, on: bool) -> Result<(), String> {
        self.engine.friendly_fire(on)
    }

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
        self.faults.note(published.fault(self.client)?)?;
        let receiver = self.receiver.as_mut().ok_or("no display receiver")?;
        published.deliver(self.client, receiver, self.now_ms)?;
        self.engine.infantry_powers(&mut receiver.powers);
        let ack = Publication::acknowledgement(receiver);
        // Snapshot ACKs retire baselines; edge ACKs independently retire the
        // world's reliable outcomes. Omitting the latter fills a long local
        // battle's fire queue even though no network packet was ever lost.
        self.engine.acknowledge(
            ack,
            receiver.own.generation,
            &receiver.outcomes.values[..receiver.outcomes.count],
        )?;
        receiver.outcomes.count = 0;
        receiver.received.count = 0;
        Ok(())
    }

    pub fn infantry_input(&mut self, sequence: f64, input: bfe_sim::units::infantry::Controls) -> Result<(), String> {
        self.engine.infantry_input(sequence, input)
    }

    /// The command of this sequence's pilot controls (a battle without a Space battlefield drops them).
    pub fn pilot_input(&mut self, sequence: f64, input: bfe_sim::space::local::PilotControls) -> Result<(), String> {
        self.engine.pilot_input(sequence, input)
    }

    fn publish_into_slot(&mut self, slot: &mut PublicationSlot) -> Result<(), String> {
        self.engine.space_sample(&mut slot.space)?;
        slot.mission_score = self.engine.mission_score();
        self.engine.mission_view(self.tick() as u32, &mut slot.mission_view);
        slot.infantry = self.engine.infantry_sample();
        self.engine.infantry_contacts(&mut slot.contacts);
        self.engine.infantry_powers(&mut slot.powers);
        self.engine.hero_audio(&mut slot.hero_audio);
        let current_tick = self.tick();
        let client_id = self.client;
        let now_ms = self.now_ms;
        self.facts[WireWorldFacts::TICK] = current_tick;
        self.facts[WireWorldFacts::AT] = now_ms;
        self.facts[WireWorldFacts::UNIX_MS] = self.unix_ms + now_ms.floor();
        self.engine.door_frame(&mut slot.doors)?;
        let published = self.engine.publish(&self.facts)?;
        published.copy_into(&mut slot.block, &mut slot.bytes);
        slot.tick = current_tick;
        slot.client = client_id;
        // The render thread delivers this slot either way: without the player's
        // own update it reads like a lost packet.
        self.faults.note(published.fault(client_id)?)?;
        self.outcomes.count = 0;
        published.outcomes(client_id, self.own.epoch, self.own.generation, &mut self.outcomes)?;
        self.acknowledge_own()
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
        let receiver = self.receiver.as_ref().ok_or("no display receiver")?;
        self.engine.input(Input {
            command,
            look,
            flight,
            tick: input.tick,
            now: input.tick - 1.,
            at_ms: self.now_ms,
            epoch: receiver.epoch,
            generation: receiver.own.generation,
            view_tick: Some(bfe_client::online::clock::rendered_tick(receiver.clock.shown, receiver.latest)),
            fire_aim: None,
            flight_mode: receiver.uses_flight_controls(),
        })?;
        let output = self.engine.step(input)?;
        if let Err(error) = self.clock.complete(&output) {
            self.refused = self.refused.saturating_add(1);
            return Err(error.into());
        }
        self.ticks += 1;
        self.publish()
    }

    /// Queues one command of the player's for the tick it is stamped for, as a
    /// server queues a remote player's. A stamp the world has already passed
    /// goes into the next tick.
    pub fn submit(
        &mut self,
        command: &ClientCommand,
        look: &ClientLook,
        flight: &ClientFlightCommand,
        // The target sample the screen showed when this command was made.
        view_tick: f64,
        fire_aim: Option<[f64; 2]>,
        // Whether the screen's receiver had the player flying when the
        // command was packed (it packs flight controls then).
        flight_mode: bool,
    ) -> Result<(), String> {
        if self.closed {
            return Err("local battle is closed".into());
        }
        let next = self.clock.input().tick;
        let (epoch, generation) = self
            .receiver
            .as_ref()
            .map_or((self.own.epoch, self.own.generation), |r| (r.epoch, r.own.generation));
        self.engine.input(Input {
            command,
            look,
            flight,
            tick: command.tick.max(next),
            now: next - 1.,
            at_ms: self.now_ms,
            epoch,
            generation,
            view_tick: Some(view_tick),
            fire_aim,
            flight_mode,
        })
    }

    /// Steps one 60 Hz tick on the dedicated world thread, with the commands
    /// `submit` queued for it, and copies the resulting publication into `slot`.
    pub fn step_into_slot(&mut self, slot: &mut PublicationSlot) -> Result<f64, String> {
        if self.closed {
            return Err("local battle is closed".into());
        }
        let mut input = self.clock.input();
        self.now_ms += 1000. / 60.;
        input.at = self.now_ms;
        input.mouse_scale = 1.;
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
