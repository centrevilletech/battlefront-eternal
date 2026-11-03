// Makes the simulation store's code from its schemas (docs/sim-store.md):
// every kind of entity, record, enum and union declared in
// rust/sim/schema/*.mjs becomes, on the Rust side (rust/sim/src/gen/), a
// struct and the code that copies it to and from the sync blocks, and on
// the JavaScript side (src/sim/gen/), a mirror class with the same fields
// as the JavaScript objects it stands for, the loops that copy those fields
// to and from the sync blocks, a comparer, the mirror write detector, and
// an exporter that turns an object into the plain values the replay
// harness records (tools/sim-capture). Both sides come from the same
// declarations, so they cannot disagree about a field.
//
//   node tools/sim-schema.mjs           write the generated files
//   node tools/sim-schema.mjs --check   compare them with what is there;
//                                       exit 1 if any differ
//
// The generated files are committed (the game builds without running
// this); tools/build-rust.mjs runs the check before it builds the module.
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
// The shipped schemas, and the generator's own test schemas, whose code is
// compiled into the crate's tests only (rust/sim/src/gen_test, cfg(test))
// and imported by the JavaScript tests only (tests/sim-gen): each target is
// generated alike, with its own kinds and codes.
export const TARGETS = [
  { name: 'gen', schema: 'rust/sim/schema', rust: 'rust/sim/src/gen', js: 'src/sim/gen', compare: '../compare.js', test: false },
  { name: 'gen_test', schema: 'rust/sim/schema-test', rust: 'rust/sim/src/gen_test', js: 'tests/sim-gen', compare: '../../src/sim/compare.js', test: true },
];
const SCHEMA = join(ROOT, TARGETS[0].schema);

const SCALARS = new Set(['f64', 'f32', 'i32', 'u32', 'u8', 'bool', 'vec3', 'quat', 'str']);
const RUST_KEYWORDS = new Set(['as', 'break', 'const', 'continue', 'crate', 'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'static', 'struct', 'super', 'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while', 'async', 'await', 'dyn', 'abstract', 'become', 'box', 'do', 'final', 'macro', 'override', 'priv', 'typeof', 'unsized', 'virtual', 'yield', 'try', 'gen']);

const pascal = (s) => s.replace(/[^A-Za-z0-9]+(.)?/g, (_, c) => (c ? c.toUpperCase() : '')).replace(/^./, (c) => c.toUpperCase());
const snake = (s) => {
  const out = s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase();
  return RUST_KEYWORDS.has(out) ? `r#${out}` : out;
};
const upper = (s) => snake(s).replace(/^r#/, '').toUpperCase();
const kindConst = (kind) => `KIND_${upper(kind)}`;
const jsKey = (name) => (/^[A-Za-z_$][\w$]*$/.test(name) ? `.${name}` : `[${JSON.stringify(name)}]`);
const jsPath = (path) => path.map(jsKey).join('');

// FNV-1a over UTF-16 units: a layout's hash, the same on both sides.
function fnv(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) { h ^= text.charCodeAt(i); h = Math.imul(h, 0x01000193) >>> 0; }
  return h >>> 0;
}

// ---- reading the schemas ----

export async function loadSchemas(dir = SCHEMA) {
  const files = existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith('.mjs')).sort() : [];
  const out = [];
  for (const file of files) {
    const mod = await import(pathToFileURL(join(dir, file)).href);
    out.push({ file: file.replace(/\.mjs$/, ''), schema: mod.default });
  }
  return out;
}

function fail(where, message) { throw Error(`${where}: ${message}`); }

// A field's type: its base and whether it can be null, undefined or both.
export function parseType(text, where) {
  if (typeof text !== 'string') fail(where, 'a type must be a string');
  const [base, ...rest] = text.split('|').map((s) => s.trim());
  const type = { text, nullable: { null: false, undefined: false } };
  for (const r of rest) {
    if (r !== 'null' && r !== 'undefined') fail(where, `"${r}" in a type can only be null or undefined`);
    if (type.nullable[r]) fail(where, `${r} twice`);
    type.nullable[r] = true;
  }
  let m;
  if (base.startsWith('map:')) {
    // A Map with string keys; its values' type follows (never null).
    type.base = 'map';
    type.value = parseType(base.slice(4), where);
    if (optional(type.value) || type.value.base === 'map') fail(where, 'a map\'s values are a type that is never null or undefined, and no map');
  } else if (SCALARS.has(base)) type.base = base;
  else if ((m = /^f64x([2-9]|1[0-6])$/.exec(base))) { type.base = 'f64xN'; type.n = Number(m[1]); }
  else if ((m = /^(enum|ref|union|def|rec|list|set):([A-Za-z_][\w]*)$/.exec(base))) { type.base = m[1]; type.of = m[2]; }
  else fail(where, `unknown type "${base}"`);
  if ((type.base === 'list' || type.base === 'set') && (type.nullable.null && type.nullable.undefined)) fail(where, 'a list or set can be null or undefined, not both');
  return type;
}

const optional = (t) => t.nullable.null || t.nullable.undefined;

function parseField(name, spec, where) {
  const s = typeof spec === 'string' ? { type: spec } : spec;
  if (!s || typeof s !== 'object') fail(where, 'a field is a type or { type, js, mirror, import }');
  for (const k of Object.keys(s)) if (!['type', 'js', 'mirror', 'import', 'doc'].includes(k)) fail(where, `unknown field option "${k}"`);
  const type = parseType(s.type, where);
  const js = (s.js ?? name).split('.');
  const mirror = s.mirror ?? true;
  if (mirror && js.length > 1) fail(where, `a field read from ${s.js} cannot be mirrored (set mirror: false)`);
  return { name, js, type, mirror, import: s.import ?? true, where };
}

export function buildModel(files) {
  const model = { enums: new Map(), unions: new Map(), records: new Map(), kinds: new Map(), commands: new Map(), events: new Map(), imports: new Map(), files: files.map((f) => f.file) };
  const names = new Map(); // Rust type name -> where
  const claim = (rust, where) => {
    if (names.has(rust)) fail(where, `${rust} is also ${names.get(rust)}`);
    names.set(rust, where);
  };
  for (const { file, schema } of files) {
    for (const k of Object.keys(schema)) if (!['enums', 'unions', 'records', 'kinds', 'commands', 'events', 'imports'].includes(k)) fail(file, `unknown section "${k}"`);
    // A command (JavaScript writing into Rust's state), an event (Rust telling
    // JavaScript) and an import (Rust asking JavaScript): each names records
    // for its arguments, result or payload; a command names the Rust function
    // that runs it.
    for (const [name, spec] of Object.entries(schema.commands || {})) {
      const where = `${file} command ${name}`;
      if (model.commands.has(name)) fail(where, 'declared twice');
      if (!spec || typeof spec.args !== 'string' || typeof spec.rust !== 'string') fail(where, 'a command is { args: Record, result: Record or null, rust: "module::function" }');
      model.commands.set(name, { name, file, args: spec.args, result: spec.result ?? null, rust: spec.rust, where });
    }
    for (const [name, spec] of Object.entries(schema.events || {})) {
      const where = `${file} event ${name}`;
      if (model.events.has(name)) fail(where, 'declared twice');
      if (typeof spec !== 'string') fail(where, 'an event is the name of the record it carries');
      model.events.set(name, { name, file, record: spec, where });
    }
    for (const [name, spec] of Object.entries(schema.imports || {})) {
      const where = `${file} import ${name}`;
      if (model.imports.has(name)) fail(where, 'declared twice');
      if (!spec || typeof spec.args !== 'string') fail(where, 'an import is { args: Record, result: Record or null }');
      model.imports.set(name, { name, file, args: spec.args, result: spec.result ?? null, where });
    }
    for (const [name, values] of Object.entries(schema.enums || {})) {
      const where = `${file} enum ${name}`;
      if (!Array.isArray(values) || !values.length || values.some((v) => typeof v !== 'string') || new Set(values).size !== values.length || values.length > 255) fail(where, 'an enum is a list of distinct strings');
      if (model.enums.has(name)) fail(where, 'declared twice');
      claim(name, where);
      model.enums.set(name, { name, values, file, variants: values.map(pascal) });
    }
    for (const [name, list] of Object.entries(schema.unions || {})) {
      const where = `${file} union ${name}`;
      if (model.unions.has(name)) fail(where, 'declared twice');
      claim(name, where);
      const variants = list.map((v) => {
        if (v === 'null' || v === 'undefined') return { tag: v, rust: pascal(v) };
        let m;
        if ((m = /^lit:(.+)$/.exec(v))) return { tag: 'lit', value: m[1], rust: pascal(m[1]) };
        if ((m = /^ref:([A-Za-z_]\w*)$/.exec(v))) return { tag: 'ref', kind: m[1], rust: pascal(m[1]) };
        return fail(where, `"${v}" is not null, undefined, lit:<text> or ref:<kind>`);
      });
      if (new Set(variants.map((v) => v.rust)).size !== variants.length) fail(where, 'two variants of one name');
      model.unions.set(name, { name, variants, file });
    }
    for (const [name, fields] of Object.entries(schema.records || {})) {
      const where = `${file} record ${name}`;
      if (model.records.has(name)) fail(where, 'declared twice');
      claim(name, where);
      model.records.set(name, { name, file, fields: Object.entries(fields).map(([f, spec]) => parseField(f, spec, `${where}.${f}`)) });
    }
    for (const [name, spec] of Object.entries(schema.kinds || {})) {
      const where = `${file} kind ${name}`;
      for (const k of Object.keys(spec)) if (!['singleton', 'key', 'home', 'fields', 'skip', 'class', 'doc'].includes(k)) fail(where, `unknown kind option "${k}"`);
      let kind = model.kinds.get(name);
      if (!kind) {
        kind = { name, rust: pascal(name), singleton: false, key: null, home: null, classes: [], fields: [], skip: new Map(), file: null, facets: [] };
        model.kinds.set(name, kind);
      }
      kind.facets.push(file);
      for (const opt of ['singleton', 'key', 'home']) {
        if (spec[opt] === undefined) continue;
        if (kind[`${opt}Set`] && JSON.stringify(kind[opt]) !== JSON.stringify(spec[opt])) fail(where, `${opt} differs from another declaration`);
        kind[opt] = spec[opt];
        kind[`${opt}Set`] = true;
      }
      for (const [f, spec2] of Object.entries(spec.fields || {})) {
        const field = parseField(f, spec2, `${where}.${f}`);
        const had = kind.fields.find((x) => x.name === f);
        if (had) {
          if (had.type.text !== field.type.text || had.js.join('.') !== field.js.join('.')) fail(where, `field ${f} is declared otherwise in ${had.where}`);
          had.mirror ||= field.mirror;
          had.import ||= field.import;
          continue;
        }
        kind.fields.push(field);
        kind.file ??= file;
      }
      for (const [f, why] of Object.entries(spec.skip || {})) kind.skip.set(f, why);
      for (const c of [spec.class ?? []].flat()) if (!kind.classes.includes(c)) kind.classes.push(c);
    }
  }
  // Codes: the kinds' names in order, from 1.
  const kinds = [...model.kinds.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  kinds.forEach((k, i) => { k.code = i + 1; });
  model.kindList = kinds;
  for (const k of kinds) {
    const where = `kind ${k.name}`;
    k.stored = k.fields.length > 0;
    if (!k.file) k.file = null;
    if (k.stored || k.singleton) claim(k.rust, where);
    if (k.key !== null && typeof k.key !== 'string') fail(where, 'key names the numeric field that identifies an object');
    if (k.singleton && (k.key || k.home)) fail(where, 'a singleton has no key or home');
    if (k.home) {
      const [system, list] = k.home.split('.');
      const owner = model.kinds.get(system);
      const field = owner?.fields.find((f) => f.name === list);
      if (!owner?.singleton || field?.type.base !== 'list' || field.type.of !== k.name) fail(where, `home ${k.home} is not a singleton's list of ${k.name}`);
      k.homeKind = owner;
      k.homeField = field;
    } else if (k.stored && !k.singleton && !k.key) fail(where, 'an entity kind with fields needs a home list or a key');
    for (const f of k.skip.keys()) if (k.fields.some((x) => x.name === f)) fail(where, `${f} is both a field and skipped`);
  }
  // Every name a type uses must exist.
  const check = (field) => {
    const t = field.type.base === 'map' ? field.type.value : field.type;
    if (t.base === 'enum' && !model.enums.has(t.of)) fail(field.where, `no enum ${t.of}`);
    if (t.base === 'union' && !model.unions.has(t.of)) fail(field.where, `no union ${t.of}`);
    if (t.base === 'rec' && !model.records.has(t.of)) fail(field.where, `no record ${t.of}`);
    if (['ref', 'list', 'set'].includes(t.base) && !model.kinds.has(t.of)) fail(field.where, `no kind ${t.of}`);
    if (['ref', 'list', 'set'].includes(t.base) && model.kinds.get(t.of).singleton) fail(field.where, `${t.of} is a singleton`);
  };
  for (const r of model.records.values()) r.fields.forEach(check);
  for (const k of kinds) k.fields.forEach(check);
  for (const u of model.unions.values()) for (const v of u.variants) if (v.tag === 'ref' && !model.kinds.has(v.kind)) fail(`union ${u.name}`, `no kind ${v.kind}`);
  for (const u of model.unions.values()) if (u.variants.filter((v) => v.tag === 'ref' && model.kinds.get(v.kind).fields.length).length > 1) fail(`union ${u.name}`, 'at most one of its kinds can be one the store holds fields of');
  // Calls' records exist; each section's codes are its names in order, from 1.
  for (const [section, list] of [['commands', model.commands], ['events', model.events], ['imports', model.imports]]) {
    const sorted = [...list.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    sorted.forEach((c, i) => {
      c.code = i + 1;
      for (const r of [c.args, c.result, c.record]) if (r && !model.records.has(r)) fail(c.where, `no record ${r}`);
    });
    model[`${section}List`] = sorted;
  }
  // Records nest by value: no record may hold itself.
  const visiting = new Set();
  const walk = (name) => {
    if (visiting.has(name)) fail(`record ${name}`, 'holds itself');
    visiting.add(name);
    for (const f of model.records.get(name).fields) { const t = f.type.base === 'map' ? f.type.value : f.type; if (t.base === 'rec') walk(t.of); }
    visiting.delete(name);
  };
  for (const name of model.records.keys()) walk(name);
  // Layout hashes, over each kind's fields in order with their types and
  // flags (records spelled out): any change to a row's layout changes it.
  const spell = (fields) => fields.map((f) => `${f.name}=${f.js.join('.')}${f.mirror ? '' : '-m'}${f.import ? '' : '-i'}:${f.type.base === 'rec' ? `{${spell(model.records.get(f.type.of).fields)}}` : f.type.text}`).join(';');
  for (const k of kinds) k.hash = fnv(`${k.name}${k.singleton ? '!' : ''}${k.key ? `#${k.key}` : ''}(${spell(k.fields)})`);
  return model;
}

// ---- the sync rows' layout ----

// Words a field takes in a row.
function words(model, type) {
  const tag = optional(type) ? 1 : 0;
  switch (type.base) {
    case 'vec3': return tag + 3;
    case 'quat': return tag + 4;
    case 'f64xN': return tag + type.n;
    case 'ref': return tag + 2;
    case 'union': return tag + 3;
    case 'list': case 'set': case 'map': return tag + 2;
    case 'rec': return tag + model.records.get(type.of).fields.reduce((n, f) => n + words(model, f.type), 0);
    default: return tag + 1;
  }
}

// Words a map's entry takes in the heap: its key's string and its value.
const entryWords = (model, t) => 1 + words(model, t.value);

// A kind's row layout: 'export' (the fields JavaScript reads once Rust owns
// the kind, into its mirrors), 'import' (the fields a Rust phase reads
// while JavaScript owns it), or 'full' (every field: whole states, for
// tests and tools). A row starts with the entity's slot and generation.
function layout(model, kind, which) {
  const fields = kind.fields.filter((f) => (which === 'export' ? f.mirror : which === 'import' ? f.import : true));
  let at = 2;
  const placed = fields.map((f) => { const p = { field: f, at }; at += words(model, f.type); return p; });
  return { fields: placed, stride: at };
}

const bare = (t) => ({ ...t, nullable: { null: false, undefined: false } });
const stored = (model, kind) => model.kinds.get(kind).stored;

// ---- Rust ----

function rustType(model, t, inner = false) {
  let base;
  switch (t.base) {
    case 'f64': case 'f32': case 'i32': case 'u32': case 'u8': case 'bool': base = t.base; break;
    case 'vec3': base = 'Vec3'; break;
    case 'quat': base = 'Quat'; break;
    case 'f64xN': base = `[f64; ${t.n}]`; break;
    case 'str': base = 'StrId'; break;
    case 'enum': case 'union': case 'rec': base = t.of; break;
    case 'def': base = 'DefId'; break;
    case 'ref': base = `Ref<${model.kinds.get(t.of).rust}>`; break;
    case 'list': base = `Vec<Ref<${model.kinds.get(t.of).rust}>>`; break;
    case 'set': base = `RefSet<${model.kinds.get(t.of).rust}>`; break;
    case 'map': base = `StrMap<${rustType(model, t.value)}>`; break;
  }
  if (inner || !optional(t)) return base;
  return t.nullable.null && t.nullable.undefined ? `Nullable<${base}>` : `Option<${base}>`;
}

// Inside a match on an optional field, `v` is a reference: the value of a
// copyable type is `(*v)`, a record, list or set is used through `v`.
const deref = (t) => (['rec', 'list', 'set', 'map'].includes(t.base) ? 'v' : '(*v)');

// Rust: put value `x` (bare type) into row words from `k`.
function rustPut(model, t, x, k) {
  switch (t.base) {
    case 'f64': return `row[${k}] = ${x};`;
    case 'f32': case 'i32': case 'u32': case 'u8': return `row[${k}] = ${x} as f64;`;
    case 'bool': return `row[${k}] = if ${x} { 1.0 } else { 0.0 };`;
    case 'vec3': case 'quat': case 'f64xN': { const n = t.base === 'vec3' ? 3 : t.base === 'quat' ? 4 : t.n; return `row[${k}..${k} + ${n}].copy_from_slice(&${x});`; }
    case 'str': case 'def': return `row[${k}] = ${x}.0 as f64;`;
    case 'enum': return `row[${k}] = ${x} as u8 as f64;`;
    case 'ref': return `row[${k}] = ${x}.slot as f64; row[${k} + 1] = ${x}.gen as f64;`;
    case 'union': return `${x}.put(&mut row[${k}..${k} + 3]);`;
    case 'rec': return `${x}.put(&mut row[${k}..], heap);`;
    case 'list': return `row[${k}] = heap.len() as f64; row[${k} + 1] = ${x}.len() as f64; for r in ${x}.iter() { heap.push(r.slot as f64); heap.push(r.gen as f64); }`;
    case 'set': return `row[${k}] = heap.len() as f64; row[${k} + 1] = ${x}.items.len() as f64; for r in ${x}.items.iter() { heap.push(r.slot as f64); heap.push(r.gen as f64); }`;
    case 'map': {
      // The entries' words first (key, value), each value's lists after them.
      const w = entryWords(model, t), v = t.value;
      const value = ['rec', 'list', 'set'].includes(v.base) ? 'e' : '(*e)';
      return `{ let at = heap.len(); row[${k}] = at as f64; row[${k} + 1] = ${x}.entries.len() as f64; heap.resize(at + ${x}.entries.len() * ${w}, 0.0); for (i, (key, e)) in ${x}.entries.iter().enumerate() { let mut entry = [0.0f64; ${w}]; entry[0] = key.0 as f64; { let row = &mut entry; ${rustPut(model, v, value, 1)} } heap[at + i * ${w}..at + (i + 1) * ${w}].copy_from_slice(&entry); } }`;
    }
  }
}

function rustPutField(model, t, x, k) {
  if (!optional(t)) return rustPut(model, t, x, k);
  const inner = rustPut(model, bare(t), deref(t), `${k} + 1`);
  const clear = `row[${k} + 1..${k} + ${words(model, t)}].fill(0.0);`;
  if (t.nullable.null && t.nullable.undefined) return `match &${x} { Nullable::Value(v) => { row[${k}] = 0.0; ${inner} } Nullable::Null => { row[${k}] = 1.0; ${clear} } Nullable::Undefined => { row[${k}] = 2.0; ${clear} } }`;
  return `match &${x} { Some(v) => { row[${k}] = 0.0; ${inner} } None => { row[${k}] = ${t.nullable.null ? '1.0' : '2.0'}; ${clear} } }`;
}

// Rust: the bare type's value from row words at `k`.
function rustTake(model, t, k) {
  switch (t.base) {
    case 'f64': return `row[${k}]`;
    case 'f32': return `row[${k}] as f32`;
    case 'i32': return `word_i32(row[${k}])`;
    case 'u32': return `word_u32(row[${k}])`;
    case 'u8': return `word_u32(row[${k}]) as u8`;
    case 'bool': return `row[${k}] != 0.0`;
    case 'vec3': return `[row[${k}], row[${k} + 1], row[${k} + 2]]`;
    case 'quat': return `[row[${k}], row[${k} + 1], row[${k} + 2], row[${k} + 3]]`;
    case 'f64xN': return `{ let mut a = [0.0; ${t.n}]; a.copy_from_slice(&row[${k}..${k} + ${t.n}]); a }`;
    case 'str': return `StrId(word_u32(row[${k}]))`;
    case 'def': return `DefId(word_u32(row[${k}]))`;
    case 'enum': return `${t.of}::from_u8(word_u32(row[${k}]) as u8).unwrap_or_default()`;
    case 'ref': return `Ref::new(word_u32(row[${k}]), word_u32(row[${k} + 1]))`;
    case 'union': return `${t.of}::take(&row[${k}..${k} + 3])`;
    case 'rec': return `${t.of}::take(&row[${k}..], heap)`;
    case 'list': return `{ let (at, n) = (word_u32(row[${k}]) as usize, word_u32(row[${k} + 1]) as usize); (0..n).map(|i| Ref::new(word_u32(heap[at + 2 * i]), word_u32(heap[at + 2 * i + 1]))).collect() }`;
    case 'set': return `{ let (at, n) = (word_u32(row[${k}]) as usize, word_u32(row[${k} + 1]) as usize); RefSet { items: (0..n).map(|i| Ref::new(word_u32(heap[at + 2 * i]), word_u32(heap[at + 2 * i + 1]))).collect() } }`;
    case 'map': {
      const w = entryWords(model, t);
      return `{ let (at, n) = (word_u32(row[${k}]) as usize, word_u32(row[${k} + 1]) as usize); let mut m = StrMap::default(); for i in 0..n { let row = &heap[at + i * ${w}..at + (i + 1) * ${w}]; let value = ${rustTake(model, t.value, 1)}; m.entries.push((StrId(word_u32(row[0])), value)); } m }`;
    }
  }
}

function rustTakeField(model, t, k) {
  if (!optional(t)) return rustTake(model, t, k);
  const inner = rustTake(model, bare(t), `${k} + 1`);
  if (t.nullable.null && t.nullable.undefined) return `match word_u32(row[${k}]) { 0 => Nullable::Value(${inner}), 1 => Nullable::Null, _ => Nullable::Undefined }`;
  return `if row[${k}] == 0.0 { Some(${inner}) } else { None }`;
}

function rustStruct(model, name, fields, doc) {
  const lines = [];
  if (doc) lines.push(`/// ${doc}`);
  lines.push('#[derive(Clone, Debug, Default, PartialEq)]', `pub struct ${name} {`);
  for (const f of fields) lines.push(`    pub ${snake(f.name)}: ${rustType(model, f.type)},`);
  lines.push('}', '');
  return lines;
}

// A row's put and take over placed fields, `words` long: the row is taken
// as an array of that length once, so its constant indices need no checks.
function rustRows(model, placed, words, putName, takeName, takeSelf) {
  const lines = [];
  const view = (mut) => (words ? `        let row: &${mut ? 'mut ' : ''}[f64; ${words}] = (&${mut ? 'mut ' : ''}row[..${words}]).try_into().unwrap();` : null);
  lines.push(`    pub fn ${putName}(&self, row: &mut [f64], heap: &mut Vec<f64>) {`);
  if (placed.length) lines.push(view(true));
  for (const { field, at } of placed) lines.push(`        ${rustPutField(model, field.type, `self.${snake(field.name)}`, at)}`);
  lines.push('    }', '');
  if (takeSelf) {
    lines.push(`    pub fn ${takeName}(&mut self, row: &[f64], heap: &[f64]) {`);
    if (placed.length) lines.push(view(false));
    for (const { field, at } of placed) lines.push(`        self.${snake(field.name)} = ${rustTakeField(model, field.type, at)};`);
  } else {
    lines.push(`    pub fn ${takeName}(row: &[f64], heap: &[f64]) -> Self {`);
    if (placed.length) lines.push(view(false));
    lines.push('        Self {');
    for (const { field, at } of placed) lines.push(`            ${snake(field.name)}: ${rustTakeField(model, field.type, at)},`);
    lines.push('        }');
  }
  lines.push('    }', '');
  return lines;
}

// Two layouts with the same fields share their code.
const sameLayout = (a, b) => a.stride === b.stride && a.fields.length === b.fields.length && a.fields.every((p, i) => p.field === b.fields[i].field);

function rustRecord(model, rec) {
  let at = 0;
  const placed = rec.fields.map((f) => { const p = { field: f, at }; at += words(model, f.type); return p; });
  return [...rustStruct(model, rec.name, rec.fields), `impl ${rec.name} {`, `    pub const WORDS: usize = ${at};`, '', ...rustRows(model, placed, at, 'put', 'take', false), '}', ''];
}

function rustEnum(e) {
  const lines = ['#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]', '#[repr(u8)]', `pub enum ${e.name} {`];
  e.values.forEach((v, i) => lines.push(`    ${i === 0 ? '#[default] ' : ''}${e.variants[i]} = ${i},`));
  lines.push('}', '', `impl ${e.name} {`, `    pub const NAMES: [&'static str; ${e.values.length}] = [${e.values.map((v) => JSON.stringify(v)).join(', ')}];`, '');
  lines.push('    pub fn from_u8(v: u8) -> Option<Self> {', '        match v {');
  e.variants.forEach((v, i) => lines.push(`            ${i} => Some(${e.name}::${v}),`));
  lines.push('            _ => None,', '        }', '    }', '}', '');
  return lines;
}

function rustUnion(model, u) {
  const lines = ['#[derive(Clone, Copy, Debug, PartialEq)]', `pub enum ${u.name} {`];
  u.variants.forEach((v) => lines.push(`    ${v.tag === 'ref' ? `${v.rust}(Ref<${model.kinds.get(v.kind).rust}>)` : v.rust},`));
  const first = u.variants[0];
  lines.push('}', '', `impl Default for ${u.name} {`, `    fn default() -> Self { ${u.name}::${first.rust}${first.tag === 'ref' ? '(Ref::new(0, 0))' : ''} }`, '}', '', `impl ${u.name} {`);
  lines.push('    /// Its row words: the variant, then a reference\'s slot and generation.', '    pub fn put(&self, row: &mut [f64]) {', '        match self {');
  u.variants.forEach((v, i) => lines.push(v.tag === 'ref' ? `            ${u.name}::${v.rust}(r) => { row[0] = ${i}.0; row[1] = r.slot as f64; row[2] = r.gen as f64; }` : `            ${u.name}::${v.rust} => { row[0] = ${i}.0; row[1] = 0.0; row[2] = 0.0; }`));
  lines.push('        }', '    }', '', '    pub fn take(row: &[f64]) -> Self {', '        match word_u32(row[0]) {');
  u.variants.forEach((v, i) => lines.push(v.tag === 'ref' ? `            ${i} => ${u.name}::${v.rust}(Ref::new(word_u32(row[1]), word_u32(row[2]))),` : `            ${i} => ${u.name}::${v.rust},`));
  lines.push(`            _ => ${u.name}::default(),`, '        }', '    }', '}', '');
  return lines;
}

function rustKind(model, k) {
  const lines = rustStruct(model, k.rust, k.fields, `A ${k.name}${k.singleton ? ' (one a battle)' : ''}, from ${k.facets.map((f) => `${model.target.schema}/${f}.mjs`).join(', ')}.`);
  const ex = layout(model, k, 'export'), im = layout(model, k, 'import'), full = layout(model, k, 'full');
  lines.push(`impl ${k.rust} {`);
  lines.push(`    /// Words a row takes: slot, generation, then the fields mirrored (export),`, `    /// read by a Rust phase while JavaScript owns the kind (import), or all (full).`);
  lines.push(`    pub const EXPORT_STRIDE: usize = ${ex.stride};`, `    pub const IMPORT_STRIDE: usize = ${im.stride};`, `    pub const FULL_STRIDE: usize = ${full.stride};`, '');
  lines.push(...rustRows(model, full.fields, full.stride, 'put_full', 'take_full', true));
  // The export and import layouts, where they are not the full one.
  for (const [which, l] of [['export', ex], ['import', im]]) {
    if (sameLayout(l, full)) lines.push(`    #[inline(always)]`, `    pub fn put_${which}(&self, row: &mut [f64], heap: &mut Vec<f64>) { self.put_full(row, heap) }`, '', `    #[inline(always)]`, `    pub fn take_${which}(&mut self, row: &[f64], heap: &[f64]) { self.take_full(row, heap) }`, '');
    else lines.push(...rustRows(model, l.fields, l.stride, `put_${which}`, `take_${which}`, true));
  }
  lines.push('}', '');
  return lines;
}

function rustFile(model, file) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/${file}.mjs; do not edit.`, '#![allow(clippy::all, unused_braces, unused_parens, unused_variables)]', '', 'use super::*;', ''];
  for (const e of model.enums.values()) if (e.file === file) lines.push(...rustEnum(e));
  for (const u of model.unions.values()) if (u.file === file) lines.push(...rustUnion(model, u));
  for (const r of model.records.values()) if (r.file === file) lines.push(...rustRecord(model, r));
  for (const k of model.kindList) if (k.file === file) lines.push(...rustKind(model, k));
  return lines.join('\n').replace(/\n\n+$/, '\n');
}

function rustIndex(model) {
  const kinds = model.kindList;
  const held = kinds.filter((k) => k.stored);
  const lines = [
    `// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`,
    model.target.test ? '//! The generator\'s test schemas\' code (the crate\'s tests only).' : '//! The store\'s generated code: a module a schema file, and here the kinds\'',
    ...(model.target.test ? [] : ['//! codes, the store that holds every kind with fields, and its sync', '//! dispatch (docs/sim-store.md).']),
    '#![allow(clippy::all, unused_variables, unused_imports, dead_code)]',
    '',
    'pub use crate::store::block::Block;',
    'pub use crate::store::{word_i32, word_u32, DefId, KindCode, Nullable, Quat, Ref, RefSet, Slab, StrId, StrMap, Vec3};',
    '',
  ];
  for (const f of model.files) lines.push(`mod ${snake(f)};`, `pub use self::${snake(f)}::*;`);
  lines.push('#[cfg(feature = "replay")]', 'mod replay;', '#[cfg(feature = "replay")]', 'pub use self::replay::Replay;', '');
  for (const k of kinds) lines.push(`pub const ${kindConst(k.name)}: KindCode = ${k.code};`);
  lines.push('/// How many kinds there are (codes run 1 to KINDS).', `pub const KINDS: usize = ${kinds.length};`);
  lines.push(`pub const KIND_NAMES: [&str; ${kinds.length + 1}] = ["", ${kinds.map((k) => JSON.stringify(k.name)).join(', ')}];`);
  lines.push('/// Each kind\'s layout hash (its fields, types, flags and order), by code: the', '/// generated JavaScript has the same, and the bridge checks them.', `pub const KIND_HASHES: [u32; ${kinds.length + 1}] = [0, ${kinds.map((k) => `0x${k.hash.toString(16).padStart(8, '0')}`).join(', ')}];`);
  lines.push('/// Whether each kind\'s objects carry their own number (its slot), by code.', `pub const KIND_KEYED: [bool; ${kinds.length + 1}] = [false, ${kinds.map((k) => String(Boolean(k.key))).join(', ')}];`, '');
  for (const k of kinds.filter((x) => !x.stored && !x.singleton)) lines.push('/// A kind the store holds no fields of (only references to).', '#[derive(Clone, Copy, Debug, Default)]', `pub struct ${k.rust};`, '');
  lines.push('/// Every kind with fields: a singleton by value, the rest by slot.', '#[derive(Default)]', 'pub struct Store {');
  for (const k of held) lines.push(`    pub ${snake(k.name)}: ${k.singleton ? k.rust : `Slab<${k.rust}>`},`);
  lines.push('}', '');
  // A kind's entities in its order: its home list's, or its slots'.
  const order = (k) => (k.home ? `self.${snake(k.homeKind.name)}.${snake(k.homeField.name)}.clone()` : `self.${snake(k.name)}.iter().map(|(r, _)| r).collect::<Vec<_>>()`);
  lines.push('impl Store {');
  lines.push('    /// A kind\'s export block: a row for each entity (in its home list\'s order, or', '    /// its slots\'). `mode` 0: its mirrored fields, after the rows of the entities', '    /// retired since the last such export (handed over: the slab forgets them);', '    /// 1: every field, live entities only; 2: as 0, leaving the retired where they', '    /// are. False for a kind the store holds no fields of.', '    pub fn export(&mut self, kind: KindCode, block: &mut Block, mode: u32) -> bool {', '        let full = mode == 1;', '        match kind {');
  for (const k of held) {
    if (k.singleton) {
      lines.push(`            ${kindConst(k.name)} => {`, `                block.begin(1, if full { ${k.rust}::FULL_STRIDE } else { ${k.rust}::EXPORT_STRIDE });`, '                let (row, heap) = block.row(0);', `                if full { self.${snake(k.name)}.put_full(row, heap) } else { self.${snake(k.name)}.put_export(row, heap) }`, '            }');
    } else {
      const slab = `self.${snake(k.name)}`;
      lines.push(`            ${kindConst(k.name)} => {`, `                let list = ${order(k)};`, `                let retired = if mode == 0 { std::mem::take(&mut ${slab}.retired) } else { Vec::new() };`, `                let shown = if mode == 2 { ${slab}.retired.len() } else { retired.len() };`,
        `                block.begin(shown + list.len(), if full { ${k.rust}::FULL_STRIDE } else { ${k.rust}::EXPORT_STRIDE });`, '                block.set_retired(shown);',
        `                let old = if mode == 2 { &${slab}.retired } else { &retired };`,
        '                for (i, (r, v)) in old.iter().enumerate() {', '                    let (row, heap) = block.row(i);', '                    row[0] = r.slot as f64;', '                    row[1] = r.gen as f64;', '                    v.put_export(row, heap);', '                }',
        '                for (i, &r) in list.iter().enumerate() {', '                    let (row, heap) = block.row(shown + i);', '                    row[0] = r.slot as f64;', '                    row[1] = r.gen as f64;', `                    if let Some(v) = ${slab}.get(r) { if full { v.put_full(row, heap) } else { v.put_export(row, heap) } }`, '                }', '            }');
    }
  }
  lines.push('            _ => return false,', '        }', '        block.finish();', '        true', '    }', '');
  lines.push('    /// Retires an entity of a kind the store holds by slot (`Slab::retire`).', '    pub fn retire(&mut self, kind: KindCode, slot: u32, gen: u32) -> bool {', '        match kind {');
  for (const k of held) if (!k.singleton) lines.push(`            ${kindConst(k.name)} => self.${snake(k.name)}.retire(Ref::new(slot, gen)),`);
  lines.push('            _ => false,', '        }', '    }', '');
  lines.push('    /// Takes in an import block JavaScript wrote, its import fields or all of them', '    /// (`full`): each row\'s entity (made if its slot holds none of that generation)', '    /// gets the row\'s fields, and what the block no longer lists is removed.', '    pub fn import(&mut self, kind: KindCode, block: &Block, full: bool) -> bool {', '        match kind {');
  for (const k of held) {
    if (k.singleton) {
      lines.push(`            ${kindConst(k.name)} => {`, '                let (row, heap) = block.read(0);', `                if full { self.${snake(k.name)}.take_full(row, heap) } else { self.${snake(k.name)}.take_import(row, heap) }`, '            }');
    } else {
      lines.push(`            ${kindConst(k.name)} => {`, '                let mut listed: Vec<bool> = Vec::new();', '                for i in 0..block.rows() {', '                    let (row, heap) = block.read(i);', '                    let r = Ref::new(word_u32(row[0]), word_u32(row[1]));', `                    let v = self.${snake(k.name)}.ensure(r);`, '                    if full { v.take_full(row, heap) } else { v.take_import(row, heap) }', '                    let s = r.slot as usize;', '                    if listed.len() <= s { listed.resize(s + 1, false); }', '                    listed[s] = true;', '                }', `                self.${snake(k.name)}.retain(|r, _| listed.get(r.slot as usize).copied().unwrap_or(false));`, '            }');
    }
  }
  lines.push('            _ => return false,', '        }', '        true', '    }', '}');
  lines.push(...rustCalls(model));
  return lines.join('\n') + '\n';
}

// Commands' dispatch, events' emitters and imports' calls.
function rustCalls(model) {
  const lines = [''];
  const consts = (prefix, list) => {
    for (const c of list) lines.push(`pub const ${prefix}_${upper(c.name)}: u32 = ${c.code};`);
    lines.push(`pub const ${prefix}_NAMES: [&str; ${list.length + 1}] = ["", ${list.map((c) => JSON.stringify(c.name)).join(', ')}];`, '');
  };
  consts('COMMAND', model.commandsList);
  consts('EVENT', model.eventsList);
  consts('IMPORT', model.importsList);
  lines.push('/// Runs the command `code` with the arguments JavaScript wrote in `sim.calls`,', '/// its result written there after it; false for no such command.', 'pub fn run_command(sim: &mut crate::abi::Sim, code: u32) -> bool {', '    match code {');
  for (const c of model.commandsList) {
    lines.push(`        COMMAND_${upper(c.name)} => {`, `            let args = { let (row, heap) = sim.calls.args(); ${c.args}::take(row, heap) };`);
    if (c.result) lines.push(`            let result = crate::${c.rust}(sim, args);`, `            sim.calls.result(${c.result}::WORDS, |row, heap| result.put(row, heap));`);
    else lines.push(`            crate::${c.rust}(sim, args);`, '            sim.calls.result(0, |_, _| {});');
    lines.push('        }');
  }
  lines.push('        _ => return false,', '    }', '    true', '}', '');
  for (const e of model.eventsList) {
    lines.push(`impl ${e.record} {`, `    /// Emits it as the event ${e.name} (drained in order at the end of the phase).`, '    pub fn emit(&self, events: &mut crate::store::calls::Events) {', `        events.emit(EVENT_${upper(e.name)}, ${e.record}::WORDS, |row, heap| self.put(row, heap));`, '    }', '}', '');
  }
  if (model.importsList.length) {
    lines.push('#[cfg(target_arch = "wasm32")]', '#[link(wasm_import_module = "bfe")]', 'extern "C" {');
    for (const c of model.importsList) lines.push(`    fn bfe_import_${snake(c.name).replace(/^r#/, '')}();`);
    lines.push('}', '');
    for (const c of model.importsList) {
      const fn = snake(c.name).replace(/^r#/, '');
      lines.push(`/// Asks JavaScript (the import ${c.name}): its arguments go out in the call block,`, '/// its result comes back there. The module only (a native build serves its', '/// boundary from elsewhere: the replay harness, or Rust).', '#[cfg(target_arch = "wasm32")]', `pub fn import_${fn}(calls: &mut crate::store::calls::Calls, args: &${c.args}) -> ${c.result ?? '()'} {`, `    calls.result(${c.args}::WORDS, |row, heap| args.put(row, heap));`, `    unsafe { bfe_import_${fn}() };`);
      if (c.result) lines.push('    let (row, heap) = calls.args();', `    ${c.result}::take(row, heap)`);
      lines.push('}', '');
    }
  }
  return lines;
}

// ---- Rust: the replay harness's values (the `replay` feature) ----

// Reading the bare type from value `x` (a V) at path `p` (a &str).
function replayIn(model, t, x, p) {
  const k = ['ref', 'list', 'set'].includes(t.base) ? model.kinds.get(t.of) : null;
  const one = (e, path) => (k.stored ? `${k.rust}::ref_in(${e}, cx, ${path})?` : `cx.ref_in(${kindConst(k.name)}, ${e}, ${path})?`);
  switch (t.base) {
    case 'f64': return `num(${x}, ${p})?`;
    case 'f32': return `num_f32(${x}, ${p})?`;
    case 'i32': return `(int(${x}, ${p}, i32::MIN as f64, i32::MAX as f64)? as i32)`;
    case 'u32': return `(int(${x}, ${p}, 0.0, u32::MAX as f64)? as u32)`;
    case 'u8': return `(int(${x}, ${p}, 0.0, 255.0)? as u8)`;
    case 'bool': return `boolean(${x}, ${p})?`;
    case 'vec3': return `vec3(${x}, ${p})?`;
    case 'quat': return `quat(${x}, ${p})?`;
    case 'f64xN': return `numbers::<${t.n}>(${x}, ${p})?`;
    case 'str': return `cx.str_in(${x}, ${p})?`;
    case 'enum': return `${t.of}::from_u8(name_of(${x}, &${t.of}::NAMES, ${p})?).unwrap()`;
    case 'def': return `cx.def_in(${JSON.stringify(t.of)}, ${x}, ${p})?`;
    case 'ref': return one(x, p);
    case 'union': case 'rec': return `${t.of}::from_value(${x}, cx, ${p})?`;
    case 'list': return `{ let mut out = Vec::new(); for (i, e) in list(${x}, ${p})?.into_iter().enumerate() { let at = format!("{}[{i}]", ${p}); out.push(${one('e', '&at')}); } out }`;
    case 'set': return `{ let mut out = RefSet::default(); for (i, e) in set(${x}, ${p})?.into_iter().enumerate() { let at = format!("{}[{i}]", ${p}); out.items.push(${one('e', '&at')}); } out }`;
    case 'map': return `{ let mut out = StrMap::default(); for (key, e) in map(${x}, ${p})? { let at = format!("{}<{}>", ${p}, key.show()); let id = cx.str_in(key, &at)?; let value = ${replayIn(model, t.value, 'e', '&at')}; out.entries.push((id, value)); } out }`;
  }
}

function replayInField(model, t, x, p) {
  const inner = replayIn(model, bare(t), x, p);
  if (!optional(t)) return inner;
  const bad = (what) => `return Err(format!("{}: ${what}", ${p}))`;
  if (t.nullable.null && t.nullable.undefined) return `match ${x}.value { Value::Null => Nullable::Null, Value::Undefined => Nullable::Undefined, _ => Nullable::Value(${inner}) }`;
  return t.nullable.null
    ? `match ${x}.value { Value::Null => None, Value::Undefined => ${bad('undefined where its schema says null')}, _ => Some(${inner}) }`
    : `match ${x}.value { Value::Undefined => None, Value::Null => ${bad('null where its schema says undefined')}, _ => Some(${inner}) }`;
}

// The value of the bare type of Rust expression `s`.
function replayOut(model, t, s) {
  const k = ['ref', 'list', 'set'].includes(t.base) ? model.kinds.get(t.of) : null;
  const one = (r) => (k.stored ? `${k.rust}::ref_out(${r}, cx, a)` : `cx.ref_out(${kindConst(k.name)}, ${r})`);
  switch (t.base) {
    case 'f64': return `Value::Num(${s})`;
    case 'f32': case 'i32': case 'u32': case 'u8': return `Value::Num(${s} as f64)`;
    case 'bool': return `Value::Bool(${s})`;
    case 'vec3': return `a.vec3(${s})`;
    case 'quat': return `a.quat(${s})`;
    case 'f64xN': return `{ let items = ${s}.iter().map(|&x| Value::Num(x)).collect(); a.array(items) }`;
    case 'str': return `cx.str_out(${s})`;
    case 'enum': return `Value::Str(${t.of}::NAMES[${s} as usize].to_string())`;
    case 'def': return `cx.def_out(${JSON.stringify(t.of)}, ${s})`;
    case 'ref': return one(s);
    case 'union': case 'rec': return `${s}.to_value(cx, a)`;
    case 'list': return `{ let mut items = Vec::new(); for &r in ${s}.iter() { items.push(${one('r')}); } a.array(items) }`;
    case 'set': return `{ let mut items = Vec::new(); for &r in ${s}.items.iter() { items.push(${one('r')}); } a.set(items) }`;
    case 'map': {
      const v = t.value, value = ['rec', 'list', 'set'].includes(v.base) ? 'e' : '(*e)';
      return `{ let mut entries = Vec::new(); for (key, e) in ${s}.entries.iter() { let k = cx.str_out(*key); let x = ${replayOut(model, v, value)}; entries.push((k, x)); } a.map(entries) }`;
    }
  }
}

function replayOutField(model, t, s) {
  if (!optional(t)) return replayOut(model, t, s);
  const inner = replayOut(model, bare(t), deref(t));
  if (t.nullable.null && t.nullable.undefined) return `match &${s} { Nullable::Value(v) => ${inner}, Nullable::Null => Value::Null, Nullable::Undefined => Value::Undefined }`;
  return `match &${s} { Some(v) => ${inner}, None => Value::${t.nullable.null ? 'Null' : 'Undefined'} }`;
}

function replayStruct(model, name, fields, what) {
  const lines = [`impl ${name} {`];
  lines.push('    /// Its fields from a value (the export of the JavaScript object).', `    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {`, '        object(v, path)?;', `        Ok(${name} {`);
  for (const f of fields) lines.push(`            ${snake(f.name)}: { let p = format!("{path}.${f.name}"); ${replayInField(model, f.type, `v.get(${JSON.stringify(f.name)})`, '&p')} },`);
  lines.push('        })', '    }', '');
  lines.push('    /// Its value, as the export of the JavaScript object it stands for.', `    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {`, '        let fields = vec![');
  for (const f of fields) lines.push(`            (${JSON.stringify(f.name)}, ${replayOutField(model, f.type, `self.${snake(f.name)}`)}),`);
  lines.push('        ];', `        a.object("", fields)`, '    }');
  if (what) {
    const k = what;
    const slab = `cx.store.${snake(k.name)}`;
    lines.push('');
    lines.push(`    /// The ${k.name} an object met in a value is: made in the store the first time.`, `    pub fn ref_in(v: V, cx: &mut Replay, path: &str) -> Result<Ref<${name}>> {`);
    lines.push(`        let (slot, gen) = cx.entity_in(${kindConst(k.name)}, v, path, |cx| {`);
    if (k.key) {
      lines.push(`            let slot = int(v.get(${JSON.stringify(k.key)}), &format!("{path}.${k.key}"), 0.0, u32::MAX as f64)? as u32;`, `            if let Some((old, _)) = ${slab}.at(slot) { ${slab}.remove(old); }`, `            let r = ${slab}.insert_at(slot, 0, ${name}::default());`);
    } else lines.push(`            let r = ${slab}.insert(${name}::default());`);
    lines.push(`            cx.entity_made(${kindConst(k.name)}, v, r.slot, r.gen);`, `            let value = ${name}::from_value(v, cx, path)?;`, `            *${slab}.get_mut(r).unwrap() = value;`, '            Ok((r.slot, r.gen))', '        })?;', '        Ok(Ref::new(slot, gen))', '    }', '');
    lines.push(`    /// The value of the ${k.name} a reference names (the same value wherever it is met`, '    /// again, its node made before its fields so a field can name it).', `    pub fn ref_out(r: Ref<${name}>, cx: &mut Replay, a: &mut Arena) -> Value {`, `        if let Some(v) = cx.entity_out(${kindConst(k.name)}, r.slot, r.gen) { return v; }`, '        let node = a.nodes.len();', '        a.nodes.push(Node::Array(Vec::new()));', '        let v = Value::Node(node as u32);', `        cx.entity_written(${kindConst(k.name)}, r.slot, r.gen, &v);`, `        let e = ${slab}.get(r).cloned().unwrap_or_default();`, '        if let Value::Node(made) = e.to_value(cx, a) { a.nodes.swap(node, made as usize); }', '        v', '    }');
  }
  lines.push('}', '');
  return lines;
}

function replayUnion(model, u) {
  const lines = [`impl ${u.name} {`, `    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {`, '        match v.value {'];
  const nul = u.variants.find((x) => x.tag === 'null'), und = u.variants.find((x) => x.tag === 'undefined');
  if (nul) lines.push(`            Value::Null => return Ok(${u.name}::${nul.rust}),`);
  if (und) lines.push(`            Value::Undefined => return Ok(${u.name}::${und.rust}),`);
  const lits = u.variants.filter((x) => x.tag === 'lit');
  if (lits.length) lines.push(`            Value::Str(s) => match s.as_str() { ${lits.map((x) => `${JSON.stringify(x.value)} => return Ok(${u.name}::${x.rust}),`).join(' ')} _ => {} },`);
  const handles = u.variants.filter((x) => x.tag === 'ref' && !model.kinds.get(x.kind).stored);
  if (handles.length) lines.push(`            Value::Handle(kind, _) => match kind.as_str() { ${handles.map((x) => `${JSON.stringify(x.kind)} => return Ok(${u.name}::${x.rust}(cx.ref_in(${kindConst(x.kind)}, v, path)?)),`).join(' ')} _ => {} },`);
  const entity = u.variants.find((x) => x.tag === 'ref' && model.kinds.get(x.kind).stored);
  if (entity) lines.push(`            Value::Node(_) => return Ok(${u.name}::${entity.rust}(${model.kinds.get(entity.kind).rust}::ref_in(v, cx, path)?)),`);
  lines.push('            _ => {}', '        }', `        Err(format!("{path}: none of ${u.name}'s variants ({})", v.show()))`, '    }', '');
  lines.push(`    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {`, '        match self {');
  for (const x of u.variants) {
    if (x.tag === 'null') lines.push(`            ${u.name}::${x.rust} => Value::Null,`);
    else if (x.tag === 'undefined') lines.push(`            ${u.name}::${x.rust} => Value::Undefined,`);
    else if (x.tag === 'lit') lines.push(`            ${u.name}::${x.rust} => Value::Str(${JSON.stringify(x.value)}.to_string()),`);
    else {
      const k = model.kinds.get(x.kind);
      lines.push(`            ${u.name}::${x.rust}(r) => ${k.stored ? `${k.rust}::ref_out(*r, cx, a)` : `cx.ref_out(${kindConst(k.name)}, *r)`},`);
    }
  }
  lines.push('        }', '    }', '}', '');
  return lines;
}

function rustReplay(model) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`, '//! Every kind, record and union read from and written as the replay harness\'s', '//! values (the `replay` feature; rust/sim/src/store/replay.rs).', '#![allow(clippy::all, unused_braces, unused_parens, unused_variables, unused_mut, unused_imports)]', '', 'use super::*;', 'use crate::store::replay::{boolean, int, list, map, name_of, num, num_f32, numbers, object, quat, set, vec3, Arena, Node, ReplayOf, Result, StoreKinds, Value, V};', '',
     '/// The replay context of this store.', 'pub type Replay = ReplayOf<Store>;', '', 'impl StoreKinds for Store {', '    const NAMES: &\'static [&\'static str] = &KIND_NAMES;', '    const KEYED: &\'static [bool] = &KIND_KEYED;', '}', ''];
  for (const u of model.unions.values()) lines.push(...replayUnion(model, u));
  for (const r of model.records.values()) lines.push(...replayStruct(model, r.name, r.fields, null));
  for (const k of model.kindList) if (k.stored) lines.push(...replayStruct(model, k.rust, k.fields, k.singleton ? null : k));
  lines.push('impl Replay {', '    /// Reads a singleton\'s state value (`exportState(name, object)`) into the store:', '    /// the singleton\'s fields, and every entity its lists hold, made anew.', '    pub fn read_value(&mut self, kind: KindCode, v: V) -> Result<()> {', '        match kind {');
  for (const k of model.kindList) if (k.singleton) lines.push(`            ${kindConst(k.name)} => { let value = ${k.rust}::from_value(v, self, ${JSON.stringify(k.name)})?; self.store.${snake(k.name)} = value; }`);
  lines.push('            _ => return Err(format!("{} is no singleton", KIND_NAMES.get(kind as usize).copied().unwrap_or("?"))),', '        }', '        Ok(())', '    }', '');
  lines.push('    /// A singleton\'s state as its exporter gives it.', '    pub fn write_value(&mut self, kind: KindCode, a: &mut Arena) -> Value {', '        match kind {');
  for (const k of model.kindList) if (k.singleton) lines.push(`            ${kindConst(k.name)} => { let s = self.store.${snake(k.name)}.clone(); s.to_value(self, a) }`);
  lines.push('            _ => Value::Undefined,', '        }', '    }', '}');
  return lines.join('\n') + '\n';
}

// ---- JavaScript ----

const jsEnum = (e) => `${e.name}_NAMES`;
const jsEnumIndex = (e) => `${e.name}_INDEX`;

// The expression for field `f` of object `o` (a nested path read through
// `?.`, as a mirror holds no copy of the object it is read from), and
// `(o.a ??= {}).b` for a nested path when it is written.
const jsGet = (f, o = 'o') => (f.js.length > 1 ? `${o}${jsKey(f.js[0])}${f.js.slice(1).map((k) => `?.${k}`).join('')}` : `${o}${jsPath(f.js)}`);
const jsSet = (f, o = 'm') => (f.js.length > 1 ? `(${o}${jsPath(f.js.slice(0, -1))} ??= {})${jsKey(f.js.at(-1))}` : `${o}${jsKey(f.js[0])}`);

// Block (export direction): read the bare type from words at `k` into
// `target`, updating a Vector3, Quaternion, array, record, list or set it
// holds in place (aliases of a mirror's Vector3 keep seeing it), making one
// when it holds none.
function jsSync(model, t, target, k) {
  switch (t.base) {
    case 'f64': case 'f32': case 'i32': case 'u32': case 'u8': return `${target} = f[at + ${k}];`;
    case 'bool': return `${target} = f[at + ${k}] !== 0;`;
    case 'vec3': return `(${target} ??= new THREE.Vector3()).set(f[at + ${k}], f[at + ${k + 1}], f[at + ${k + 2}]);`;
    case 'quat': return `(${target} ??= new THREE.Quaternion()).set(f[at + ${k}], f[at + ${k + 1}], f[at + ${k + 2}], f[at + ${k + 3}]);`;
    case 'f64xN': return `{ const a = (${target} ??= new Array(${t.n})); ${Array.from({ length: t.n }, (_, i) => `a[${i}] = f[at + ${k + i}];`).join(' ')} }`;
    case 'str': return `${target} = b.strings.at(f[at + ${k}]);`;
    case 'enum': return `${target} = ${jsEnum(model.enums.get(t.of))}[f[at + ${k}]];`;
    case 'def': return `${target} = b.defs.${t.of}.at(f[at + ${k}]);`;
    case 'ref': return `${target} = b.refs[${kindConst(t.of)}].at(f[at + ${k}], f[at + ${k + 1}]);`;
    case 'union': return `${target} = sync${t.of}(b, f, at + ${k});`;
    case 'rec': return `${target} = sync${t.of}(b, f, at + ${k}, h, ${target} ?? new ${t.of}Mirror());`;
    case 'list': return `${target} = b.list(${kindConst(t.of)}, f, h + f[at + ${k}], f[at + ${k + 1}], ${target} ?? []);`;
    case 'set': return `${target} = b.set(${kindConst(t.of)}, f, h + f[at + ${k}], f[at + ${k + 1}], ${target} ?? new Set());`;
    case 'map': {
      // Each entry: its key's string, then its value's words (an inner block
      // whose `at` is the entry's).
      const w = entryWords(model, t);
      return `{ const map = (${target} ??= new Map()), start = h + f[at + ${k}], n = f[at + ${k + 1}]; map.clear(); for (let i = 0; i < n; i++) { const e = start + i * ${w}; let v; { const at = e; ${jsSync(model, t.value, 'v', 1)} } map.set(b.strings.at(f[e]), v); } }`;
    }
  }
}

function jsSyncField(model, t, target, k) {
  if (!optional(t)) return jsSync(model, t, target, k);
  return `{ const tag = f[at + ${k}]; if (tag === 0) { ${jsSync(model, bare(t), target, k + 1)} } else ${target} = tag === 1 ? null : undefined; }`;
}

// Block (import direction): write the bare type of value `v` at `k`.
function jsPut(model, t, v, k) {
  switch (t.base) {
    case 'f64': case 'f32': case 'i32': case 'u32': case 'u8': return `f[at + ${k}] = ${v};`;
    case 'bool': return `f[at + ${k}] = ${v} ? 1 : 0;`;
    case 'vec3': return `{ const v = ${v}; f[at + ${k}] = v.x; f[at + ${k + 1}] = v.y; f[at + ${k + 2}] = v.z; }`;
    case 'quat': return `{ const v = ${v}; f[at + ${k}] = v.x; f[at + ${k + 1}] = v.y; f[at + ${k + 2}] = v.z; f[at + ${k + 3}] = v.w; }`;
    case 'f64xN': return `{ const v = ${v}; ${Array.from({ length: t.n }, (_, i) => `f[at + ${k + i}] = v[${i}];`).join(' ')} }`;
    case 'str': return `f[at + ${k}] = b.strings.id(${v});`;
    case 'enum': return `f[at + ${k}] = ${jsEnumIndex(model.enums.get(t.of))}.get(${v});`;
    case 'def': return `f[at + ${k}] = b.defs.${t.of}.id(${v});`;
    case 'ref': return `b.refs[${kindConst(t.of)}].put(${v}, f, at + ${k});`;
    case 'union': return `put${t.of}(b, ${v}, f, at + ${k});`;
    case 'rec': return `put${t.of}(b, ${v}, f, at + ${k}, heap);`;
    case 'list': case 'set': return `heap.list(b.refs[${kindConst(t.of)}], ${v}, f, at + ${k});`;
    case 'map': return `heap.map(b.strings, ${v}, f, at + ${k}, ${entryWords(model, t)}, (f, at, v) => { ${jsPut(model, t.value, 'v', 1)} });`;
  }
}

function jsPutField(model, t, v, k) {
  if (!optional(t)) return jsPut(model, t, v, k);
  const n = words(model, t);
  return `{ const x = ${v}; if (x === null || x === undefined) { f[at + ${k}] = x === null ? 1 : 2; f.fill(0, at + ${k + 1}, at + ${k + n}); } else { f[at + ${k}] = 0; ${jsPut(model, bare(t), 'x', k + 1)} } }`;
}

// Comparing two objects (Object.is for numbers, identity for references):
// the first differing path, or null.
function jsSame(model, t, a, b, path) {
  const p = JSON.stringify(path);
  switch (t.base) {
    case 'vec3': return `if (!sameVec(${a}, ${b})) return ${p};`;
    case 'quat': return `if (!sameQuat(${a}, ${b})) return ${p};`;
    case 'f64xN': return `if (!sameArray(${a}, ${b}, ${t.n})) return ${p};`;
    case 'rec': return `{ const d = same${t.of}(${a}, ${b}); if (d) return ${p} + d; }`;
    case 'list': return `if (!sameList(${a}, ${b})) return ${p};`;
    case 'set': return `if (!sameList(${a} && [...${a}], ${b} && [...${b}])) return ${p};`;
    case 'map': return `if (!sameMap(${a}, ${b}, (x, y) => ${jsSameValue(model, t.value, 'x', 'y')})) return ${p};`;
    default: return `if (!Object.is(${a}, ${b})) return ${p};`;
  }
}

// Whether two values of a bare type are the same (an expression).
function jsSameValue(model, t, a, b) {
  switch (t.base) {
    case 'vec3': return `sameVec(${a}, ${b})`;
    case 'quat': return `sameQuat(${a}, ${b})`;
    case 'f64xN': return `sameArray(${a}, ${b}, ${t.n})`;
    case 'rec': return `same${t.of}(${a}, ${b}) === null`;
    case 'list': return `sameList(${a}, ${b})`;
    case 'set': return `sameList(${a} && [...${a}], ${b} && [...${b}])`;
    default: return `Object.is(${a}, ${b})`;
  }
}

function jsSameField(model, t, a, b, path) {
  if (!optional(t)) return jsSame(model, t, a, b, path);
  return `{ const p = ${a}, q = ${b}; if (p === null || p === undefined || q === null || q === undefined) { if (p !== q) return ${JSON.stringify(path)}; } else { ${jsSame(model, bare(t), 'p', 'q', path)} } }`;
}

// The replay harness's value of a field (tools/sim-capture), made through
// the export context `cx` (src/sim/runtime.js ExportContext): a record and
// an entity of a kind the store holds are exported where they are met (one
// value an entity, through cx.seen); a reference to anything else is what
// cx.ref(kind, object) makes of it (the harness's handle), a definition what
// cx.def(table, def) makes of it; numbers, strings, enums, arrays of numbers
// and three.js values are the values themselves.
function jsExport(model, t, v) {
  const kindOf = (name) => model.kinds.get(name);
  const one = (name, x) => (kindOf(name).stored ? `export${kindOf(name).rust}(${x}, cx)` : `cx.ref(${JSON.stringify(name)}, ${x})`);
  switch (t.base) {
    case 'rec': return `export${t.of}(${v}, cx)`;
    case 'union': return `export${t.of}(${v}, cx)`;
    case 'def': return `cx.def(${JSON.stringify(t.of)}, ${v})`;
    case 'ref': return one(t.of, v);
    case 'list': return `${v}.map((x) => ${one(t.of, 'x')})`;
    case 'set': return `new Set([...${v}].map((x) => ${one(t.of, 'x')}))`;
    case 'map': {
      const inner = jsExport(model, t.value, 'x');
      return inner === 'x' ? `new Map(${v})` : `new Map([...${v}].map(([k, x]) => [k, ${inner}]))`;
    }
    default: return v;
  }
}

function jsExportField(model, t, v) {
  const inner = jsExport(model, bare(t), 'x');
  if (inner === 'x') return v;
  if (!optional(t)) return jsExport(model, t, v);
  return `((x) => (x === null || x === undefined ? x : ${inner}))(${v})`;
}

function jsMirrorInit(model, t) {
  if (optional(t)) return t.nullable.null ? 'null' : 'undefined';
  switch (t.base) {
    case 'f64': case 'f32': return 'NaN';
    case 'i32': case 'u32': case 'u8': return '0';
    case 'bool': return 'false';
    case 'vec3': return 'new THREE.Vector3()';
    case 'quat': return 'new THREE.Quaternion()';
    case 'f64xN': return `[${Array(t.n).fill('NaN').join(', ')}]`;
    case 'str': return "''";
    case 'enum': return JSON.stringify(model.enums.get(t.of).values[0]);
    case 'rec': return `new ${t.of}Mirror()`;
    case 'list': return '[]';
    case 'set': return 'new Set()';
    case 'map': return 'new Map()';
    default: return 'null';
  }
}

function jsMirror(model, name, fields, slots) {
  const lines = [`export class ${name} {`, '  constructor() {'];
  for (const f of fields) lines.push(`    this${jsKey(f.name)} = ${jsMirrorInit(model, f.type)};`);
  if (slots) lines.push('    this.simSlot = -1;', '    this.simGen = -1;');
  lines.push('  }', '}', '');
  return lines;
}

function jsSameFn(model, name, fields) {
  const lines = [`export function same${name}(a, b) {`];
  for (const f of fields) lines.push(`  ${jsSameField(model, f.type, jsGet(f, 'a'), jsGet(f, 'b'), `.${f.name}`)}`);
  lines.push('  return null;', '}', '');
  return lines;
}

function jsExportFn(model, name, fields) {
  const lines = [`export function export${name}(o, cx) {`, '  let v = cx.seen.get(o);', '  if (v) return v;', '  v = {};', '  cx.seen.set(o, v);'];
  for (const f of fields) lines.push(`  v${jsKey(f.name)} = ${jsExportField(model, f.type, jsGet(f))};`);
  lines.push('  return v;', '}', '');
  return lines;
}

function jsSyncFn(model, name, placed, target = 'm') {
  const lines = [`export function ${name}(b, f, at, h, m) {`];
  for (const { field, at } of placed) lines.push(`  ${jsSyncField(model, field.type, jsSet(field, target), at)}`);
  lines.push('  return m;', '}', '');
  return lines;
}

function jsPutFn(model, name, placed) {
  const lines = [`export function ${name}(b, o, f, at, heap) {`];
  for (const { field, at } of placed) lines.push(`  ${jsPutField(model, field.type, jsGet(field), at)}`);
  lines.push('}', '');
  return lines;
}

function jsRecord(model, rec) {
  let at = 0;
  const placed = rec.fields.map((f) => { const p = { field: f, at }; at += words(model, f.type); return p; });
  return [
    ...jsMirror(model, `${rec.name}Mirror`, rec.fields, false),
    ...jsSyncFn(model, `sync${rec.name}`, placed),
    ...jsPutFn(model, `put${rec.name}`, placed),
    ...jsSameFn(model, rec.name, rec.fields),
    ...jsExportFn(model, rec.name, rec.fields),
  ];
}

function jsUnion(model, u) {
  const lines = [];
  const refs = u.variants.map((v, i) => [v, i]).filter(([v]) => v.tag === 'ref');
  const tests = [];
  u.variants.forEach((v, i) => {
    if (v.tag === 'null') tests.push(`if (x === null) return ${i};`);
    else if (v.tag === 'undefined') tests.push(`if (x === undefined) return ${i};`);
    else if (v.tag === 'lit') tests.push(`if (x === ${JSON.stringify(v.value)}) return ${i};`);
  });
  // The kinds the store only refers to by their tests; the one kind it
  // holds (a union has at most one) is any other object.
  const held = refs.find(([v]) => model.kinds.get(v.kind).stored);
  for (const [v, i] of refs) if (!model.kinds.get(v.kind).stored) tests.push(`if (b.refs[${kindConst(v.kind)}].is(x)) return ${i};`);
  if (held) tests.push(`if (x !== null && typeof x === "object") return ${held[1]};`);
  lines.push(`// Which of ${u.name}'s variants a value is.`, `export function which${u.name}(b, x) {`, ...tests.map((t) => `  ${t}`), `  throw Error(\`${u.name}: \${x?.constructor?.name ?? x} is none of its variants\`);`, '}', '');
  lines.push(`export function sync${u.name}(b, f, at) {`, '  switch (f[at]) {');
  u.variants.forEach((v, i) => lines.push(`    case ${i}: return ${v.tag === 'null' ? 'null' : v.tag === 'undefined' ? 'undefined' : v.tag === 'lit' ? JSON.stringify(v.value) : `b.refs[${kindConst(v.kind)}].at(f[at + 1], f[at + 2])`};`));
  lines.push(`    default: throw Error('${u.name}: no such variant');`, '  }', '}', '');
  lines.push(`export function export${u.name}(x, cx) {`, `  switch (which${u.name}(cx, x)) {`);
  u.variants.forEach((v, i) => {
    const k = v.tag === 'ref' ? model.kinds.get(v.kind) : null;
    lines.push(`    case ${i}: return ${v.tag === 'null' ? 'null' : v.tag === 'undefined' ? 'undefined' : v.tag === 'lit' ? JSON.stringify(v.value) : k.stored ? `export${k.rust}(x, cx)` : `cx.ref(${JSON.stringify(v.kind)}, x)`};`);
  });
  lines.push('  }', '}', '');
  lines.push(`export function put${u.name}(b, x, f, at) {`, `  const i = which${u.name}(b, x);`, '  f[at] = i;');
  if (refs.length) lines.push(`  switch (i) { ${refs.map(([v, i]) => `case ${i}: b.refs[${kindConst(v.kind)}].put(x, f, at + 1); return;`).join(' ')} }`);
  lines.push('  f[at + 1] = 0;', '  f[at + 2] = 0;', '}', '');
  return lines;
}

function jsKind(model, k) {
  const ex = layout(model, k, 'export'), im = layout(model, k, 'import'), full = layout(model, k, 'full');
  const U = upper(k.name);
  const lines = [`// ${k.name}: from ${k.facets.map((f) => `${model.target.schema}/${f}.mjs`).join(', ')}.`];
  lines.push(...jsMirror(model, `${k.rust}Mirror`, k.fields.filter((f) => f.mirror), !k.singleton));
  // Every own key a JavaScript object of the kind may have: its fields and
  // what is skipped (the completeness check: a field nobody declared is
  // state a replay would miss).
  lines.push(`export const ${U}_KEYS = new Set([${[...new Set([...k.fields.map((f) => f.js[0]), ...k.skip.keys()])].map((n) => JSON.stringify(n)).join(', ')}]);`);
  lines.push(`export const ${U}_EXPORT_STRIDE = ${ex.stride}, ${U}_IMPORT_STRIDE = ${im.stride}, ${U}_FULL_STRIDE = ${full.stride};`, '');
  lines.push('// An export row (from `at`; the heap from `h`) into mirror `m`.', ...jsSyncFn(model, `sync${k.rust}`, ex.fields));
  lines.push('// A full row into object `m`.', ...jsSyncFn(model, `syncFull${k.rust}`, full.fields));
  lines.push('// Object `o`\'s import fields into a row (lists to `heap`).', ...jsPutFn(model, `put${k.rust}`, im.fields));
  lines.push('// Every field of object `o` into a row.', ...jsPutFn(model, `putFull${k.rust}`, full.fields));
  // The mirror write detector.
  lines.push('// The first mirrored field of `m` that is not what the export row says (a', '// write JavaScript made to Rust\'s state without a command), or null.', `export function check${k.rust}(b, f, at, h, m) {`, `  const want = sync${k.rust}(b, f, at, h, new ${k.rust}Mirror());`);
  for (const { field } of ex.fields) lines.push(`  ${jsSameField(model, field.type, `m${jsKey(field.name)}`, `want${jsKey(field.name)}`, `.${field.name}`)}`);
  lines.push('  return null;', '}', '');
  lines.push(...jsSameFn(model, k.rust, k.fields));
  lines.push(...jsExportFn(model, k.rust, k.fields));
  return lines;
}

function jsFile(model, file) {
  // Names this file uses from other files, and the kind codes it uses.
  const uses = new Map();
  const use = (name, from) => { if (from !== file) { if (!uses.has(from)) uses.set(from, new Set()); uses.get(from).add(name); } };
  const codes = new Set();
  const need = (t0) => {
    const t = t0.base === 'map' ? t0.value : t0;
    if (t.base === 'enum') { const e = model.enums.get(t.of); use(jsEnum(e), e.file); use(jsEnumIndex(e), e.file); }
    if (t.base === 'union') { const u = model.unions.get(t.of); for (const p of ['sync', 'put', 'export']) use(`${p}${u.name}`, u.file); }
    if (t.base === 'rec') { const r = model.records.get(t.of); for (const p of ['sync', 'put', 'same', 'export']) use(`${p}${r.name}`, r.file); use(`${r.name}Mirror`, r.file); }
    if (['ref', 'list', 'set'].includes(t.base)) {
      codes.add(kindConst(t.of));
      const k = model.kinds.get(t.of);
      if (k.stored) use(`export${k.rust}`, k.file);
    }
  };
  for (const r of model.records.values()) if (r.file === file) r.fields.forEach((f) => need(f.type));
  for (const k of model.kindList) if (k.file === file) k.fields.forEach((f) => need(f.type));
  for (const u of model.unions.values()) if (u.file === file) for (const v of u.variants) if (v.tag === 'ref') {
    codes.add(kindConst(v.kind));
    const k = model.kinds.get(v.kind);
    if (k.stored) use(`export${k.rust}`, k.file);
  }
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/${file}.mjs; do not edit.`, 'import * as THREE from "three";', `import { sameArray, sameList, sameMap, sameQuat, sameVec } from "${model.target.compare}";`];
  if (codes.size) lines.push(`import { ${[...codes].sort().join(', ')} } from "./kinds.js";`);
  for (const [from, names] of [...uses].sort()) lines.push(`import { ${[...names].sort().join(', ')} } from "./${from}.js";`);
  lines.push('');
  for (const e of model.enums.values()) {
    if (e.file !== file) continue;
    lines.push(`export const ${jsEnum(e)} = ${JSON.stringify(e.values)};`, `export const ${jsEnumIndex(e)} = new Map(${jsEnum(e)}.map((name, i) => [name, i]));`, '');
  }
  for (const u of model.unions.values()) if (u.file === file) lines.push(...jsUnion(model, u));
  for (const r of model.records.values()) if (r.file === file) lines.push(...jsRecord(model, r));
  for (const k of model.kindList) if (k.file === file) lines.push(...jsKind(model, k));
  return lines.join('\n').replace(/\n\n+$/, '\n');
}

function jsKinds(model) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`, '// Each kind\'s code (its place among the kinds\' names, from 1).'];
  for (const k of model.kindList) lines.push(`export const ${kindConst(k.name)} = ${k.code};`);
  lines.push(`export const KINDS = ${model.kindList.length};`);
  return lines.join('\n') + '\n';
}

// The imports' names and symbols alone, for the module's loader
// (src/sim/module.js), which needs nothing else of the generated code.
function jsImports(model) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`, '// What the module imports from JavaScript, by code: the bridge serves each.'];
  lines.push(`export const IMPORT_SYMBOLS = [null, ${model.importsList.map((c) => `{ name: ${JSON.stringify(c.name)}, code: ${c.code}, symbol: ${JSON.stringify(`bfe_import_${snake(c.name).replace(/^r#/, '')}`)} }`).join(', ')}];`);
  return lines.join('\n') + '\n';
}

function jsIndex(model) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`, '// Every kind: its code, name, layout and generated functions, for the', '// store\'s runtime (src/sim/runtime.js) to dispatch on (docs/sim-store.md).'];
  const callRecords = new Set([...model.commandsList.flatMap((c) => [c.args, c.result]), ...model.eventsList.map((e) => e.record), ...model.importsList.flatMap((c) => [c.args, c.result])].filter(Boolean));
  for (const f of model.files) {
    const names = [];
    for (const k of model.kindList) if (k.file === f) names.push(`${k.rust}Mirror`, `sync${k.rust}`, `syncFull${k.rust}`, `put${k.rust}`, `putFull${k.rust}`, `check${k.rust}`, `same${k.rust}`, `export${k.rust}`, `${upper(k.name)}_KEYS`);
    for (const r of callRecords) if (model.records.get(r).file === f) names.push(`${r}Mirror`, `sync${r}`, `put${r}`);
    if (names.length) lines.push(`import { ${names.join(', ')} } from "./${f}.js";`);
  }
  lines.push('export * from "./kinds.js";');
  for (const f of model.files) lines.push(`export * from "./${f}.js";`);
  lines.push('', '// By code: name, layout hash (the same as the module\'s), singleton, key (the', '// numeric field that names an object), home (singleton kind and list field),', '// whether the store holds fields of it, row strides, and its functions.', 'export const KIND_TABLE = [null,');
  for (const k of model.kindList) {
    const base = `name: ${JSON.stringify(k.name)}, code: ${k.code}, hash: 0x${k.hash.toString(16).padStart(8, '0')}, singleton: ${k.singleton}, key: ${JSON.stringify(k.key)}, home: ${k.home ? `{ kind: ${k.homeKind.code}, field: ${JSON.stringify(k.homeField.name)} }` : 'null'}, stored: ${k.stored}, classes: ${JSON.stringify(k.classes)}`;
    if (!k.stored) { lines.push(`  { ${base} },`); continue; }
    const s = (w) => layout(model, k, w).stride;
    lines.push(`  { ${base}, exportStride: ${s('export')}, importStride: ${s('import')}, fullStride: ${s('full')}, Mirror: ${k.rust}Mirror, sync: sync${k.rust}, syncFull: syncFull${k.rust}, put: put${k.rust}, putFull: putFull${k.rust}, check: check${k.rust}, same: same${k.rust}, export: export${k.rust}, keys: ${upper(k.name)}_KEYS },`);
  }
  lines.push('];', '');
  const recFns = (r) => ({ words: words(model, { base: 'rec', of: r, nullable: { null: false, undefined: false } }) });
  const fnsOf = (r) => `{ words: ${recFns(r).words}, Mirror: ${r}Mirror, sync: sync${r}, put: put${r} }`;
  lines.push('// Commands, events and imports by code: name, and their records\' words and functions.');
  lines.push(`export const COMMAND_TABLE = [null, ${model.commandsList.map((c) => `{ name: ${JSON.stringify(c.name)}, code: ${c.code}, args: ${fnsOf(c.args)}, result: ${c.result ? fnsOf(c.result) : 'null'} }`).join(', ')}];`);
  lines.push(`export const EVENT_TABLE = [null, ${model.eventsList.map((e) => `{ name: ${JSON.stringify(e.name)}, code: ${e.code}, record: ${fnsOf(e.record)} }`).join(', ')}];`);
  lines.push(`export const IMPORT_TABLE = [null, ${model.importsList.map((c) => `{ name: ${JSON.stringify(c.name)}, code: ${c.code}, symbol: ${JSON.stringify(`bfe_import_${snake(c.name).replace(/^r#/, '')}`)}, args: ${fnsOf(c.args)}, result: ${c.result ? fnsOf(c.result) : 'null'} }`).join(', ')}];`, '');
  lines.push(`export const DEF_TABLES = ${JSON.stringify([...new Set([...model.kindList.flatMap((k) => k.fields), ...[...model.records.values()].flatMap((r) => r.fields)].filter((f) => f.type.base === 'def').map((f) => f.type.of))].sort())};`);
  return lines.join('\n') + '\n';
}

// ---- writing ----

export async function generate() {
  const out = new Map(), models = [];
  for (const target of TARGETS) {
    const model = buildModel(await loadSchemas(join(ROOT, target.schema)));
    model.target = target;
    models.push(model);
    const rust = join(ROOT, target.rust), js = join(ROOT, target.js);
    out.set(join(rust, 'mod.rs'), rustIndex(model));
    out.set(join(rust, 'replay.rs'), rustReplay(model));
    for (const f of model.files) out.set(join(rust, `${snake(f)}.rs`), rustFile(model, f));
    out.set(join(js, 'kinds.js'), jsKinds(model));
    out.set(join(js, 'imports.js'), jsImports(model));
    out.set(join(js, 'index.js'), jsIndex(model));
    for (const f of model.files) out.set(join(js, `${f}.js`), jsFile(model, f));
  }
  return { model: models[0], models, out };
}

const listed = (dir) => (existsSync(dir) ? readdirSync(dir).map((f) => join(dir, f)) : []);

export async function main(args) {
  const check = args.includes('--check');
  const { out } = await generate();
  const problems = [];
  for (const [path, text] of out) {
    const rel = path.slice(ROOT.length);
    if (check) {
      if (!existsSync(path)) problems.push(`${rel} is missing`);
      else if (readFileSync(path, 'utf8') !== text) problems.push(`${rel} differs from what the schemas make`);
    } else {
      mkdirSync(join(path, '..'), { recursive: true });
      if (!existsSync(path) || readFileSync(path, 'utf8') !== text) writeFileSync(path, text);
    }
  }
  for (const path of TARGETS.flatMap((t) => [...listed(join(ROOT, t.rust)), ...listed(join(ROOT, t.js))])) {
    if (out.has(path)) continue;
    if (check) problems.push(`${path.slice(ROOT.length)} is not made by the schemas`);
    else rmSync(path);
  }
  if (problems.length) {
    console.error(`${problems.join('\n')}\n\nThe store's generated code does not match rust/sim/schema/. Run \`node tools/sim-schema.mjs\` and commit what it writes.`);
    process.exitCode = 1;
  } else console.log(check ? "The store's generated code matches its schemas." : `wrote ${out.size} files`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch((error) => { console.error(error.message || error); process.exitCode = 1; });
}
