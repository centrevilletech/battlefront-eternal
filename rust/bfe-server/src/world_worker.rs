//! Native authoritative world host (`src/world-worker.js`, `SnapshotHost` in
//! `src/runtime/snapshot-host.js`, and `createMultiplayerWorld` in `src/runtime/multiplayer-world.js`).
//!
//! Why `NativeWorldHost` owns the simulation context and wire codecs on the server's tick thread:
//! In the Node dedicated server (`src/world-worker.js`), the worker thread loads the mission
//! (`createGame` + `createMultiplayerWorld` + `createSnapshotHost`), steps the simulation at 60 Hz,
//! and publishes accumulated entity state at 20 Hz (`publish()` in `src/world-worker.js`).
//!
//! ## Why `WORLD_SLICE_MS = 4` ms does not apply in `bfeserver`, and what bounds a late tick
//!
//! In the Node dedicated server (`export const WORLD_SLICE_MS = 4` and `WorldRouter.drainSlice()` in
//! `src/game-session.js`), the session router runs on the single-threaded JavaScript event loop that
//! also services UDP socket receive callbacks, `bfe1` handshakes, `/info`, and operator HTTP requests.
//! Because structured-clone `postMessage` deserialization and 64-client fanout on that single JS
//! thread can stall incoming datagrams, `WorldRouter.drainSlice()` budgets `WORLD_SLICE_MS = 4` ms per
//! `setImmediate` turn (`WORLD_SLICE_MS / 2 = 2` ms for draining worker inbox messages and the
//! remaining budget for flushing `input-ack`, `entity-edge-results`, and `BNS1`/`BNO1` snapshots).
//!
//! In `bfeserver`, that cooperative event-loop slice budget does not apply by design:
//! 1. **Dedicated 60 Hz OS thread**: `NativeWorldHost` and its 60 Hz simulation + 20 Hz
//!    `world_publish_into` + UDP poll/flush loop run on their own dedicated OS thread
//!    (`tick_thread` in `src/local_server.rs`), isolated from operator HTTP, `/info` TCP, and LAN
//!    discovery threads, with zero cross-thread `postMessage` serialization (`world_publish_into`
//!    writes directly into pre-allocated `Publication` buffers in-place).
//! 2. **What bounds a late tick**:
//!    - **Outer 60 Hz wall-clock catch-up bound (`4 * TICK_DURATION` = `4 / 60` s ≈ `66.7` ms)**:
//!      In `src/local_server.rs` (matching `count < 4` and `debt > 8 / 60` in `src/world-worker.js`),
//!      when `now > next_tick + TICK_DURATION * 4`, accumulated wall-clock debt beyond 4 ticks is
//!      dropped (`dropped_ticks` incremented and reported in `WireWorldFacts::DROPPED_TICKS` and
//!      per-client `input-ack.dropped`) and `next_tick` resets to `now + TICK_DURATION`. A transient
//!      OS scheduling stall or cgroup CPU throttle can therefore never trigger an unbounded catch-up
//!      spiral.
//!    - **Inner `WorldClock` step bound (`battle_clock()`)**: `WorldTickInput.clock` caps `max_steps`
//!      per invocation and records `clock.overloads`, `clock.dropped`, and `clock.step_cost`.
//!    - **Per-client state bandwidth and staleness bounds**: `Publisher::run` in `bfe_sim` enforces
//!      each client's sliding-window `state_bytes_per_second` budget (`c.budget_used(at) < host.budget`,
//!      falling back to 10 Hz own-only datagrams when exceeded) and `stale_after` window (`5_000` ms),
//!      while `UdpGameServer::send_state` drops outbound state datagrams older than
//!      `STATE_STALE_MS = 250` ms.
//!
//! ## Pipeline over `bfe_sim::abi::context::Context`
//!
//! 1. **Cold world admission (`Context::world_admit` in `rust/sim/src/abi/context_admit.rs`)**:
//!    `load_from_content` parses the battle query (`?map=...&side=...&ai=...&seed=...`) into
//!    `bfe_sim::world::admit::Options` and calls `context.world_admit(&files, &admit_options)`.
//!    - When `admit::provided()` is `true` and admission succeeds, `NativeWorldHost` calls
//!      `context.world_step_init(admitted.setup)`, stores `admitted.handles` and `admitted.player_id`,
//!      configures the 20 Hz snapshot cadence (`interval = 3`), uses `admitted.registry`, and sets
//!      `world_admitted = true`.
//!    - While the `admit` lane has `admit::provided() == false` (returning `ErrorKind::Unavailable`),
//!      `NativeWorldHost` falls back to `players_init()` + `build_entity_registry_descriptor` and
//!      sets `world_admitted = false` so `/healthz` honestly reports `multiplayerReady: false`
//!      (a server that has not admitted a real world through `Context::world_admit` must never
//!      claim `multiplayerReady: true`).
//! 2. **Client binary input wire framing (`decode_snapshot_ack_keepalive` and `decode_entity_input`,
//!    porting `decodeSnapshotAckKeepalive` and `decodeEntityInput` in `src/runtime/network-entity-input.js`)**:
//!    - `BNA1` (`0x31414e42`, 10 bytes): `[magic: u32 LE, epoch: u16 LE, tick: u32 LE]`.
//!    - `BNI1` (`0x31494e42`, `16..=1100` bytes): header `[magic: u32, epoch: u32, generation: u32,
//!      nv: u8 (<= 8), nc: u8 (<= 8), ne: u8 (<= 32), na: u8 (<= 1)]`, followed by `nv` view
//!      descriptors (`flags <= 1023`, `profile == 0`, optional `mouse_scale`, `aspect`, `view_tick`,
//!      flight `sensitivity`/`curve`, and 4 gamepad `axes` + `buttons`), `nc` held-key/look commands
//!      (`16-byte` bitmask over `INPUT_CODES` + `[look_x, look_y]`), `ne` discrete edges (`kind 0..=5`:
//!      action, fire with aim, team select `1..=7`, spawn `[class_id, post_id]`, suicide, cancel),
//!      and optional trailing `[epoch: u16, tick: u32]` snapshot ack with zero trailing bytes.
//! 3. **Zero-allocation publication decoding (`decode_publication_block`, porting `publish()` in
//!    `src/world-worker.js` and `WorldRouter.route` in `src/game-session.js`)**:
//!    Reads `WirePublicationOutput` from the reused `Publication` block (`world_publish_into`) and
//!    fans out shared `BNS1` baseline groups (`WirePublishedGroup`), per-client `BNO1` own-player
//!    datagrams (`WirePublishedOwn`), `input-ack` (`WirePublishedAck`), `entity-edge-results`
//!    (`WirePublishedEdges`), and base64-encoded `gameplay-events` (`WirePublishedRecord` /
//!    `WirePublishedReliable`).
//!
//! Detailed method-by-method correspondence between `NativeWorldHost` (`world_worker.rs`),
//! `src/world-worker.js`, `src/runtime/snapshot-host.js`, and `src/game-session.js`:
//! - `NativeWorldHost::load_from_content(root, query, epoch)`:
//!   Ports `boot()` in `src/world-worker.js` and `createGame` + `createLocalServer` +
//!   `createSnapshotHost` in `src/game.js` / `src/runtime/snapshot-host.js`. Verifies the content
//!   fingerprint (`bfe_content_native::fingerprint::hash`), builds the canonical `entity-registry`
//!   descriptor (`build_entity_registry_descriptor`), creates `bfe_sim::abi::context::Context`,
//!   and calls `Context::world_admit(&files, &admit_options)`:
//!   - When `world_admit` returns `Ok(admitted)`: initializes `world_step_init(admitted.setup)`,
//!     retains `admitted.handles` (`mailbox`, `player`, `publication`, `host`, `edges`, `client`),
//!     enables reliable outcome retention (`players_outcomes_enable`), sets `wire_host` snapshot
//!     cadence to `PUBLISH_EVERY_TICKS = 3` (`20` Hz), resets `wire_edges` to `(epoch, 1)`, and
//!     sets `world_admitted = true`.
//!   - When `world_admit` returns `Err(err)` with `SimErrorKind::Unavailable`: falls back to
//!     `context.players_init()` for transport/protocol harness tests while keeping
//!     `world_admitted = false` so `/healthz` reports `multiplayerReady: false`.
//! - `NativeWorldHost::add_player(peer_id, name, spectator, auto_spawn, team_select)`:
//!   Ports `handle("join")` in `src/world-worker.js`, `world.addPlayer`, and
//!   `entityHost.addClient(id)` in `src/runtime/snapshot-host.js`. Assigns the lowest free native
//!   client slot in `1..=256`, registers or resets the client in `wire_host` (`wire_host_op` +
//!   `wire_host_set`) and `wire_publish` (`wire_publish_bind`), and returns `PlayerWelcome`.
//! - `NativeWorldHost::remove_player(peer_id)`:
//!   Ports `leave(client)` in `src/world-worker.js` and `entityHost.removeClient(id)` in
//!   `src/runtime/snapshot-host.js`, releasing the client slot and dropping its `wire_edges` bank.
//! - `NativeWorldHost::handle_start(peer_id, now_ms)` & `handle_resync(peer_id, now_ms)`:
//!   Ports `handle("start")` and `handle("resync")` in `src/world-worker.js` (`setStarted`,
//!   `setReady`, and `resync` on `wire_host` via `wire_host_op(9, ...)`).
//! - `NativeWorldHost::handle_edge_ack(peer_id, message, now_ms)`:
//!   Ports `edgeAck(id, message)` in `src/runtime/snapshot-host.js` (`WeCall { op: 7.0 }` on
//!   `wire_edges_run`).
//! - `NativeWorldHost::handle_input(peer_id, bytes, now_ms)`:
//!   Ports `input(id, bytes)` in `src/runtime/snapshot-host.js`:
//!   - 10-byte `BNA1` (`0x31414e42`) snapshot-ack keepalives call `wire_host_op(4, native_id, tick, epoch)`.
//!   - `BNI1` (`0x31494e42`) entity-input frames update `snapshot_ack`, push discrete gameplay
//!     edges (`WeCall { op: 1.0 }`), and submit per-tick movement/look commands into
//!     `input_mailbox_owned_run` (`ImOperation::Push`) and `players_input`.
//! - `Context::world_host_start`, `Context::world_host_client`, `Context::world_host_client_leave`,
//!   and `Context::world_tick` (`rust/sim/src/host/mod.rs`):
//!   `load_from_content` calls `Context::world_host_start(Some(Capture { combat, presentation, store_source }))`
//!   after `Context::world_admit`. `add_player` registers each client with
//!   `Context::world_host_client(player, edges)`, and `remove_player` unregisters its edge runtime
//!   with `Context::world_host_client_leave(edges)`.
//! - `NativeWorldHost::step(now_ms, unix_ms)`:
//!   Ports `step()` and `publish()` in `src/world-worker.js`, executing `context.world_tick`
//!   (which runs `advanceEdges`, the pre-step owner input banks and AI hearing journal,
//!   `world_step`, the post-step notice drain into the `SimEvents` journal and presentation
//!   capture, `mirrorShots`, `mirror`, and `consumedEdges` in the exact JavaScript order),
//!   `SimBattle` victory inspection, and `context.world_publish_into` + `decode_publication_block`.
//!
//! Wire format and rate/budget constants enforced by `world_worker.rs` (`src/world-worker.js`,
//! `src/runtime/network-entity-input.js`, `src/runtime/network-entity-snapshot.js`, and
//! `src/runtime/snapshot-host.js`):
//! - `INPUT_MAGIC = 0x31494e42` (`"BNI1"` little-endian): client entity-input packet header magic.
//! - `ACK_MAGIC = 0x31414e42` (`"BNA1"` little-endian): 10-byte client snapshot-ack keepalive magic.
//! - `SNAPSHOT_MAGIC = 0x31534e42` (`"BNS1"` little-endian): shared entity snapshot baseline/delta magic.
//! - `OWN_MAGIC = 0x314f4e42` (`"BNO1"` little-endian): per-client own-player state datagram magic.
//! - `PUBLISH_EVERY_TICKS = 3`: 20 Hz snapshot publication cadence (`60 Hz / 3 = 20 Hz`), configured
//!   via `wire_host_set(host_handle, 0, 0, 3.0)` on `Context::world_admit`.
//! - `MAX_LEAD_TICKS = 120.0`: maximum future tick lead (`cmd.tick - self.tick <= 120.0`) accepted by
//!   `handle_input` before a client command is discarded (`input()` in `src/runtime/snapshot-host.js`).
//! - `MAX_VIEWS = 8`, `MAX_COMMANDS = 32`, `MAX_EDGES = 32`: per-packet array bounds enforced by
//!   `decode_entity_input` (`src/runtime/network-entity-input.js`).
//! - `MAX_ACK_IDS = 32`, `EDGE_CAPACITY = 256.0`: per-message acknowledgement and edge ring capacity
//!   enforced by `handle_edge_ack` and `WeCall` (`src/runtime/network-entity-edge-host.js`).
//! - `INPUT_CODES` (104 entries, `"KeyA"` .. `"WheelDown"`) and `INPUT_ACTIONS` (26 entries,
//!   `"jump"` .. `"squadWait"`): pre-interned in `Context` during `load_from_content` so 60 Hz
//!   `handle_input` never allocates or re-intern UTF-16 strings per tick.
//! - `VIEW_FIELD_NAMES` (7 entries: `"fov"`, `"third"`, `"first"`, `"eye"`, `"center"`, `"offset"`,
//!   `"tilt"`) and `EDGE_KIND_NAMES` (6 entries: `"action"`, `"fire"`, `"team"`, `"spawn"`,
//!   `"enter"`, `"exit"`): pre-interned in `Context` at battle load.
//! - Native client slot reuse (`1..=256`): `add_player` selects the lowest unused `native_id` in
//!   `1..=256` and `remove_player` frees it and drops any per-client `wire_edges` bank (`native_id > 1`)
//!   so join/leave churn across 64-client lobbies and multi-hour soaks never leaks handles or slots.

use crate::udp_peer::{ACK_MAGIC, INPUT_MAGIC};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bfe_content::battle::{Battle, Loader};
use bfe_content::content::Content;
use bfe_content::defs::Defs;
use bfe_content::files::DirFiles;
use bfe_content_native::fingerprint;
use bfe_content_native::setup::battle_clock;
use bfe_sim::abi::context::Context;
use bfe_sim::abi::context_step::{bank, Publication};
use bfe_sim::gen::{
    self, Battle as SimBattle, BlockHeader, ImEntry, ImField, ImOperation, ImResult,
    InputMailboxCall, InputMailboxSetup, PlayerSetup, WeCall, WeEdge, WiView, WirePublicationLink,
    WirePublicationOutput, WirePublishedAck, WirePublishedEdges, WirePublishedFault,
    WirePublishedGroup, WirePublishedOutcome, WirePublishedOwn, WirePublishedRecord,
    WirePublishedReliable, WireSpan, WireWorldFacts, WorldTickInput,
};
use bfe_sim::host::Capture as HostCapture;
use bfe_sim::store::block::{Block, HEADER};
use bfe_sim::store::StrId;
use bfe_sim::wire::publisher::Publisher;
use bfe_sim::world::admit::{
    Call as AdmitCall, Handles as AdmitHandles, Options as AdmitOptions, Returned as AdmitReturned,
};
use bfe_transport::layout::{INPUT_ACTIONS, INPUT_CODES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// Canonical 7 view property names expected by `InputMailbox` (`src/sim/wasm/runtime/input-mailbox.js`
/// and `rust/sim/src/input/owner.rs`).
const VIEW_FIELD_NAMES: [&str; 7] = [
    "mouseScale",
    "invertMouse",
    "firstPerson",
    "aspect",
    "viewTick",
    "flightSettings",
    "gamepad",
];

/// Canonical 6 discrete edge kind names expected by `wireEdgesRun` (`rust/sim/src/wire/edges.rs`).
const EDGE_KIND_NAMES: [&str; 6] = ["action", "fire", "team", "spawn", "suicide", "cancel"];

/// Initial player state returned when `add_player` admits a client (`world.player(m.client)` in
/// `handle("join")` in `src/world-worker.js`).
///
/// Why both `unit_id` and `player_id` are serialized:
/// In `src/game-session.js` (`m.type === "joined"`), the server spreads `...m.player` (`unitId`,
/// `generation`, `team`, `className`, `spectator`, `teamSelect`) and aliases `playerId: m.player.unitId`
/// so both legacy and snapshot-delta client paths read the controlled unit handle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerWelcome {
    /// Authoritative unit entity ID controlled by this player (`1` when spectator or unspawned).
    pub unit_id: u32,
    /// Alias of `unit_id` (`playerId: m.player.unitId` in `src/game-session.js`).
    pub player_id: u32,
    /// Monotonically increasing spawn generation (`0` before first spawn, incremented on each spawn).
    pub generation: u32,
    /// Assigned team index (`1` for Republic/Allies, `2` for CIS/Empire).
    pub team: u8,
    /// Default or currently selected soldier class name (e.g. `"rep_inf_trooper"`).
    pub class_name: String,
    /// Whether the player is currently in free-camera spectator mode.
    pub spectator: bool,
    /// Whether the client should open the team/spawn selection overlay (`!config.autoAssignTeams`).
    pub team_select: bool,
    /// Server simulation tick at the moment `welcome` was generated (`m.tick` in `src/world-worker.js`).
    pub tick: u32,
}

/// Single per-tick movement/look command decoded from a `BNI1` input packet
/// (`commands` entry in `decodeEntityInput` in `src/runtime/network-entity-input.js`).
///
/// Clients bundle up to `nc <= 8` recent unacknowledged commands per `BNI1` packet so a single lost
/// UDP datagram on the `state` lane does not cause a gap in the authoritative input mailbox.
#[derive(Clone, Debug)]
pub struct DecodedInputCommand {
    /// Content/battle epoch this command targets (`u32`).
    pub epoch: u32,
    /// Player spawn generation this command targets (`u32`).
    pub generation: u32,
    /// Client-assigned monotonic input sequence number (`u32`), echoed back in `input-ack`.
    pub sequence: u32,
    /// Client simulation tick when the command was sampled (`u32`).
    pub tick: u32,
    /// Indices into `bfe_transport::layout::INPUT_CODES` for keys/buttons held during this tick.
    pub down_codes: Vec<usize>,
    /// Cumulative mouse/stick look delta `[yaw, pitch]` in radians (`[f64; 2]`).
    pub look_total: [f64; 2],
    /// Camera and controller view parameters referenced by `vi` (`WiView`).
    pub view: WiView,
}

/// Single discrete gameplay edge decoded from a `BNI1` input packet
/// (`edges` entry in `decodeEntityInput` in `src/runtime/network-entity-input.js`).
///
/// Edge `kind` values:
/// - `0`: discrete action (`INPUT_ACTIONS[action_index]`)
/// - `1`: weapon fire (`button`, `pressed`, `view_tick`, `aim_yaw`, `aim_pitch`)
/// - `2`: team selection (`team` in `1..=7`)
/// - `3`: spawn request (`class_id`, `post_id`)
/// - `4`: suicide / respawn request
/// - `5`: spawn cancel
///
/// Clients re-send unacknowledged edges (`ne <= 32`) in every `BNI1` packet until the server
/// confirms receipt via `entity-edge-results` (`received` / `results`) on the reliable `events` lane.
#[derive(Clone, Debug)]
pub struct DecodedEdge {
    /// Client-assigned monotonic edge ID (`> 0`), acknowledged in `entity-edge-results`.
    pub id: u32,
    /// Client tick at which the discrete edge was triggered.
    pub tick: u32,
    /// Battle epoch from the enclosing `BNI1` header.
    pub epoch: u32,
    /// Spawn generation from the enclosing `BNI1` header.
    pub generation: u32,
    /// Discriminant in `0..=5` (`0=action, 1=fire, 2=team, 3=spawn, 4=suicide, 5=cancel`).
    pub kind: u8,
    /// Index into `INPUT_ACTIONS` when `kind == 0`.
    pub action_index: u8,
    /// Fire button index (`0` primary, `1` secondary) when `kind == 1`.
    pub button: u8,
    /// Whether the fire trigger was pressed (`true`) or released (`false`) when `kind == 1`.
    pub pressed: bool,
    /// Interpolated remote snapshot tick rendered on the client when aiming (`kind == 1`).
    pub view_tick: f64,
    /// World-space aim yaw in radians (`kind == 1`).
    pub aim_yaw: f64,
    /// World-space aim pitch in radians (`kind == 1`).
    pub aim_pitch: f64,
    /// Requested team ID in `1..=7` when `kind == 2`.
    pub team: u8,
    /// 1-indexed soldier class ID from `entity-registry` when `kind == 3`.
    pub class_id: u16,
    /// 1-indexed command post ID from `entity-registry` when `kind == 3`.
    pub post_id: u16,
}

/// Complete decoded `BNI1` client entity-input datagram (`decodeEntityInput` in `src/runtime/network-entity-input.js`).
#[derive(Clone, Debug)]
pub struct DecodedInputPacket {
    /// Battle epoch (`u32`).
    pub epoch: u32,
    /// Controlled unit spawn generation (`u32`).
    pub generation: u32,
    /// Up to 8 per-tick movement/look commands (`nc <= 8`).
    pub commands: Vec<DecodedInputCommand>,
    /// Up to 32 discrete gameplay edges (`ne <= 32`).
    pub edges: Vec<DecodedEdge>,
    /// Optional piggybacked `(epoch, tick)` snapshot delta baseline acknowledgement (`na <= 1`).
    pub snapshot_ack: Option<(u16, u32)>,
}

/// Decodes a 10-byte `BNA1` (`0x31414e42`) snapshot acknowledgement keepalive
/// (`export function decodeSnapshotAckKeepalive` in `src/runtime/network-entity-input.js`).
///
/// Wire layout (10 bytes little-endian):
/// - `0..4`: `0x31414e42` (`'BNA1'`)
/// - `4..6`: `epoch` (`u16`)
/// - `6..10`: acknowledged `tick` (`u32`)
///
/// Why `BNA1` exists alongside `BNI1`:
/// Spectators and players on the spawn/team-select screen have no active unit `generation` to send
/// `BNI1` commands for, but still need to advance their `BNS1` delta snapshot baseline tick and keep
/// `GameSessionState.last_seen` fresh (`SESSION_TIMEOUT_MS = 15_000`).
pub fn decode_snapshot_ack_keepalive(bytes: &[u8]) -> Result<(u16, u32), &'static str> {
    if bytes.len() != 10 {
        return Err("invalid snapshot ack keepalive");
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != ACK_MAGIC {
        return Err("wrong snapshot ack protocol");
    }
    let epoch = u16::from_le_bytes([bytes[4], bytes[5]]);
    let tick = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
    Ok((epoch, tick))
}

/// Decodes a `BNI1` (`0x31494e42`) entity input datagram
/// (`export function decodeEntityInput` in `src/runtime/network-entity-input.js`).
///
/// Validates all scalar finiteness, count bounds (`nv <= 8`, `nc <= 8`, `ne <= 32`, `na <= 1`),
/// view flag ranges, `INPUT_CODES` bitmask indices, `INPUT_ACTIONS` indices, fire aim bounds,
/// and rejects any packet with trailing unconsumed bytes (`at != bytes.len()`).
pub fn decode_entity_input(bytes: &[u8]) -> Result<DecodedInputPacket, &'static str> {
    // `BNI1` header is 16 bytes; max packet payload is 1100 bytes (`INPUT_MAX_BYTES`).
    if bytes.len() < 16 || bytes.len() > 1100 {
        return Err("input packet exceeds bounds");
    }
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Result<&[u8], &'static str> {
        if *at + n > bytes.len() {
            return Err("input packet exceeds bounds");
        }
        let s = &bytes[*at..*at + n];
        *at += n;
        Ok(s)
    };
    let u8_at = |at: &mut usize| -> Result<u8, &'static str> { Ok(take(at, 1)?[0]) };
    let u16_at = |at: &mut usize| -> Result<u16, &'static str> {
        let b = take(at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: &mut usize| -> Result<u32, &'static str> {
        let b = take(at, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let f64_at = |at: &mut usize| -> Result<f64, &'static str> {
        let b = take(at, 8)?;
        let v = f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        // Every float on the wire (`mouse_scale`, `aspect`, `view_tick`, `look_x`, `look_y`, `aim_yaw`, `aim_pitch`)
        // must be finite (`Number.isFinite(v)` in `src/runtime/network-entity-input.js`) so `NaN` or `Infinity`
        // can never poison the authoritative simulation state.
        if !v.is_finite() {
            return Err("invalid input scalar");
        }
        Ok(v)
    };

    // Header: `[magic: u32, epoch: u32, generation: u32, nv: u8, nc: u8, ne: u8, na: u8]`.
    if u32_at(&mut at)? != INPUT_MAGIC {
        return Err("wrong input protocol");
    }
    let epoch = u32_at(&mut at)?;
    let generation = u32_at(&mut at)?;
    let nv = u8_at(&mut at)? as usize;
    let nc = u8_at(&mut at)? as usize;
    let ne = u8_at(&mut at)? as usize;
    let na = u8_at(&mut at)? as usize;
    if nv > 8 || nc > 8 || ne > 32 || na > 1 {
        return Err("input count limit");
    }

    // Decode `nv` deduplicated view descriptors (`WiView` in `decodeEntityInput` in `src/runtime/network-entity-input.js`).
    // Flag bits:
    // - `4`: flight mode active (required when `256` or `512` is set)
    // - `16`: 4 gamepad axes (`[-1.0, 1.0]`) + `u16` buttons bitmask present
    // - `32`: `mouse_scale` (`(0.0, 2.0]`) present
    // - `64`: `aspect` (`(0.0, 10.0]`) present
    // - `128`: `view_tick` (`[0.0, 4294967295.0]`) present
    // - `256`: flight `sensitivity` (`(0.0, 10.0]`) present
    // - `512`: flight `curve` (`[0.1, 4.0]`) present
    let mut views = Vec::with_capacity(nv);
    for _ in 0..nv {
        let flags = u16_at(&mut at)?;
        let profile = u16_at(&mut at)?;
        if flags > 1023 || profile != 0 {
            return Err("invalid input view");
        }
        if flags & (256 | 512) != 0 && flags & 4 == 0 {
            return Err("invalid flight view");
        }
        let mouse_scale = if flags & 32 != 0 {
            let v = f64_at(&mut at)?;
            if v <= 0.0 || v > 2.0 {
                return Err("invalid input view range");
            }
            v
        } else {
            0.0
        };
        let aspect = if flags & 64 != 0 {
            let v = f64_at(&mut at)?;
            if v <= 0.0 || v > 10.0 {
                return Err("invalid input view range");
            }
            v
        } else {
            0.0
        };
        let view_tick = if flags & 128 != 0 {
            let v = f64_at(&mut at)?;
            if !(0.0..=4_294_967_295.0).contains(&v) {
                return Err("invalid input view range");
            }
            v
        } else {
            0.0
        };
        let sensitivity = if flags & 256 != 0 {
            let v = f64_at(&mut at)?;
            if v <= 0.0 || v > 10.0 {
                return Err("invalid input view range");
            }
            v
        } else {
            0.0
        };
        let curve = if flags & 512 != 0 {
            let v = f64_at(&mut at)?;
            if !(0.1..=4.0).contains(&v) {
                return Err("invalid input view range");
            }
            v
        } else {
            0.0
        };
        let mut axes = [0.0; 4];
        let mut buttons = 0.0;
        if flags & 16 != 0 {
            for axis in &mut axes {
                let v = f64_at(&mut at)?;
                if v.abs() > 1.0 {
                    return Err("invalid input view range");
                }
                *axis = v;
            }
            buttons = u16_at(&mut at)? as f64;
        }
        views.push(WiView {
            flags: flags as f64,
            profile: profile as f64,
            profile_found: true,
            mouse_scale,
            aspect,
            view_tick,
            sensitivity,
            curve,
            axes,
            buttons,
        });
    }

    // Decode `nc` per-tick input commands referencing `views[vi]`.
    // Each command carries `[sequence: u32, tick: u32, vi: u8, held_bitmask: [u8; 16], look_x: f64, look_y: f64]`.
    let mut commands = Vec::with_capacity(nc);
    for _ in 0..nc {
        let sequence = u32_at(&mut at)?;
        let tick = u32_at(&mut at)?;
        let vi = u8_at(&mut at)?;
        let Some(view) = views.get(vi as usize).cloned() else {
            return Err("invalid view reference");
        };
        let mut down_codes = Vec::new();
        for byte_idx in 0..16 {
            let b = u8_at(&mut at)?;
            for bit in 0..8 {
                if b & (1 << bit) != 0 {
                    let code_idx = byte_idx * 8 + bit;
                    if code_idx >= INPUT_CODES.len() {
                        return Err("unknown held input");
                    }
                    down_codes.push(code_idx);
                }
            }
        }
        let look_x = f64_at(&mut at)?;
        let look_y = f64_at(&mut at)?;
        commands.push(DecodedInputCommand {
            epoch,
            generation,
            sequence,
            tick,
            down_codes,
            look_total: [look_x, look_y],
            view,
        });
    }

    // Decode `ne` discrete gameplay edge events (`kind 0..=5`).
    let mut edges = Vec::with_capacity(ne);
    for _ in 0..ne {
        let id = u32_at(&mut at)?;
        let tick = u32_at(&mut at)?;
        let kind = u8_at(&mut at)?;
        if id == 0 || kind > 5 {
            return Err("invalid edge");
        }
        let mut edge = DecodedEdge {
            id,
            tick,
            epoch,
            generation,
            kind,
            action_index: 0,
            button: 0,
            pressed: false,
            view_tick: 0.0,
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            team: 0,
            class_id: 0,
            post_id: 0,
        };
        match kind {
            // Kind 0: discrete action (`INPUT_ACTIONS[act]`).
            0 => {
                let act = u8_at(&mut at)?;
                if (act as usize) >= INPUT_ACTIONS.len() {
                    return Err("unknown action");
                }
                edge.action_index = act;
            }
            // Kind 1: weapon fire edge with lag-compensated `view_tick` and aim angles (`aim_yaw`, `aim_pitch`).
            1 => {
                let flags = u8_at(&mut at)?;
                if flags > 3 {
                    return Err("invalid fire flags");
                }
                edge.button = flags & 1;
                edge.pressed = (flags & 2) != 0;
                edge.view_tick = f64_at(&mut at)?;
                if !(0.0..=4_294_967_295.0).contains(&edge.view_tick) {
                    return Err("invalid fire aim");
                }
                edge.aim_yaw = f64_at(&mut at)?;
                edge.aim_pitch = f64_at(&mut at)?;
            }
            // Kind 2: team selection (`1..=7`).
            2 => {
                let team = u8_at(&mut at)?;
                if !(1..=7).contains(&team) {
                    return Err("invalid team");
                }
                edge.team = team;
            }
            // Kind 3: spawn selection (`class_id: u16`, `post_id: u16`).
            3 => {
                edge.class_id = u16_at(&mut at)?;
                edge.post_id = u16_at(&mut at)?;
            }
            // Kinds 4 (suicide) and 5 (cancel spawn) carry no extra payload bytes.
            _ => {}
        }
        edges.push(edge);
    }

    // Decode optional piggybacked snapshot acknowledgement (`na == 1`).
    let snapshot_ack = if na == 1 {
        Some((u16_at(&mut at)?, u32_at(&mut at)?))
    } else {
        None
    };
    if at != bytes.len() {
        return Err("trailing input bytes");
    }
    Ok(DecodedInputPacket {
        epoch,
        generation,
        commands,
        edges,
        snapshot_ack,
    })
}

/// Builds the canonical `entity-registry` JSON descriptor from prepared content
/// (`entityRegistryDescriptor` in `src/runtime/network-entity-registry.js` and
/// `ensureEntityHost().descriptor()` in `src/world-worker.js`).
///
/// Sent on the reliable `events` lane immediately before `welcome` when a player joins so the
/// client's snapshot decoder knows the battle's 1-indexed class, weapon, command-post, and combat
/// definition tables.
pub fn build_entity_registry_descriptor(
    root: &Path,
    map: &str,
    query: &str,
    content_epoch: u32,
) -> Result<(Defs, Value), String> {
    let (defs, descriptor, _) = load_entity_registry(root, map, query, content_epoch)?;
    Ok((defs, descriptor))
}

// Retain the registry's objects, not duplicate map geometry, for cold joins.
fn load_entity_registry(
    root: &Path,
    map: &str,
    query: &str,
    content_epoch: u32,
) -> Result<(Defs, Value, bfe_content::value::Value), String> {
    let files = DirFiles::new(root);
    let loader = Loader::new(&files);
    let battle = Battle::load(&loader, map, query)?;
    let content = Content::build(battle)?;
    let defs = Defs::build(&content)?;
    let registry = &defs.registry;
    let table = |names: &[String]| -> Vec<Value> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| json!([i + 1, n]))
            .collect()
    };
    let descriptor = json!({
        "combatDefinitions": registry.combat.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "contentEpoch": content_epoch,
        "classes": table(&registry.classes),
        "weapons": table(&registry.weapons),
        "posts": table(&registry.posts),
        "inputCodes": &INPUT_CODES[..],
        "bindingProfiles": [[0, {}]],
    });
    Ok((defs, descriptor, bfe_content::registry::registry_content(&content)))
}

/// Outbound JSON message addressed to a single peer (`toClients` in `src/world-worker.js`).
#[derive(Clone, Debug)]
pub struct OutgoingPeerEvents {
    pub peer_id: String,
    pub message: Value,
}

/// Shared `BNS1` entity delta snapshot group encoded once per baseline and fanned out to all
/// `clients` sharing that baseline (`out.shared` in `publish()` in `src/world-worker.js`).
#[derive(Clone, Debug)]
pub struct OutgoingSharedGroup {
    pub clients: Vec<String>,
    pub parts: Vec<Vec<u8>>,
    pub captured_at_unix_ms: u64,
}

/// Per-client `BNO1` own-player correction datagram (`out.ownPackets` in `publish()` in `src/world-worker.js`).
#[derive(Clone, Debug)]
pub struct OutgoingOwnPacket {
    pub peer_id: String,
    pub bytes: Vec<u8>,
    pub captured_at_unix_ms: u64,
}

/// Decoded output of one 20 Hz world publication (`publish()` in `src/world-worker.js`).
#[derive(Clone, Debug, Default)]
pub struct DecodedPublication {
    pub epoch: u32,
    pub tick: u32,
    pub captured_at_unix_ms: u64,
    pub shared: Vec<OutgoingSharedGroup>,
    pub own_packets: Vec<OutgoingOwnPacket>,
    pub events: Vec<OutgoingPeerEvents>,
    pub faults: Vec<(Option<String>, String)>,
}

/// Decodes a `Publication` (`WirePublicationOutput` block + byte arena) into transport-ready
/// shared `BNS1` groups, per-client `BNO1` packets, `input-ack`, `entity-edge-results`, and
/// `gameplay-events` (`publish()` in `src/world-worker.js` and `WorldRouter.route` in `src/game-session.js`).
///
/// Layout of `pub_data`:
/// - `pub_data.block.words` begins with a `BlockHeader` (`ROWS == 1`, `STRIDE == WirePublicationOutput::WORDS`),
///   followed by the single `WirePublicationOutput` root row and the variable-length `heap` words.
/// - `pub_data.bytes` holds the packed binary spans (`WireSpan { offset, length }`) for `BNS1`
///   snapshot parts, `BNO1` own-player datagrams, UTF-16LE edge status strings, and binary combat
///   event records.
#[allow(clippy::chunks_exact_to_as_chunks)]
pub fn decode_publication_block(
    pub_data: &Publication,
    client_by_native: &HashMap<u32, String>,
) -> Result<DecodedPublication, &'static str> {
    let words = &pub_data.block.words;
    if words.len() < BlockHeader::WORDS {
        return Err("invalid publication block");
    }
    let rows_count = words[BlockHeader::ROWS] as usize;
    let stride = words[BlockHeader::STRIDE] as usize;
    if rows_count != 1 || stride != WirePublicationOutput::WORDS {
        return Err("invalid publication block header");
    }
    let start = words[BlockHeader::HEAP] as usize;
    let count = words[BlockHeader::HEAP_WORDS] as usize;
    if start < BlockHeader::WORDS + WirePublicationOutput::WORDS || start + count > words.len() {
        return Err("invalid publication heap range");
    }
    let row = &words[BlockHeader::WORDS..BlockHeader::WORDS + WirePublicationOutput::WORDS];
    let heap = &words[start..start + count];
    let byte_len = (row[WirePublicationOutput::BYTE_LENGTH] as usize).min(pub_data.bytes.len());
    let bytes = &pub_data.bytes[..byte_len];

    let epoch = row[WirePublicationOutput::EPOCH] as u32;
    let server_tick = row[WirePublicationOutput::TICK] as u32;
    let captured_at_unix_ms = row[WirePublicationOutput::CAPTURED_AT_UNIX_MS] as u64;
    let dropped_ticks = row[WirePublicationOutput::DROPPED_TICKS] as u32;

    let slice_table = |at: usize, elem_stride: usize| -> Result<&[f64], &'static str> {
        let off = row[at] as usize;
        let len = row[at + 1] as usize;
        let end = off
            .checked_add(len.checked_mul(elem_stride).ok_or("table overflow")?)
            .ok_or("table overflow")?;
        heap.get(off..end).ok_or("table exceeds heap")
    };
    let slice_span = |r: &[f64], at: usize| -> Result<&[u8], &'static str> {
        let off = r[at + WireSpan::OFFSET] as usize;
        let len = r[at + WireSpan::LENGTH] as usize;
        bytes.get(off..off + len).ok_or("span exceeds byte arena")
    };

    let mut out = DecodedPublication {
        epoch,
        tick: server_tick,
        captured_at_unix_ms,
        ..Default::default()
    };

    // Decode `out.faults` (`snapshotFault` in `src/world-worker.js`).
    for fault in slice_table(WirePublicationOutput::FAULTS, WirePublishedFault::WORDS)?
        .chunks_exact(WirePublishedFault::WORDS)
    {
        let cid = fault[WirePublishedFault::CLIENT] as u32;
        let peer = if cid == 0 {
            None
        } else {
            client_by_native.get(&cid).cloned()
        };
        let msg = String::from_utf8_lossy(slice_span(fault, WirePublishedFault::MESSAGE)?)
            .chars()
            .take(240)
            .collect();
        out.faults.push((peer, msg));
    }

    // Decode `out.inputAcks` (`input-ack` in `GameSession.inputAck` in `src/game-session.js`).
    for ack in slice_table(WirePublicationOutput::ACKS, WirePublishedAck::WORDS)?
        .chunks_exact(WirePublishedAck::WORDS)
    {
        let cid = ack[WirePublishedAck::CLIENT] as u32;
        let Some(peer_id) = client_by_native.get(&cid) else {
            continue;
        };
        out.events.push(OutgoingPeerEvents {
            peer_id: peer_id.clone(),
            message: json!({
                "type": "input-ack",
                "epoch": epoch,
                "serverTick": server_tick,
                "generation": ack[WirePublishedAck::GENERATION] as u32,
                "ack": ack[WirePublishedAck::ACK] as u32,
                "ackTick": ack[WirePublishedAck::ACK_TICK] as u32,
                "dropped": dropped_ticks,
                "discardedInput": ack[WirePublishedAck::DISCARDED_INPUT] as u32,
                "discardedActions": ack[WirePublishedAck::DISCARDED_ACTIONS] as u32,
                "ackActions": 0,
            }),
        });
    }

    // Decode `out.edgeResults` (`entity-edge-results` in `GameSession.edgeFeedback` in `src/game-session.js`).
    for edge in slice_table(WirePublicationOutput::EDGES, WirePublishedEdges::WORDS)?
        .chunks_exact(WirePublishedEdges::WORDS)
    {
        let cid = edge[WirePublishedEdges::CLIENT] as u32;
        let Some(peer_id) = client_by_native.get(&cid) else {
            continue;
        };
        let rec_off = edge[WirePublishedEdges::RECEIVED] as usize;
        let rec_len = edge[WirePublishedEdges::RECEIVED + 1] as usize;
        let received_slice = heap
            .get(rec_off..rec_off + rec_len)
            .ok_or("edge received exceeds heap")?;
        let received: Vec<u32> = received_slice.iter().map(|&v| v as u32).collect();

        let res_off = edge[WirePublishedEdges::RESULTS] as usize;
        let res_len = edge[WirePublishedEdges::RESULTS + 1] as usize;
        let res_slice = heap
            .get(res_off..res_off + res_len * WirePublishedOutcome::WORDS)
            .ok_or("edge results exceed heap")?;
        let mut results = Vec::with_capacity(res_len);
        for r in res_slice.chunks_exact(WirePublishedOutcome::WORDS) {
            let status_bytes = slice_span(r, WirePublishedOutcome::STATUS)?;
            let mut units = Vec::with_capacity(status_bytes.len() / 2);
            for pair in status_bytes.chunks_exact(2) {
                units.push(u16::from_le_bytes([pair[0], pair[1]]));
            }
            let status = String::from_utf16_lossy(&units);
            results.push(json!({
                "id": r[WirePublishedOutcome::ID] as u32,
                "status": status,
                "tick": r[WirePublishedOutcome::TICK] as u32,
            }));
        }
        out.events.push(OutgoingPeerEvents {
            peer_id: peer_id.clone(),
            message: json!({
                "type": "entity-edge-results",
                "epoch": edge[WirePublishedEdges::EPOCH] as u32,
                "generation": edge[WirePublishedEdges::GENERATION] as u32,
                "received": received,
                "results": results,
            }),
        });
    }

    // Decode `out.reliableEvents` (`gameplay-events` in `WorldRouter.broadcastEvents` in `src/game-session.js`).
    let records_slice = slice_table(WirePublicationOutput::RECORDS, WirePublishedRecord::WORDS)?;
    let mut encoded_records = Vec::new();
    for rec in records_slice.chunks_exact(WirePublishedRecord::WORDS) {
        let payload = slice_span(rec, WirePublishedRecord::PAYLOAD)?;
        encoded_records.push(json!({
            "kind": "combat",
            "id": rec[WirePublishedRecord::ID] as u32,
            "generation": rec[WirePublishedRecord::GENERATION] as u32,
            "tick": rec[WirePublishedRecord::TICK] as u32,
            "schema": rec[WirePublishedRecord::SCHEMA] as u32,
            "payload": BASE64.encode(payload),
        }));
    }

    for rel in slice_table(
        WirePublicationOutput::RELIABLE,
        WirePublishedReliable::WORDS,
    )?
    .chunks_exact(WirePublishedReliable::WORDS)
    {
        let cid = rel[WirePublishedReliable::CLIENT] as u32;
        let Some(peer_id) = client_by_native.get(&cid) else {
            continue;
        };
        let idx_off = rel[WirePublishedReliable::RECORDS] as usize;
        let idx_len = rel[WirePublishedReliable::RECORDS + 1] as usize;
        let idx_slice = heap
            .get(idx_off..idx_off + idx_len)
            .ok_or("reliable indices exceed heap")?;
        let mut peer_records = Vec::with_capacity(idx_len);
        for &idx in idx_slice {
            if let Some(rec) = encoded_records.get(idx as usize) {
                peer_records.push(rec.clone());
            }
        }
        if !peer_records.is_empty() {
            out.events.push(OutgoingPeerEvents {
                peer_id: peer_id.clone(),
                message: json!({
                    "type": "gameplay-events",
                    "tick": server_tick,
                    "records": peer_records,
                }),
            });
        }
    }

    // Decode `out.shared` baseline groups (`BNS1` datagram parts in `snapshotPublication` in `src/world-worker.js`).
    for group in slice_table(WirePublicationOutput::SHARED, WirePublishedGroup::WORDS)?
        .chunks_exact(WirePublishedGroup::WORDS)
    {
        let cli_off = group[WirePublishedGroup::CLIENTS] as usize;
        let cli_len = group[WirePublishedGroup::CLIENTS + 1] as usize;
        let cli_slice = heap
            .get(cli_off..cli_off + cli_len)
            .ok_or("group clients exceed heap")?;
        let clients: Vec<String> = cli_slice
            .iter()
            .filter_map(|&cid| client_by_native.get(&(cid as u32)).cloned())
            .collect();
        if clients.is_empty() {
            continue;
        }
        let part_off = group[WirePublishedGroup::PARTS] as usize;
        let part_len = group[WirePublishedGroup::PARTS + 1] as usize;
        let part_slice = heap
            .get(part_off..part_off + part_len * WireSpan::WORDS)
            .ok_or("group parts exceed heap")?;
        let mut parts = Vec::with_capacity(part_len);
        for part in part_slice.chunks_exact(WireSpan::WORDS) {
            parts.push(slice_span(part, 0)?.to_vec());
        }
        out.shared.push(OutgoingSharedGroup {
            clients,
            parts,
            captured_at_unix_ms,
        });
    }

    // Decode `out.ownPackets` (`BNO1` per-client own-player datagrams in `snapshotPublication` in `src/world-worker.js`).
    for own in slice_table(WirePublicationOutput::OWN, WirePublishedOwn::WORDS)?
        .chunks_exact(WirePublishedOwn::WORDS)
    {
        let cid = own[WirePublishedOwn::CLIENT] as u32;
        let Some(peer_id) = client_by_native.get(&cid) else {
            continue;
        };
        let packet_bytes = slice_span(own, WirePublishedOwn::SPAN)?.to_vec();
        out.own_packets.push(OutgoingOwnPacket {
            peer_id: peer_id.clone(),
            bytes: packet_bytes,
            captured_at_unix_ms,
        });
    }

    Ok(out)
}

/// Composes the canonical `welcome` event payload (`m.type === "joined"` handler in `GameSession`
/// in `src/game-session.js` and `welcome` callback in `startLocalServer` in `src/local-server.js`):
/// `{ type: "welcome", ...info(), serverId: identity.id, ...m.player, playerId: m.player.unitId, tick: m.tick }`.
pub fn build_welcome_message(info: &Value, server_id: &str, player: &PlayerWelcome) -> Value {
    let mut map = info.as_object().cloned().unwrap_or_default();
    map.insert("type".to_string(), Value::String("welcome".to_string()));
    map.insert("serverId".to_string(), Value::String(server_id.to_string()));
    map.insert("unitId".to_string(), json!(player.unit_id));
    map.insert("playerId".to_string(), json!(player.player_id));
    map.insert("generation".to_string(), json!(player.generation));
    map.insert("team".to_string(), json!(player.team));
    map.insert(
        "className".to_string(),
        Value::String(player.class_name.clone()),
    );
    map.insert("spectator".to_string(), Value::Bool(player.spectator));
    map.insert("teamSelect".to_string(), Value::Bool(player.team_select));
    map.insert("tick".to_string(), json!(player.tick));
    map.insert(
        "snapshotMode".to_string(),
        Value::String("snapshot-delta-v1".to_string()),
    );
    Value::Object(map)
}

/// Looks up a generated simulation operation ID by name in `bfe_sim::gen::OPERATION_NAMES`.
fn find_operation(name: &str) -> Result<u32, String> {
    gen::OPERATION_NAMES
        .iter()
        .position(|&candidate| candidate == name)
        .filter(|&id| id != 0)
        .map(|id| id as u32)
        .ok_or_else(|| format!("missing operation {name} in generated table"))
}

/// Parses a battle query string (`?map=nab2&side=rep&ai=classic&seed=1`) into `bfe_sim::world::admit::Options`
/// for `Context::world_admit`.
pub fn parse_admit_options(query: &str, fallback_map: &str) -> AdmitOptions {
    let mut map = fallback_map.to_string();
    let mut side = "rep".to_string();
    let mut enhanced = false;
    let mut seed = 1u32;
    let mut heroes = false;
    let mut unit = None;
    for pair in query.trim_start_matches('?').split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "map" if !v.is_empty() => map = v.to_string(),
                "side" if !v.is_empty() => side = v.to_string(),
                "ai" => enhanced = v == "enhanced",
                "seed" => {
                    if let Ok(parsed) = v.parse::<u32>() {
                        seed = parsed;
                    }
                }
                "heroes" => heroes = v == "1" || v == "true",
                "unit" if !v.is_empty() => unit = Some(v.to_string()),
                _ => {}
            }
        }
    }
    AdmitOptions {
        map,
        side,
        enhanced,
        seed,
        heroes,
        unit,
    }
}

/// Native simulation world host wrapping `bfe_sim::abi::context::Context` (`src/world-worker.js`).
///
/// Calls `Context::world_admit` during `load_from_content`:
/// - When `Context::world_admit` succeeds (`admit::provided() == true`), `world_admitted` is `true`,
///   `world_step_init(admitted.setup)` is initialized, and `step()` drives `world_tick` +
///   `world_publish_into` on the admitted world.
/// - While `Context::world_admit` returns `ErrorKind::Unavailable` (before the `admit` lane flips
///   `admit::provided()` to `true`), `world_admitted` is `false` so `/healthz` reports
///   `multiplayerReady: false`.
pub struct NativeWorldHost {
    content_root: std::path::PathBuf,
    registry_content: bfe_content::value::Value,
    match_seed: u32,
    pub links: Vec<WirePublicationLink>,
    /// Underlying native simulation context (`bfe_sim::abi::context::Context`).
    pub context: Context,
    /// Prepared content definitions (`Defs` from `bfe_content`).
    pub defs: Defs,
    /// Canonical `entity-registry` JSON payload sent to joining clients before `welcome`.
    pub registry_descriptor: Value,
    /// `bfe-content-sha256-...` digest for the active mission query.
    pub fingerprint: String,
    /// Active mission query string (`?map=nab2&side=rep&ai=classic&seed=1&match=...`).
    pub query: String,
    /// Active map identifier (e.g. `"nab2"`).
    pub map: String,
    /// Monotonic battle epoch (`1` on initial load, incremented on map rotation).
    pub epoch: u32,
    /// Current 60 Hz simulation tick (`0` before first `step()`).
    pub tick: u32,
    /// Cumulative late-tick drop count (`droppedTicks` in `src/world-worker.js`).
    pub dropped_ticks: u32,
    /// Most recent monotonic timestamp (`now_ms`) observed on the tick thread.
    pub last_now_ms: f64,
    /// Whether the active battle has reached victory/defeat (`battle_mirror.finished`).
    pub ended: bool,
    /// Winning team (`1`, `2`, or `0` for draw) once `ended` is `true`.
    pub winner: Option<u8>,
    /// `true` if and only if `Context::world_admit` succeeded on this world (`multiplayerReady`).
    pub world_admitted: bool,
    /// Reusable per-tick input struct passed to `context.world_tick`.
    pub tick_input: WorldTickInput,
    /// Handles returned by `Context::world_admit` (`mailbox`, `player`, `publication`, `host`, `edges`, `client`).
    pub admitted_handles: AdmitHandles,
    /// Authoritative unit slot ID assigned to the admitted player (`admitted.player_id as u32`, e.g. `65` on `nab2`).
    pub admitted_player_id: u32,
    /// Native publication session handle returned by `admitted.handles.publication` (`0` when unadmitted).
    pub publication_session: u32,
    /// Native host handle returned by `admitted.handles.host` (`0` when unadmitted).
    pub host_handle: u32,
    /// Pre-allocated `Publication` buffer reused on every `world_publish_into` call.
    pub published: Publication,
    /// Pre-allocated operation block buffer for `wireHostOp` / `wireEdgesRun` / `inputMailboxOwnedRun`.
    pub op_block: Block,
    /// Pre-allocated argument words buffer for `run_operation_into`.
    pub op_words: Vec<f64>,
    /// Pre-allocated heap words buffer for `run_operation_into`.
    pub op_heap: Vec<f64>,
    /// Pre-allocated buffer for `context.read_bank(bank::F4_BATTLE, ...)`.
    pub bank_words: Vec<f64>,
    /// Pre-allocated buffer for `WireWorldFacts` passed to `world_publish_into`.
    pub facts_words: Vec<f64>,
    /// Exported `SimBattle` mirror inspected for `finished` / `loser` state.
    pub battle_mirror: SimBattle,
    /// Maps native simulation client slot ID (`u32`) to 64-hex player identity (`String`).
    pub client_by_native: HashMap<u32, String>,
    /// Maps 64-hex player identity (`String`) to native simulation client slot ID (`u32`).
    pub native_by_client: HashMap<String, u32>,
    /// Maps native simulation client slot ID (`u32`) to its `wire_edges` bank handle (`u32`).
    pub edges_by_native: HashMap<u32, u32>,
    /// Maps native simulation client slot ID (`u32`) to its `players` handle (`u32`).
    pub player_by_native: HashMap<u32, u32>,
    /// Maps native simulation client slot ID (`u32`) to its `input_mailbox` handle (`u32`).
    pub mailbox_by_native: HashMap<u32, u32>,
    /// Active player welcome records keyed by 64-hex player identity.
    pub player_welcomes: HashMap<String, PlayerWelcome>,
    /// Operation ID for `wireHostOp`.
    pub op_host_op: u32,
    /// Operation ID for `wireEdgesRun`.
    pub op_edges_run: u32,
    /// Operation ID for `inputMailboxOwnedRun`.
    pub op_mailbox_run: u32,
    /// Pre-interned `StrId` table for `INPUT_CODES`.
    pub code_str_ids: Vec<StrId>,
    /// Pre-interned `StrId` table for `INPUT_ACTIONS`.
    pub action_str_ids: Vec<StrId>,
    /// Pre-interned `StrId` table for the 7 `VIEW_FIELD_NAMES`.
    pub view_field_str_ids: [StrId; 7],
    /// Pre-interned `StrId` table for the 6 `EDGE_KIND_NAMES`.
    pub edge_kind_str_ids: [StrId; 6],
    /// Pre-interned `StrId` for `"firePrimary"`.
    pub fire_primary_str_id: StrId,
    /// Pre-interned `StrId` for `"fireSecondary"`.
    pub fire_secondary_str_id: StrId,
    /// Queued synthetic/recorded `Publication` frames for deterministic transport/wire tests.
    pub recorded_publications: Vec<Publication>,
    /// Read cursor into `recorded_publications`.
    pub recorded_cursor: usize,
}

impl NativeWorldHost {
    /// Loads prepared content from `root` for `query` and initializes `Context` (`src/world-worker.js`).
    ///
    /// Invokes `Context::world_admit(&files, &admit_options)`:
    /// - If `world_admit` succeeds, initializes `world_step_init(admitted.setup)` and marks
    ///   `world_admitted = true`.
    /// - If `world_admit` returns `ErrorKind::Unavailable`, marks `world_admitted = false` so
    ///   `/healthz` never reports `multiplayerReady: true` for an unadmitted world.
    pub fn load_from_content(root: &Path, query: &str, epoch: u32) -> Result<Self, String> {
        let map = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix("map="))
            .unwrap_or("nab2")
            .to_string();
        let files = DirFiles::new(root);
        let fp = fingerprint::hash(&files, query)?;
        let (defs, mut registry_descriptor, registry_content) =
            load_entity_registry(root, &map, query, epoch)?;

        let mut context = Context::new();
        let admit_options = parse_admit_options(query, &map);
        let mut world_admitted = false;
        let mut admitted_handles = AdmitHandles::default();
        let mut admitted_player_id = 1u32;
        let mut publication_session = 0u32;
        let mut host_handle = 0u32;
        let mut initial_units = 0u32;
        let edges_by_native = HashMap::new();
        let player_by_native = HashMap::new();
        let mailbox_by_native = HashMap::new();

        // Attempt cold world admission via `Context::world_admit`.
        // When `world_admit` cannot complete cold admission (either `SimErrorKind::Unavailable` or
        // an incomplete cold stage / synthetic test fixture without recording receipts), dispose the
        // partially populated `Context`, initialize a clean `Context` with `players_init()`, and
        // keep `world_admitted = false` so `/healthz` reports `multiplayerReady: false`.
        match context.world_network_admit(&files, &admit_options) {
            Ok(admitted) => {
                context
                    .world_step_init(admitted.setup)
                    .map_err(|e| e.to_string())?;
                admitted_handles = admitted.handles;
                admitted_player_id = (admitted.player_id as u32).max(1);
                publication_session = admitted.handles.publication;
                host_handle = admitted.handles.host;
                context
                    .world_network_epoch(host_handle, epoch)
                    .map_err(|e| e.to_string())?;
                initial_units = admitted.units;
                if !admitted.registry.is_null() {
                    registry_descriptor = admitted.registry;
                }
                // Start the authority host (`Context::world_host_start`) with the wire capture
                // handles created by admission (`combat`, `presentation`, `store_source`).
                context.world_host_random(admit_options.seed, None).map_err(|e| e.to_string())?;
                let (explosions, host_content) = bfe_sim::host::content_tables_from_registry(
                    &context, admitted_handles.store_source, &registry_content, &defs.registry.combat,
                )?;
                let capture = HostCapture {
                    explosions,
                    combat: admitted_handles.combat,
                    presentation: admitted_handles.presentation,
                    store_source: admitted_handles.store_source,
                };
                context
                    .world_host_start(Some(capture), host_content)
                    .map_err(|e| e.to_string())?;
                // Enable outcome retention on the admitted player's reliable action and fire lanes
                // (`players_outcomes_enable`) so `consumed_edges` can report edge completion back
                // to the client via `entity-edge-results`.
                let _ = context.players_outcomes_enable(admitted_handles.player);
                // Configure `wire_host` to publish world snapshots every 3 ticks (20 Hz), matching
                // `PUBLISH_EVERY_TICKS = 3` in `src/world-worker.js` and `MAX_LOCAL_INTERVAL = 3`
                // in `src/runtime/snapshot-host.js`.
                let _ = context.invoke_export(
                    "wire_host_set",
                    &AdmitCall {
                        args: vec![host_handle as f64, 0.0, 0.0, 3.0],
                        ..Default::default()
                    },
                );

                world_admitted = true;
            }
            // A refused admission leaves the service up and not ready. Discard
            // partial native owners before admitting any synthetic connections.
            Err(_err) => {
                let _ = context.dispose();
                context = Context::new();
                let _ = context.players_init();
            }
        }

        // Pre-intern `INPUT_CODES`, `INPUT_ACTIONS`, `VIEW_FIELD_NAMES`, and `EDGE_KIND_NAMES`
        // UTF-16 string IDs in `context` once at battle load so the 60 Hz input mailbox and edge
        // paths never allocate or re-intern strings per tick.
        let intern_str = |s: &str| -> Result<StrId, String> {
            let units: Vec<u16> = s.encode_utf16().collect();
            context.intern(&units).map_err(|e| e.to_string())
        };
        let mut code_str_ids = Vec::with_capacity(INPUT_CODES.len());
        for code in &INPUT_CODES {
            code_str_ids.push(intern_str(code)?);
        }
        let mut action_str_ids = Vec::with_capacity(INPUT_ACTIONS.len());
        for action in &INPUT_ACTIONS {
            action_str_ids.push(intern_str(action)?);
        }
        let mut view_field_str_ids = [StrId(0); 7];
        for (slot, name) in view_field_str_ids.iter_mut().zip(VIEW_FIELD_NAMES) {
            *slot = intern_str(name)?;
        }
        let mut edge_kind_str_ids = [StrId(0); 6];
        for (slot, name) in edge_kind_str_ids.iter_mut().zip(EDGE_KIND_NAMES) {
            *slot = intern_str(name)?;
        }
        let fire_primary_str_id = intern_str("firePrimary")?;
        let fire_secondary_str_id = intern_str("fireSecondary")?;

        // Pre-allocate reusable `Publication`, `op_block`, `op_words`, `op_heap`, `bank_words`,
        // and `facts_words` buffers so `step()` and `world_publish_into` run with zero heap growth.
        let policy = Publisher::default();
        let published = Publication {
            block: Block {
                words: Vec::with_capacity(policy.block.words.capacity()),
                heap: Vec::new(),
            },
            bytes: Vec::with_capacity(policy.bytes.capacity()),
        };
        let mut op_block = Block::default();
        op_block.words.reserve(4096);
        op_block.heap.reserve(4096);

        let op_host_op = find_operation("wireHostOp").unwrap_or(0);
        let op_edges_run = find_operation("wireEdgesRun").unwrap_or(0);
        let op_mailbox_run = find_operation("inputMailboxOwnedRun").unwrap_or(0);

        let mut host = Self {
            context,
            defs,
            registry_descriptor,
            fingerprint: fp,
            query: query.to_string(),
            map,
            epoch,
            tick: 0,
            dropped_ticks: 0,
            last_now_ms: 0.0,
            ended: false,
            winner: None,
            content_root: root.to_path_buf(),
            registry_content,
            match_seed: admit_options.seed,
            links: Vec::with_capacity(256),
            world_admitted,
            tick_input: WorldTickInput {
                dt: 1.0 / 60.0,
                tick: 1.0,
                clock: battle_clock(),
                units: initial_units,
                // game.js omits AI advancement when AI_MODE is off. Keep the
                // server's advertised bot setting and authoritative tick aligned.
                ai: !query
                    .trim_start_matches('?')
                    .split('&')
                    .any(|part| part == "ai=off"),
                live: true,
                camera: 0,
                offline_camera: true,
                squads: 0,
                at: 0.0,
                mouse_scale: 1.0,
            },
            admitted_handles,
            admitted_player_id,
            publication_session,
            host_handle,
            published,
            op_block,
            op_words: Vec::with_capacity(1024),
            op_heap: Vec::with_capacity(1024),
            bank_words: Vec::with_capacity(1024),
            facts_words: Vec::with_capacity(
                WireWorldFacts::WORDS + 64 * WirePublicationLink::WORDS,
            ),
            battle_mirror: SimBattle::default(),
            client_by_native: HashMap::new(),
            native_by_client: HashMap::new(),
            edges_by_native,
            player_by_native,
            mailbox_by_native,
            player_welcomes: HashMap::new(),
            op_host_op,
            op_edges_run,
            op_mailbox_run,
            code_str_ids,
            action_str_ids,
            view_field_str_ids,
            edge_kind_str_ids,
            fire_primary_str_id,
            fire_secondary_str_id,
            recorded_publications: Vec::new(),
            recorded_cursor: 0,
        };

        // Initialize the admitted edge host to `(epoch, generation = 1)` (`EntityEdges.reset` in
        // `src/runtime/network-entity-edge-host.js`) so edges for generation 1 are accepted immediately.
        if host.world_admitted && host.admitted_handles.edges != 0 {
            host.reset_edges_host(host.admitted_handles.edges, epoch as f64, 1.0);
        }

        Ok(host)
    }

    /// Runs a generated record operation (`run_operation_into`) using pre-allocated `op_words`,
    /// `op_heap`, and `op_block` buffers (`Arguments::record` + `Arguments::run` in
    /// `rust/bfe-client/src/local/arguments.rs`).
    fn run_record_op(
        &mut self,
        op: u32,
        scalars: &[f64],
        record_words: usize,
        put: impl FnOnce(&mut [f64], &mut Vec<f64>),
    ) -> Result<(), String> {
        if op == 0 {
            return Err("operation not registered".to_string());
        }
        self.op_words.clear();
        self.op_words.extend_from_slice(scalars);
        self.op_words.resize(scalars.len() + record_words, 0.0);
        self.op_heap.clear();
        put(&mut self.op_words[scalars.len()..], &mut self.op_heap);
        self.op_words.extend_from_slice(&self.op_heap);
        self.context
            .run_operation_into(op, &self.op_words, &mut self.op_block)
            .map_err(|e| format!("{e:?}"))
    }

    /// Resets a `wire_edges` bank (`op = 0` in `rust/sim/src/wire/edges.rs`) to `(epoch, generation)`.
    fn reset_edges_host(&mut self, edges_handle: u32, epoch: f64, generation: f64) {
        if edges_handle == 0 || self.op_edges_run == 0 {
            return;
        }
        let call = WeCall {
            op: 0.0,
            epoch,
            generation,
            capacity: 256.0,
            ..Default::default()
        };
        let _ = self.run_record_op(
            self.op_edges_run,
            &[edges_handle as f64],
            WeCall::WORDS,
            |row, heap| call.put(row, heap),
        );
    }

    /// Invokes `wire_host_set(host_handle, client_id, field, value)` via `Context::invoke_export`.
    fn set_host_client_field(&self, client_id: u32, field: u32, value: f64) {
        if !self.world_admitted || self.host_handle == 0 {
            return;
        }
        let _ = self.context.invoke_export(
            "wire_host_set",
            &AdmitCall {
                args: vec![
                    self.host_handle as f64,
                    client_id as f64,
                    field as f64,
                    value,
                ],
                ..Default::default()
            },
        );
    }

    /// Creates a new `input_mailbox` handle (`input_mailbox_create`) initialized with `self.epoch`,
    /// capacity `256.0`, and the pre-interned `self.code_str_ids` table (`session::create_mailbox`
    /// in `rust/sim/src/world/admit/session.rs`).
    fn create_input_mailbox(&self) -> Option<u32> {
        let setup = InputMailboxSetup {
            epoch: self.epoch as f64,
            capacity: 256.0,
            codes: self.code_str_ids.clone(),
        };
        let mut call = AdmitCall::default();
        call.row.resize(InputMailboxSetup::WORDS, 0.0);
        setup.put(&mut call.row, &mut call.heap);
        match self.context.invoke_export("input_mailbox_create", &call) {
            Ok(AdmitReturned::Scalar(v)) if v as u32 != 0 => Some(v as u32),
            _ => None,
        }
    }

    /// Invokes `wire_host_op(host_handle, op, client_id, a, b)` via `run_operation_into`.
    fn run_host_op(&mut self, op: u32, client_id: u32, a: f64, b: f64) {
        if !self.world_admitted || self.host_handle == 0 || self.op_host_op == 0 {
            return;
        }
        self.op_words.clear();
        self.op_words.extend_from_slice(&[
            self.host_handle as f64,
            op as f64,
            client_id as f64,
            a,
            b,
        ]);
        let _ =
            self.context
                .run_operation_into(self.op_host_op, &self.op_words, &mut self.op_block);
    }

    /// Restart a completed, empty battle so a later visitor can join a live match.
    /// No connected player's publication baseline may cross this replacement.
    pub fn restart_finished_empty(&mut self) -> Result<bool, String> {
        if !self.ended || !self.native_by_client.is_empty() {
            return Ok(false);
        }
        let mut next = Self::load_from_content(
            &self.content_root,
            &self.query,
            self.epoch.checked_add(1).ok_or("battle epoch exhausted")?,
        )?;
        next.last_now_ms = self.last_now_ms;
        *self = next;
        Ok(true)
    }

    /// Chat routing reads the player's current authority team, including a
    /// switch while bodiless. The join welcome is only an initial assignment.
    pub fn player_team(&self, peer_id: &str) -> Option<f64> {
        let native = self.native_by_client.get(peer_id)?;
        let player = self.player_by_native.get(native)?;
        self.context.world_network_team(*player).ok().flatten()
    }

    /// Admits a player into the world roster (`world.addPlayer` and `entityHost.addClient` in
    /// `handle("join")` in `src/world-worker.js`).
    pub fn add_player(
        &mut self,
        peer_id: &str,
        name: &str,
        spectator: bool,
        auto_spawn: bool,
        team_select: bool,
    ) -> Result<PlayerWelcome, String> {
        if self.native_by_client.contains_key(peer_id) {
            return Err("player already joined".into());
        }
        let default_class = self
            .defs
            .classes
            .first()
            .map(|c| self.defs.names.text(c.name).to_string())
            .or_else(|| self.defs.registry.classes.first().cloned())
            .ok_or_else(|| "no player classes in content".to_string())?;

        // Reuse the existing slot if this peer_id is already registered, otherwise pick the lowest
        // unused slot in `1..=256` so join/leave churn in long-running or 64-client sessions never
        // collides on `native_id`.
        let native_id = if let Some(&existing) = self.native_by_client.get(peer_id) {
            existing
        } else {
            (1u32..=256)
                .find(|id| !self.client_by_native.contains_key(id))
                .ok_or_else(|| "server full".to_string())?
        };
        let mut team = if native_id % 2 == 1 { 1 } else { 2 };
        let mut unit_id = native_id;
        let mut generation = if spectator || !auto_spawn { 0 } else { 1 };
        let mut default_class = default_class;
        if self.world_admitted && self.host_handle != 0 && self.publication_session != 0 {
            let mailbox_id = self.create_input_mailbox().ok_or("input mailbox refused")?;
            // addPlayer refuses admission without borrowing another client's mailbox.
            let player = match self.context.world_network_join(
                &DirFiles::new(&self.content_root),
                native_id,
                peer_id,
                name,
                spectator,
                auto_spawn,
                team_select,
                mailbox_id,
                self.last_now_ms,
            ) {
                Ok(player) => player,
                Err(error) => {
                    let _ = self.context.invoke_export(
                        "input_mailbox_drop",
                        &AdmitCall {
                            args: vec![mailbox_id as f64],
                            ..Default::default()
                        },
                    );
                    return Err(error.to_string());
                }
            };
            // bindPlayer's lazy network weapon stream uses the match seed.
            self.context.world_host_random(self.match_seed, (!spectator).then_some(player.unit_id))
                .map_err(|e| e.to_string())?;
            let (explosions, content) = bfe_sim::host::content_tables_from_registry(
                &self.context, self.admitted_handles.store_source,
                &self.registry_content, &self.defs.registry.combat,
            )?;
            self.context.world_host_content(explosions, content).map_err(|e| e.to_string())?;
            let edges_id = match self
                .context
                .invoke_export("wire_edges_create", &AdmitCall::default())
                .map_err(|e| e.to_string())?
            {
                AdmitReturned::Scalar(v) if v != 0. => v as u32,
                _ => return Err("entity edges refused".into()),
            };
            team = player.team as u8;
            unit_id = player.unit_id;
            generation = player.generation as u32;
            default_class = player.class_name;
            self.mailbox_by_native.insert(native_id, mailbox_id);
            self.player_by_native.insert(native_id, player.player);
            self.edges_by_native.insert(native_id, edges_id);
            self.reset_edges_host(edges_id, self.epoch as f64, generation as f64);
            self.run_host_op(0, native_id, self.last_now_ms, 0.0);
            self.context
                .invoke_export(
                    "wire_publish_bind",
                    &AdmitCall {
                        args: vec![
                            self.publication_session as f64,
                            native_id as f64,
                            native_id as f64,
                            edges_id as f64,
                        ],
                        ..Default::default()
                    },
                )
                .map_err(|e| e.to_string())?;
            self.set_host_client_field(native_id, 2, 1.0);
            self.set_host_client_field(native_id, 3, 1.0);
            self.set_host_client_field(native_id, 6, self.last_now_ms);
            self.context
                .world_host_client(player.player, edges_id)
                .map_err(|e| e.to_string())?;
        } else {
            // Even when cold world_admit is not active (e.g. unit/transport harness tests),
            // allocate a player + edges pair and register with `Context::world_host_client`
            // so `world_host_client` and `world_host_client_leave` are exercised on every join/leave.
            let mailbox_id = self.create_input_mailbox().unwrap_or(0);
            let player_setup = PlayerSetup {
                mailbox: mailbox_id,
                character: None,
                spectator,
                reliable_fire: true,
                separate_actions: true,
                team_select,
                auto_respawn: false,
                ordinal: native_id as f64,
                team: team as f64,
                recon_key: None,
                at: self.last_now_ms,
            };
            let player_id = self.context.players_create(&player_setup).unwrap_or(0);
            if player_id != 0 {
                let _ = self.context.players_outcomes_enable(player_id);
            }
            let edges_id = match self
                .context
                .invoke_export("wire_edges_create", &AdmitCall::default())
            {
                Ok(AdmitReturned::Scalar(v)) if v as u32 != 0 => v as u32,
                _ => 0,
            };
            if mailbox_id != 0 {
                self.mailbox_by_native.insert(native_id, mailbox_id);
            }
            if player_id != 0 {
                self.player_by_native.insert(native_id, player_id);
            }
            if edges_id != 0 {
                self.edges_by_native.insert(native_id, edges_id);
                self.reset_edges_host(edges_id, self.epoch as f64, generation as f64);
            }
            if player_id != 0 && edges_id != 0 {
                let _ = self.context.world_host_client(player_id, edges_id);
            }
        }

        self.client_by_native.insert(native_id, peer_id.to_string());
        self.native_by_client.insert(peer_id.to_string(), native_id);
        let welcome = PlayerWelcome {
            unit_id,
            player_id: unit_id,
            generation,
            team,
            class_name: default_class,
            spectator,
            team_select,
            tick: self.tick,
        };
        self.player_welcomes
            .insert(peer_id.to_string(), welcome.clone());
        Ok(welcome)
    }

    /// Removes a disconnected player from the world roster (`leave(client)` in `src/world-worker.js`).
    pub fn remove_player(&mut self, peer_id: &str) {
        if let Some(native_id) = self.native_by_client.remove(peer_id) {
            self.client_by_native.remove(&native_id);
            if self.world_admitted && self.host_handle != 0 {
                self.run_host_op(1, native_id, 0.0, 0.0);
                if let Some(edges) = self.edges_by_native.remove(&native_id) {
                    let _ = self.context.world_host_client_leave(edges);
                    let _ = self.context.invoke_export(
                        "wire_edges_drop",
                        &AdmitCall {
                            args: vec![edges as f64],
                            ..Default::default()
                        },
                    );
                }
                if let Some(player) = self.player_by_native.remove(&native_id) {
                    let _ = self.context.world_network_leave(native_id, player);
                }
                if let Some(mailbox) = self.mailbox_by_native.remove(&native_id) {
                    let _ = self.context.invoke_export(
                        "input_mailbox_drop",
                        &AdmitCall {
                            args: vec![mailbox as f64],
                            ..Default::default()
                        },
                    );
                }
            } else {
                if let Some(edges_id) = self.edges_by_native.remove(&native_id) {
                    let _ = self.context.world_host_client_leave(edges_id);
                    if edges_id != 0 {
                        let _ = self.context.invoke_export(
                            "wire_edges_drop",
                            &AdmitCall {
                                args: vec![edges_id as f64],
                                ..Default::default()
                            },
                        );
                    }
                }
                if let Some(player_id) = self.player_by_native.remove(&native_id) {
                    if player_id != 0 {
                        let _ = self.context.players_drop(player_id);
                    }
                }
                if let Some(mailbox_id) = self.mailbox_by_native.remove(&native_id) {
                    if mailbox_id != 0 {
                        let _ = self.context.invoke_export(
                            "input_mailbox_drop",
                            &AdmitCall {
                                args: vec![mailbox_id as f64],
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
        self.player_welcomes.remove(peer_id);
    }

    /// Marks a joined client as started and ready for snapshot streaming (`handle("start")` in
    /// `src/world-worker.js` and `setStarted` / `setReady` in `src/runtime/snapshot-host.js`).
    pub fn handle_start(&mut self, peer_id: &str, now_ms: f64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(&native_id) = self.native_by_client.get(peer_id) else {
            return;
        };
        self.set_host_client_field(native_id, 2, 1.0);
        self.set_host_client_field(native_id, 3, 1.0);
        self.set_host_client_field(native_id, 6, self.last_now_ms);
        self.run_host_op(9, native_id, 0.0, 0.0);
    }

    /// Resets a client's snapshot delta baseline to force a full keyframe on the next publication
    /// (`handle("resync")` in `src/world-worker.js` and `Host::resync` in `rust/sim/src/wire/host.rs`).
    pub fn handle_resync(&mut self, peer_id: &str, now_ms: f64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(&native_id) = self.native_by_client.get(peer_id) else {
            return;
        };
        self.set_host_client_field(native_id, 6, self.last_now_ms);
        self.run_host_op(9, native_id, 0.0, 0.0);
    }

    /// Acknowledges delivered edge outcome IDs from a client `entity-edge-ack` message on the
    /// reliable `events` lane (`edgeAck(id, message)` in `src/runtime/snapshot-host.js` and
    /// `Host::acknowledge` in `rust/sim/src/wire/edges.rs`).
    pub fn handle_edge_ack(&mut self, peer_id: &str, message: &Value, now_ms: f64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(&native_id) = self.native_by_client.get(peer_id) else {
            return;
        };
        if !self.world_admitted {
            return;
        }
        self.set_host_client_field(native_id, 6, self.last_now_ms);
        let edges_id = self
            .edges_by_native
            .get(&native_id)
            .copied()
            .unwrap_or(self.admitted_handles.edges);
        if edges_id == 0 {
            return;
        }
        let epoch = message
            .get("epoch")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.epoch as f64);
        let generation = message
            .get("generation")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let ids: Vec<f64> = message
            .get("ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_f64()).take(32).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return;
        }
        let call = WeCall {
            op: 7.0,
            epoch,
            generation,
            ids,
            capacity: 256.0,
            ..Default::default()
        };
        let _ = self.run_record_op(
            self.op_edges_run,
            &[edges_id as f64],
            WeCall::WORDS,
            |row, heap| call.put(row, heap),
        );
    }

    /// Decodes and admits an inbound `BNA1` snapshot-ack keepalive or `BNI1` entity-input datagram
    /// from `peer_id` (`input(id, bytes)` in `src/runtime/snapshot-host.js`).
    ///
    /// Why malformed datagrams never disconnect or stall other players:
    /// All wire validation occurs in `decode_snapshot_ack_keepalive` / `decode_entity_input` before
    /// touching `self.context`, and any per-command refusal is isolated to that command entry.
    pub fn handle_input(&mut self, peer_id: &str, bytes: &[u8], now_ms: f64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(&native_id) = self.native_by_client.get(peer_id) else {
            return;
        };
        if !self.world_admitted {
            return;
        }
        self.set_host_client_field(native_id, 6, self.last_now_ms);

        // 10-byte `BNA1` snapshot acknowledgement keepalive (`isSnapshotAckKeepalive` in `snapshot-host.js`).
        if bytes.len() == 10 {
            if let Ok((ack_epoch, ack_tick)) = decode_snapshot_ack_keepalive(bytes) {
                self.run_host_op(4, native_id, ack_tick as f64, ack_epoch as f64);
                return;
            }
        }

        // `BNI1` entity input packet (`decodeEntityInput` in `snapshot-host.js`).
        let Ok(pkt) = decode_entity_input(bytes) else {
            return;
        };
        if let Some((ack_epoch, ack_tick)) = pkt.snapshot_ack {
            self.run_host_op(4, native_id, ack_tick as f64, ack_epoch as f64);
        }
        if pkt.epoch != self.epoch {
            return;
        }

        // Forward discrete gameplay edges (`pkt.edges`) into the client's `wire_edges` receiver
        // (`state.edges.receive(packet, game.worldClock.tick)` in `snapshot-host.js`).
        if !pkt.edges.is_empty() {
            let edges_id = self
                .edges_by_native
                .get(&native_id)
                .copied()
                .unwrap_or(self.admitted_handles.edges);
            if edges_id != 0 {
                let mut we_edges = Vec::with_capacity(pkt.edges.len());
                for edge in &pkt.edges {
                    let kind_sid = self
                        .edge_kind_str_ids
                        .get(edge.kind as usize)
                        .copied()
                        .unwrap_or(self.edge_kind_str_ids[0]);
                    let action_sid = if edge.kind == 0 {
                        self.action_str_ids.get(edge.action_index as usize).copied()
                    } else {
                        None
                    };
                    let button_sid = if edge.kind == 1 {
                        Some(if edge.button == 1 {
                            self.fire_secondary_str_id
                        } else {
                            self.fire_primary_str_id
                        })
                    } else {
                        None
                    };
                    we_edges.push(WeEdge {
                        id: edge.id as f64,
                        tick: edge.tick as f64,
                        epoch: edge.epoch as f64,
                        generation: edge.generation as f64,
                        kind: kind_sid,
                        action: action_sid,
                        button: button_sid,
                        pressed: (edge.kind == 1).then_some(edge.pressed),
                        view_tick: (edge.kind == 1).then_some(edge.view_tick),
                        aim_yaw: (edge.kind == 1).then_some(edge.aim_yaw),
                        aim_pitch: (edge.kind == 1).then_some(edge.aim_pitch),
                        team: (edge.kind == 2).then_some(edge.team as f64),
                        class_id: (edge.kind == 3).then_some(edge.class_id as f64),
                        post_id: (edge.kind == 3).then_some(edge.post_id as f64),
                    });
                }
                let call = WeCall {
                    op: 1.0,
                    epoch: pkt.epoch as f64,
                    generation: pkt.generation as f64,
                    tick: self.tick as f64,
                    edges: we_edges,
                    capacity: 256.0,
                    ..Default::default()
                };
                let _ = self.run_record_op(
                    self.op_edges_run,
                    &[edges_id as f64],
                    WeCall::WORDS,
                    |row, heap| call.put(row, heap),
                );
            }
        }

        // Push per-tick movement/look commands (`pkt.commands`) into the client's mailbox
        // (`world.input(id, command)` in `src/runtime/snapshot-host.js` and `Native::submit` in
        // `rust/bfe-client/src/local/context.rs`).
        let mailbox_handle = self
            .mailbox_by_native
            .get(&native_id)
            .copied()
            .unwrap_or(self.admitted_handles.mailbox);
        let player_handle = self
            .player_by_native
            .get(&native_id)
            .copied()
            .unwrap_or(self.admitted_handles.player);
        if mailbox_handle != 0 && player_handle != 0 {
            let view_fields: Vec<ImField> = self
                .view_field_str_ids
                .iter()
                .map(|&key| ImField { key, value: 0.0 })
                .collect();
            for cmd in &pkt.commands {
                let lead = cmd.tick as f64 - self.tick as f64;
                if !lead.is_finite() || lead > 120.0 {
                    continue;
                }
                let codes: Vec<StrId> = cmd
                    .down_codes
                    .iter()
                    .filter_map(|&idx| self.code_str_ids.get(idx).copied())
                    .collect();
                let entry = ImEntry {
                    identity: cmd.sequence as f64,
                    sequence: cmd.sequence as f64,
                    tick: cmd.tick as f64,
                    generation: cmd.generation as f64,
                    down: cmd.sequence as f64,
                    codes,
                    look_total: Some(cmd.look_total),
                    publication: cmd.view.clone(),
                    view: view_fields.clone(),
                    ..Default::default()
                };
                let mailbox_call = InputMailboxCall {
                    op: ImOperation::Push,
                    entry: Some(entry),
                    sequence: cmd.sequence as f64,
                    epoch_matches: cmd.epoch == self.epoch,
                    tick: self.tick as f64,
                    generation: cmd.generation as f64,
                };
                let mut accepted = false;
                if self
                    .run_record_op(
                        self.op_mailbox_run,
                        &[mailbox_handle as f64],
                        InputMailboxCall::WORDS,
                        |row, heap| mailbox_call.put(row, heap),
                    )
                    .is_ok()
                {
                    let words = &self.op_block.words;
                    if words.len() >= HEADER + ImResult::WORDS {
                        let start = words[BlockHeader::HEAP] as usize;
                        let count = words[BlockHeader::HEAP_WORDS] as usize;
                        if start + count <= words.len() {
                            let res = ImResult::take(
                                &words[HEADER..HEADER + ImResult::WORDS],
                                &words[start..start + count],
                            );
                            accepted = res.accepted;
                        }
                    }
                }
                let _ = self.context.players_input(
                    player_handle,
                    self.last_now_ms,
                    lead,
                    u32::from(accepted),
                );
            }
        }
    }

    /// Steps the authoritative simulation by one 60 Hz tick and returns the decoded publication
    /// (`step()` and `publish()` in `src/world-worker.js`).
    ///
    /// Execution order per tick (`world-worker.js` and `Context::world_tick` in `rust/sim/src/host/mod.rs`):
    /// 1. Call `self.context.world_tick(...)`, which runs:
    ///    - `advanceEdges` across every joined client registered via `world_host_client`,
    ///    - the pre-step owner input banks (`F2_INPUT`, `COLLISION_ENABLED`, `MOVEMENT_FLAGS`,
    ///      `F3_SEPARATION`, `F4_LIVE_INPUT`, `F4_TAIL_INPUT`) and AI hearing journal (`AI_JOURNAL`),
    ///    - `world_step` to advance physics, combat, capture posts, and bots,
    ///    - the post-step unit notice drain into the `SimEvents` journal and presentation
    ///      `Capture` (schema 8 cosmetic records), dispenser bindings, and `F3_ACKNOWLEDGEMENT`,
    ///    - `mirrorShots` and `mirror` (`players_shots`, `players_views`), and
    ///    - `consumedEdges` across every joined client.
    /// 2. Inspect `SimBattle` victory state and publish into `self.published`
    ///    (`world_publish_into`), then decode with `decode_publication_block`.
    pub fn step(
        &mut self,
        now_ms: f64,
        unix_ms: u64,
    ) -> Result<Option<DecodedPublication>, String> {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        // WorldTick.input derives the next identity from the completed clock.
        // A refused host boundary must not strand every later step one tick ahead.
        self.tick = if self.publication_session != 0 {
            self.tick_input.clock.tick as u32 + 1
        } else {
            self.tick + 1
        };
        self.tick_input.tick = self.tick as f64;
        self.tick_input.at = self.last_now_ms;

        // When `world_admit` initialized a live publication session, run `world_tick` and
        // publish into the pre-allocated `self.published` buffer (`world_tick` + `world_publish_into`).
        if self.publication_session != 0 {
            // preparePhase's foreign rows change when bots or connected players
            // spawn and die. Admission's count only describes the initial world.
            self.tick_input.units = self.context.world_prepare_units().map_err(|e| e.to_string())?;
            let output = self
                .context
                .world_tick(self.tick_input.clone())
                .map_err(|e| e.to_string())?;
            self.tick_input.clock.tick = output.clock.tick;
            self.tick_input.clock.elapsed = output.clock.elapsed;
            self.tick_input.clock.compensation = output.clock.compensation;
            self.tick_input.clock.debt = output.clock.debt;
            self.tick_input.clock.alpha = output.clock.alpha;
            self.tick_input.clock.overloads = output.clock.overloads;
            self.tick_input.clock.dropped = output.clock.dropped;
            self.tick_input.clock.step_cost = output.clock.step_cost;
            // WorldTick.complete never publishes a refused authority step.
            if !output.accepted {
                return Err(format!(
                    "native world refused authoritative tick {} (clock {}, foreign rows {})",
                    self.tick_input.tick, output.clock.tick, self.tick_input.units,
                ));
            }

            if let Ok(words) = self.context.export(u32::from(gen::KIND_BATTLE), 0) {
                if words.len() >= HEADER + SimBattle::EXPORT_STRIDE {
                    let stride = words[BlockHeader::STRIDE] as usize;
                    let heap_at = words[BlockHeader::HEAP] as usize;
                    let heap_len = words[BlockHeader::HEAP_WORDS] as usize;
                    if stride == SimBattle::EXPORT_STRIDE && heap_at + heap_len <= words.len() {
                        self.battle_mirror.take_export(
                            &words[HEADER..HEADER + stride],
                            &words[heap_at..heap_at + heap_len],
                        );
                        if self.battle_mirror.finished {
                            self.ended = true;
                            self.winner = match self.battle_mirror.loser {
                                Some(1.0) => Some(2),
                                Some(2.0) => Some(1),
                                _ => Some(0),
                            };
                        }
                    }
                }
            }
            let _ = self
                .context
                .read_bank(bank::F4_BATTLE, &mut self.bank_words);

            self.facts_words.clear();
            self.facts_words.resize(WireWorldFacts::WORDS, 0.0);
            self.facts_words[WireWorldFacts::TICK] = self.tick as f64;
            self.facts_words[WireWorldFacts::AT] = self.last_now_ms;
            self.facts_words[WireWorldFacts::UNIX_MS] = unix_ms as f64;
            self.facts_words[WireWorldFacts::DROPPED_TICKS] = self.dropped_ticks as f64;
            self.facts_words[WireWorldFacts::LINKS + 1] = self.links.len() as f64;
            for link in &mut self.links {
                if let Some(player) = self.player_by_native.get(&link.id) {
                    link.discarded_actions = self
                        .context
                        .world_network_discarded(*player)
                        .map_err(|e| e.to_string())?;
                }
                let mut row = [0.; WirePublicationLink::WORDS];
                link.put(&mut row, &mut Vec::new());
                self.facts_words.extend_from_slice(&row);
            }
            self.context
                .world_publish_into(
                    self.publication_session,
                    &self.facts_words,
                    &mut self.published,
                )
                .map_err(|e| e.to_string())?;
            let decoded = decode_publication_block(&self.published, &self.client_by_native)?;
            return Ok(Some(decoded));
        }

        // In synthetic publication-replay tests (`recorded_publications`), decode the next recorded frame.
        if self.recorded_cursor < self.recorded_publications.len() {
            let pub_frame = &self.recorded_publications[self.recorded_cursor];
            self.recorded_cursor += 1;
            let decoded = decode_publication_block(pub_frame, &self.client_by_native)?;
            return Ok(Some(decoded));
        }

        Ok(None)
    }
}

impl Drop for NativeWorldHost {
    /// Disposes the underlying `bfe_sim::abi::context::Context` when `NativeWorldHost` is dropped
    /// (`game.dispose()` in `src/world-worker.js`), ensuring `Context::world_replay_live_handles()`
    /// returns to `0` across sequential battle loads, map rotations, and server teardown.
    fn drop(&mut self) {
        let _ = self.context.dispose();
    }
}
