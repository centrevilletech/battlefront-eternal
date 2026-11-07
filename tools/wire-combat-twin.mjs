// Native combat oracle. The same adapter writes the generated record row,
// while the original JavaScript producers supply independent expected bytes.
import assert from 'node:assert/strict';
import { Heap } from '../src/sim/runtime.js';
// This record-only oracle captures the concrete adapter's generated inputs.
import { WireCombat } from '../src/sim/wasm/runtime/wire-combat.js';
import { combatCases, combatEncoders } from './wire-combat-fixtures.mjs';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let input, expected, failure;
const bridge = {
  x: { wire_combat_create: () => 1, wire_combat_words: () => 44, wire_combat_encode: () => 0, wire_combat_drop() {} },
  callArgs(record, value) { input = { record, value }; },
  callResult() { return { error: failure?.message ?? null, parts: expected ? [{ bytes: expected.payload }] : [] }; },
};
const native = new WireCombat({ bridge });
for (const { encoder, value, label } of combatCases()) {
  const encode = combatEncoders[encoder]; input = expected = failure = null;
  try { expected = encode(value); } catch (e) { failure = e; }
  if (failure) assert.throws(() => encode({ ...value, native }), e => e.message === failure.message, label);
  else assert.deepEqual(encode({ ...value, native }), expected, label);
  assert.ok(input, `${label}: no native input`);
  const heap = new Heap(), row = new Float64Array(input.record.words);
  input.record.put(bridge, input.value, row, 0, heap);
  process.stdout.write(JSON.stringify({ label, row: words(row), heap: words(heap.words),
    error: failure?.message ?? null, bytes: expected ? Buffer.from(expected.payload).toString('hex') : null }) + '\n');
}
native.dispose();
