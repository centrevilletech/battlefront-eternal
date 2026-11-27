//! Recording host for Battlefront II (2005) mission setup scripts.
//!
//! Loads a script from a `.lvl` archive, binds the host functions that the
//! archive's own scripts call (derived from their bytecode, see
//! [`archive_host_functions`]) as real callable Lua functions under a named
//! [`ImportProfile`] (`"pc-fresh"`), serves `ScriptCB_DoFile("name")` from the
//! same archive, runs the top-level chunk followed by `ScriptPreInit` and
//! `ScriptInit` if defined (never `ScriptPostLoad`), records any unanswered
//! value-returning queries and the undefined globals the scripts test, and
//! emits a lossless JSON manifest (schema 4).
//!
//! The crate ships no list of game names. The only ones it knows are the three
//! setup phases, `ScriptCB_DoFile` (served from the archive) and `OpenAudioStream`
//! (sequential handles). Every other host function is derived from the archive's
//! own scripts; a value the preparation step wants a query or global to have is
//! supplied as data ([`RecorderOptions::answers`]), and a value-returning call that
//! nothing answers is reported in `unresolved_queries`.

use std::collections::{BTreeMap, BTreeSet};

use crate::archive::ScriptArchive;
use crate::chunk::{load_chunk_with_limits, ChunkLimits, Constant, Proto};
use crate::error::ScriptError;
use crate::numfmt;
use crate::opcode::{OpCode, MAXSTACK};
use crate::value::{TableId, Value};
use crate::vm::{CallCtx, OutputSink, Vm, VmLimits};

/// JSON schema version emitted by [`Manifest::to_json_pretty`]. Version 3 added
/// `probed_globals` and the `up` field of cycle markers; version 4 adds
/// `skipped_scripts`.
pub const MANIFEST_SCHEMA_VERSION: u32 = 4;

/// Reserved tag key used by the lossless wrapper objects.
const TAGGED_KEY: &str = "$bfe";

/// Named host environment profile used when recording a mission setup manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportProfile {
    /// Fresh import profile (`"pc-fresh"`):
    ///
    /// - `ScriptCB_DoFile("name")` loads and runs `name` from the same archive
    /// - `OpenAudioStream(...)` returns deterministic sequential handles `1, 2, ...`
    /// - Every [`RecorderOptions::answers`] entry sets its global or answers its call
    /// - Every host function the archive's scripts call (see [`scan_archive`]) and
    ///   every name in [`RecorderOptions::extra_host_functions`] is bound as a real
    ///   callable Lua function that records its call (a value-returning one is
    ///   listed in `unresolved_queries`), while other undefined script globals and
    ///   include guards remain `nil`.
    #[default]
    PcFresh,
}

impl ImportProfile {
    /// Stable profile identifier written to `"profile"` in manifests.
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportProfile::PcFresh => "pc-fresh",
        }
    }
}

/// Cumulative safety limits applied during mission manifest recording and JSON serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecorderLimits {
    /// Maximum host calls recorded across all phases.
    pub max_calls: usize,
    /// Maximum nested table depth snapshotted per value.
    pub max_depth: usize,
    /// Maximum cumulative value nodes snapshotted across the entire manifest
    /// (bounds shared-DAG tree expansion).
    pub max_nodes: usize,
    /// Maximum serialized JSON output size in bytes (also bounds cumulative
    /// snapshotted string payload bytes).
    pub max_output_bytes: usize,
}

impl Default for RecorderLimits {
    fn default() -> Self {
        Self {
            max_calls: 50_000,
            max_depth: 64,
            max_nodes: 200_000,
            max_output_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Full configuration for [`record_manifest_with_config`].
#[derive(Debug, Clone)]
pub struct RecorderOptions {
    /// Host behavior profile recorded into `"profile"`.
    pub profile: ImportProfile,
    /// VM execution safety limits.
    pub vm_limits: VmLimits,
    /// Recorder snapshot and output limits.
    pub recorder_limits: RecorderLimits,
    /// Host function names to bind as callable recording functions besides the
    /// ones derived from the archive. A host that implements a function the
    /// scripts only call behind an existence test (`if Name then Name() end`)
    /// lists it here so the guarded call runs and is recorded.
    pub extra_host_functions: Vec<String>,
    /// Values the preparation step supplies: globals set before the chunk runs and
    /// host calls answered with a value (recorded, not listed as unresolved).
    pub answers: Vec<ProfileAnswer>,
    /// A scan of the archive made once with [`scan_archive`] and reused for every
    /// entry recorded from it; `None` scans the archive for this recording.
    pub scan: Option<std::rc::Rc<ArchiveScan>>,
}

impl Default for RecorderOptions {
    fn default() -> Self {
        Self {
            profile: ImportProfile::PcFresh,
            vm_limits: VmLimits::default(),
            recorder_limits: RecorderLimits::default(),
            extra_host_functions: Vec::new(),
            answers: Vec::new(),
            scan: None,
        }
    }
}

/// A value the preparation step supplies for a name the scripts read or call. The
/// value must be `nil`, a boolean, a number or a string.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileAnswer {
    /// A global variable set before the chunk runs.
    Global { name: String, value: RecordedValue },
    /// A host function that records its call and returns `value`.
    Call { name: String, value: RecordedValue },
}

/// Result of [`scan_archive`]: the host functions the archive's scripts call, and
/// the members that could not be decoded (left out of the scan).
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveScan {
    /// Sorted host function names (see [`scan_archive`]).
    pub host_functions: Vec<String>,
    /// Archive members that failed to load, with the error, in archive order.
    pub skipped: Vec<(String, String)>,
    /// Guarded globals per canonical archive member, computed once during preparation.
    pub tested_by_script: BTreeMap<String, BTreeSet<String>>,
}

/// A recorded Lua value serialized losslessly into manifest JSON.
///
/// A snapshot is a tree: a table reached twice (for example `{s, s}`) is written
/// at each place it occurs, and only a table that contains itself, directly or
/// through its children, becomes a [`RecordedValue::Cycle`] marker. Functions
/// and userdata are opaque markers, so two different functions used as keys
/// give two entries that look alike (a general table keeps both, in order).
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedValue {
    Nil,
    Bool(bool),
    /// Lua 5.0 `f32`. Finite numbers other than `-0.0` serialize as plain JSON
    /// numbers; `-0.0`, `nan`, `inf`, and `-inf` serialize as
    /// `{"$bfe": "number", "special": "..."}`.
    Number(f32),
    /// Valid UTF-8 Lua string (plain JSON string).
    String(String),
    /// Arbitrary non-UTF-8 Lua byte string (`{"$bfe": "bytes", "hex": "..."}`).
    Bytes(Vec<u8>),
    /// Pure contiguous 1-based Lua array (`1..=n`, no other keys, `n >= 1`).
    Array(Vec<RecordedValue>),
    /// Plain UTF-8-string-keyed table containing no `"$bfe"` key (or empty `{}`).
    Object(Vec<(String, RecordedValue)>),
    /// General table with non-string, non-UTF-8, mixed, or `"$bfe"` keys
    /// (`{"$bfe": "table", "entries": [[k, v], ...]}`), preserving key types and order.
    Table(Vec<(RecordedValue, RecordedValue)>),
    /// Marker for a Lua closure or native function (`{"$bfe": "function"}`).
    Function,
    /// Marker for host userdata (`{"$bfe": "userdata"}`).
    Userdata,
    /// Marker for a table that is one of its own ancestors in the snapshot
    /// (`{"$bfe": "cycle", "up": n}`). `up` counts the tables from the marker to
    /// the one it refers to: 1 is the table holding the marker, 2 its parent.
    Cycle {
        /// Distance to the referenced ancestor table (at least 1).
        up: usize,
    },
}

/// One recorded host call observed during mission initialization.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedCall {
    /// Execution phase: `"chunk"`, `"ScriptPreInit"`, or `"ScriptInit"`.
    pub phase: String,
    /// Global function name called by the script.
    pub name: String,
    /// Positional arguments passed to the call.
    pub args: Vec<RecordedValue>,
}

/// A host call whose return value was requested by the script (`C != 1`),
/// but not answered by the active [`ImportProfile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedQuery {
    /// Execution phase in which the query occurred.
    pub phase: String,
    /// Global host function name that was queried.
    pub name: String,
    /// Zero-based index of this call inside [`Manifest::calls`].
    pub call_index: usize,
    /// Number of results requested by the caller (`None` for open `LUA_MULTRET`).
    pub expected_results: Option<usize>,
}

/// Complete ordered manifest produced by running a mission script's setup phases.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// Schema version (`2`).
    pub schema: u32,
    /// Import profile name (for example `"pc-fresh"`).
    pub profile: String,
    /// Canonical script name as stored in the archive.
    pub script: String,
    /// Ordered list of scripts loaded (entry script plus `ScriptCB_DoFile` targets).
    pub loaded_scripts: Vec<String>,
    /// Phases that executed (`"chunk"`, and `"ScriptPreInit"` / `"ScriptInit"` if defined).
    pub phases: Vec<String>,
    /// Value-returning host calls not answered by the active profile.
    pub unresolved_queries: Vec<UnresolvedQuery>,
    /// Globals the loaded scripts test in a condition or comparison (for example
    /// `if OptionalHook then`, including `_G.Name` and `rawget(_G, "Name")` with a
    /// constant name) that were still undefined when recording finished, sorted.
    /// A guarded call of such a name did not run.
    pub probed_globals: Vec<String>,
    /// Archive members the host-function scan could not decode, with the error (their
    /// own calls are missing from the derived bindings).
    pub skipped_scripts: Vec<(String, String)>,
    /// Ordered host calls recorded across all executed phases.
    pub calls: Vec<RecordedCall>,
}

impl Manifest {
    /// Serialize this manifest to a deterministic pretty-printed JSON
    /// string using the default output byte limit.
    pub fn to_json_pretty(&self) -> String {
        self.to_json_pretty_bounded(RecorderLimits::default().max_output_bytes)
            .expect("manifest fits within default output byte limit")
    }

    /// Serialize this manifest to a deterministic pretty-printed JSON
    /// string using an iterative serializer, failing cleanly if output exceeds `max_bytes`.
    pub fn to_json_pretty_bounded(&self, max_bytes: usize) -> Result<String, ScriptError> {
        let mut out = String::with_capacity(4096.min(max_bytes));
        push_bounded(&mut out, "{\n", max_bytes)?;
        push_bounded(
            &mut out,
            &format!("  \"schema\": {},\n", self.schema),
            max_bytes,
        )?;
        push_bounded(&mut out, "  \"profile\": ", max_bytes)?;
        push_json_str_bounded(&self.profile, &mut out, max_bytes)?;
        push_bounded(&mut out, ",\n  \"script\": ", max_bytes)?;
        push_json_str_bounded(&self.script, &mut out, max_bytes)?;
        push_bounded(&mut out, ",\n  \"loaded_scripts\": [", max_bytes)?;
        for (i, s) in self.loaded_scripts.iter().enumerate() {
            if i > 0 {
                push_bounded(&mut out, ", ", max_bytes)?;
            }
            push_json_str_bounded(s, &mut out, max_bytes)?;
        }
        push_bounded(&mut out, "],\n  \"phases\": [", max_bytes)?;
        for (i, p) in self.phases.iter().enumerate() {
            if i > 0 {
                push_bounded(&mut out, ", ", max_bytes)?;
            }
            push_json_str_bounded(p, &mut out, max_bytes)?;
        }
        push_bounded(&mut out, "],\n  \"unresolved_queries\": [", max_bytes)?;
        if !self.unresolved_queries.is_empty() {
            push_bounded(&mut out, "\n", max_bytes)?;
            for (i, q) in self.unresolved_queries.iter().enumerate() {
                if i > 0 {
                    push_bounded(&mut out, ",\n", max_bytes)?;
                }
                push_bounded(&mut out, "    {\"phase\": ", max_bytes)?;
                push_json_str_bounded(&q.phase, &mut out, max_bytes)?;
                push_bounded(&mut out, ", \"name\": ", max_bytes)?;
                push_json_str_bounded(&q.name, &mut out, max_bytes)?;
                push_bounded(
                    &mut out,
                    &format!(", \"call_index\": {}, \"expected_results\": ", q.call_index),
                    max_bytes,
                )?;
                match q.expected_results {
                    Some(n) => push_bounded(&mut out, &n.to_string(), max_bytes)?,
                    None => push_bounded(&mut out, "null", max_bytes)?,
                }
                push_bounded(&mut out, "}", max_bytes)?;
            }
            push_bounded(&mut out, "\n  ", max_bytes)?;
        }
        push_bounded(&mut out, "],\n  \"probed_globals\": [", max_bytes)?;
        for (i, name) in self.probed_globals.iter().enumerate() {
            if i > 0 {
                push_bounded(&mut out, ", ", max_bytes)?;
            }
            push_json_str_bounded(name, &mut out, max_bytes)?;
        }
        push_bounded(&mut out, "],\n  \"skipped_scripts\": [", max_bytes)?;
        for (i, (name, error)) in self.skipped_scripts.iter().enumerate() {
            if i > 0 {
                push_bounded(&mut out, ", ", max_bytes)?;
            }
            push_bounded(&mut out, "{\"script\": ", max_bytes)?;
            push_json_str_bounded(name, &mut out, max_bytes)?;
            push_bounded(&mut out, ", \"error\": ", max_bytes)?;
            push_json_str_bounded(error, &mut out, max_bytes)?;
            push_bounded(&mut out, "}", max_bytes)?;
        }
        push_bounded(&mut out, "],\n  \"calls\": [", max_bytes)?;
        if !self.calls.is_empty() {
            push_bounded(&mut out, "\n", max_bytes)?;
            for (i, call) in self.calls.iter().enumerate() {
                if i > 0 {
                    push_bounded(&mut out, ",\n", max_bytes)?;
                }
                push_bounded(&mut out, "    {\n      \"phase\": ", max_bytes)?;
                push_json_str_bounded(&call.phase, &mut out, max_bytes)?;
                push_bounded(&mut out, ",\n      \"name\": ", max_bytes)?;
                push_json_str_bounded(&call.name, &mut out, max_bytes)?;
                push_bounded(&mut out, ",\n      \"args\": [", max_bytes)?;
                for (j, arg) in call.args.iter().enumerate() {
                    if j > 0 {
                        push_bounded(&mut out, ", ", max_bytes)?;
                    }
                    write_value_json_bounded(arg, &mut out, 6, max_bytes)?;
                }
                push_bounded(&mut out, "]\n    }", max_bytes)?;
            }
            push_bounded(&mut out, "\n  ", max_bytes)?;
        }
        push_bounded(&mut out, "]\n}\n", max_bytes)?;
        Ok(out)
    }
}

struct RecorderHost<'a> {
    archive: &'a ScriptArchive<'a>,
    profile: ImportProfile,
    current_phase: String,
    loaded_scripts: Vec<String>,
    calls: Vec<RecordedCall>,
    unresolved_queries: Vec<UnresolvedQuery>,
    limits: RecorderLimits,
    total_nodes: usize,
    total_string_bytes: usize,
    next_audio_stream_handle: u32,
}

/// Record the setup manifest of `script_name` from `archive` using the default
/// [`ImportProfile::PcFresh`] profile and default limits.
///
/// Loads `script_name`, registers real callable bindings for the host functions
/// the archive's scripts call, provides `ScriptCB_DoFile` backed by `archive`,
/// executes the chunk, then calls
/// `ScriptPreInit()` and `ScriptInit()` if defined as global functions.
/// `ScriptPostLoad` is intentionally never called.
pub fn record_manifest(
    archive: &ScriptArchive<'_>,
    script_name: &str,
) -> Result<Manifest, ScriptError> {
    record_manifest_with_config(archive, script_name, RecorderOptions::default())
}

/// Same as [`record_manifest`], using custom [`VmLimits`].
pub fn record_manifest_with_limits(
    archive: &ScriptArchive<'_>,
    script_name: &str,
    limits: VmLimits,
) -> Result<Manifest, ScriptError> {
    record_manifest_with_config(
        archive,
        script_name,
        RecorderOptions {
            vm_limits: limits,
            ..RecorderOptions::default()
        },
    )
}

/// Same as [`record_manifest`], using explicit [`VmLimits`] and [`RecorderLimits`].
pub fn record_manifest_with_options(
    archive: &ScriptArchive<'_>,
    script_name: &str,
    limits: VmLimits,
    recorder_limits: RecorderLimits,
) -> Result<Manifest, ScriptError> {
    record_manifest_with_config(
        archive,
        script_name,
        RecorderOptions {
            vm_limits: limits,
            recorder_limits,
            ..RecorderOptions::default()
        },
    )
}

/// Record a mission setup manifest with explicit [`RecorderOptions`].
pub fn record_manifest_with_config(
    archive: &ScriptArchive<'_>,
    script_name: &str,
    options: RecorderOptions,
) -> Result<Manifest, ScriptError> {
    let entry = archive.get(script_name).ok_or_else(|| {
        ScriptError::runtime(format!("script '{script_name}' not found in archive"))
    })?;
    let canonical_name = entry.name.clone();
    let entry_body = entry.body;

    let mut host = RecorderHost {
        archive,
        profile: options.profile,
        current_phase: "chunk".to_string(),
        loaded_scripts: vec![canonical_name.clone()],
        calls: Vec::new(),
        unresolved_queries: Vec::new(),
        limits: options.recorder_limits,
        total_nodes: 0,
        total_string_bytes: 0,
        next_audio_stream_handle: 0,
    };

    let scan = match &options.scan {
        Some(scan) => std::rc::Rc::clone(scan),
        None => std::rc::Rc::new(scan_archive(archive, recorder_chunk_limits(&options.vm_limits))),
    };
    let mut vm: Vm<RecorderHost<'_>> = Vm::with_limits(options.vm_limits)?;
    vm.set_output_sink(OutputSink::Discard);
    vm.open_stdlib()?;
    install_profile_bindings(
        &mut vm,
        options.profile,
        &options.answers,
        &scan.host_functions,
        &options.extra_host_functions,
    )?;

    let mut phases = vec!["chunk".to_string()];
    let mut results = Vec::new();

    let chunk_cl = vm.load_chunk_bytes(entry_body)?;
    vm.call_into(&mut host, Value::Closure(chunk_cl), &[], &mut results)?;

    for phase in ["ScriptPreInit", "ScriptInit"] {
        let gval = vm.get_global(phase)?;
        if gval.is_function() {
            host.current_phase = phase.to_string();
            phases.push(phase.to_string());
            vm.call_into(&mut host, gval, &[], &mut results)?;
        }
    }

    let mut probed_globals = Vec::new();
    let tested: BTreeSet<&String> = host
        .loaded_scripts
        .iter()
        .filter_map(|name| scan.tested_by_script.get(name))
        .flatten()
        .collect();
    for name in tested {
        if vm.get_global(name)?.is_nil() {
            probed_globals.push(name.clone());
        }
    }

    Ok(Manifest {
        schema: MANIFEST_SCHEMA_VERSION,
        profile: host.profile.as_str().to_string(),
        script: canonical_name,
        loaded_scripts: host.loaded_scripts,
        phases,
        unresolved_queries: host.unresolved_queries,
        probed_globals,
        skipped_scripts: scan.skipped.clone(),
        calls: host.calls,
    })
}

fn bind_generic_host_fn(
    vm: &mut Vm<RecorderHost<'_>>,
    name: &str,
) -> Result<(), ScriptError> {
    if !vm.get_global(name)?.is_nil() {
        return Ok(());
    }
    let name_sid = vm.intern_str(name)?;
    let fid = vm.create_native_fn_with_upval_slice(record_bound_host_call, &[Value::String(name_sid)])?;
    // Rooted until stored: storing the global may allocate, and so collect.
    vm.with_temp_roots(&[Value::NativeFn(fid)], |vm| {
        vm.set_global(name, Value::NativeFn(fid))
    })
}

/// The script value of a supplied answer (scalars only).
fn answer_value(vm: &mut Vm<RecorderHost<'_>>, value: &RecordedValue) -> Result<Value, ScriptError> {
    Ok(match value {
        RecordedValue::Nil => Value::Nil,
        RecordedValue::Bool(b) => Value::Bool(*b),
        RecordedValue::Number(n) => Value::Number(*n),
        RecordedValue::String(s) => Value::String(vm.intern_str(s)?),
        RecordedValue::Bytes(b) => Value::String(vm.intern_bytes(b)?),
        _ => {
            return Err(ScriptError::InvalidConfig(
                "a profile answer must be nil, a boolean, a number or a string",
            ))
        }
    })
}

fn install_profile_bindings(
    vm: &mut Vm<RecorderHost<'_>>,
    profile: ImportProfile,
    answers: &[ProfileAnswer],
    derived_host_functions: &[String],
    extra_host_functions: &[String],
) -> Result<(), ScriptError> {
    match profile {
        ImportProfile::PcFresh => {
            vm.register_fn("ScriptCB_DoFile", host_dofile)?;
            vm.register_fn("OpenAudioStream", host_open_audio_stream)?;
        }
    }
    // Supplied answers come next, so the derived generic bindings do not replace them.
    for answer in answers {
        match answer {
            // Each new value stays rooted across the allocations that follow it.
            ProfileAnswer::Global { name, value } => {
                let v = answer_value(vm, value)?;
                vm.with_temp_roots(&[v], |vm| vm.set_global(name, v))?;
            }
            ProfileAnswer::Call { name, value } => {
                let name_v = Value::String(vm.intern_str(name)?);
                vm.with_temp_roots(&[name_v], |vm| {
                    let v = answer_value(vm, value)?;
                    vm.with_temp_roots(&[v], |vm| {
                        let fid =
                            vm.create_native_fn_with_upval_slice(record_answered_call, &[name_v, v])?;
                        vm.with_temp_roots(&[Value::NativeFn(fid)], |vm| {
                            vm.set_global(name, Value::NativeFn(fid))
                        })
                    })
                })?;
            }
        }
    }

    for name in derived_host_functions.iter().chain(extra_host_functions) {
        bind_generic_host_fn(vm, name)?;
    }
    Ok(())
}

/// Chunk limits for scripts the recorder loads: the default limits, capped by the
/// VM's memory limit.
fn recorder_chunk_limits(vm_limits: &VmLimits) -> ChunkLimits {
    let mut chunk_limits = ChunkLimits::default();
    chunk_limits.max_input_bytes = chunk_limits.max_input_bytes.min(vm_limits.memory_limit);
    chunk_limits.max_decoded_bytes = chunk_limits.max_decoded_bytes.min(vm_limits.memory_limit);
    chunk_limits
}

/// Host function names derived from an archive's own scripts.
///
/// A name is included when some script of the archive calls the global of that
/// name (directly, after `local f = Name`, through `_G.Name` or a constant
/// `rawget(_G, "Name")`, or stores it in a table constructor or field to call
/// later) without also testing it, and no script of the archive assigns it.
/// The shipped game's scripts run those calls unguarded, so its host provides
/// them. Names a script only calls behind an existence test are not included
/// (see [`RecorderOptions::extra_host_functions`]), nor are the globals of the
/// Lua standard library. Scripts that fail to load are skipped. The result is
/// sorted.
///
/// The scan follows registers in instruction order and does not follow jumps or
/// names built at run time, so it is a heuristic: a call through a name computed
/// by the script is not seen.
pub fn archive_host_functions(archive: &ScriptArchive<'_>, limits: ChunkLimits) -> Vec<String> {
    scan_archive(archive, limits).host_functions
}

/// [`archive_host_functions`] with the members that failed to decode reported instead
/// of silently left out. Scan an archive once and pass the result in
/// [`RecorderOptions::scan`] when recording several of its entries.
pub fn scan_archive(archive: &ScriptArchive<'_>, limits: ChunkLimits) -> ArchiveScan {
    let mut called = BTreeSet::new();
    let mut written = BTreeSet::new();
    let mut skipped = Vec::new();
    let mut tested_by_script = BTreeMap::new();
    for script in archive.scripts() {
        let proto = match load_chunk_with_limits(script.body, limits) {
            Ok(proto) => proto,
            Err(e) => {
                skipped.push((script.name.clone(), e.to_string()));
                continue;
            }
        };
        let mut summary = GlobalUseSummary::default();
        scan_proto_globals(&proto, &mut summary);
        for name in &summary.called_or_listed {
            if !summary.tested.contains(name) {
                called.insert(name.clone());
            }
        }
        written.append(&mut summary.written);
        tested_by_script
            .entry(script.name.clone())
            .or_insert(summary.tested);
    }
    let host_functions = called
        .difference(&written)
        .filter(|name| !crate::stdlib::is_global(name))
        .cloned()
        .collect();
    ArchiveScan {
        host_functions,
        skipped,
        tested_by_script,
    }
}

#[derive(Default)]
struct GlobalUseSummary {
    /// Globals assigned anywhere in the chunk tree (`SETGLOBAL`, `_G.Name = v`,
    /// `rawset(_G, "Name", v)`).
    written: BTreeSet<String>,
    /// Globals inspected as boolean/equality/ordering conditions (guards/hooks).
    tested: BTreeSet<String>,
    /// Globals called, or stored in a table to be called through it.
    called_or_listed: BTreeSet<String>,
}

fn proto_const_string(proto: &Proto, idx: usize) -> Option<String> {
    match proto.constants.get(idx) {
        Some(Constant::String(bytes)) => {
            let s = numfmt::cstr_prefix(bytes);
            std::str::from_utf8(s).ok().map(str::to_string)
        }
        _ => None,
    }
}

/// The string named by an `RK` operand: a string constant, or a register that
/// holds one (after `LOADK`).
fn rk_string(proto: &Proto, operand: usize, reg_const: &BTreeMap<usize, String>) -> Option<String> {
    if operand >= MAXSTACK {
        proto_const_string(proto, operand - MAXSTACK)
    } else {
        reg_const.get(&operand).cloned()
    }
}

fn scan_proto_globals(proto: &Proto, summary: &mut GlobalUseSummary) {
    // Registers holding the value of a named global, and registers holding a
    // string constant (for `_G[name]`, `rawget(_G, name)` and `rawset`).
    let mut reg_global: BTreeMap<usize, String> = BTreeMap::new();
    let mut reg_const: BTreeMap<usize, String> = BTreeMap::new();
    let mut pc = 0usize;
    while pc < proto.code.len() {
        let ins = proto.code[pc];
        let Some(op) = ins.op() else {
            pc += 1;
            continue;
        };
        let a = ins.a();
        let holds = |reg: &BTreeMap<usize, String>, r: usize, name: &str| {
            reg.get(&r).is_some_and(|n| n == name)
        };
        match op {
            OpCode::GetGlobal => {
                reg_const.remove(&a);
                if let Some(name) = proto_const_string(proto, ins.bx()) {
                    reg_global.insert(a, name);
                } else {
                    reg_global.remove(&a);
                }
            }
            OpCode::SetGlobal => {
                if let Some(name) = proto_const_string(proto, ins.bx()) {
                    summary.written.insert(name);
                }
            }
            OpCode::LoadK => {
                reg_global.remove(&a);
                if let Some(text) = proto_const_string(proto, ins.bx()) {
                    reg_const.insert(a, text);
                } else {
                    reg_const.remove(&a);
                }
            }
            OpCode::Move => {
                match reg_global.get(&ins.b()).cloned() {
                    Some(name) => reg_global.insert(a, name),
                    None => reg_global.remove(&a),
                };
                match reg_const.get(&ins.b()).cloned() {
                    Some(text) => reg_const.insert(a, text),
                    None => reg_const.remove(&a),
                };
            }
            OpCode::GetTable => {
                // `_G.Name` and `_G["Name"]` read the global `Name`.
                let name = if holds(&reg_global, ins.b(), "_G") {
                    rk_string(proto, ins.c(), &reg_const)
                } else {
                    None
                };
                reg_const.remove(&a);
                match name {
                    Some(name) => reg_global.insert(a, name),
                    None => reg_global.remove(&a),
                };
            }
            OpCode::SetTable => {
                // `_G.Name = v` assigns the global; storing a global's value in a
                // field (`{fn = Name}`) lets the script call it through the table.
                if holds(&reg_global, a, "_G") {
                    if let Some(name) = rk_string(proto, ins.b(), &reg_const) {
                        summary.written.insert(name);
                    }
                }
                let value = ins.c();
                if value < MAXSTACK {
                    if let Some(name) = reg_global.get(&value) {
                        summary.called_or_listed.insert(name.clone());
                    }
                }
            }
            OpCode::Call | OpCode::TailCall => {
                let mut result = None;
                if let Some(name) = reg_global.get(&a) {
                    // `rawget(_G, "Name")` (two arguments) and `rawset(_G, "Name", v)`.
                    let target = reg_const.get(&(a + 2)).cloned();
                    if holds(&reg_global, a + 1, "_G") {
                        if name == "rawget" && ins.b() == 3 && ins.c() != 1 {
                            result = target;
                        } else if name == "rawset" {
                            if let Some(target) = target {
                                summary.written.insert(target);
                            }
                        }
                    }
                    summary.called_or_listed.insert(name.clone());
                }
                reg_global.retain(|&reg, _| reg < a);
                reg_const.retain(|&reg, _| reg < a);
                if let Some(name) = result {
                    reg_global.insert(a, name);
                }
            }
            OpCode::SetList | OpCode::SetListO => {
                // e.g. `local f = {OptionalAPI}; f[1](2)` stores `GETGLOBAL` in `R(A+1..)`.
                for (&reg, name) in &reg_global {
                    if reg > a {
                        summary.called_or_listed.insert(name.clone());
                    }
                }
                reg_global.retain(|&reg, _| reg <= a);
                reg_const.retain(|&reg, _| reg <= a);
            }
            OpCode::Test | OpCode::Not => {
                if let Some(name) = reg_global.get(&ins.b()) {
                    summary.tested.insert(name.clone());
                }
                reg_global.remove(&a);
                reg_const.remove(&a);
            }
            OpCode::Eq | OpCode::Lt | OpCode::Le => {
                if let Some(name) = reg_global.get(&ins.b()) {
                    summary.tested.insert(name.clone());
                }
                if let Some(name) = reg_global.get(&ins.c()) {
                    summary.tested.insert(name.clone());
                }
            }
            OpCode::LoadNil => {
                let b = ins.b();
                reg_global.retain(|&reg, _| reg < a || reg > b);
                reg_const.retain(|&reg, _| reg < a || reg > b);
            }
            OpCode::Closure => {
                let nup = proto
                    .protos
                    .get(ins.bx())
                    .map_or(0, |child| child.nups as usize);
                reg_global.remove(&a);
                reg_const.remove(&a);
                pc += nup;
            }
            OpCode::LoadBool
            | OpCode::GetUpval
            | OpCode::NewTable
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Pow
            | OpCode::Unm
            | OpCode::Concat => {
                reg_global.remove(&a);
                reg_const.remove(&a);
            }
            OpCode::SelfOp => {
                for r in [a, a + 1] {
                    reg_global.remove(&r);
                    reg_const.remove(&r);
                }
            }
            OpCode::TForLoop => {
                reg_global.retain(|&reg, _| reg < a);
                reg_const.retain(|&reg, _| reg < a);
            }
            _ => {}
        }
        pc += 1;
    }

    for child in proto.protos.iter() {
        scan_proto_globals(child, summary);
    }
}

fn push_recorded_call(
    ctx: &mut CallCtx<'_, RecorderHost<'_>>,
    name: String,
) -> Result<usize, ScriptError> {
    if ctx.host().calls.len() >= ctx.host().limits.max_calls {
        return Err(ScriptError::ResourceLimit("recorder call limit exceeded"));
    }
    let args = snapshot_args_bounded(ctx)?;
    let phase = ctx.host().current_phase.clone();
    let call_index = ctx.host().calls.len();
    ctx.host_mut().calls.push(RecordedCall { phase, name, args });
    Ok(call_index)
}

fn host_dofile(ctx: &mut CallCtx<'_, RecorderHost<'_>>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let req_name = ctx.vm().string_latin1_unchecked(sid);

    push_recorded_call(ctx, "ScriptCB_DoFile".to_string())?;

    let script = ctx.host().archive.get(&req_name).ok_or_else(|| {
        ScriptError::runtime(format!(
            "ScriptCB_DoFile: script '{req_name}' not found in archive"
        ))
    })?;
    let canonical = script.name.clone();
    let body = script.body;
    ctx.host_mut().loaded_scripts.push(canonical);

    let cl = ctx.vm_mut().load_chunk_bytes(body)?;
    let mut out = Vec::new();
    ctx.call_into(Value::Closure(cl), &[], &mut out)?;
    for v in out {
        ctx.push_return(v)?;
    }
    Ok(())
}

/// A call answered by a supplied [`ProfileAnswer::Call`]: recorded, returning its value.
fn record_answered_call(ctx: &mut CallCtx<'_, RecorderHost<'_>>) -> Result<(), ScriptError> {
    let name = match ctx.upvalue(0) {
        Value::String(sid) => ctx.vm().string_latin1_unchecked(sid),
        _ => "<unknown>".to_string(),
    };
    let value = ctx.upvalue(1);
    push_recorded_call(ctx, name)?;
    ctx.push_return(value)
}

fn host_open_audio_stream(ctx: &mut CallCtx<'_, RecorderHost<'_>>) -> Result<(), ScriptError> {
    push_recorded_call(ctx, "OpenAudioStream".to_string())?;
    let host = ctx.host_mut();
    host.next_audio_stream_handle = host.next_audio_stream_handle.saturating_add(1);
    let handle = host.next_audio_stream_handle;
    ctx.push_return(Value::Number(handle as f32))
}

fn record_bound_host_call(ctx: &mut CallCtx<'_, RecorderHost<'_>>) -> Result<(), ScriptError> {
    let name = match ctx.upvalue(0) {
        Value::String(sid) => ctx.vm().string_latin1_unchecked(sid),
        _ => "<unknown>".to_string(),
    };
    let expected = ctx.expected_results();
    let call_index = push_recorded_call(ctx, name.clone())?;
    // Surface any call whose return value was expected by Lua (`C != 1`),
    // because the active profile did not answer it.
    if expected != Some(0) {
        let phase = ctx.host().current_phase.clone();
        ctx.host_mut().unresolved_queries.push(UnresolvedQuery {
            phase,
            name,
            call_index,
            expected_results: expected,
        });
    }
    Ok(())
}

fn snapshot_args_bounded(
    ctx: &mut CallCtx<'_, RecorderHost<'_>>,
) -> Result<Vec<RecordedValue>, ScriptError> {
    let n = ctx.arg_count();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let arg = ctx.arg(i);
        let (vm, host) = ctx.split();
        let rv = snapshot_value_bounded(
            vm,
            arg,
            host.limits,
            &mut host.total_nodes,
            &mut host.total_string_bytes,
        )?;
        out.push(rv);
    }
    Ok(out)
}

enum TableEntryState {
    Idle,
    ReadingKey(Value),
    ReadingValue(RecordedValue),
}

enum SnapFrame {
    Array {
        tid: TableId,
        remaining: std::vec::IntoIter<Value>,
        items: Vec<RecordedValue>,
    },
    Object {
        tid: TableId,
        pending_key: Option<String>,
        remaining: std::vec::IntoIter<(String, Value)>,
        pairs: Vec<(String, RecordedValue)>,
    },
    Table {
        tid: TableId,
        state: TableEntryState,
        remaining: std::vec::IntoIter<(Value, Value)>,
        pairs: Vec<(RecordedValue, RecordedValue)>,
    },
}

/// Iteratively convert a runtime Lua [`Value`] into a deterministic, lossless
/// [`RecordedValue`] while enforcing cumulative node, string-byte,
/// and nesting depth limits.
pub fn snapshot_value_bounded<H>(
    vm: &Vm<H>,
    root: Value,
    limits: RecorderLimits,
    total_nodes: &mut usize,
    total_string_bytes: &mut usize,
) -> Result<RecordedValue, ScriptError> {
    let mut ancestors: Vec<TableId> = Vec::new();
    let mut frames: Vec<SnapFrame> = Vec::new();
    let mut next_val: Option<Value> = Some(root);

    loop {
        if let Some(val) = next_val.take() {
            *total_nodes = total_nodes.saturating_add(1);
            if *total_nodes > limits.max_nodes {
                return Err(ScriptError::ResourceLimit("recorder node limit exceeded"));
            }

            let leaf = match val {
                Value::Nil => Some(RecordedValue::Nil),
                Value::Bool(b) => Some(RecordedValue::Bool(b)),
                Value::Number(n) => Some(RecordedValue::Number(n)),
                Value::String(sid) => {
                    let raw = vm.string_bytes_unchecked(sid);
                    match std::str::from_utf8(raw) {
                        Ok(utf8) => {
                            *total_string_bytes = total_string_bytes.saturating_add(utf8.len());
                            if *total_string_bytes > limits.max_output_bytes {
                                return Err(ScriptError::ResourceLimit(
                                    "recorder output byte limit exceeded",
                                ));
                            }
                            Some(RecordedValue::String(utf8.to_string()))
                        }
                        Err(_) => {
                            // Hex encoding takes 2 chars per raw byte.
                            let encoded_len = raw.len().saturating_mul(2);
                            *total_string_bytes = total_string_bytes.saturating_add(encoded_len);
                            if *total_string_bytes > limits.max_output_bytes {
                                return Err(ScriptError::ResourceLimit(
                                    "recorder output byte limit exceeded",
                                ));
                            }
                            Some(RecordedValue::Bytes(raw.to_vec()))
                        }
                    }
                }
                Value::Closure(_) | Value::NativeFn(_) => Some(RecordedValue::Function),
                Value::Userdata(_) => Some(RecordedValue::Userdata),
                Value::Table(tid) => {
                    if let Some(pos) = ancestors.iter().position(|&t| t == tid) {
                        Some(RecordedValue::Cycle {
                            up: ancestors.len() - pos,
                        })
                    } else {
                        if frames.len() >= limits.max_depth {
                            return Err(ScriptError::ResourceLimit(
                                "recorder nesting depth limit exceeded",
                            ));
                        }
                        let entries = vm.table_entries(tid)?;
                        let is_dense_array = !entries.is_empty()
                            && entries.iter().enumerate().all(|(idx, (k, _))| {
                                matches!(*k, Value::Number(n) if n == ((idx + 1) as f32))
                            });
                        ancestors.push(tid);
                        if is_dense_array {
                            let vals: Vec<Value> = entries.into_iter().map(|(_, v)| v).collect();
                            frames.push(SnapFrame::Array {
                                tid,
                                items: Vec::with_capacity(vals.len()),
                                remaining: vals.into_iter(),
                            });
                        } else {
                            // Use plain JSON object ONLY when every key is a valid UTF-8
                            // Lua string and none collides with the `$bfe` sentinel key.
                            let all_plain_utf8_keys = entries.iter().all(|(k, _)| {
                                if let Value::String(sid) = *k {
                                    let raw = vm.string_bytes_unchecked(sid);
                                    matches!(std::str::from_utf8(raw), Ok(s) if s != TAGGED_KEY)
                                } else {
                                    false
                                }
                            });
                            if all_plain_utf8_keys {
                                let mut kvs = Vec::with_capacity(entries.len());
                                for (k, v) in entries {
                                    let Value::String(sid) = k else {
                                        unreachable!();
                                    };
                                    let key_str = std::str::from_utf8(vm.string_bytes_unchecked(sid))
                                        .expect("validated UTF-8 key")
                                        .to_string();
                                    *total_string_bytes =
                                        total_string_bytes.saturating_add(key_str.len());
                                    if *total_string_bytes > limits.max_output_bytes {
                                        return Err(ScriptError::ResourceLimit(
                                            "recorder output byte limit exceeded",
                                        ));
                                    }
                                    kvs.push((key_str, v));
                                }
                                frames.push(SnapFrame::Object {
                                    tid,
                                    pending_key: None,
                                    pairs: Vec::with_capacity(kvs.len()),
                                    remaining: kvs.into_iter(),
                                });
                            } else {
                                frames.push(SnapFrame::Table {
                                    tid,
                                    state: TableEntryState::Idle,
                                    pairs: Vec::with_capacity(entries.len()),
                                    remaining: entries.into_iter(),
                                });
                            }
                        }
                        None
                    }
                }
            };

            if let Some(rv) = leaf {
                if let Some(done) = attach_recorded(&mut frames, &mut ancestors, rv, &mut next_val) {
                    return Ok(done);
                }
            } else if let Some(done) = advance_frames(&mut frames, &mut ancestors, &mut next_val) {
                return Ok(done);
            }
            continue;
        }

        if let Some(done) = advance_frames(&mut frames, &mut ancestors, &mut next_val) {
            return Ok(done);
        }
    }
}

fn attach_recorded(
    frames: &mut Vec<SnapFrame>,
    ancestors: &mut Vec<TableId>,
    mut finished: RecordedValue,
    next_val: &mut Option<Value>,
) -> Option<RecordedValue> {
    loop {
        match frames.last_mut() {
            None => return Some(finished),
            Some(SnapFrame::Array { items, .. }) => {
                items.push(finished);
            }
            Some(SnapFrame::Object {
                pending_key, pairs, ..
            }) => {
                let k = pending_key.take().expect("pending object key");
                pairs.push((k, finished));
            }
            Some(SnapFrame::Table { state, pairs, .. }) => {
                let prev = std::mem::replace(state, TableEntryState::Idle);
                match prev {
                    TableEntryState::ReadingKey(val_to_read) => {
                        *state = TableEntryState::ReadingValue(finished);
                        *next_val = Some(val_to_read);
                        return None;
                    }
                    TableEntryState::ReadingValue(key_rv) => {
                        pairs.push((key_rv, finished));
                    }
                    TableEntryState::Idle => unreachable!("unexpected idle table frame"),
                }
            }
        }
        if let Some(done) = advance_top_or_pop(frames, ancestors, next_val) {
            finished = done;
            continue;
        }
        return None;
    }
}

fn advance_frames(
    frames: &mut Vec<SnapFrame>,
    ancestors: &mut Vec<TableId>,
    next_val: &mut Option<Value>,
) -> Option<RecordedValue> {
    if let Some(finished) = advance_top_or_pop(frames, ancestors, next_val) {
        return attach_recorded(frames, ancestors, finished, next_val);
    }
    None
}

fn advance_top_or_pop(
    frames: &mut Vec<SnapFrame>,
    ancestors: &mut Vec<TableId>,
    next_val: &mut Option<Value>,
) -> Option<RecordedValue> {
    match frames.last_mut()? {
        SnapFrame::Array { remaining, .. } => {
            if let Some(v) = remaining.next() {
                *next_val = Some(v);
                return None;
            }
        }
        SnapFrame::Object {
            pending_key,
            remaining,
            ..
        } => {
            if let Some((k, v)) = remaining.next() {
                *pending_key = Some(k);
                *next_val = Some(v);
                return None;
            }
        }
        SnapFrame::Table {
            state, remaining, ..
        } => {
            if let Some((k, v)) = remaining.next() {
                *state = TableEntryState::ReadingKey(v);
                *next_val = Some(k);
                return None;
            }
        }
    }
    let popped = frames.pop().unwrap();
    let tid = match &popped {
        SnapFrame::Array { tid, .. }
        | SnapFrame::Object { tid, .. }
        | SnapFrame::Table { tid, .. } => *tid,
    };
    let popped_anc = ancestors.pop();
    debug_assert_eq!(popped_anc, Some(tid));
    Some(match popped {
        SnapFrame::Array { items, .. } => RecordedValue::Array(items),
        SnapFrame::Object { pairs, .. } => RecordedValue::Object(pairs),
        SnapFrame::Table { pairs, .. } => RecordedValue::Table(pairs),
    })
}

/// Convert a runtime Lua [`Value`] into a deterministic [`RecordedValue`] tree
/// using default [`RecorderLimits`].
pub fn snapshot_value<H>(
    vm: &Vm<H>,
    val: Value,
    _ancestors: &mut Vec<TableId>,
) -> Result<RecordedValue, ScriptError> {
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    snapshot_value_bounded(vm, val, RecorderLimits::default(), &mut nodes, &mut bytes)
}

fn push_bounded(out: &mut String, s: &str, max_bytes: usize) -> Result<(), ScriptError> {
    if out.len().saturating_add(s.len()) > max_bytes {
        return Err(ScriptError::ResourceLimit(
            "manifest JSON output byte limit exceeded",
        ));
    }
    out.push_str(s);
    Ok(())
}

fn push_spaces_bounded(out: &mut String, n: usize, max_bytes: usize) -> Result<(), ScriptError> {
    if out.len().saturating_add(n) > max_bytes {
        return Err(ScriptError::ResourceLimit(
            "manifest JSON output byte limit exceeded",
        ));
    }
    for _ in 0..n {
        out.push(' ');
    }
    Ok(())
}

fn push_json_str_bounded(s: &str, out: &mut String, max_bytes: usize) -> Result<(), ScriptError> {
    let escaped = serde_json::to_string(s).expect("string JSON serialization cannot fail");
    push_bounded(out, &escaped, max_bytes)
}

fn push_hex_bytes_bounded(bytes: &[u8], out: &mut String, max_bytes: usize) -> Result<(), ScriptError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let need = bytes.len().saturating_mul(2);
    if out.len().saturating_add(need) > max_bytes {
        return Err(ScriptError::ResourceLimit(
            "manifest JSON output byte limit exceeded",
        ));
    }
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    Ok(())
}

enum JsonTask<'a> {
    Value(&'a RecordedValue, usize),
    Raw(&'static str),
    Str(&'a str),
    Spaces(usize),
}

fn write_value_json_bounded(
    root: &RecordedValue,
    out: &mut String,
    root_indent: usize,
    max_bytes: usize,
) -> Result<(), ScriptError> {
    let mut stack: Vec<JsonTask<'_>> = vec![JsonTask::Value(root, root_indent)];
    while let Some(task) = stack.pop() {
        match task {
            JsonTask::Raw(s) => push_bounded(out, s, max_bytes)?,
            JsonTask::Str(s) => push_json_str_bounded(s, out, max_bytes)?,
            JsonTask::Spaces(n) => push_spaces_bounded(out, n, max_bytes)?,
            JsonTask::Value(val, indent) => match val {
                RecordedValue::Nil => push_bounded(out, "null", max_bytes)?,
                RecordedValue::Bool(true) => push_bounded(out, "true", max_bytes)?,
                RecordedValue::Bool(false) => push_bounded(out, "false", max_bytes)?,
                RecordedValue::Number(n) => {
                    if n.is_nan() {
                        push_bounded(out, "{\"$bfe\": \"number\", \"special\": \"nan\"}", max_bytes)?;
                    } else if *n == f32::INFINITY {
                        push_bounded(out, "{\"$bfe\": \"number\", \"special\": \"inf\"}", max_bytes)?;
                    } else if *n == f32::NEG_INFINITY {
                        push_bounded(
                            out,
                            "{\"$bfe\": \"number\", \"special\": \"-inf\"}",
                            max_bytes,
                        )?;
                    } else if *n == 0.0 && n.is_sign_negative() {
                        push_bounded(
                            out,
                            "{\"$bfe\": \"number\", \"special\": \"-0\"}",
                            max_bytes,
                        )?;
                    } else {
                        let s = numfmt::format_json_number(*n);
                        push_bounded(out, &s, max_bytes)?;
                    }
                }
                RecordedValue::String(s) => push_json_str_bounded(s, out, max_bytes)?,
                RecordedValue::Bytes(bytes) => {
                    push_bounded(out, "{\"$bfe\": \"bytes\", \"hex\": \"", max_bytes)?;
                    push_hex_bytes_bounded(bytes, out, max_bytes)?;
                    push_bounded(out, "\"}", max_bytes)?;
                }
                RecordedValue::Function => {
                    push_bounded(out, "{\"$bfe\": \"function\"}", max_bytes)?;
                }
                RecordedValue::Userdata => {
                    push_bounded(out, "{\"$bfe\": \"userdata\"}", max_bytes)?;
                }
                RecordedValue::Cycle { up } => push_bounded(
                    out,
                    &format!("{{\"$bfe\": \"cycle\", \"up\": {up}}}"),
                    max_bytes,
                )?,
                RecordedValue::Array(items) => {
                    push_bounded(out, "[", max_bytes)?;
                    let nested = items.iter().any(|v| {
                        matches!(
                            v,
                            RecordedValue::Array(_)
                                | RecordedValue::Object(_)
                                | RecordedValue::Table(_)
                        )
                    });
                    stack.push(JsonTask::Raw("]"));
                    if nested {
                        let inner = indent + 2;
                        stack.push(JsonTask::Spaces(indent));
                        stack.push(JsonTask::Raw("\n"));
                        for (i, item) in items.iter().enumerate().rev() {
                            stack.push(JsonTask::Value(item, inner));
                            stack.push(JsonTask::Spaces(inner));
                            if i > 0 {
                                stack.push(JsonTask::Raw(",\n"));
                            }
                        }
                        stack.push(JsonTask::Raw("\n"));
                    } else {
                        for (i, item) in items.iter().enumerate().rev() {
                            stack.push(JsonTask::Value(item, indent));
                            if i > 0 {
                                stack.push(JsonTask::Raw(", "));
                            }
                        }
                    }
                }
                RecordedValue::Object(pairs) => {
                    if pairs.is_empty() {
                        push_bounded(out, "{}", max_bytes)?;
                        continue;
                    }
                    push_bounded(out, "{\n", max_bytes)?;
                    let inner = indent + 2;
                    stack.push(JsonTask::Raw("}"));
                    stack.push(JsonTask::Spaces(indent));
                    stack.push(JsonTask::Raw("\n"));
                    for (i, (k, v)) in pairs.iter().enumerate().rev() {
                        stack.push(JsonTask::Value(v, inner));
                        stack.push(JsonTask::Raw(": "));
                        stack.push(JsonTask::Str(k));
                        stack.push(JsonTask::Spaces(inner));
                        if i > 0 {
                            stack.push(JsonTask::Raw(",\n"));
                        }
                    }
                }
                RecordedValue::Table(pairs) => {
                    push_bounded(out, "{\"$bfe\": \"table\", \"entries\": [", max_bytes)?;
                    if pairs.is_empty() {
                        push_bounded(out, "]}", max_bytes)?;
                        continue;
                    }
                    let inner = indent + 2;
                    stack.push(JsonTask::Raw("]}"));
                    stack.push(JsonTask::Spaces(indent));
                    stack.push(JsonTask::Raw("\n"));
                    for (i, (k, v)) in pairs.iter().enumerate().rev() {
                        stack.push(JsonTask::Raw("]"));
                        stack.push(JsonTask::Value(v, inner));
                        stack.push(JsonTask::Raw(", "));
                        stack.push(JsonTask::Value(k, inner));
                        stack.push(JsonTask::Raw("["));
                        stack.push(JsonTask::Spaces(inner));
                        if i > 0 {
                            stack.push(JsonTask::Raw(",\n"));
                        }
                    }
                    stack.push(JsonTask::Raw("\n"));
                }
            },
        }
    }
    Ok(())
}
