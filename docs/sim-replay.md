# Proving the simulation's port: captures, replays and the world-hash gate

The world's simulation is moving from JavaScript to Rust one group of
subsystems at a time ([sim-store.md](sim-store.md)), and no tick may change
on the way: the world worker, the page's replay of its own soldier and the
dedicated server must keep agreeing with each other and with today's
battles. Each part of the move is proven twice. Before it takes the battle
over, its Rust is replayed against what the JavaScript did in real battles,
phase by phase and call by call. When it takes the battle over, whole
battles are fingerprinted tick by tick with it and without it. This note
describes the tools for both.

Everything here runs under the Node the port takes as its reference: Node
24.22, whose V8 (13.6) is the production server's. Other V8 versions
compute some Math functions differently in the last bit (the game's
Electron already parts from the server at the second tick of a Naboo
battle), so every capture and every fingerprint run records its Node and V8,
and the tools refuse to compare across them.

## The phases of a tick

The tick is cut into the phases it runs, in order, from the local server's
step down to the game's (`tools/sim-capture/phases.mjs`): the snapshot
host's edges, the hit history, the players' commands, their views, the
step's start, the hulls' tick poses, the AI, every unit's tick, the hulls,
turrets and their gunners, the local gunner, recon droids, soldiers pushed
apart, live props, rounds in flight, items, the battlefield's bounds, the
battle's rules, the target reticle, grenade warnings, the spawn screen, the
host's consumed edges and the publication. They are named once for both
languages: `rust/sim-replay/src/phase.rs` holds the same list in the same
order (`Phase::Ordnance`, `Phase::UnitTick`, ...), and a test on each side
fails if the two part. A port, its replays and its flip name the phases it
owns by these names.

## Capturing a battle

`tools/sim-capture.mjs` runs a battle exactly as `tools/world-hash.mjs` runs
it and records the frames its specs ask for:

    BFE_CONTENT_DIR=<game data> node tools/sim-capture.mjs --out DIR \
      --specs ordnance,items -- --map nab2 --side rep --seed 1 --ticks 2400

A frame is a phase of a tick, or a method of a live object called inside
one (each bolt's `stepBolt`). For each it writes the state the frame starts
from, every call it makes across its boundary (the arguments as they went
in, the result and the arguments as they came out, and the random draws made
inside the call), the draws the frame makes itself in order and from which
stream, and the state it ends with. Every tick, sampled or not, also records
which phase drew how many numbers from which stream, so a draw that moves
from one phase to another shows at once. Frames are sampled every seventh
tick and on every tick next to a death, spawn, boarding or explosion.

The specs (`tools/sim-capture/specs.mjs`) belong to the packages: each
names its frames, the state they start and end with, and its boundary, the
calls its Rust is served rather than runs. Kinds the store's schemas declare
are written through the store's own exporters (`cap.export(kind, object)`),
so a replay reads the shape the store holds. Anything a record names but
does not hold is a handle: a unit by its registry id, a hull by its place in
the battle's list, a collision body by its serial, a content definition by
its class (written once, whole, the first time a record names it), and an
instance of any other class numbered by its kind.

Nothing in the game changes for a capture. The markers around each phase
go in as the modules load (`tools/sim-capture/hooks.mjs`; each anchors on a
statement that must be found exactly once, or the capture stops), the
shims call straight through, and nothing reads a getter. A capture is three
runs of the battle: a scan for the ticks with events, the capture, and a
plain world-hash run, and all three must print the same fingerprints, or
the capture is thrown away.

`tools/sim-corpus.txt` lists the standing corpus: ten maps, three sides on
each map with battles in both eras, and two battles with the player's shots
judged in the past. Captures hold game data: they live with the game's
files, never in the repository.

The format (BFECAP, `tools/sim-capture/format.mjs`) keeps every number's 64
bits, -0 and NaN included, every string's UTF-16 code units, and every
object's key order and identity: an object met twice in a record is written
once and referred to after. Records count their strings and objects afresh,
so a reader can skip any record by its length.
`node tools/sim-capture/dump.mjs FILE --summary` looks into one.

## Replaying in Rust

`rust/sim-replay` reads captures exactly, assembles their frames and serves
them to a port. A package's replay test, behind its crate's `replay`
feature, starts each frame from its captured state, runs its Rust, and asks
a `Log` for each call and draw as the Rust makes it: the log hands back what
the JavaScript got, provided the Rust asks for the same thing at the same
point with the same arguments, and says where it parts otherwise. The state
the Rust ends with is compared with the capture's by `diff`, bit for bit as
`Object.is` compares numbers, identity included, and the first difference
comes back with its path:

    ordnance.stepBolt: 62955 frames from 32 captures, 62955 the same, 0 different, 0 not replayed

`BFE_REPLAY_CORPUS` names the directory of captures; without it the replay
tests pass without replaying, as CI has no game data. The crate's own tests
include a replay of a bolt's step, a transliteration of `stepBolt` used to
prove the capture and the reader on a real phase: every bolt step of the
corpus replays the same, and moving one number by one ulp fails every frame
that reaches it.

## The world-hash gate

`tools/world-hash-matrix.mjs` fingerprints every battle of
`tools/world-hash-matrix.txt` in each variant asked for, several at a time,
and compares them tick by tick on all three hashes (the replay state, a deep
digest of every unit, hull and round, and the published bytes):

    BFE_CONTENT_DIR=<game data> node tools/world-hash-matrix.mjs --out DIR --variants js,mv --jobs 8

A variant is world-hash options added to every battle (`mv` is
`--movement wasm`; a flip names its own with `--variant NAME=OPTIONS`); the
first is the reference. `--against DIR` compares with an earlier run, say
one recorded before a change, `--shard K/N` shares the work among machines,
and `--resume` keeps what a run already holds. The set is fifty-one battles
over all seventeen maps, most with the player's shots judged in the past,
with a droideka as the player, with the player manning a turret and then a
hull's seat, with the Enhanced AI, and one battle fought twice in one
process.

## The ladder in one command

`tools/sim-verify.mjs` runs each rung that lives in the repository: `v1` the
twin tests of the kernels and the Rust crates, `v2` the replays of a corpus,
`v3` the in-battle shadow check, `v4` the world-hash matrix, `v5` the
prediction checks run as they are and with a variant's environment, their
digests compared, and `v6` a battle under this Node and under another (the
game's Electron), reporting where each hash first parts. Timing on the
emulated laptop and the laptop itself (`v7`) runs outside the repository.
