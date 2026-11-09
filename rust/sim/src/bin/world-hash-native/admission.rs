//! The world's source, one function with two implementations
//! (`admit_world`): the native cold admission `Context::world_admit` once
//! `bfe_sim::world::admit::provided()` says it exists, and until then the
//! recorded admission call stream (`tools/world-hash.mjs --record-calls`,
//! one gzip JSON line a call) replayed through `Context::invoke_export`
//! exactly as rust/sim/src/bin/admit-replay.rs does: the host's memory
//! writes between exports become the calls' scratch bytes and the module
//! addresses the host passed back become the pool keys the native bodies
//! answered. The replay stops at the recording's checkpoint (welcome
//! published, before the first tick), which is where the Node tool's
//! battle loop begins.
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use bfe_content::files::DirFiles;
use bfe_sim::abi::context::{Context, ErrorKind};
use bfe_sim::abi::context_step::bank;
use bfe_sim::gen::{
    AiJournalEvent, AiPhaseBoundary, AiPhaseCamera, AiPhaseHeader, F2Phase, F3Acknowledgement,
    F3AimHeader, F3Boundary, F3ForeignHeader, F3RandomGlobals, F3Separation, F4LiveInput, F4Phase,
    F4TailInput, WirePublicationLink, WireWorldFacts,
};

/// A host-written input bank by its module address: the page wrote these
/// through the descriptors' pointers, so the recording holds them only as
/// memory writes. Natively the owners read their own banks, which the
/// recorded writes must reach too (`bank:NAME`), byte for byte.
struct Region {
    bank: &'static str,
    at: u64,
    bytes: usize,
    /// A bank of bytes (the kernels' flags) rather than of f64 words.
    u8: bool,
}

/// The input banks a descriptor names, with their sizes.
fn regions_of(export: &str, row: &[f64], f2_bank_bytes: usize) -> Vec<Region> {
    let region = |bank, index: usize, bytes: usize| Region {
        bank,
        at: row.get(index).copied().unwrap_or(0.0) as u64,
        bytes,
        u8: false,
    };
    match export {
        "f3_init" if row.len() >= F3Boundary::WORDS => {
            let states = row[F3Boundary::RANDOM_CAPACITY] as usize
                * row[F3Boundary::RANDOM_WORDS] as usize
                * 8;
            vec![
                region(
                    "F3_ACKNOWLEDGEMENT",
                    F3Boundary::ACKNOWLEDGEMENT,
                    F3Acknowledgement::WORDS * 8,
                ),
                region("F3_RANDOM_STATES", F3Boundary::RANDOM_STATES, states),
                region(
                    "F3_RANDOM_GLOBALS",
                    F3Boundary::RANDOM_GLOBALS,
                    F3RandomGlobals::WORDS * 8,
                ),
                region(
                    "F3_SEPARATION",
                    F3Boundary::SEPARATION,
                    F3Separation::WORDS * 8,
                ),
                region(
                    "F3_FOREIGN_HEADER",
                    F3Boundary::FOREIGN_HEADER,
                    F3ForeignHeader::WORDS * 8,
                ),
                region(
                    "F3_AIM_HEADER",
                    F3Boundary::AIM_HEADER,
                    F3AimHeader::WORDS * 8,
                ),
            ]
        }
        "f4_live_init" if row.len() >= F4Phase::WORDS => vec![
            region("F4_LIVE_INPUT", F4Phase::LIVE_INPUT, F4LiveInput::WORDS * 8),
            region("F4_TAIL_INPUT", F4Phase::TAIL_INPUT, F4TailInput::WORDS * 8),
        ],
        "ai_world_boundary" if row.len() >= AiPhaseBoundary::WORDS => vec![
            region(
                "AI_HEADER",
                AiPhaseBoundary::HEADER,
                AiPhaseHeader::WORDS * 8,
            ),
            region(
                "AI_CAMERAS",
                AiPhaseBoundary::CAMERAS,
                row[AiPhaseBoundary::CAMERA_CAPACITY] as usize * AiPhaseCamera::WORDS * 8,
            ),
            region(
                "AI_JOURNAL",
                AiPhaseBoundary::JOURNAL,
                row[AiPhaseBoundary::JOURNAL_CAPACITY] as usize * AiJournalEvent::WORDS * 8,
            ),
        ],
        "f2_init" if row.len() >= F2Phase::WORDS => {
            vec![region("F2_INPUT", F2Phase::INPUT, f2_bank_bytes)]
        }
        _ => Vec::new(),
    }
}

/// Each recorded write that touches a known bank reaches the native bank:
/// the bank's current words, patched over the overlap, written back.
fn mirror_writes(
    context: &Context,
    memory: &Memory,
    regions: &[Region],
    writes: &[(u64, usize)],
) -> Result<(), String> {
    for region in regions {
        if region.at == 0 || region.bytes == 0 {
            continue;
        }
        let end = region.at + region.bytes as u64;
        if !writes
            .iter()
            .any(|(at, len)| *at < end && at + *len as u64 > region.at)
        {
            continue;
        }
        let name = format!("bank:{}", region.bank);
        let mut bytes: Vec<u8> = match context.invoke_export(&name, &Call::default()) {
            Ok(Returned::Row(words)) if !region.u8 => {
                words.iter().flat_map(|w| w.to_le_bytes()).collect()
            }
            Ok(Returned::Bytes(bytes)) if region.u8 => bytes,
            _ => continue,
        };
        for (at, len) in writes {
            let from = (*at).max(region.at);
            let to = (at + *len as u64)
                .min(end)
                .min(region.at + bytes.len() as u64);
            if from >= to {
                continue;
            }
            let patch = memory.read(from, (to - from) as usize);
            bytes[(from - region.at) as usize..(to - region.at) as usize].copy_from_slice(&patch);
        }
        let call = Call {
            scratch: bytes,
            ..Default::default()
        };
        if let Err(e) = context.invoke_export(&name, &call) {
            return Err(format!("mirroring {}: {}", region.bank, e.message()));
        }
    }
    Ok(())
}
use bfe_sim::world::admit::{Call, Options, Returned};
use serde_json::Value;

/// The runtime handles the host keeps after admission: the pool keys of the
/// publisher, store source, snapshot host, wire observers, the local
/// player's mailbox, edge runtime and player.
#[derive(Clone, Copy, Debug, Default)]
pub struct Handles {
    pub publication: u32,
    pub store_source: u32,
    pub host: u32,
    pub combat: u32,
    pub presentation: u32,
    pub mailbox: u32,
    pub edges: u32,
    pub player: u32,
}

/// What the row's host knows once the world is admitted.
#[derive(Clone, Debug, Default)]
pub struct Admitted {
    pub handles: Handles,
}

/// `fs::read` of a `.gz` file inflated (the recorder streams gzip members,
/// so every member is read), or a plain file.
pub fn read_maybe_gzip(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let text = if path.extension().and_then(|s| s.to_str()) == Some("gz") {
        let mut out = String::new();
        flate2::read::MultiGzDecoder::new(&bytes[..])
            .read_to_string(&mut out)
            .map_err(|e| format!("gzip {}: {e}", path.display()))?;
        out
    } else {
        String::from_utf8(bytes).map_err(|e| format!("invalid utf8 in {}: {e}", path.display()))?
    };
    Ok(text)
}

pub fn decode64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap_or_default()
}

fn bits(text: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(text, 16).unwrap_or(0))
}

fn words(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).map(bits).collect())
        .unwrap_or_default()
}

/// The world, admitted natively when the native admission exists and from
/// the row's recording until then.
pub fn admit_world(
    context: &mut Context,
    label: &str,
    options: &Options,
    admissions_dir: &Path,
    content_dir: &Path,
) -> Result<Handles, String> {
    if bfe_sim::world::admit::provided() {
        eprintln!("{label}: world source: native admission");
        let files = DirFiles::new(content_dir.to_path_buf());
        let admitted = context
            .world_admit(&files, options)
            .map_err(|e| format!("world_admit failed: {}", e.message()))?;
        let h = admitted.handles;
        return Ok(Handles {
            publication: h.publication,
            store_source: h.store_source,
            host: h.host,
            combat: h.combat,
            presentation: h.presentation,
            mailbox: h.mailbox,
            edges: h.edges,
            player: h.player,
        });
    }
    replay_recorded(context, label, admissions_dir, 0) // matrix-b
}

const HANDLES: &[&str] = &[
    "client_prediction_create",
    "wire_budget_create",
    "wire_combat_create",
    "wire_cosmetics_create",
    "wire_create",
    "wire_edges_create",
    "wire_host_create",
    "wire_presentation_create",
    "wire_publish_create",
    "wire_semantic_create",
    "wire_shots_create",
    "wire_stagger_create",
    "wire_store_source_create",
];

const READS_SCRATCH: &[&str] = &[
    "camera_init",
    "camera_add_definition",
    "body_construct",
    "sim_str_intern",
];

const READS_IO: &[&str] = &[
    "grid_new",
    "body_set",
    "world_candidates",
    "world_cast",
    "world_cast_moving",
    "grid_ray",
    "tree_ray",
    "terrain_near_ray",
    "gather_begin",
    "gather_end",
    "overlap_begin",
    "overlap_end",
    "push_walls_ray",
];

const FILLS: &[&str] = &[
    "body_aim_ptr",
    "body_preview_frame_ptr",
    "body_preview_looks_ptr",
    "camera_commands_ptr",
    "camera_flags_ptr",
    "camera_input_ptr",
    "candidates_ptr",
    "clip_data",
    "clip_tracks",
    "enabled_ptr",
    "f4_tail_commands",
    "grid_cells",
    "grid_index",
    "grid_start",
    "input_reserve",
    "io_ptr",
    "movement_flags_ptr",
    "movement_io_ptr",
    "places_ptr",
    "pose_construction_input",
    "pose_out",
    "pose_q",
    "pose_t",
    "pose_w",
    "positions_at",
    "set_bounds",
    "set_polygons",
    "sim_call_room",
    "sim_import_result_room",
    "sim_import_room",
    "sim_scratch",
    "sound_info",
    "sound_input",
    "tree_index",
    "tree_indirect",
    "tree_polygons",
    "tree_root",
    "veh_bytes",
    "veh_flags",
    "veh_input",
    "wire_cosmetics_payload",
    "wire_cosmetics_room",
    "wire_presentation_room",
    "wire_semantic_room",
    "wire_store_world_input",
    "world_order",
    "world_step_init",
];

const KEPT_BUFFERS: &[&str] = &["pose_construction_input", "veh_bytes", "veh_input"];
const VIEW_BANKS: &[&str] = &[
    "camera_input_ptr",
    "camera_commands_ptr",
    "camera_output_ptr",
    "camera_flags_ptr",
    "io_ptr",
    "enabled_ptr",
    "candidates_ptr",
    "places_ptr",
    "movement_io_ptr",
    "movement_flags_ptr",
    "body_aim_ptr",
    "body_preview_frame_ptr",
    "body_preview_looks_ptr",
    "f4_tail_commands",
    "wire_store_world_input",
    "input_reserve",
];
const IO_WORDS: usize = 64;
const MAX_BODIES: usize = 1 << 16;

/// The host's memory as the writes describe it: offset to bytes, newest last.
#[derive(Default)]
struct Memory(BTreeMap<u64, Vec<u8>>);
impl Memory {
    fn write(&mut self, offset: u64, bytes: Vec<u8>) {
        let end = offset + bytes.len() as u64;
        let overlapping: Vec<u64> = self
            .0
            .range(..end)
            .rev()
            .take_while(|(o, b)| **o + b.len() as u64 > offset || **o >= offset)
            .map(|(o, _)| *o)
            .collect();
        for o in overlapping {
            let b = self.0.remove(&o).unwrap();
            let bend = o + b.len() as u64;
            if o < offset {
                self.0.insert(o, b[..(offset - o) as usize].to_vec());
            }
            if bend > end {
                self.0.insert(end, b[(end - o) as usize..].to_vec());
            }
        }
        self.0.insert(offset, bytes);
    }

    /// Fleet::veh_input only grows its retained Vec. A collider's smaller
    /// request must keep pose words that a later sparse pull may not rewrite.
    /// The other lifecycle buffers resize to exactly the requested length.
    fn keep(&mut self, at: u64, len: usize, previous: Option<(u64, usize)>, grows: bool) -> usize {
        if let Some((old_at, old_len)) = previous {
            if old_at != at {
                let moved = self.read(old_at, old_len.min(len));
                self.write(at, moved);
            }
            if len > old_len {
                self.write(at + old_len as u64, vec![0; len - old_len]);
            }
            if grows {
                len.max(old_len)
            } else {
                len
            }
        } else {
            self.write(at, vec![0; len]);
            len
        }
    }

    fn read(&self, offset: u64, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        let end = offset + len as u64;
        for (o, b) in self.0.range(..end).rev() {
            let bend = *o + b.len() as u64;
            if bend <= offset {
                if *o < offset {
                    break;
                } else {
                    continue;
                }
            }
            let from = offset.max(*o);
            let to = end.min(bend);
            out[(from - offset) as usize..(to - offset) as usize]
                .copy_from_slice(&b[(from - o) as usize..(to - o) as usize]);
        }
        out
    }
}

/// The recorded admission, call by call (admit-replay.rs's loop without
/// its comparisons): every export invoked with the memory the host wrote.
pub(super) fn replay_recorded(
    context: &mut Context,
    label: &str,
    admissions_dir: &Path,
    battle: usize,
) -> Result<Handles, String> {
    // matrix-b
    eprintln!(
        "{label}: world source: recorded admission (battle {})",
        battle + 1
    );
    let gz_path = admissions_dir.join(format!("{label}.jsonl.gz"));
    let plain_path = admissions_dir.join(format!("{label}.jsonl"));
    let path = if gz_path.exists() {
        gz_path
    } else {
        plain_path
    };
    let content = read_maybe_gzip(&path)?;

    let lines: Vec<Value> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("json parse error: {e}")))
        .collect::<Result<Vec<_>, _>>()?;

    let lines = super::battles::admission_lines(lines, battle)?; // matrix-b
    let mut memory = Memory::default();
    let mut handles: BTreeMap<u64, f64> = BTreeMap::new();
    let mut admitted = Handles::default();
    let (mut io_at, mut enabled_at, mut places_at, mut scratch_at, mut scratch_len) =
        (0u64, 0u64, 0u64, 0u64, 0usize);
    // The world facts bank (`wire_store_world_input`): the host writes the
    // tick, clock and links there before each `wire_publish`.
    let mut facts_at = 0u64;
    let mut publication = bfe_sim::abi::context_step::Publication::default();
    let mut regions: Vec<Region> = Vec::new();
    let f2_bank_bytes = match context.invoke_export("bank:F2_INPUT", &Call::default()) {
        Ok(Returned::Row(words)) => words.len() * 8,
        _ => 0,
    };
    let mut kept: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    let mut replayed = 0usize;

    for (index, line) in lines.iter().enumerate() {
        match line["type"].as_str().unwrap_or("") {
            "battle" => {
                if line["battle"].as_u64() != Some(battle as u64) {
                    // matrix-b
                    break;
                }
                *context = Context::new();
                handles.clear();
                memory = Memory::default();
            }
            "checkpoint" => {
                let mut written = Vec::new();
                for write in line["writes"].as_array().into_iter().flatten() {
                    let offset = write["offset"].as_u64().unwrap_or(0);
                    let bytes = decode64(write["bytes"].as_str().unwrap_or(""));
                    written.push((offset, bytes.len()));
                    memory.write(offset, bytes);
                }
                mirror_writes(context, &memory, &regions, &written)?;
                let mut out = Vec::new();
                for id in bank::ALL.iter() {
                    let _ = context.read_bank(*id, &mut out);
                }
            }
            "end" => break,
            "call" => {
                let mut written = Vec::new();
                for write in line["writes"].as_array().into_iter().flatten() {
                    let offset = write["offset"].as_u64().unwrap_or(0);
                    let bytes = decode64(write["bytes"].as_str().unwrap_or(""));
                    written.push((offset, bytes.len()));
                    memory.write(offset, bytes);
                }
                mirror_writes(context, &memory, &regions, &written)?;
                let export = line["export"].as_str().unwrap_or("");
                // A descriptor the host read names its input banks' addresses.
                if matches!(
                    export,
                    "f3_init" | "f4_live_init" | "ai_world_boundary" | "f2_init"
                ) {
                    let row = words(&line["returned"]["block"]["words"]);
                    regions.extend(regions_of(export, &row, f2_bank_bytes));
                }
                // The kernels' flag banks: the page wrote every body's enabled
                // flag there (`bodiesIn`, `bodyFlags`) through these pointers.
                if matches!(export, "enabled_ptr" | "movement_flags_ptr") {
                    let at = line["returned"]["scalar"].as_str().map(bits).unwrap_or(0.0) as u64;
                    if at != 0 {
                        let bank = if export == "enabled_ptr" {
                            "COLLISION_ENABLED"
                        } else {
                            "MOVEMENT_FLAGS"
                        };
                        regions.push(Region {
                            bank,
                            at,
                            bytes: 1 << 16,
                            u8: true,
                        });
                    }
                }
                let mut call = Call {
                    args: words(&line["arguments"]),
                    row: words(&line["args"]["words"]),
                    heap: words(&line["args"]["heap"]),
                    ..Default::default()
                };
                for arg in call.args.iter_mut() {
                    if let Some(key) = handles.get(&arg.to_bits()) {
                        *arg = *key;
                    }
                }
                let returned_scalar = line["returned"]["scalar"].as_str().map(bits);
                let returned_addr = returned_scalar.map(|v| v as u64).unwrap_or(0);
                if READS_SCRATCH.contains(&export) {
                    let (at, len) = match export {
                        "camera_init" => (call.args[0] as u64, call.args[1] as usize),
                        "camera_add_definition" => (call.args[1] as u64, call.args[2] as usize),
                        "body_construct" => (call.args[0] as u64, call.args[1] as usize),
                        _ => (scratch_at, scratch_len),
                    };
                    call.scratch = memory.read(at, len);
                } else if READS_IO.contains(&export) {
                    let mut scratch = memory.read(io_at, IO_WORDS * 8);
                    if enabled_at != 0 {
                        scratch.extend(memory.read(enabled_at, MAX_BODIES));
                    }
                    if places_at != 0 {
                        scratch.extend(memory.read(places_at, MAX_BODIES));
                    }
                    call.scratch = scratch;
                } else if returned_addr != 0 && FILLS.contains(&export) {
                    let len = match export {
                        "sim_scratch" => Some(call.args[0] as usize),
                        "sim_import_room" => Some(
                            (5 + call.args[1] as usize * call.args[2] as usize
                                + call.args[3] as usize)
                                * 8,
                        ),
                        "tree_root" => Some(call.args[1] as usize * 4),
                        "tree_polygons" => Some(call.args[1] as usize * 4),
                        "veh_bytes" => Some(call.args[0] as usize),
                        "veh_input" => Some(call.args[0] as usize * 8),
                        "input_reserve" => Some(call.args[0] as usize * 8),
                        "pose_construction_input" => Some(call.args[0] as usize),
                        _ => None,
                    };
                    let len = match len {
                        Some(len) => len,
                        None => match context.invoke_export(export, &call) {
                            Ok(Returned::Row(words)) => words.len() * 8,
                            Ok(Returned::Bytes(bytes)) => bytes.len(),
                            _ => 0,
                        },
                    };
                    if KEPT_BUFFERS.contains(&export) {
                        let retained = memory.keep(
                            returned_addr,
                            len,
                            kept.get(export).copied(),
                            export == "veh_input",
                        );
                        kept.insert(export.to_string(), (returned_addr, retained));
                    } else if !VIEW_BANKS.contains(&export) {
                        memory.write(returned_addr, vec![0; len]);
                    }
                    let mut written = false;
                    if let Some(next) = lines.get(index + 1) {
                        for write in next["writes"].as_array().into_iter().flatten() {
                            let offset = write["offset"].as_u64().unwrap_or(0);
                            let bytes = decode64(write["bytes"].as_str().unwrap_or(""));
                            if offset < returned_addr + len as u64
                                && offset + bytes.len() as u64 > returned_addr
                            {
                                written = true;
                            }
                            memory.write(offset, bytes);
                        }
                    }
                    if written {
                        call.scratch = memory.read(returned_addr, len);
                    }
                    if export == "sim_import_room" && call.scratch.len() >= 40 {
                        call.scratch.drain(..40);
                    }
                }

                if export == "sim_str_intern" {
                    let text = line["intern"]["text"].as_str().unwrap_or("");
                    call.scratch = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                } else if let Some(intern) = line["intern"].as_object() {
                    if let Some(text) = intern.get("text").and_then(|t| t.as_str()) {
                        call.strings.push(text.encode_utf16().collect());
                    }
                }

                if export == "world_events_run" {
                    let mut drained = Vec::new();
                    let _ = context.read_bank(bank::EVENTS, &mut drained);
                    continue;
                }
                // The welcome's publication: natively the publisher takes the
                // facts row as an argument (`world_publish_into`), where the
                // module read the bank the host had just written. Replaying
                // the export without them would publish tick 0 with no link,
                // and the own summaries cached for the local client would
                // differ from the Node host's at the first tick.
                if export == "wire_publish" {
                    let head = memory.read(facts_at, WireWorldFacts::WORDS * 8);
                    let mut facts: Vec<f64> = head
                        .chunks_exact(8)
                        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                        .collect();
                    let links = facts[WireWorldFacts::LINKS + 1] as usize;
                    let tail = memory.read(
                        facts_at + (WireWorldFacts::WORDS * 8) as u64,
                        links * WirePublicationLink::WORDS * 8,
                    );
                    facts.extend(
                        tail.chunks_exact(8)
                            .map(|c| f64::from_le_bytes(c.try_into().unwrap())),
                    );
                    let session = call.args.first().copied().unwrap_or(0.0) as u32;
                    replayed += 1;
                    context
                        .world_publish_into(session, &facts, &mut publication)
                        .map_err(|e| {
                            format!("admission publish {}: {}", line["sequence"], e.message())
                        })?;
                    continue;
                }

                replayed += 1;
                let answer = context.invoke_export(export, &call);
                super::recon::admitted(context, export, &call)?; // matrix-b
                match answer {
                    Err(error) => {
                        if error.kind() != ErrorKind::Unavailable {
                            return Err(format!(
                                "admission call {} ({export}) failed: {}",
                                line["sequence"],
                                error.message()
                            ));
                        }
                    }
                    Ok(Returned::Scalar(value)) => {
                        if HANDLES.contains(&export) {
                            handles.insert(returned_scalar.unwrap_or(0.0).to_bits(), value);
                        }
                        match export {
                            "wire_publish_create" => admitted.publication = value as u32,
                            "wire_store_source_create" => admitted.store_source = value as u32,
                            "wire_host_create" => admitted.host = value as u32,
                            "wire_combat_create" => admitted.combat = value as u32,
                            "wire_presentation_create" => admitted.presentation = value as u32,
                            "input_mailbox_create" => admitted.mailbox = value as u32,
                            "wire_edges_create" => admitted.edges = value as u32,
                            "players_create" => admitted.player = value as u32,
                            _ => {}
                        }
                    }
                    Ok(_) => {}
                }
                match export {
                    "wire_store_world_input" => facts_at = returned_addr,
                    "io_ptr" => io_at = returned_addr,
                    "enabled_ptr" => enabled_at = returned_addr,
                    "places_ptr" => places_at = returned_addr,
                    "sim_scratch" => {
                        scratch_at = returned_addr;
                        scratch_len = call.args.first().copied().unwrap_or(0.0) as usize;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    eprintln!(
        "{label}: {replayed} of {} recorded calls replayed",
        lines.len()
    );
    Ok(admitted)
}

#[cfg(test)]
mod tests {
    use super::Memory;

    #[test]
    fn collider_request_preserves_sparse_pose_tail() {
        let mut memory = Memory::default();
        let at = 128;
        let mut len = memory.keep(at, 80, None, true);
        memory.write(at, vec![1; 80]);
        len = memory.keep(at, 16, Some((at, len)), true);
        memory.write(at, vec![2; 16]);
        len = memory.keep(at, 80, Some((at, len)), true);
        assert_eq!(len, 80);
        assert_eq!(memory.read(at + 16, 64), vec![1; 64]);
        let moved = 512;
        assert_eq!(memory.keep(moved, 96, Some((at, len)), true), 96);
        assert_eq!(memory.read(moved, 16), vec![2; 16]);
        assert_eq!(memory.read(moved + 16, 64), vec![1; 64]);
        assert_eq!(memory.read(moved + 80, 16), vec![0; 16]);
    }
}
