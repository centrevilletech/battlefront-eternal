// Synthetic cosmetic events only. The fixture compares every encoded byte,
// priority, metadata record and ordered queue state with the JS originals
// before this corpus is accepted for native replay.
import { openSync, writeSync, closeSync } from 'node:fs';
import { loadSim } from '../src/sim/module.js';
import { createWireCosmetics } from '../src/runtime/wire-cosmetics.js';
import { cosmeticCodecCases, cosmeticOutboxCases } from '../tests/fixtures/cosmetic-cases.js';
const path = process.argv[2];
if (!path) throw Error('usage: node tools/cosmetics-twin.mjs OUTPUT');
const fd = openSync(path, 'w'); let count = 0;
const u32 = n => {const b=Buffer.alloc(4);b.writeUInt32LE(n);writeSync(fd,b);};
const bytes = b => {u32(b.length);writeSync(fd,b);};
const floats = values => {u32(values.length);const b=Buffer.alloc(values.length*8);for(let i=0;i<values.length;i++)b.writeDoubleLE(values[i],i*8);writeSync(fd,b);};
u32(0x31434642);
const native = createWireCosmetics(await loadSim(), { trace({session,op,words,payload,result,error}) {
  u32(session);u32(op);floats(words);bytes(payload);bytes(Buffer.from(error??''));floats(result.words);bytes(result.bytes);count++;
} });
try {
  const records = cosmeticCodecCases(native), operations = cosmeticOutboxCases(native);
  console.log(`${records} cosmetic records, ${operations} outbox operations, ${count} JS-verified cases for native replay`);
} finally {native.dispose();closeSync(fd);}
