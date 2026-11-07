# Native world and store arenas

The addon wraps the existing thread-affine simulation Context, including its
world boundaries. Method presence alone does not claim a native world. Entries
without a native body fail `Unavailable` and leave their owner usable. World
and publication readiness are separate handshake capabilities.

`new NativeHost().create({inputWords, outputWords})` reserves addon arenas at
admission. Omitting the argument reserves 8192 words for each arena. The
existing Context methods still allocate cold store replies and decode buffers;
the fixed step rows themselves reuse addon storage reserved at admission.

- `operation(code, args, out)` accepts a generated one-row sync block using
  the generated `OPERATION_*` IDs. Scalars precede the record row and heap.
  The Context admits only its native table; no operation calls JS.
- `importKind(kind, flags, block)` and `exportKind(kind, mode, out)` use the
  generated Store layouts, flags and modes. Common headers, row strides,
  counts and slab slots are checked before calling the shared decoder.
- `intern(units)` and `string(id)` exchange arrays of UTF-16 code units,
  including lone surrogates. `retire(kind, slot, generation)` uses local keys.
- `events(out)` copies the ordered event words and clears them after delivery.

Numeric arenas must be ordinary, attached `Float64Array` views. Arguments may
overlap output views. Copies preserve floating bits, including signed zero and
NaN payloads; arithmetic performed by a command keeps that command's semantics.
No JS getters or callbacks run while numeric memory is borrowed.

Each output method returns the number of words written. A return greater than
`out.length` is the required capacity, and leaves all output words unchanged.
The result is retained, including consumed retirement rows. Retry the same
request with sufficient capacity at admission to copy that result exactly once.
An operation retry must have identical argument bits. Other mutations fail
`Busy` until delivery or close. Replaying a world tick is never part of this
contract. Extra output capacity is left untouched.

`diagnostics()` returns process-wide created/released/live Context counters,
owner arena reserved/used word counts, last fault, and closed/poisoned flags.
Arena counts exclude Context allocations that its API does not expose. The
counters and diagnostics exist in production builds. A caught operation panic
poisons its owner; diagnostics and close remain available. Explicit close, GC
and environment cleanup release the Context and retained arenas exactly once.
The `lifecycle-probe` feature adds fault injection and teardown fixtures only.
Its replay probes feed recorded cold assets and observations through Context;
the map smoke always calls the production step method and rejects refused ticks.

After building the fixture addon, run the additional boundary cases with
`node --expose-gc --test rust/bfe-node/tests/arenas.mjs`. The existing addon and
lifecycle fixtures remain under `tests/node-host-*.mjs`.


World calls use raw generated rows: `stepInit(setup)` takes four words and
`step(input, out)` uses the generated input and output row sizes (currently
23 and 24 words, including player ingress and its event barrier). A short output retains
the completed result; only a retry with identical input bits may deliver it.
`readBank(bank, out)` follows the same retained-capacity rule, including banks
whose read consumes changes. `reserve({inputWords, outputWords, publicationBytes})` is an explicit
admission boundary that can grow numeric arenas and packet span locations.
Every owner reserves at least one fixed input/output row during creation, plus
a 64 KiB publication byte window unless `publicationBytes` selects another size.
Context writes operation and publication replies into those retained buffers;
the addon does not clone an intermediate reply. Operation and publication
inputs larger than the reserved input arena fail before dispatch; call
`reserve` at admission before submitting their larger generated heaps.

`publish({session, words}, out)` accepts a publication session pool key and a
`WireWorldFacts` row with its generated heap. It returns `{words, packets}`.
The numeric result remains a generated `WirePublicationOutput` block. Each
`WireSpan.offset` indexes `packets` and its length is unchanged. Shared packet
parts, own packets, status text, record payloads and fault text all use that
same indexing rule; repeated byte spans share one buffer. Every buffer has
exactly the span's size and owns a single copy made with
`Env::create_arraybuffer`. Buffers survive the owner, later publications and
transfer to another Worker. A short numeric output returns an empty packet
array and retains the whole result for delivery at admission.

`publicationReservedBytes` and `publicationUsedBytes` in diagnostics count
retained publication bytes and packet span locations separately from numeric
arena words. Closing an owner frees both stores. A panic caught by Context
returns `GenericFailure` on the initiating addon call; subsequent operations
fail `Poisoned`. Diagnostics and idempotent close remain available.

The handshake reads `provided()` from the linked simulation. `nativeWorld` and
`nativePublication` reflect its step and publication bodies independently.
`operationTable` is absent when operations are unavailable, and otherwise is
the complete generated boundary fingerprint (including the operation table,
record layouts and encoders), also exposed as `schema`. Addon ABI 2 identifies
the complete world boundary alongside the existing store-arena surface.
