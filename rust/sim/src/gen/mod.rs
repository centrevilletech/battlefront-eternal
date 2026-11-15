//! The store's generated code: a module a schema file, and here the kinds'
//! codes, the store that holds every kind with fields, and its sync
//! dispatch (docs/sim-store.md). Written into OUT_DIR from rust/sim/schema by
//! build.rs (the generator is rust/sim-gen); `cargo expand -p sim gen` or the
//! build's OUT_DIR shows it.
#![allow(clippy::all, unused_variables, unused_imports, dead_code)]

include!(concat!(env!("OUT_DIR"), "/gen.rs"));
