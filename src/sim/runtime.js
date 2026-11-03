// The entity store's JavaScript side (docs/sim-store.md): what the code
// tools/sim-schema.mjs generates in src/sim/gen/ is written against. The
// tables that give JavaScript objects the numbers Rust knows them by
// (slots, definitions, strings), the pools of mirror objects that stand for
// entities Rust owns, and SimBridge, which moves a kind's rows in and out of
// a battle's instance of the module.
import { DEF_TABLES, KIND_TABLE } from "./gen/index.js";

export { sameArray, sameList, sameMap, sameQuat, sameVec } from "./compare.js";

// A sync block's header: rows, stride, where the heap starts, its length,
// and how many of the rows (the first) are retired entities'
// (rust/sim/src/store/block.rs).
const HEADER = 5;

// The keys of `object` its kind's schema does not list (the completeness
// check: a field nobody declared is state a replay would miss).
export function undeclared(object, keys) {
  const out = [];
  for (const key in object) if (!keys.has(key)) out.push(key);
  return out;
}

// A kind by name.
export function kindNamed(name) {
  const k = KIND_TABLE.find((x) => x?.name === name);
  if (!k) throw Error(`no kind ${name}`);
  return k;
}

// The plain values of a singleton's state and every entity in its lists,
// for the replay harness to record (tools/sim-capture), made through an
// ExportContext: an entity met twice is one value.
export function exportState(name, object, cx = new ExportContext()) {
  return kindNamed(name).export(object, cx);
}

// A table of content definitions of one sort (an ordnance class, a weapon):
// each object's index, in the order they were registered. The battle
// registers its definitions as it loads, in the order the Rust content
// loader numbers them (docs/sim-store.md, "Definitions"); one met that was
// never registered is registered then.
export class DefTable {
  constructor(name) {
    this.name = name;
    this.ids = new WeakMap();
    this.list = [];
  }

  register(def) {
    let id = this.ids.get(def);
    if (id === undefined) { id = this.list.length; this.list.push(def); this.ids.set(def, id); }
    return id;
  }

  id(def) {
    if (def === null || typeof def !== "object") throw Error(`${this.name}: not a definition (${def})`);
    return this.ids.get(def) ?? this.register(def);
  }

  at(id) {
    const def = this.list[id];
    if (def === undefined) throw Error(`${this.name}: no definition ${id}`);
    return def;
  }
}

// The slots of a kind's JavaScript objects, for references Rust holds to
// a kind JavaScript still owns: a kind with a key uses the object's number
// (generation 0); any other is given the slot freed last (or a new one) and
// a generation, until release(). `test` says whether a value is one of the
// kind (for unions); `resolve` finds an object by its key.
export class Identity {
  constructor(kind, { key = KIND_TABLE[kind].key, name = KIND_TABLE[kind].name, test = null, resolve = null } = {}) {
    this.kind = kind;
    this.name = name;
    this.key = key;
    this.test = test;
    this.resolve = resolve;
    this.ids = new WeakMap();
    this.objects = [];
    this.gens = [];
    this.free = [];
  }

  is(value) {
    if (value === null || typeof value !== "object") return false;
    if (this.test) return this.test(value);
    return this.key ? this.objects[value[this.key]] === value : this.ids.has(value);
  }

  slotOf(object) {
    if (this.key) {
      const id = object[this.key];
      if (!Number.isInteger(id) || id < 0) throw Error(`${this.name}: an object without a ${this.key}`);
      this.objects[id] = object;
      return id;
    }
    let slot = this.ids.get(object);
    if (slot === undefined) {
      slot = this.free.length ? this.free.pop() : this.objects.length;
      this.ids.set(object, slot);
      this.objects[slot] = object;
      this.gens[slot] = (this.gens[slot] ?? 0) + 1;
    }
    return slot;
  }

  release(object) {
    const slot = this.ids.get(object);
    if (slot === undefined) return;
    this.ids.delete(object);
    this.objects[slot] = undefined;
    this.free.push(slot);
  }

  put(object, f, at) {
    if (object === null || typeof object !== "object") throw Error(`${this.name}: not an object (${object})`);
    const slot = this.slotOf(object);
    f[at] = slot;
    f[at + 1] = this.key ? 0 : this.gens[slot];
  }

  at(slot, gen) {
    if (this.key) {
      const object = this.objects[slot] ?? this.resolve?.(slot);
      if (object === undefined || object === null) throw Error(`${this.name}: nothing numbered ${slot}`);
      return object;
    }
    if (this.gens[slot] !== gen || this.objects[slot] === undefined) throw Error(`${this.name}: slot ${slot} is not of generation ${gen}`);
    return this.objects[slot];
  }
}

// One mirror object a slot for a kind Rust owns: unported code reads
// these as it read the JavaScript objects (same fields, same Vector3s
// kept). A kind with a key keeps its slot's mirror for good, as its
// JavaScript objects are pooled; any other gets a new mirror when its
// slot's generation changes, as each of its JavaScript objects was new.
export class MirrorPool {
  constructor(kind, table = KIND_TABLE) {
    this.kind = kind;
    this.Mirror = table[kind].Mirror;
    this.keyed = Boolean(table[kind].key);
    this.objects = [];
  }

  is(value) { return value instanceof this.Mirror; }

  // The mirror of a retired entity, for its last state: the slot's, if it
  // is that life's (then the slot forgets it, unless the kind keeps one
  // mirror a slot), else a new one nobody holds.
  retire(slot, gen) {
    const m = this.objects[slot];
    if (m === undefined || m.simGen !== gen) return new this.Mirror();
    if (!this.keyed) this.objects[slot] = undefined;
    return m;
  }

  at(slot, gen) {
    let m = this.objects[slot];
    if (m === undefined || (!this.keyed && m.simGen !== gen)) {
      m = new this.Mirror();
      m.simSlot = slot;
      m.simGen = gen;
      this.objects[slot] = m;
    }
    return m;
  }

  put(object, f, at) { f[at] = object.simSlot; f[at + 1] = object.simGen; }
}

// The heap of an import block being written: what its rows' lists, sets
// and maps hold.
export class Heap {
  constructor() { this.words = []; this.pair = [0, 0]; }
  clear() { this.words.length = 0; }
  list(refs, items, f, at) {
    const words = this.words, pair = this.pair, start = words.length;
    let n = 0;
    for (const item of items) { refs.put(item, pair, 0); words.push(pair[0], pair[1]); n++; }
    f[at] = start;
    f[at + 1] = n;
  }
  // A Map with string keys: its entries' words (the key's string, then the
  // value, `size` words in all) and after them whatever its values' lists
  // hold; put(words, at, value) writes a value.
  map(strings, map, f, at, size, put) {
    const words = this.words, start = words.length, n = map.size;
    for (let i = 0; i < n * size; i++) words.push(0);
    let i = 0;
    for (const [key, value] of map) {
      const e = start + i++ * size;
      words[e] = strings.id(key);
      put(words, e, value);
    }
    f[at] = start;
    f[at + 1] = n;
  }
}

// ---- the context and the bridge to an instance ----

// What the generated row code needs on the JavaScript side: the definition
// tables and each kind's references (an Identity, or a MirrorPool once Rust
// owns the kind). A union's kinds are told apart by each kind's test: one
// given by name in `tests`, else whether the object is an instance of one
// of the classes its schema names (`class`), looked up in `classes` (the
// constructors themselves, as a minified build renames them).
export class SimContext {
  // `kinds` and `defTables`: a generated kind table and its definition
  // tables (the store's by default; the generator's tests pass theirs).
  constructor({ classes = {}, tests = {}, kinds = KIND_TABLE, defTables = DEF_TABLES } = {}) {
    this.kinds = kinds;
    this.defs = Object.fromEntries(defTables.map((name) => [name, new DefTable(name)]));
    this.refs = kinds.map((k) => {
      if (!k) return null;
      const ctors = k.classes.map((name) => classes[name]).filter(Boolean);
      const test = tests[k.name] ?? (ctors.length ? (o) => ctors.some((c) => o instanceof c) : null);
      return new Identity(k.code, { key: k.key, name: k.name, test });
    });
  }

  setRefs(kind, refs) { this.refs[kind] = refs; return refs; }

  // Rust now owns the kind: its references are mirrors.
  own(kind) { return this.setRefs(kind, new MirrorPool(kind, this.kinds)); }

  list(kind, f, start, n, into) {
    const refs = this.refs[kind];
    into.length = n;
    for (let i = 0; i < n; i++) into[i] = refs.at(f[start + 2 * i], f[start + 2 * i + 1]);
    return into;
  }

  set(kind, f, start, n, into) {
    const refs = this.refs[kind];
    into.clear();
    for (let i = 0; i < n; i++) into.add(refs.at(f[start + 2 * i], f[start + 2 * i + 1]));
    return into;
  }
}

// What the exporters need: SimContext's tests (for unions), the values
// already made (`seen`: an entity met again is the same value), and what
// to make of a reference to a kind the store holds no fields of, and of a
// definition. The replay harness's `ref(kind, object)` gives its handle,
// named by the store's kind (`unit`, `body`, ...) with a key that is the
// same for the same object all through a capture; `def(table, def)` its
// definition handle. Without them a reference is the object itself.
export class ExportContext extends SimContext {
  constructor({ ref = (kind, object) => object, def = (table, d) => d, ...options } = {}) {
    super(options);
    this.seen = new Map();
    this.ref = ref;
    this.def = def;
  }
}

// A battle's instance of the module, as the store sees it: its exports,
// views of its memory (made again when it grows: `views()` once a phase
// boundary, never in the middle of one) and its string table.
export class SimBridge extends SimContext {
  constructor(instance, options = {}) {
    super(options);
    this.x = instance.exports;
    if (this.x.sim_abi() !== 1) throw Error(`the simulation's module has store ABI ${this.x.sim_abi()}, not 1`);
    // A module built from other schemas than the generated JavaScript's.
    if (this.x.sim_kinds() !== KIND_TABLE.length - 1) throw Error(`the simulation's module knows ${this.x.sim_kinds()} kinds, the store's JavaScript ${KIND_TABLE.length - 1}: rebuild it (npm run rust:build)`);
    // (A u32 comes back from the module as an i32: >>> 0 reads it unsigned.)
    for (const k of KIND_TABLE) if (k && this.x.sim_kind_hash(k.code) >>> 0 !== k.hash) throw Error(`the simulation's module lays ${k.name} out otherwise than the store's JavaScript: rebuild it (npm run rust:build)`);
    this.memory = this.x.memory;
    this.buffer = null;
    this.f = null;
    this.u8 = null;
    this.views();
    this.strings = new StringTable(this);
    this.heap = new Heap();
    this.rows = new Float64Array(1024);
    // How often the memory had grown when `views()` looked (debug: not in
    // a phase; release: views are made again where it is noticed).
    this.grown = 0;
  }

  // The memory's views, made again if it grew (a grown memory's old buffer
  // is detached: its byteLength is 0).
  views() {
    if (this.buffer === this.memory.buffer && this.buffer.byteLength) return;
    if (this.buffer) this.grown++;
    this.buffer = this.memory.buffer;
    this.f = new Float64Array(this.buffer);
    this.u8 = new Uint8Array(this.buffer);
  }

  // Bytes into the scratch buffer.
  scratch(bytes) {
    const ptr = this.x.sim_scratch(bytes.length);
    this.views();
    this.u8.set(bytes, ptr);
  }

  // A kind's export block (`mode` as sim_export takes it: 0 the mirrored
  // fields after the retired entities' rows, 1 every field, 2 the mirrored
  // fields without the retired): calls visit(f, at, h, slot, gen, retired)
  // for each row (`at` its first word, `h` the heap's), and returns the row
  // count.
  exported(kind, mode, visit) {
    const ptr = this.x.sim_export(kind, mode);
    if (!ptr) throw Error(`${KIND_TABLE[kind].name}: the store holds no fields of it`);
    this.views();
    const f = this.f, base = ptr / 8, rows = f[base], stride = f[base + 1], h = base + f[base + 2], retired = f[base + 4];
    for (let i = 0; i < rows; i++) {
      const at = base + HEADER + i * stride;
      visit(f, at, h, f[at], f[at + 1], i < retired);
    }
    return rows;
  }

  // Copies the export block of a kind Rust owns into its mirrors (a
  // singleton's into `target`); returns the row count. An entity retired
  // since the last sync gets its last state into its mirror (whoever kept
  // the mirror, or one of its Vector3s, sees it), and the pool lets the
  // mirror go.
  sync(kind, target = null) {
    const k = KIND_TABLE[kind], pool = this.refs[kind];
    if (k.singleton) return this.exported(kind, 0, (f, at, h) => k.sync(this, f, at, h, target));
    return this.exported(kind, 0, (f, at, h, slot, gen, retired) => k.sync(this, f, at, h, retired ? pool.retire(slot, gen) : pool.at(slot, gen)));
  }

  // Every field of every entity of a kind, as new objects made by `make()`
  // (a mirror by default): [{ slot, gen, object }] in the store's order.
  read(kind, make = () => new KIND_TABLE[kind].Mirror()) {
    const k = KIND_TABLE[kind], out = [];
    this.exported(kind, 1, (f, at, h, slot, gen) => out.push({ slot, gen, object: k.syncFull(this, f, at, h, make()) }));
    return out;
  }

  // The mirror write detector: after a JavaScript phase, any mirrored
  // field that is not what the store holds (a write JavaScript made to
  // Rust's state without a command). Returns [path, ...] (empty: clean).
  check(kind, target = null) {
    const k = KIND_TABLE[kind], pool = this.refs[kind], found = [];
    this.exported(kind, 2, (f, at, h, slot, gen, retired) => {
      if (retired) return;
      const d = k.check(this, f, at, h, k.singleton ? target : pool.at(slot, gen));
      if (d) found.push(`${k.name}${k.singleton ? "" : `[${slot}]`}${d}`);
    });
    return found;
  }

  // Retires an entity of a kind the store holds (Rust's Slab::retire: its
  // last state stays for the next sync).
  retire(kind, slot, gen) {
    if (this.x.sim_retire(kind, slot, gen) !== 0) throw Error(`${KIND_TABLE[kind].name}: no entity in slot ${slot} of generation ${gen}`);
  }

  // Writes the import block of a kind JavaScript owns from its objects (a
  // singleton's one object) in their order, its import fields or all of
  // them (`full`), and has the store take it.
  write(kind, objects, full = false) {
    const k = KIND_TABLE[kind], stride = full ? k.fullStride : k.importStride, put = full ? k.putFull : k.put;
    const n = objects.length, heap = this.heap, refs = this.refs[kind];
    if (this.rows.length < n * stride) this.rows = new Float64Array(Math.max(n * stride, this.rows.length * 2));
    const rows = this.rows;
    heap.clear();
    for (let i = 0; i < n; i++) {
      const at = i * stride, o = objects[i];
      if (k.singleton) { rows[at] = 0; rows[at + 1] = 0; } else refs.put(o, rows, at);
      put(this, o, rows, at, heap);
    }
    const ptr = this.x.sim_import_room(kind, n, stride, heap.words.length);
    this.views();
    const base = ptr / 8;
    this.f.set(rows.subarray(0, n * stride), base + HEADER);
    this.f.set(heap.words, base + HEADER + n * stride);
    if (this.x.sim_import(kind, full ? 1 : 0) !== 0) throw Error(`${k.name}: the store holds no fields of it`);
  }
}

// The instance's strings: an index for each, the same on both sides
// (strings are interned in Rust; this keeps a copy of each it has seen).
export class StringTable {
  constructor(bridge) {
    this.bridge = bridge;
    this.ids = new Map();
    this.list = [];
  }

  id(s) {
    let id = this.ids.get(s);
    if (id !== undefined) return id;
    if (typeof s !== "string") throw Error(`not a string (${s})`);
    const units = new Uint8Array(s.length * 2);
    for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); units[2 * i] = c & 0xff; units[2 * i + 1] = c >> 8; }
    this.bridge.scratch(units);
    id = this.bridge.x.sim_str_intern(s.length);
    this.ids.set(s, id);
    this.list[id] = s;
    return id;
  }

  at(id) {
    let s = this.list[id];
    if (s !== undefined) return s;
    const b = this.bridge, len = b.x.sim_str_len(id), ptr = b.x.sim_str_ptr(id);
    b.views();
    const units = new Uint16Array(b.buffer, ptr, len);
    s = "";
    for (let i = 0; i < len; i++) s += String.fromCharCode(units[i]);
    this.ids.set(s, id);
    this.list[id] = s;
    return s;
  }
}
