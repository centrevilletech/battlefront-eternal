// Concrete memory/export compatibility for the WASM phase adapters only.
// Native providers reuse BlockBridge's generated I/O over owned arenas.
import { BlockBridge, Heap } from '../bridge-core.js';
import { wasmBackendFor } from './backend.js';
import { BLOCK_HEADER_WORDS as HEADER } from '../gen/boundary.js';
import { COMMAND_TABLE, EVENT_TABLE, IMPORT_TABLE, KIND_TABLE } from '../gen/index.js';
// A battle's WASM exports and the phase bindings' retained full-memory views.
// The generated record decoder above also accepts independent native arenas.
export class SimBridge extends BlockBridge {
  constructor(source, options = {}) {
    const backend = source.getBridge ? source : wasmBackendFor(source);
    super(backend.blocks, options);
    this.backend = backend;
    this.closed = false;
    this.memory = backend.blocks.memory;
    this.strings = new StringTable(this);
    this.heap = new Heap();
    this.rows = new Float64Array(1024);
    // What serves the module's imports, by name (a schema's imports):
    // handler(args) gives the result. Each import's arguments object, by
    // code, made the first time it is served.
    this.handlers = {};
    this.served = [];
    this.phaseCounters = null;
    this.beforeImport = null;
    // The import block: where it is (in f64s) and its size.
    backend.connect(this, options);
    this.commandCodes = new Map(COMMAND_TABLE.filter(Boolean).map((c) => [c.name, c.code]));
  }
  // Bound once at world creation. Native providers supply coarse operations,
  // not a projection of the old C exports.
  world(phase, clock) { return this.backend.world(phase, clock); }
  publisher(resources) { return this.backend.publisher(resources); }
  get x() { return this.backend.exports; }
  set x(value) { this.backend.setExports(value); }
  close() { return this.backend.close(); }

  // A call's record into the call block: its words, then its heap.
  callArgs(record, value) {
    const heap = this.heap, words = record.words;
    if (this.rows.length < words) this.rows = new Float64Array(Math.max(words, this.rows.length * 2));
    const rows = this.rows;
    heap.clear();
    record.put(this, value, rows, 0, heap);
    this.backend.arguments(rows, words, heap.words);
    const hw = heap.words;
    if (this.boundaryCounters) this.boundaryCounters.inputWords += words + hw.length;
  }

  // Serves imports (a phase's own, by import name): `handlers` adds
  // name -> handler(args) to those served; an import no schema declares, or
  // one another phase already serves, is refused.
  register(handlers) {
    for (const [name, handler] of Object.entries(handlers)) {
      if (!IMPORT_TABLE.some((imp) => imp?.name === name)) throw Error(`no schema declares an import ${name}`);
      if (this.handlers[name]) throw Error(`the import ${name} is served already`);
      this.handlers[name] = handler;
    }
    return this;
  }

  // Runs a command (by name, or by code, which is quicker) on its arguments
  // (an object with its args record's fields) at once, and gives its result
  // (into `into`, else a new object), or undefined for a command that has
  // none. Every command uses its generated argument/result block, including
  // the old flat scalar commands.
  command(name, args, into = null) {
    const c = COMMAND_TABLE[typeof name === "number" ? name : this.commandCodes.get(name)];
    if (!c) throw Error(`no command ${name}`);
    this.callArgs(c.args, args);
    const block = this.backend.command(c.code);
    return c.result ? this.decode(c.result, block, into) : undefined;
  }

  // Hands the events emitted since the last drain to handlers[name](event),
  // in the order they were emitted (each a new object, which a handler may
  // keep), and forgets them; returns how many there were.
  drain(handlers) {
    const ptr = this.backend.events();
    const block = this.readBlock(ptr);
    const f = block.words, base = block.base, end = base + 1 + f[base];
    if (this.boundaryCounters) this.boundaryCounters.outputWords += f[base];
    let count = 0;
    for (let at = base + 1; at < end; at += 2 + f[at + 1]) {
      const e = EVENT_TABLE[f[at]], r = e.record;
      const event = r.sync(this, f, at + 2, at + 2 + r.words, new r.Mirror());
      handlers[e.name]?.(event);
      count++;
    }
    this.backend.clearEvents();
    return count;
  }

  // The same event format, read directly at an owned phase's publication.
  // A retained mirror is scratch: handlers copy any values they keep. Offset
  // lets an aimer consume the prefix before the phase appends more events.
  // The caller owns the epoch and clearing, so this adds no getter exports.
  drainFrom(ptr, offset, mirrors, handlers, selected = false, cursor = null) {
    const block = this.readBlock(ptr);
    const f = block.words, base = block.base, end = base + 1 + f[base];
    let at = base + 1 + offset;
    while (at < end) {
      const event = EVENT_TABLE[f[at]], record = event.record, next = at + 2 + f[at + 1];
      // Structural birth binding may read only its declared records before
      // ordinary notifications drain. Skipping others must not decode their
      // references early or run a gameplay observer inside the native span.
      if (!selected || handlers[event.name]) {
        record.sync(this, f, at + 2, at + 2 + record.words, mirrors[event.code]);
        // Owning consumers acknowledge one record before invoking observers.
        // A nested command may replace views or append another event batch.
        if (cursor) cursor.consumeEvent(next - base - 1);
        handlers[event.name]?.(mirrors[event.code]);
      }
      at = next;
      if (cursor) break;
    }
    return at - base - 1;
  }

  // Serves an import the module called: its arguments out of the import
  // block (the same stretch of memory every time: rust/sim/src/store/
  // calls.rs), handlers[name](args), the result written back there. The
  // arguments object is the bridge's own, made once an import and read
  // into again at its next call: a handler keeps nothing of it (it copies
  // a vector it hands on to code that might keep it), and reads all it
  // needs before it calls code that could come back into the module (which
  // may make the same import again).
  serve(code) {
    const imp = IMPORT_TABLE[code], handler = this.handlers[imp.name];
    if (!handler) throw Error(`the simulation's module asked for ${imp.name}, which nothing serves`);
    // An owning phase can bind newly created identities from direct memory
    // before generated arguments resolve them. This hook never runs gameplay
    // notifications and does not enter another export on the native stack.
    this.beforeImport?.(code);
    const args = (this.served[code] ??= new imp.args.Mirror());
    const block = this.readBlock(this.exchange), base = block.base;
    const f = block.words, counters = this.phaseCounters;
    if (counters) { counters.importCalls++; counters.outputFields += f[base] * f[base + 1] + f[base + 3]; }
    if (this.boundaryCounters) {
      this.boundaryCounters.importCalls++; this.boundaryCounters.hooks++;
      this.boundaryCounters.outputWords += f[base] * f[base + 1] + f[base + 3];
    }
    const result = handler(imp.args.sync(this, f, base + HEADER, base + f[base + 2], args));
    if (!imp.result) return;
    const record = imp.result, heap = this.heap, words = record.words;
    if (this.rows.length < words) this.rows = new Float64Array(Math.max(words, this.rows.length * 2));
    heap.clear();
    record.put(this, result, this.rows, 0, heap);
    const hw = heap.words;
    if (counters) counters.inputFields += HEADER + words + hw.length;
    if (HEADER + words + hw.length > this.exchangeWords) throw Error(`${imp.name}: a result larger than the import block`);
    const resultBlock = this.readBlock(this.exchange);
    const g = resultBlock.words, at = resultBlock.base + HEADER, rows = this.rows;
    g[base] = 1; g[base + 1] = words; g[base + 2] = HEADER + words; g[base + 3] = hw.length; g[base + 4] = 0;
    for (let i = 0; i < words; i++) g[at + i] = rows[i];
    for (let i = 0; i < hw.length; i++) g[at + words + i] = hw[i];
    if (this.boundaryCounters) this.boundaryCounters.inputWords += words + hw.length;
  }

  // Every reader observes the backend's one view bank. A result read can
  // refresh it just as views() can, without leaving a second cache stale.
  get f() { return this.blocks.view.words; }
  get buffer() { return this.blocks.view.words.buffer; }
  get u8() { return this.blocks.bytes; }
  get grown() { return this.blocks.view.generation - 1; }
  views() { this.blocks.refresh(); }

  // Room for `bytes` more bytes of the battle's data, made as it loads so
  // play does not grow the memory (sim_reserve); returns the memory's pages.
  reserve(bytes) {
    return this.backend.reserve(bytes);
  }

  // Bytes are owned by the caller until this synchronous cold transfer ends.
  scratch(bytes) { this.backend.scratch(bytes); }

  // A kind's export block (`mode` as sim_export takes it: 0 the mirrored
  // fields after the retired entities' rows, 1 every field, 2 the mirrored
  // fields without the retired): calls visit(f, at, h, slot, gen, retired)
  // for each row (`at` its first word, `h` the heap's), and returns the row
  // count.
  exported(kind, mode, visit) {
    const ptr = this.backend.exportKind(kind, mode);
    if (!ptr) throw Error(`${KIND_TABLE[kind].name}: the store holds no fields of it`);
    const block = this.readBlock(ptr);
    const f = block.words, base = block.base, rows = f[base], stride = f[base + 1], h = base + f[base + 2], retired = f[base + 4];
    if (this.boundaryCounters) {
      this.boundaryCounters.outputWords += rows * stride + f[base + 3];
      this.boundaryCounters.publishedRows += rows;
    }
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
    if (this.backend.retire(kind, slot, gen) !== 0) throw Error(`${KIND_TABLE[kind].name}: no entity in slot ${slot} of generation ${gen}`);
  }

  // Writes the import block of a kind JavaScript owns from its objects (a
  // singleton's one object) in their order, its import fields or all of
  // them (`full`), and has the store take it: an entity of the kind the
  // block does not list leaves the store, as it left the JavaScript's list.
  write(kind, objects, full = false) {
    this.importRows(kind, objects, full, false);
  }

  // As write, for some of a kind's entities: the store keeps every entity
  // the block does not list as it is (its rows and whatever the store holds
  // of it only, such as a unit's combat). The way to write a few units (a
  // seat's riders, the unit a hook changed) without touching the others.
  writeSome(kind, objects, full = false) {
    this.importRows(kind, objects, full, true);
  }

  importRows(kind, objects, full, some) {
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
    const ptr = this.backend.importRoom(kind, n, stride, heap.words.length);
    const block = this.readBlock(ptr), base = block.base, f = block.words;
    f.set(rows.subarray(0, n * stride), base + HEADER);
    f.set(heap.words, base + HEADER + n * stride);
    if (this.boundaryCounters) this.boundaryCounters.inputWords += n * stride + heap.words.length;
    if (this.backend.importKind(kind, (full ? 1 : 0) | (some ? 2 : 0)) !== 0) throw Error(`${k.name}: the store holds no fields of it`);
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
