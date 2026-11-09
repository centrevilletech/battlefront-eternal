//! The authority host's tick: what the JavaScript server did around the
//! world's step that the step itself does not, as one native entry,
//! `Context::world_tick`. The page's `LocalServer.step`
//! (src/runtime/local-server.js) ran `host.advanceEdges()`, then
//! `world.step(now)` (src/runtime/multiplayer-world.js), whose `game.step`
//! (src/game.js) wrote the owners' input banks before `world_step_run` and
//! drained the unit phase's notifications after it, then
//! `host.consumedEdges(tick)`. A native client or server that calls the bare
//! `world_step` skips all of that: the AI hears nothing, the collision
//! kernel keeps stale body flags, the notices go unhandled and no cosmetic
//! combat record reaches the wire. This module keeps the host's state
//! between ticks on the Context and runs those duties in the page's order:
//!
//! 1. `advanceEdges`: each client's edge runtime resets when its unit's life
//!    moved, then advances to the tick (edges.rs);
//! 2. the pre-step writes: the fleet's input header, both kernels' body
//!    flags, the separation scope, the live and tail inputs, the AI's
//!    header and the hearing journal (before.rs);
//! 3. `world_step`;
//! 4. the post-step drain: the notices into the SimEvents journal and the
//!    presentation capture, the dispenser bindings, the acknowledgement,
//!    the rounds' retired lists (after.rs, drain.rs, capture.rs);
//! 5. `mirrorShots` and `mirror()`: the players' presses and changed views;
//! 6. `consumedEdges`: each client's lanes' outcomes into its edge runtime.
//!
//! Publication stays the caller's (`world_publish_into`), as does input
//! admission (mailbox, `players_input`, fire, acknowledgements).
mod after;
pub mod battles; // matrix-b
mod before;
mod capture;
mod content;
pub use content::{content_tables, content_tables_from_registry};
mod drain;
mod edges;
pub mod journal;
pub mod mount; // matrix-b
mod pads; // matrix-b
#[cfg(test)]
mod presentation_fault; // matrix-b
pub mod random; // matrix-b
pub mod recon;
mod vehicle_input; // matrix-b // matrix-b

pub use capture::Capture;

use crate::abi::context::{Context, Error};
use crate::abi::context_step::Fault;
use crate::gen::{self, BlockHeader as B, PlayerView, WorldTickInput, WorldTickOutput};
use crate::store::block::Block;
use crate::store::StrId;
use crate::world::admit::{Call, Returned};
use journal::Journal;

/// What the host's records name from the battle's content that the
/// world's own tables do not keep, from whoever admitted the world.
#[derive(Clone, Debug, Default)]
pub struct Content {
    /// Each ordnance definition's class (`def.class`), by definition
    /// index: a grenade record's `def`.
    pub ordnance_classes: Vec<Option<StrId>>,
}

/// One network client of the authority (`snapshot-host.js` `clients` and
/// `multiplayer-world.js` `byHandle`, which the local server keeps as one):
/// its players handle, its entity edge runtime, the generation that runtime
/// last answered (`state.edges.generation`), the host's mirror of his body
/// (`p.unit`, a store slot) and of its presses (`u.networkShots`, by button).
struct Client {
    player: u32,
    edges: u32,
    generation: f64,
    unit: Option<u32>,
    presses: [Option<f64>; 2],
}

/// The authority host's state between ticks, owned by the Context.
pub struct Authority {
    pad_owners: pads::Owners,              // matrix-b
    vehicle_input: vehicle_input::Policy,  // matrix-b
    recon_definitions: recon::Definitions, // matrix-b
    /// Whether `world_host_start` took the admission's flags and lives.
    started: bool,
    /// `vs.water?.level`: the level the last step left (NaN before it).
    water_level: f64,
    /// The page's SimEvents journal, fed by the notices' handlers.
    journal: Journal,
    /// `AiJournalFacts.cursor`: how far the AI's hearing has read.
    journal_cursor: usize,
    /// The unit phase's event journal as the host last read it: its epoch
    /// and the word offset after the last event handled.
    event_epoch: u32,
    event_offset: usize,
    /// Each unit's life as the host last saw it (`binding.life`): slot,
    /// store generation, life, for the dispenser bindings a new life needs.
    lives: Vec<(u32, u32, f64)>,
    lives_known: bool,
    /// `c.bodies[i].enabled` as the page kept them: the kernels' enabled
    /// flags written before every step (`bodiesIn`), toggled by the fleet's
    /// body changes and the buildings' shown states.
    enabled: Vec<u8>,
    /// The flags as last written, to tell the owners' own toggles apart.
    written: Vec<u8>,
    /// The body flags the page toggled since the last step (a building's
    /// shown state), applied over the kernel flags before the next step.
    toggles: Vec<(usize, u8)>,
    /// The combat capture's launched rounds awaiting their impact.
    projectiles: Vec<((u8, u32, u32), String)>,
    /// The dynamic capture's rounds awaiting their impact.
    dynamics: Vec<((u8, u32, u32), crate::gen::WireDynamicFact)>,
    /// The presentation capture over the wire's combat capture, when the
    /// host publishes one.
    capture: Option<Capture>,
    /// The content names the records carry.
    content: Content,
    clients: Vec<Client>,
    /// The clients' presses by body (store slot) for the drain.
    presses: Vec<(u32, [Option<f64>; 2])>,
    /// The players the last tick changed (`native.changed()`), for the
    /// caller's own mirrors (a mailbox's synchronize, a roster).
    views: Vec<PlayerView>,
    edge_actions: gen::PlayerActions,
    args: Arguments,
}

impl Default for Authority {
    fn default() -> Self {
        Self {
            recon_definitions: recon::Definitions::default(), // matrix-b
            started: false,
            water_level: f64::NAN,
            pad_owners: pads::Owners::default(), // matrix-b
            vehicle_input: vehicle_input::Policy::default(), // matrix-b
            journal: Journal::new(),
            journal_cursor: 0,
            event_epoch: 0,
            event_offset: 0,
            lives: Vec::new(),
            lives_known: false,
            enabled: Vec::new(),
            written: Vec::new(),
            toggles: Vec::new(),
            projectiles: Vec::new(),
            dynamics: Vec::new(),
            capture: None,
            content: Content::default(),
            clients: Vec::new(),
            presses: Vec::new(),
            views: Vec::new(),
            edge_actions: gen::PlayerActions {
                entry: gen::IaEntry {
                    actions: Vec::with_capacity(1),
                    ..Default::default()
                },
                human: Vec::with_capacity(1),
                ..Default::default()
            },
            args: Arguments {
                words: Vec::with_capacity(512),
                heap: Vec::with_capacity(256),
                answer: Block::default(),
            },
        }
    }
}

impl Authority {
    /// What the host knows before the first tick: the body flags the page
    /// last wrote into the movement kernel during admission (`bodiesIn`
    /// writes every body) and the lives the admission built, whose
    /// dispensers it bound; later changes bind theirs.
    fn start(
        &mut self,
        context: &Context,
        capture: Option<Capture>,
        content: Content,
    ) -> Result<(), String> {
        self.enabled = match context.invoke_export("bank:MOVEMENT_FLAGS", &Call::default()) {
            Ok(Returned::Bytes(bytes)) => bytes,
            other => return Err(format!("bank:MOVEMENT_FLAGS: {other:?}")),
        };
        self.pad_owners = pads::Owners::bind(context)?; // matrix-b
        self.vehicle_input.admit(context)?; // matrix-b
        self.written = self.enabled.clone();
        self.capture = capture;
        self.content = content;
        self.bind_dispensers(context)?;
        self.started = true;
        Ok(())
    }

    /// `LocalServer.step` around `game.step`, but its publication.
    pub(crate) fn tick(
        &mut self,
        context: &Context,
        input: WorldTickInput,
    ) -> Result<WorldTickOutput, String> {
        if !self.started {
            self.start(context, None, Content::default())?;
        }
        let (tick, dt, at) = (input.tick, input.dt, input.at);
        self.advance_edges(context, tick, at)?;
        self.before_step(context, tick, dt)?;
        let output = context
            .world_step(input)
            .map_err(|e| e.message().to_string())?;
        // A refused step changed nothing the host reads.
        if !output.accepted {
            return Ok(output);
        }
        self.water_level = output.level;
        // `game.beforeWorldFinish` mirrors the players' presses before
        // `worldTick.finish()` drains the notices that read them.
        self.mirror_shots(context)?;
        self.acknowledge(context, tick, output.players_events as usize)?;
        self.mirror(context)?;
        self.consumed_edges(context, tick)?;
        Ok(output)
    }
}

impl Context {
    /// The host's state at the first tick: the admission's body flags and
    /// lives, the wire's presentation capture when the host publishes one,
    /// and the content names its records carry. `world_tick` starts without
    /// a capture or names when this was not called.
    pub fn world_host_start(
        &self,
        capture: Option<Capture>,
        content: Content,
    ) -> Result<(), Error> {
        self.host(|host| host.start(self, capture, content))?
            .map_err(Error::host)
    }

    /// Admission can extend definition tables when a network class is prepared.
    /// Refresh names without resetting the authority's flags, journal or clients.
    pub fn world_host_content(&self, explosions: Vec<u32>, content: Content) -> Result<(), Error> {
        self.host(|host| {
            if let Some(capture) = &mut host.capture {
                capture.explosions = explosions;
            }
            host.content = content;
        })
    }

    /// The fleet's flight assists (`vehicles.flightAssists = aiDifficulty
    /// !== 'hard'`): on by default, as at the default difficulty.
    pub fn world_host_flight_assists(&self, on: bool) -> Result<(), Error> {
        self.host(|host| host.vehicle_input.set_assists(on))
    }

    /// A network client joins the authority (`snapshot-host.js` `add`): its
    /// players handle and its entity edge runtime. The host reads the
    /// runtime's generation (`feedback()`, a read) and the player's body.
    pub fn world_host_client(&self, player: u32, edges: u32) -> Result<(), Error> {
        self.host(|host| host.add_client(self, player, edges))?
            .map_err(Error::host)
    }

    /// A network client leaves the authority (`snapshot-host.js` `removeClient`):
    /// drops every client entry bound to `edges` so `advance_edges` and
    /// `consumed_edges` stop driving that edge runtime.
    pub fn world_host_client_leave(&self, edges: u32) -> Result<(), Error> {
        self.host(|host| {
            host.clients.retain(|c| c.edges != edges);
        })
    }

    /// The SimEvents journal, read-only (`worldState`'s events).
    pub fn world_host_journal<T>(&self, read: impl FnOnce(&Journal) -> T) -> Result<T, Error> {
        self.host(|host| read(&host.journal))
    }

    /// The players the last tick changed, read-only.
    pub fn world_host_views<T>(&self, read: impl FnOnce(&[PlayerView]) -> T) -> Result<T, Error> {
        self.host(|host| read(&host.views))
    }

    /// The unit phase's journal as the host last drained it: its epoch and
    /// the word offset after the last event handled (a diagnostic).
    pub fn world_host_events(&self) -> Result<(u32, usize), Error> {
        self.host(|host| (host.event_epoch, host.event_offset))
    }
}

/// The reusable argument and answer arenas of `run_operation_into`
/// (bfe-client's `Arguments`): scalars, then the record's row and heap.
struct Arguments {
    words: Vec<f64>,
    heap: Vec<f64>,
    answer: Block,
}
impl Arguments {
    fn record(
        &mut self,
        scalars: &[f64],
        words: usize,
        put: impl FnOnce(&mut [f64], &mut Vec<f64>),
    ) {
        self.words.clear();
        self.words.extend_from_slice(scalars);
        self.words.resize(scalars.len() + words, 0.0);
        self.heap.clear();
        put(&mut self.words[scalars.len()..], &mut self.heap);
        self.words.extend_from_slice(&self.heap);
    }
    fn run(&mut self, context: &Context, op: u32) -> Result<(), String> {
        context
            .run_operation_into(op, &self.words, &mut self.answer)
            .map_err(|fault| match fault {
                Fault::Context(error) => format!("operation {op}: {}", error.message()),
                other => format!("operation {op} refused: {other:?}"),
            })
    }
    /// The answer block's one row and its heap.
    fn answer(&self) -> (&[f64], &[f64]) {
        block_row(&self.answer.words)
    }
}

/// A block's first row and heap (`Block::read(0)` over borrowed words).
fn block_row(words: &[f64]) -> (&[f64], &[f64]) {
    let stride = words[B::STRIDE] as usize;
    let heap = words[B::HEAP] as usize;
    let heap_words = words[B::HEAP_WORDS] as usize;
    (
        &words[B::WORDS..B::WORDS + stride],
        &words[heap..heap + heap_words],
    )
}

/// One generated record's row and heap as contiguous words.
fn put<const N: usize>(put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Vec<f64> {
    let mut row = [0.0; N];
    let mut heap = Vec::new();
    put(&mut row, &mut heap);
    let mut words = row.to_vec();
    words.extend_from_slice(&heap);
    words
}

/// `bank:NAME` through `invoke_export`: the host's words into an owner's
/// input bank, as the page wrote them through the descriptor's address.
fn write_bank(context: &Context, name: &str, words: &[f64]) -> Result<(), String> {
    let call = Call {
        scratch: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
        ..Default::default()
    };
    match context.invoke_export(&format!("bank:{name}"), &call) {
        Ok(Returned::Refused) => Err(format!("bank {name} refused the write")),
        Ok(_) => Ok(()),
        Err(e) => Err(format!("bank {name}: {}", e.message())),
    }
}

/// The store's unit rows (`KIND_UNIT`, full): slot, store generation, row.
pub fn unit_rows(context: &Context) -> Result<Vec<(u32, u32, gen::Unit)>, String> {
    let units = context
        .export(u32::from(gen::KIND_UNIT), 1)
        .map_err(|e| e.message().to_string())?;
    let stride = units[B::STRIDE] as usize;
    let rows = units[B::ROWS] as usize;
    let heap_at = units[B::HEAP] as usize;
    let heap = &units[heap_at..heap_at + units[B::HEAP_WORDS] as usize];
    let mut out = Vec::with_capacity(rows);
    for i in 0..rows {
        let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
        let mut unit = gen::Unit::default();
        unit.take_full(row, heap);
        out.push((row[0] as u32, row[1] as u32, unit));
    }
    Ok(out)
}

/// The seat leases (`KIND_SEAT_LEASE`, full), by slot.
fn lease_rows(context: &Context) -> Result<Vec<(u32, gen::SeatLease)>, String> {
    let words = context
        .export(u32::from(gen::KIND_SEAT_LEASE), 1)
        .map_err(|e| e.message().to_string())?;
    let stride = words[B::STRIDE] as usize;
    let rows = words[B::ROWS] as usize;
    let heap_at = words[B::HEAP] as usize;
    let heap = &words[heap_at..heap_at + words[B::HEAP_WORDS] as usize];
    let mut out = Vec::with_capacity(rows);
    for i in 0..rows {
        let row = &words[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
        let mut lease = gen::SeatLease::default();
        lease.take_full(row, heap);
        out.push((row[0] as u32, lease));
    }
    Ok(out)
}

/// A unit's store generation (the reference's) by slot, 0 for an unknown slot.
fn unit_generation(units: &[(u32, u32, gen::Unit)], slot: u32) -> u32 {
    units
        .iter()
        .find(|(s, _, _)| *s == slot)
        .map_or(0, |(_, g, _)| *g)
}

/// `b.strings.id(text)`: the store's id for a UTF-16 string.
fn intern(context: &Context, text: &str) -> Result<StrId, String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    context.intern(&units).map_err(|e| e.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{InputMailboxSetup, PlayerSetup};
    use crate::world::admit::{Call, Returned};

    #[test]
    fn world_host_client_leave_removes_joined_edge_runtime() {
        let context = Context::new();
        context.players_init().expect("players_init");
        let setup = InputMailboxSetup {
            epoch: 1.0,
            capacity: 256.0,
            codes: Vec::new(),
        };
        let mut call = Call::default();
        call.row.resize(InputMailboxSetup::WORDS, 0.0);
        setup.put(&mut call.row, &mut call.heap);
        let mailbox = match context
            .invoke_export("input_mailbox_create", &call)
            .expect("input_mailbox_create")
        {
            Returned::Scalar(id) => id as u32,
            other => panic!("unexpected input_mailbox_create return: {other:?}"),
        };
        assert_ne!(mailbox, 0);
        let player = context
            .players_create(&PlayerSetup {
                mailbox,
                reliable_fire: true,
                separate_actions: true,
                ordinal: 1.0,
                team: 1.0,
                ..Default::default()
            })
            .expect("players_create");
        assert_ne!(player, 0);
        let edges = match context
            .invoke_export("wire_edges_create", &Call::default())
            .expect("wire_edges_create")
        {
            Returned::Scalar(id) => id as u32,
            other => panic!("unexpected wire_edges_create return: {other:?}"),
        };
        assert_ne!(edges, 0);

        context
            .world_host_client(player, edges)
            .expect("world_host_client");
        assert_eq!(context.host(|h| h.clients.len()).unwrap(), 1);
        assert_eq!(context.host(|h| h.clients[0].edges).unwrap(), edges);

        // Leaving with an unrelated edge handle keeps the joined client.
        context
            .world_host_client_leave(edges + 1)
            .expect("world_host_client_leave other");
        assert_eq!(context.host(|h| h.clients.len()).unwrap(), 1);

        // Leaving with the client's edge handle removes it from the host.
        context
            .world_host_client_leave(edges)
            .expect("world_host_client_leave matching");
        assert_eq!(context.host(|h| h.clients.len()).unwrap(), 0);
    }
}
