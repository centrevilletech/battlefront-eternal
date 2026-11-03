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
const SCHEMA = join(ROOT, 'rust/sim/schema');
const RUST_OUT = join(ROOT, 'rust/sim/src/gen');
const JS_OUT = join(ROOT, 'src/sim/gen');

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
  if (SCALARS.has(base)) type.base = base;
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
  const model = { enums: new Map(), unions: new Map(), records: new Map(), kinds: new Map(), files: files.map((f) => f.file) };
  const names = new Map(); // Rust type name -> where
  const claim = (rust, where) => {
    if (names.has(rust)) fail(where, `${rust} is also ${names.get(rust)}`);
    names.set(rust, where);
  };
  for (const { file, schema } of files) {
    for (const k of Object.keys(schema)) if (!['enums', 'unions', 'records', 'kinds'].includes(k)) fail(file, `unknown section "${k}"`);
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
      for (const k of Object.keys(spec)) if (!['singleton', 'key', 'home', 'fields', 'skip', 'doc'].includes(k)) fail(where, `unknown kind option "${k}"`);
      let kind = model.kinds.get(name);
      if (!kind) {
        kind = { name, rust: pascal(name), singleton: false, key: null, home: null, fields: [], skip: new Map(), file: null, facets: [] };
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
    const t = field.type;
    if (t.base === 'enum' && !model.enums.has(t.of)) fail(field.where, `no enum ${t.of}`);
    if (t.base === 'union' && !model.unions.has(t.of)) fail(field.where, `no union ${t.of}`);
    if (t.base === 'rec' && !model.records.has(t.of)) fail(field.where, `no record ${t.of}`);
    if (['ref', 'list', 'set'].includes(t.base) && !model.kinds.has(t.of)) fail(field.where, `no kind ${t.of}`);
    if (['ref', 'list', 'set'].includes(t.base) && model.kinds.get(t.of).singleton) fail(field.where, `${t.of} is a singleton`);
  };
  for (const r of model.records.values()) r.fields.forEach(check);
  for (const k of kinds) k.fields.forEach(check);
  for (const u of model.unions.values()) for (const v of u.variants) if (v.tag === 'ref' && !model.kinds.has(v.kind)) fail(`union ${u.name}`, `no kind ${v.kind}`);
  // Records nest by value: no record may hold itself.
  const visiting = new Set();
  const walk = (name) => {
    if (visiting.has(name)) fail(`record ${name}`, 'holds itself');
    visiting.add(name);
    for (const f of model.records.get(name).fields) if (f.type.base === 'rec') walk(f.type.of);
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
    case 'list': case 'set': return tag + 2;
    case 'rec': return tag + model.records.get(type.of).fields.reduce((n, f) => n + words(model, f.type), 0);
    default: return tag + 1;
  }
}

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
  }
  if (inner || !optional(t)) return base;
  return t.nullable.null && t.nullable.undefined ? `Nullable<${base}>` : `Option<${base}>`;
}

// Inside a match on an optional field, `v` is a reference: the value of a
// copyable type is `(*v)`, a record, list or set is used through `v`.
const deref = (t) => (['rec', 'list', 'set'].includes(t.base) ? 'v' : '(*v)');

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
  const lines = rustStruct(model, k.rust, k.fields, `A ${k.name}${k.singleton ? ' (one a battle)' : ''}, from ${k.facets.map((f) => `rust/sim/schema/${f}.mjs`).join(', ')}.`);
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
  const lines = [`// Generated by tools/sim-schema.mjs from rust/sim/schema/${file}.mjs; do not edit.`, '#![allow(clippy::all, unused_braces, unused_parens, unused_variables)]', '', 'use super::*;', ''];
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
    '// Generated by tools/sim-schema.mjs from rust/sim/schema/; do not edit.',
    '//! The store\'s generated code: a module a schema file, and here the kinds\'',
    '//! codes, the store that holds every kind with fields, and its sync',
    '//! dispatch (docs/sim-store.md).',
    '#![allow(clippy::all, unused_variables)]',
    '',
    'pub use crate::store::block::Block;',
    'pub use crate::store::{word_i32, word_u32, DefId, KindCode, Nullable, Quat, Ref, RefSet, Slab, StrId, Vec3};',
    '',
  ];
  for (const f of model.files) lines.push(`mod ${snake(f)};`, `pub use self::${snake(f)}::*;`);
  lines.push('');
  for (const k of kinds) lines.push(`pub const ${kindConst(k.name)}: KindCode = ${k.code};`);
  lines.push('/// How many kinds there are (codes run 1 to KINDS).', `pub const KINDS: usize = ${kinds.length};`);
  lines.push(`pub const KIND_NAMES: [&str; ${kinds.length + 1}] = ["", ${kinds.map((k) => JSON.stringify(k.name)).join(', ')}];`);
  lines.push('/// Each kind\'s layout hash (its fields, types, flags and order), by code: the', '/// generated JavaScript has the same, and the bridge checks them.', `pub const KIND_HASHES: [u32; ${kinds.length + 1}] = [0, ${kinds.map((k) => `0x${k.hash.toString(16).padStart(8, '0')}`).join(', ')}];`, '');
  for (const k of kinds.filter((x) => !x.stored && !x.singleton)) lines.push('/// A kind the store holds no fields of (only references to).', '#[derive(Clone, Copy, Debug, Default)]', `pub struct ${k.rust};`, '');
  lines.push('/// Every kind with fields: a singleton by value, the rest by slot.', '#[derive(Default)]', 'pub struct Store {');
  for (const k of held) lines.push(`    pub ${snake(k.name)}: ${k.singleton ? k.rust : `Slab<${k.rust}>`},`);
  lines.push('}', '');
  // A kind's entities in its order: its home list's, or its slots'.
  const order = (k) => (k.home ? `self.${snake(k.homeKind.name)}.${snake(k.homeField.name)}.clone()` : `self.${snake(k.name)}.iter().map(|(r, _)| r).collect::<Vec<_>>()`);
  lines.push('impl Store {');
  lines.push('    /// A kind\'s export block: a row for each entity (in its home list\'s order, or', '    /// its slots\'), its mirrored fields, or all of them (`full`). False for a kind', '    /// the store holds no fields of.', '    pub fn export(&self, kind: KindCode, block: &mut Block, full: bool) -> bool {', '        match kind {');
  for (const k of held) {
    if (k.singleton) {
      lines.push(`            ${kindConst(k.name)} => {`, `                block.begin(1, if full { ${k.rust}::FULL_STRIDE } else { ${k.rust}::EXPORT_STRIDE });`, '                let (row, heap) = block.row(0);', `                if full { self.${snake(k.name)}.put_full(row, heap) } else { self.${snake(k.name)}.put_export(row, heap) }`, '            }');
    } else {
      lines.push(`            ${kindConst(k.name)} => {`, `                let list = ${order(k)};`, `                block.begin(list.len(), if full { ${k.rust}::FULL_STRIDE } else { ${k.rust}::EXPORT_STRIDE });`, '                for (i, &r) in list.iter().enumerate() {', '                    let (row, heap) = block.row(i);', '                    row[0] = r.slot as f64;', '                    row[1] = r.gen as f64;', `                    if let Some(v) = self.${snake(k.name)}.get(r) { if full { v.put_full(row, heap) } else { v.put_export(row, heap) } }`, '                }', '            }');
    }
  }
  lines.push('            _ => return false,', '        }', '        block.finish();', '        true', '    }', '');
  lines.push('    /// Takes in an import block JavaScript wrote, its import fields or all of them', '    /// (`full`): each row\'s entity (made if its slot holds none of that generation)', '    /// gets the row\'s fields, and what the block no longer lists is removed.', '    pub fn import(&mut self, kind: KindCode, block: &Block, full: bool) -> bool {', '        match kind {');
  for (const k of held) {
    if (k.singleton) {
      lines.push(`            ${kindConst(k.name)} => {`, '                let (row, heap) = block.read(0);', `                if full { self.${snake(k.name)}.take_full(row, heap) } else { self.${snake(k.name)}.take_import(row, heap) }`, '            }');
    } else {
      lines.push(`            ${kindConst(k.name)} => {`, '                let mut listed: Vec<bool> = Vec::new();', '                for i in 0..block.rows() {', '                    let (row, heap) = block.read(i);', '                    let r = Ref::new(word_u32(row[0]), word_u32(row[1]));', `                    let v = self.${snake(k.name)}.ensure(r);`, '                    if full { v.take_full(row, heap) } else { v.take_import(row, heap) }', '                    let s = r.slot as usize;', '                    if listed.len() <= s { listed.resize(s + 1, false); }', '                    listed[s] = true;', '                }', `                self.${snake(k.name)}.retain(|r, _| listed.get(r.slot as usize).copied().unwrap_or(false));`, '            }');
    }
  }
  lines.push('            _ => return false,', '        }', '        true', '    }', '}');
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
    default: return `if (!Object.is(${a}, ${b})) return ${p};`;
  }
}

function jsSameField(model, t, a, b, path) {
  if (!optional(t)) return jsSame(model, t, a, b, path);
  return `{ const p = ${a}, q = ${b}; if (p === null || p === undefined || q === null || q === undefined) { if (p !== q) return ${JSON.stringify(path)}; } else { ${jsSame(model, bare(t), 'p', 'q', path)} } }`;
}

// The replay harness's value of a field (tools/sim-capture): a record and
// an entity of a kind the store holds are exported where they are met (one
// value an entity, through `seen`); everything else is the value itself,
// references included (the harness names units, bodies and definitions).
function jsExport(model, t, v) {
  switch (t.base) {
    case 'rec': return `export${t.of}(${v}, seen)`;
    case 'ref': return stored(model, t.of) ? `export${model.kinds.get(t.of).rust}(${v}, seen)` : v;
    case 'list': return stored(model, t.of) ? `${v}.map((x) => export${model.kinds.get(t.of).rust}(x, seen))` : v;
    case 'set': return stored(model, t.of) ? `new Set([...${v}].map((x) => export${model.kinds.get(t.of).rust}(x, seen)))` : v;
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
  const lines = [`export function export${name}(o, seen = null) {`, '  let v = seen?.get(o);', '  if (v) return v;', '  v = {};', '  seen?.set(o, v);'];
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
  for (const [v, i] of refs) tests.push(`if (b.refs[${kindConst(v.kind)}].is(x)) return ${i};`);
  lines.push(`// Which of ${u.name}'s variants a value is.`, `export function which${u.name}(b, x) {`, ...tests.map((t) => `  ${t}`), `  throw Error(\`${u.name}: \${x?.constructor?.name ?? x} is none of its variants\`);`, '}', '');
  lines.push(`export function sync${u.name}(b, f, at) {`, '  switch (f[at]) {');
  u.variants.forEach((v, i) => lines.push(`    case ${i}: return ${v.tag === 'null' ? 'null' : v.tag === 'undefined' ? 'undefined' : v.tag === 'lit' ? JSON.stringify(v.value) : `b.refs[${kindConst(v.kind)}].at(f[at + 1], f[at + 2])`};`));
  lines.push(`    default: throw Error('${u.name}: no such variant');`, '  }', '}', '');
  lines.push(`export function put${u.name}(b, x, f, at) {`, `  const i = which${u.name}(b, x);`, '  f[at] = i;');
  if (refs.length) lines.push(`  switch (i) { ${refs.map(([v, i]) => `case ${i}: b.refs[${kindConst(v.kind)}].put(x, f, at + 1); return;`).join(' ')} }`);
  lines.push('  f[at + 1] = 0;', '  f[at + 2] = 0;', '}', '');
  return lines;
}

function jsKind(model, k) {
  const ex = layout(model, k, 'export'), im = layout(model, k, 'import'), full = layout(model, k, 'full');
  const U = upper(k.name);
  const lines = [`// ${k.name}: from ${k.facets.map((f) => `rust/sim/schema/${f}.mjs`).join(', ')}.`];
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
  const need = (t) => {
    if (t.base === 'enum') { const e = model.enums.get(t.of); use(jsEnum(e), e.file); use(jsEnumIndex(e), e.file); }
    if (t.base === 'union') { const u = model.unions.get(t.of); for (const p of ['sync', 'put']) use(`${p}${u.name}`, u.file); }
    if (t.base === 'rec') { const r = model.records.get(t.of); for (const p of ['sync', 'put', 'same', 'export']) use(`${p}${r.name}`, r.file); use(`${r.name}Mirror`, r.file); }
    if (['ref', 'list', 'set'].includes(t.base)) {
      codes.add(kindConst(t.of));
      const k = model.kinds.get(t.of);
      if (k.stored) use(`export${k.rust}`, k.file);
    }
  };
  for (const r of model.records.values()) if (r.file === file) r.fields.forEach((f) => need(f.type));
  for (const k of model.kindList) if (k.file === file) k.fields.forEach((f) => need(f.type));
  for (const u of model.unions.values()) if (u.file === file) for (const v of u.variants) if (v.tag === 'ref') codes.add(kindConst(v.kind));
  const lines = [`// Generated by tools/sim-schema.mjs from rust/sim/schema/${file}.mjs; do not edit.`, 'import * as THREE from "three";', 'import { sameArray, sameList, sameQuat, sameVec } from "../compare.js";'];
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
  const lines = ['// Generated by tools/sim-schema.mjs from rust/sim/schema/; do not edit.', '// Each kind\'s code (its place among the kinds\' names, from 1).'];
  for (const k of model.kindList) lines.push(`export const ${kindConst(k.name)} = ${k.code};`);
  lines.push(`export const KINDS = ${model.kindList.length};`);
  return lines.join('\n') + '\n';
}

function jsIndex(model) {
  const lines = ['// Generated by tools/sim-schema.mjs from rust/sim/schema/; do not edit.', '// Every kind: its code, name, layout and generated functions, for the', '// store\'s runtime (src/sim/runtime.js) to dispatch on (docs/sim-store.md).'];
  for (const f of model.files) {
    const names = [];
    for (const k of model.kindList) if (k.file === f) names.push(`${k.rust}Mirror`, `sync${k.rust}`, `syncFull${k.rust}`, `put${k.rust}`, `putFull${k.rust}`, `check${k.rust}`, `same${k.rust}`, `export${k.rust}`, `${upper(k.name)}_KEYS`);
    if (names.length) lines.push(`import { ${names.join(', ')} } from "./${f}.js";`);
  }
  lines.push('export * from "./kinds.js";');
  for (const f of model.files) lines.push(`export * from "./${f}.js";`);
  lines.push('', '// By code: name, layout hash (the same as the module\'s), singleton, key (the', '// numeric field that names an object), home (singleton kind and list field),', '// whether the store holds fields of it, row strides, and its functions.', 'export const KIND_TABLE = [null,');
  for (const k of model.kindList) {
    const base = `name: ${JSON.stringify(k.name)}, code: ${k.code}, hash: 0x${k.hash.toString(16).padStart(8, '0')}, singleton: ${k.singleton}, key: ${JSON.stringify(k.key)}, home: ${k.home ? `{ kind: ${k.homeKind.code}, field: ${JSON.stringify(k.homeField.name)} }` : 'null'}, stored: ${k.stored}`;
    if (!k.stored) { lines.push(`  { ${base} },`); continue; }
    const s = (w) => layout(model, k, w).stride;
    lines.push(`  { ${base}, exportStride: ${s('export')}, importStride: ${s('import')}, fullStride: ${s('full')}, Mirror: ${k.rust}Mirror, sync: sync${k.rust}, syncFull: syncFull${k.rust}, put: put${k.rust}, putFull: putFull${k.rust}, check: check${k.rust}, same: same${k.rust}, export: export${k.rust}, keys: ${upper(k.name)}_KEYS },`);
  }
  lines.push('];', '', `export const DEF_TABLES = ${JSON.stringify([...new Set([...model.kindList.flatMap((k) => k.fields), ...[...model.records.values()].flatMap((r) => r.fields)].filter((f) => f.type.base === 'def').map((f) => f.type.of))].sort())};`);
  return lines.join('\n') + '\n';
}

// ---- writing ----

export async function generate() {
  const model = buildModel(await loadSchemas());
  const out = new Map();
  out.set(join(RUST_OUT, 'mod.rs'), rustIndex(model));
  for (const f of model.files) out.set(join(RUST_OUT, `${snake(f)}.rs`), rustFile(model, f));
  out.set(join(JS_OUT, 'kinds.js'), jsKinds(model));
  out.set(join(JS_OUT, 'index.js'), jsIndex(model));
  for (const f of model.files) out.set(join(JS_OUT, `${f}.js`), jsFile(model, f));
  return { model, out };
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
  for (const path of [...listed(RUST_OUT), ...listed(JS_OUT)]) {
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
