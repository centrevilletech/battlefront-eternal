//! Native authoritative world host (`src/world-worker.js`, `SnapshotHost` in
//! `src/runtime/snapshot-host.js`, and `createMultiplayerWorld` in `src/runtime/multiplayer-world.js`).
//!
//! Why `NativeWorldHost` owns the simulation context and wire codecs on the server's tick thread:
//! In the Node dedicated server (`src/world-worker.js`), the worker thread loads the mission
//! (`createGame` + `createMultiplayerWorld` + `createSnapshotHost`), steps the simulation at 60 Hz,
//! and publishes accumulated entity state at 20 Hz (`publish()` in `src/world-worker.js`).
//!
//! In `bfeserver`, `NativeWorldHost` ports that pipeline directly over `bfe_sim::abi::context::Context`:
//! 1. **Cold world admission (`Context::world_admit` in `rust/sim/src/abi/context_admit.rs`)**:
//!    `load_from_content` parses the battle query (`?map=...&side=...&ai=...&seed=...`) into
//!    `bfe_sim::world::admit::Options` and calls `context.world_admit(&files, &admit_options)`.
//!    - When `admit::provided()` is `true` and admission succeeds, `NativeWorldHost` calls
//!      `context.world_step_init(admitted.setup)`, stores `admitted.handles.publication` and
//!      `admitted.handles.host`, uses `admitted.registry`, and sets `world_admitted = true`.
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

use crate::udp_peer::{ACK_MAGIC, INPUT_MAGIC};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bfe_content::battle::{Battle, Loader};
use bfe_content::content::Content;
use bfe_content::defs::Defs;
use bfe_content::files::DirFiles;
use bfe_content_native::fingerprint;
use bfe_content_native::setup::battle_clock;
use bfe_sim::abi::context::{Context, ErrorKind as SimErrorKind};
use bfe_sim::abi::context_step::{bank, Publication};
use bfe_sim::gen::{
    self, Battle as SimBattle, BlockHeader, WiView, WirePublicationLink, WirePublicationOutput,
    WirePublishedAck, WirePublishedEdges, WirePublishedFault, WirePublishedGroup,
    WirePublishedOutcome, WirePublishedOwn, WirePublishedRecord, WirePublishedReliable, WireSpan,
    WireWorldFacts, WorldTickInput,
};
use bfe_sim::store::block::{Block, HEADER};
use bfe_sim::store::StrId;
use bfe_sim::wire::publisher::Publisher;
use bfe_sim::world::admit::Options as AdmitOptions;
use bfe_transport::layout::{INPUT_ACTIONS, INPUT_CODES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

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
/// Decodes a 9-byte client `snapshot-ack` (`0x02`) or `keepalive` (`0x03`) datagram on Channel 1
/// (`Lane::Input`), porting `decodeSnapshotAck(bytes)` in `packages/bfe-server/src/world-worker.js`.
///
/// Exact 9-byte wire layout (little-endian):
/// - `0`:    kind (`0x02` = `snapshot-ack`, `0x03` = `keepalive`)
/// - `1..5`: `epoch` (`u32` LE, masked to `u16` by caller) - match epoch of the acknowledged state packet
/// - `5..9`: `ack_tick` (`u32` LE) - authoritative simulation tick received by the client
///
/// Why `snapshot-ack` shares Channel 1 with `entity-input`:
/// Keeping all client-to-server high-frequency unreliable control frames on `Lane::Input` lets
/// `NativeWorldHost` advance the client's delta-compression baseline (`ack_tick`) immediately
/// before encoding the next tick's `bni1` state snapshot.
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
/// Decodes a `BFEI` v2 client command frame from Channel 1 (`Lane::Input`), porting
/// `decodeEntityInput(bytes)` in `packages/bfe-server/src/world-worker.js`.
///
/// Wire layout (`BFEI` v2, little-endian):
/// - Header (`16` bytes):
///   - `0..4`:   magic `"BFEI"` (`0x42, 0x46, 0x45, 0x49`)
///   - `4`:      version `2` (`u8`)
///   - `5`:      frame type `0x01` (`entity-input`, `u8`)
///   - `6..8`:   `epoch` (`u16` LE) - match epoch counter; frames from prior map rotations are dropped
///   - `8..12`:  `ack_tick` (`u32` LE) - piggybacked client snapshot baseline ack
///   - `12`:     `cmd_count` (`u8`, `1..=MAX_INPUT_COMMANDS`) - number of redundant `DecodedInputCommand`s
///   - `13`:     `edge_count` (`u8`, `0..=MAX_INPUT_EDGES`) - discrete one-shot action edges
///   - `14..16`: reserved (`u16` LE)
/// - Followed by `cmd_count * 30` bytes of continuous movement/aim commands (`throttle`, `strafe`,
///   `yaw`, `pitch`, `buttons`, `weapon_slot`, `view_quat`) and `edge_count * 12` bytes of discrete
///   `DecodedEdge` records, all validated against NaN/Inf floats and clamped to unit ranges.
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
/// Builds the authoritative `entity-registry` descriptor (`{ type: "entity-registry", epoch, hash, entries }`)
/// sent reliably on Channel 0 (`Lane::Events`) immediately after a client completes the UDP handshake,
/// porting `buildEntityRegistry()` in `packages/bfe-server/src/world-worker.js`.
///
/// Why the registry is hashed and sorted deterministically by `typeId`:
/// Both the Electron client (`packages/bfe-net`) and native `bfe-client` verify the 64-hex SHA-256
/// `hash` over the canonical `typeId:className:category` table before decoding any `bni1` state
/// snapshot, preventing silent quantization mismatches if content packs differ.
pub fn build_entity_registry_descriptor(
    root: &Path,
    map: &str,
    query: &str,
    content_epoch: u32,
) -> Result<(Defs, Value), String> {
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
    Ok((defs, descriptor))
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
///
/// Unpacks a raw authoritative `bfe_sim::Publication` byte block into per-client reliable events,
/// shared multicast state groups, and owner-private state packets, porting
/// `decodePublicationBlock(block)` in `packages/bfe-server/src/world-worker.js`.
///
/// Publication wire layout produced by `bfe_sim::Context::step` (`world-worker.js`):
/// - Header (`32` bytes):
///   - `0..4`:   magic `"BFEP"` (`0x42, 0x46, 0x45, 0x50`)
///   - `4..6`:   version `1` (`u16` LE)
///   - `6..8`:   `epoch` (`u16` LE)
///   - `8..12`:  `tick` (`u32` LE)
///   - `12..16`: `ack_count` (`u32` LE) - number of client input sequence acknowledgments
///   - `16..20`: `edge_count` (`u32` LE) - number of discrete gameplay edge notifications
///   - `20..24`: `event_count` (`u32` LE) - number of reliable JSON gameplay events
///   - `24..28`: `shared_group_count` (`u32` LE) - number of visibility-culled `bni1` delta groups
///   - `28..32`: `own_packet_count` (`u32` LE) - number of owner-specific `bni1` state packets
/// - Why `shared_groups` and `own_packets` are separated:
///   Clients sharing the same `ack_tick` baseline receive identical `bni1` delta bytes from a
///   `OutgoingSharedGroup` without re-encoding per recipient, while player-private prediction state
///   is delivered in `OutgoingOwnPacket`.
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

    for rel in slice_table(WirePublicationOutput::RELIABLE, WirePublishedReliable::WORDS)?
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
/// Constructs the reliable Channel 0 (`Lane::Events`) `welcome` JSON payload sent to a newly
/// admitted player right after `entity-registry`, porting `buildWelcome(info, serverId, player)`
/// from `packages/bfe-server/src/world-worker.js`.
///
/// Includes `serverId`, `epoch`, `tick`, `playerId`, `slot`, `team`, `map`, `mode`, `match`, and
/// `maxPlayers` so both `bfe-client` and the Electron renderer can initialize their local world
/// state and HUD before the first `bni1` state snapshot arrives on Channel 2.
pub fn build_welcome_message(info: &Value, server_id: &str, player: &PlayerWelcome) -> Value {
    let mut map = info.as_object().cloned().unwrap_or_default();
    map.insert("type".to_string(), Value::String("welcome".to_string()));
    map.insert(
        "serverId".to_string(),
        Value::String(server_id.to_string()),
    );
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
///   `world_step_init(admitted.setup)` is initialized, and `step()` drives `world_step` +
///   `world_publish_into` on the admitted world.
/// - While `Context::world_admit` returns `ErrorKind::Unavailable` (before the `admit` lane flips
///   `admit::provided()` to `true`), `world_admitted` is `false` so `/healthz` reports
///   `multiplayerReady: false`.
///
/// Authoritative in-process simulation host porting the worker-side responsibilities of
/// `packages/bfe-server/src/world-worker.js`.
///
/// How `NativeWorldHost` builds its world today (`load_from_content`):
/// 1. Enumerates all map/vehicle/mode files under the validated content root into `[HostFile]`
///    slices (`bfe_sim::HostFile`) and parses the playlist query into `bfe_sim::AdmitOptions`
///    (`parse_admit_options`).
/// 2. Creates `bfe_sim::Context::new()` and invokes `ctx.world_admit(&host_files, &admit_options)`.
/// 3. When `world_admit` returns `Ok(())`, `world_admitted` is set to `true` and `LocalDedicatedServer`
///    reports `multiplayerReady: true` on `/healthz`.
/// 4. While the `admit` lane has not landed yet (`bfe_sim::world::admit::provided() == false`),
///    `ctx.world_admit(...)` returns `Err(ErrorKind::Unavailable)`. `NativeWorldHost` records
///    `world_admitted = false` so `LocalDedicatedServer` and `/healthz` honestly report
///    `multiplayerReady: false` while still allowing transport, handshake, `entity-registry`,
///    `welcome`, and synthetic `Publication` tests to run.
pub struct NativeWorldHost {
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
    /// Whether the active battle has reached victory/defeat (`battle_mirror.finished`).
    pub ended: bool,
    /// Winning team (`1`, `2`, or `0` for draw) once `ended` is `true`.
    pub winner: Option<u8>,
    /// `true` if and only if `Context::world_admit` succeeded on this world (`multiplayerReady`).
    pub world_admitted: bool,
    /// Reusable per-tick input struct passed to `context.world_step`.
    pub tick_input: WorldTickInput,
    /// Native publication session handle returned by `admitted.handles.publication` (`0` when unadmitted).
    pub publication_session: u32,
    /// Native host handle returned by `admitted.handles.host` (`0` when unadmitted).
    pub host_handle: u32,
    /// Pre-allocated `Publication` buffer reused on every `world_publish_into` call.
    pub published: Publication,
    /// Pre-allocated operation block buffer for `wireHostOp` / `wireEdgesRun` / `inputMailboxOwnedRun`.
    pub op_block: Block,
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
        let (defs, mut registry_descriptor) =
            build_entity_registry_descriptor(root, &map, query, epoch)?;

        let context = Context::new();
        let admit_options = parse_admit_options(query, &map);
        let mut world_admitted = false;
        let mut publication_session = 0u32;
        let mut host_handle = 0u32;
        let mut initial_units = 0u32;

        // Attempt cold world admission via `Context::world_admit`.
        // When `admit::provided()` is `false` (`SimErrorKind::Unavailable`), fall back to
        // `players_init()` and keep `world_admitted = false` so `/healthz` reports `multiplayerReady: false`.
        match context.world_admit(&files, &admit_options) {
            Ok(admitted) => {
                context
                    .world_step_init(admitted.setup)
                    .map_err(|e| e.to_string())?;
                publication_session = admitted.handles.publication;
                host_handle = admitted.handles.host;
                initial_units = admitted.units;
                if !admitted.registry.is_null() {
                    registry_descriptor = admitted.registry;
                }
                world_admitted = true;
            }
            Err(err) if err.kind() == SimErrorKind::Unavailable => {
                let _ = context.players_init();
            }
            Err(err) => return Err(err.to_string()),
        }

        // Pre-intern `INPUT_CODES` and `INPUT_ACTIONS` UTF-16 string IDs in `context` once at battle load
        // so the 60 Hz input mailbox path never allocates or re-interns strings per tick.
        let mut code_str_ids = Vec::with_capacity(INPUT_CODES.len());
        for code in &INPUT_CODES {
            let units: Vec<u16> = code.encode_utf16().collect();
            let sid = context.intern(&units).map_err(|e| e.to_string())?;
            code_str_ids.push(sid);
        }
        let mut action_str_ids = Vec::with_capacity(INPUT_ACTIONS.len());
        for action in &INPUT_ACTIONS {
            let units: Vec<u16> = action.encode_utf16().collect();
            let sid = context.intern(&units).map_err(|e| e.to_string())?;
            action_str_ids.push(sid);
        }

        // Pre-allocate reusable `Publication`, `op_block`, `bank_words`, and `facts_words` buffers
        // so `step()` and `world_publish_into` run with zero heap growth per tick.
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

        Ok(Self {
            context,
            defs,
            registry_descriptor,
            fingerprint: fp,
            query: query.to_string(),
            map,
            epoch,
            tick: 0,
            ended: false,
            winner: None,
            world_admitted,
            tick_input: WorldTickInput {
                dt: 1.0 / 60.0,
                tick: 1.0,
                clock: battle_clock(),
                units: initial_units,
                ai: true,
                live: true,
                camera: 0,
                offline_camera: true,
                squads: 0,
                at: 0.0,
                mouse_scale: 1.0,
            },
            publication_session,
            host_handle,
            published,
            op_block,
            bank_words: Vec::with_capacity(1024),
            facts_words: Vec::with_capacity(
                WireWorldFacts::WORDS + 64 * WirePublicationLink::WORDS,
            ),
            battle_mirror: SimBattle::default(),
            client_by_native: HashMap::new(),
            native_by_client: HashMap::new(),
            player_welcomes: HashMap::new(),
            op_host_op,
            op_edges_run,
            op_mailbox_run,
            code_str_ids,
            action_str_ids,
            recorded_publications: Vec::new(),
            recorded_cursor: 0,
        })
    }

    /// Admits a player into the world roster (`world.addPlayer` and `entityHost.addClient` in
    /// `handle("join")` in `src/world-worker.js`).
    pub fn add_player(
        &mut self,
        peer_id: &str,
        spectator: bool,
        auto_spawn: bool,
        team_select: bool,
    ) -> Result<PlayerWelcome, String> {
        let default_class = self
            .defs
            .classes
            .first()
            .map(|c| self.defs.names.text(c.name).to_string())
            .or_else(|| self.defs.registry.classes.first().cloned())
            .ok_or_else(|| "no player classes in content".to_string())?;
        let ordinal = (self.player_welcomes.len() + 1) as u32;
        let team = if ordinal % 2 == 1 { 1 } else { 2 };
        let unit_id = if spectator || !auto_spawn { 1 } else { ordinal };
        let generation = if spectator || !auto_spawn { 0 } else { 1 };
        let native_id = ordinal;
        self.client_by_native
            .insert(native_id, peer_id.to_string());
        self.native_by_client
            .insert(peer_id.to_string(), native_id);
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
        }
        self.player_welcomes.remove(peer_id);
    }

    /// Steps the authoritative simulation by one 60 Hz tick and returns the decoded publication
    /// (`step()` and `publish()` in `src/world-worker.js`).
    /// Advances the authoritative simulation by one 60 Hz tick (`dt = 1/60s`), porting `stepTick()`
    /// in `packages/bfe-server/src/world-worker.js`.
    ///
    /// Execution order per tick (`world-worker.js`):
    /// 1. Stage all queued player join/leave roster changes, `BFEI` v2 movement/aim inputs, and
    ///    `snapshot-ack` baseline ticks into `self.ctx` via typed `bfe_sim` input writers.
    /// 2. Call `self.ctx.step()` to advance physics, combat, capture points, and bots.
    /// 3. Drain all produced `bfe_sim::Publication` blocks (plus any recorded test publications)
    ///    and decode them with `decode_publication_block` for transmission by `UdpGameServer`.
    pub fn step(
        &mut self,
        now_ms: f64,
        unix_ms: u64,
    ) -> Result<Option<DecodedPublication>, String> {
        self.tick += 1;
        self.tick_input.tick = self.tick as f64;
        self.tick_input.at = now_ms;

        // When `world_admit` initialized a live publication session, step the native world and
        // publish into the pre-allocated `self.published` buffer (`world_step` + `world_publish_into`).
        if self.publication_session != 0 {
            let output = self
                .context
                .world_step(self.tick_input.clone())
                .map_err(|e| e.to_string())?;
            self.tick_input.clock.tick = output.clock.tick;
            self.tick_input.clock.elapsed = output.clock.elapsed;
            self.tick_input.clock.compensation = output.clock.compensation;
            self.tick_input.clock.debt = output.clock.debt;
            self.tick_input.clock.alpha = output.clock.alpha;
            self.tick_input.clock.overloads = output.clock.overloads;
            self.tick_input.clock.dropped = output.clock.dropped;
            self.tick_input.clock.step_cost = output.clock.step_cost;

            if let Ok(words) = self.context.export(u32::from(gen::KIND_BATTLE), 0) {
                if words.len() >= HEADER + SimBattle::EXPORT_STRIDE {
                    let stride = words[BlockHeader::STRIDE] as usize;
                    let heap_at = words[BlockHeader::HEAP] as usize;
                    let heap_len = words[BlockHeader::HEAP_WORDS] as usize;
                    if stride == SimBattle::EXPORT_STRIDE && heap_at + heap_len <= words.len() {
                        self.battle_mirror.take_import(
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
            let _ = self.context.read_bank(bank::F4_BATTLE, &mut self.bank_words);

            self.facts_words.clear();
            self.facts_words.resize(WireWorldFacts::WORDS, 0.0);
            self.facts_words[WireWorldFacts::TICK] = self.tick as f64;
            self.facts_words[WireWorldFacts::AT] = now_ms;
            self.facts_words[WireWorldFacts::UNIX_MS] = unix_ms as f64;
            self.facts_words[WireWorldFacts::DROPPED_TICKS] = 0.0;
            self.context
                .world_publish_into(
                    self.publication_session,
                    &self.facts_words,
                    &mut self.published,
                )
                .map_err(|e| e.to_string())?;
            let decoded =
                decode_publication_block(&self.published, &self.client_by_native)?;
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
