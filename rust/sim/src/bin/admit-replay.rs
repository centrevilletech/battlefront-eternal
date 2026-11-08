//! Replays a recorded admission call log (tools/world-hash.mjs --record-calls,
//! one JSON object a line, read uncompressed on stdin) through
//! `Context::invoke_export`, call by call, as the JavaScript host made the
//! calls against the WebAssembly module: the host's memory writes between
//! exports become the calls' scratch bytes, module addresses the host passed
//! back become the pool keys the native bodies answered, and every numeric
//! answer is compared with the recorded one bit for bit. At the recording's
//! checkpoint (welcome done, before the first tick) the 28 banks are read and
//! hashed as the recorder hashed them. Prints one JSON summary.
//!
//!   zcat RECORDING.jsonl.gz | admit-replay
use bfe_sim::abi::context::{Context, ErrorKind};
use bfe_sim::abi::context_step::bank;
use bfe_sim::world::admit::{Call, Returned};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::BufRead;

/// Host-import round trips a native host has no body for: the recording's
/// host answered them; the native hosts read the banks instead.
const SKIPPED: &[&str] = &["world_events_run"];
/// Module facts a native body cannot share: memory pages grown, and the
/// publication descriptor whose spans are module addresses (block-relative
/// natively; Context::world_publish hands the bytes).
const MODULE_ONLY: &[&str] = &["sim_reserve", "sim_memory_pages", "wire_publish"];
/// Exports whose answer is a plain number in the module too: compared.
const NUMERIC: &[&str] = &["ai_world_activate","ai_world_begin","ai_world_command","ai_world_content","ai_world_dangers","ai_world_phase","ai_world_roster","body_aim_capacity","body_new","body_preview_capacity","cache_new","camera_add_definition","camera_director_best","camera_director_follow","camera_director_show","camera_init","clip_new","cmb_set","cmb_weapon_set","f2_allocations","f2_body_words","f2_command","f2_core_allocations","f2_firing_words","f2_lifecycle_allocations","f2_ordnance_allocations","f2_retire","f2_rider_birth_words","f2_seats_adopt_words","f2_stage_allocations","f2_turret_words","f3_acquire","f3_adopt","f3_body_skeleton","f3_command","f3_dispenser_binding","f3_factory_acquire","f3_factory_player_spawn","f3_factory_realize","f3_pose","f3_recon_command","f3_registry_add","f3_relations","f3_step","f4_battle_capacity","f4_battle_command","f4_human_ready","f4_live_command","f4_startup_class","f4_tail_capacity","f4_unit_spawn","gather_begin","gather_end","grid_new","grid_ray","input_actions_words","input_controls_axis","input_controls_words","input_fire_words","input_mailbox_create","input_mailbox_rejected","input_queue_words","items_args_words","items_update","mover_new","mover_tick","network_seats_words","nothing","ord_args_words","ord_update","overlap_begin","overlap_end","players_create","players_fire","players_link","pose_construct_batches","pose_construct_vehicle","pose_construction_length","pose_new","positions_new","set_new","sim_abi","sim_import","sim_import_capacity","sim_kind_hash","sim_kinds","sim_memory_pages","sim_reserve","sim_retire","sim_str_count","sim_str_intern","sim_str_len","sound_new","terrain_near_ray","tree_new","tree_ray","tree_root_len","tree_roots","units_droideka_class","veh_flight_acc","veh_flight_keys","veh_flight_tune","veh_ground","veh_pack","veh_pull","wire_bind_store","wire_budget_get","wire_budget_run","wire_combat_commit","wire_combat_dynamic","wire_combat_next_id","wire_combat_words","wire_cosmetics_byte_count","wire_cosmetics_error_len","wire_cosmetics_metric","wire_cosmetics_run","wire_cosmetics_word_count","wire_edges_words","wire_host_get","wire_host_op","wire_host_words","wire_input_words","wire_presentation_error_field","wire_presentation_len","wire_presentation_run","wire_publish_bind","wire_registry_words","wire_semantic_len","wire_semantic_run","wire_shots_attach","wire_shots_counter","wire_shots_overflow","wire_stagger_due","wire_stagger_held","wire_stagger_schemas","wire_stagger_words","wire_store_source_configure","wire_store_source_words","wire_words","world_candidates","world_cast","world_cast_moving","world_cast_rays","world_clock_time","world_clock_words","world_events_words","world_local_words","world_new"];
/// Exports answering a module address the host passes back as a handle:
/// mapped to the pool key the native body answered.
const HANDLES: &[&str] = &["client_prediction_create","wire_budget_create","wire_combat_create","wire_cosmetics_create","wire_create","wire_edges_create","wire_host_create","wire_presentation_create","wire_publish_create","wire_semantic_create","wire_shots_create","wire_stagger_create","wire_store_source_create"];
/// Exports that read the scratch bytes a prior sim_scratch call sized.
const READS_SCRATCH: &[&str] = &["camera_init", "camera_add_definition", "body_construct", "sim_str_intern"];
/// Collision queries that read the exchange slots and enabled flags the host wrote.
const READS_IO: &[&str] = &["grid_new", "body_set", "world_candidates", "world_cast", "world_cast_moving", "grid_ray", "tree_ray", "terrain_near_ray", "gather_begin", "gather_end", "overlap_begin", "overlap_end", "push_walls_ray"];
/// Buffers that outlive their call (views the page keeps writing into):
/// never zero-filled by the export.
/// Exports that hand the host a buffer to write (a `*mut` return in the
/// module): the only ones a host write can fill. A `*const` answer is a
/// result the host reads; invoking such an export twice would repeat its
/// effect (body_spawn's output replaces the exchange it reads).
const FILLS: &[&str] = &["body_aim_ptr", "body_preview_frame_ptr", "body_preview_looks_ptr", "camera_commands_ptr", "camera_flags_ptr", "camera_input_ptr", "candidates_ptr", "clip_data", "clip_tracks", "enabled_ptr", "f4_tail_commands", "grid_cells", "grid_index", "grid_start", "input_reserve", "io_ptr", "movement_flags_ptr", "movement_io_ptr", "places_ptr", "pose_construction_input", "pose_out", "pose_q", "pose_t", "pose_w", "positions_at", "set_bounds", "set_polygons", "sim_call_room", "sim_import_result_room", "sim_import_room", "sim_scratch", "sound_info", "sound_input", "tree_index", "tree_indirect", "tree_polygons", "tree_root", "veh_bytes", "veh_flags", "veh_input", "wire_cosmetics_payload", "wire_cosmetics_room", "wire_presentation_room", "wire_semantic_room", "wire_store_world_input", "world_order", "world_step_init"];
/// Buffers the module resizes in place, keeping their bytes across calls.
const KEPT_BUFFERS: &[&str] = &["pose_construction_input", "veh_bytes", "veh_input"];
const VIEW_BANKS: &[&str] = &["camera_input_ptr", "camera_commands_ptr", "camera_output_ptr", "camera_flags_ptr", "io_ptr", "enabled_ptr", "candidates_ptr", "places_ptr", "movement_io_ptr", "movement_flags_ptr", "body_aim_ptr", "body_preview_frame_ptr", "body_preview_looks_ptr", "f4_tail_commands", "wire_store_world_input", "input_reserve"];
const IO_WORDS: usize = 64;
const MAX_BODIES: usize = 1 << 16;

fn bits(text: &str) -> f64 { f64::from_bits(u64::from_str_radix(text, 16).unwrap_or(0)) }
fn words(value: &Value) -> Vec<f64> { value.as_array().map(|a| a.iter().filter_map(|v| v.as_str()).map(bits).collect()).unwrap_or_default() }
fn base64(text: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let (mut out, mut acc, mut n) = (Vec::with_capacity(text.len() * 3 / 4), 0u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        let Some(v) = T.iter().position(|t| *t == c) else { continue };
        acc = (acc << 6) | v as u32; n += 6;
        if n >= 8 { n -= 8; out.push((acc >> n) as u8); }
    }
    out
}
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// The host's memory as the writes describe it: offset to bytes, newest last.
#[derive(Default)]
struct Memory(BTreeMap<u64, Vec<u8>>);
impl Memory {
    fn write(&mut self, offset: u64, bytes: Vec<u8>) {
        // Split anything the new write overlaps, so each byte has one owner.
        let end = offset + bytes.len() as u64;
        let overlapping: Vec<u64> = self.0.range(..end).rev().take_while(|(o, b)| **o + b.len() as u64 > offset || **o >= offset).map(|(o, _)| *o).collect();
        for o in overlapping {
            let b = self.0.remove(&o).unwrap();
            let bend = o + b.len() as u64;
            if o < offset { self.0.insert(o, b[..(offset - o) as usize].to_vec()); }
            if bend > end { self.0.insert(end, b[(end - o) as usize..].to_vec()); }
        }
        self.0.insert(offset, bytes);
    }
    fn read(&self, offset: u64, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        let end = offset + len as u64;
        for (o, b) in self.0.range(..end).rev() {
            let bend = *o + b.len() as u64;
            if bend <= offset { if *o < offset { break; } else { continue; } }
            let from = offset.max(*o); let to = end.min(bend);
            out[(from - offset) as usize..(to - offset) as usize].copy_from_slice(&b[(from - o) as usize..(to - o) as usize]);
        }
        out
    }
}

// SHA-256, as the recorder hashes each bank's raw little-endian words.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    let mut h: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    let mut m = data.to_vec(); m.push(0x80);
    while m.len() % 64 != 56 { m.push(0); }
    m.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap()); }
        for i in 16..64 { let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3); let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10); w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1); }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25); let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22); let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) { *x = x.wrapping_add(y); }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() { out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes()); }
    out
}

fn main() {
    let lines: Vec<Value> = std::io::stdin().lock().lines().filter_map(|l| l.ok()).filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(&l).expect("a recording line")).collect();
    let mut memory = Memory::default();
    let mut handles: BTreeMap<u64, f64> = BTreeMap::new();
    let mut context = Context::new();
    let (mut calls, mut compared, mut mismatches, mut blocks_compared, mut block_mismatches) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut first: Vec<Value> = Vec::new();
    let mut nobody: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures: Vec<Value> = Vec::new();
    let (mut io_at, mut enabled_at, mut places_at, mut scratch_at, mut scratch_len) = (0u64, 0u64, 0u64, 0u64, 0usize);
    let mut banks: Vec<Value> = Vec::new();
    let mut poisoned = false;
    // Buffers the module grows in place (`resize`) keep their bytes and move
    // with a reallocation: the shadow follows them, zeroing only the grown tail.
    let mut kept: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    let mut by_export: BTreeMap<String, u64> = BTreeMap::new();
    let mut notes: Vec<Value> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match line["type"].as_str().unwrap_or("") {
            "battle" => { if line["battle"].as_u64() != Some(0) { break; } context = Context::new(); handles.clear(); memory = Memory::default(); }
            "checkpoint" => {
                // The small banks' words, for the report when a digest differs.
                let mut small: Vec<Value> = Vec::new();
                for id in [bank::F3_RANDOM_GLOBALS, bank::F3_ACKNOWLEDGEMENT, bank::F2_HULLS, bank::F2_PADS, bank::F2_LIVES, bank::F2_POSES] {
                    let mut words = Vec::new();
                    if context.read_bank(id, &mut words).is_ok() && words.len() <= 16 { small.push(json!({"bank": id, "words": words})); }
                }
                notes.push(json!({"smallBanks": small}));
                for write in line["writes"].as_array().into_iter().flatten() { memory.write(write["offset"].as_u64().unwrap_or(0), base64(write["bytes"].as_str().unwrap_or(""))); }
                let mut out = Vec::new();
                for (i, id) in bank::ALL.iter().enumerate() {
                    let recorded = &line["banks"][i];
                    let digest = match context.read_bank(*id, &mut out) {
                        Ok(_) => { let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect(); Some((out.len(), hex(&sha256(&bytes)))) }
                        Err(error) => { failures.push(json!({"bank": id, "error": error.message()})); None }
                    };
                    let same = digest.as_ref().is_some_and(|(n, d)| Some(*n as u64) == recorded["words"].as_u64() && Some(d.as_str()) == recorded["sha256"].as_str());
                    banks.push(json!({"bank": id, "name": recorded["name"], "same": same, "words": digest.as_ref().map(|d| d.0), "recordedWords": recorded["words"], "sha256": digest.as_ref().map(|d| d.1.clone()), "recorded": recorded["sha256"]}));
                }
            }
            "end" => break,
            "call" => {
                if poisoned { break; }
                for write in line["writes"].as_array().into_iter().flatten() { memory.write(write["offset"].as_u64().unwrap_or(0), base64(write["bytes"].as_str().unwrap_or(""))); }
                let export = line["export"].as_str().unwrap_or("");
                let mut call = Call { args: words(&line["arguments"]), row: words(&line["args"]["words"]), heap: words(&line["args"]["heap"]), ..Default::default() };
                // Addresses the host passes back are the native pool keys.
                for arg in call.args.iter_mut() { if let Some(key) = handles.get(&arg.to_bits()) { *arg = *key; } }
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
                    if enabled_at != 0 { scratch.extend(memory.read(enabled_at, MAX_BODIES)); }
                    if places_at != 0 { scratch.extend(memory.read(places_at, MAX_BODIES)); }
                    call.scratch = scratch;
                } else if returned_addr != 0 && FILLS.contains(&export) {
                    // A buffer the host fills after the call and the next export
                    // consumes. The recording is a memory diff, so unchanged
                    // bytes (zeros in a fresh buffer) are never listed: model
                    // the module's zero-fill of a fresh buffer, apply the writes
                    // the next line carries, then read the whole region back.
                    let len = match export {
                        "sim_scratch" => Some(call.args[0] as usize),
                        "sim_import_room" => Some((5 + call.args[1] as usize * call.args[2] as usize + call.args[3] as usize) * 8),
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
                        // Read the bank first to learn its size (an empty-scratch
                        // call answers the owned bank).
                        None => match context.invoke_export(export, &call) {
                            Ok(Returned::Row(words)) => words.len() * 8,
                            Ok(Returned::Bytes(bytes)) => bytes.len(),
                            _ => 0,
                        },
                    };
                    if KEPT_BUFFERS.contains(&export) {
                        if let Some((old_addr, old_len)) = kept.get(export).copied() {
                            if old_addr != returned_addr { let moved = memory.read(old_addr, old_len.min(len)); memory.write(returned_addr, moved); }
                            if len > old_len { memory.write(returned_addr + old_len as u64, vec![0; len - old_len]); }
                        } else { memory.write(returned_addr, vec![0; len]); }
                        kept.insert(export.to_string(), (returned_addr, len));
                    } else if !VIEW_BANKS.contains(&export) { memory.write(returned_addr, vec![0; len]); }
                    // Only a buffer the host wrote into is filled; a read (the
                    // host taking a view of a built table) carries no scratch.
                    let mut written = false;
                    if let Some(next) = lines.get(index + 1) {
                        for write in next["writes"].as_array().into_iter().flatten() {
                            let offset = write["offset"].as_u64().unwrap_or(0);
                            let bytes = base64(write["bytes"].as_str().unwrap_or(""));
                            if offset < returned_addr + len as u64 && offset + bytes.len() as u64 > returned_addr { written = true; }
                            memory.write(offset, bytes);
                        }
                    }
                    if written { call.scratch = memory.read(returned_addr, len); }
                    if export == "sim_import_room" && call.scratch.len() >= 40 { call.scratch.drain(..40); }
                }
                // The host interned through the module's scratch: the recording
                // keeps the text itself, the exact bytes the body reads.
                if export == "sim_str_intern" {
                    let text = line["intern"]["text"].as_str().unwrap_or("");
                    call.scratch = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                } else if let Some(intern) = line["intern"].as_object() { if let Some(text) = intern.get("text").and_then(|t| t.as_str()) { call.strings.push(text.encode_utf16().collect()); } }
                // The events port's host round trip has a native twin: the
                // events bank, drained by its read.
                if export == "world_events_run" {
                    let mut drained = Vec::new();
                    let _ = context.read_bank(bank::EVENTS, &mut drained);
                    *nobody.entry(export.to_string()).or_default() += 1;
                    calls += 1;
                    continue;
                }
                calls += 1;
                let answer = context.invoke_export(export, &call);
                // ADMIT_REPLAY_DUMP=export prints that export's calls and answers.
                if std::env::var("ADMIT_REPLAY_DUMP").map_or(false, |e| e == export) {
                    eprintln!("dump {} {} args {:?} row {:?} scratch {} -> {:?} recorded {:?}", line["sequence"], export, call.args, call.row, call.scratch.len(), answer.as_ref().map(|a| match a { Returned::Row(w) => json!(w), Returned::Scalar(v) => json!(v), Returned::Bytes(b) => json!(b.len()), _ => json!(null) }), line["returned"]);
                }
                match answer {
                    Err(error) => {
                        if error.kind() == ErrorKind::Unavailable { *nobody.entry(export.to_string()).or_default() += 1; }
                        else { failures.push(json!({"sequence": line["sequence"], "export": export, "error": error.message()})); if error.kind() == ErrorKind::Poisoned { poisoned = true; } }
                        if failures.len() > 50 { break; }
                        continue;
                    }
                    Ok(Returned::Scalar(value)) => {
                        match export { "io_ptr" => {} _ => {} }
                        if HANDLES.contains(&export) { handles.insert(returned_scalar.unwrap_or(0.0).to_bits(), value); }
                        else if NUMERIC.contains(&export) && !MODULE_ONLY.contains(&export) {
                            compared += 1;
                            if Some(value.to_bits()) != returned_scalar.map(f64::to_bits) {
                                mismatches += 1;
                                *by_export.entry(export.to_string()).or_default() += 1;
                                if first.len() < 12 { first.push(json!({"sequence": line["sequence"], "export": export, "expected": returned_scalar, "actual": value})); }
                            }
                        }
                    }
                    Ok(Returned::Row(ours)) => {
                        // The recorder keeps the words the host read: a block
                        // without its five header words, or a layout's prefix
                        // of a longer native row. Either alignment is exact.
                        if let Some(block) = line["returned"]["block"].as_object().filter(|_| !MODULE_ONLY.contains(&export)) {
                            let theirs = words(&block["words"]);
                            blocks_compared += 1;
                            let same = |from: usize| ours.len() >= from + theirs.len() && ours[from..from + theirs.len()].iter().zip(&theirs).all(|(a, b)| a.to_bits() == b.to_bits());
                            if !(same(0) || same(5)) {
                                block_mismatches += 1;
                                *by_export.entry(export.to_string()).or_default() += 1;
                                let from = if ours.len() == theirs.len() + 5 { 5 } else { 0 };
                                let at = ours[from.min(ours.len())..].iter().zip(&theirs).position(|(a, b)| a.to_bits() != b.to_bits());
                                if first.len() < 12 { first.push(json!({"sequence": line["sequence"], "export": export, "blockWords": [ours.len(), theirs.len()], "from": from, "firstWord": at, "expected": at.map(|i| theirs[i]), "actual": at.map(|i| ours[from + i])})); }
                            }
                        }
                    }
                    Ok(Returned::Bytes(bytes)) => {
                        // A failed pose construction answers its error text.
                        if export == "pose_construction_output" && bytes.len() < 400 && notes.len() < 6 { notes.push(json!({"sequence": line["sequence"], "text": String::from_utf8_lossy(&bytes)})); }
                    }
                    Ok(_) => {}
                }
                match export {
                    "io_ptr" => io_at = returned_addr, "enabled_ptr" => enabled_at = returned_addr, "places_ptr" => places_at = returned_addr,
                    "sim_scratch" => { scratch_at = returned_addr; scratch_len = call.args.first().copied().unwrap_or(0.0) as usize; }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let same = banks.iter().filter(|b| b["same"] == true).count();
    println!("{}", json!({"calls": calls, "numericCompared": compared, "numericMismatches": mismatches, "blocksCompared": blocks_compared, "blockMismatches": block_mismatches, "firstDifferences": first, "mismatchesByExport": by_export, "notes": notes, "noBody": nobody, "failures": failures, "banksSame": same, "banks": banks}));
    let _ = context.dispose();
    let unexpected = nobody.keys().any(|name| !SKIPPED.contains(&name.as_str()));
    if mismatches != 0 || block_mismatches != 0 || !failures.is_empty() || same != bank::ALL.len() || unexpected { std::process::exit(1); }
}
