// V8's side of v8math-check (src/main.rs): reads requests from stdin, each
// a little-endian u32 count and that many input tuples of doubles, and
// answers each with V8's results, computed by the JavaScript the game runs.
// pow answers twice per tuple: Math.pow(x, y), then x ** y; so does
// pow_legacy, for a V8 that computes pow with its own fdlibm (Node 22, or
// Node 24 with --no-use-std-math-pow).
//
//   node oracle.mjs FUNCTION
import { readFileSync, readSync, writeSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// One function per case, so each loop is compiled for its own Math call.
const unary = {
  sin: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.sin(i[k]); },
  cos: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.cos(i[k]); },
  tan: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.tan(i[k]); },
  asin: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.asin(i[k]); },
  acos: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.acos(i[k]); },
  atan: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.atan(i[k]); },
  sinh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.sinh(i[k]); },
  cosh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.cosh(i[k]); },
  tanh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.tanh(i[k]); },
  asinh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.asinh(i[k]); },
  acosh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.acosh(i[k]); },
  atanh: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.atanh(i[k]); },
  exp: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.exp(i[k]); },
  expm1: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.expm1(i[k]); },
  log: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.log(i[k]); },
  log1p: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.log1p(i[k]); },
  log2: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.log2(i[k]); },
  log10: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.log10(i[k]); },
  cbrt: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.cbrt(i[k]); },
  sqrt: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.sqrt(i[k]); },
  abs: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.abs(i[k]); },
  floor: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.floor(i[k]); },
  ceil: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.ceil(i[k]); },
  trunc: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.trunc(i[k]); },
  round: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.round(i[k]); },
  fround: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.fround(i[k]); },
  sign: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.sign(i[k]); },
  clz32: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.clz32(i[k]); },
  to_int32: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = i[k] | 0; },
  to_uint32: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = i[k] >>> 0; },
};
const binary = {
  atan2: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.atan2(i[2 * k], i[2 * k + 1]); },
  hypot: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.hypot(i[2 * k], i[2 * k + 1]); },
  min: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.min(i[2 * k], i[2 * k + 1]); },
  max: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.max(i[2 * k], i[2 * k + 1]); },
  imul: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.imul(i[2 * k], i[2 * k + 1]); },
  pow_legacy: (i, o, n) => {
    for (let k = 0; k < n; k++) {
      const x = i[2 * k], y = i[2 * k + 1];
      o[2 * k] = Math.pow(x, y);
      o[2 * k + 1] = x ** y;
    }
  },
  pow: (i, o, n) => {
    for (let k = 0; k < n; k++) {
      const x = i[2 * k], y = i[2 * k + 1];
      o[2 * k] = Math.pow(x, y);
      o[2 * k + 1] = x ** y;
    }
  },
};
const ternary = {
  hypot3: (i, o, n) => { for (let k = 0; k < n; k++) o[k] = Math.hypot(i[3 * k], i[3 * k + 1], i[3 * k + 2]); },
};

// fma has no JavaScript: its oracle is the WebAssembly build of this crate
// (V8MATH_WASM, or where cargo writes it), so the native comparer checks
// that the module's software fused multiply-add rounds as the hardware's.
let wasm;
function wasmFma(i, o, n) {
  if (!wasm) {
    const path = process.env.V8MATH_WASM ?? new URL('../target/wasm32-unknown-unknown/release/v8math_check.wasm', import.meta.url);
    wasm = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(path))).exports;
  }
  // (A call may grow the memory, detaching views made before it.)
  const namesAt = wasm.names_ptr(), namesLen = wasm.names_len();
  const id = new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, namesAt, namesLen)).split(',').indexOf('fma');
  const at = wasm.load(id, n);
  new Float64Array(wasm.memory.buffer, at, 3 * n).set(i.subarray(0, 3 * n));
  wasm.run(id);
  o.set(new Float64Array(wasm.memory.buffer, wasm.outputs_ptr(), n));
}
ternary.fma = wasmFma;

/** Every function: its arity, results per tuple, and the loop computing them. */
export const functions = {};
for (const [table, arity] of [[unary, 1], [binary, 2], [ternary, 3]]) {
  for (const [name, run] of Object.entries(table)) {
    functions[name] = { arity, outs: name === 'pow' || name === 'pow_legacy' ? 2 : 1, run };
  }
}

function readFull(bytes) {
  let done = 0;
  while (done < bytes.length) {
    const got = readSync(0, bytes, done, bytes.length - done, null);
    if (got === 0) return done;
    done += got;
  }
  return done;
}

function writeFull(bytes) {
  let done = 0;
  while (done < bytes.length) done += writeSync(1, bytes, done, bytes.length - done);
}

/** Answers the native comparer's requests on stdin until it closes. */
function serve(name) {
  const f = functions[name];
  if (!f) throw Error(`oracle.mjs: no function ${name}`);
  const { arity, outs, run } = f;
  const header = new Uint8Array(4);
  let input = new ArrayBuffer(0);
  let output = new ArrayBuffer(0);
  for (;;) {
    const got = readFull(header);
    if (got === 0) break;
    if (got !== 4) throw Error('oracle.mjs: truncated request');
    const n = new DataView(header.buffer).getUint32(0, true);
    if (input.byteLength < n * arity * 8) input = new ArrayBuffer(n * arity * 8);
    if (output.byteLength < n * outs * 8) output = new ArrayBuffer(n * outs * 8);
    const inBytes = new Uint8Array(input, 0, n * arity * 8);
    if (readFull(inBytes) !== inBytes.length) throw Error('oracle.mjs: truncated request');
    run(new Float64Array(input, 0, n * arity), new Float64Array(output, 0, n * outs), n);
    writeFull(new Uint8Array(output, 0, n * outs * 8));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) serve(process.argv[2]);
