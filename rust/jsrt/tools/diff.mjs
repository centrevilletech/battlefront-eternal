// Differential test: rust/jsrt against this Node's JavaScript.
//
// Run with the production server's Node (24.22, V8 13.6) - that is
// the math reference. Build the probe first:
//   cargo build -p jsrt --release --bin jsrt-probe
// then: node tools/diff.mjs [scale]
// scale multiplies the case counts (default 1). Every check must be
// bit-identical (Object.is, with NaNs alike); any mismatch is
// printed with its input and counted. Exit status is nonzero if
// any exact check fails. The one recorded exception is the
// Float64Array default sort, whose V8 path is a C++ std::sort
// (unstable for equal values, notably +0/-0): it is reported, not
// gated - see the notes in rust/jsrt/src/sort.rs.

import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname, join} from 'node:path';
import {ClassicRandom, RandomStreams} from '../../../src/ai/random.js';
import {spreadSeed, seededRandom} from '../../../src/weapons.js';

const here = dirname(fileURLToPath(import.meta.url));
const probe = process.env.JSRT_PROBE || join(here, '..', '..', 'target', 'release', 'jsrt-probe');
const scale = Number(process.argv[2] || 1);

// mulberry32, seeded: the harness's own case generator.
function generator(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rnd = generator(0x5eed1234);
const u32 = () => Math.floor(rnd() * 4294967296);

const bitsBuf = new ArrayBuffer(8);
const asF = new Float64Array(bitsBuf);
const asU = new Uint32Array(bitsBuf);
function fromBits(hi, lo) { asU[0] = lo; asU[1] = hi; return asF[0]; }
function bitsOf(x) { asF[0] = x; return [asU[1], asU[0]]; }
function sameValue(a, b) {
  if (Number.isNaN(a) && Number.isNaN(b)) return true;
  const [ah, al] = bitsOf(a), [bh, bl] = bitsOf(b);
  return ah === bh && al === bl;
}

// A case stream mixing the shapes JS semantics care about.
function numbers(n) {
  const out = new Float64Array(n);
  const specials = [0, -0, 1, -1, 0.5, -0.5, 1.5, -1.5, 2.5, -2.5,
    0.49999999999999994, -0.49999999999999994, 2147483647, 2147483648,
    -2147483648, -2147483649, 4294967295, 4294967296, -4294967296,
    9007199254740991, 9007199254740992, 9007199254740994, 1e21, -1e21,
    1e-7, 5e-324, -5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
    Infinity, -Infinity, NaN, 8388608, 0.1, 1 / 3, Math.PI, 2 ** 31, -(2 ** 31),
    2 ** 32, 2 ** 52, 2 ** 53, -(2 ** 53), 123456789.123456789, 1e15 + 0.5];
  for (let i = 0; i < n; i++) {
    const pick = rnd();
    if (pick < 0.08) out[i] = specials[Math.floor(rnd() * specials.length)];
    else if (pick < 0.38) out[i] = fromBits(u32(), u32()); // any bit pattern
    else if (pick < 0.58) out[i] = (rnd() * 2 - 1) * 10 ** (Math.floor(rnd() * 24) - 12);
    else if (pick < 0.78) out[i] = Math.floor(rnd() * 200000) - 100000 + (rnd() < 0.5 ? 0.5 : 0);
    else if (pick < 0.9) out[i] = (u32() - 2147483648) / (1 + Math.floor(rnd() * 1000));
    else out[i] = (rnd() * 2 - 1) * 10 ** (Math.floor(rnd() * 640) - 320);
  }
  return out;
}

function runProbe(args, input) {
  const r = spawnSync(probe, args, {input, maxBuffer: 1 << 30});
  if (r.status !== 0) throw new Error(`probe ${args.join(' ')} failed: ${r.stderr}`);
  return r.stdout;
}
function probeF64(args, inputF64) {
  const out = runProbe(args, Buffer.from(inputF64.buffer, inputF64.byteOffset, inputF64.byteLength));
  return new Float64Array(out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength));
}

let totalBad = 0;
let totalCases = 0;
function report(name, bad, cases, firstBad) {
  totalCases += cases;
  totalBad += bad;
  console.log(`${name.padEnd(18)} ${String(cases).padStart(12)} cases  ${bad} mismatches${bad ? `  first: ${firstBad}` : ''}`);
}
function compareF64(name, got, want, inputs) {
  let bad = 0, first = '';
  for (let i = 0; i < want.length; i++) {
    if (!sameValue(got[i], want[i])) {
      bad++;
      if (!first) first = `in=${inputs ? inputs[i] : '?'} got=${got[i]} want=${want[i]}`;
    }
  }
  report(name, bad, want.length, first);
}

const N = Math.floor(2_000_000 * scale);
const xs = numbers(N);
for (const [mode, js] of [
  ['to-int32', x => x | 0], ['to-uint32', x => x >>> 0], ['round', Math.round],
  ['floor', Math.floor], ['ceil', Math.ceil], ['trunc', Math.trunc],
  ['sign', Math.sign], ['abs', Math.abs], ['fround', Math.fround],
  ['clz32', Math.clz32], ['to-integer', x => { const n = Number(x); return Number.isNaN(n) ? 0 : Math.trunc(n); }],
  ['uint8-clamp', x => new Uint8ClampedArray([x])[0]],
]) {
  compareF64(mode, probeF64([mode], xs), xs.map(js), xs);
}

const pairs = new Float64Array(N * 2);
{ const a = numbers(N), b = numbers(N); for (let i = 0; i < N; i++) { pairs[2 * i] = a[i]; pairs[2 * i + 1] = b[i]; } }
for (const [mode, js] of [
  ['min', Math.min], ['max', Math.max], ['imul', Math.imul],
  ['mod', (a, b) => a % b], ['hypot2', Math.hypot],
]) {
  const want = new Float64Array(N);
  for (let i = 0; i < N; i++) want[i] = js(pairs[2 * i], pairs[2 * i + 1]);
  compareF64(mode, probeF64([mode], pairs), want, null);
}
{
  const triples = new Float64Array(N * 3);
  { const a = numbers(N), b = numbers(N), c = numbers(N); for (let i = 0; i < N; i++) { triples[3 * i] = a[i]; triples[3 * i + 1] = b[i]; triples[3 * i + 2] = c[i]; } }
  const want = new Float64Array(N);
  for (let i = 0; i < N; i++) want[i] = Math.hypot(triples[3 * i], triples[3 * i + 1], triples[3 * i + 2]);
  compareF64('hypot3', probeF64(['hypot3'], triples), want, null);
  const quads = new Float64Array(N * 4);
  { const a = numbers(N); quads.set(triples.subarray(0, N * 3).filter((_, i) => true), 0); }
  for (let i = 0; i < N; i++) { quads[4 * i] = triples[3 * i]; quads[4 * i + 1] = triples[3 * i + 1]; quads[4 * i + 2] = triples[3 * i + 2]; quads[4 * i + 3] = numbers(1)[0]; }
  for (let i = 0; i < N; i++) want[i] = Math.hypot(quads[4 * i], quads[4 * i + 1], quads[4 * i + 2], quads[4 * i + 3]);
  compareF64('hypot4', probeF64(['hypot4'], quads), want, null);
}

// Number text.
{
  const input = numbers(Math.floor(500_000 * scale));
  const got = runProbe(['numstr'], Buffer.from(input.buffer)).toString().trimEnd().split('\n');
  let bad = 0, first = '';
  for (let i = 0; i < input.length; i++) {
    const want = String(input[i]);
    if (got[i] !== want) { bad++; if (!first) first = `x=${input[i]} got=${got[i]} want=${want}`; }
  }
  report('numstr', bad, input.length, first);
  const gotJ = runProbe(['jsonnum'], Buffer.from(input.buffer)).toString().trimEnd().split('\n');
  bad = 0; first = '';
  for (let i = 0; i < input.length; i++) {
    const want = JSON.stringify(input[i]);
    if (gotJ[i] !== want) { bad++; if (!first) first = `x=${input[i]} got=${gotJ[i]} want=${want}`; }
  }
  report('jsonnum', bad, input.length, first);
  const xsf = numbers(Math.floor(200_000 * scale));
  const pairIn = new Float64Array(xsf.length * 2);
  for (let i = 0; i < xsf.length; i++) { pairIn[2 * i] = xsf[i]; pairIn[2 * i + 1] = Math.floor(rnd() * 22) - 1; }
  const gotF = runProbe(['tofixed'], Buffer.from(pairIn.buffer)).toString().trimEnd().split('\n');
  bad = 0; first = '';
  for (let i = 0; i < xsf.length; i++) {
    let want;
    try { want = xsf[i].toFixed(pairIn[2 * i + 1]); } catch { want = 'RangeError'; }
    if (gotF[i] !== want) { bad++; if (!first) first = `x=${xsf[i]} f=${pairIn[2 * i + 1]} got=${gotF[i]} want=${want}`; }
  }
  report('tofixed', bad, xsf.length, first);
}

// The game's generators, against the game's own classes.
for (const seed of [0, 1, 12345, 0xffffffff, 0x80000000]) {
  const count = Math.floor(100_000 * scale);
  const r = new ClassicRandom(seed);
  const want = new Float64Array(count * 6);
  for (let i = 0; i < count; i++) {
    want[6 * i] = r.next(); want[6 * i + 1] = r.uniform();
    want[6 * i + 2] = r.int(-3, 7); want[6 * i + 3] = r.range(-2.5, 9.25);
    want[6 * i + 4] = r.mod(97); want[6 * i + 5] = r.seed;
  }
  compareF64(`classic:${seed}`, probeF64(['classic', String(seed), String(count)], new Float64Array(0)), want, null);
}
for (const seed of [0, 77, 4294967295]) {
  const count = Math.floor(50_000 * scale);
  const names = ['gameplay', 'player-weapons', 'network-recon:7', 'ai', ''];
  const streams = new RandomStreams(seed);
  const want = new Float64Array(count * 2);
  for (let i = 0; i < count; i++) {
    const s = streams.get(names[i % names.length]);
    want[2 * i] = s.uniform(); want[2 * i + 1] = s.seed;
  }
  compareF64(`streams:${seed}`, probeF64(['streams', String(seed), String(count)], new Float64Array(0)), want, null);
}
{
  const cases = numbers(Math.floor(50_000 * scale) * 4);
  const want = [];
  for (let i = 0; i < cases.length; i += 4) {
    const unit = {id: cases[i], generation: cases[i + 1]};
    const seed = spreadSeed(unit, cases[i + 2], cases[i + 3]);
    want.push(seed);
    const m = seededRandom(seed);
    for (let k = 0; k < 8; k++) want.push(m());
  }
  compareF64('spread+mulberry', probeF64(['spread'], cases), Float64Array.from(want), null);
  const seeds = numbers(Math.floor(50_000 * scale));
  const wantM = [];
  for (const s of seeds) { const m = seededRandom(s); for (let k = 0; k < 8; k++) wantM.push(m()); }
  compareF64('mulberry', probeF64(['mulberry'], seeds), Float64Array.from(wantM), null);
}

// Sorts.
function arraysOf(count, maxLen, gen) {
  const arrays = [];
  for (let i = 0; i < count; i++) {
    const len = i < 130 ? i : Math.floor(rnd() * maxLen);
    arrays.push(Array.from({length: len}, gen));
  }
  return arrays;
}
const cmpKinds = [
  (a, b) => a.key - b.key,
  () => 1,
  () => -1,
  (a, b) => (a.key % 3) - (b.key % 3),
  null, // stateful, built per array
];
function statefulCmp() {
  let calls = 0;
  return () => { calls++; return [-1, 1, 0][(calls - 1) % 3]; };
}
{
  const keyPool = numbers(4096);
  const keyOf = () => {
    const p = rnd();
    if (p < 0.5) return Math.floor(rnd() * 12) - 4;         // heavy ties
    if (p < 0.7) return keyPool[Math.floor(rnd() * keyPool.length)];
    if (p < 0.8) return [NaN, Infinity, -Infinity, -0, 0][Math.floor(rnd() * 5)];
    return rnd() * 200 - 100;
  };
  const arrays = arraysOf(Math.floor(4000 * scale), 300, () => keyOf());
  for (let kind = 0; kind < cmpKinds.length; kind++) {
    const buf = [arrays.length];
    const want = [];
    for (const keys of arrays) {
      buf.push(keys.length, ...keys, ...keys.map((_, i) => i));
      const pairs = keys.map((key, tag) => ({key, tag}));
      pairs.sort(kind === 4 ? statefulCmp() : cmpKinds[kind]);
      want.push(...pairs.map(p => p.tag));
    }
    compareF64(`sortpairs:${kind}`, probeF64(['sortpairs', String(kind)], Float64Array.from(buf)), Float64Array.from(want), null);
  }
  // Default comparator and the typed paths over plain values.
  const bufD = [arrays.length];
  const wantD = [], wantT = [];
  for (const keys of arrays) {
    bufD.push(keys.length, ...keys);
    wantD.push(...[...keys].sort());
    wantT.push(...Float64Array.from(keys).sort());
  }
  compareF64('sortdefault', probeF64(['sortdefault'], Float64Array.from(bufD)), Float64Array.from(wantD), null);
  const gotT = probeF64(['typeddefault'], Float64Array.from(bufD));
  let badT = 0, firstT = '';
  for (let i = 0; i < wantT.length; i++) {
    if (!sameValue(gotT[i], wantT[i])) { badT++; if (!firstT) firstT = `got=${gotT[i]} want=${wantT[i]} at ${i}`; }
  }
  console.log(`${'typeddefault*'.padEnd(18)} ${String(wantT.length).padStart(12)} cases  ${badT} mismatches (V8 std::sort, reported not gated)${badT ? `  first: ${firstT}` : ''}`);
  totalCases += wantT.length;
  for (let kind = 0; kind < cmpKinds.length; kind++) {
    const want = [];
    for (const keys of arrays) {
      const a = Float64Array.from(keys);
      const cmp = kind === 4 ? statefulCmp() : (x, y) => cmpKinds[kind]({key: x}, {key: y});
      a.sort(cmp);
      want.push(...a);
    }
    compareF64(`typedcmp:${kind}`, probeF64(['typedcmp', String(kind)], Float64Array.from(bufD)), Float64Array.from(want), null);
  }
}
{
  // Strings by code units, including lone surrogates and astral pairs.
  const unit = () => {
    const p = rnd();
    if (p < 0.55) return 0x61 + Math.floor(rnd() * 4);
    if (p < 0.7) return 0x30 + Math.floor(rnd() * 12);
    if (p < 0.8) return 0xd800 + Math.floor(rnd() * 0x800);
    if (p < 0.9) return Math.floor(rnd() * 0x10000);
    return [0, 0x5f, 0xe000, 0xffff][Math.floor(rnd() * 4)];
  };
  const arrays = arraysOf(Math.floor(2000 * scale), 60, () => {
    const n = Math.floor(rnd() * 9);
    return Array.from({length: n}, unit);
  });
  const bytes = [];
  const push32 = v => { const b = Buffer.alloc(4); b.writeUInt32LE(v); bytes.push(b); };
  push32(arrays.length);
  for (const arr of arrays) {
    push32(arr.length);
    for (const s of arr) { push32(s.length); const b = Buffer.alloc(s.length * 2); s.forEach((u, i) => b.writeUInt16LE(u, i * 2)); bytes.push(b); }
  }
  const out = runProbe(['sortstrings'], Buffer.concat(bytes));
  let pos = 0, bad = 0, cases = 0, first = '';
  const read32 = () => { const v = out.readUInt32LE(pos); pos += 4; return v; };
  for (const arr of arrays) {
    const want = arr.map(u => String.fromCharCode(...u)).sort().map(s => [...s].map(ch => ch.charCodeAt(0)).join(','));
    // String sort in JS sorts the strings; per-string code units:
    const wantUnits = arr.map(u => String.fromCharCode(...u)).sort().map(s => Array.from({length: s.length}, (_, i) => s.charCodeAt(i)).join(','));
    void want;
    for (const w of wantUnits) {
      const n = read32();
      const got = [];
      for (let i = 0; i < n; i++) { got.push(out.readUInt16LE(pos)); pos += 2; }
      cases++;
      if (got.join(',') !== w) { bad++; if (!first) first = `got=[${got}] want=[${w}]`; }
    }
  }
  report('sortstrings', bad, cases, first);
}
{
  // Map/Set op traces.
  const ops = [];
  const keys = Array.from({length: 48}, (_, i) => `k${i}`);
  for (let i = 0; i < 6000 * scale; i++) {
    const k = keys[Math.floor(rnd() * keys.length)];
    const p = rnd();
    if (p < 0.42) ops.push(`S ${k} ${Math.floor(rnd() * 100)}`);
    else if (p < 0.62) ops.push(`D ${k}`);
    else if (p < 0.7) ops.push(`H ${k}`);
    else if (p < 0.78) ops.push(`G ${k}`);
    else if (p < 0.84) ops.push('K');
    else if (p < 0.9) ops.push('E');
    else if (p < 0.94) ops.push(`M ${k}`);
    else if (p < 0.98) ops.push(`R ${k}`);
    else ops.push('C');
  }
  const text = ops.join('\n') + '\n';
  const got = runProbe(['mapops'], Buffer.from(text)).toString();
  const map = new Map(), set = new Set();
  let want = '';
  for (const line of ops) {
    const [op, k, v] = line.split(' ');
    if (op === 'S') { map.set(k, Number(v)); set.add(k); }
    else if (op === 'D') want += `del ${map.delete(k)} ${set.delete(k)}\n`;
    else if (op === 'C') { map.clear(); set.clear(); }
    else if (op === 'H') want += `has ${map.has(k)} ${set.has(k)}\n`;
    else if (op === 'G') want += `get ${map.get(k)}\n`;
    else if (op === 'K') { want += `keys ${[...map.keys()].join(',')}\nvals ${[...set.values()].join(',')}\n`; }
    else if (op === 'E') want += `entries ${[...map.entries()].map(([a, b]) => `${a}=${b}`).join(',')}\n`;
    else if (op === 'M') {
      const seen = []; let done = false;
      for (const key of map.keys()) { seen.push(key); if (!done) { done = true; map.delete(k); } }
      want += `iterdel ${seen.join(',')}\n`;
    } else if (op === 'R') {
      const seen = []; let done = false; const value = map.get(k);
      for (const key of map.keys()) {
        seen.push(key);
        if (!done) { done = true; if (value !== undefined) { map.delete(k); map.set(k, value); } }
      }
      want += `iterre ${seen.join(',')}\n`;
    }
  }
  const bad = got === want ? 0 : 1;
  let first = '';
  if (bad) {
    const g = got.split('\n'), w = want.split('\n');
    for (let i = 0; i < Math.max(g.length, w.length); i++) if (g[i] !== w[i]) { first = `line ${i}: got '${g[i]}' want '${w[i]}'`; break; }
  }
  report('mapops', bad, ops.length, first);
  // Object key order through a real object (same rule the Rust
  // unit tests pin), spot-checked against V8 itself.
  {
    let ok = 0;
    for (let t = 0; t < 1000; t++) {
      const o = {};
      const inserted = [];
      for (let i = 0; i < 24; i++) {
        const k = rnd() < 0.5 ? String(Math.floor(rnd() * 5000000000)) : `s${Math.floor(rnd() * 40)}`;
        if (!(k in o)) inserted.push(k);
        o[k] = i;
      }
      const wantKeys = Object.keys(o);
      const isIdx = k => { const u = Number(k) >>> 0; return String(u) === k && u !== 4294967295; };
      const mine = [...inserted.filter(isIdx).sort((a, b) => (Number(a) >>> 0) - (Number(b) >>> 0)), ...inserted.filter(k => !isIdx(k))];
      if (JSON.stringify(mine) === JSON.stringify(wantKeys)) ok++;
    }
    console.log(`(object-key rule self-check in JS: ${ok}/1000 - the Rust side is unit-tested)`);
  }
}

console.log(`TOTAL ${totalCases} cases, ${totalBad} mismatches (typeddefault excluded from the gate)`);
process.exit(totalBad === 0 ? 0 : 1);
