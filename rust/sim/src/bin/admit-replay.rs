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
use bfe_content::files::DirFiles;
use bfe_sim::abi::context::{Context, ErrorKind};
use bfe_sim::abi::context_step::bank;
use bfe_sim::world::admit::{Call, Options, Returned, Stage};
use bfe_sim::world::owner::World;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
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

fn parse_matrix_options<'a>(args: &[&'a str]) -> Options {
    let opt = |key: &str, fallback: &'a str| -> &'a str {
        args.iter()
            .position(|&a| a == key)
            .and_then(|i| args.get(i + 1).copied())
            .unwrap_or(fallback)
    };
    Options {
        map: opt("--map", "nab2").to_string(),
        side: opt("--side", "rep").to_string(),
        enhanced: opt("--ai", "classic") == "enhanced",
        seed: opt("--seed", "1").parse::<u32>().unwrap_or(1),
        heroes: args.contains(&"--heroes"),
        unit: args
            .iter()
            .position(|&a| a == "--unit")
            .and_then(|i| args.get(i + 1).map(|s| (*s).to_string())),
    }
}

fn find_banks_file(banks_dir: &str, row_idx: usize, label: &str) -> String {
    let direct = format!("{banks_dir}/{label}.banks");
    if std::path::Path::new(&direct).exists() {
        return direct;
    }
    let prefix = format!("{row_idx:02}-");
    if let Ok(entries) = std::fs::read_dir(banks_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name.ends_with(".banks") {
                return entry.path().to_string_lossy().into_owned();
            }
        }
    }
    direct
}

/// The matrix line of one label: its options as tools/world-hash-matrix.txt spells them.
fn matrix_options(matrix_file: &str, label: &str) -> Result<Options, String> {
    let text = std::fs::read_to_string(matrix_file).map_err(|e| format!("{matrix_file}: {e}"))?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.first() == Some(&label) { return Ok(parse_matrix_options(&parts[1..])); }
    }
    Err(format!("{label}: not in {matrix_file}"))
}

/// The cold sequence's calls for a label, compared one by one with the
/// recorded host's admission calls as the replay reconstructs them. The
/// recording's module plumbing (sim_scratch, sim_import_room, memory
/// facts) has no cold twin and is passed over; every export the cold
/// sequence dispatches must appear in the recording in the same order with
/// the same scalar arguments, argument row, heap, scratch bytes and
/// interned texts. The first difference names the export and the word.
/// Argument words that are module scratch addresses (the body reads the
/// call's scratch bytes instead): never equal natively, never compared.
const ADDRESS_ARGS: &[(&str, usize)] = &[("camera_init", 0), ("camera_add_definition", 1), ("body_construct", 0)];

struct ColdCalls {
    /// The cold world after admission: its registry is checked against the
    /// KIND_UNITS import the host recorded before attachF3. A partial run
    /// (--from) has none of its own: the recorded prefix made the registry.
    world: Option<Box<World>>,
    calls: Vec<(Stage, &'static str, Call)>,
    exports: BTreeSet<&'static str>,
    next: usize,
    compared: u64,
    /// sim_str_intern texts the host interned since the last compared call.
    interns: Vec<String>,
    differences: Vec<Value>,
    stopped: bool,
    /// The bytes the host wrote into the loan a sizing call handed out
    /// (veh_bytes, veh_input); the cold consumer carries them in its scratch.
    loan: Vec<u8>,
    /// Consumed loans whose reconstructed bytes differed (advisory).
    loan_notes: u64,
}
/// Host plumbing never compared: the bodies' order refresh the page makes
/// at many seams (rayBodies), whose list the kernel rebuilds either way.
const UNCOMPARED: &[&str] = &["world_order"];
/// Sizing calls that hand the host a loan, and the calls that consume it.
const LOANS: &[&str] = &["veh_bytes", "veh_input"];
const CONSUMERS: &[&str] = &["veh_pack", "veh_pull", "veh_collider"];
impl ColdCalls {
    fn build(content_dir: &str, matrix_file: &str, label: &str) -> Result<Self, String> {
        let options = matrix_options(matrix_file, label)?;
        let files = DirFiles::new(content_dir);
        let mut world = World::new();
        let mut rows = bfe_sim::world::admit::Rows::default();
        // A panic inside admission is reported with the calls made before it,
        // which the recording still checks one by one.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bfe_sim::world::admit::admit_into(&mut world, &files, &options, &mut rows)));
        let failure = match outcome {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(panic) => Some(format!("panic: {}", panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
        };
        if let Some(failure) = &failure { eprintln!("cold admission stopped: {failure}"); }
        let calls: Vec<(Stage, &'static str, Call)> = rows.calls.into_iter().flat_map(|(_, calls)| calls).collect();
        let calls: Vec<(Stage, &'static str, Call)> = calls.into_iter().filter(|(_, name, _)| !UNCOMPARED.contains(name)).collect();
        let exports = calls.iter().map(|(_, name, _)| *name).collect();
        let mut differences = Vec::new();
        if let Some(failure) = failure { differences.push(json!({"field": "cold admission", "cold": failure})); }
        Ok(ColdCalls { world: Some(world), calls, exports, next: 0, compared: 0, interns: Vec::new(), differences, stopped: false, loan: Vec::new(), loan_notes: 0 })
    }
    /// The calls a lane's stages made cold over the recorded prefix
    /// (bfe_sim::world::admit::partial), compared from the boundary on.
    fn partial(calls: Vec<(Stage, &'static str, Call)>) -> Self {
        let calls: Vec<(Stage, &'static str, Call)> = calls.into_iter().filter(|(_, name, _)| !UNCOMPARED.contains(name)).collect();
        let exports = calls.iter().map(|(_, name, _)| *name).collect();
        ColdCalls { world: None, calls, exports, next: 0, compared: 0, interns: Vec::new(), differences: Vec::new(), stopped: false, loan: Vec::new(), loan_notes: 0 }
    }
    fn note(&mut self, difference: Value) {
        if self.differences.len() < 200 { self.differences.push(difference); }
    }
    /// The host's registry handover (sim_import of KIND_UNITS, game.js before
    /// attachF3): its member list must be a prefix of the cold world's, in
    /// order, with the same kinds and generations, and its next id the
    /// prefix's end. The members after it are the Units stage's own adds.
    fn registry(&mut self, line: &Value, recorded: &Call) {
        let mut recorded_units = bfe_sim::gen::Units::default();
        if recorded.row.len() < bfe_sim::gen::Units::FULL_STRIDE { return; }
        recorded_units.take_full(&recorded.row, &recorded.heap);
        let Some(world) = self.world.as_ref() else { return; };
        let cold = bfe_sim::world::admit::registry_members(world);
        let same = cold.len() >= recorded_units.list.len() && cold[..recorded_units.list.len()] == recorded_units.list[..]
            && recorded_units.next_id == recorded_units.list.len() as f64 + 1.0;
        if !same {
            let show = |list: &[bfe_sim::gen::Member]| -> Vec<String> { list.iter().map(|m| format!("{m:?}")).collect() };
            let at = cold.iter().zip(&recorded_units.list).position(|(a, b)| a != b);
            self.note(json!({"sequence": line["sequence"], "export": "sim_import", "field": "registry", "member": at, "lengths": [recorded_units.list.len(), cold.len()],
                "recordedNextId": recorded_units.next_id, "recorded": show(&recorded_units.list), "cold": show(&cold[..cold.len().min(recorded_units.list.len() + 2)])}));
        }
    }
    fn compare(&mut self, line: &Value, export: &str, recorded: &Call) {
        if self.stopped || UNCOMPARED.contains(&export) { return; }
        if export == "sim_import" {
            // The host interns a row's texts as it writes an import; they
            // are the import's, not the next export's. A Unit's import is
            // the exception: its side and class name are the body's own
            // interns, which the cold construction makes for its skeleton.
            if recorded.args.first().copied() != Some(f64::from(bfe_sim::gen::KIND_UNIT)) { self.interns.clear(); }
            if recorded.args.first().copied() == Some(f64::from(bfe_sim::gen::KIND_UNITS)) { self.registry(line, recorded); }
            // An import the cold sequence makes itself (the host objects it
            // constructs natively: posts, the battle, the human spawn) is
            // compared row for row with the host's; any other is passed over.
            let cold_import = matches!(self.calls.get(self.next), Some((_, "sim_import", cold)) if cold.args.first() == recorded.args.first());
            if !cold_import { return; }
        }
        if export == "sim_str_intern" {
            if let Some(text) = line["intern"]["text"].as_str() { self.interns.push(text.to_string()); }
            return;
        }
        if !self.exports.contains(export) { return; }
        let sequence = line["sequence"].clone();
        // The kernel's body order refresh (collision.js rayBodies) is a call
        // the fleet stage makes natively without recording one; only the
        // page body's spawn records it (with the spawn row the recorder
        // attributed to it), so a recorded one the cold side skipped passes.
        if export == "world_order" && !matches!(self.calls.get(self.next), Some((_, "world_order", _))) { return; }
        // The import rooms the other owners' imports size (their imports are
        // passed over above) are not the cold sequence's either.
        if export == "sim_import_room" && !matches!(self.calls.get(self.next), Some((_, "sim_import_room", _))) { return; }
        let Some((stage, name, cold)) = self.calls.get(self.next) else {
            self.note(json!({"sequence": sequence, "export": export, "field": "order", "recorded": export, "cold": "no call left"}));
            self.stopped = true;
            return;
        };
        if *name != export {
            let upcoming: Vec<&str> = self.calls[self.next..].iter().take(10).map(|(_, name, _)| *name).collect();
            self.note(json!({"sequence": sequence, "export": export, "field": "order", "stage": format!("{stage:?}"), "recorded": export, "cold": name, "coldNext": upcoming}));
            self.stopped = true;
            return;
        }
        self.next += 1;
        self.compared += 1;
        let stage = format!("{stage:?}");
        let skip = ADDRESS_ARGS.iter().find(|(name, _)| *name == export).map(|(_, index)| *index);
        let words = |field: &str, theirs: &[f64], ours: &[f64], differences: &mut Vec<Value>| {
            let at = theirs.iter().zip(ours).enumerate().position(|(i, (a, b))| a.to_bits() != b.to_bits() && !(field == "args" && Some(i) == skip));
            if theirs.len() != ours.len() || at.is_some() {
                if differences.len() < 200 {
                    differences.push(json!({"sequence": sequence, "export": export, "stage": stage, "field": field, "lengths": [theirs.len(), ours.len()],
                        "word": at, "recorded": at.map(|i| format!("{:016x} ({})", theirs[i].to_bits(), theirs[i])), "cold": at.map(|i| format!("{:016x} ({})", ours[i].to_bits(), ours[i]))}));
                }
            }
        };
        let mut differences = std::mem::take(&mut self.differences);
        // A firing birth differing anywhere is shown decoded, both sides.
        if export == "f2_firing_adopt" && (recorded.row != cold.row || recorded.heap != cold.heap) && differences.len() < 12 {
            let decode = |c: &Call| if c.row.len() >= bfe_sim::gen::FfBirth::WORDS { format!("{:?}", bfe_sim::gen::FfBirth::take(&c.row, &c.heap)) } else { String::new() };
            differences.push(json!({"sequence": sequence, "export": export, "stage": stage, "field": "birth", "recorded": decode(recorded), "cold": decode(cold)}));
        }
        if export == "f2_turret_adopt" && (recorded.row != cold.row || recorded.heap != cold.heap) && differences.len() < 12 {
            let decode = |c: &Call| if c.row.len() >= bfe_sim::gen::F2TurretBirth::WORDS { format!("{:?}", bfe_sim::gen::F2TurretBirth::take(&c.row, &c.heap)) } else { String::new() };
            differences.push(json!({"sequence": sequence, "export": export, "stage": stage, "field": "birth", "recorded": decode(recorded), "cold": decode(cold)}));
        }
        words("args", &recorded.args, &cold.args, &mut differences);
        // F3.bodiesIn's rows go straight into the F3 input bank; the
        // recording has no row for them (only the count argument).
        // spawnSoldier writes its row through sim_call_room; the recording
        // keeps those words on the room's line, not the spawn's.
        if !matches!(export, "f3_body_names" | "body_spawn") { words("row", &recorded.row, &cold.row, &mut differences); }
        words("heap", &recorded.heap, &cold.heap, &mut differences);
        // A loan's bytes belong to the consumer's scratch, not the sizing call's.
        let (theirs_scratch, ours_scratch) = if LOANS.contains(&export) { self.loan = recorded.scratch.clone(); (Vec::new(), cold.scratch.clone()) }
            else if CONSUMERS.contains(&export) { (std::mem::take(&mut self.loan), cold.scratch.clone()) }
            else { (recorded.scratch.clone(), cold.scratch.clone()) };
        // A consumed loan the host wrote into was often reallocated by the
        // module; the recorder's memory diff never lists bytes equal to what
        // the new block held, so its reconstruction is lossy there. Those
        // differences are noted, not counted (the F2 banks cover the values).
        // A buffer the host fills after the call (FILLS) is recorded as memory
        // writes, not as the call's scratch: an empty recorded scratch there
        // is the recorder's shape, not a difference.
        let advisory = CONSUMERS.contains(&export) || (FILLS.contains(&export) && theirs_scratch.is_empty());
        if theirs_scratch != ours_scratch && advisory {
            self.loan_notes += 1;
            // ADMIT_REPLAY_LOANS=1 shows them, as numbers from the first word.
            if std::env::var("ADMIT_REPLAY_LOANS").is_ok() && differences.len() < 12 && CONSUMERS.contains(&export) {
                let at = theirs_scratch.chunks_exact(8).zip(ours_scratch.chunks_exact(8)).position(|(a, b)| a != b);
                let words = |bytes: &[u8]| -> Vec<f64> { bytes.chunks_exact(8).skip(at.unwrap_or(0)).take(12).map(|x| f64::from_le_bytes(x.try_into().unwrap_or([0; 8]))).collect() };
                differences.push(json!({"sequence": sequence, "export": export, "field": "loan", "args": recorded.args, "lengths": [theirs_scratch.len(), ours_scratch.len()], "word": at, "recorded": words(&theirs_scratch), "cold": words(&ours_scratch)}));
            }
        }
        if theirs_scratch != ours_scratch && !advisory && differences.len() < 200 {
            let at = theirs_scratch.iter().zip(&ours_scratch).position(|(a, b)| a != b);
            let show = |bytes: &[u8]| -> String { let from = at.unwrap_or(0).min(bytes.len()); bytes[from..bytes.len().min(from + 80)].iter().map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' }).collect() };
            // Word-sized scratch (pulled poses, collider rows) is shown as numbers from the first differing word.
            let words = |bytes: &[u8]| -> Vec<f64> { let from = at.unwrap_or(0) / 8 * 8; bytes[from.min(bytes.len())..].chunks_exact(8).take(10).map(|x| f64::from_le_bytes(x.try_into().unwrap_or([0; 8]))).collect() };
            let numeric = theirs_scratch.len() % 8 == 0 && ours_scratch.len() % 8 == 0;
            differences.push(json!({"sequence": sequence, "export": export, "stage": stage, "field": "scratch", "lengths": [theirs_scratch.len(), ours_scratch.len()], "byte": at, "word": at.map(|a| a / 8),
                "recorded": show(&theirs_scratch), "cold": show(&ours_scratch),
                "recordedWords": numeric.then(|| words(&theirs_scratch)), "coldWords": numeric.then(|| words(&ours_scratch))}));
        }
        let ours: Vec<String> = cold.strings.iter().map(|units| String::from_utf16_lossy(units)).collect();
        // The page's own table lags the module's where a body interned a text
        // itself: the host then interns it again (the module answers the old
        // id). Such an intern is no difference when the cold world holds the
        // text; a cold intern the host never made, or a missing one, is.
        // A partial run has no cold world of its own: every host intern counts.
        let table = self.world.as_deref().map(|world| bfe_sim::world::admit::string_table(world));
        let theirs: Vec<String> = std::mem::take(&mut self.interns).into_iter()
            .filter(|text| ours.contains(text) || !table.is_some_and(|t| t.list.iter().any(|units| String::from_utf16_lossy(units) == *text))).collect();
        if theirs != ours && differences.len() < 200 {
            differences.push(json!({"sequence": sequence, "export": export, "stage": stage, "field": "strings", "recorded": theirs, "cold": ours}));
        }
        self.differences = differences;
    }
    fn summary(&self) -> Value {
        json!({"coldCalls": self.calls.len(), "coldCompared": self.compared, "coldLeft": self.calls.len() - self.next, "coldStopped": self.stopped, "coldLoanNotes": self.loan_notes, "coldDifferences": self.differences})
    }
}

/// Where a partial run (--from) takes the recorded world over from the
/// replay: at the first recorded call of an export only that stage makes,
/// or right after the last call of the stage before it when the stage's own
/// first exports (the viewpoint's skeleton and clips) also occur earlier.
#[derive(Clone, Copy)]
enum Boundary { Before(&'static str), After(&'static str) }
fn first_export(stage: Stage) -> Option<Boundary> {
    match stage {
        Stage::Units => Some(Boundary::Before("f3_init")),
        Stage::Policies => Some(Boundary::Before("ai_weapon_policies")),
        // createWorldSim's last owner query precedes game.js's headless
        // viewpoint adoption (rows/battle/stock.rs).
        Stage::World => Some(Boundary::After("world_local_words")),
        Stage::LiveTail => Some(Boundary::Before("f4_live_init")),
        Stage::Session => Some(Boundary::Before("players_init")),
        _ => None,
    }
}
fn stage_named(name: &str) -> Option<Stage> {
    [Stage::Units, Stage::Policies, Stage::Ai, Stage::World, Stage::LiveTail, Stage::Session].into_iter().find(|stage| format!("{stage:?}") == name)
}

/// The fleet's and props' bindings a partial run takes as fixtures, decoded
/// from the recorded rows of the stages the other lanes derive live: f3_init's
/// bodySerial, F3Props.prepare's building rows, F3Hulls.prepare's hull rows
/// with their body names, and the kernel's full body list when sent.
fn fixtures(lines: &[Value]) -> bfe_sim::world::admit::partial::Fixtures {
    use bfe_sim::gen::{F3BodyName, F3BuildingSetup, F3HullSetup};
    let mut out = bfe_sim::world::admit::partial::Fixtures::default();
    let mut hull_group = false;
    // The host writes F3BodyName rows straight into the F3 input bank, which
    // the recording does not carry: the hulls' kernel slots come from the
    // fleet's body_new answers (the last ones before the fleet's own calls)
    // and the prepared variants' f2_prepare_body arguments; the body refs
    // are the fleet lane's to number, so the kernel slot stands in for them.
    let mut kernels: Vec<u32> = Vec::new();
    let mut hull_kernels: Vec<u32> = Vec::new();
    let mut fleet_started = false;
    let mut ai_begun = false;
    let name = |kernel: u32| F3BodyName { body: bfe_sim::store::Ref::new(kernel, 0), kernel };
    for line in lines {
        // A --twice recording holds a second battle: only the first is admitted here.
        if line["type"].as_str() == Some("battle") && line["battle"].as_u64() != Some(0) { break; }
        if line["type"].as_str() != Some("call") || line["phase"].as_str() != Some("admission") { continue; }
        let export = line["export"].as_str().unwrap_or("");
        if export == "sim_import" {
            let arguments = words(&line["arguments"]);
            let (row, heap) = (words(&line["args"]["words"]), words(&line["args"]["heap"]));
            if arguments.first().copied() == Some(f64::from(bfe_sim::gen::KIND_UNITS)) && row.len() >= bfe_sim::gen::Units::FULL_STRIDE {
                let mut units = bfe_sim::gen::Units::default(); units.take_full(&row, &heap); out.registry = Some(units);
            }
            continue;
        }
        if matches!(export, "sim_str_intern" | "sim_scratch" | "sim_call_room" | "sim_import_room" | "sim_import" | "io_ptr" | "enabled_ptr" | "candidates_ptr") { continue; }
        let row = words(&line["args"]["words"]);
        let heap = words(&line["args"]["heap"]);
        if export == "ai_world_begin" && !ai_begun {
            ai_begun = true;
            if row.len() >= bfe_sim::gen::AiWorldSetup::WORDS { out.ai_setup = Some(bfe_sim::gen::AiWorldSetup::take(&row, &heap)); }
        }
        match export {
            "body_new" => { if let Some(slot) = line["returned"]["scalar"].as_str().map(bits) { kernels.push(slot as u32); if fleet_started { hull_kernels.push(slot as u32); } } }
            "pose_construct_vehicle" => { fleet_started = true; }
            "f2_prepare_body" => { if let Some(slot) = words(&line["arguments"]).first() { hull_kernels.push(*slot as u32); } }
            "f3_init" => { out.body_serial = words(&line["arguments"]).get(2).copied().unwrap_or(0.0) as u32; }
            // F3Factory.reserve's bound (game.js: units.nextId + 512).
            "f3_factory_reserve" => { out.construction_limit = words(&line["arguments"]).first().map(|v| *v as u32); }
            "pose_new" => { if let Some(id) = line["returned"]["scalar"].as_str().map(bits) { out.poses.push(id as u32); } }
            "f4_animated_birth" => { if row.len() >= bfe_sim::gen::F4AnimationSetup::WORDS { out.animated_bodies.push(bfe_sim::gen::F4AnimationSetup::take(&row, &heap).body); } }
            // The viewpoint's spawnSoldier, before the fleet: its answer row.
            "body_spawn" if out.viewpoint_spawn.is_empty() => { out.viewpoint_spawn = words(&line["returned"]["block"]["words"]); }
            "f3_building" => { if row.len() >= F3BuildingSetup::WORDS { out.buildings.push(F3BuildingSetup::take(&row, &heap)); } }
            "f3_hull" => {
                if row.len() >= F3HullSetup::WORDS {
                    let kernel = hull_kernels.get(out.hulls.len()).copied().unwrap_or(0);
                    out.hulls.push((F3HullSetup::take(&row, &heap), name(kernel), None));
                    hull_group = true;
                }
                continue;
            }
            "f3_body_names" => {
                let count = words(&line["arguments"]).first().copied().unwrap_or(0.0) as usize;
                if count != 1 {
                    let names: Vec<F3BodyName> = kernels.iter().take(count).map(|kernel| name(*kernel)).collect();
                    match out.hulls.last_mut().filter(|_| hull_group) {
                        Some(hull) if hull.2.is_none() => { out.bodies = names.clone(); hull.2 = Some(names); }
                        _ => { out.bodies = names; out.bodies_changed = true; }
                    }
                }
                continue;
            }
            _ => {}
        }
        hull_group = false;
    }
    out
}

/// ADMIT_REPLAY_BANK=N writes bank N's words (their bits) to stderr, one
/// JSON line, for a word-level comparison of two runs.
fn dump_bank(id: u32, words: &[f64]) {
    if std::env::var("ADMIT_REPLAY_BANK").ok().and_then(|v| v.parse::<u32>().ok()) == Some(id) {
        let bits: Vec<String> = words.iter().map(|w| format!("{:016x}", w.to_bits())).collect();
        eprintln!("{}", json!({"bankDump": id, "words": bits}));
    }
}

fn run_cold_matrix(args: &[String]) {
    let content_dir = args.get(1).expect("content_dir");
    let banks_dir = args.get(2).expect("banks_dir");
    let matrix_file = args.get(3).expect("matrix_file");
    let only = args.get(4).map(String::as_str);
    let files = DirFiles::new(content_dir);
    let text = std::fs::read_to_string(matrix_file).expect("read matrix file");
    let mut rows = 0usize;
    let mut exact = 0usize;
    let mut failures = Vec::new();
    let mut all_digests = Vec::new();
    let mut row_idx = 0usize;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let current_idx = row_idx;
        row_idx += 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        let label = parts[0];
        if let Some(filter) = only {
            if label != filter && format!("{current_idx:02}-{label}") != filter {
                continue;
            }
        }
        rows += 1;
        let options = parse_matrix_options(&parts[1..]);
        let context = Context::new();
        let admitted = match context.world_admit(&files, &options) {
            Ok(admitted) => admitted,
            Err(error) => {
                eprintln!("[{current_idx:02}] {label}: world_admit failed: {}", error.message());
                failures.push(json!({"label": label, "error": error.message()}));
                let _ = context.dispose();
                continue;
            }
        };
        let _ = context.world_step_init(admitted.setup);
        let bank_path = find_banks_file(banks_dir, current_idx, label);
        let bank_lines: Vec<Value> = std::fs::read_to_string(&bank_path)
            .expect("read .banks file")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<Value>(l).expect("bank json"))
            .filter(|v| v["battle"].as_u64().unwrap_or(0) == 0)
            .collect();
        let mut out = Vec::new();
        let mut diffs = Vec::new();
        // Every bank's digest, for a comparison with the recorded replay's
        // own banks (the recorder's checkpoint cannot reproduce some banks).
        let mut digests = Vec::new();
        for (i, id) in bank::ALL.iter().enumerate() {
            let recorded = &bank_lines[i];
            let ok = context.read_bank(*id, &mut out).is_ok();
            let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect();
            let digest = hex(&sha256(&bytes));
            digests.push(json!({"bank": id, "name": recorded["name"], "words": out.len(), "sha256": digest}));
            dump_bank(*id, &out);
            let same = ok
                && (out.len() as u64) == recorded["words"].as_u64().unwrap_or(u64::MAX)
                && digest == recorded["sha256"].as_str().unwrap_or("");
            if !same {
                diffs.push(json!({
                    "bank": id,
                    "name": recorded["name"],
                    "words": out.len(),
                    "recordedWords": recorded["words"],
                    "sha256": digest,
                    "recorded": recorded["sha256"],
                }));
            }
        }
        let _ = context.dispose();
        if diffs.is_empty() {
            exact += 1;
            eprintln!("[{current_idx:02}] {label}: 28/28 banks match");
        } else {
            let names: Vec<&str> = diffs.iter().filter_map(|d| d["name"].as_str()).collect();
            eprintln!("[{current_idx:02}] {label}: {} banks differ: {:?}", diffs.len(), names);
            failures.push(json!({"label": label, "diffs": diffs}));
        }
        all_digests.push(json!({"label": label, "banks": digests}));
    }
    println!(
        "{}",
        json!({"rows": rows, "exact": exact, "differing": rows - exact, "failures": failures, "digests": all_digests})
    );
    if exact != rows {
        std::process::exit(1);
    }
}

/// --posts CONTENT_DIR MATRIX_FILE LABEL: admit the battle natively and
/// compare the command vehicles' posts it imported before createNativeAI's
/// first call (KIND_MOBILE_POST rows and the KIND_MOBILE_POSTS list) with the
/// host's recorded imports on stdin, field by field, texts resolved through
/// each side's own string table (ids may differ between the two tables).
fn run_posts(args: &[String]) {
    use bfe_sim::gen::{MobilePost, MobilePosts, KIND_MOBILE_POST, KIND_MOBILE_POSTS};
    let [content_dir, matrix_file, label] = [1, 2, 3].map(|i| args.get(i).cloned().unwrap_or_default());
    let options = match matrix_options(&matrix_file, &label) { Ok(o) => o, Err(e) => { eprintln!("{e}"); std::process::exit(2); } };
    let files = DirFiles::new(&content_dir);
    let mut world = World::new();
    let (_, rows) = match bfe_sim::world::admit::admit_with_rows(&mut world, &files, &options) {
        Ok(r) => r,
        Err(e) => { println!("{}", json!({"label": label, "admitted": false, "error": e.to_string()})); std::process::exit(1); }
    };
    let cold_import = |kind: u16| rows.calls.iter().flat_map(|(_, c)| c.iter()).find(|(_, name, call)| *name == "sim_import" && call.args.first() == Some(&f64::from(kind))).map(|(_, _, c)| (c.row.clone(), c.heap.clone()));
    // The recording's texts: every sim_str_intern's answer is the new id;
    // the ids below the first one are the module's preset table, as ours.
    let mut texts: BTreeMap<u32, String> = BTreeMap::new();
    let mut recorded: BTreeMap<u16, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for l in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(line) = serde_json::from_str::<Value>(&l) else { continue; };
        if line["type"].as_str() == Some("battle") && line["battle"].as_u64() != Some(0) { break; }
        let export = line["export"].as_str().unwrap_or("");
        if export == "ai_world_begin" { break; }
        if export == "sim_str_intern" {
            if let (Some(id), Some(text)) = (line["returned"]["scalar"].as_str().map(bits), line["intern"]["text"].as_str()) { texts.insert(id as u32, text.to_string()); }
        }
        if export == "sim_import" {
            let arguments = words(&line["arguments"]);
            for kind in [KIND_MOBILE_POST, KIND_MOBILE_POSTS] {
                if arguments.first() == Some(&f64::from(kind)) { recorded.insert(kind, (words(&line["args"]["words"]), words(&line["args"]["heap"]))); }
            }
        }
    }
    let ours = |id: bfe_sim::store::StrId| bfe_sim::world::admit::string_text(&world, id);
    let theirs = |id: bfe_sim::store::StrId| texts.get(&id.0).cloned().unwrap_or_else(|| bfe_sim::world::admit::string_text(&world, id));
    let show = |p: &MobilePost, text: &dyn Fn(bfe_sim::store::StrId) -> String| json!({
        "index": p.index, "name": text(p.name), "label": text(p.label), "team": p.team, "active": p.active, "mobile": p.mobile,
        "vehicle": format!("{:?}", p.vehicle), "life": p.life, "yaw": p.yaw.to_bits(), "radius": p.radius.to_bits(), "speed": p.speed.to_bits(),
        "lastPosition": p.last_position.map(f64::to_bits), "desc": [text(p.desc.name), text(p.desc.display_name), p.desc.value_bleed.to_string(), p.desc.mobile.to_string()],
        "ai": format!("{:?}", p.ai), "reported": format!("{:?}", p.reported) });
    let decode = |(row, heap): &(Vec<f64>, Vec<f64>)| -> Vec<MobilePost> {
        row.chunks_exact(MobilePost::FULL_STRIDE).map(|r| { let mut p = MobilePost::default(); p.take_full(r, heap); p }).collect()
    };
    let cold = cold_import(KIND_MOBILE_POST).map(|r| decode(&r)).unwrap_or_default();
    let host = recorded.get(&KIND_MOBILE_POST).map(decode).unwrap_or_default();
    let cold_shown: Vec<Value> = cold.iter().map(|p| show(p, &ours)).collect();
    let host_shown: Vec<Value> = host.iter().map(|p| show(p, &theirs)).collect();
    let list = |r: Option<(Vec<f64>, Vec<f64>)>| r.filter(|(row, _)| row.len() >= MobilePosts::FULL_STRIDE).map(|(row, heap)| { let mut m = MobilePosts::default(); m.take_full(&row, &heap); format!("{:?}", m.posts) });
    let cold_list = list(cold_import(KIND_MOBILE_POSTS));
    let host_list = list(recorded.get(&KIND_MOBILE_POSTS).cloned());
    let same = cold_shown == host_shown && cold_list == host_list;
    let differing: Vec<Value> = cold_shown.iter().zip(&host_shown).filter(|(a, b)| a != b).map(|(a, b)| json!({"cold": a, "host": b})).collect();
    println!("{}", json!({"label": label, "admitted": true, "mobilePosts": [cold.len(), host.len()], "list": [cold_list, host_list], "same": same, "differing": differing,
        "posts": cold_shown.iter().map(|p| json!([p["name"], p["label"], p["team"], p["active"], p["vehicle"]])).collect::<Vec<_>>()}));
    if !same { std::process::exit(1); }
}

fn main() {
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if cli_args.first().map(String::as_str) == Some("--cold") {
        run_cold_matrix(&cli_args);
        return;
    }
    if cli_args.first().map(String::as_str) == Some("--posts") {
        run_posts(&cli_args);
        return;
    }
    // --cold-calls CONTENT_DIR MATRIX_FILE LABEL: compare the cold sequence's calls with the recording on stdin.
    // --cold-calls CONTENT_DIR MATRIX_FILE LABEL --from STAGE [--through STAGE]: replay the recording up to
    // the stage's first call, run that stage through the last one named (default Player) cold on the replayed
    // world (partial.rs), compare from there.
    let mut cold: Option<ColdCalls> = None;
    let mut partial: Option<(Stage, Boundary, String, bfe_sim::world::admit::Options, Stage)> = None;
    // An After boundary arms at its export; the partial run starts at the
    // next admission call, before that call is replayed.
    let mut armed = false;
    if cli_args.first().map(String::as_str) == Some("--cold-calls") {
        let [content_dir, matrix_file, label] = [1, 2, 3].map(|i| cli_args.get(i).cloned().unwrap_or_default());
        if cli_args.get(4).map(String::as_str) == Some("--from") {
            let name = cli_args.get(5).cloned().unwrap_or_default();
            let Some(stage) = stage_named(&name) else { eprintln!("--from: unknown stage {name}"); std::process::exit(2); };
            let Some(boundary) = first_export(stage) else { eprintln!("--from {name}: no boundary export for that stage"); std::process::exit(2); };
            let through = if cli_args.get(6).map(String::as_str) == Some("--through") {
                let name = cli_args.get(7).cloned().unwrap_or_default();
                let Some(through) = stage_named(&name) else { eprintln!("--through: unknown stage {name}"); std::process::exit(2); };
                through
            } else { Stage::Player };
            match matrix_options(&matrix_file, &label) {
                Ok(options) => partial = Some((stage, boundary, content_dir, options, through)),
                Err(error) => { eprintln!("{error}"); std::process::exit(1); }
            }
        } else {
            match ColdCalls::build(&content_dir, &matrix_file, &label) {
                Ok(built) => cold = Some(built),
                Err(error) => { eprintln!("cold admission failed: {error}"); std::process::exit(1); }
            }
        }
    }
    let lines: Vec<Value> = std::io::stdin().lock().lines().filter_map(|l| l.ok()).filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(&l).expect("a recording line")).collect();
    let partial_fixtures = partial.as_ref().map(|_| fixtures(&lines));
    let mut partial_stopped: Option<String> = None;
    // After the boundary the cold stages own the world: the recording is
    // only compared, never replayed into it.
    let mut comparing_only = false;
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
                        Ok(_) => { dump_bank(*id, &out); let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect(); Some((out.len(), hex(&sha256(&bytes)))) }
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
                let admission = line["phase"].as_str() == Some("admission");
                let takes_over = partial.as_ref().is_some_and(|p| admission && match p.1 {
                    Boundary::Before(name) => name == export,
                    Boundary::After(_) => armed,
                });
                if let Some((_, Boundary::After(name), _, _, _)) = partial.as_ref() { if admission && *name == export { armed = true; } }
                if let Some((stage, _, content_dir, options, through)) = partial.as_ref().filter(|_| takes_over) {
                    let files = DirFiles::new(content_dir);
                    let fixtures = partial_fixtures.as_ref().expect("fixtures decoded");
                    match context.owned(|world| bfe_sim::world::admit::partial::run(world, &files, options, *stage, *through, fixtures)) {
                        Ok(Ok(run)) => {
                            partial_stopped = run.stopped.map(|error| error.to_string());
                            cold = Some(ColdCalls::partial(run.calls));
                        }
                        Ok(Err(error)) => { eprintln!("partial admission from {stage:?} failed: {error}"); std::process::exit(1); }
                        Err(error) => { eprintln!("partial admission from {stage:?} poisoned the world: {}", error.message()); std::process::exit(1); }
                    }
                    partial = None;
                    comparing_only = true;
                }
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
                } else if comparing_only && returned_addr != 0 && FILLS.contains(&export) {
                    // The cold stages own the world now, so the bank's size is
                    // not asked of a body: the recorded bytes at the returned
                    // address, as long as the cold call's scratch, after the
                    // writes the next line carries (the diff lists only the
                    // bytes the host changed; a fresh buffer is zero).
                    let len = cold.as_ref().and_then(|c| c.calls.get(c.next)).filter(|(_, name, _)| *name == export).map_or(0, |(_, _, call)| call.scratch.len());
                    if len > 0 {
                        if KEPT_BUFFERS.contains(&export) {
                            if let Some((old_addr, old_len)) = kept.get(export).copied() {
                                if old_addr != returned_addr { let moved = memory.read(old_addr, old_len.min(len)); memory.write(returned_addr, moved); }
                                if len > old_len { memory.write(returned_addr + old_len as u64, vec![0; len - old_len]); }
                            } else { memory.write(returned_addr, vec![0; len]); }
                            kept.insert(export.to_string(), (returned_addr, len));
                        } else if !VIEW_BANKS.contains(&export) { memory.write(returned_addr, vec![0; len]); }
                        if let Some(next) = lines.get(index + 1) {
                            for write in next["writes"].as_array().into_iter().flatten() { memory.write(write["offset"].as_u64().unwrap_or(0), base64(write["bytes"].as_str().unwrap_or(""))); }
                        }
                        call.scratch = memory.read(returned_addr, len);
                    }
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
                    // In the spawn phase, world.fire at tick 161 may intern "consumed"
                    // through bridge.strings before the recorded spawn call runs.
                    if line["phase"].as_str() == Some("spawn")
                        && context.string_count().ok().is_some_and(|n| returned_addr == (n + 1) as u64)
                    {
                        let mut prior = Call::default();
                        prior.strings.push("consumed".encode_utf16().collect());
                        let _ = context.invoke_export("sim_call_room", &prior);
                    }
                    let text = line["intern"]["text"].as_str().unwrap_or("");
                    call.scratch = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                } else if let Some(intern) = line["intern"].as_object() { if let Some(text) = intern.get("text").and_then(|t| t.as_str()) { call.strings.push(text.encode_utf16().collect()); } }
                if let Some(cold) = cold.as_mut() { if line["phase"].as_str() == Some("admission") { cold.compare(line, export, &call); } }
                if comparing_only {
                    // Every cold call matched: the partial proof is complete.
                    if cold.as_ref().is_some_and(|c| c.next == c.calls.len() || c.stopped) { break; }
                    continue;
                }
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
    let mut summary = json!({"calls": calls, "numericCompared": compared, "numericMismatches": mismatches, "blocksCompared": blocks_compared, "blockMismatches": block_mismatches, "firstDifferences": first, "mismatchesByExport": by_export, "notes": notes, "noBody": nobody, "failures": failures, "banksSame": same, "banks": banks});
    let mut cold_differs = false;
    if let Some(cold) = &cold {
        cold_differs = !cold.differences.is_empty() || cold.next != cold.calls.len();
        if let (Some(all), Some(part)) = (summary.as_object_mut(), cold.summary().as_object()) { all.extend(part.clone()); }
    }
    if comparing_only || partial.is_some() {
        if let Some(all) = summary.as_object_mut() {
            all.insert("partialStopped".into(), json!(partial_stopped));
            all.insert("partialReached".into(), json!(comparing_only));
        }
        // A partial proof is judged on its own calls alone.
        println!("{summary}");
        if cold_differs || partial_stopped.is_some() || !comparing_only { std::process::exit(1); }
        return;
    }
    println!("{summary}");
    let _ = context.dispose();
    let unexpected = nobody.keys().any(|name| !SKIPPED.contains(&name.as_str()));
    if mismatches != 0 || block_mismatches != 0 || !failures.is_empty() || same != bank::ALL.len() || unexpected || cold_differs { std::process::exit(1); }
}
