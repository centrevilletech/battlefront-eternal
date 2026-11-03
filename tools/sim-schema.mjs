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

const SCALARS = new Set(['f64', 'f32', 'i32', 'u32', 'u8', 'bool', 'vec3', 'quat', 'mat4', 'str']);
const RUST_KEYWORDS = new Set(['as', 'break', 'const', 'continue', 'crate', 'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'static', 'struct', 'super', 'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while', 'async', 'await', 'dyn', 'abstract', 'become', 'box', 'do', 'final', 'macro', 'override', 'priv', 'typeof', 'unsized', 'virtual', 'yield', 'try', 'gen']);

const pascal = (s) => s.replace(/[^A-Za-z0-9]+(.)?/g, (_, c) => (c ? c.toUpperCase() : '')).replace(/^./, (c) => c.toUpperCase());
const snake = (s) => {
  const out = s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase();
  return RUST_KEYWORDS.has(out) ? `r#${out}` : out;
};
const upper = (s) => snake(s).replace(/^r#/, '').toUpperCase();
const kindConst = (kind) => `KIND_${upper(kind)}`;
const jsKey = (name) => (/^[A-Za-z_$][\w$]*$/.test(name) ? `.${name}` : /^\d+$/.test(name) ? `[${name}]` : `[${JSON.stringify(name)}]`);
// A key read through `?.` (a numeric one as `?.[0]`).
const jsOptKey = (name) => (/^[A-Za-z_$][\w$]*$/.test(name) ? `?.${name}` : `?.${jsKey(name).replace(/^\./, '')}`);
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
  if (base.startsWith('list:')) {
    // An array: of references to a kind (`list:bolt`, as `list:ref:bolt`),
    // or of any other type that is never null (records by value, numbers,
    // vectors, booleans, strings, enums, unions), its order kept.
    const rest = base.slice(5);
    type.base = 'list';
    type.value = /^[A-Za-z_]\w*$/.test(rest) && !SCALARS.has(rest) && !/^f64x\d+$/.test(rest) ? parseType(`ref:${rest}`, where) : parseType(rest, where);
    if (optional(type.value) || ['map', 'list', 'set'].includes(type.value.base)) fail(where, 'a list\'s items are a type that is never null or undefined, and no list, set or map');
    if (type.value.base === 'ref') type.of = type.value.of;
    return type;
  }
  if (base.startsWith('map:')) {
    // A Map with string keys; its values' type follows (never null).
    type.base = 'map';
    type.value = parseType(base.slice(4), where);
    if (optional(type.value) || type.value.base === 'map') fail(where, 'a map\'s values are a type that is never null or undefined, and no map');
  } else if (SCALARS.has(base)) type.base = base;
  else if ((m = /^f64x([2-9]|1[0-6])$/.exec(base))) { type.base = 'f64xN'; type.n = Number(m[1]); }
  else if ((m = /^(enum|ref|union|def|rec|set|index):([A-Za-z_][\w]*)$/.exec(base))) { type.base = m[1]; type.of = m[2]; }
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
        // A value of a type: a number, a vector, a string, an enum, a record.
        const type = parseType(v, where);
        if (optional(type) || ['list', 'set', 'map', 'union', 'ref', 'index', 'def'].includes(type.base)) fail(where, `"${v}" cannot be a union's variant`);
        return { tag: 'value', type, rust: pascal(['rec', 'enum'].includes(type.base) ? type.of : type.base === 'f64xN' ? `f64x${type.n}` : type.base) };
      });
      if (new Set(variants.map((v) => v.rust)).size !== variants.length) fail(where, 'two variants of one name');
      model.unions.set(name, { name, variants, file });
    }
    for (const [name, fields] of Object.entries(schema.records || {})) {
      const where = `${file} record ${name}`;
      if (model.records.has(name)) fail(where, 'declared twice');
      claim(name, where);
      // `$class`: the JavaScript class its objects are (a union of records
      // tells them apart by it); every other key is a field.
      const { $class = null, ...plain } = fields;
      model.records.set(name, { name, file, classes: [$class ?? []].flat(), fields: Object.entries(plain).map(([f, spec]) => parseField(f, spec, `${where}.${f}`)) });
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
  // A singleton's list that is a kind's home: where that kind's entities
  // are written out whole (everywhere else a reference to one is a handle).
  model.isHome = (field) => kinds.some((k) => k.homeField === field);
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
    const t = ['map', 'list'].includes(field.type.base) ? field.type.value : field.type;
    if (t.base === 'enum' && !model.enums.has(t.of)) fail(field.where, `no enum ${t.of}`);
    if (t.base === 'union' && !model.unions.has(t.of)) fail(field.where, `no union ${t.of}`);
    if (t.base === 'rec' && !model.records.has(t.of)) fail(field.where, `no record ${t.of}`);
    if (['ref', 'set'].includes(t.base) && !model.kinds.has(t.of)) fail(field.where, `no kind ${t.of}`);
    if (['ref', 'set'].includes(t.base) && model.kinds.get(t.of).singleton) fail(field.where, `${t.of} is a singleton`);
  };
  for (const r of model.records.values()) r.fields.forEach(check);
  for (const k of kinds) k.fields.forEach(check);
  for (const u of model.unions.values()) for (const v of u.variants) if (v.tag === 'ref' && !model.kinds.has(v.kind)) fail(`union ${u.name}`, `no kind ${v.kind}`);
  for (const u of model.unions.values()) if (u.variants.filter((v) => v.tag === 'ref' && model.kinds.get(v.kind).fields.length).length > 1) fail(`union ${u.name}`, 'at most one of its kinds can be one the store holds fields of');
  for (const u of model.unions.values()) {
    for (const v of u.variants) if (v.tag === 'value' && v.type.base === 'rec' && !model.records.has(v.type.of)) fail(`union ${u.name}`, `no record ${v.type.of}`);
    for (const v of u.variants) if (v.tag === 'value' && v.type.base === 'enum' && !model.enums.has(v.type.of)) fail(`union ${u.name}`, `no enum ${v.type.of}`);
    const recs = u.variants.filter((v) => v.tag === 'value' && v.type.base === 'rec');
    if (recs.length > 1 && recs.some((v) => !model.records.get(v.type.of).classes.length)) fail(`union ${u.name}`, 'with more than one record, each record names its $class');
    // Its row: the variant, then the largest variant's words.
    u.words = unionWords(model, u);
  }
  // An index names an earlier field of the same struct, a list of records:
  // the item it is (by identity).
  const indexes = (fields, where) => fields.forEach((f, i) => {
    if (f.type.base !== 'index') return;
    if (f.js.length > 1) fail(`${where}.${f.name}`, 'an index is a field of its own object (no path)');
    const list = fields.slice(0, i).find((g) => g.name === f.type.of);
    if (!list || list.type.base !== 'list' || list.type.value.base !== 'rec') fail(`${where}.${f.name}`, `index:${f.type.of} names no earlier list of records`);
  });
  for (const r of model.records.values()) indexes(r.fields, `record ${r.name}`);
  for (const k of kinds) indexes(k.fields, `kind ${k.name}`);
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
    for (const f of model.records.get(name).fields) { const t = ['map', 'list'].includes(f.type.base) ? f.type.value : f.type; if (t.base === 'rec') walk(t.of); }
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
    case 'mat4': return tag + 16;
    case 'f64xN': return tag + type.n;
    case 'ref': return tag + 2;
    case 'union': return tag + unionWords(model, model.unions.get(type.of));
    case 'list': case 'set': case 'map': return tag + 2;
    case 'rec': return tag + model.records.get(type.of).fields.reduce((n, f) => n + words(model, f.type), 0);
    default: return tag + 1;
  }
}

// A union's words: its variant, then the largest variant's words.
function unionWords(model, u) {
  return (u.words ??= 1 + Math.max(0, ...u.variants.map((v) => (v.tag === 'ref' ? 2 : v.tag === 'value' ? words(model, v.type) : 0))));
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
    case 'index': base = 'u32'; break;
    case 'vec3': base = 'Vec3'; break;
    case 'quat': base = 'Quat'; break;
    case 'mat4': base = 'Mat4'; break;
    case 'f64xN': base = `[f64; ${t.n}]`; break;
    case 'str': base = 'StrId'; break;
    case 'enum': case 'union': case 'rec': base = t.of; break;
    case 'def': base = 'DefId'; break;
    case 'ref': base = `Ref<${model.kinds.get(t.of).rust}>`; break;
    case 'list': base = `Vec<${rustType(model, t.value)}>`; break;
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
    case 'f32': case 'i32': case 'u32': case 'u8': case 'index': return `row[${k}] = ${x} as f64;`;
    case 'bool': return `row[${k}] = if ${x} { 1.0 } else { 0.0 };`;
    case 'vec3': case 'quat': case 'mat4': case 'f64xN': { const n = t.base === 'vec3' ? 3 : t.base === 'quat' ? 4 : t.base === 'mat4' ? 16 : t.n; return `row[${k}..${k} + ${n}].copy_from_slice(&${x});`; }
    case 'str': case 'def': return `row[${k}] = ${x}.0 as f64;`;
    case 'enum': return `row[${k}] = ${x} as u8 as f64;`;
    case 'ref': return `row[${k}] = ${x}.slot as f64; row[${k} + 1] = ${x}.gen as f64;`;
    case 'union': return `${x}.put(&mut row[${k}..${k} + ${unionWords(model, model.unions.get(t.of))}], heap);`;
    case 'rec': return `${x}.put(&mut row[${k}..], heap);`;
    case 'list': {
      // The items' words in the heap, each `w` long, the lists their records hold after them.
      const w = words(model, t.value), v = t.value;
      const item = ['rec', 'list', 'set', 'map'].includes(v.base) ? 'e' : '(*e)';
      return `{ let at = heap.len(); row[${k}] = at as f64; row[${k} + 1] = ${x}.len() as f64; heap.resize(at + ${x}.len() * ${w}, 0.0); for (i, e) in ${x}.iter().enumerate() { let mut item = [0.0f64; ${w}]; { let row = &mut item; ${rustPut(model, v, item, 0)} } heap[at + i * ${w}..at + (i + 1) * ${w}].copy_from_slice(&item); } }`;
    }
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
    case 'u32': case 'index': return `word_u32(row[${k}])`;
    case 'u8': return `word_u32(row[${k}]) as u8`;
    case 'bool': return `row[${k}] != 0.0`;
    case 'vec3': return `[row[${k}], row[${k} + 1], row[${k} + 2]]`;
    case 'quat': return `[row[${k}], row[${k} + 1], row[${k} + 2], row[${k} + 3]]`;
    case 'mat4': return `{ let mut m = [0.0; 16]; m.copy_from_slice(&row[${k}..${k} + 16]); m }`;
    case 'f64xN': return `{ let mut a = [0.0; ${t.n}]; a.copy_from_slice(&row[${k}..${k} + ${t.n}]); a }`;
    case 'str': return `StrId(word_u32(row[${k}]))`;
    case 'def': return `DefId(word_u32(row[${k}]))`;
    case 'enum': return `${t.of}::from_u8(word_u32(row[${k}]) as u8).unwrap_or_default()`;
    case 'ref': return `Ref::new(word_u32(row[${k}]), word_u32(row[${k} + 1]))`;
    case 'union': return `${t.of}::take(&row[${k}..${k} + ${unionWords(model, model.unions.get(t.of))}], heap)`;
    case 'rec': return `${t.of}::take(&row[${k}..], heap)`;
    case 'list': {
      const w = words(model, t.value);
      return `{ let (at, n) = (word_u32(row[${k}]) as usize, word_u32(row[${k} + 1]) as usize); let mut out = Vec::with_capacity(n); for i in 0..n { let row = &heap[at + i * ${w}..at + (i + 1) * ${w}]; out.push(${rustTake(model, t.value, 0)}); } out }`;
    }
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
  const copy = u.variants.every((v) => v.tag !== 'value' || !['rec', 'str'].includes(v.type.base) || v.type.base === 'str');
  const payload = (v) => (v.tag === 'ref' ? `Ref<${model.kinds.get(v.kind).rust}>` : rustType(model, v.type));
  const lines = [`#[derive(Clone, ${copy ? 'Copy, ' : ''}Debug, PartialEq)]`, `pub enum ${u.name} {`];
  u.variants.forEach((v) => lines.push(`    ${['ref', 'value'].includes(v.tag) ? `${v.rust}(${payload(v)})` : v.rust},`));
  const first = u.variants[0];
  lines.push('}', '', `impl Default for ${u.name} {`, `    fn default() -> Self { ${u.name}::${first.rust}${first.tag === 'ref' ? '(Ref::new(0, 0))' : first.tag === 'value' ? '(Default::default())' : ''} }`, '}', '', `impl ${u.name} {`);
  lines.push(`    pub const WORDS: usize = ${unionWords(model, u)};`, '');
  lines.push('    /// Its row words: the variant, then its value (a reference\'s slot and generation).', '    pub fn put(&self, row: &mut [f64], heap: &mut Vec<f64>) {', `        row[1..${unionWords(model, u)}].fill(0.0);`, '        match self {');
  u.variants.forEach((v, i) => {
    if (v.tag === 'ref') lines.push(`            ${u.name}::${v.rust}(r) => { row[0] = ${i}.0; row[1] = r.slot as f64; row[2] = r.gen as f64; }`);
    else if (v.tag === 'value') lines.push(`            ${u.name}::${v.rust}(v) => { row[0] = ${i}.0; ${rustPut(model, v.type, deref(v.type), 1)} }`);
    else lines.push(`            ${u.name}::${v.rust} => row[0] = ${i}.0,`);
  });
  lines.push('        }', '    }', '', '    pub fn take(row: &[f64], heap: &[f64]) -> Self {', '        match word_u32(row[0]) {');
  u.variants.forEach((v, i) => {
    if (v.tag === 'ref') lines.push(`            ${i} => ${u.name}::${v.rust}(Ref::new(word_u32(row[1]), word_u32(row[2]))),`);
    else if (v.tag === 'value') lines.push(`            ${i} => ${u.name}::${v.rust}(${rustTake(model, v.type, 1)}),`);
    else lines.push(`            ${i} => ${u.name}::${v.rust},`);
  });
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
    'pub use crate::store::{word_i32, word_u32, DefId, KindCode, Mat4, Nullable, Quat, Ref, RefSet, Slab, StrId, StrMap, Vec3};',
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
  lines.push('    /// `count` new entities of a kind (its fields\' defaults) at the end of its home', '    /// list, if it has one (tests and benchmarks).', '    pub fn fill(&mut self, kind: KindCode, count: usize) {', '        match kind {');
  for (const k of held) {
    if (k.singleton || k.key) continue;
    lines.push(`            ${kindConst(k.name)} => for _ in 0..count {`, `                let r = self.${snake(k.name)}.insert(Default::default());`);
    if (k.home) lines.push(`                self.${snake(k.homeKind.name)}.${snake(k.homeField.name)}.push(r);`);
    lines.push('            },');
  }
  lines.push('            _ => {}', '        }', '    }', '');
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

// Whether a record's words are all it holds (no list, set or map: no heap),
// so a command taking it can take its words as the call's parameters.
function flat(model, name) {
  return model.records.get(name).fields.every((f) => !['list', 'set', 'map'].includes(f.type.base) && (f.type.base !== 'rec' || flat(model, f.type.of)));
}
const recordWords = (model, name) => words(model, { base: 'rec', of: name, nullable: { null: false, undefined: false } });

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
  // A command whose arguments hold no list, set or map also has an export
  // of its own taking their words as its parameters: one call, nothing
  // written into memory (the bridge's command() uses it). Its result, if
  // any, is in the call block, whose start it returns.
  if (!model.target.test) for (const c of model.commandsList) {
    if (!flat(model, c.args)) continue;
    const n = recordWords(model, c.args);
    const params = Array.from({ length: n }, (_, i) => `w${i}: f64`).join(', ');
    lines.push(`/// The command ${c.name}, its arguments' words as parameters.`, '#[no_mangle]', `pub extern "C" fn cmd_${snake(c.name).replace(/^r#/, '')}(${params}) -> *const f64 {`, `    let row: [f64; ${n}] = [${Array.from({ length: n }, (_, i) => `w${i}`).join(', ')}];`, `    let args = ${c.args}::take(&row, &[]);`, '    let sim = crate::abi::sim();');
    if (c.result) lines.push(`    let result = crate::${c.rust}(sim, args);`, `    sim.calls.result(${c.result}::WORDS, |row, heap| result.put(row, heap));`, '    sim.calls.block.ptr()');
    else lines.push(`    crate::${c.rust}(sim, args);`, '    std::ptr::null()');
    lines.push('}', '');
  }
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
  const k = ['ref', 'set'].includes(t.base) ? model.kinds.get(t.of) : null;
  const one = (e, path) => (k.stored ? `${k.rust}::ref_in(${e}, cx, ${path})?` : `cx.ref_in(${kindConst(k.name)}, ${e}, ${path})?`);
  switch (t.base) {
    case 'index': return `{ let id = ${x}.node_id(); list(v.get(${JSON.stringify(t.of)}), ${p})?.iter().position(|item| item.node_id().is_some() && item.node_id() == id).ok_or_else(|| format!("{}: not one of ${t.of}", ${p}))? as u32 }`;
    case 'f64': return `num(${x}, ${p})?`;
    case 'f32': return `num_f32(${x}, ${p})?`;
    case 'i32': return `(int(${x}, ${p}, i32::MIN as f64, i32::MAX as f64)? as i32)`;
    case 'u32': return `(int(${x}, ${p}, 0.0, u32::MAX as f64)? as u32)`;
    case 'u8': return `(int(${x}, ${p}, 0.0, 255.0)? as u8)`;
    case 'bool': return `boolean(${x}, ${p})?`;
    case 'vec3': return `vec3(${x}, ${p})?`;
    case 'quat': return `quat(${x}, ${p})?`;
    case 'mat4': return `mat4(${x}, ${p})?`;
    case 'f64xN': return `numbers::<${t.n}>(${x}, ${p})?`;
    case 'str': return `cx.str_in(${x}, ${p})?`;
    case 'enum': return `${t.of}::from_u8(name_of(${x}, &${t.of}::NAMES, ${p})?).unwrap()`;
    case 'def': return `cx.def_in(${JSON.stringify(t.of)}, ${x}, ${p})?`;
    case 'ref': return one(x, p);
    case 'union': case 'rec': return `${t.of}::from_value(${x}, cx, ${p})?`;
    case 'list': return `{ let mut out = Vec::new(); for (i, e) in list(${x}, ${p})?.into_iter().enumerate() { let at = format!("{}[{i}]", ${p}); out.push(${replayIn(model, t.value, 'e', '&at')}); } out }`;
    case 'set': return `{ let mut out = RefSet::default(); for (i, e) in set(${x}, ${p})?.into_iter().enumerate() { let at = format!("{}[{i}]", ${p}); out.items.push(${one('e', '&at')}); } out }`;
    case 'map': return `{ let mut out = StrMap::default(); for (key, e) in map(${x}, ${p})? { let at = format!("{}<{}>", ${p}, key.show()); let id = cx.str_in(key, &at)?; let value = ${replayIn(model, t.value, 'e', '&at')}; out.entries.push((id, value)); } out }`;
  }
}

function replayInField(model, t, x, p, home = false) {
  const inner = home ? `{ let mut out = Vec::new(); for (i, e) in list(${x}, ${p})?.into_iter().enumerate() { let at = format!("{}[{i}]", ${p}); out.push(${model.kinds.get(t.of).rust}::value_in(e, cx, &at)?); } out }` : replayIn(model, bare(t), x, p);
  if (!optional(t)) return inner;
  const bad = (what) => `return Err(format!("{}: ${what}", ${p}))`;
  if (t.nullable.null && t.nullable.undefined) return `match ${x}.value { Value::Null => Nullable::Null, Value::Undefined => Nullable::Undefined, _ => Nullable::Value(${inner}) }`;
  return t.nullable.null
    ? `match ${x}.value { Value::Null => None, Value::Undefined => ${bad('undefined where its schema says null')}, _ => Some(${inner}) }`
    : `match ${x}.value { Value::Undefined => None, Value::Null => ${bad('null where its schema says undefined')}, _ => Some(${inner}) }`;
}

// The value of the bare type of Rust expression `s`.
function replayOut(model, t, s) {
  const k = ['ref', 'set'].includes(t.base) ? model.kinds.get(t.of) : null;
  const one = (r) => (k.stored ? `${k.rust}::ref_out(${r}, cx, a)` : `cx.ref_out(${kindConst(k.name)}, ${r})`);
  switch (t.base) {
    case 'index': return `item_of(a, &fields, ${JSON.stringify(t.of)}, ${s} as usize)`;
    case 'f64': return `Value::Num(${s})`;
    case 'f32': case 'i32': case 'u32': case 'u8': return `Value::Num(${s} as f64)`;
    case 'bool': return `Value::Bool(${s})`;
    case 'vec3': return `a.vec3(${s})`;
    case 'quat': return `a.quat(${s})`;
    case 'mat4': return `mat4_value(a, ${s})`;
    case 'f64xN': return `{ let items = ${s}.iter().map(|&x| Value::Num(x)).collect(); a.array(items) }`;
    case 'str': return `cx.str_out(${s})`;
    case 'enum': return `Value::Str(${t.of}::NAMES[${s} as usize].to_string())`;
    case 'def': return `cx.def_out(${JSON.stringify(t.of)}, ${s})`;
    case 'ref': return one(s);
    case 'union': case 'rec': return `${s}.to_value(cx, a)`;
    case 'list': {
      const v = t.value, item = ['rec', 'list', 'set', 'map'].includes(v.base) ? 'e' : '(*e)';
      return `{ let mut items = Vec::new(); for e in ${s}.iter() { let x = ${replayOut(model, v, item)}; items.push(x); } a.array(items) }`;
    }
    case 'set': return `{ let mut items = Vec::new(); for &r in ${s}.items.iter() { items.push(${one('r')}); } a.set(items) }`;
    case 'map': {
      const v = t.value, value = ['rec', 'list', 'set'].includes(v.base) ? 'e' : '(*e)';
      return `{ let mut entries = Vec::new(); for (key, e) in ${s}.entries.iter() { let k = cx.str_out(*key); let x = ${replayOut(model, v, value)}; entries.push((k, x)); } a.map(entries) }`;
    }
  }
}

function replayOutField(model, t, s, home = false) {
  if (home) return `{ let mut items = Vec::new(); for &r in ${s}.iter() { let x = ${model.kinds.get(t.of).rust}::value_out(r, cx, a); items.push(x); } a.array(items) }`;
  if (!optional(t)) return replayOut(model, t, s);
  const inner = replayOut(model, bare(t), deref(t));
  if (t.nullable.null && t.nullable.undefined) return `match &${s} { Nullable::Value(v) => ${inner}, Nullable::Null => Value::Null, Nullable::Undefined => Value::Undefined }`;
  return `match &${s} { Some(v) => ${inner}, None => Value::${t.nullable.null ? 'Null' : 'Undefined'} }`;
}

function replayStruct(model, name, fields, what) {
  const lines = [`impl ${name} {`];
  lines.push('    /// Its fields from a value (the export of the JavaScript object).', `    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {`, '        object(v, path)?;', `        Ok(${name} {`);
  for (const f of fields) lines.push(`            ${snake(f.name)}: { let p = format!("{path}.${f.name}"); ${replayInField(model, f.type, `v.get(${JSON.stringify(f.name)})`, '&p', model.isHome(f))} },`);
  lines.push('        })', '    }', '');
  lines.push('    /// Its value, as the export of the JavaScript object it stands for.', `    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {`, `        let mut fields: Vec<(&str, Value)> = Vec::with_capacity(${fields.length});`);
  for (const f of fields) lines.push(`        { let x = ${replayOutField(model, f.type, `self.${snake(f.name)}`, model.isHome(f))}; fields.push((${JSON.stringify(f.name)}, x)); }`);
  lines.push(`        a.object("", fields)`, '    }');
  if (what) {
    const k = what;
    const slab = `cx.store.${snake(k.name)}`;
    const make = k.key
      ? `|cx, key| { let slot = key.unwrap_or(0); if let Some((old, _)) = ${slab}.at(slot) { ${slab}.remove(old); } let r = ${slab}.insert_at(slot, 0, ${name}::default()); (r.slot, r.gen) }`
      : `|cx, _| { let r = ${slab}.insert(${name}::default()); (r.slot, r.gen) }`;
    lines.push('');
    lines.push(`    /// The ${k.name} a handle names (made with its fields' defaults the first time;`, '    /// its value fills them in where it lives).', `    pub fn ref_in(v: V, cx: &mut Replay, path: &str) -> Result<Ref<${name}>> {`, `        let (slot, gen) = cx.stored_in(${kindConst(k.name)}, v, path, ${make})?;`, '        Ok(Ref::new(slot, gen))', '    }', '');
    lines.push(`    /// A ${k.name}'s value where it lives (its home list): its handle (\`$id\`) and fields.`, `    pub fn value_in(v: V, cx: &mut Replay, path: &str) -> Result<Ref<${name}>> {`, '        object(v, path)?;', `        let r = ${name}::ref_in(v.get("$id"), cx, &format!("{path}.$id"))?;`, `        let value = ${name}::from_value(v, cx, path)?;`, `        *${slab}.get_mut(r).ok_or_else(|| format!("{path}: its entity is gone"))? = value;`, '        Ok(r)', '    }', '');
    lines.push(`    /// The handle of the ${k.name} a reference names.`, `    pub fn ref_out(r: Ref<${name}>, cx: &mut Replay, a: &mut Arena) -> Value { cx.stored_out(${kindConst(k.name)}, r.slot, r.gen) }`, '');
    lines.push(`    /// A ${k.name}'s value where it lives: its handle first, then its fields.`, `    pub fn value_out(r: Ref<${name}>, cx: &mut Replay, a: &mut Arena) -> Value {`, `        let id = cx.stored_out(${kindConst(k.name)}, r.slot, r.gen);`, `        let e = ${slab}.get(r).cloned().unwrap_or_default();`, '        let v = e.to_value(cx, a);', '        if let Value::Node(n) = v { if let Node::Object { fields, .. } = &mut a.nodes[n as usize] { fields.insert(0, ("$id".to_string(), id)); } }', '        v', '    }');
  }
  lines.push('}', '');
  return lines;
}

function replayUnion(model, u) {
  const values = u.variants.filter((x) => x.tag === 'value');
  const recs = values.filter((x) => x.type.base === 'rec');
  const tagged = recs.length > 1; // a record's value then names its record ($record)
  const lines = [`impl ${u.name} {`, `    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {`, '        match v.value {'];
  const nul = u.variants.find((x) => x.tag === 'null'), und = u.variants.find((x) => x.tag === 'undefined');
  if (nul) lines.push(`            Value::Null => return Ok(${u.name}::${nul.rust}),`);
  if (und) lines.push(`            Value::Undefined => return Ok(${u.name}::${und.rust}),`);
  const lits = u.variants.filter((x) => x.tag === 'lit');
  const strs = values.filter((x) => x.type.base === 'str' || x.type.base === 'enum');
  if (lits.length || strs.length) {
    const arms = lits.map((x) => `${JSON.stringify(x.value)} => return Ok(${u.name}::${x.rust}),`);
    const rest = strs.map((x) => (x.type.base === 'str' ? `return Ok(${u.name}::${x.rust}(cx.str_in(v, path)?));` : `if let Ok(i) = name_of(v, &${x.type.of}::NAMES, path) { return Ok(${u.name}::${x.rust}(${x.type.of}::from_u8(i).unwrap())); }`)).join(' ');
    lines.push(`            Value::Str(s) => { match s.as_str() { ${arms.join(' ')} _ => {} } ${rest} }`);
  }
  const num = values.find((x) => ['f64', 'f32', 'i32', 'u32', 'u8'].includes(x.type.base));
  if (num) lines.push(`            Value::Num(_) => return Ok(${u.name}::${num.rust}(${replayIn(model, num.type, 'v', 'path')})),`);
  const bool = values.find((x) => x.type.base === 'bool');
  if (bool) lines.push(`            Value::Bool(b) => return Ok(${u.name}::${bool.rust}(*b)),`);
  const handles = u.variants.filter((x) => x.tag === 'ref');
  const refIn = (x) => (model.kinds.get(x.kind).stored ? `${model.kinds.get(x.kind).rust}::ref_in(v, cx, path)?` : `cx.ref_in(${kindConst(x.kind)}, v, path)?`);
  if (handles.length) lines.push(`            Value::Handle(kind, _) => match kind.as_str() { ${handles.map((x) => `${JSON.stringify(x.kind)} => return Ok(${u.name}::${x.rust}(${refIn(x)})),`).join(' ')} _ => {} },`);
  const shapes = values.filter((x) => ['vec3', 'quat', 'mat4', 'f64xN'].includes(x.type.base) || x.type.base === 'rec');
  if (shapes.length) {
    const arms = [];
    for (const x of values) {
      if (x.type.base === 'vec3') arms.push(`Some(Node::Vec3(_)) => return Ok(${u.name}::${x.rust}(vec3(v, path)?)),`);
      if (x.type.base === 'quat') arms.push(`Some(Node::Quat(_)) => return Ok(${u.name}::${x.rust}(quat(v, path)?)),`);
      if (x.type.base === 'mat4') arms.push(`Some(Node::Mat4(_)) => return Ok(${u.name}::${x.rust}(mat4(v, path)?)),`);
      if (x.type.base === 'f64xN') arms.push(`Some(Node::Array(_)) => return Ok(${u.name}::${x.rust}(numbers::<${x.type.n}>(v, path)?)),`);
    }
    if (recs.length) arms.push(`Some(Node::Object { .. }) => { ${tagged ? `match v.get("$record").str() { ${recs.map((x) => `Some(${JSON.stringify(x.type.of)}) => return Ok(${u.name}::${x.rust}(${x.type.of}::from_value(v, cx, path)?)),`).join(' ')} _ => {} }` : `return Ok(${u.name}::${recs[0].rust}(${recs[0].type.of}::from_value(v, cx, path)?));`} }`);
    lines.push(`            Value::Node(_) => match v.node() { ${arms.join(' ')} _ => {} },`);
  }
  lines.push('            _ => {}', '        }', `        Err(format!("{path}: none of ${u.name}'s variants ({})", v.show()))`, '    }', '');
  lines.push(`    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {`, '        match self {');
  for (const x of u.variants) {
    if (x.tag === 'null') lines.push(`            ${u.name}::${x.rust} => Value::Null,`);
    else if (x.tag === 'undefined') lines.push(`            ${u.name}::${x.rust} => Value::Undefined,`);
    else if (x.tag === 'lit') lines.push(`            ${u.name}::${x.rust} => Value::Str(${JSON.stringify(x.value)}.to_string()),`);
    else if (x.tag === 'ref') {
      const k = model.kinds.get(x.kind);
      lines.push(`            ${u.name}::${x.rust}(r) => ${k.stored ? `${k.rust}::ref_out(*r, cx, a)` : `cx.ref_out(${kindConst(k.name)}, *r)`},`);
    } else if (x.type.base === 'rec' && tagged) {
      lines.push(`            ${u.name}::${x.rust}(v) => { let out = v.to_value(cx, a); if let Value::Node(n) = out { if let Node::Object { fields, .. } = &mut a.nodes[n as usize] { fields.insert(0, ("$record".to_string(), Value::Str(${JSON.stringify(x.type.of)}.to_string()))); } } out }`);
    } else lines.push(`            ${u.name}::${x.rust}(v) => ${replayOut(model, x.type, deref(x.type).replace('(*v)', '(*v)'))},`);
  }
  lines.push('        }', '    }', '}', '');
  return lines;
}

function rustReplay(model) {
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/; do not edit.`, '//! Every kind, record and union read from and written as the replay harness\'s', '//! values (the `replay` feature; rust/sim/src/store/replay.rs).', '#![allow(clippy::all, unused_braces, unused_parens, unused_variables, unused_mut, unused_imports)]', '', 'use super::*;', 'use crate::store::replay::{boolean, int, item_of, list, map, mat4, mat4_value, name_of, num, num_f32, numbers, object, quat, set, vec3, Arena, Node, ReplayOf, Result, StoreKinds, Value, V};', '',
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
const jsGet = (f, o = 'o') => (f.js.length > 1 ? `${o}${jsKey(f.js[0])}${f.js.slice(1).map(jsOptKey).join('')}` : `${o}${jsPath(f.js)}`);
// Each object on the way made if missing (an array before a numeric key).
const jsSet = (f, o = 'm') => {
  let x = o;
  for (let i = 0; i < f.js.length - 1; i++) x = `(${x}${jsKey(f.js[i])} ??= ${/^\d+$/.test(f.js[i + 1]) ? '[]' : '{}'})`;
  return `${x}${jsKey(f.js.at(-1))}`;
};

// Block (export direction): read the bare type from words at `k` into
// `target`, updating a Vector3, Quaternion, array, record, list or set it
// holds in place (aliases of a mirror's Vector3 keep seeing it), making one
// when it holds none.
function jsSync(model, t, target, k) {
  switch (t.base) {
    case 'f64': case 'f32': case 'i32': case 'u32': case 'u8': return `${target} = f[at + ${k}];`;
    case 'index': return `${target} = ${target.replace(/\.[A-Za-z_$][\w$]*$/, '')}.${t.of}[f[at + ${k}]];`;
    case 'bool': return `${target} = f[at + ${k}] !== 0;`;
    case 'vec3': return `(${target} ??= new THREE.Vector3()).set(f[at + ${k}], f[at + ${k + 1}], f[at + ${k + 2}]);`;
    case 'quat': return `(${target} ??= new THREE.Quaternion()).set(f[at + ${k}], f[at + ${k + 1}], f[at + ${k + 2}], f[at + ${k + 3}]);`;
    case 'mat4': return `{ const e = (${target} ??= new THREE.Matrix4()).elements; for (let i = 0; i < 16; i++) e[i] = f[at + ${k} + i]; }`;
    case 'f64xN': return `{ const a = (${target} ??= new Array(${t.n})); ${Array.from({ length: t.n }, (_, i) => `a[${i}] = f[at + ${k + i}];`).join(' ')} }`;
    case 'str': return `${target} = b.strings.at(f[at + ${k}]);`;
    case 'enum': return `${target} = ${jsEnum(model.enums.get(t.of))}[f[at + ${k}]];`;
    case 'def': return `${target} = b.defs.${t.of}.at(f[at + ${k}]);`;
    case 'ref': return `${target} = b.refs[${kindConst(t.of)}].at(f[at + ${k}], f[at + ${k + 1}]);`;
    case 'union': return `${target} = sync${t.of}(b, f, at + ${k}, h, ${target});`;
    case 'rec': return `${target} = sync${t.of}(b, f, at + ${k}, h, ${target} ?? new ${t.of}Mirror());`;
    case 'list': {
      // Each item read into the one the array holds there (Vector3s and records kept), or a new one.
      const w = words(model, t.value);
      return `{ const items = (${target} ??= []), start = h + f[at + ${k}], n = f[at + ${k + 1}]; items.length = n; for (let i = 0; i < n; i++) { let v = items[i]; { const at = start + i * ${w}; ${jsSync(model, t.value, 'v', 0)} } items[i] = v; } }`;
    }
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
    case 'index': return `{ const $i = (o.${t.of} ?? []).indexOf(${v}); if ($i < 0) throw Error("${t.of}: the item is none of them"); f[at + ${k}] = $i; }`;
    case 'bool': return `f[at + ${k}] = ${v} ? 1 : 0;`;
    case 'vec3': return `{ const $v = ${v}; f[at + ${k}] = $v.x; f[at + ${k + 1}] = $v.y; f[at + ${k + 2}] = $v.z; }`;
    case 'quat': return `{ const $v = ${v}; f[at + ${k}] = $v.x; f[at + ${k + 1}] = $v.y; f[at + ${k + 2}] = $v.z; f[at + ${k + 3}] = $v.w; }`;
    case 'mat4': return `{ const e = ${v}.elements; for (let i = 0; i < 16; i++) f[at + ${k} + i] = e[i]; }`;
    case 'f64xN': return `{ const $v = ${v}; ${Array.from({ length: t.n }, (_, i) => `f[at + ${k + i}] = $v[${i}];`).join(' ')} }`;
    case 'str': return `f[at + ${k}] = b.strings.id(${v});`;
    case 'enum': return `f[at + ${k}] = ${jsEnumIndex(model.enums.get(t.of))}.get(${v});`;
    case 'def': return `f[at + ${k}] = b.defs.${t.of}.id(${v});`;
    case 'ref': return `b.refs[${kindConst(t.of)}].put(${v}, f, at + ${k});`;
    case 'union': return `put${t.of}(b, ${v}, f, at + ${k}, heap);`;
    case 'rec': return `put${t.of}(b, ${v}, f, at + ${k}, heap);`;
    case 'set': return `heap.list(b.refs[${kindConst(t.of)}], ${v}, f, at + ${k});`;
    case 'list': return `heap.items(${v}, f, at + ${k}, ${words(model, t.value)}, (f, at, v) => { ${jsPut(model, t.value, 'v', 0)} });`;
    case 'map': return `heap.map(b.strings, ${v}, f, at + ${k}, ${entryWords(model, t)}, (f, at, v) => { ${jsPut(model, t.value, 'v', 1)} });`;
  }
}

function jsPutField(model, t, v, k) {
  if (!optional(t)) return jsPut(model, t, v, k);
  const n = words(model, t);
  return `{ const $x = ${v}; if ($x === null || $x === undefined) { f[at + ${k}] = $x === null ? 1 : 2; f.fill(0, at + ${k + 1}, at + ${k + n}); } else { f[at + ${k}] = 0; ${jsPut(model, bare(t), '$x', k + 1)} } }`;
}

// Comparing two objects (Object.is for numbers, identity for references):
// the first differing path, or null.
function jsSame(model, t, a, b, path) {
  const p = JSON.stringify(path);
  switch (t.base) {
    case 'vec3': return `if (!sameVec(${a}, ${b})) return ${p};`;
    case 'quat': return `if (!sameQuat(${a}, ${b})) return ${p};`;
    case 'mat4': return `if (!sameArray(${a}?.elements, ${b}?.elements, 16)) return ${p};`;
    case 'f64xN': return `if (!sameArray(${a}, ${b}, ${t.n})) return ${p};`;
    case 'rec': return `{ const d = same${t.of}(${a}, ${b}); if (d) return ${p} + d; }`;
    case 'union': return `if (!same${t.of}(${a}, ${b})) return ${p};`;
    case 'list': return `if (!sameItems(${a}, ${b}, (x, y) => ${jsSameValue(model, t.value, 'x', 'y')})) return ${p};`;
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
    case 'mat4': return `sameArray(${a}?.elements, ${b}?.elements, 16)`;
    case 'f64xN': return `sameArray(${a}, ${b}, ${t.n})`;
    case 'rec': return `same${t.of}(${a}, ${b}) === null`;
    case 'union': return `same${t.of}(${a}, ${b})`;
    case 'list': return `sameItems(${a}, ${b}, (x, y) => ${jsSameValue(model, t.value, 'x', 'y')})`;
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
  // Any reference is the handle the context makes; entities are written
  // out whole only where they live (their home list, jsExportFn).
  const one = (name, x) => `cx.ref(${JSON.stringify(name)}, ${x})`;
  switch (t.base) {
    case 'rec': return `export${t.of}(${v}, cx)`;
    case 'union': return `export${t.of}(${v}, cx)`;
    case 'def': return `cx.def(${JSON.stringify(t.of)}, ${v})`;
    case 'ref': return one(t.of, v);
    case 'list': return `${v}.map((x) => ${jsExport(model, t.value, 'x')})`;
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
    case 'mat4': return 'new THREE.Matrix4()';
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
  for (const f of fields) {
    if (f.type.base === 'index') {
      // The same item of the list (by its place), or both null or undefined alike.
      const [x, y, list] = [jsGet(f, 'a'), jsGet(f, 'b'), f.type.of];
      lines.push(`  { const x = ${x}, y = ${y}; if (x === null || x === undefined || y === null || y === undefined ? x !== y : (a.${list} ?? []).indexOf(x) !== (b.${list} ?? []).indexOf(y)) return ${JSON.stringify(`.${f.name}`)}; }`);
      continue;
    }
    lines.push(`  ${jsSameField(model, f.type, jsGet(f, 'a'), jsGet(f, 'b'), `.${f.name}`)}`);
  }
  lines.push('  return null;', '}', '');
  return lines;
}

function jsExportFn(model, name, fields, kind = null) {
  const lines = [`export function export${name}(o, cx) {`, '  let v = cx.seen.get(o);', '  if (v) return v;', '  v = {};', '  cx.seen.set(o, v);'];
  // An entity of a kind the store holds leads with its own handle, which
  // references to it elsewhere are.
  if (kind && !kind.singleton) lines.push(`  v.$id = cx.ref(${JSON.stringify(kind.name)}, o);`);
  for (const f of fields) {
    const home = model.isHome(f);
    if (f.type.base === 'index') {
      // The very value the list's item was exported as (cx.seen), so the
      // capture holds one object where JavaScript held one.
      const rec = fields.find((g) => g.name === f.type.of).type.value.of;
      lines.push(`  v${jsKey(f.name)} = ((x) => (x === null || x === undefined ? x : export${rec}(x, cx)))(${jsGet(f)});`);
      continue;
    }
    lines.push(`  v${jsKey(f.name)} = ${home ? `${jsGet(f)}.map((x) => export${model.kinds.get(f.type.of).rust}(x, cx))` : jsExportField(model, f.type, jsGet(f))};`);
  }
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
  const values = u.variants.map((v, i) => [v, i]).filter(([v]) => v.tag === 'value');
  const recs = values.filter(([v]) => v.type.base === 'rec');
  const tagged = recs.length > 1;
  const refs = u.variants.map((v, i) => [v, i]).filter(([v]) => v.tag === 'ref');
  // Which variant a value is (-1: none): null, undefined and literals; then
  // numbers, booleans and strings by their type; vectors and matrices by
  // three's flags; records by their class's test (b.recordTests); the kinds
  // the store refers to by their tests; and last, the one kind it holds or
  // the one record with no class, as any other object.
  const tests = [];
  u.variants.forEach((v, i) => {
    if (v.tag === 'null') tests.push(`if (x === null) return ${i};`);
    else if (v.tag === 'undefined') tests.push(`if (x === undefined) return ${i};`);
    else if (v.tag === 'lit') tests.push(`if (x === ${JSON.stringify(v.value)}) return ${i};`);
  });
  for (const [v, i] of values) {
    const t = v.type;
    if (['f64', 'f32', 'i32', 'u32', 'u8'].includes(t.base)) tests.push(`if (typeof x === "number") return ${i};`);
    else if (t.base === 'bool') tests.push(`if (typeof x === "boolean") return ${i};`);
    else if (t.base === 'str') tests.push(`if (typeof x === "string") return ${i};`);
    else if (t.base === 'enum') tests.push(`if (${jsEnumIndex(model.enums.get(t.of))}.has(x)) return ${i};`);
    else if (t.base === 'vec3') tests.push(`if (x?.isVector3) return ${i};`);
    else if (t.base === 'quat') tests.push(`if (x?.isQuaternion) return ${i};`);
    else if (t.base === 'mat4') tests.push(`if (x?.isMatrix4) return ${i};`);
    else if (t.base === 'f64xN') tests.push(`if (Array.isArray(x) && x.length === ${t.n}) return ${i};`);
  }
  for (const [v, i] of recs) if (model.records.get(v.type.of).classes.length) tests.push(`if (b?.recordTests?.${v.type.of}?.(x)) return ${i};`);
  for (const [v, i] of refs) if (!model.kinds.get(v.kind).stored) tests.push(`if (b?.refs[${kindConst(v.kind)}].is(x)) return ${i};`);
  const rest = refs.find(([v]) => model.kinds.get(v.kind).stored) ?? recs.find(([v]) => !model.records.get(v.type.of).classes.length);
  if (rest) tests.push(`if (x !== null && typeof x === "object") return ${rest[1]};`);
  lines.push(`export function variantOf${u.name}(b, x) {`, ...tests.map((t) => `  ${t}`), '  return -1;', '}', '');
  lines.push(`// Which of ${u.name}'s variants a value is.`, `export function which${u.name}(b, x) {`, `  const i = variantOf${u.name}(b, x);`, `  if (i < 0) throw Error(\`${u.name}: \${x?.constructor?.name ?? x} is none of its variants\`);`, '  return i;', '}', '');
  const w = unionWords(model, u);
  lines.push(`// An export row's union (from \`at\`) as a value: a vector or record the field held, if it is of the variant, read into in place.`, `export function sync${u.name}(b, f, at, h, current) {`, '  switch (f[at]) {');
  u.variants.forEach((v, i) => {
    if (v.tag === 'null') lines.push(`    case ${i}: return null;`);
    else if (v.tag === 'undefined') lines.push(`    case ${i}: return undefined;`);
    else if (v.tag === 'lit') lines.push(`    case ${i}: return ${JSON.stringify(v.value)};`);
    else if (v.tag === 'ref') lines.push(`    case ${i}: return b.refs[${kindConst(v.kind)}].at(f[at + 1], f[at + 2]);`);
    else lines.push(`    case ${i}: { let v = variantOf${u.name}(b, current) === ${i} ? current : undefined; ${jsSync(model, v.type, 'v', 1)} return v; }`);
  });
  lines.push(`    default: throw Error('${u.name}: no such variant');`, '  }', '}', '');
  lines.push(`export function put${u.name}(b, x, f, at, heap) {`, `  const i = which${u.name}(b, x);`, `  f.fill(0, at + 1, at + ${w});`, '  f[at] = i;', '  switch (i) {');
  for (const [v, i] of refs) lines.push(`    case ${i}: b.refs[${kindConst(v.kind)}].put(x, f, at + 1); break;`);
  for (const [v, i] of values) lines.push(`    case ${i}: { ${jsPut(model, v.type, 'x', 1)} break; }`);
  lines.push('  }', '}', '');
  // Same: identity, else of one value variant with the same value.
  lines.push(`export function same${u.name}(x, y) {`, '  if (Object.is(x, y)) return true;', '  if (x === null || y === null || x === undefined || y === undefined) return false;');
  for (const [v] of values) {
    const t = v.type;
    if (['f64', 'f32', 'i32', 'u32', 'u8', 'bool', 'str', 'enum'].includes(t.base)) continue; // Object.is above
    if (t.base === 'vec3') lines.push('  if (x.isVector3 && y.isVector3) return sameVec(x, y);');
    else if (t.base === 'quat') lines.push('  if (x.isQuaternion && y.isQuaternion) return sameQuat(x, y);');
    else if (t.base === 'mat4') lines.push('  if (x.isMatrix4 && y.isMatrix4) return sameArray(x.elements, y.elements, 16);');
    else if (t.base === 'f64xN') lines.push(`  if (Array.isArray(x) && Array.isArray(y)) return sameArray(x, y, ${t.n});`);
    else if (t.base === 'rec') lines.push(`  if (typeof x === "object" && typeof y === "object" && same${t.of}(x, y) === null) return true;`);
  }
  lines.push('  return false;', '}', '');
  lines.push(`export function export${u.name}(x, cx) {`, `  switch (which${u.name}(cx, x)) {`);
  u.variants.forEach((v, i) => {
    let out;
    if (v.tag === 'null') out = 'null';
    else if (v.tag === 'undefined') out = 'undefined';
    else if (v.tag === 'lit') out = JSON.stringify(v.value);
    else if (v.tag === 'ref') out = `cx.ref(${JSON.stringify(v.kind)}, x)`;
    else if (v.type.base === 'rec') out = tagged ? `Object.assign({ $record: ${JSON.stringify(v.type.of)} }, export${v.type.of}(x, cx))` : `export${v.type.of}(x, cx)`;
    else out = jsExport(model, v.type, 'x');
    lines.push(`    case ${i}: return ${out};`);
  });
  lines.push('  }', '}', '');
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
  lines.push(...jsExportFn(model, k.rust, k.fields, k));
  return lines;
}

function jsFile(model, file) {
  // Names this file uses from other files, and the kind codes it uses.
  const uses = new Map();
  const use = (name, from) => { if (from !== file) { if (!uses.has(from)) uses.set(from, new Set()); uses.get(from).add(name); } };
  const codes = new Set();
  let t0Field = null;
  const need = (t0) => {
    const t = ['map', 'list'].includes(t0.base) ? t0.value : t0;
    if (t.base === 'enum') { const e = model.enums.get(t.of); use(jsEnum(e), e.file); use(jsEnumIndex(e), e.file); }
    if (t.base === 'union') { const u = model.unions.get(t.of); for (const p of ['sync', 'put', 'export', 'same']) use(`${p}${u.name}`, u.file); }
    if (t.base === 'rec') { const r = model.records.get(t.of); for (const p of ['sync', 'put', 'same', 'export']) use(`${p}${r.name}`, r.file); use(`${r.name}Mirror`, r.file); }
    if (['ref', 'set'].includes(t.base) || (t0.base === 'list' && t.base === 'ref')) codes.add(kindConst(t.of));
    if (t0.base === 'list' && t.base === 'ref' && model.kinds.get(t.of).homeField === t0Field) use(`export${model.kinds.get(t.of).rust}`, model.kinds.get(t.of).file);
  };
  for (const r of model.records.values()) if (r.file === file) r.fields.forEach((f) => { t0Field = f; need(f.type); });
  for (const k of model.kindList) if (k.file === file) k.fields.forEach((f) => { t0Field = f; need(f.type); });
  for (const u of model.unions.values()) if (u.file === file) for (const v of u.variants) {
    if (v.tag === 'ref') codes.add(kindConst(v.kind));
    if (v.tag === 'value') need(v.type);
  }
  const lines = [`// Generated by tools/sim-schema.mjs from ${model.target.schema}/${file}.mjs; do not edit.`, 'import * as THREE from "three";', `import { sameArray, sameItems, sameList, sameMap, sameQuat, sameVec } from "${model.target.compare}";`];
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
  // Commands', events' and imports' codes (their names in order, from 1).
  for (const [prefix, list] of [['COMMAND', model.commandsList], ['EVENT', model.eventsList], ['IMPORT', model.importsList]]) for (const c of list) lines.push(`export const ${prefix}_${upper(c.name)} = ${c.code};`);
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
  lines.push('// The records a union tells apart by class: their classes.', `export const RECORD_CLASSES = ${JSON.stringify(Object.fromEntries([...model.records.values()].filter((r) => r.classes.length).map((r) => [r.name, r.classes])))};`, '');
  const recFns = (r) => ({ words: words(model, { base: 'rec', of: r, nullable: { null: false, undefined: false } }) });
  const fnsOf = (r) => `{ words: ${recFns(r).words}, Mirror: ${r}Mirror, sync: sync${r}, put: put${r} }`;
  lines.push('// Commands, events and imports by code: name, and their records\' words and functions.');
  // A flat command's binding: its words into the bridge's scratch, then one call.
  const fast = (c) => {
    if (model.target.test || !flat(model, c.args)) return 'null';
    const n = recordWords(model, c.args);
    return `(b, o) => { const s = b.words; put${c.args}(b, o, s, 0, null); return b.x.cmd_${snake(c.name).replace(/^r#/, '')}(${Array.from({ length: n }, (_, i) => `s[${i}]`).join(', ')}); }`;
  };
  lines.push(`export const COMMAND_TABLE = [null, ${model.commandsList.map((c) => `{ name: ${JSON.stringify(c.name)}, code: ${c.code}, args: ${fnsOf(c.args)}, result: ${c.result ? fnsOf(c.result) : 'null'}, fast: ${fast(c)} }`).join(', ')}];`);
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
