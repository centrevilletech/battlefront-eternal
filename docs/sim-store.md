# The simulation module and entity store

`rust/sim` owns the authoritative world: collision, movement, units, combat,
vehicles, recon, live objects, battle rules, AI and snapshot publication.
Production browser, worker and Node hosts load its committed WebAssembly from
`src/sim-wasm.js`. A required module that fails to load stops startup.

`rust/sim/src/world/owner.rs` groups the battle's canonical state and phase
owners. Internal services borrow that World instead of acquiring another
subsystem singleton. The shared phase bodies compile for native targets too;
that is separate from exposing a complete native server operation. The current
server bundle still selects WASM; see [node-host.md](node-host.md).

## One provider for each world

`src/game.js` receives its simulation factory before attaching kernels or
stores. `src/sim/wasm/backend.js` supplies the production provider. Its WASM
memory bindings, including the collision, movement and pose adapters, live in
`src/sim/wasm/`. `src/sim/bridge-core.js` owns shared identities and generated
record I/O. `BlockView` supplies words, section base, length and generation.
Native store arenas do not turn memory addresses into JavaScript numbers.

The browser compiles the simulation module once per thread and instantiates
it for each battle. Content, rigs, slabs and working buffers are admitted
before gameplay. Views are refreshed after memory growth. A provider releases
its resources on explicit close, failed construction or fatal tick errors;
close requested from a synchronous observer completes after the active return.

`sim_reset` is a store-only helper for tools: it finishes AI and replaces the
canonical generated Store. Collision, movement, pose, fleet and the other
phase, input, wire and page owners retain their state. Start a new battle
with a new World or WASM instance.

## The authoritative world tick

`src/runtime/world-tick.js` prepares a generated `WorldTickInput` and invokes
one backend world operation. `rust/sim/src/world/step.rs` advances water,
squads, the viewpoint clock and camera, AI, units/fleet/recon/separation,
live objects, ordnance, items, the battle tail and grenade dangers. It returns
`WorldTickOutput`; JavaScript does not select the internal phase order.

Canonical rows remain available to synchronous native callbacks. Damage,
shots, death and boarding retain their observation order. Mirror banks commit
changed words at the completed outer boundary, relative to the host's consumed
baseline. Snapshot publication has its own Rust wire owner and operation;
it reads canonical state rather than rebuilding every actor in JavaScript.

JavaScript retains cold admission, external input and host notifications, plus
facades used by page and network consumers. A facade does not own the world
phase that writes its represented state.

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
| `map:num:type` | a Map with number keys, in JavaScript's order (insertion; a key set again keeps its place, one deleted and set again goes last; keys as SameValueZero, -0 as 0) | `NumMap<T>` (`jsrt::map::JsMap`'s semantics) | 2 (and the heap) |
| `map:ref:K:type` | a Map whose keys are objects of a kind, or of a union of kinds `K` (each variant a kind), by identity, in the same order | `RefMap<Ref<Kind>, T>`, `RefMap<K, T>` | 2 (and the heap) |
| `obj:type` | a plain object keyed by array indices (a table by team number, `{ 1: 150, 2: 150 }`): its keys iterate ascending however they were set | `IntObj<T>` | 2 (and the heap) |
| `Float64Array(N)`, `Int32Array(N)`, ... | a typed array of N elements (Float64, Float32, Int32, Uint32, Int16, Uint16, Int8, Uint8), kept typed in the mirror | `TypedArray<f64, N>`, ... | N |

`|null`, `|undefined` or both after a type make it nullable (`Option<T>`, or
`Nullable<T>` when both are possible, since code can tell them apart). A
list's items, and a map's or an object's values, are never null unless their
type is written in parentheses: `list:(ref:character|null)` is a list whose
items can be null (`Vec<Option<Ref<Character>>>`, each item a tag and its
words), while `list:ref:character|null` is a list that can be null. A
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

An immutable object with fields can declare `identity: true` instead of a key
or a home list. Launch attribution uses this form: its slab reference identifies
the snapshot, while its stored id and generation describe the launching life.
The bridge keeps its ordinary `Identity`; a native creation notice binds the
frozen facade before a later event refers to it. Slot-order exports still work,
without inventing a live gameplay list for those immutable objects.

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
  (the initial field two objects differ in, by Object.is) and `exportBolt` (the
  replay harness's value of a bolt).

### Sync blocks

What crosses at a phase boundary is a kind's block: a row of f64 for each
entity in the module's memory, `[rows, stride, heap start, heap length, retired rows]`
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
- `writeSome(kind, objects, full)` writes the rows of some of a kind's
  entities and leaves every other entity of the kind as the store holds it
  (and whatever the store keeps of the ones it writes beyond their rows,
  such as a unit's combat record): the way to write a few units, a seat's
  riders or the units a hook changed, without dropping the rest. Its write-back must also
  select that subset: other objects may have advanced in JavaScript since
  their stored rows were written;
- `sync(kind, target)` copies an export block into the kind's mirrors (a
  singleton's into `target`), updating each mirror's Vector3s in place so
  whatever holds them keeps seeing them, retired entities' last states
  first; an object keyed by numbers, a Map keyed by numbers or objects and a
  typed array are updated in place too (the same object, Map or array, its
  values' arrays and records read into), a Map's entries moved only where
  its order changed;
- `read(kind)` gives every field of every entity as new objects;
- `check(kind)` is the mirror write detector: after a JavaScript phase, any
  mirrored field that is not what the store holds, which is a write
  JavaScript made to Rust's state without a command; a debug run stops at
  its earliest mismatch. It leaves retired entities for the next sync.

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
compares with a capture's, the earliest difference with its path. `Replay`
maps the harness's handles to slots (a keyed kind's key is its slot; any
other key gets the next slot of its kind) and definition handles to
indices, and back, so the values come out with the handles they went in
with. `rust/sim/tests/replay_values.rs` holds the store to that.

### Commands, events and imports

Schemas also declare command arguments, results, events and remaining host
imports. Generated encoders place those records in retained call blocks.
Scalar commands validate qualified identities and update the touched owner;
ordinary world phases call shared Rust services directly.

Ordered events carry observations to the host after the owning boundary.
State-sensitive callbacks borrow the active native service scope at the point
of observation. An actual host import must not retain a row borrow across
reentry. Native imports without an implementation report an explicit failure;
compilation does not silently install a JavaScript world.

### Ordnance and items

`rust/sim/src/ordnance/` owns bolts, beams, grenades and guided rounds.
`rust/sim/src/items/` owns mines, power-ups and dispenser state. Their world
services use the same canonical collision, unit, combat, fleet and battle
owners. Definitions and geometry handles are prepared at admission. Launch,
impact, pickup and retirement observations retain their original order.
The WASM host adapters are `src/sim/wasm/ordnance-sim.js` and
`src/sim/wasm/items-sim.js`; page drawing is a separate presentation concern.

## Determinism and validation

Rows preserve floating-point values and qualified identity. Gameplay arithmetic
uses `rust/v8math`, `rust/jsrt` and `rust/three` where reference behavior depends
on number, collection or geometry semantics. Cosmetic randomness and device
clocks are separate from authoritative gameplay streams and fixed ticks.

`tools/sim-schema.mjs` regenerates records, and `npm run rust:check` verifies
schemas and committed WebAssembly. Native store fixtures cover layouts,
retirement and bit preservation. `tools/world-hash-matrix.mjs` compares state,
deep digest and published bytes with frozen reference hashes. See
[sim-replay.md](sim-replay.md) and [world-replay.md](world-replay.md) for
phase and whole-world input capture.
