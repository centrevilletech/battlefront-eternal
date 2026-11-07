import test from 'node:test';
import assert from 'node:assert/strict';
import { WasmBlocks, NativeBlocks } from '../src/sim/block-view.js';
import { BlockBridge, SimBridge } from '../src/sim/runtime.js';
import { COMMAND_TABLE } from '../src/sim/gen/index.js';
import { Heap } from '../src/sim/runtime.js';
import { BlockHeaderLayout as Header } from '../src/sim/gen/boundary.js';
import { putWireBaselineChoice, syncWireBaselineChoice, WireBaselineChoiceMirror } from '../src/sim/gen/wire-host.js';
import { loadSim } from '../src/sim/module.js';

// The generated decoder receives the same block-relative offsets from either
// backend. String identities stay local to the configured bridge.
test('WASM and native blocks share generated record decoding', async () => {
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
  const blocks = new NativeBlocks();
  blocks.accept(words);
  const context = new BlockBridge(blocks);
  context.strings = bridge.strings;
  assert.deepEqual(context.callResult(record, null), bridge.callResult(record, pointer));
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

test('native arena reuse and invalid extent rejection', () => {
  const blocks = new NativeBlocks(), a = new Float64Array([1, 1, 6, 1, 0, -0, NaN]);
  const view = blocks.accept(a);
  for (let i = 0; i < 10000; i++) assert.equal(blocks.readBlock(), view);
  assert.equal(blocks.accept(a), view);
  for (const bad of [[], [1, 1, 6, 0, 2, 0], [Infinity, 0, 5, 0, 0], [1, 1, 5, 0, 0], [0, 0, 5, 100, 0]]) {
    assert.throws(() => blocks.accept(new Float64Array(bad)), /block/i);
  }
  assert.equal(blocks.readBlock(), view);
  assert.equal(view.words, a);
});

// Nested lists exercise block-relative heap offsets, including an arena that
// begins inside an independently owned backing allocation.
test('generated nested heaps decode at WASM and native section bases', () => {
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
  const nativeBlocks = new NativeBlocks(); nativeBlocks.accept(section);
  const wasm = new BlockBridge(new WasmBlocks(memory)), native = new BlockBridge(nativeBlocks);
  const record = { sync: syncWireBaselineChoice, Mirror: WireBaselineChoiceMirror };
  const a = wasm.callResult(record, offset), b = native.callResult(record, null);
  assert.deepEqual(a, b);
  assert.deepEqual({ ...a, acks: a.acks.map(row => ({ ...row })) }, input);
});
