//! The production local engine owns one thread-affine Context. All gameplay
//! remains in that owner; the client supplies generated input and clock facts
//! and borrows its canonical publication for the shared presentation session.
use super::{
    arguments::{self, Arguments},
    commands::{Commands, Names},
    publication::Publication,
    runtime::{Engine, HostCounters, Input},
    setup::{Admitted, Handles},
};
use bfe_sim::{
    abi::{
        context::{Context, ErrorKind},
        context_step::Publication as OwnedPublication,
    },
    gen::{
        self, HsCommand, HsCommandKind, IaHuman, PlayerActions, PlayerAdmission, PlayerFire,
        WeCall, WorldTickInput, WorldTickOutput, WorldTickSetup,
    },
    host::Capture,
    wire::publisher::Publisher,
};

// Keep disposal independent of later scene/window admission. A failing loader
// can drop this guard before Runtime exists; aliases still observe disposal.
struct OwnedContext {
    value: Context,
    closed: bool,
}
impl OwnedContext {
    fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            let _ = self.value.dispose();
        }
    }
}
impl Drop for OwnedContext {
    fn drop(&mut self) {
        self.close();
    }
}

const SNAPSHOT_ACK: f64 = 4.;
const OUTCOME_ACK: f64 = 7.;

/// Production local simulation engine backed by a thread-affine [`Context`].
///
/// Initialization binds the authority host's presentation [`Capture`] (`world_host_start`)
/// and registers the local player's entity-edges runtime (`world_host_client`).
/// Each 60 Hz step executes [`Context::world_tick`] so edge advancement, pre-step input
/// banks (including AI hearing), `world_step`, notice draining, and shot/view mirrors
/// all run natively in lockstep.
pub struct Native {
    context: OwnedContext,
    handles: Handles,
    commands: Commands,
    args: Arguments,
    published: OwnedPublication,
    ack: WeCall,
    actions: PlayerActions,
    accepted: usize,
    refused: u64,
    host_op: u32,
    edges_op: u32,
    journal_seen: usize,
    last_input_sequence: f64,
    counters: HostCounters,
    host_content: bfe_sim::host::Content,
    explosions: Vec<u32>,
}

impl Native {
    pub fn new(context: Context, admission: &Admitted) -> Result<Self, String> {
        let context = OwnedContext {
            value: context,
            closed: false,
        };
        let commands = Commands::new(Names::new(&context.value)?);
        let host_op = arguments::required("wireHostOp")?;
        let edges_op = arguments::required("wireEdgesRun")?;
        // Mirror the shared publisher's admitted arenas, using its reservation
        // policy instead of a second client-side byte/record capacity. The
        // temporary owner is cold and drops before the first publication.
        let policy = Publisher::default();
        let published = OwnedPublication {
            block: bfe_sim::store::block::Block {
                words: Vec::with_capacity(policy.block.words.capacity()),
                heap: Vec::new(),
            },
            bytes: Vec::with_capacity(policy.bytes.capacity()),
        };
        Ok(Self {
            context,
            handles: admission.handles,
            commands,
            args: Arguments::new(),
            published,
            ack: WeCall {
                op: OUTCOME_ACK,
                capacity: bfe_sim::wire::edges::Host::default().capacity,
                ids: Vec::with_capacity(32),
                ..Default::default()
            },
            host_op,
            edges_op,
            actions: PlayerActions {
                payload_valid: true,
                human_valid: true,
                ..Default::default()
            },
            accepted: arguments::mailbox_accepted()?,
            refused: 0,
            journal_seen: 0,
            last_input_sequence: 0.,
            counters: HostCounters::default(),
            host_content: admission.host_content.clone(),
            explosions: admission.explosions.clone(),
        })
    }

    pub fn handles(&self) -> Handles {
        self.handles
    }

    /// Reads newly emitted records from `Context::world_host_journal` since `self.journal_seen`
    /// without allocating on the heap, accumulating AI hearing counts and per-kind counts.
    /// The ring holds `CAPACITY` (4096) entries indexed by `seq % CAPACITY`; advancing
    /// `journal_seen` after each tick guarantees every record emitted by `Drain` is counted
    /// once even over multi-minute battles where `seq` far exceeds the ring size.
    fn sample_host_journal(&mut self) {
        let seen = self.journal_seen;
        let mut next_seen = seen;
        let mut counters = self.counters;
        match self.context.value.world_host_journal(|journal| {
            let head = journal.seq;
            let start = seen.max(head.saturating_sub(bfe_sim::host::journal::CAPACITY));
            for record in journal
                .records()
                .filter(|record| record.seq >= start && record.seq < head)
            {
                let kind = record.event.kind();
                // The typed journal uses these same kinds for AI hearing.
                if kind == "shot" {
                    counters.hearing_shot += 1;
                }
                if kind == "damage" {
                    counters.hearing_damage += 1;
                }
                match kind {
                    "spawn" => counters.kind_spawn += 1,
                    "vo" => counters.kind_vo += 1,
                    "shot" => counters.kind_shot += 1,
                    "damage" => counters.kind_damage += 1,
                    "board" => counters.kind_board += 1,
                    "unboard" => counters.kind_unboard += 1,
                    _ => counters.kind_other += 1,
                }
            }
            next_seen = head;
            counters.journal_seq = head as u64;
        }) {
            Ok(()) => {
                self.journal_seen = next_seen;
                self.counters = counters;
            }
            Err(error) => {
                if error.kind() == ErrorKind::Poisoned {
                    self.counters.context_poisoned = true;
                }
            }
        }
    }
}

impl Engine for Native {
    fn initialize(&mut self, setup: WorldTickSetup) -> Result<(), String> {
        self.context
            .value
            .world_step_init(setup)
            .map_err(|e| e.to_string())?;
        // Bind the authority host's initial movement flags, dispenser lives, and
        // wire presentation capture (`combat`, `presentation`, `store_source`),
        // then register the local client's player and entity-edges handles.
        // Without `Capture`, `Drain` cannot emit schema-8 presentation records
        // or combat round launch/impact records into the wire publisher.
        let capture = Capture {
            combat: self.handles.combat,
            presentation: self.handles.presentation,
            store_source: self.handles.store_source,
            explosions: std::mem::take(&mut self.explosions),
        };
        self.context
            .value
            .world_host_start(Some(capture), std::mem::take(&mut self.host_content))
            .map_err(|e| e.to_string())?;
        self.context
            .value
            .world_host_client(self.handles.player, self.handles.edges)
            .map_err(|e| e.to_string())
    }

    fn input(&mut self, input: Input<'_>) -> Result<(), String> {
        // The mailbox retains held state between commands. Submitting a new
        // synthetic identity here would acknowledge input the renderer never sent.
        let sequence = input.command.sequence;
        if sequence <= self.last_input_sequence {
            return Ok(());
        }
        self.submit(input, |context, arguments, op| arguments.run(context, op))?;
        self.last_input_sequence = sequence;
        Ok(())
    }

    fn human_command(
        &mut self,
        kind: HsCommandKind,
        team: u8,
        class_name: &str,
        post: &str,
    ) -> Result<(), String> {
        // Intern class and post names in the world's string table before
        // queuing the human spawn/team command for the next `players_actions` call.
        let class_id = if class_name.is_empty() {
            None
        } else {
            Some(
                self.context
                    .value
                    .intern(&class_name.encode_utf16().collect::<Vec<_>>())
                    .map_err(super::setup::describe)?,
            )
        };
        let post_id = if post.is_empty() {
            None
        } else {
            Some(
                self.context
                    .value
                    .intern(&post.encode_utf16().collect::<Vec<_>>())
                    .map_err(super::setup::describe)?,
            )
        };
        self.actions.human.push(IaHuman {
            command: HsCommand {
                kind,
                team: team as f64,
                class_name: class_id,
                post: post_id,
            },
            valid: true,
        });
        Ok(())
    }

    fn step(&mut self, mut input: WorldTickInput) -> Result<WorldTickOutput, String> {
        input.units = self
            .context
            .value
            .world_prepare_units()
            .map_err(|error| error.to_string())?;
        // Step via `Context::world_tick` so the authority host runs `advance_edges`,
        // pre-step input bank writes (including AI hearing), `world_step`, notice
        // draining into `SimEvents` and `Capture`, shot/view mirrors, and `consumed_edges`.
        match self.context.value.world_tick(input) {
            Ok(output) => {
                self.sample_host_journal();
                Ok(output)
            }
            Err(error) => {
                if error.kind() == ErrorKind::Poisoned {
                    self.counters.context_poisoned = true;
                }
                Err(error.to_string())
            }
        }
    }

    // Context retains its encoder while this caller retains only the borrowed
    // result arenas. No generated take() rebuilds nested lists for delivery.
    fn publish(&mut self, facts: &[f64]) -> Result<Publication<'_>, String> {
        if let Err(error) = self.context.value.world_publish_into(
            self.handles.publication,
            facts,
            &mut self.published,
        ) {
            if error.kind() == ErrorKind::Poisoned {
                self.counters.context_poisoned = true;
            }
            return Err(error.to_string());
        }
        let pub_view = Publication::new(&self.published.block, &self.published.bytes)
            .map_err(str::to_string)?;
        // Count reliable combat records (`schema == 8` presentation vs all schemas)
        // delivered to this local client on the borrowed publication block.
        let (pres, total) = pub_view.count_client_records(self.handles.client);
        self.counters.presentation_records_published = self
            .counters
            .presentation_records_published
            .saturating_add(pres);
        self.counters.combat_records_published =
            self.counters.combat_records_published.saturating_add(total);
        Ok(pub_view)
    }

    fn acknowledge(
        &mut self,
        snapshot: [f64; 2],
        generation: f64,
        outcomes: &[u32],
    ) -> Result<(), String> {
        // SnapshotHost.acceptAck is independent of reliable outcome receipts.
        // Neither path serializes bytes or calls an application transport.
        self.args.words.clear();
        self.args.words.extend_from_slice(&[
            self.handles.host as f64,
            SNAPSHOT_ACK,
            self.handles.client as f64,
            snapshot[0],
            snapshot[1],
        ]);
        self.args.run(&self.context.value, self.host_op)?;
        if outcomes.is_empty() {
            return Ok(());
        }
        self.ack.epoch = snapshot[0];
        self.ack.generation = generation;
        self.ack.ids.clear();
        self.ack
            .ids
            .extend(outcomes.iter().take(32).map(|&id| id as f64));
        let ack = &self.ack;
        self.args.record(
            &[self.handles.edges as f64],
            gen::WeCall::WORDS,
            |row, heap| ack.put(row, heap),
        );
        self.args.run(&self.context.value, self.edges_op)
    }

    fn close(&mut self) {
        self.context.close();
    }
    fn refused(&self) -> u64 {
        self.refused.saturating_add(self.commands.refused)
    }
    fn host_counters(&self) -> HostCounters {
        self.counters
    }
}
impl Native {
    // Context is the production sender, inlined through this boundary. A
    // synthetic admission test can inspect its generated operations without
    // replacing the mailbox, controller, clock or gameplay algorithms.
    pub fn submit(
        &mut self,
        input: Input<'_>,
        mut run: impl FnMut(&Context, &mut Arguments, u32) -> Result<(), String>,
    ) -> Result<(), String> {
        self.commands.capture(
            input.command,
            input.look,
            input.flight,
            input.epoch,
            input.generation,
            input.tick,
            input.command.first_person.unwrap_or(true),
            input.command.aspect,
            input.view_tick,
            input.flight_mode,
        )?;
        let now = input.tick - 1.;
        self.commands.mailbox.tick = now;
        let call = &self.commands.mailbox;
        self.args.record(
            &[self.handles.mailbox as f64],
            gen::InputMailboxCall::WORDS,
            |row, heap| call.put(row, heap),
        );
        run(
            &self.context.value,
            &mut self.args,
            gen::OPERATION_INPUT_MAILBOX_OWNED_RUN,
        )?;
        let accepted = self.args.row(gen::ImResult::WORDS)?[self.accepted] != 0.;
        self.refused = self.refused.saturating_add(u64::from(!accepted));
        // world.input's receipt updates its stale-input clock and maximum
        // input lead independently of the mailbox's replaceable queue.
        self.args.words.clear();
        self.args.words.extend_from_slice(&[
            self.handles.player as f64,
            input.at_ms,
            input.tick - now,
            u8::from(accepted) as f64,
        ]);
        run(
            &self.context.value,
            &mut self.args,
            gen::OPERATION_PLAYERS_INPUT,
        )?;

        if !self.commands.actions.actions.is_empty() || !self.actions.human.is_empty() {
            // The reliable lane owns actions independently of movement. Lend
            // its generated entry by swapping retained vectors, then return
            // them even on refusal; no action list is cloned for submission.
            self.actions.same_epoch = input.epoch == 1.;
            self.actions.tick = now;
            self.actions.at = input.at_ms;
            std::mem::swap(&mut self.actions.entry, &mut self.commands.actions);
            let actions = &self.actions;
            self.args.record(
                &[self.handles.player as f64],
                PlayerActions::WORDS,
                |row, heap| actions.put(row, heap),
            );
            let result = run(
                &self.context.value,
                &mut self.args,
                gen::OPERATION_PLAYERS_ACTIONS,
            );
            std::mem::swap(&mut self.actions.entry, &mut self.commands.actions);
            self.actions.human.clear();
            result?;
            let reply = PlayerAdmission::take(self.args.row(PlayerAdmission::WORDS)?, &[]);
            if let Some(error) = reply.error {
                let text = self
                    .context
                    .value
                    .string(error)
                    .map_err(super::setup::describe)?;
                return Err(String::from_utf16_lossy(&text));
            }
            self.refused = self.refused.saturating_add(u64::from(!reply.accepted));
        }
        for edge in &self.commands.fires {
            // FireCommands supplies the actor's aim when an edge names none,
            // after this tick's look has been applied by the players' phase.
            let fire = PlayerFire {
                edge: edge.clone(),
                epoch_one: input.epoch == 1.,
                pressed_valid: true,
                tick: now,
                at: input.at_ms,
            };
            self.args.record(
                &[self.handles.player as f64],
                PlayerFire::WORDS,
                |row, heap| fire.put(row, heap),
            );
            run(
                &self.context.value,
                &mut self.args,
                gen::OPERATION_PLAYERS_FIRE,
            )?;
            self.refused = self
                .refused
                .saturating_add(u64::from(self.args.row(1)?[0] == 0.));
        }
        Ok(())
    }
}
