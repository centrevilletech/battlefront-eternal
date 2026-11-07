# Whole-world input replay

The harness can record the standing world matrix and replay its inputs through
WebAssembly without reading installed content:

```sh
node tools/world-hash-matrix.mjs --out RECORD_HASHES --record-corpus CORPUS --against REFERENCES --require-comparison
node tools/world-hash-matrix.mjs --out REPLAY_HASHES --replay-corpus CORPUS --against REFERENCES --require-comparison
```

A corpus has one directory per matrix label, with `setup.json` and
`inputs.json.gz`, plus shared content-addressed `blobs/*.gz`. `manifest.json`
lists the complete matrix and provenance. Version 1 records cold fetched bytes
(including missing-file results), world options, seeds, per-battle construction clocks and setup rows, the deterministic
clock, input/fire/ACK commands, mount attempts and outcomes and every generated
`WorldTickInput` row. Rows store little-endian f64 bits, preserving signed zero
and NaN payloads. SHA-256 checks cover input files and uncompressed assets.
The corpus contains no authoritative world snapshots or expected fingerprints.
Replay recreates normal cold admission and calls the same coordinator exports
as the page. It checks the regenerated input rows before supplying the recorded
bytes. State, deep digest and publication hashes are recomputed from that world.

The recording contains game assets and stays outside the repository. It is an
input proof, not a replacement for complete packet/event and service acceptance.

## Native driver contract

`--backend native --replay-corpus CORPUS --native-driver BINARY` injects the
native harness only. `tools/world-native.mjs` verifies/decompresses corpus data
and feeds the Rust driver. Node canonicalizes returned observation JSON and
packet bytes with the same three SHA-1 truncations as the WASM harness.
No recorded expected output or JS simulation is supplied to Rust.

The `world-twin` binary requires Cargo feature `world-twin`, off by default.
It requires panic=unwind. It compiles against the native world API on
`Context` (rust/sim/src/abi/context_step.rs carries the signature table) but
is **not yet a qualified native backend**: `provided()` reports which entries
have bodies. Its entries are:

- Existing `Context::new`, `dispose` and `ErrorKind::Disposed`.
- `world_step_init(WorldTickSetup)` binds the battle's water clock.
  `world_step(WorldTickInput)` guards the owning `World::step` borrow and
  poisons the context on a panic; it fails `Unavailable` until the native
  tick ingress lands, because the tick is still bound to the WebAssembly
  module.
- Harness-only `world_replay_admit(&serde_json::Value) -> Result<WorldTickSetup, Error>`:
  admit the options and decoded cold assets, local player and publication host.
- Harness-only `world_replay_prepare(&Value) -> Result<(), Error>`: apply recorded
  input/fire/mount commands and prepare the native owners' input banks.
- Harness-only `world_replay_publish(&Value, &[WorldTickOutput]) -> Result<Value, Error>`: apply the
  actual step outputs and recorded ACKs at their post-step slot, then return `{prefix,tick,stateJson,digestJson,packets}`.
  Ordered JSON text retains `worldState` and the deeper observer's field order;
  Node parses/stringifies it to normalize numeric spelling with the same V8.
  Native observers must use ordered serializers rather than JSON maps.
  Packets retain order and use `{channel,bytes}` (base64) or `{channel,text}`.
- Diagnostic-only `world_replay_live_handles() -> usize`,
  `world_replay_panic() -> Result<(), Error>` and `ErrorKind::Poisoned`.
  The live-handle diagnostic remains readable after disposal.

The three replay adapters fail `Unavailable`. The corpus records cold asset
bytes, options, setup rows and tick inputs, not the host's admission calls:
a native admission therefore means loading content and building every owner
from those bytes in Rust, which the JavaScript host does today through the
store imports and operations. Until that exists, the native proof of the 61
rows runs the JavaScript host over the native backend (the addon and
`src/sim/native-backend.js`), not this driver.

The host-facing entries beside the tick are `world_publish_into`
(`wire_publish_in` for one publication session, spans relative to the
returned bytes), `read_bank` (every bank a phase descriptor names, by
number) and `run_operation_into` (the generated `OPERATION_*` table: the
flat-row commands and the input mailbox, edge and host operations).

Prepared scenarios are `interleave`, `threads`, `sequential` (20 worlds), `close`
and `poison`, passed through `world-native.mjs --scenario NAME`. They have not
been run while the native entries are unavailable. Replay streams observations;
isolation cases retain compact fingerprints instead of every full world JSON.
Contexts are made inside
worker threads; only owned corpus values cross threads. Each close checks zero
live handles, and a later step on a closed or poisoned context must fail.
