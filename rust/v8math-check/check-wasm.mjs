// Compares v8math built as WebAssembly with the V8 running this script:
// the module makes the inputs (the same as the native comparer makes from
// the same seed and chunk), V8 computes its results here with the
// JavaScript the game runs (oracle.mjs), the module computes its own, and
// the bit patterns are compared (two NaNs are equal, as in JavaScript;
// NaNs whose bits differ are counted apart). Prints a JSON line per
// function and exits with 1 when any result differs.
//
//   cargo build --release --target wasm32-unknown-unknown -p v8math-check --lib
//   node check-wasm.mjs [--special] [--seed S] [--chunks FIRST..END]
//                       [--chunk-size N] [--jobs J] FUNCTION... | all
//
// --jobs shares the chunks among that many worker threads. V8MATH_WASM
// names another build of the module.
import { readFileSync } from 'node:fs';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { functions } from './oracle.mjs';

const path = process.env.V8MATH_WASM ?? new URL('../target/wasm32-unknown-unknown/release/v8math_check.wasm', import.meta.url);
const w = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(path))).exports;
// (A call may grow the memory, detaching views made before it.)
const namesAt = w.names_ptr(), namesLen = w.names_len();
const names = new TextDecoder().decode(new Uint8Array(w.memory.buffer, namesAt, namesLen)).split(',');
const hex = (lo, hi) => '0x' + hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');

/** Checks one function on the special values (chunks null) or on random chunks [first, end). */
function check(name, seed, chunks, chunkSize) {
  const id = names.indexOf(name);
  const f = functions[name];
  if (id < 0 || !f) throw Error(`check-wasm.mjs: no function ${name}`);
  let tuples = 0, mismatches = 0, nanBits = 0, shown = 0;
  const batches = chunks ? Array.from({ length: chunks[1] - chunks[0] }, (_, k) => chunks[0] + k) : [null];
  for (const chunk of batches) {
    const n = w.prepare(id, seed, BigInt(chunk ?? 0), chunk === null ? 0 : chunkSize);
    const inputs = new Float64Array(w.memory.buffer, w.inputs_ptr(), n * f.arity);
    const v8 = new Float64Array(n * f.outs);
    f.run(inputs, v8, n);
    w.run(id);
    const ours = new Float64Array(w.memory.buffer, w.outputs_ptr(), n);
    const ourWords = new Uint32Array(w.memory.buffer, w.outputs_ptr(), 2 * n);
    const v8Words = new Uint32Array(v8.buffer);
    for (let t = 0; t < n; t++) {
      for (let o = 0; o < f.outs; o++) {
        const k = t * f.outs + o;
        if (ourWords[2 * t] === v8Words[2 * k] && ourWords[2 * t + 1] === v8Words[2 * k + 1]) continue;
        if (ours[t] !== ours[t] && v8[k] !== v8[k]) { nanBits++; continue; }
        mismatches++;
        if (shown++ < 20) {
          const inWords = new Uint32Array(w.memory.buffer, w.inputs_ptr(), 2 * n * f.arity);
          const xs = [];
          for (let a = 0; a < f.arity; a++) {
            const i = t * f.arity + a;
            xs.push(`${hex(inWords[2 * i], inWords[2 * i + 1])} (${inputs[i]})`);
          }
          console.error(`${name}[${o}](${xs.join(', ')}) = ${hex(v8Words[2 * k], v8Words[2 * k + 1])} (${v8[k]}) in V8, ${hex(ourWords[2 * t], ourWords[2 * t + 1])} (${ours[t]}) here`);
        }
      }
    }
    tuples += n;
  }
  return { tuples, mismatches, nanBits };
}

if (!isMainThread) {
  const { name, seed, chunks, chunkSize } = workerData;
  parentPort.postMessage(check(name, seed, chunks, chunkSize));
} else {
  let special = false, seed = 1n, chunks = [0, 1], chunkSize = 1 << 20, jobs = 1;
  const wanted = [];
  const args = process.argv.slice(2);
  while (args.length) {
    const a = args.shift();
    if (a === '--special') special = true;
    else if (a === '--seed') seed = BigInt(args.shift());
    else if (a === '--chunks') chunks = args.shift().split('..').map(Number);
    else if (a === '--chunk-size') chunkSize = Number(args.shift());
    else if (a === '--jobs') jobs = Number(args.shift());
    // all: what this V8 computes; pow_legacy needs Node 22 or --no-use-std-math-pow.
    else if (a === 'all') wanted.push(...names.filter((n) => functions[n] && !['glibc_pow', 'fma', 'pow_legacy'].includes(n)));
    else wanted.push(a);
  }
  let failed = false;
  for (const name of wanted) {
    const started = performance.now();
    let r;
    if (special) {
      r = check(name, seed, null, chunkSize);
    } else {
      const [a, b] = chunks;
      const j = Math.max(1, Math.min(jobs, b - a));
      const parts = Array.from({ length: j }, (_, k) => [a + Math.floor((b - a) * k / j), a + Math.floor((b - a) * (k + 1) / j)]);
      const results = await Promise.all(parts.map((part) => new Promise((resolve, reject) => {
        // (V8's flags are the process's: workers run under them too.)
        const worker = new Worker(new URL(import.meta.url), { workerData: { name, seed, chunks: part, chunkSize } });
        worker.once('message', resolve);
        worker.once('error', reject);
      })));
      r = results.reduce((s, x) => ({ tuples: s.tuples + x.tuples, mismatches: s.mismatches + x.mismatches, nanBits: s.nanBits + x.nanBits }));
    }
    failed ||= r.mismatches > 0;
    console.log(JSON.stringify({
      fn: name, build: 'wasm', inputs: special ? 'special' : 'random', seed: Number(seed), chunks,
      tuples: r.tuples, mismatches: r.mismatches, nan_bits_differ: r.nanBits, node: process.version,
      v8: process.versions.v8, flags: process.execArgv.join(' '),
      seconds: +((performance.now() - started) / 1000).toFixed(1),
    }));
  }
  process.exitCode = failed ? 1 : 0;
}
