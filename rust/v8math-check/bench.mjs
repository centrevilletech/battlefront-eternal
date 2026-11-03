// Times v8math as WebAssembly and V8's own Math on the same inputs of the
// size the game uses (lib.rs, typical): nanoseconds a call, the best of
// several rounds over a million inputs, the loop and its loads and stores
// included. `v8math-check --bench` gives the native build's.
//
//   node bench.mjs [FUNCTION...]
import { readFileSync } from 'node:fs';
import { functions } from './oracle.mjs';

const path = process.env.V8MATH_WASM ?? new URL('../target/wasm32-unknown-unknown/release/v8math_check.wasm', import.meta.url);
const w = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(path))).exports;
const namesAt = w.names_ptr(), namesLen = w.names_len();
const names = new TextDecoder().decode(new Uint8Array(w.memory.buffer, namesAt, namesLen)).split(',');
const wanted = process.argv.length > 2 ? process.argv.slice(2) : names;
const n = 1 << 20;

function best(f, rounds = 7) {
  let b = Infinity;
  for (let r = 0; r < rounds; r++) {
    const t = performance.now();
    f();
    b = Math.min(b, performance.now() - t);
  }
  return +(b * 1e6 / n).toFixed(2);
}

for (const name of wanted) {
  const id = names.indexOf(name);
  const f = functions[name];
  if (id < 0) throw Error(`bench.mjs: no function ${name}`);
  w.prepare_typical(id, n);
  const arity = f?.arity ?? 3;
  const inputs = new Float64Array(w.memory.buffer, w.inputs_ptr(), n * arity).slice();
  const row = { fn: name, node: process.version, wasm_ns: best(() => w.run(id)) };
  // V8's own, where JavaScript has the function (pow_legacy is V8's only with --no-use-std-math-pow)
  if (f && name !== 'fma' && name !== 'glibc_pow') {
    const out = new Float64Array(n * f.outs);
    row.v8_ns = best(() => f.run(inputs, out, n));
    if (f.outs === 2) row.v8_ns_note = 'Math.pow and ** each: half of this per call';
  }
  console.log(JSON.stringify(row));
}
