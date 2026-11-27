//! Deterministic pure-Rust Lua 5.0 bytecode runtime for Star Wars: Battlefront II (2005)
//! mission scripts.
//!
//! Virtual machine and standard library semantics are ported from Lua 5.0.3 (MIT License,
//! Copyright (C) 1994-2006 Tecgraf, PUC-Rio), with the 2005 game's 32-bit float number type
//! and `MAXSTACK = 128` register/constant split (cross-checked against `lua5.0-swbf2-x64`,
//! GPL-3.0).

#![forbid(unsafe_code)]

pub mod archive;
pub mod chunk;
pub mod error;
pub mod numfmt;
pub mod opcode;
pub mod recorder;
pub mod stdlib;
mod units;
pub mod value;
pub mod vm;

pub use archive::{ArchiveLimits, ArchiveScript, ScriptArchive};
pub use chunk::{load_chunk, load_chunk_with_limits, ChunkLimits, Constant, Proto};
pub use error::{ArchiveError, LoadError, ScriptError};
pub use recorder::{
    archive_host_functions, record_manifest, record_manifest_with_config, record_manifest_with_limits,
    record_manifest_with_options, scan_archive, ArchiveScan, ImportProfile, Manifest,
    ProfileAnswer, RecordedCall, RecordedValue, RecorderLimits, RecorderOptions, UnresolvedQuery,
    MANIFEST_SCHEMA_VERSION,
};
pub use value::{
    canonicalize_number, ClosureId, CollectorStats, NativeFnId, StringId, TableId, UserdataId,
    Value, ValueRef, CANONICAL_NAN_BITS,
};
pub use vm::{
    CallCtx, DefaultRng, NativeClosure, NativeFn, OutputSink, ScriptRng, UnknownGlobalCallHandler,
    Vm, VmLimits, TICK_COLLECTOR_UNITS,
};
