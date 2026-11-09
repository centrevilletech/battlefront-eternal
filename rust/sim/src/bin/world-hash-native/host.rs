//! The local server's input plumbing, as `tools/world-hash.mjs` drives it
//! through `src/runtime/local-server.js`, `snapshot-host.js` and
//! `multiplayer-world.js`, over the native world API instead of the
//! WebAssembly exports. Order within a tick (world-hash.mjs lines 187-203):
//! `world.input(id, command)` (the mailbox's admit and push, then
//! `players_input`), `world.fire(id, edge)`, then `server.step()`, whose
//! host duties around the step are the simulation's own
//! (`Context::world_tick`), then `publish()`; then the frame's snapshot acks
//! (`host.input` of a keepalive, `policy.acceptAck`). The JavaScript
//! bookkeeping whose values reach the native side (the mailbox's slot
//! identities) is modelled here field for field.
use bfe_sim::abi::context::Context;
use bfe_sim::abi::context_step::{bank, Fault, Publication};
use bfe_sim::gen::{
    self, BlockHeader as B, IfEdge, ImEntry, ImField, ImLook, ImOperation, ImResult, ImState,
    InputMailboxCall, PlayerFire, PlayerLanes, WiView, WirePublicationLink, WireWorldFacts,
};
use bfe_sim::host::{Capture, Content};
use bfe_sim::store::block::Block;
use bfe_sim::store::StrId;
use bfe_sim::world::admit::{Call, Returned};
use serde_json::Value;

use super::admission::Admitted;

/// `createWireHostBridge.add`: the first client's native id is 1
/// (`nextId = 1`), and the local server adds exactly one.
pub const LOCAL_CLIENT: u32 = 1;
/// `wire-store-source.js`: the first actor's peer id (`nextPeer = 1`).
const LOCAL_PEER: u32 = 1;
/// `createLocalServer`: `new InputMailbox(1, 256, bridge)`; the JavaScript
/// keeps `capacity + 8` reference slots.
const MAILBOX_CAPACITY: usize = 256;
const SNAPSHOT_ACK: f64 = 4.0;
/// `publicationView` flag bits (src/runtime/input-view.js).
const VIEW_TICK_FLAG: f64 = 128.0;
/// The seven view keys `InputMailbox` admits, in `keys` order; only
/// `viewTick` (index 4) occurs in a matrix command.
const VIEW_TICK_KEY: usize = 4;

/// One `references[i]` of the JavaScript InputMailbox: whether a command
/// still occupies the slot, whether the native queue still has it
/// (`pending`), and whether the native state borrows it (`held`).
#[derive(Clone, Copy, Default)]
struct Slot {
    command: bool,
    pending: bool,
    held: bool,
    pending_index: usize,
}

/// `InputMailbox` (src/sim/wasm/runtime/input-mailbox.js): the slot
/// allocator whose ids become the entry's identity, `down` token, look
/// tokens (`id*2048+i`) and view tokens (`id*8+key`).
struct Mailbox {
    slots: Vec<Slot>,
    cursor: usize,
    pending: Vec<usize>,
    retained: Vec<usize>,
    previous_retained: Vec<usize>,
    /// `this.exchange`: the last owned run's result, whose `retired` list
    /// `cleanup()` reads even after `synchronize()` replaced the state.
    last_retired: Vec<f64>,
    last_state: ImState,
}

impl Mailbox {
    fn new() -> Self {
        Self {
            slots: vec![Slot::default(); MAILBOX_CAPACITY + 8],
            cursor: 0,
            pending: Vec::new(),
            retained: Vec::new(),
            previous_retained: Vec::new(),
            last_retired: Vec::new(),
            last_state: ImState::default(),
        }
    }
    /// `retireReference(row)`.
    fn retire(&mut self, id: usize) {
        let row = &mut self.slots[id - 1];
        if !row.pending {
            return;
        }
        row.pending = false;
        let at = row.pending_index;
        let last = self.pending.pop().expect("a pending reference");
        if at < self.pending.len() {
            self.pending[at] = last;
            self.slots[last - 1].pending_index = at;
        }
    }
    /// What `run()` does with its result before the caller reads it.
    fn after_run(&mut self, result: &ImResult) {
        self.last_state = result.state.clone();
        self.last_retired = result.retired.clone();
        for id in &result.retired {
            self.retire(*id as usize);
        }
    }
    /// `cleanup()`: held owners swap, then every slot neither pending nor
    /// held among the old owners and the last run's retired ids is freed.
    fn cleanup(&mut self) {
        std::mem::swap(&mut self.retained, &mut self.previous_retained);
        let old = std::mem::take(&mut self.previous_retained);
        self.retained.clear();
        for id in &old {
            self.slots[*id - 1].held = false;
        }
        if let Some(down) = self.last_state.down {
            let id = down as usize;
            self.slots[id - 1].held = true;
            self.retained.push(id);
        }
        for field in &self.last_state.view {
            if field.value < 8.0 {
                continue;
            }
            let id = (field.value / 8.0).floor() as usize;
            if !self.slots[id - 1].held {
                self.slots[id - 1].held = true;
                self.retained.push(id);
            }
        }
        for id in &old {
            let row = &mut self.slots[*id - 1];
            if !row.pending && !row.held {
                row.command = false;
            }
        }
        for id in &self.last_retired {
            let row = &mut self.slots[*id as usize - 1];
            if !row.pending && !row.held {
                row.command = false;
            }
        }
        self.previous_retained = old;
    }
    /// The slot `push()` takes: the first free one from the cursor.
    fn take_slot(&mut self) -> Option<usize> {
        let n = self.slots.len();
        let slot = (0..n)
            .map(|i| (self.cursor + i) % n)
            .find(|i| !self.slots[*i].command)?;
        self.cursor = (slot + 1) % n;
        Some(slot + 1)
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
    fn scalars(&mut self, scalars: &[f64]) {
        self.words.clear();
        self.words.extend_from_slice(scalars);
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
pub fn block_row(words: &[f64]) -> (&[f64], &[f64]) {
    let stride = words[B::STRIDE] as usize;
    let heap = words[B::HEAP] as usize;
    let heap_words = words[B::HEAP_WORDS] as usize;
    (
        &words[B::WORDS..B::WORDS + stride],
        &words[heap..heap + heap_words],
    )
}

pub struct Host {
    pub random_bindings: super::random::Bindings, // matrix-b
    pub context: Context,
    pub admitted: Admitted,
    args: Arguments,
    mailbox: Mailbox,
    /// `b.strings.id(name)` for the held key names, interned once.
    codes: Vec<(String, StrId)>,
    view_tick_key: StrId,
    pub publication: Publication,
    /// The local player's unit (store slot) as the views last named it.
    pub player_unit: Option<u32>,
    /// The match seed (`--seed`), the page's stream seed.
    pub seed: u32,
}

impl Host {
    pub fn new(context: Context, admitted: Admitted, seed: u32) -> Result<Self, String> {
        let view_tick_key = intern(&context, "viewTick")?;
        Ok(Self {
            random_bindings: super::random::Bindings::default(), // matrix-b
            context,
            admitted,
            args: Arguments {
                words: Vec::with_capacity(512),
                heap: Vec::with_capacity(256),
                answer: Block::default(),
            },
            mailbox: Mailbox::new(),
            codes: Vec::new(),
            view_tick_key,
            publication: Publication::default(),
            player_unit: None,
            seed,
        })
    }

    fn code(&mut self, name: &str) -> Result<StrId, String> {
        if let Some((_, id)) = self.codes.iter().find(|(n, _)| n == name) {
            return Ok(*id);
        }
        let id = intern(&self.context, name)?;
        self.codes.push((name.to_string(), id));
        Ok(id)
    }

    /// `InputMailbox.run(op)`: one owned run with the call record, its
    /// result decoded and applied to the slot model.
    fn mailbox_run(&mut self, call: &InputMailboxCall) -> Result<ImResult, String> {
        let mailbox = self.admitted.handles.mailbox as f64;
        self.args
            .record(&[mailbox], InputMailboxCall::WORDS, |row, heap| {
                call.put(row, heap)
            });
        self.args
            .run(&self.context, gen::OPERATION_INPUT_MAILBOX_OWNED_RUN)?;
        let (row, heap) = self.args.answer();
        let result = ImResult::take(row, heap);
        self.mailbox.after_run(&result);
        Ok(result)
    }

    /// `world.input(id, command)` for the frame's recorded command:
    /// `InputMailbox.push` (admit, slot, push, cleanup) and
    /// `native.input(handle, now, lead, accepted)`. `world_tick` is
    /// `game.worldClock.tick` before the step.
    pub fn input(&mut self, command: &Value, world_tick: f64, now: f64) -> Result<bool, String> {
        let sequence = command["sequence"].as_f64().unwrap_or(0.0);
        let tick = command["tick"].as_f64().unwrap_or(0.0);
        let generation = command["generation"].as_f64().unwrap_or(0.0);
        let epoch_matches = command["epoch"].as_f64() == Some(1.0);
        // multiplayer-world.js input(): a command stamped more than 120
        // ticks from the world's is refused before the mailbox sees it.
        if (tick - world_tick).abs() > 120.0 {
            return Ok(false);
        }
        let admit = InputMailboxCall {
            op: ImOperation::Admit,
            entry: None,
            sequence,
            epoch_matches,
            tick: 0.0,
            generation: 0.0,
        };
        let admitted = self.mailbox_run(&admit)?.accepted;
        if !admitted {
            // `run('admit')` refused: no cleanup runs (push returned early).
            self.players_input(now, tick - world_tick, false)?;
            return Ok(false);
        }
        let Some(id) = self.mailbox.take_slot() else {
            return Err("input queue full".into());
        };
        {
            let row = &mut self.mailbox.slots[id - 1];
            row.command = true;
            row.pending = true;
        }
        let mut codes = Vec::new();
        for name in command["down"].as_array().into_iter().flatten() {
            codes.push(self.code(name.as_str().unwrap_or(""))?);
        }
        let mut actions = Vec::new();
        for name in command["actions"].as_array().into_iter().flatten() {
            actions.push(self.code(name.as_str().unwrap_or(""))?);
        }
        let look: Vec<ImLook> = command["look"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, point)| ImLook {
                point: [
                    point[0].as_f64().unwrap_or(f64::NAN),
                    point[1].as_f64().unwrap_or(f64::NAN),
                ],
                object: Some((id * 2048 + i) as f64),
            })
            .collect();
        // publicationView(command, e.publication): only viewTick can be
        // set on a matrix command; every other view field is undefined.
        let view_tick = command.get("viewTick").and_then(Value::as_f64);
        let publication = WiView {
            flags: if view_tick.is_some() {
                VIEW_TICK_FLAG
            } else {
                0.0
            },
            profile: 0.0,
            profile_found: true,
            mouse_scale: f64::NAN,
            aspect: f64::NAN,
            view_tick: view_tick.unwrap_or(f64::NAN),
            sensitivity: f64::NAN,
            curve: f64::NAN,
            axes: [0.0; 4],
            buttons: 0.0,
        };
        let view = match view_tick {
            Some(_) => vec![ImField {
                key: self.view_tick_key,
                value: (id * 8 + VIEW_TICK_KEY) as f64,
            }],
            None => Vec::new(),
        };
        let entry = ImEntry {
            publication,
            codes,
            identity: id as f64,
            sequence,
            tick,
            generation,
            down: id as f64,
            actions,
            look,
            look_total: None,
            view,
            human: Vec::new(),
            fire: Vec::new(),
        };
        let push = InputMailboxCall {
            op: ImOperation::Push,
            entry: Some(entry),
            sequence,
            epoch_matches,
            tick: 0.0,
            generation: 0.0,
        };
        let accepted = self.mailbox_run(&push)?.accepted;
        if accepted {
            let row = &mut self.mailbox.slots[id - 1];
            row.pending_index = self.mailbox.pending.len();
            self.mailbox.pending.push(id);
        } else {
            let row = &mut self.mailbox.slots[id - 1];
            row.pending = false;
            row.command = false;
        }
        self.mailbox.cleanup();
        self.players_input(now, tick - world_tick, accepted)?;
        Ok(accepted)
    }

    /// `native.input(p.handle, performance.now(), lead, accepted)`.
    fn players_input(&mut self, at: f64, lead: f64, accepted: bool) -> Result<(), String> {
        self.args.scalars(&[
            self.admitted.handles.player as f64,
            at,
            lead,
            f64::from(u8::from(accepted)),
        ]);
        self.args.run(&self.context, gen::OPERATION_PLAYERS_INPUT)
    }

    /// `world.fire(id, edge)` -> `native.fire(p.handle, edge, worldClock.tick,
    /// performance.now())`. The host's shape checks pass for a recorded edge.
    pub fn fire(&mut self, edge: &Value, world_tick: f64, now: f64) -> Result<bool, String> {
        let fire = PlayerFire {
            edge: IfEdge {
                id: edge["id"].as_f64().unwrap_or(0.0),
                external_id: edge.get("externalId").and_then(Value::as_f64),
                tick: edge["tick"].as_f64().unwrap_or(0.0),
                generation: edge["generation"].as_f64().unwrap_or(0.0),
                button: edge["button"].as_f64().unwrap_or(0.0),
                pressed: edge["pressed"].as_bool().unwrap_or(false),
                aim_yaw: edge.get("aimYaw").and_then(Value::as_f64),
                aim_pitch: edge.get("aimPitch").and_then(Value::as_f64),
                view_tick: edge.get("viewTick").and_then(Value::as_f64),
            },
            epoch_one: edge["epoch"].as_f64() == Some(1.0),
            pressed_valid: edge["pressed"].is_boolean(),
            tick: world_tick,
            at: now,
        };
        self.args.record(
            &[self.admitted.handles.player as f64],
            PlayerFire::WORDS,
            |row, heap| fire.put(row, heap),
        );
        self.args.run(&self.context, gen::OPERATION_PLAYERS_FIRE)?;
        let (row, _) = self.args.answer();
        Ok(row[0] != 0.0)
    }

    /// `LaneView.read(drain)`: the lane's counters and, when draining, its
    /// outcomes.
    fn lanes(&self, lane: u32, drain: bool) -> Result<PlayerLanes, String> {
        self.context
            .players_lanes(
                self.admitted.handles.player,
                lane,
                u32::from(drain),
                |words| {
                    words.map(|w| {
                        let (row, heap) = block_row(w);
                        PlayerLanes::take(row, heap)
                    })
                },
            )
            .map_err(|e| e.message().to_string())?
            .ok_or_else(|| "unknown player".to_string())
    }

    /// What `mirror()` does with the local player's view after the step,
    /// beyond the simulation's own mirror: his mailbox synchronizes, and
    /// his body is the view's.
    pub fn after_step(&mut self) -> Result<(), String> {
        let handle = self.admitted.handles.player;
        let view = self
            .context
            .world_host_views(|views| {
                views
                    .iter()
                    .find(|v| v.handle == handle)
                    .map(|v| (v.deleted, v.unit.map(|u| u.slot)))
            })
            .map_err(|e| e.message().to_string())?;
        if let Some((deleted, unit)) = view {
            if deleted {
                return Err("the local player was retired".into());
            }
            if unit.is_some() {
                self.player_unit = unit;
            }
            self.synchronize()?;
        }
        Ok(())
    }

    /// `InputMailbox.synchronize()`: `input_mailbox_inspect`, then the
    /// queue's identities stay, the rest of the pending slots retire.
    fn synchronize(&mut self) -> Result<(), String> {
        let call = Call {
            args: vec![self.admitted.handles.mailbox as f64],
            ..Default::default()
        };
        let words = match self.context.invoke_export("input_mailbox_inspect", &call) {
            Ok(Returned::Row(words)) => words,
            Ok(_) => return Err("input mailbox inspection refused".into()),
            Err(e) => return Err(format!("input_mailbox_inspect: {}", e.message())),
        };
        let (row, heap) = block_row(&words);
        let state = ImState::take(row, heap);
        let queued: Vec<usize> = state.queue.iter().map(|e| e.identity as usize).collect();
        self.mailbox.last_state = state;
        self.mailbox.cleanup();
        let mut i = 0;
        while i < self.mailbox.pending.len() {
            let id = self.mailbox.pending[i];
            if queued.contains(&id) {
                i += 1;
                continue;
            }
            self.mailbox.retire(id);
            if !self.mailbox.slots[id - 1].held {
                self.mailbox.slots[id - 1].command = false;
            }
        }
        Ok(())
    }

    /// `server.publish()`: `host.publish({at: performance.now(), unixMs:
    /// Date.now()})`, whose facts row the store source writes with one link
    /// per actor (`actor.actions.discarded`, no round trip, no path).
    pub fn publish(&mut self, tick: f64, now: f64, unix_ms: f64) -> Result<(), String> {
        let discarded = self.lanes(0, false)?.actions_discarded;
        let facts = WireWorldFacts {
            tick,
            at: now,
            unix_ms,
            dropped_ticks: 0.0,
            links: vec![WirePublicationLink {
                id: LOCAL_PEER,
                discarded_actions: discarded,
                rtt: f64::NAN,
                has_kind: false,
                relay: false,
                has_family: false,
                family_six: false,
            }],
        };
        let mut row = [0.0; WireWorldFacts::WORDS];
        let mut heap = Vec::new();
        facts.put(&mut row, &mut heap);
        let mut words = row.to_vec();
        words.extend_from_slice(&heap);
        self.context
            .world_publish_into(
                self.admitted.handles.publication,
                &words,
                &mut self.publication,
            )
            .map_err(|e| e.message().to_string())
    }

    /// `policy.acceptAck(state, {epoch, tick})`: `wire_host_op(handle, 4,
    /// nativeId, tick, epoch)` for a snapshot ack keepalive the page sent.
    pub fn ack(&mut self, tick: f64, epoch: f64) -> Result<(), String> {
        self.args.scalars(&[
            self.admitted.handles.host as f64,
            SNAPSHOT_ACK,
            LOCAL_CLIENT as f64,
            tick,
            epoch,
        ]);
        self.args.run(&self.context, gen::OPERATION_WIRE_HOST_OP)
    }
}

impl Host {
    /// What the JavaScript host knows about the local player after admission
    /// and before the first tick, and the host duties' start: the page's
    /// one draw from its gameplay stream, the authority's flags, lives and
    /// capture with the explosions' registry ids, the local client (his
    /// edge runtime) and his unit. His
    /// Character is the one human in the roster (`roster.reserve(team, id,
    /// name)`); its `unit` names his body.
    pub fn learn_player(&mut self, explosions: Vec<u32>, content: Content) -> Result<(), String> {
        self.materialize_gameplay_stream()?;
        let handles = &self.admitted.handles;
        let capture = Capture {
            combat: handles.combat,
            presentation: handles.presentation,
            store_source: handles.store_source,
            explosions,
        };
        self.context
            .world_host_start(Some(capture), content)
            .map_err(|e| e.message().to_string())?;
        self.context
            .world_host_client(handles.player, handles.edges)
            .map_err(|e| e.message().to_string())?;
        let characters = self
            .context
            .export(u32::from(gen::KIND_CHARACTER), 0)
            .map_err(|e| e.message().to_string())?;
        let stride = characters[B::STRIDE] as usize;
        let rows = characters[B::ROWS] as usize;
        let heap_at = characters[B::HEAP] as usize;
        let heap = &characters[heap_at..heap_at + characters[B::HEAP_WORDS] as usize];
        for i in 0..rows {
            // An export row: the slot and generation words, then the fields.
            let row = &characters[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
            let mut character = gen::Character::default();
            character.take_export(row, heap);
            if character.human {
                self.player_unit = character.unit.map(|r| r.slot);
            }
        }
        self.random_bindings = super::random::Bindings::from(self.context.world_host_random(self.seed, self.player_unit).map_err(|e| e.message().to_string())?); // matrix-b
        Ok(())
    }

    /// The page drew once from its `gameplay` stream while it placed its
    /// player (`points[Math.floor(gameplayRandom() * points.length)]` in
    /// src/game.js), so the stream exists on its side with that state, and
    /// `F3Random.push` hands it to the world before the first tick: entry 0
    /// of the random bank (the first `configure` binding) present, its state
    /// the stream's after one draw. The native world drew nothing from it;
    /// a native admission that places the player draws it itself.
    fn materialize_gameplay_stream(&mut self) -> Result<(), String> {
        use bfe_sim::gen::F3RandomState;
        let mut words = Vec::new();
        self.context
            .read_bank(bank::F3_RANDOM_STATES, &mut words)
            .map_err(|e| e.message().to_string())?;
        if words.len() < F3RandomState::WORDS {
            return Ok(());
        }
        let mut stream =
            jsrt::rng::ClassicRandom::from_state(jsrt::hash::fnv1a_mix(self.seed, "gameplay"));
        stream.next_u32();
        let row = F3RandomState {
            state: stream.state,
            present: true,
            sequence: 0,
        };
        let mut heap = Vec::new();
        row.put(&mut words[..F3RandomState::WORDS], &mut heap);
        let call = Call {
            scratch: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
            ..Default::default()
        };
        match self.context.invoke_export("bank:F3_RANDOM_STATES", &call) {
            Ok(Returned::Refused) => Err("bank F3_RANDOM_STATES refused the write".into()),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("bank F3_RANDOM_STATES: {}", e.message())),
        }
    }
}

/// `b.strings.id(text)`: the store's id for a UTF-16 string.
fn intern(context: &Context, text: &str) -> Result<StrId, String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    context.intern(&units).map_err(|e| e.message().to_string())
}
