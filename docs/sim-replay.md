# Proving the simulation's port: captures, replays and the world-hash gate

The authoritative simulation runs in Rust (`rust/sim`), shared by local worlds
in `bfe-client`, `bfeserver` and private client prediction
([sim-store.md](sim-store.md)). Phase captures retain the boundary facts needed
to replay a core independently; whole-world input corpora reconstruct admission
and tick input without using recorded authoritative output. Frozen fingerprints
establish behavior across changes.

Simulation math uses the shared Rust compatibility libraries (`rust/v8math`,
`rust/jsrt`, `rust/three`) so `x86_64` and `aarch64` produce identical bits;
raw platform `libm` results are never used in gameplay math.

## The phases of a tick

The tick is cut into the phases it runs, in order, from the server step down to
the world tail: the snapshot host's edges, the hit history, the players'
commands, their views, the step's start, the hulls' tick poses, the AI, every
unit's tick, the hulls, turrets and their gunners, the local gunner, recon
droids, soldiers pushed apart, live props, rounds in flight, items, the
battlefield's bounds, the battle's rules, the target reticle, grenade warnings,
the spawn screen, the host's consumed edges and the publication.
`rust/sim-replay (phase.rs)` holds the canonical phase list (`Phase::Ordnance`,
`Phase::UnitTick`, and so on).

## Capture format (`BFECAP`)

A phase capture records each sampled frame's starting state, every boundary call
(arguments, results, and random draws made inside the call), the random draws
the phase makes in order and from which stream, and the ending state. Every
tick, sampled or not, also records which phase drew how many numbers from which
stream. Frames are sampled every seventh tick and on every tick next to a death,
spawn, boarding or explosion.

The binary format (`BFECAP`, `rust/sim-replay`) keeps every number's 64 bits,
`-0` and `NaN` included, every string's UTF-16 code units, and every object's
key order and identity. Captures derived from retail game data stay outside the
repository.

## Replaying in Rust

`rust/sim-replay` reads captures, assembles their frames and serves them to
`rust/sim`. A crate's replay test, behind its `replay` feature, starts each
frame from its captured state, runs its Rust implementation, and queries a `Log`
for each boundary call and random draw:

    ordnance.stepBolt: 62955 frames from 32 captures, 62955 the same, 0 different, 0 not replayed

`BFE_REPLAY_CORPUS` names the directory of external captures; without it the
replay tests run their self-contained synthetic fixtures in CI without game
files.

## The world-hash gate

`cargo xtask matrix` (`rust/xtask (matrix.rs)`) fingerprints every battle in
`tools/world-hash-matrix.txt` and compares them tick by tick on all three hashes
(the replay state, a deep digest of every unit, hull and round, and the
published bytes) against `tests/fixtures/world-hashes.json`:

    cd rust && cargo xtask matrix --corpus /path/to/world-corpus --against ../tests/fixtures/world-hashes.json

The standing matrix has 61 configurations over 17 maps, including Enhanced AI,
heroes, mounted players, rewind shots and sequential battles. Compare state,
deep digest and published bytes; a matching final position alone is
insufficient. Whole-world input capture and the native replay contract are
described in [world-replay.md](world-replay.md).

## Running checks

Run the workspace unit and replay tests from `rust/`:

    cargo test --locked -p sim --features replay -p sim-replay

For cross-architecture published-bytes determinism without external game files,
run:

    cargo test --locked -p sim --test arm64_determinism
