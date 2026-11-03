// The simulation capture format (BFECAP, version 1): what tools/sim-capture.mjs
// records of a running battle and what rust/sim-replay reads back, exactly:
// every number keeps its 64 bits (-0 and NaN included), every string its
// UTF-16 code units, every object its key order and its identity (an object
// met twice in a record is written once and then referred to), and what
// a record names but does not hold (a unit, a hull, a collision body, a
// content definition) is a handle.
//
// A file is the eight bytes "BFECAP01" and then records, each:
//   varuint length of the rest | u8 type | varuint tick | string name | value
// Types: 1 the header (the file's first record), 2 a frame (a phase or a
// method of one, captured), 3 a tick (what every phase drew, and the
// tick's fingerprints), 4 a definition from the game's content, or another
// object handles name that never changes (an owner snapshot, under
// ownerSnapshot:N), named by its key and written once, right after the
// first record that names it, 5 the world (the units' lives, teams and
// classes before a frame that asks for them). Strings and object identities are counted afresh in
// every record, so a reader can skip any record by its length.
//
// Values, a tag byte and what follows:
//   0x00 undefined   0x01 null   0x02 false   0x03 true
//   0x04 number, f64 little-endian
//   0x05 number, an integer of magnitude under 2^52 (not -0): zigzag varint
//   0x06 string, new: varuint UTF-8 length, the bytes (well-formed strings)
//   0x07 string, new: varuint UTF-16 length, u16 code units (any other)
//   0x08 string met before in this record: varuint its number
//   0x09 an object met before in this record: varuint its number
//   0x0a array: varuint n, n values
//   0x0b object: string class ("" for a plain object), varuint n, n x (string key, value)
//   0x0c Vector3: 3 x f64   0x0d Quaternion: 4 x f64 (x, y, z, w)
//   0x0e Matrix4: 16 x f64 (elements)   0x0f Matrix3: 9 x f64
//   0x10 Euler: 3 x f64, string order   0x11 Box3: 6 x f64 (min, max)
//   0x12 typed array: u8 kind (TYPED below), varuint length, the elements little-endian
//   0x13 Map: varuint n, n x (key, value)   0x14 Set: varuint n, n values
//   0x15 function: string name
//   0x16 handle: string kind, value key (a number or a string)
//   0x17 left out: string why (a getter, too deep, a kind never captured)
// Arrays, objects, the three.js values, typed arrays, Maps and Sets are
// numbered in the order they are first met (from 0 in each record).
// Strings are numbered the same way, apart from objects.
export const MAGIC = 'BFECAP01';
export const RECORD = { header: 1, frame: 2, tick: 3, def: 4, world: 5 };
export const TAG = {
  undefined: 0x00, null: 0x01, false: 0x02, true: 0x03, f64: 0x04, int: 0x05, utf8: 0x06, utf16: 0x07, stringRef: 0x08,
  objectRef: 0x09, array: 0x0a, object: 0x0b, vec3: 0x0c, quat: 0x0d, mat4: 0x0e, mat3: 0x0f, euler: 0x10, box3: 0x11,
  typed: 0x12, map: 0x13, set: 0x14, function: 0x15, handle: 0x16, omitted: 0x17,
};
export const TYPED = [null, Float32Array, Float64Array, Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
  Int32Array, Uint32Array, BigInt64Array, BigUint64Array];

// A handle: what a record names without holding it. kind: 'unit', 'vehicle',
// 'body', 'def', 'system', ...; key: its number or name.
export class Handle {
  constructor(kind, key) { this.kind = kind; this.key = key; }
}
// What a record leaves out, and why.
export class Omitted {
  constructor(why) { this.why = why; }
}
// A function a record names (by its name).
export class FunctionName {
  constructor(name) { this.name = name; }
}

const encoder = new TextEncoder();
const f64 = new Float64Array(1), f64Bytes = new Uint8Array(f64.buffer);

// Writes records into one growing buffer. value() is the default value
// writer; a caller who knows its objects passes `identify(object)`, which
// returns a Handle, an Omitted, or undefined to write the object out.
export class Writer {
  constructor({ identify = () => undefined, maxDepth = 64 } = {}) {
    this.bytes = new Uint8Array(1 << 16);
    this.length = 0;
    this.identify = identify;
    this.maxDepth = maxDepth;
    // Objects written out whole wherever met, whatever identify says.
    this.whole = new Set();
  }

  reserve(n) {
    if (this.length + n <= this.bytes.length) return;
    let size = this.bytes.length * 2;
    while (size < this.length + n) size *= 2;
    const grown = new Uint8Array(size);
    grown.set(this.bytes.subarray(0, this.length));
    this.bytes = grown;
  }
  byte(b) { this.reserve(1); this.bytes[this.length++] = b; }
  raw(array) { this.reserve(array.length); this.bytes.set(array, this.length); this.length += array.length; }
  varuint(n) {
    // Up to 2^53, without 32-bit operators.
    if (!(Number.isInteger(n) && n >= 0 && n <= Number.MAX_SAFE_INTEGER)) throw Error(`varuint ${n}`);
    while (n >= 128) { this.byte((n % 128) + 128); n = Math.floor(n / 128); }
    this.byte(n);
  }
  float(x) { f64[0] = x; this.raw(f64Bytes); }

  // A whole record: its type, tick, name and value.
  record(type, tick, name, value) {
    const start = this.length;
    this.reserve(10);
    this.length += 10; // room for the length, moved below once known
    const body = this.length;
    this.byte(type);
    this.varuint(tick);
    this.strings = new Map();
    this.objects = new Map();
    this.string(name);
    this.value(value, 0);
    const size = this.length - body;
    const head = [];
    for (let n = size; ; n = Math.floor(n / 128)) { if (n < 128) { head.push(n); break; } head.push((n % 128) + 128); }
    this.bytes.copyWithin(start + head.length, body, this.length);
    this.bytes.set(head, start);
    this.length = start + head.length + size;
    this.strings = this.objects = null;
  }

  string(s) {
    const known = this.strings.get(s);
    if (known !== undefined) { this.byte(TAG.stringRef); this.varuint(known); return; }
    this.strings.set(s, this.strings.size);
    if (s.isWellFormed()) {
      const utf8 = encoder.encode(s);
      this.byte(TAG.utf8); this.varuint(utf8.length); this.raw(utf8);
    } else {
      this.byte(TAG.utf16); this.varuint(s.length);
      for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); this.byte(c & 0xff); this.byte(c >>> 8); }
    }
  }

  number(x) {
    if (Number.isInteger(x) && Math.abs(x) < 2 ** 52 && !Object.is(x, -0)) {
      this.byte(TAG.int);
      this.varuint(x >= 0 ? x * 2 : -x * 2 - 1);
    } else { this.byte(TAG.f64); this.float(x); }
  }

  // Numbered the first time: false then (the caller writes it out).
  seen(object) {
    const known = this.objects.get(object);
    if (known !== undefined) { this.byte(TAG.objectRef); this.varuint(known); return true; }
    this.objects.set(object, this.objects.size);
    return false;
  }

  value(v, depth) {
    switch (typeof v) {
      case 'undefined': return this.byte(TAG.undefined);
      case 'boolean': return this.byte(v ? TAG.true : TAG.false);
      case 'number': return this.number(v);
      case 'string': return this.string(v);
      case 'function': this.byte(TAG.function); return this.string(v.name || '');
      case 'bigint': case 'symbol': this.byte(TAG.omitted); return this.string(typeof v);
    }
    if (v === null) return this.byte(TAG.null);
    if (v instanceof Handle) { this.byte(TAG.handle); this.string(v.kind); return typeof v.key === 'number' ? this.number(v.key) : this.string(String(v.key)); }
    if (v instanceof Omitted) { this.byte(TAG.omitted); return this.string(v.why); }
    if (v instanceof FunctionName) { this.byte(TAG.function); return this.string(v.name); }
    if (this.objects.has(v)) return this.seen(v);
    const id = this.whole.has(v) ? undefined : this.identify(v);
    if (id !== undefined) return this.value(id, depth);
    if (depth >= this.maxDepth) { this.byte(TAG.omitted); return this.string('too deep'); }
    if (this.seen(v)) return;
    const d = depth + 1;
    if (v.isVector3) { this.byte(TAG.vec3); this.float(v.x); this.float(v.y); return this.float(v.z); }
    if (v.isQuaternion) { this.byte(TAG.quat); this.float(v._x); this.float(v._y); this.float(v._z); return this.float(v._w); }
    if (v.isMatrix4) { this.byte(TAG.mat4); for (const e of v.elements) this.float(e); return; }
    if (v.isMatrix3) { this.byte(TAG.mat3); for (const e of v.elements) this.float(e); return; }
    if (v.isEuler) { this.byte(TAG.euler); this.float(v._x); this.float(v._y); this.float(v._z); return this.string(v._order); }
    if (v.isBox3) { this.byte(TAG.box3); for (const p of [v.min, v.max]) { this.float(p.x); this.float(p.y); this.float(p.z); } return; }
    if (Array.isArray(v)) { this.byte(TAG.array); this.varuint(v.length); for (let i = 0; i < v.length; i++) this.value(v[i], d); return; }
    if (ArrayBuffer.isView(v) && !(v instanceof DataView)) {
      const kind = TYPED.findIndex((T) => T && v instanceof T);
      this.byte(TAG.typed); this.byte(kind); this.varuint(v.length);
      return this.raw(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
    }
    if (v instanceof Map) { this.byte(TAG.map); this.varuint(v.size); for (const [k, x] of v) { this.value(k, d); this.value(x, d); } return; }
    if (v instanceof Set) { this.byte(TAG.set); this.varuint(v.size); for (const x of v) this.value(x, d); return; }
    // Any other object: its own enumerable keys in their order, a getter
    // left out (reading one could run game code: a lazy shape, a count).
    const proto = Object.getPrototypeOf(v);
    const className = proto === Object.prototype || proto === null ? '' : (proto.constructor?.name ?? '?');
    this.byte(TAG.object); this.string(className);
    const keys = Object.keys(v);
    this.varuint(keys.length);
    for (const key of keys) {
      this.string(key);
      const desc = Object.getOwnPropertyDescriptor(v, key);
      if (desc.get || desc.set) { this.byte(TAG.omitted); this.string('getter'); } else this.value(desc.value, d);
    }
  }

  // The file's bytes so far.
  take() { const out = this.bytes.slice(0, this.length); this.length = 0; return out; }
}

// Reads a file's records back into plain values (Handle, Omitted and
// FunctionName as above; three.js values as { $: 'vec3', x, y, z } and so
// on), for tests and for looking into a capture.
export function* readRecords(bytes) {
  if (new TextDecoder().decode(bytes.subarray(0, 8)) !== MAGIC) throw Error('not a BFECAP01 file');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 8;
  const varuint = () => { let n = 0, scale = 1, b; do { b = bytes[at++]; n += (b & 127) * scale; scale *= 128; } while (b & 128); return n; };
  const float = () => { const x = view.getFloat64(at, true); at += 8; return x; };
  while (at < bytes.length) {
    const size = varuint(), end = at + size;
    const type = bytes[at++], tick = varuint();
    const strings = [], objects = [];
    const string = () => {
      const tag = bytes[at++];
      if (tag === TAG.stringRef) return strings[varuint()];
      const n = varuint();
      let s;
      if (tag === TAG.utf8) { s = new TextDecoder().decode(bytes.subarray(at, at + n)); at += n; }
      else if (tag === TAG.utf16) { s = ''; for (let i = 0; i < n; i++, at += 2) s += String.fromCharCode(bytes[at] | (bytes[at + 1] << 8)); }
      else throw Error(`string tag ${tag} at ${at - 1}`);
      strings.push(s);
      return s;
    };
    const value = () => {
      const tag = bytes[at++];
      switch (tag) {
        case TAG.undefined: return undefined;
        case TAG.null: return null;
        case TAG.false: return false;
        case TAG.true: return true;
        case TAG.f64: return float();
        case TAG.int: { const z = varuint(); return z % 2 ? -(z + 1) / 2 : z / 2; }
        case TAG.utf8: case TAG.utf16: case TAG.stringRef: at--; return string();
        case TAG.objectRef: return objects[varuint()];
        case TAG.array: { const a = []; objects.push(a); const n = varuint(); for (let i = 0; i < n; i++) a.push(value()); return a; }
        case TAG.object: {
          const className = string(), o = className ? { $class: className } : {};
          objects.push(o);
          const n = varuint();
          for (let i = 0; i < n; i++) { const k = string(); o[k] = value(); }
          return o;
        }
        case TAG.vec3: { const o = { $: 'vec3', x: float(), y: float(), z: float() }; objects.push(o); return o; }
        case TAG.quat: { const o = { $: 'quat', x: float(), y: float(), z: float(), w: float() }; objects.push(o); return o; }
        case TAG.mat4: { const o = { $: 'mat4', elements: Array.from({ length: 16 }, float) }; objects.push(o); return o; }
        case TAG.mat3: { const o = { $: 'mat3', elements: Array.from({ length: 9 }, float) }; objects.push(o); return o; }
        case TAG.euler: { const o = { $: 'euler', x: float(), y: float(), z: float() }; objects.push(o); o.order = string(); return o; }
        case TAG.box3: { const o = { $: 'box3', min: [float(), float(), float()], max: [float(), float(), float()] }; objects.push(o); return o; }
        case TAG.typed: {
          const T = TYPED[bytes[at++]], n = varuint(), size = n * T.BYTES_PER_ELEMENT;
          const copy = new Uint8Array(bytes.subarray(at, at + size)); at += size;
          const o = new T(copy.buffer); objects.push(o); return o;
        }
        case TAG.map: { const m = new Map(); objects.push(m); const n = varuint(); for (let i = 0; i < n; i++) { const k = value(); m.set(k, value()); } return m; }
        case TAG.set: { const s = new Set(); objects.push(s); const n = varuint(); for (let i = 0; i < n; i++) s.add(value()); return s; }
        case TAG.function: return new FunctionName(string());
        case TAG.handle: { const kind = string(); return new Handle(kind, value()); }
        case TAG.omitted: return new Omitted(string());
        default: throw Error(`value tag ${tag} at ${at - 1}`);
      }
    };
    const name = string(), v = value();
    if (at !== end) throw Error(`record at ${end - size} ends at ${at}, not ${end}`);
    yield { type, tick, name, value: v };
  }
}
