// The binary tick packet:
// one ArrayBuffer per tick, transferred to the page or copied into the
// shared ring (transport.js SharedRing), never cloned. Values are written to
// four typed streams (tags and masks in bytes, ids and bitmasks in 16-bit
// words, small integers in 32-bit words, everything else in float64) laid
// out as aligned sections of the one buffer, followed by the packet's text
// part: a JSON object with the fields that are references or rare (built
// units, seats, calls, list refs, the worker's health) and the definitions
// the binary part needs (new strings, new schemas).
//
// Layout (byte offsets):
//   0   Uint32[8]   magic, u8 count, u16 count, i32 count, f64 count,
//                   text bytes, flags, 0
//   32  Uint8[]     tags, flags
//   ..  Uint16[]    string ids (keys, schemas, strings), bitmask words
//   ..  Int32[]     32-bit integers, list ids
//   ..  Float64[]   numbers, vector components
//   ..  UTF-8       the JSON text
// Each section starts at a multiple of its element size.
//
// A tracked object's record: its key (a string id), its schema (an id into
// the schema table: the object's field names in a fixed order, taken from
// the first observation of that prototype's key list), one bitmask word per
// 16 fields saying which fields follow, then the changed fields' values in
// schema order, each a tag byte and its payload:
//   0 null  1 undefined  2 false  3 true  4 float64  5 int32  6 string id
//   7 Vector2 (2 f64)  8 Vector3 (3 f64)  9 Quaternion (4 f64)
//   10 Euler (3 f64 + order string id)  11 number array (u16 length, f64s)
// Strings are interned once per session and travel in the text part the
// first time they are used; a keyframe (the first packet, and the one that
// answers a resync) carries every string and schema again.

const MAGIC = 0x31454642; // "BFE1"
const HEADER_BYTES = 32;
export const TAG = { NULL: 0, UNDEFINED: 1, FALSE: 2, TRUE: 3, F64: 4, I32: 5, STRING: 6, VEC2: 7, VEC3: 8, VEC4: 9, EULER: 10, ARRAY: 11 };

const encoder = new TextEncoder(), decoder = new TextDecoder();
// Non-finite numbers in the text part (a never-expiring item's life), which
// JSON has no spelling for.
const replacer = (key, value) => (typeof value === 'number' && !Number.isFinite(value) ? { $f: value > 0 ? 1 : value < 0 ? -1 : 0 } : value);
const reviver = (key, value) => (value && typeof value === 'object' && '$f' in value ? (value.$f > 0 ? Infinity : value.$f < 0 ? -Infinity : NaN) : value);

const grow = (array) => { const next = new array.constructor(array.length * 2); next.set(array); return next; };
const align = (offset, size) => (offset + size - 1) & ~(size - 1);
// An integer the 32-bit stream can carry exactly (not -0, not fractional,
// not beyond the sign bit).
export const isInt32 = (v) => (v | 0) === v && (v !== 0 || 1 / v > 0);

export class BinaryWriter {
  constructor({ network = false } = {}) {
    this.network = network;
    this.u8 = new Uint8Array(8192); this.u16 = new Uint16Array(4096); this.i32 = new Int32Array(1024); this.f64 = new Float64Array(4096);
    this.n8 = this.n16 = this.n32 = this.n64 = 0;
    this.strings = new Map(); this.stringList = []; this.newStrings = [];
    this.schemaIds = new Map(); // keys array -> Map(link -> id)
    this.schemaList = []; this.newSchemas = [];
    this.full = false;
  }
  // A new packet: the streams empty, definitions since the last one kept.
  begin() { this.n8 = this.n16 = this.n32 = this.n64 = 0; }
  // The next packet carries every definition (a keyframe).
  keyframe() { this.full = true; }

  put8(v) { if (this.n8 === this.u8.length) this.u8 = grow(this.u8); this.u8[this.n8++] = v; }
  put16(v) { if (this.n16 === this.u16.length) this.u16 = grow(this.u16); this.u16[this.n16++] = v; }
  put32(v) { if (this.n32 === this.i32.length) this.i32 = grow(this.i32); this.i32[this.n32++] = v; }
  putF(v) { if (this.n64 === this.f64.length) this.f64 = grow(this.f64); this.f64[this.n64++] = this.network && Number.isFinite(v) && Math.abs(v) < 2 ** 30 ? Math.round(v * 65536) / 65536 : v; }
  // `count` zeroed 16-bit words (a bitmask), returning their index.
  reserve16(count) {
    if (this.n16 + count > this.u16.length) this.u16 = grow(this.u16);
    const at = this.n16, u16 = this.u16;
    for (let i = 0; i < count; i++) u16[at + i] = 0;
    this.n16 = at + count;
    return at;
  }
  // A string's id, interned for the session.
  string(s) {
    let id = this.strings.get(s);
    if (id === undefined) {
      id = this.stringList.length;
      if (id > 0xffff) throw new Error('packet string table full');
      this.strings.set(s, id); this.stringList.push(s); this.newStrings.push(s);
    }
    return id;
  }
  // The schema id for a key list (one per prototype key order), with the
  // link field (replica.js: a body's character, a character's unit) first.
  schema(keys, link = null) {
    let byLink = this.schemaIds.get(keys);
    if (!byLink) this.schemaIds.set(keys, (byLink = new Map()));
    let id = byLink.get(link);
    if (id === undefined) {
      id = this.schemaList.length;
      const fields = (link ? [link] : []).concat(keys).map((name) => this.string(name));
      this.schemaList.push(fields); this.newSchemas.push([id, fields]);
      byLink.set(link, id);
    }
    return id;
  }
  // A plain value (number, string, boolean, null, undefined).
  value(v) {
    if (v === null) this.put8(TAG.NULL);
    else if (v === undefined) this.put8(TAG.UNDEFINED);
    else {
      const t = typeof v;
      if (t === 'number') { if (isInt32(v)) { this.put8(TAG.I32); this.put32(v); } else { this.put8(TAG.F64); this.putF(v); } }
      else if (t === 'boolean') this.put8(v ? TAG.TRUE : TAG.FALSE);
      else if (t === 'string') { this.put8(TAG.STRING); this.put16(this.string(v)); }
      else throw new Error(`packet cannot carry a ${t}`);
    }
  }
  numbers(array) {
    this.put8(TAG.ARRAY); this.put16(array.length);
    for (let i = 0; i < array.length; i++) this.putF(array[i]);
  }

  // The packet: the streams, then `rest` (plus the definitions) as text.
  finish(rest) {
    const full = this.full;
    rest.defs = full ? { s: this.stringList, k: this.schemaList.map((fields, id) => [id, fields]), full: true } : { s: this.newStrings, k: this.newSchemas };
    const text = encoder.encode(JSON.stringify(rest, replacer));
    this.newStrings = []; this.newSchemas = []; this.full = false;
    const o8 = HEADER_BYTES, o16 = align(o8 + this.n8, 2), o32 = align(o16 + this.n16 * 2, 4), o64 = align(o32 + this.n32 * 4, 8), oText = o64 + this.n64 * 8;
    const buffer = new ArrayBuffer(oText + text.length);
    new Uint32Array(buffer, 0, 8).set([MAGIC, this.n8, this.n16, this.n32, this.n64, text.length, 0, 0]);
    new Uint8Array(buffer, o8, this.n8).set(this.u8.subarray(0, this.n8));
    new Uint16Array(buffer, o16, this.n16).set(this.u16.subarray(0, this.n16));
    new Int32Array(buffer, o32, this.n32).set(this.i32.subarray(0, this.n32));
    new Float64Array(buffer, o64, this.n64).set(this.f64.subarray(0, this.n64));
    new Uint8Array(buffer, oText, text.length).set(text);
    return buffer;
  }
}

// The page's side: the string and schema tables, and each packet's views.
export class BinaryReader {
  constructor() { this.strings = []; this.schemas = []; }
  // The packet's text part as an object, with `bin` (the streams and a
  // cursor into each) and `reader` (this) for the state and list readers.
  read(buffer, byteOffset = 0, accept = null) {
    const header = new Uint32Array(buffer, byteOffset, 8);
    if (header[0] !== MAGIC) throw new Error('not a tick packet');
    const n8 = header[1], n16 = header[2], n32 = header[3], n64 = header[4], textLength = header[5];
    const o8 = byteOffset + HEADER_BYTES, o16 = align(o8 + n8, 2), o32 = align(o16 + n16 * 2, 4), o64 = align(o32 + n32 * 4, 8), oText = o64 + n64 * 8;
    const text = decoder.decode(new Uint8Array(buffer, oText, textLength));
    const rest = JSON.parse(text, text.includes('"$f"') ? reviver : undefined);
    if (accept && !accept(rest)) return null;
    const defs = rest.defs;
    if (defs) {
      if (defs.full) { this.strings.length = 0; this.schemas.length = 0; }
      for (const s of defs.s) this.strings.push(s);
      for (const [id, fields] of defs.k) this.schemas[id] = fields.map((i) => this.strings[i]);
    }
    rest.bin = { u8: new Uint8Array(buffer, o8, n8), u16: new Uint16Array(buffer, o16, n16), i32: new Int32Array(buffer, o32, n32), f64: new Float64Array(buffer, o64, n64), p8: 0, p16: 0, p32: 0, p64: 0 };
    rest.reader = this;
    rest.bytes = oText + textLength - byteOffset;
    return rest;
  }
}
