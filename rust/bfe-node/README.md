# Native store arenas

The addon wraps the existing thread-affine simulation Context. It reports
`nativeWorld: false` and `nativePublication: false`; world stepping and packet
publication are not exposed. The handshake and staging ABI stay at 1 until
those entries and the generated operation table are available together.

`new NativeHost().create({inputWords, outputWords})` reserves addon arenas at
admission. Omitting the argument reserves 8192 words for each arena. The
existing Context methods still allocate cold store replies and decode buffers;
this API does not claim an allocation-free native world tick.

- `operation(code, args, out)` accepts a generated one-row sync block. Only the
  existing native `echo` and `fill` commands are admitted. No command calls JS.
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

After building the fixture addon, run the additional boundary cases with
`node --expose-gc --test rust/bfe-node/tests/arenas.mjs`. The existing addon and
lifecycle fixtures remain under `tests/node-host-*.mjs`.
