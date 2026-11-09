# Whole-world input replay

`cargo xtask matrix` (`rust/xtask (matrix.rs)`) is a local diagnostic tool that
replays the 61-cell world-hash matrix (`tools/world-hash-matrix.txt`) through
the native Rust simulation (`rust/sim`, `world-hash-native`) and compares the
per-tick hashes (`state`, `digest`, `published`) against a reference directory
(not a CI gate):

```sh
cd rust
cargo xtask matrix --inputs /path/to/world-corpus --against /path/to/reference-hashes
```

A corpus has one directory per matrix label, with `setup.json` and
`inputs.json.gz`, plus shared content-addressed `blobs/*.gz`. `manifest.json`
lists the complete matrix and provenance. Version 1 records cold fetched bytes
(including missing-file results), world options, seeds, per-battle construction
clocks and setup rows, the deterministic clock, input/fire/ACK commands, mount
attempts and outcomes and every generated `WorldTickInput` row. Rows store
little-endian `f64` bits, preserving signed zero and NaN payloads. SHA-256
checks cover input files and uncompressed assets. The corpus contains no
authoritative world snapshots or expected fingerprints. Replay recreates normal
cold admission, steps the authoritative world tick, and recomputes state, deep
digest and publication hashes directly from the Rust world.

The recording contains game assets and stays outside the repository.

## Native world replay contract

`Context` (`rust/sim/rust/abi/context_step.rs` and `rust/sim/rust/world/`) exposes
the native world lifecycle and replay entry points:

- `Context::new`, `dispose` and `ErrorKind::Disposed`.
- `world_step_init(WorldTickSetup)` binds the battle's water clock, and
  `world_step(WorldTickInput)` advances the owning `World::step` borrow and
  poisons the context on a panic.
- `world_replay_admit(&serde_json::Value) -> Result<WorldTickSetup, Error>`
  admits the options and decoded cold assets, local player and publication host.
- `world_replay_prepare(&Value) -> Result<(), Error>` applies recorded
  input/fire/mount commands and prepares the native owners' input banks.
- `world_replay_publish(&Value, &[WorldTickOutput]) -> Result<Value, Error>`
  applies the step outputs and recorded ACKs at their post-step slot, then
  returns `{prefix,tick,stateJson,digestJson,packets}` using ordered serializers
  and `jsrt::numfmt` numeric formatting.
- Diagnostic entries `world_replay_live_handles() -> usize`,
  `world_replay_panic() -> Result<(), Error>` and `ErrorKind::Poisoned` verify
  that every closed context leaves zero live handles and rejects subsequent
  steps.
