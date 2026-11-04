// Saves the generated input blocks and independent JavaScript wire bytes
// for rust/sim/tests/wire_codec_twin.rs, including native/WASM comparison.
import { openSync, writeSync, closeSync } from 'node:fs';
import { loadSim } from '../src/sim/module.js';
import { createWireSim } from '../src/runtime/wire-sim.js';
import { wireCases } from '../tests/fixtures/wire-cases.js';
const file = process.argv[2];
if (!file) throw Error('usage: node tools/wire-twin.mjs OUTPUT');
const fd = openSync(file, 'w'); let cases = 0;
const u32 = n => { const b = Buffer.alloc(4); b.writeUInt32LE(n); writeSync(fd,b); };
const native = createWireSim(await loadSim(), { trace(op, bridge) {
  bridge.views(); const ptr=bridge.x.sim_calls(), f=bridge.f, at=ptr/8;
  const words=f[at+2]+f[at+3]; u32(op); u32(words);
  writeSync(fd, new Uint8Array(bridge.buffer,ptr,words*8));
} });
try { wireCases(native, { result(parts) {u32(parts.length); for (const part of parts) {u32(part.length); writeSync(fd,part);} cases++;} }); }
finally { closeSync(fd); }
console.log(`${cases} JS/WASM cases written for native byte comparison`);
