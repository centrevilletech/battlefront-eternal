//! Native authoritative world host (`SnapshotHost` in
//! `src/runtime/snapshot-host.js`, and `createMultiplayerWorld`).

use crate::udp_peer::{ACK_MAGIC, INPUT_MAGIC};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bfe_content::battle::{Battle, Loader};
use bfe_content::content::Content;
use bfe_content::defs::Defs;
use bfe_content::files::DirFiles;
use bfe_content_native::fingerprint;
use bfe_content_native::setup::first_tick;
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
use bfe_family::GameFamily;
use bfe_transport::layout::{INPUT_ACTIONS, INPUT_CODES, SWBF2_INPUT_ACTIONS, SWBF2_INPUT_CODES};
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
/// `handle("join")`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerWelcome {
    /// Authoritative unit entity ID controlled by this player (`1` when spectator or unspawned).
    pub unit_id: u32,
    /// Alias of `unit_id` (`playerId: m.player.unitId`).
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
    /// Server simulation tick at the moment `welcome` was generated (`m.tick`).
    pub tick: u32,
}

/// Single per-tick movement/look command decoded from a `BNI1` input packet
/// (`commands` entry in `decodeEntityInput`).
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
/// (`edges` entry in `decodeEntityInput`).
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

/// Complete decoded `BNI1` client entity-input datagram (`decodeEntityInput`).
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

/// Decoded `BNI1` packet plus any 2005 records-section entries carried beside it.
#[derive(Clone, Debug)]
pub struct DecodedPlannedInput {
    pub packet: DecodedInputPacket,
    pilot_entries: [bfe_sim::wire::space::PilotEntry; bfe_sim::wire::space::MAX_PILOT_ENTRIES],
    pilot_count: usize,
}

impl DecodedPlannedInput {
    pub fn pilot_entries(&self) -> &[bfe_sim::wire::space::PilotEntry] {
        &self.pilot_entries[..self.pilot_count]
    }
}

/// Decodes a 10-byte `BNA1` (`0x31414e42`) snapshot acknowledgement keepalive
/// (`export function decodeSnapshotAckKeepalive`).
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
/// (`export function decodeEntityInput`).
///
/// Validates all scalar finiteness, count bounds (`nv <= 8`, `nc <= 8`, `ne <= 32`, `na <= 1`),
/// view flag ranges, `INPUT_CODES` bitmask indices, `INPUT_ACTIONS` indices, fire aim bounds,
/// and rejects any packet with trailing unconsumed bytes (`at != bytes.len()`).
/// This is the 2004 game's decoder, exactly as released.
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
        // must be finite (`Number.isFinite(v)`) so `NaN` or `Infinity`
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

    // Decode `nv` deduplicated view descriptors (`WiView` in `decodeEntityInput`).
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

/// The input tables of a battle's game: held codes, then action names, each
/// indexed by its wire value. The 2005 tables extend the 2004 ones.
pub fn input_tables(family: GameFamily) -> (&'static [&'static str], &'static [&'static str]) {
    match family {
        GameFamily::Swbf1 => (&INPUT_CODES, &INPUT_ACTIONS),
        GameFamily::Swbf2 => (&SWBF2_INPUT_CODES, &SWBF2_INPUT_ACTIONS),
    }
}

/// Decodes a `BNI1` datagram for a battle of `family`. A 2004 battle uses the
/// released decoder; a 2005 battle accepts its own held codes and actions.
pub fn decode_entity_input_for(bytes: &[u8], family: GameFamily) -> Result<DecodedInputPacket, &'static str> {
    decode_entity_input_planned(bytes, family, None)
}

/// [`decode_entity_input_for`] with a 2005 client's plan of the match, by
/// which the records section of its packets is read.
pub fn decode_entity_input_planned(bytes: &[u8], family: GameFamily, plan: Option<&bfe_sim::wire::revisions::Plan>)
    -> Result<DecodedInputPacket, &'static str>
{
    decode_entity_input_with_records(bytes, family, plan).map(|decoded| decoded.packet)
}

/// Decodes a `BNI1` datagram together with any 2005 records-section entries
/// (such as Kind 16 `PILOT` controls) allowed by `plan`.
pub fn decode_entity_input_with_records(
    bytes: &[u8],
    family: GameFamily,
    plan: Option<&bfe_sim::wire::revisions::Plan>,
) -> Result<DecodedPlannedInput, &'static str> {
    match family {
        GameFamily::Swbf1 => decode_entity_input(bytes).map(|packet| DecodedPlannedInput {
            packet,
            pilot_entries: [Default::default(); bfe_sim::wire::space::MAX_PILOT_ENTRIES],
            pilot_count: 0,
        }),
        GameFamily::Swbf2 => decode_entity_input_swbf2(bytes, plan),
    }
}

// The 2005 game's decoder: a copy of the released decoder's layout and checks
// with the 2005 tables (`codes` held codes and `actions` action IDs) and the
// 2005 records section after the acknowledgement. The released decoder above
// stays the 2004 game's, unchanged.
fn decode_entity_input_swbf2(bytes: &[u8], plan: Option<&bfe_sim::wire::revisions::Plan>) -> Result<DecodedPlannedInput, &'static str> {
    let (codes, actions) = (SWBF2_INPUT_CODES.len(), SWBF2_INPUT_ACTIONS.len());
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
        // must be finite (`Number.isFinite(v)`) so `NaN` or `Infinity`
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
    // Bit 0: an acknowledgement follows; bit 1: a 2005 records section follows it.
    let flags = u8_at(&mut at)? as usize;
    let (na, records) = (flags & 1, flags & 2 != 0);
    if nv > 8 || nc > 8 || ne > 32 || flags > 3 {
        return Err("input count limit");
    }

    // Decode `nv` deduplicated view descriptors (`WiView` in `decodeEntityInput`).
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
                    if code_idx >= codes {
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
                if (act as usize) >= actions {
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
    // The 2005 records section: every record names its kind, revision and
    // length, validated against the client's plan of the match.
    let mut pilot_entries = [bfe_sim::wire::space::PilotEntry::default(); bfe_sim::wire::space::MAX_PILOT_ENTRIES];
    let mut pilot_count = 0usize;
    if records {
        use bfe_sim::wire::revisions::{Section, Use, PILOT, SECTION_RECORDS};
        let mut section_buf = [Section { kind: 0, revision: 0, body: &[] }; SECTION_RECORDS];
        let (section_len, used) =
            bfe_sim::wire::revisions::read_section_into(&bytes[at..], &mut section_buf)?;
        let mut saw_pilot = false;
        for record in &section_buf[..section_len] {
            match plan.map(|plan| plan.of(record.kind)) {
                Some(Use::Revision(revision)) if revision == record.revision => {
                    if record.kind == PILOT && record.revision == bfe_sim::wire::space::REVISION {
                        if saw_pilot {
                            return Err("duplicate Battlefront II pilot record");
                        }
                        saw_pilot = true;
                        let count = bfe_sim::wire::space::read_pilot_entries_into(record.body, &mut pilot_entries)?;
                        for i in 0..count {
                            let seq = pilot_entries[i].sequence;
                            if !commands.iter().any(|c| c.sequence == seq)
                                || pilot_entries[..i].iter().any(|prev| prev.sequence == seq)
                            {
                                return Err("invalid Battlefront II pilot sequence");
                            }
                        }
                        pilot_count = count;
                    } else {
                        return Err("a Battlefront II input record this server does not read");
                    }
                }
                Some(Use::Revision(_)) => {
                    return Err("a Battlefront II input record of another revision than the match's");
                }
                Some(Use::Skip(_)) => {
                    return Err("a Battlefront II input record the client skips");
                }
                Some(Use::Unnamed) | None => {
                    return Err("a Battlefront II input record this match does not carry");
                }
            }
        }
        at += used;
    }
    if at != bytes.len() {
        return Err("trailing input bytes");
    }
    Ok(DecodedPlannedInput {
        packet: DecodedInputPacket {
            epoch,
            generation,
            commands,
            edges,
            snapshot_ack,
        },
        pilot_entries,
        pilot_count,
    })
}

/// Builds the canonical `entity-registry` JSON descriptor from prepared content
/// (`entityRegistryDescriptor` in `src/runtime/network-entity-registry.js` and
/// `ensureEntityHost().descriptor()`).
pub fn build_entity_registry_descriptor(
    root: &Path,
    map: &str,
    query: &str,
    content_epoch: u32,
) -> Result<(Defs, Value), String> {
    let files = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf1);
    let (defs, descriptor, _) = load_entity_registry(&files, map, query, content_epoch, GameFamily::Swbf1)?;
    Ok((defs, descriptor))
}

// Retain the registry's objects, not duplicate map geometry, for cold joins.
fn load_entity_registry(
    files: &DirFiles,
    map: &str,
    query: &str,
    content_epoch: u32,
    family: GameFamily,
) -> Result<(Defs, Value, bfe_content::value::Value), String> {
    let loader = Loader::new(files);
    let battle = match files.family {
        bfe_family::GameFamily::Swbf1 => Battle::load(&loader, map, query),
        family => Battle::load_for_family(&loader, family, map, query),
    }?;
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
        "inputCodes": input_tables(family).0,
        "bindingProfiles": [[0, {}]],
    });
    Ok((defs, descriptor, bfe_content::registry::registry_content(&content)))
}

/// Outbound JSON message addressed to a single peer (`toClients`).
#[derive(Clone, Debug)]
pub struct OutgoingPeerEvents {
    pub peer_id: String,
    pub message: Value,
}

/// Shared `BNS1` entity delta snapshot group encoded once per baseline and fanned out to all
/// `clients` sharing that baseline (`out.shared` in `publish()`).
#[derive(Clone, Debug)]
pub struct OutgoingSharedGroup {
    pub clients: Vec<String>,
    pub parts: Vec<Vec<u8>>,
    pub captured_at_unix_ms: u64,
}

/// Per-client `BNO1` own-player correction datagram (`out.ownPackets` in `publish()`).
#[derive(Clone, Debug)]
pub struct OutgoingOwnPacket {
    pub peer_id: String,
    pub bytes: Vec<u8>,
    pub captured_at_unix_ms: u64,
}

/// Decoded output of one 20 Hz world publication (`publish()`).
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

/// Publication faults per player. The publisher marks a fault for one player
/// when it cannot write that player's update; the world itself is intact and
/// the other players are served. Each player and reason is logged once; a
/// player still faulting a second after the fault began is returned once, with
/// the reason, so the server can end that session instead of leaving the player
/// frozen. A world-wide fault (no player) is logged once per reason.
#[derive(Default)]
pub struct FaultWatch {
    open: HashMap<String, u64>,
    logged: std::collections::HashSet<(Option<String>, String)>,
}

impl FaultWatch {
    pub const LIMIT_MS: u64 = 1000;

    pub fn note(&mut self, faults: &[(Option<String>, String)], now_ms: u64) -> Vec<(String, String)> {
        let mut ended = Vec::new();
        self.open.retain(|peer, _| faults.iter().any(|(p, _)| p.as_deref() == Some(peer)));
        for (peer, message) in faults {
            if self.logged.insert((peer.clone(), message.clone())) {
                match peer {
                    Some(peer) => eprintln!("publication fault for player {peer}: {message}; their update is skipped"),
                    None => eprintln!("publication fault for every player: {message}"),
                }
            }
            let Some(peer) = peer else { continue };
            let since = *self.open.entry(peer.clone()).or_insert(now_ms);
            if now_ms.saturating_sub(since) >= Self::LIMIT_MS && !ended.iter().any(|(p, _)| p == peer) {
                ended.push((peer.clone(), message.clone()));
            }
        }
        for (peer, _) in &ended {
            self.open.remove(peer);
        }
        ended
    }

    /// Forgets a player who left, so a later player with the same id starts clean.
    pub fn forget(&mut self, peer: &str) {
        self.open.remove(peer);
        self.logged.retain(|(p, _)| p.as_deref() != Some(peer));
    }
}

/// Decodes a `Publication` (`WirePublicationOutput` block + byte arena) into transport-ready
/// shared `BNS1` groups, per-client `BNO1` packets, `input-ack`, `entity-edge-results`, and
/// `gameplay-events` (`publish()` and `WorldRouter.route`).
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

    // Decode `out.faults` (`snapshotFault`).
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

    // Decode `out.inputAcks` (`input-ack` in `GameSession.inputAck`).
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

    // Decode `out.edgeResults` (`entity-edge-results` in `GameSession.edgeFeedback`).
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

    // Decode `out.reliableEvents` (`gameplay-events` in `WorldRouter.broadcastEvents`).
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

    // Decode `out.shared` baseline groups (`BNS1` datagram parts in `snapshotPublication`).
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

    // Decode `out.ownPackets` (`BNO1` per-client own-player datagrams in `snapshotPublication`).
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

/// Composes the canonical `welcome` event payload sent to a player who joined:
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

/// Whether a 2005 flight ceiling is what a mission script can supply: a
/// finite IEEE binary32 (magnitude at most 3.4028234663852886e38), widened
/// to f64 exactly.
pub fn flight_ceiling_readable(ceiling: f64) -> bool {
    ceiling.is_finite() && f64::from(ceiling as f32) == ceiling
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
    let (mut mode, mut tier, mut size) = ("classic", "medium", "full");
    let mut friendly_fire = true;
    for pair in query.trim_start_matches('?').split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "map" if !v.is_empty() => map = v.to_string(),
                "side" if !v.is_empty() => side = v.to_string(),
                "ai" => {
                    enhanced = v == "enhanced";
                    mode = v;
                }
                "tier" => tier = v,
                "size" => size = v,
                "seed" => {
                    if let Ok(parsed) = v.parse::<u32>() {
                        seed = parsed;
                    }
                }
                "heroes" => heroes = v == "1" || v == "true",
                "unit" if !v.is_empty() => unit = Some(v.to_string()),
                "ff" => friendly_fire = !(v == "0" || v == "false"),
                _ => {}
            }
        }
    }
    AdmitOptions {
        family: bfe_family::GameFamily::Swbf1,
        map,
        side,
        mode: bfe_family::DEFAULT_MODE.to_string(),
        enhanced,
        seed,
        heroes,
        unit,
        practice_target: None,
        name: None,
        bots: bfe_sim::world::admit::Bots { friendly_fire, ..bfe_sim::world::admit::Bots::read(mode, tier, size) },
    }
}


// The mission a 2005 battle's query selects from its map, as the approved
// fingerprint and loader select it.
fn swbf2_mission(files: &DirFiles, map: &str, query: &str) -> Result<Option<String>, String> {
    use bfe_content::files::Files;
    let path = format!("/maps/{map}/map.json");
    let raw = files.read(&path).ok_or("missing map content")?;
    let world = bfe_content::json::parse_bytes(&raw).map_err(|e| e.to_string())?;
    bfe_content::map::pick_swbf2_mission_strict(&world, &path, None, Some(&bfe_content::mission::Query::parse(query)))
}

// Class movement numbers as names and exact words, for comparing two admissions.
fn class_words(classes: &[(String, bfe_sim::movement::soldier::swbf2::Tuning)]) -> Vec<(&str, Vec<[u8; 8]>)> {
    classes.iter().map(|(name, tuning)| (name.as_str(), bfe_sim::movement::soldier::swbf2::words::put(tuning).iter().map(|w| w.to_le_bytes()).collect())).collect()
}

/// Native simulation world host wrapping `bfe_sim::abi::context::Context`.
pub struct NativeWorldHost {
    /// The game of the rotation entry this battle was loaded for: its input
    /// tables, decoder and records.
    pub family: GameFamily,
    /// The movement numbers a 2005 battle's admission read from its content,
    /// by class: a restart must read the same ones. Empty for a 2004 battle.
    swbf2_classes: Vec<(String, bfe_sim::movement::soldier::swbf2::Tuning)>,
    /// The games the server that loaded this battle may host; a restart
    /// checks the battle's game against it again.
    gate: bfe_family::Gate,
    /// A 2005 battle's flight ceiling in metres, the one its soldiers were
    /// admitted with, for the whole epoch: its welcome carries it.
    flight_ceiling: Option<f64>,
    /// Each 2005 client's plan of the match, by peer id.
    plans: HashMap<String, bfe_sim::wire::revisions::Plan>,
    /// The classes a scripted 2005 battle's teams may choose, as joining
    /// clients are told them; none in every other battle.
    roster: Option<bfe_sim::wire::mission::Roster>,
    /// What its mission announces, and to whom it still has to be sent.
    notices: crate::notices::Notices,
    /// The matched registry's weapon ids by class name, which notices name.
    weapon_ids: HashMap<String, u16>,
    /// BFE_AI_TRACE: a census of the bots every ten seconds on stderr.
    census: Option<bfe_sim::host::census::Census>,
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
    mission: Option<MissionLaunch>,
    /// The position box the admitted world's snapshots use; none unadmitted.
    pub wire_bounds: Option<bfe_sim::wire::PositionBounds>,
    /// Active mission query string (`?map=nab2&side=rep&ai=classic&seed=1&match=...`).
    pub query: String,
    /// The query the battle's content is read with: the battle query itself
    /// for a 2004 battle, its bare-map form for a 2005 one.
    content_query: String,
    /// Active map identifier (e.g. `"nab2"`).
    pub map: String,
    /// Monotonic battle epoch (`1` on initial load, incremented on map rotation).
    pub epoch: u32,
    /// Current 60 Hz simulation tick (`0` before first `step()`).
    pub tick: u32,
    /// Cumulative late-tick drop count (`droppedTicks`).
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
    /// Per-client state bytes per second the snapshot host may publish (infinite until set).
    pub state_budget: f64,
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

use bfe_sim::world::admit::mission::{MissionPreferences,MissionSelection};

/// One resolved slot on a scripted dedicated authority.
#[derive(Clone)]
pub struct DeveloperMission {
    pub options: AdmitOptions,
    pub selection: MissionSelection,
    pub preferences: MissionPreferences,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MissionStartup { Checking, Playing, Advance, Parked(&'static str) }

struct MissionRotation {
    slots: Vec<DeveloperMission>,
    next: usize,
    remaining: usize,
}

const ROTATION_STOPPED: &str = "Every mission in the rotation failed to start. The rotation is stopped.";
const MISSION_FAILED: &str = "The mission failed to start.";

impl MissionRotation {
    fn admit(&mut self, root: &Path, epoch: &mut u32) -> Result<Option<NativeWorldHost>, bfe_sim::abi::context::Error> {
        use bfe_sim::abi::context::{Error, ErrorKind};
        while self.remaining > 0 {
            let slot = &self.slots[self.next];
            self.next = (self.next + 1) % self.slots.len();
            self.remaining -= 1;
            match NativeWorldHost::admit_scripted_slot(root, slot, *epoch) {
                Ok(host) => return Ok(Some(host)),
                Err(error) if error.kind() == ErrorKind::MissionSetup => {
                    if self.remaining > 0 {
                        *epoch = epoch.checked_add(1).ok_or_else(|| Error::host("battle epoch exhausted"))?;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        eprintln!("{ROTATION_STOPPED} Checked {} slots.", self.slots.len());
        Ok(None)
    }
}

struct MissionLaunch {
    startup: MissionStartup,
    rotation: Option<MissionRotation>,
    selection: MissionSelection,
    building_sources: Vec<usize>,
}

struct HostAdmission {
    options: AdmitOptions,
    world: Result<bfe_sim::world::admit::Admitted, bfe_sim::abi::context::Error>,
    defs: Defs,
    fingerprint: String,
    descriptor: Value,
    registry: bfe_content::Value,
    mission: Option<MissionLaunch>,
    gate: bfe_family::Gate,
}

fn authority_content(context: &Context, source: u32, registry: &bfe_content::Value, defs: &Defs, mission: Option<&MissionLaunch>)
    -> Result<(Vec<u32>, bfe_sim::host::Content), String> {
    if let Some(m) = mission {
        bfe_sim::host::mission_content_tables(context, source, registry, &defs.registry.combat, &m.building_sources)
    } else {
        bfe_sim::host::content_tables_from_registry(context, source, registry, &defs.registry.combat)
    }
}

impl NativeWorldHost {
    /// Loads the battle of a rotation entry, whose game was decided when the
    /// entry was read (`config::rotation_family`), never from the query. A game
    /// this build cannot play is refused before any content is read; a 2004
    /// battle loads exactly as [`Self::load_from_content`] always loaded it.
    ///
    /// A 2005 battle is read from the 2005 content folder by its bare map
    /// ([`crate::config::swbf2_content`]); the host keeps the battle query as
    /// written for the network and reloads with the content query. Its world
    /// must be admitted: a refused admission is the load's error.
    pub fn load_battle(root: &Path, family: GameFamily, query: &str, epoch: u32, gate: bfe_family::Gate) -> Result<Self, String> {
        gate.require(family).map_err(|e| e.to_string())?;
        match family {
            GameFamily::Swbf1 => Self::load_from_content(root, query, epoch),
            GameFamily::Swbf2 => {
                let (folder, content_query) = crate::config::swbf2_content(root, query)?;
                let mut host = Self::load_for(&folder, &content_query, epoch, family, gate)?;
                host.query = query.to_string();
                Ok(host)
            }
        }
    }

    /// Loads prepared content from `root` for the 2004 battle `query` and
    /// initializes `Context`.
    pub fn load_from_content(root: &Path, query: &str, epoch: u32) -> Result<Self, String> {
        Self::load_from_content_for(root, query, epoch, GameFamily::Swbf1)
    }

    /// The same load for a battle of `family`: its admission, content
    /// fingerprint, input tables, decoder and records follow that game. A
    /// 2004 battle is exactly `load_from_content`.
    pub fn load_from_content_for(root: &Path, query: &str, epoch: u32, family: GameFamily) -> Result<Self, String> {
        Self::load_for(root, query, epoch, family, bfe_family::Gate::BUILT)
    }

    /// `load_from_content_for` under the gate of the server that loads it.
    /// A 2005 battle's admission reads its soldier classes' movement numbers
    /// from the same content its digest covers, and only an admitted 2005
    /// world loads: its admission's refusal is the error. A 2004 battle whose
    /// admission is refused loads a world that is not ready, as released.
    pub fn load_for(root: &Path, query: &str, epoch: u32, family: GameFamily, gate: bfe_family::Gate) -> Result<Self, String> {
        gate.require(family).map_err(|e| e.to_string())?;
        let map = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix("map="))
            .unwrap_or("nab2")
            .to_string();
        let map = if family == bfe_family::GameFamily::Swbf2 {
            let fields = bfe_content::mission::Query::parse(query);
            let id = fields.get("map");
            bfe_family::GameFamily::split_id(id).1.to_owned()
        } else { map };
        let files = DirFiles::from_resolved_root(root, family);
        let fp = fingerprint::hash_for_family(&files, family, query)?;
        let script = match family {
            GameFamily::Swbf1 => None,
            GameFamily::Swbf2 => swbf2_mission(&files, &map, query)?,
        };
        let mut admit_options = AdmitOptions { family, ..parse_admit_options(query, &map) };
        if family == bfe_family::GameFamily::Swbf2 {
            let fields = bfe_content::mission::Query::parse(query);
            admit_options.map.clone_from(&map);
            if !fields.get("mode").is_empty() { admit_options.mode = fields.get("mode").to_owned(); }
            // The modes the script host runs are admitted through their mission
            // selection, so a dedicated authority runs the same script, rosters
            // and scoring as a local one.
            if matches!(admit_options.mode.as_str(), "eli" | "ass") {
                return Self::load_scripted(&files, root, query, epoch, admit_options, script.as_deref(), fp, gate);
            }
        }
        let (defs, registry_descriptor, registry_content) =
            load_entity_registry(&files, &map, query, epoch, family)?;
        let context = Context::new();
        let world = match family {
            bfe_family::GameFamily::Swbf1 => context.world_network_admit(&files, &admit_options),
            family => context.world_network_admit_for_family(&files, family, &admit_options, script.as_deref()),
        };
        Self::start_admitted(root,query,map,epoch,context,HostAdmission {options:admit_options,world,defs,fingerprint:fp,
            descriptor:registry_descriptor,registry:registry_content,mission:None,gate})
    }

    /// The scripted 2005 battle of `query`, admitted from the files its digest
    /// covers: the mission the digest selected (`script`) must be the one whose
    /// scripts are admitted, so a client that verifies the digest has verified
    /// the mission the authority runs. It has no rotation of its own: the
    /// service schedules the next battle, and a restart loads this one again.
    #[allow(clippy::too_many_arguments)]
    fn load_scripted(files: &DirFiles, root: &Path, query: &str, epoch: u32, options: AdmitOptions, script: Option<&str>, fp: String, gate: bfe_family::Gate) -> Result<Self, String> {
        let fields = bfe_content::mission::Query::parse(query);
        let era = match fields.get("era") {
            "" => bfe_content::mission::era_of_side(&options.side).unwrap_or("cw"),
            era => era,
        };
        let selection = MissionSelection {
            family: GameFamily::Swbf2,
            map: options.map.clone(),
            era: era.to_owned(),
            mode: options.mode.clone(),
            seed: options.seed,
        };
        let context = Context::new();
        let (world, resources) = context
            .world_network_mission_content(files, &options, &selection, MissionPreferences::default())
            .map_err(|e| e.to_string())?;
        let admitted = bfe_content::mission::Query::parse(&resources.query);
        if script != Some(admitted.get("mission")) {
            return Err(format!(
                "the {} {} mission of {} is {}, but the battle's content digest covers {}",
                selection.era, selection.mode, selection.map, admitted.get("mission"), script.unwrap_or("none"),
            ));
        }
        let descriptor = world.registry.clone();
        let mission = MissionLaunch {
            selection,
            startup: MissionStartup::Checking,
            rotation: None,
            building_sources: resources.building_sources,
        };
        Self::start_admitted(root, query, options.map.clone(), epoch, context, HostAdmission {
            options, world: Ok(world), defs: resources.definitions, fingerprint: fp,
            descriptor, registry: resources.registry, mission: Some(mission), gate,
        })
    }

    /// Start a native dedicated authority for a resolved developer selection.
    pub fn developer_mission(root:&Path, options:&AdmitOptions, selection:&MissionSelection,
        preferences:MissionPreferences,epoch:u32)->Result<Self,String> {
        Self::developer_rotation(root, vec![DeveloperMission {
            options: options.clone(), selection: selection.clone(), preferences,
        }], epoch)
    }

    /// Start at the first usable slot. A failed setup spends that slot's one attempt.
    pub fn developer_rotation(root: &Path, slots: Vec<DeveloperMission>, mut epoch: u32) -> Result<Self, String> {
        if slots.is_empty() { return Err("empty mission rotation".into()); }
        let mut rotation = MissionRotation { next: 0, remaining: slots.len(), slots };
        let mut host = match rotation.admit(root, &mut epoch).map_err(|e| e.to_string())? {
            Some(host) => host,
            None => {
                // No admitted battle exists. The service can still answer status and join requests.
                let last = rotation.slots.last().unwrap();
                let mission = MissionLaunch { selection: last.selection.clone(), startup: MissionStartup::Parked(ROTATION_STOPPED),
                    rotation: None, building_sources: vec![] };
                let error = bfe_sim::abi::context::Error::new(bfe_sim::abi::context::ErrorKind::MissionSetup, ROTATION_STOPPED);
                let mut parked = Self::start_admitted(root, "", last.selection.map.clone(), epoch, Context::new(), HostAdmission {
                    options: last.options.clone(), world: Err(error), defs: Defs::default(), fingerprint: String::new(),
                    descriptor: Value::Null, registry: bfe_content::Value::Null, mission: Some(mission), gate: bfe_family::Gate::BUILT,
                })?;
                parked.ended = true;
                parked.winner = Some(0);
                parked
            }
        };
        host.mission.as_mut().unwrap().rotation = Some(rotation);
        Ok(host)
    }

    fn admit_scripted_slot(root: &Path, slot: &DeveloperMission, epoch: u32) -> Result<Self, bfe_sim::abi::context::Error> {
        use bfe_sim::abi::context::Error;
        let DeveloperMission { options, selection, preferences } = slot;
        let context = Context::new();
        let files = DirFiles::from_resolved_root(root, selection.family);
        let (world, resources) = context.world_network_mission_content(&files, options, selection, *preferences)?;
        let fingerprint = fingerprint::hash_for_family(&files, selection.family, &resources.query).map_err(Error::host)?;
        let descriptor = world.registry.clone();
        let mission = MissionLaunch { selection: selection.clone(),
            startup: MissionStartup::Checking, rotation: None, building_sources: resources.building_sources };
        Self::start_admitted(root, &resources.query, selection.map.clone(), epoch, context, HostAdmission {
            options: options.clone(), world: Ok(world), defs: resources.definitions, fingerprint,
            descriptor, registry: resources.registry, mission: Some(mission), gate: bfe_family::Gate::BUILT,
        }).map_err(Error::host)
    }

    pub fn mission_selection(&self) -> Option<&MissionSelection> {
        self.mission.as_ref().map(|m| &m.selection)
    }

    fn advance_scripted_rotation(&mut self, finished_playable_match: bool) -> Result<bool, String> {
        let mission = self.mission.as_mut().unwrap();
        if matches!(mission.startup, MissionStartup::Parked(_)) { return Ok(false); }
        let Some(mut rotation) = mission.rotation.take() else {
            // A service's battle: the service schedules what follows it.
            mission.startup = MissionStartup::Parked(MISSION_FAILED);
            return Ok(false);
        };
        if finished_playable_match { rotation.remaining = rotation.slots.len(); }
        // Park before attempting another admission. Even a non-script load error
        // leaves the worker quiescent on subsequent service ticks.
        mission.startup = MissionStartup::Parked("No mission is available.");
        self.world_admitted = false;
        let mut epoch = self.epoch;
        if rotation.remaining != 0 { epoch = epoch.checked_add(1).ok_or("battle epoch exhausted")?; }
        match rotation.admit(&self.content_root, &mut epoch) {
            Ok(Some(mut next)) => {
                next.mission.as_mut().unwrap().rotation = Some(rotation);
                next.last_now_ms = self.last_now_ms;
                next.set_state_budget(self.state_budget);
                *self = next;
                Ok(true)
            }
            Ok(None) => {
                self.mission.as_mut().unwrap().startup = MissionStartup::Parked(ROTATION_STOPPED);
                Ok(false)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn start_admitted(root:&Path, query:&str, map:String, epoch:u32, mut context:Context, admission:HostAdmission) -> Result<Self,String> {
        let HostAdmission {options:admit_options,world:admission,defs,fingerprint:fp,descriptor:mut registry_descriptor,registry:registry_content,mission,gate}=admission;
        let family = mission.as_ref().map_or(admit_options.family, |m| m.selection.family);
        let mut world_admitted = false;
        let mut wire_bounds = None;
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
        match admission {
            Ok(admitted) => {
                context
                    .world_step_init(admitted.setup)
                    .map_err(|e| e.to_string())?;
                admitted_handles = admitted.handles;
                wire_bounds = Some(admitted.wire_bounds);
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
                let (explosions, host_content) = authority_content(
                    &context, admitted_handles.store_source, &registry_content, &defs, mission.as_ref(),
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
                // `PUBLISH_EVERY_TICKS = 3` and `MAX_LOCAL_INTERVAL = 3`.
                let _ = context.invoke_export(
                    "wire_host_set",
                    &AdmitCall {
                        args: vec![host_handle as f64, 0.0, 0.0, 3.0],
                        ..Default::default()
                    },
                );

                world_admitted = true;
            }
            // A 2005 world is admitted or not loaded at all, except the parked host of a
            // rotation none of whose missions started: it answers status and refuses joins.
            Err(err) if family == GameFamily::Swbf2
                && !matches!(mission.as_ref().map(|m| &m.startup), Some(MissionStartup::Parked(_))) => return Err(err.to_string()),
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
        let (input_codes, input_actions) = input_tables(family);
        let mut code_str_ids = Vec::with_capacity(input_codes.len());
        for code in input_codes {
            code_str_ids.push(intern_str(code)?);
        }
        let mut action_str_ids = Vec::with_capacity(input_actions.len());
        for action in input_actions {
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
            family,
            context,
            defs,
            registry_descriptor,
            fingerprint: fp,
            mission,
            wire_bounds,
            query: query.to_string(),
            content_query: query.to_string(),
            map,
            epoch,
            tick: 0,
            dropped_ticks: 0,
            last_now_ms: 0.0,
            ended: false,
            winner: None,
            content_root: root.to_path_buf(),
            swbf2_classes: Vec::new(),
            gate,
            flight_ceiling: None,
            plans: HashMap::new(),
            roster: None,
            notices: Default::default(),
            weapon_ids: HashMap::new(),
            registry_content,
            match_seed: admit_options.seed,
            links: Vec::with_capacity(256),
            world_admitted,
            tick_input: first_tick(initial_units, &admit_options.bots),
            admitted_handles,
            admitted_player_id,
            publication_session,
            host_handle,
            state_budget: f64::INFINITY,
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
            census: std::env::var_os("BFE_AI_TRACE").map(|_| Default::default()),
            fire_secondary_str_id,
            recorded_publications: Vec::new(),
            recorded_cursor: 0,
        };

        // Initialize the admitted edge host to `(epoch, generation = 1)` (`EntityEdges.reset` in
        // `src/runtime/network-entity-edge-host.js`) so edges for generation 1 are accepted immediately.
        if host.world_admitted && host.admitted_handles.edges != 0 {
            host.reset_edges_host(host.admitted_handles.edges, epoch as f64, 1.0);
        }
        // The numbers the 2005 admission read from the content, kept for the
        // restart's check (a parked host has no world to read them from).
        if family == GameFamily::Swbf2 && host.world_admitted {
            host.swbf2_classes = host.context.owned(|w| bfe_sim::world::admit::sequence::swbf2_classes(w)).map_err(|e| e.to_string())?;
            // The ceiling its soldiers were admitted with: a finite binary32,
            // as a mission script supplies it, widened exactly.
            let ceiling = host.context.owned(|w| bfe_sim::world::admit::network::flight_ceiling(w)).map_err(|e| e.to_string())?
                .ok_or("the Battlefront II battle has no admitted world")?;
            if !flight_ceiling_readable(ceiling) {
                return Err(format!("the Battlefront II mission's flight ceiling {ceiling} is not a finite binary32 value"));
            }
            host.flight_ceiling = Some(ceiling);
            let registry = &host.registry_descriptor;
            host.roster = host.context.owned(|w| bfe_sim::wire::mission::Roster::of(w, registry)).map_err(|e| e.to_string())??;
            host.weapon_ids = registry.get("weapons").and_then(Value::as_array).into_iter().flatten()
                .filter_map(|row| Some((row[1].as_str()?.to_string(), u16::try_from(row[0].as_u64()?).ok()?)))
                .collect();
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

    /// Holds each client to `bytes_per_second` of state (config `stateBytesPerSecond`) the way the
    /// snapshot host does it: a client over budget skips whole snapshots, so every snapshot it does
    /// get can be decoded and acknowledged, and its own datagram still goes out every 100 ms.
    pub fn set_state_budget(&mut self, bytes_per_second: f64) {
        self.state_budget = bytes_per_second;
        if bytes_per_second.is_finite() {
            self.set_host_client_field(0, 20, bytes_per_second);
        }
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
        match self.family {
            GameFamily::Swbf1 => self.released_restart_finished_empty(),
            GameFamily::Swbf2 => self.swbf2_restart_finished_empty(),
        }
    }

    fn released_restart_finished_empty(&mut self) -> Result<bool, String> {
        if !self.ended || !self.native_by_client.is_empty() {
            return Ok(false);
        }
        let mut next = Self::load_for(
            &self.content_root,
            &self.content_query,
            self.epoch.checked_add(1).ok_or("battle epoch exhausted")?,
            GameFamily::Swbf1,
            self.gate,
        )?;
        next.query = self.query.clone();
        next.last_now_ms = self.last_now_ms;
        next.set_state_budget(self.state_budget);
        *self = next;
        Ok(true)
    }

    fn swbf2_restart_finished_empty(&mut self) -> Result<bool, String> {
        if !self.ended || !self.native_by_client.is_empty() {
            return Ok(false);
        }
        if let Some(mission) = &self.mission {
            // An explicit end can precede the first rules tick. Only a
            // reported startup failure spends the current pass's attempt.
            let playable = matches!(mission.startup, MissionStartup::Checking | MissionStartup::Playing);
            if mission.rotation.is_some() || !playable {
                return self.advance_scripted_rotation(playable);
            }
        }
        self.gate.require(self.family).map_err(|e| e.to_string())?;
        let mut next = Self::load_for(
            &self.content_root,
            &self.content_query,
            self.epoch.checked_add(1).ok_or("battle epoch exhausted")?,
            self.family,
            self.gate,
        )?;
        next.query = self.query.clone();
        // A 2005 battle restarts only as the battle its players were admitted
        // to: the reconstructed world is verified (content digest, position
        // box, registry and class numbers) before it replaces the accepted one.
        let registry = |value: &Value| {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() { object.remove("contentEpoch"); }
            value
        };
        if (next.fingerprint.as_str(), next.wire_bounds, next.world_admitted) != (self.fingerprint.as_str(), self.wire_bounds, self.world_admitted)
            || registry(&next.registry_descriptor) != registry(&self.registry_descriptor)
            || class_words(&next.swbf2_classes) != class_words(&self.swbf2_classes)
            || next.roster != self.roster
        {
            return Err("the battle's content changed: it cannot restart as the same battle".into());
        }
        next.last_now_ms = self.last_now_ms;
        next.set_state_budget(self.state_budget);
        *self = next;
        Ok(true)
    }

    /// Admit the resolved next rotation entry after the previous developer battle empties.
    pub fn rotate_developer_mission(
        &mut self,
        options: &AdmitOptions,
        selection: &MissionSelection,
        preferences: MissionPreferences,
    ) -> Result<bool, String> {
        if self.mission.is_none() || !self.ended || !self.native_by_client.is_empty() {
            return Ok(false);
        }
        *self = self.next_developer_mission(options, selection, preferences)?;
        Ok(true)
    }

    fn next_developer_mission(
        &self,
        options: &AdmitOptions,
        selection: &MissionSelection,
        preferences: MissionPreferences,
    ) -> Result<Self, String> {
        let epoch = self.epoch.checked_add(1).ok_or("battle epoch exhausted")?;
        let mut next = Self::developer_mission(&self.content_root, options, selection, preferences, epoch)?;
        next.last_now_ms = self.last_now_ms;
        next.set_state_budget(self.state_budget);
        Ok(next)
    }

    /// The registry a joining player is sent before his welcome: the released
    /// message, and for a scripted 2005 battle its mission's roster as a
    /// record beside it.
    pub fn registry_message(&self) -> Value {
        use bfe_sim::wire::{mission, revisions};
        let mut message = json!({"type": "entity-registry", "registry": self.registry_descriptor});
        if let (Some(roster), Some(fields)) = (&self.roster, message.as_object_mut()) {
            let record = revisions::record_message(revisions::REGISTRY, mission::REVISION, roster.body());
            fields.insert("records".to_string(), json!([record]));
        }
        message
    }

    /// Sends each player the notices that wait for him.
    pub fn send_notices(&mut self, send: impl FnMut(&str, &[u8]) -> bool) {
        self.notices.send(self.tick, send);
    }

    /// The welcome a player admitted to this battle is sent: the released
    /// welcome, and for a 2005 battle its flight ceiling (`flightCeiling`).
    pub fn welcome_message(&self, info: &Value, server_id: &str, player: &PlayerWelcome) -> Value {
        let mut welcome = build_welcome_message(info, server_id, player);
        if let (Some(ceiling), Some(fields)) = (self.flight_ceiling, welcome.as_object_mut()) {
            fields.insert("flightCeiling".to_string(), json!(ceiling));
        }
        welcome
    }

    /// Whether the admitted battle carries a 2005 Space Assault battlefield.
    pub fn has_space(&self) -> bool {
        self.world_admitted
            && self
                .context
                .owned(|w| w.space().is_some())
                .unwrap_or(false)
    }

    /// A 2005 battle's flight ceiling in metres; none for a 2004 battle.
    pub fn flight_ceiling(&self) -> Option<f64> {
        self.flight_ceiling
    }

    /// The movement numbers this 2005 battle's admission read from its
    /// content, by class name; empty for a 2004 battle.
    pub fn swbf2_classes(&self) -> &[(String, bfe_sim::movement::soldier::swbf2::Tuning)] {
        &self.swbf2_classes
    }

    /// Chat routing reads the player's current authority team, including a
    /// switch while bodiless. The join welcome is only an initial assignment.
    pub fn player_team(&self, peer_id: &str) -> Option<f64> {
        let native = self.native_by_client.get(peer_id)?;
        let player = self.player_by_native.get(native)?;
        self.context.world_network_team(*player).ok().flatten()
    }

    /// Admits a player into the world roster (`world.addPlayer` and `entityHost.addClient` in
    /// `handle("join")`).
    pub fn add_player(
        &mut self,
        peer_id: &str,
        name: &str,
        spectator: bool,
        auto_spawn: bool,
        team_select: bool,
    ) -> Result<PlayerWelcome, String> {
        match self.family {
            GameFamily::Swbf1 => self.released_add_player(peer_id, name, spectator, auto_spawn, team_select),
            GameFamily::Swbf2 => self.swbf2_add_player(peer_id, name, spectator, auto_spawn, team_select),
        }
    }

    // A 2004 battle's join, as released.
    fn released_add_player(
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
                &DirFiles::new(&self.content_root, GameFamily::Swbf1),
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

    // A 2005 battle's join: it checks the publication bind and gives back the
    // mailbox, player, edges bank and host client when any step refuses.
    fn swbf2_add_player(
        &mut self,
        peer_id: &str,
        name: &str,
        spectator: bool,
        auto_spawn: bool,
        team_select: bool,
    ) -> Result<PlayerWelcome, String> {
        if let Some(MissionStartup::Parked(reason)) = self.mission.as_ref().map(|m| m.startup) {
            return Err(reason.into());
        }
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
                &DirFiles::from_resolved_root(&self.content_root, self.family),
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
            // Any error after world_network_join rolls back the mailbox,
            // admitted player, edges bank and host client registration.
            let mut edges_id = 0u32;
            let mut host_added = false;
            let bound = (|| -> Result<u32, String> {
                self.context
                    .world_host_random(self.match_seed, (!spectator).then_some(player.unit_id))
                    .map_err(|e| e.to_string())?;
                let (explosions, content) = authority_content(
                    &self.context,
                    self.admitted_handles.store_source,
                    &self.registry_content,
                    &self.defs,
                    self.mission.as_ref(),
                )?;
                self.context
                    .world_host_content(explosions, content)
                    .map_err(|e| e.to_string())?;
                edges_id = match self
                    .context
                    .invoke_export("wire_edges_create", &AdmitCall::default())
                    .map_err(|e| e.to_string())?
                {
                    AdmitReturned::Scalar(v) if v != 0. => v as u32,
                    _ => return Err("entity edges refused".into()),
                };
                self.reset_edges_host(edges_id, self.epoch as f64, player.generation as f64);
                self.run_host_op(0, native_id, self.last_now_ms, 0.0);
                host_added = true;
                match self
                    .context
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
                    .map_err(|e| e.to_string())?
                {
                    AdmitReturned::Scalar(v) if v != 0. => {}
                    _ => return Err("publication bind refused".into()),
                }
                self.set_host_client_field(native_id, 2, 1.0);
                self.set_host_client_field(native_id, 3, 1.0);
                self.set_host_client_field(native_id, 6, self.last_now_ms);
                self.context
                    .world_host_client(player.player, edges_id)
                    .map_err(|e| e.to_string())?;
                Ok(edges_id)
            })();
            let edges_id = match bound {
                Ok(edges_id) => edges_id,
                Err(error) => {
                    if host_added {
                        self.run_host_op(1, native_id, 0.0, 0.0);
                    }
                    if edges_id != 0 {
                        let _ = self.context.world_host_client_leave(edges_id);
                        let _ = self.context.invoke_export(
                            "wire_edges_drop",
                            &AdmitCall {
                                args: vec![edges_id as f64],
                                ..Default::default()
                            },
                        );
                    }
                    let _ = self.context.world_network_leave(native_id, player.player);
                    let _ = self.context.invoke_export(
                        "input_mailbox_drop",
                        &AdmitCall {
                            args: vec![mailbox_id as f64],
                            ..Default::default()
                        },
                    );
                    return Err(error);
                }
            };
            team = player.team as u8;
            unit_id = player.unit_id;
            generation = player.generation as u32;
            default_class = player.class_name;
            self.mailbox_by_native.insert(native_id, mailbox_id);
            self.player_by_native.insert(native_id, player.player);
            self.edges_by_native.insert(native_id, edges_id);
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

    /// Removes a disconnected player from the world roster (`leave(client)`).
    pub fn remove_player(&mut self, peer_id: &str) {
        self.plans.remove(peer_id);
        self.notices.leave(peer_id);
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
    /// and `setStarted` / `setReady`).
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
    /// (`handle("resync")` and `Host::resync` in `rust/sim/src/wire/host.rs`).
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
    /// from `peer_id` (`input(id, bytes)`).
    ///
    /// Why malformed datagrams never disconnect or stall other players:
    /// All wire validation occurs in `decode_snapshot_ack_keepalive` / `decode_entity_input` before
    /// touching `self.context`, and any per-command refusal is isolated to that command entry.
    /// A joined 2005 client's plan of the match, from its admission; none
    /// for a 2004 battle.
    pub fn set_plan(&mut self, peer_id: &str, plan: Option<bfe_sim::wire::revisions::Plan>) {
        use bfe_sim::wire::revisions::{Use, NOTICE};
        if plan.as_ref().is_some_and(|plan| matches!(plan.of(NOTICE), Use::Revision(_))) {
            let team = self.player_team(peer_id).map(|team| team as u8);
            self.notices.join(peer_id, self.tick, team);
        }
        match plan {
            Some(plan) => { self.plans.insert(peer_id.to_string(), plan); }
            None => { self.plans.remove(peer_id); }
        }
    }

    pub fn handle_input(&mut self, peer_id: &str, bytes: &[u8], now_ms: f64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(&native_id) = self.native_by_client.get(peer_id) else {
            return;
        };
        if !self.world_admitted {
            return;
        }
        self.set_host_client_field(native_id, 6, self.last_now_ms);

        // 10-byte `BNA1` snapshot acknowledgement keepalive (`isSnapshotAckKeepalive`).
        if bytes.len() == 10 {
            if let Ok((ack_epoch, ack_tick)) = decode_snapshot_ack_keepalive(bytes) {
                self.run_host_op(4, native_id, ack_tick as f64, ack_epoch as f64);
                return;
            }
        }

        // `BNI1` entity input packet (`decodeEntityInput`), with the battle's input tables
        // and, in a 2005 match, the client's plan.
        let Ok(decoded) = decode_entity_input_with_records(bytes, self.family, self.plans.get(peer_id)) else {
            return;
        };
        let pilot_entries = decoded.pilot_entries();
        let pkt = &decoded.packet;
        if let Some((ack_epoch, ack_tick)) = pkt.snapshot_ack {
            self.run_host_op(4, native_id, ack_tick as f64, ack_epoch as f64);
        }
        if pkt.epoch != self.epoch {
            return;
        }

        // Forward discrete gameplay edges (`pkt.edges`) into the client's `wire_edges` receiver
        // (`state.edges.receive(packet, game.worldClock.tick)`).
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
        // (`world.input(id, command)` and `Native::submit` in
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
                if accepted {
                    if let Some(pilot) = pilot_entries.iter().find(|e| e.sequence == cmd.sequence) {
                        // The controls ride only for the life the command was
                        // stamped for: an old life's never reach the next.
                        let _ = self.context.owned(|w| {
                            if w.space_player_unit(player_handle).is_some_and(|u| u.life == cmd.generation as f64) {
                                w.queue_space_pilot_input(player_handle, cmd.sequence as f64, pilot.controls);
                            }
                        });
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

    // Queues what the mission announced on this tick for its players.
    fn announce(&mut self) {
        let mut cues = Vec::new();
        let (feed, weapons) = (&mut self.notices.feed, &self.weapon_ids);
        let collected = self.context.owned(|w| {
            feed.collect(w, |weapon| weapons.get(weapon).copied().unwrap_or(0), |team, cue| cues.push((team, cue)));
        });
        if collected.is_err() || cues.is_empty() {
            return;
        }
        let teams: HashMap<String, u8> = self.native_by_client.keys()
            .filter_map(|peer| Some((peer.clone(), self.player_team(peer)? as u8)))
            .collect();
        self.notices.queue(self.tick, &cues, |peer| teams.get(peer).copied());
    }

    /// Steps the authoritative simulation by one 60 Hz tick and returns the decoded publication
    /// (`step()` and `publish()`).
    pub fn step(
        &mut self,
        now_ms: f64,
        unix_ms: u64,
    ) -> Result<Option<DecodedPublication>, String> {
        match self.family {
            GameFamily::Swbf1 => self.released_step(now_ms, unix_ms),
            GameFamily::Swbf2 => self.swbf2_step(now_ms, unix_ms),
        }
    }

    fn released_step(
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
            if let Some(census) = &mut self.census {
                if let Ok(Some(line)) = census.sample(&self.context, self.tick as u64, 600) {
                    for line in line.lines() { eprintln!("ai census server {line}"); }
                }
            }
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

    fn swbf2_step(
        &mut self,
        now_ms: f64,
        unix_ms: u64,
    ) -> Result<Option<DecodedPublication>, String> {
        if let Some(startup) = self.mission.as_ref().map(|m| m.startup) {
            match startup {
                MissionStartup::Advance if !self.advance_scripted_rotation(false)? => return Ok(None),
                MissionStartup::Parked(_) => return Ok(None),
                _ => {}
            }
        }
        self.last_now_ms = self.last_now_ms.max(now_ms);
        self.tick = if self.publication_session != 0 {
            self.tick_input.clock.tick as u32 + 1
        } else {
            self.tick + 1
        };
        self.tick_input.tick = self.tick as f64;
        self.tick_input.at = self.last_now_ms;

        if self.publication_session != 0 {
            self.tick_input.units = self.context.world_prepare_units().map_err(|e| e.to_string())?;
            let output = self
                .context
                .world_tick(self.tick_input.clone())
                .map_err(|e| e.to_string())?;
            if let Some(census) = &mut self.census {
                if let Ok(Some(line)) = census.sample(&self.context, self.tick as u64, 600) {
                    for line in line.lines() { eprintln!("ai census server {line}"); }
                }
            }
            self.tick_input.clock.tick = output.clock.tick;
            self.tick_input.clock.elapsed = output.clock.elapsed;
            self.tick_input.clock.compensation = output.clock.compensation;
            self.tick_input.clock.debt = output.clock.debt;
            self.tick_input.clock.alpha = output.clock.alpha;
            self.tick_input.clock.overloads = output.clock.overloads;
            self.tick_input.clock.dropped = output.clock.dropped;
            self.tick_input.clock.step_cost = output.clock.step_cost;
            if !output.accepted {
                return Err(format!(
                    "native world refused authoritative tick {} (clock {}, foreign rows {})",
                    self.tick_input.tick, output.clock.tick, self.tick_input.units,
                ));
            }

            if let Some(mission) = self.mission.as_mut().filter(|m| m.startup == MissionStartup::Checking) {
                match self.context.world_mission_started() {
                    Ok(true) => mission.startup = MissionStartup::Playing,
                    Ok(false) => {}
                    Err(e) if e.kind() == bfe_sim::abi::context::ErrorKind::MissionSetup => {
                        self.ended = true;
                        self.winner = Some(0);
                        mission.startup = MissionStartup::Advance;
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            if self.roster.is_some() {
                self.announce();
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
    /// (`game.dispose()`), ensuring `Context::world_replay_live_handles()`
    /// returns to `0` across sequential battle loads, map rotations, and server teardown.
    fn drop(&mut self) {
        let _ = self.context.dispose();
    }
}

#[cfg(test)]
mod friendly_fire_tests {
    #[test]
    fn the_battle_query_carries_the_friendly_fire_rule_into_admission() {
        assert!(super::parse_admit_options("?map=nab2&side=rep&ai=classic&seed=1", "nab2").bots.friendly_fire);
        let off = crate::config::with_friendly_fire("?map=nab2&side=rep&ai=classic&seed=1", false);
        assert!(!super::parse_admit_options(&off, "nab2").bots.friendly_fire);
    }
}

#[cfg(test)]
mod family_tests {
    use super::{parse_admit_options, NativeWorldHost};
    use crate::config::{query_for_rotation, rotation_family};
    use bfe_family::GameFamily;

    // The worker reads a 2004 battle query as the released worker did: raw
    // key and value pairs, later ones win, nothing decoded, no family read.
    #[test]
    fn a_2004_query_is_read_by_the_released_rules() {
        for (entry, map, side, admitted) in [
            ("swbf1/nab2:imp&side=rep:duel", "nab2", "rep", "?map=nab2&side=rep&era=cw&ai=enhanced&seed=1"),
            ("swbf1/tat2:all&family=swbf2:duel", "tat2", "all", "?map=tat2&side=all&era=gcw&ai=enhanced&seed=1"),
            ("swbf1/nab2:r%65p:conquest", "nab2", "r%65p", "?map=nab2&side=r%65p&era=gcw&ai=enhanced&seed=1"),
            ("swbf1/hot1:imp&map=tat2:duel", "tat2", "imp", "?map=tat2&side=imp&era=gcw&ai=enhanced&seed=1"),
            ("SWBF1/NAB2:REP:CONQUEST", "SWBF1/NAB2", "REP", "?map=SWBF1/NAB2&side=REP&era=gcw&ai=enhanced&seed=1"),
        ] {
            assert_eq!(rotation_family(entry), GameFamily::Swbf1);
            let query = query_for_rotation(entry, "enhanced").unwrap();
            let options = parse_admit_options(&query, "nab2");
            assert_eq!((options.family, options.map.as_str(), options.side.as_str()), (GameFamily::Swbf1, map, side), "{entry}");
            assert_eq!(options.query(), admitted, "{entry}");
        }
    }

    #[test]
    fn a_game_this_build_cannot_play_is_refused_before_content() {
        let query = query_for_rotation("swbf2/kam9:gcw:hunt", "enhanced").unwrap();
        let missing = std::path::Path::new("/nonexistent/content");
        let refused = NativeWorldHost::load_battle(missing, GameFamily::Swbf2, &query, 1, bfe_family::Gate::BUILT).err();
        assert_eq!(refused.as_deref(), Some("Battlefront II (2005) is not available in this build"));
        // The same root with a 2004 family fails on its content, not on its game.
        let missing_content = NativeWorldHost::load_battle(missing, GameFamily::Swbf1, "?map=nab2&side=rep", 1, bfe_family::Gate::BUILT).err();
        assert!(missing_content.is_some_and(|e| !e.contains("not available")));
    }
}

#[cfg(test)]
mod fault_tests {
    use super::FaultWatch;

    #[test]
    fn a_player_whose_updates_fail_for_a_second_is_ended_once() {
        let mut watch = FaultWatch::default();
        let fault = |peer: &str| vec![(Some(peer.to_string()), "unknown post".to_string())];
        assert!(watch.note(&fault("a"), 0).is_empty());
        assert!(watch.note(&fault("a"), 950).is_empty());
        // A clean publication starts the second again.
        assert!(watch.note(&[], 1000).is_empty());
        assert!(watch.note(&fault("a"), 1050).is_empty());
        assert!(watch.note(&[(None, "world".to_string())], 1100).is_empty());
        assert!(watch.note(&fault("a"), 1200).is_empty());
        assert_eq!(watch.note(&fault("a"), 2200), vec![("a".to_string(), "unknown post".to_string())]);
        // Another player is not ended with it.
        assert!(watch.note(&fault("b"), 2300).is_empty());
    }
}

#[cfg(test)]
mod mission_tests {
    use super::*;
    #[test]
    #[ignore = "requires the prepared mission_setup_fixture.py test copy"]
    fn failed_startup_advances_without_an_operator_and_stops_after_one_pass() {
        let root = std::path::PathBuf::from(std::env::var_os("BFE_SETUP_TEST_CONTENT").unwrap());
        // A 2005 Hero Assault: the released 2004 parser answers the 2004 family and mode.
        let options = AdmitOptions { family: bfe_family::GameFamily::Swbf2, mode: "eli".into(),
            ..parse_admit_options("?map=tat2&side=all&ai=classic&seed=59", "tat2") };
        let selection = MissionSelection { family: bfe_content::GameFamily::Swbf2, map: options.map.clone(),
            era: "gcw".into(), mode: "eli".into(), seed: options.seed };
        let slot = |count| DeveloperMission { options: options.clone(), selection: selection.clone(),
            preferences: MissionPreferences { bots_per_team: Some(count), assault_target: None, direct_spawn: false } };
        let mut host = NativeWorldHost::developer_rotation(&root, vec![slot(1), slot(2), slot(3)], 17).unwrap();
        assert_eq!(host.epoch, 18);
        host.step(17.0, 17).unwrap();
        assert!(host.ended && host.winner == Some(0));
        host.step(34.0, 34).unwrap();
        assert_eq!(host.epoch, 19);
        assert!(!host.ended && host.context.world_mission_started().unwrap());

        let mut host = NativeWorldHost::developer_rotation(&root, vec![slot(2), slot(1)], 40).unwrap();
        host.step(17.0, 17).unwrap();
        assert_eq!((host.ended, host.winner), (true, Some(0)));
        for n in 2_u32..=128 {
            assert!(host.step(f64::from(n) * 17.0, u64::from(n) * 17).unwrap().is_none());
            assert!(!host.restart_finished_empty().unwrap());
        }
        assert_eq!((host.epoch, host.tick), (40, 1));
        assert!(!host.world_admitted);
        assert_eq!(host.add_player("late", "Late", false, false, true).err().as_deref(), Some(ROTATION_STOPPED));

        let mut parked = NativeWorldHost::developer_rotation(&root, vec![slot(1), slot(1)], 1).unwrap();
        for tick in 1..=128 {
            assert!(parked.step(tick as f64, tick).unwrap().is_none());
            assert!(!parked.restart_finished_empty().unwrap());
        }
        assert_eq!((parked.epoch, parked.tick, parked.ended, parked.winner), (2, 0, true, Some(0)));
        assert!(!parked.world_admitted);
        assert_eq!(parked.add_player("late", "Late", false, false, true).err().as_deref(), Some(ROTATION_STOPPED));

        let mut host = NativeWorldHost::developer_rotation(&root, vec![slot(3)], 60).unwrap();
        assert_eq!(host.tick, 0);
        host.ended = true;
        assert!(host.restart_finished_empty().unwrap());
        assert_eq!((host.epoch, host.tick), (61, 0));
    }

    #[test]
    #[ignore = "requires prepared 2005 missions"]
    fn developer_dedicated_missions_publish_and_restart() {
        use bfe_sim::world::admit::Bots;
        let root=std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
        for (map,era,mode,side) in [("tat2","gcw","eli","all"),("spa6","cw","ass","rep")] {
            let options=AdmitOptions{family:bfe_family::GameFamily::Swbf2,mode:mode.into(),practice_target:None,map:map.into(),seed:59,side:side.into(),unit:None,enhanced:false,heroes:false,name:None,bots:Bots::read("classic","medium","full")};
            let selection=MissionSelection{family:bfe_content::GameFamily::Swbf2,map:map.into(),era:era.into(),mode:mode.into(),seed:options.seed};
            let mut host=NativeWorldHost::developer_mission(&root,&options,&selection,MissionPreferences{bots_per_team:Some(3),assault_target:None,direct_spawn:false},1).unwrap();
            assert!(host.world_admitted && host.publication_session!=0);
            // A server battle runs the same per-life soldier step and hero powers as a local one.
            assert!(host.context.owned(|w| bfe_sim::combat::powers::powers_in(w).is_some()).unwrap(), "{map}: no hero powers");
            let agreed = fingerprint::hash_for_family(&DirFiles::from_resolved_root(&root, selection.family), selection.family, &host.query).unwrap();
            assert!(agreed.starts_with("bfe-content-sha256-"));
            assert_eq!(host.fingerprint, agreed);
            let peer = "a".repeat(64);
            let welcome = host.add_player(&peer, "Pilot", false, true, false).unwrap();
            assert_eq!(host.player_team(&peer), Some(f64::from(welcome.team)));
            assert!(host.context.owned(|w| {
                let setup = &w.mission_state().unwrap().0.host().setup;
                setup.teams[&i32::from(welcome.team)].classes.iter()
                    .any(|class| class.class_name == welcome.class_name)
            }).unwrap());
            for tick in 1..=30 { host.step(tick as f64*1000.0/60.0,719_000+tick*17).unwrap(); }
            assert_eq!(host.tick,30);
            assert!(host.context.owned(|w|w.mission_state().is_some()).unwrap());
            println!("Dedicated {map} ticks={} admission={}",host.tick,host.fingerprint);
            host.remove_player(&peer);
            assert_eq!(host.player_team(&peer), None);
            let fp=host.fingerprint.clone();
            host.ended=true;
            assert!(host.restart_finished_empty().unwrap());
            assert!(host.world_admitted && host.epoch==2 && host.fingerprint==fp);
            let mut other = selection.clone();
            let mut options = options.clone();
            (other.map, other.era, other.mode, options.side) = if map == "tat2" {
                ("spa6".into(), "cw".into(), "ass".into(), "cis".into())
            } else {
                ("tat2".into(), "gcw".into(), "eli".into(), "imp".into())
            };
            // The options name the battle the rotation goes to: its map and its mode.
            (options.map, options.mode) = (other.map.clone(), other.mode.clone());
            let prefs = MissionPreferences { bots_per_team: Some(3), assault_target: None, direct_spawn: false };
            assert!(!host.rotate_developer_mission(&options, &other, prefs).unwrap());
            host.ended = true;
            let mut invalid = other.clone();
            invalid.mode = "absent".into();
            assert!(host.rotate_developer_mission(&options, &invalid, prefs).is_err());
            assert_eq!(host.epoch, 2);
            assert!(host.rotate_developer_mission(&options, &other, prefs).unwrap());
            assert_eq!(host.epoch, 3);
            assert_eq!(host.mission.as_ref().unwrap().selection, other);
            assert_ne!(host.fingerprint, fp);
            println!("Rotated {map} to {} epoch={}", other.map, host.epoch);
        }
    }
}
