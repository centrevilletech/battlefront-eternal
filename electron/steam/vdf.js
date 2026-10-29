// Steam's binary KeyValues, as userdata/<account>/config/shortcuts.vdf
// holds the library's non-Steam games:
//
//   00 "shortcuts" 00
//     00 "0" 00  <fields> 08
//     00 "1" 00  <fields> 08
//   08
//   08
//
// A field is a type byte, its key (a NUL-terminated string) and its value:
// 00 a nested map up to its 08, 01 a NUL-terminated string, 02 a 32-bit
// integer, 07 a 64-bit one; 03 to 06 and 0a are KeyValues' other types,
// read so that a file holding them still parses. Strings are UTF-8.
//
// Other programs keep their own entries in the same file, so an entry
// this app does not change is written back with exactly the bytes it was
// read with (`raw`), and a file that does not parse is never written.

export const TYPE = Object.freeze({ MAP: 0x00, STRING: 0x01, INT32: 0x02, FLOAT32: 0x03, POINTER: 0x04, WIDE: 0x05, COLOR: 0x06, UINT64: 0x07, END: 0x08, INT64: 0x0a, END_ALT: 0x0b });

const FIXED = { [TYPE.INT32]: 4, [TYPE.FLOAT32]: 4, [TYPE.POINTER]: 4, [TYPE.COLOR]: 4, [TYPE.UINT64]: 8, [TYPE.INT64]: 8 };
// Deeper than any file Steam writes; a corrupt file cannot recurse forever.
const MAX_DEPTH = 32;

class Reader {
  constructor(buffer) {
    this.buffer = buffer;
    this.at = 0;
  }
  byte() {
    if (this.at >= this.buffer.length) throw new Error("shortcuts.vdf ends early");
    return this.buffer[this.at++];
  }
  cstring() {
    const end = this.buffer.indexOf(0, this.at);
    if (end < 0) throw new Error("shortcuts.vdf has an unterminated string");
    const text = this.buffer.toString("utf8", this.at, end);
    this.at = end + 1;
    return text;
  }
  wide() {
    let end = this.at;
    while (end + 1 < this.buffer.length && (this.buffer[end] || this.buffer[end + 1])) end += 2;
    if (end + 1 >= this.buffer.length) throw new Error("shortcuts.vdf has an unterminated wide string");
    const text = this.buffer.toString("utf16le", this.at, end);
    this.at = end + 2;
    return text;
  }
  take(size) {
    if (this.at + size > this.buffer.length) throw new Error("shortcuts.vdf ends early");
    const slice = this.buffer.subarray(this.at, this.at + size);
    this.at += size;
    return slice;
  }
}

function readValue(reader, type, depth) {
  switch (type) {
    case TYPE.MAP: return readMap(reader, depth + 1);
    case TYPE.STRING: return reader.cstring();
    case TYPE.WIDE: return reader.wide();
    case TYPE.INT32: case TYPE.POINTER: case TYPE.COLOR: return reader.take(4).readInt32LE(0);
    case TYPE.FLOAT32: return reader.take(4).readFloatLE(0);
    case TYPE.UINT64: return reader.take(8).readBigUInt64LE(0);
    case TYPE.INT64: return reader.take(8).readBigInt64LE(0);
    default: throw new Error(`shortcuts.vdf has an unknown field type ${type}`);
  }
}

// A map's fields up to its end byte: [{type, key, value}], in file order.
function readMap(reader, depth) {
  if (depth > MAX_DEPTH) throw new Error("shortcuts.vdf nests too deeply");
  const fields = [];
  for (;;) {
    const type = reader.byte();
    if (type === TYPE.END || type === TYPE.END_ALT) return fields;
    const key = reader.cstring();
    fields.push({ type, key, value: readValue(reader, type, depth) });
  }
}

function cstring(text) {
  if (String(text).includes("\0")) throw new Error("a shortcuts.vdf string cannot hold NUL");
  return Buffer.from(`${text}\0`, "utf8");
}

function writeValue(type, value) {
  switch (type) {
    case TYPE.MAP: return writeMap(value);
    case TYPE.STRING: return cstring(value ?? "");
    case TYPE.WIDE: return Buffer.from(`${value ?? ""}\0`, "utf16le");
    case TYPE.INT32: case TYPE.POINTER: case TYPE.COLOR: { const b = Buffer.alloc(4); b.writeInt32LE(Number(value) | 0); return b; }
    case TYPE.FLOAT32: { const b = Buffer.alloc(4); b.writeFloatLE(Number(value)); return b; }
    case TYPE.UINT64: { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt.asUintN(64, BigInt(value))); return b; }
    case TYPE.INT64: { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt.asIntN(64, BigInt(value))); return b; }
    default: throw new Error(`cannot write field type ${type}`);
  }
}

export function writeMap(fields) {
  const parts = [];
  for (const { type, key, value } of fields) parts.push(Buffer.from([type]), cstring(key), writeValue(type, value));
  parts.push(Buffer.from([TYPE.END]));
  return Buffer.concat(parts);
}

// shortcuts.vdf: { rootKey, entries: [{ key, fields, raw }] }. `raw` is the
// entry's map exactly as read (its fields and end byte), written back as
// it was unless the entry's fields are replaced. An empty or missing file
// is an empty list.
export function parseShortcuts(buffer) {
  if (!buffer || !buffer.length) return { rootKey: "shortcuts", entries: [] };
  const reader = new Reader(buffer);
  if (reader.byte() !== TYPE.MAP) throw new Error("shortcuts.vdf does not start with a map");
  const rootKey = reader.cstring();
  if (rootKey.toLowerCase() !== "shortcuts") throw new Error(`shortcuts.vdf starts with "${rootKey}"`);
  const entries = [];
  for (;;) {
    const type = reader.byte();
    if (type === TYPE.END || type === TYPE.END_ALT) break;
    if (type !== TYPE.MAP) throw new Error("shortcuts.vdf has a shortcut that is not a map");
    const key = reader.cstring();
    const start = reader.at;
    const fields = readMap(reader, 1);
    entries.push({ key, fields, raw: buffer.subarray(start, reader.at) });
  }
  // Steam ends the file with the root's own end byte; anything after it is
  // not KeyValues, and a file this app does not understand stays as it is.
  if (reader.at < buffer.length && reader.byte() !== TYPE.END) throw new Error("shortcuts.vdf has data after its list");
  if (reader.at !== buffer.length) throw new Error("shortcuts.vdf has data after its end");
  return { rootKey, entries };
}

// Entries are numbered from 0 in order, as Steam numbers them.
export function serializeShortcuts({ rootKey = "shortcuts", entries }) {
  const parts = [Buffer.from([TYPE.MAP]), cstring(rootKey)];
  entries.forEach((entry, index) => {
    parts.push(Buffer.from([TYPE.MAP]), cstring(String(index)), entry.raw ?? writeMap(entry.fields));
  });
  parts.push(Buffer.from([TYPE.END, TYPE.END]));
  return Buffer.concat(parts);
}

// A field's value by key, as Steam reads them: case-insensitively (older
// files say "appname" and "exe").
export function field(fields, key) {
  const lower = key.toLowerCase();
  return fields.find((item) => item.key.toLowerCase() === lower)?.value;
}

// ---- Text KeyValues (loginusers.vdf, libraryfolders.vdf) ----

// A nested object of strings. Keys keep their case; escapes as Steam
// writes them (\\, \", \n, \t).
export function parseTextVdf(text) {
  const tokens = [...String(text).matchAll(/"((?:[^"\\]|\\.)*)"|([{}])|\/\/[^\n]*/g)];
  const unescape = (value) => value.replace(/\\(.)/g, (_, c) => ({ n: "\n", t: "\t" })[c] ?? c);
  let at = 0;
  const readObject = (depth) => {
    const out = {};
    while (at < tokens.length) {
      const token = tokens[at++];
      if (token[0].startsWith("//")) continue;
      if (token[2] === "}") return out;
      if (token[1] === undefined) continue;
      const key = unescape(token[1]);
      let next = tokens[at];
      while (next && next[0].startsWith("//")) next = tokens[++at];
      if (!next) break;
      at++;
      if (next[2] === "{") out[key] = depth < MAX_DEPTH ? readObject(depth + 1) : {};
      else if (next[1] !== undefined) out[key] = unescape(next[1]);
    }
    return out;
  };
  return readObject(0);
}
