# The simulation's module and entity store

The world's simulation is moving from JavaScript to Rust, one group of
subsystems at a time, without changing a bit of any tick: the world worker,
the page's replay of its own soldier and the dedicated server must keep
agreeing with each other and with today's battles. This note describes what
that work builds on: one WebAssembly module a battle, an entity store in it,
schemas that declare each kind of entity once, and the code generated from
them on both sides of the boundary.

During the move each kind of entity is owned by one side at a time. While
JavaScript owns a kind, its objects are the state and a Rust phase that reads
them gets a copy of the fields it declared, written in before the phase.
Once Rust owns a kind, the store is the state and JavaScript that still reads
it reads mirror objects, refreshed after each Rust phase that changed them.
Either way a phase runs on one copy, and what crosses is a few loops of
numbers at phase boundaries rather than a call for every field read.

## One module, one instance a battle

`rust/sim` is one crate and one WebAssembly module, `bfe_sim`. It holds:

- `collision`: the collision world's queries (casts, sphere gathers, overlaps,
  the ground, `terrainNear`, a hull's wall probes), which `src/ray-kernel.js`
  calls ([docs/performance.md](performance.md), "The collision's rays in
  WebAssembly");
- `movement`: the soldiers' movement (`Soldier.tick`), which
  `src/movement-kernel.js` calls;
- `pose`: the pose sampler of `src/anim.js`, which `src/anim-kernel.js` calls;
- `store` and `gen`: the entity store and the code generated from the
  schemas in `rust/sim/schema/`;
- `abi`: the store's exports.

The first three were the crates `rust/rays` and `rust/anim`; they are modules
of one crate now, so a battle holds one instance with one memory instead of
one per kind of query, and the parts ported later reach them without
crossing into JavaScript. Their exports kept their names, so their bridges
did not change. A subsystem ported later gets a module of its own beside
them (`ordnance`, `vehicles`, ...), written against the store's types.

`tools/build-rust.mjs sim` builds it and writes `src/sim-wasm.js`, the
module's bytes as base64. It is committed, like the renderer's package, so
building the game needs no Rust, and CI rebuilds it and fails on any byte
that differs. `src/sim/module.js` compiles it once a thread (`simModule()`)
and makes an instance for each battle (`loadSim()`): the world worker, a
page and plain Node (the dedicated server, `tools/world-hash.mjs`) load it
the same way, with no `.wasm` file to serve. `createGame` makes one instance
when the battle asks for one of its kernels, and always in a world (a
headless game: the local worker, the dedicated server, the tools), whose
rounds in flight it holds (below, "The rounds on the module"); it hands the
instance to each kernel (`loadRayKernel(instance)`, `loadAnimKernel(instance)`).
A page that asks for no kernel loads nothing.

The module was 141 KB of WebAssembly with the three kernels; the store and
the first schema's code (four kinds of round and their records) make it
181 KB. Code is generated row by row, so it grows with the fields declared:
identical layouts share their code, and each row is taken as a fixed-length
array so its fields need no bounds checks.

Loading it costs each thread that does (the world worker, the page) a
decode and a compile, once: in Node on one core of a fast desktop, atob
0.9 ms and the copy into bytes 3.3 ms more, then 9 to 10 ms to compile
(6.1 ms compiling a `.wasm` file's bytes as they stream in). Where the
engine has `Uint8Array.fromBase64` the loader uses it instead of the copy.
On the emulated laptop the instance a page makes for its pose sampler cost
nothing measurable (interleaved 40-second runs, three of each: page CPU
33.0 against 33.2 ms a frame, render 15.5 against 15.7 ms, with and without
it). A `.wasm` asset would save the decode and some of the compile; it is
worth it when the module is several times larger, measured then.

## Memory

A battle's instance starts with the module's statics and stack, and its
memory grows as the kernels copy the map's trees and the clips in (3 to 12
MB a map) and as the store fills. Growing it detaches every view JavaScript
holds of it. The kernels' bridges make their views again after any call
that can grow it (each checks the view's `byteLength`, which a detached view
reports as 0); the store's bridge (`SimBridge.views()`) does the same once a
phase boundary, where it reads or writes rows, never in the middle of a
phase, and counts how often it found the memory grown. The policy for the
port: memory is sized once as a battle loads (content, the store's slabs at
their pools' sizes, and a margin), so in play it does not grow and no view
is made mid-phase; a debug run treats growth inside a phase as an error.

## The entity store

### Kinds, slots and identity

Every kind of entity the simulation keeps (a bolt, a unit, a vehicle, a
command post) has a code (its place among the kinds' names, from 1) and, if
the store holds its fields, a slab: its entities by slot, each slot with a
generation (`store::Slab`). A reference to an entity is its slot and the
slot's generation (`store::Ref<T>`); removing the entity moves its slot's
generation on, so a reference to it is seen to be stale rather than naming
whatever takes the slot next. References replace the object references the
JavaScript holds, and they keep the JavaScript's identity rules, which are
behaviour (a hit names the same body, `===` and WeakMap keys still work):

- A kind whose JavaScript objects carry their own number, named by its
  schema's `key` (a unit's `id`, given once by the registry and kept for
  the battle while the pooled object takes life after life), uses that
  number as its slot, with generation 0. Its mirror is the same object for
  the whole battle, as the pooled JavaScript object was.
- Any other kind's JavaScript objects are new for every entity (a bolt is a
  new object each shot), so each life of a slot gets a new mirror.

A kind that lives in a list of a singleton (a bolt in `ordnance.bolts`) names
that list as its `home`: the list's order is the kind's order everywhere the
store lists its entities, as the JavaScript loops over it in that order.

### Schemas

Each kind is declared in a schema, a JavaScript module of plain data in
`rust/sim/schema/` (one file a package or subsystem). `ordnance.mjs`, the
first, declares the rounds in flight and is the pilot for the others:

```js
export default {
  enums: { RoundKind: ['bullet', 'missile', 'shell', 'fatray'] },
  unions: { OrdnanceOwner: ['null', 'lit:player', 'ref:unit', 'ref:vehicle', 'ref:ownerSnapshot'] },
  records: { Impact: { point: 'vec3', normal: 'vec3', terrain: 'bool', body: 'ref:body|null|undefined' } },
  kinds: {
    ordnance: { singleton: true, fields: { bolts: 'list:bolt' /* , ... */ } },
    bolt: {
      home: 'ordnance.bolts',
      fields: { def: 'def:ordnance', owner: 'union:OrdnanceOwner', position: 'vec3', life: 'f64', impact: 'rec:Impact|null' },
      skip: { glow: 'colour from the definition, for drawing' },
    },
    unit: { key: 'id' },
  },
};
```

A field's type is one of:

| Type | JavaScript | Rust | Row words |
| --- | --- | --- | ---: |
| `f64`, `f32`, `i32`, `u32`, `u8` | a number (`f32`: one Math.fround or a Float32Array keeps) | `f64`, `f32`, ... | 1 |
| `bool` | a boolean | `bool` | 1 |
| `vec3`, `quat`, `mat4` | a three.js Vector3, Quaternion or Matrix4, kept and updated in place | `[f64; 3]`, `[f64; 4]`, `[f64; 16]` | 3, 4, 16 |
| `f64xN` (N 2 to 16) | an array of N numbers | `[f64; N]` | N |
| `str` | a string the simulation compares | `StrId` | 1 |
| `enum:Name` | one of a fixed list of strings | `#[repr(u8)] enum` | 1 |
| `def:table` | a content definition object | `DefId` | 1 |
| `ref:kind` | an object of that kind | `Ref<Kind>` | 2 |
| `union:Name` | null, undefined, a literal string, an object of one of several kinds, or a value of one of several types (a number or a Vector3; one of two records, told apart by the classes they name with `$class`) | an `enum` with data | 1 + its largest variant's |
| `index:list` | one item of an earlier field of the same object, a list of records, by identity (a unit's `hitSet`, one of its `shapes`) | `u32`, the item's place | 1 |
| `rec:Name` | a plain object with declared fields, held by value | a struct | its fields' |
| `list:kind`, `set:kind` | an array or a Set of objects of that kind, in order | `Vec<Ref<Kind>>`, `RefSet<Kind>` | 2 (and the heap) |
| `list:type` | an array of any other type that is never null: records by value (a hull's seats), numbers, vectors, booleans, strings, enums, unions; its items updated in place | `Vec<T>` | 2 (and the heap) |
| `map:type` | a Map with string keys, in insertion order, its values of a type (not null) | `StrMap<T>` | 2 (and the heap) |

`|null`, `|undefined` or both after a type make it nullable (`Option<T>`, or
`Nullable<T>` when both are possible, since code can tell them apart). A
field written `{ type, js: 'damageDef.Damage', mirror: false }` reads a
nested JavaScript path (`shapes.0.radius` reads through an array); `mirror: false` keeps a field out of the mirrors and
`import: false` out of what a Rust phase is given. `skip` lists what an
object of the kind holds that is not simulation state (its model, its
trail, a page's drawing state), each with why: anything else an object holds
that its schema does not list is reported by the completeness check
(`undeclared(object, KIND_KEYS)`), since state nobody declared is state a
replay would miss. A kind can be declared in several files: each package
declares the fields it reads of another's kinds, and the generator joins
them into one kind (a field declared twice must be declared the same). A
kind declared with no fields (`unit: { key: 'id' }` above) is one the store
only refers to.

### The generator's own tests

The generator has a schema of its own, `rust/sim/schema-test/shelf.mjs`,
with every type a field can have (maps of lists, records and references,
entities that name each other, a union that holds a kind the store holds).
Its code is generated alike, into `rust/sim/src/gen_test/` (compiled into
the crate's tests only, so nothing of it reaches the module) and
`tests/sim-gen/`, and `rust/sim/src/gen_tests.rs` and
`tests/sim-schema.test.js` put it through the rows and the replay values on
both sides, a map's words checked word for word on each. The shipped
schemas belong to the packages that port each subsystem; the generator's
features are proven on its own.

### What the generator makes

`node tools/sim-schema.mjs` makes, from every schema, the Rust in
`rust/sim/src/gen/` and the JavaScript in `src/sim/gen/`; both are committed,
`tools/build-rust.mjs sim` writes them before it builds the module and
`--check` fails when they are stale. Each kind's layout hash (its fields,
types, flags and order) is in both, and `SimBridge` refuses a module whose
kinds were made from other schemas than the JavaScript's.

- Rust: a struct a kind and a record (`Bolt`, `Impact`), enums and unions,
  and each kind's rows: `put_export`/`take_export`, `put_import`/
  `take_import` and `put_full`/`take_full`. `gen::Store` holds a slab for
  every kind with fields (and singletons by value), and `Store::export` and
  `Store::import` move a kind's rows; the codes are `KIND_BOLT` and so on.
- JavaScript: `KIND_TABLE` (by code: name, layout hash, singleton, key,
  home, strides and functions), and for each kind a mirror class
  (`BoltMirror`: every mirrored field declared in its constructor, so every
  mirror has one hidden class, plus `simSlot` and `simGen`), `syncBolt` (an
  export row into a mirror), `syncFullBolt`, `putBolt` (an object into an
  import row), `putFullBolt`, `checkBolt` (the write detector), `sameBolt`
  (the first field two objects differ in, by Object.is) and `exportBolt` (the
  replay harness's value of a bolt).

### Sync blocks

What crosses at a phase boundary is a kind's block: a row of f64 for each
entity in the module's memory, `[rows, stride, heap start, heap length]`
and then rows of slot, generation and the fields in the schema's order (a
boolean 0 or 1, an enum its index, a reference its slot and generation, a
string or definition its index, a Vector3 three numbers, a nullable field a
tag first: 0 a value follows, 1 null, 2 undefined), then the heap, where a
row's lists and sets are. A map's entries are in the heap too, each
its key's string and its value's words, with the lists its values hold after
them. A kind has three layouts: export (the fields JavaScript reads once
Rust owns the kind), import (the fields a Rust phase reads while JavaScript
owns it) and full (every field, for tests and tools).

An entity Rust removes in a phase may still be held by JavaScript: a sim
event keeps a grenade's position, and the world's digest reads it after the
grenade went off. So a port retires an entity (`Slab::retire`) rather than
dropping it: its slot is freed as by `remove`, and its last state stays in
the slab until the kind's next export, which writes it first (the header's
fifth word says how many rows are retired ones). The sync gives that last
state to the entity's mirror, whoever holds it, and the pool lets the mirror
go, so a later life of the slot gets a new one.

`SimBridge` (`src/sim/runtime.js`) moves them:

- `write(kind, objects, full)` writes JavaScript objects' rows (a
  singleton's one object) in their order and has the store take them; an
  entity the block no longer lists is gone from the store, as it left the
  JavaScript list;
- `sync(kind, target)` copies an export block into the kind's mirrors (a
  singleton's into `target`), updating each mirror's Vector3s in place so
  whatever holds them keeps seeing them, retired entities' last states
  first;
- `read(kind)` gives every field of every entity as new objects;
- `check(kind)` is the mirror write detector: after a JavaScript phase, any
  mirrored field that is not what the store holds, which is a write
  JavaScript made to Rust's state without a command; a debug run fails on
  the first. It leaves retired entities for the next sync.

References go through each kind's identity on the JavaScript side
(`bridge.refs[kind]`): an `Identity` while JavaScript owns the kind (a slot
for each object, its number for a keyed kind, and a `test` that tells a
union's kinds apart), a `MirrorPool` once Rust does (`bridge.own(kind)`).

### Definitions and strings

A content definition (an ordnance class, a weapon, an explosion) is the
index of its object in the battle's table of that sort (`bridge.defs.ordnance`,
`store::DefId`). Several weapons can carry objects of the same class, which
the JavaScript keeps apart (caches keyed by the object), so the index is the
object's, not its class's. The battle registers its definitions as it loads,
in the order the Rust content loader numbers them, which is the content
package's to fix; until then an object met that was never registered is
registered as it is met. A string the simulation keeps is an index in the
instance's table (`bridge.strings`, `store::Strings`), interned in Rust and
kept as UTF-16 units, so any JavaScript string comes back the same.

### Captures for the replay harness

The replay harness (`tools/sim-capture`, `rust/sim-replay`) records a
battle's phases in its own format and replays them natively. A kind's state
goes into a capture as the plain values its exporter makes
(`exportState('ordnance', game.ordnance, cx)`): its fields in the schema's
order, Vector3s and Quaternions and arrays of numbers as they are, records
as plain objects, and the entities of a kind the store holds exported where
they are first met (once each, `cx.seen`, so identity survives). The export
context (`ExportContext`) says what a reference to anything else becomes:
`cx.ref(kind, object)` (the harness gives its handle, named by the store's
kind with a key that stays the object's for the whole capture) and
`cx.def(table, def)` (the harness's definition handle); a union's variant is
found by its kinds' tests, which the context makes from the classes the
schema names (`class: 'Vehicle'`, the constructors passed in as
`classes`) or takes by kind name (`tests`).

A union may hold one kind the store holds fields of without a test of its
own: an object that no other variant's test claims is of that kind. A union
of several such kinds (a round's owner, a unit or a hull, once both are
the store's) tells every object by a test, and each of those kinds must
name its class, or the generator refuses the schema. Null and literals are
matched first, then the tests, most specific first: the kinds the store
holds (by their mirror's class once Rust owns them, else by the
constructors the battle passes), then the other kinds that name classes,
then the plain objects a battle tells by shape (`tests`, such as a beam's
emitter), which an instance of a class could also pass. A value no test
claims is refused (`which...` throws) rather than taken for the last
variant. The variant crosses as its index, so Rust never tests.

An entity of a kind the store holds is written out whole only where it
lives, its home list, led by its own handle (`$id`); a reference to it
anywhere else (another entity's field, a list, a union) is that handle, as
a reference to anything the store does not hold is, so a unit named by a
hundred bolts is written once.

In a union of several records, a record's value starts with `$record` (its
name), so the replay can tell which it is; an index is the very value the
list's item was written as, so the capture holds one object where
JavaScript held one.

On the Rust side, with the crate's `replay` feature (it adds the harness's
crate), the generated `from_value` and `to_value` read those values into the
store's structs and write them back: `Replay::read_value(KIND_ORDNANCE, v)`
makes the singleton and every entity its lists hold, and
`Replay::write_value` gives the same values back (`Replay::reset` starts a
frame's state anew), which `sim_replay::diff`
compares with a capture's, the first difference with its path. `Replay`
maps the harness's handles to slots (a keyed kind's key is its slot; any
other key gets the next slot of its kind) and definition handles to
indices, and back, so the values come out with the handles they went in
with. `rust/sim/tests/replay_values.rs` holds the store to that.

### Commands, events and imports

What crosses outside a kind's rows is declared in a schema too, and each
one's arguments, result or payload is a record, so it crosses as that
record's words (the store's own schema, `rust/sim/schema/store.mjs`, has
the `echo` command its tests and benchmarks use):

- A command (`commands: { fire: { args: 'FireArgs', result: 'FireResult',
  rust: 'ordnance::fire' } }`) is JavaScript writing into state Rust owns:
  `bridge.command('fire', args)` writes the arguments into the call block,
  the module runs `crate::ordnance::fire(sim, args)` at once (so writes keep
  their order) and the result comes back as a new object. A caller that
  reads the touched entity back syncs its kind.
- An event (`events: { impact: 'ImpactEvent' }`) is Rust telling
  JavaScript: `event.emit(&mut sim.events)` adds it to the phase's events,
  and `bridge.drain({ impact: (e) => ... })` hands them over in the order
  they were emitted at the end of the phase. An event whose handler
  changes state the same phase reads back is an import instead.
- An import (`imports: { raycastUnits: { args: 'RaycastArgs', result:
  'RaycastHit' } }`) is Rust asking JavaScript while the state asked about
  is still JavaScript's: `gen::import_raycast_units(&args)` calls out of the
  module, the bridge serves it with `bridge.handlers.raycastUnits(args)`,
  and the result comes back. Its arguments and result cross in a block of
  their own, outside the store (a fixed stretch of the module's memory, so
  JavaScript reads the arguments and writes the result there with no other
  call: an import costs a call each way), reached only for a moment before
  and after the call. The arguments object a handler gets is the bridge's
  own, read into again at the import's next call: a handler keeps nothing
  of it (it copies a vector it hands to code that might keep one) and
  reads what it needs before it calls code that could come back into the
  module. A port whose Rust holds no reference to the store across an
  import (the rounds' and items' do not: `rust/sim/src/ordnance/module.rs`)
  can take such calls back in; otherwise a handler must not call back. The
  module's loader gives every instance the imports' functions, which reach
  the bridge attached to it. A port reaches other subsystems through a
  trait of its own; on the module, its JavaScript side is these imports,
  and in the replay tests the capture serves it.

Still to come in this package: `sim_reserve` (the store's slabs and a
margin made as a battle loads) and the measurements of what crossing
costs on the emulated laptop.

### The rounds and items on the module

The first kinds Rust owns in a battle are the rounds in flight (bolts,
beams, grenades and rounds: rockets, missiles, shells, fat rays, with the
Ordnance that lists them; `rust/sim/src/ordnance`, `src/ordnance-sim.js`)
and the things lying about (mines and power-ups, with the Items that lists
them and each dispenser's own; `rust/sim/src/items`, `src/items-sim.js`). A
world hands them over as it loads (`createGame`, unless `?sim=js` or, under
Node, `BFE_SIM=js`, which keep the JavaScript ones for A/B runs and
bisecting); from then on:

- `Ordnance.fire`, `update` and `flightMissile` (a flyer's missile), and
  `Items.update`, `spawn`, `dispense`, `dropFor`, `blast` and `hurt`, run in
  Rust through the module's own exports (`ord_*`, `items_*`), not generated
  commands: their arguments come in the call block, written by the
  generated put functions of the schemas' argument records (`FireArgs`,
  `SpawnArgs`, ...), and an answer that makes something is its row, so the
  caller gets its mirror as it is.
- The kinds' mirrors and the lists (the Ordnance's four with its `start`,
  the Items' list and its dispensers' Map) are synced in place after each
  update and command. Unported code reads them as it read the JavaScript
  objects, and writes only the fields the schemas leave to JavaScript
  (`skip`: a muzzle a page draws a round from, a grenade's `contactTime`
  and `aiDanger`).
- What they ask of the battle is `OrdnanceWorld`'s and `ItemsWorld`'s
  calls (`src/ordnance-world.js`), as the schemas' imports, served by the
  same objects. A round handed to the capture's observers (`projectile`,
  `launch`, `impact`, `grenadeContact`) crosses with its row after the
  import's words, so its mirror is current when JavaScript sees it; a
  unit's hit hands JavaScript the very vector the JavaScript rounds handed
  (`at`: a beam's end, the unit raycast's record, the Ordnance's `start`),
  as a damage event keeps it. An item class's model is measured once and
  not drawn: a world shows nothing.
- Definitions go over the first time they are used (`ord_def`,
  `ord_explosion`, `items_def`; the item pools when the items are handed
  over), each field as a tag and a value, so the Rust reads a field as
  JavaScript does (`??`, `||`, ToNumber).

This is the first flip (F1). It lands on the simulation port's
integration branch, which does not ship: nothing from it reaches players
until a world's net step with the rounds and items on the module is no
worse than with the JavaScript ones, measured on the emulated laptop or the
laptop itself (world step p50 and p90, interleaved runs against
`?sim=js`). Each call the rounds make into JavaScript costs a crossing, so
the flip pays for itself as the code they call (hulls, then units and
combat) moves to Rust and those calls stop crossing.

The exports reach the store through pointers and hold no reference to it
across an import, so a handler that comes back in (an explosion an item
set off reaching the other items, a death dropping one) finds the store as
the phase left it. A slot one entity left and another took before the next
sync keeps the first one's mirror for its retired row (`MirrorPool.retire`).

## Determinism

Everything here moves bits, never computes them: a number crosses as the
f64 it is, and the rows' integers (slots, generations, indices, tags) are
whole numbers by construction. The ported code computes, and it computes as
the reference does: the production server's V8 (Node 24.22, V8 13.6). The
game client's Electron computes some Math functions differently in their
last bit (sin, cos, tan, atan, atan2, acos, exp, log, cbrt, tanh), so client
and server already part there today; Rust that computes with the reference's
functions (`rust/v8math`) makes them agree. The movement kernel computes
the sines and cosines of its yaw and pitch with `rust/v8math`, so a client's
soldier ticks there come out as the server's do.
