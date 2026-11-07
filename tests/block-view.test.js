import test from 'node:test';
import assert from 'node:assert/strict';
import { WasmBlocks } from '../src/sim/block-view.js';
import { BlockBridge, SimBridge, SimContext } from '../src/sim/runtime.js';
import { COMMAND_TABLE } from '../src/sim/gen/index.js';
import { Heap } from '../src/sim/runtime.js';
import { BlockHeaderLayout as Header } from '../src/sim/gen/boundary.js';
import { putWireBaselineChoice, syncWireBaselineChoice, WireBaselineChoiceMirror } from '../src/sim/gen/wire-host.js';
import { loadSim } from '../src/sim/module.js';

// A copied block uses relative heap offsets. This exercises the generated
// decoder directly; string identities stay local to the configured context.
test('generated records decode a copied block without memory addresses', async () => {
  const bridge = new SimBridge(await loadSim());
  const echo = COMMAND_TABLE.find(record => record?.name === 'echo');
  const value = { value: -0, at: { x: 1, y: 2, z: 3 }, note: 'block', count: 2, events: 0 };
  // Use the real command's result, whose layout includes strings and lists.
  const record = echo.result;
  bridge.callArgs(echo.args, value);
  const pointer = bridge.x.sim_command(echo.code);
  const wasm = bridge.readBlock(pointer);
  const length = wasm.words[wasm.base + 2] + wasm.words[wasm.base + 3];
  const words = wasm.words.slice(wasm.base, wasm.base + length);
  const context = new SimContext();
  context.strings = bridge.strings;
  assert.deepEqual(record.sync(context, words, Header.words, words[Header.heap], new record.Mirror()), bridge.callResult(record, pointer));
});

test('WASM views refresh after growth and retain one BlockView', () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const blocks = new WasmBlocks(memory), first = blocks.readBlock(16);
  const generation = first.generation;
  assert.equal(first.base, 2);
  memory.grow(1);
  assert.equal(blocks.readBlock(24), first);
  assert.equal(first.base, 3);
  assert.equal(first.generation, generation + 1);
  assert.equal(first.words.byteLength, 131072);
});

// Nested lists exercise block-relative heap offsets, including an arena that
// begins inside an independently owned backing allocation.
test('generated nested heaps decode at memory and copied section bases', () => {
  const input = { tick: -0, max: NaN, acks: [{ ticks: [1, -0, NaN] }, { ticks: [7] }] };
  const row = new Float64Array(4), heap = new Heap();
  putWireBaselineChoice(null, input, row, 0, heap);
  const words = new Float64Array(Header.words + row.length + heap.words.length);
  words[Header.rows] = 1; words[Header.stride] = row.length;
  words[Header.heap] = Header.words + row.length; words[Header.heapWords] = heap.words.length;
  words.set(row, Header.words); words.set(heap.words, Header.words + row.length);
  const memory = new WebAssembly.Memory({ initial: 1 }), offset = 64;
  new Float64Array(memory.buffer).set(words, offset / 8);
  const section = new Float64Array(new ArrayBuffer(offset + words.byteLength), offset);
  section.set(words);
  const wasm = new BlockBridge(new WasmBlocks(memory));
  const context = new SimContext();
  const record = { sync: syncWireBaselineChoice, Mirror: WireBaselineChoiceMirror };
  const a = wasm.callResult(record, offset);
  const b = record.sync(context, section, Header.words, section[Header.heap], new record.Mirror());
  assert.deepEqual(a, b);
  assert.deepEqual({ ...a, acks: a.acks.map(row => ({ ...row })) }, input);
});

// Both refresh routes must update the retained views seen by phase adapters.
test('bridge adapters and block readers share one memory view bank', async () => {
  const bridge = new SimBridge(await loadSim());
  const before = bridge.grown;
  const first = bridge.f;
  bridge.memory.grow(1);
  const block = bridge.readBlock(0);
  assert.equal(first.byteLength, 0);
  assert.equal(bridge.f, block.words);
  assert.equal(bridge.u8.buffer, block.words.buffer);
  assert.equal(bridge.buffer, block.words.buffer);
  assert.equal(bridge.grown, before + 1);
  bridge.memory.grow(1);
  bridge.views();
  assert.equal(bridge.f, block.words);
  assert.equal(bridge.u8.buffer, bridge.buffer);
  assert.equal(bridge.grown, before + 2);
});
