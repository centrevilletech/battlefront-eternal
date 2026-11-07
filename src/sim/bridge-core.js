import { BLOCK_HEADER_WORDS } from './gen/boundary.js';
// The entity store's JavaScript side (docs/sim-store.md): what the code
// tools/sim-schema.mjs generates in src/sim/gen/ is written against. The
// tables that give JavaScript objects the numbers Rust knows them by
// (slots, definitions, strings), the pools of mirror objects that stand for
// entities Rust owns, and SimBridge, which moves a kind's rows in and out of
// a battle's instance of the module.
import { DEF_TABLES, KIND_TABLE, RECORD_CLASSES } from "./gen/index.js";

export { sameArray, sameEach, sameList, sameMap, sameObject, sameQuat, sameTyped, sameVec } from "./compare.js";

// A sync block's header: rows, stride, where the heap starts, its length,
// and how many of the rows (the first) are retired entities'
// (rust/sim/src/store/block.rs).
const HEADER = BLOCK_HEADER_WORDS;

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

// A later native generation can arrive before all earlier references have
// been published. Identity facades and generated mirrors share this small
// displacement store; an explicit retirement/unlink releases each old row.
function rememberDisplaced(pool, slot, gen, object) {
  pool.displaced ??= new Map();
  let lives = pool.displaced.get(slot);
  if (!lives) { lives = new Map();pool.displaced.set(slot,lives); }
  lives.set(gen,object);
}
function forgetDisplaced(pool, slot, gen) {
  const lives = pool.displaced?.get(slot), object = lives?.get(gen);
  if (!lives) return object;
  lives.delete(gen);
  if (!lives.size) pool.displaced.delete(slot);
  return object;
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
    this.nextSlot = 0;
  }

  // Reserve native identity storage without allocating logical slots. A
  // prepared immutable facade also reserves its WeakMap entry, so binding
  // it at an in-phase mirror barrier only replaces existing scalar values.
  reserve(count) {
    const start = this.objects.length;
    if (count <= start) return;
    this.objects.length = this.gens.length = count;
    for (let i = start; i < count; i++) { this.objects[i] = undefined;this.gens[i] = 0; }
  }
  prepare(object) { this.ids.set(object,undefined); }

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
      slot = this.free.length ? this.free.pop() : this.nextSlot++;
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

  // Immutable objects can originate in Rust too. A creation notice binds the
  // ordinary facade before the next event decodes a reference to that slot.
  // Mutable owned kinds continue to use MirrorPool and its generated sync.
  bind(slot, gen, object) {
    if (this.key) throw Error(`${this.name}: keyed identities bind through their key`);
    if (this.objects[slot] !== undefined && this.objects[slot] !== object) throw Error(`${this.name}: occupied slot ${slot}`);
    // Native binding consumes a free entry too. Otherwise repeated native
    // slot reuse accumulates free indices that still name occupied facades.
    const free = this.free.indexOf(slot);
    if (free >= 0) this.free.splice(free,1);
    this.ids.set(object, slot);
    this.objects[slot] = object;
    this.gens[slot] = gen;
    if (this.nextSlot <= slot) this.nextSlot = slot + 1;
  }

  // A closed native life may remain in an ordered context list until that
  // context next steps. Preserve reads of its exact generation during that
  // interval, while stale commands still encode the old qualified key.
  retain(slot, gen) {
    const object = this.at(slot,gen);
    rememberDisplaced(this,slot,gen,object);
    this.retained ??= new WeakMap();
    this.retained.set(object,{slot,gen});
    if (this.objects[slot] === object) {
      this.ids.delete(object);this.objects[slot] = undefined;
    }
    return object;
  }
  forget(slot, gen) { return forgetDisplaced(this,slot,gen); }

  put(object, f, at) {
    if (object === null || typeof object !== "object") throw Error(`${this.name}: not an object (${object})`);
    const old = this.retained?.get(object);
    if (old) { f[at] = old.slot;f[at + 1] = old.gen;return; }
    const slot = this.slotOf(object);
    f[at] = slot;
    f[at + 1] = this.key ? (this.gens[slot] ?? 0) : this.gens[slot];
  }

  at(slot, gen) {
    if (this.key) {
      const object = this.objects[slot] ?? this.resolve?.(slot);
      if (object === undefined || object === null) throw Error(`${this.name}: nothing numbered ${slot}`);
      return object;
    }
    if (this.gens[slot] !== gen || this.objects[slot] === undefined) {
      const old = this.displaced?.get(slot)?.get(gen);
      if (old !== undefined) return old;
      throw Error(`${this.name}: slot ${slot} is not of generation ${gen}`);
    }
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
  // mirror a slot), or the one a later life of the slot displaced before
  // its retirement was synced, else a new one nobody holds.
  retire(slot, gen) {
    const m = this.objects[slot];
    if (m === undefined || m.simGen !== gen) {
      const old = forgetDisplaced(this,slot,gen);
      return old === undefined ? new this.Mirror() : old;
    }
    if (!this.keyed) this.objects[slot] = undefined;
    return m;
  }

  at(slot, gen) {
    let m = this.objects[slot];
    if (m === undefined || (!this.keyed && m.simGen !== gen)) {
      const old = this.displaced?.get(slot)?.get(gen);
      if (old !== undefined) return old;
      // A slot's next life met before the last one's retirement was synced
      // (a round fired into the slot a round left in the same phase): the
      // last one's mirror waits for its retired row.
      if (m !== undefined) rememberDisplaced(this,slot,m.simGen,m);
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
  // (Truncating an array costs even when it is empty, as most are.)
  clear() { if (this.words.length) this.words.length = 0; }
  list(refs, items, f, at) {
    const words = this.words, pair = this.pair, start = words.length;
    let n = 0;
    for (const item of items) { refs.put(item, pair, 0); words.push(pair[0], pair[1]); n++; }
    f[at] = start;
    f[at + 1] = n;
  }
  // An array: its items' words (`size` each) and after them whatever their
  // records' lists hold; put(words, at, item) writes an item.
  items(items, f, at, size, put) {
    const words = this.words, start = words.length, n = items.length;
    for (let i = 0; i < n * size; i++) words[start + i] = 0;
    for (let i = 0; i < n; i++) put(words, start + i * size, items[i]);
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

  // A Map keyed by numbers or objects: its entries' words (`size` each)
  // and after them whatever its values' lists hold; put(words, at, key,
  // value) writes an entry, its key and its value.
  pairs(map, f, at, size, put) {
    const words = this.words, start = words.length, n = map.size;
    for (let i = 0; i < n * size; i++) words.push(0);
    let i = 0;
    for (const [key, value] of map) put(words, start + i++ * size, key, value);
    f[at] = start;
    f[at + 1] = n;
  }

  // A plain object keyed by array indices (a table by team number), in its
  // keys' order, which is ascending: each entry the key, then the value
  // (`size` words in all); put(words, at, value) writes a value.
  object(object, f, at, size, put) {
    const words = this.words, start = words.length;
    let n = 0;
    for (const key in object) {
      const k = +key;
      if (!(k >>> 0 === k && k !== 4294967295 && String(k) === key)) throw Error(`an object keyed by numbers has the key "${key}"`);
      n++;
    }
    for (let i = 0; i < n * size; i++) words.push(0);
    let i = 0;
    for (const key in object) {
      const e = start + i++ * size;
      words[e] = +key;
      put(words, e, object[key]);
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
  constructor({ classes = {}, tests = {}, kinds = KIND_TABLE, defTables = DEF_TABLES, recordClasses = RECORD_CLASSES } = {}) {
    this.recordClasses = recordClasses;
    this.kinds = kinds;
    // A union of records tells them apart by these (a record's $class).
    this.recordTests = Object.fromEntries(Object.entries(recordClasses).map(([name, names]) => {
      const ctors = names.map((n) => classes[n]).filter(Boolean);
      return [name, tests[name] ?? ((o) => ctors.some((c) => o instanceof c))];
    }));
    this.defs = Object.fromEntries(defTables.map((name) => [name, new DefTable(name)]));
    this.refs = kinds.map((k) => {
      if (!k) return null;
      const ctors = k.classes.map((name) => classes[name]).filter(Boolean);
      const test = tests[k.name] ?? (ctors.length ? (o) => ctors.some((c) => o instanceof c) : null);
      return new Identity(k.code, { key: k.key, name: k.name, test });
    });
  }

  setRefs(kind, refs) { this.refs[kind] = refs; return refs; }

  // More of what tells kinds apart (as the constructor takes them), from a
  // phase that joins a bridge already made: a kind JavaScript still owns
  // that has no test yet gets the one given; the first phase to give a
  // kind's test sets it.
  identify({ classes = {}, tests = {} } = {}) {
    this.kinds.forEach((k, code) => {
      const refs = this.refs[code];
      if (!k || !(refs instanceof Identity) || refs.test) return;
      const ctors = k.classes.map((name) => classes[name]).filter(Boolean);
      refs.test = tests[k.name] ?? (ctors.length ? (o) => ctors.some((c) => o instanceof c) : null);
    });
    for (const [name, names] of Object.entries(this.recordClasses)) {
      const ctors = names.map((n) => classes[n]).filter(Boolean);
      if (tests[name] || ctors.length) this.recordTests[name] = tests[name] ?? ((o) => ctors.some((c) => o instanceof c));
    }
    return this;
  }

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

// Generated record I/O shared by WASM and native operation arenas. Neither
// the encoder nor decoder needs an exports object or a native address.
export class BlockBridge extends SimContext {
  constructor(blocks, options = {}) {
    super(options);
    this.blocks = blocks;
    this.readBlock = blocks.readBlock.bind(blocks);
    this.boundaryCounters = null;
  }
  // A record out of a block Rust wrote at `ptr`, into `into` (a new
  // object if none).
  callResult(record, ptr, into = null) {
    return this.decode(record, this.readBlock(ptr), into);
  }
  decode(record, block, into = null) {
    const f = block.words, base = block.base;
    if (this.boundaryCounters) this.boundaryCounters.outputWords += f[base] * f[base + 1] + f[base + 3];
    return record.sync(this, f, base + HEADER, base + f[base + 2], into ?? new record.Mirror());
  }


}

// The provider's strings: an index for each, the same on both sides
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
    id = this.bridge.backend.intern(s);
    this.ids.set(s, id);
    this.list[id] = s;
    return id;
  }

  at(id) {
    let s = this.list[id];
    if (s !== undefined) return s;
    s = this.bridge.backend.string(id);
    this.ids.set(s, id);
    this.list[id] = s;
    return s;
  }
}
