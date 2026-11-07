import test from 'node:test';
import assert from 'node:assert/strict';
import { F4Counters, countedF4Exports } from '../src/f4-counters.js';

test('phase counters keep commands and lifecycle distinct without changing arguments', () => {
  const c = new F4Counters(), calls = [];
  const x = countedF4Exports({ f4_live() { calls.push('live'); }, f4_tail() { calls.push('tail'); },
    f4_live_command(a, b) { assert.equal(a, 7); assert.equal(b, 9); return 16; },
    f4_battle_command() {}, f4_roster_command() {}, f4_human_ready() {}, f4_init() {}, f4_viewpoint_clock() {}, f4_connection_command() {}, f4_unit_spawn() {}, memory: { buffer: new ArrayBuffer(8) } }, c);
  x.f4_live(); x.f4_tail(); assert.equal(x.f4_live_command(7, 9), 16); x.f4_battle_command(); x.f4_roster_command(); x.f4_human_ready(); x.f4_init();
  assert.deepEqual(calls, ['live', 'tail']);
  assert.equal(c.live, 1); assert.equal(c.tail, 1); assert.equal(c.commands, 4); assert.equal(c.lifecycle, 1);
  assert.equal(c.exportCalls, 7); assert.equal(c.argumentWords, 2);
  x.f4_viewpoint_clock();x.f4_connection_command();x.f4_unit_spawn();
  assert.equal(c.viewpoint,1);assert.equal(c.commands,6);assert.equal(c.lifecycle,1);
  const saved = c.snapshot(); x.f4_live(); assert.equal(saved.live, 1);
});

test('shared bridge counts real record copies and synchronous imports', async () => {
  const { loadSim } = await import('../src/sim/module.js');
  const { SimBridge } = await import('../src/sim/runtime.js');
  const { COMMAND_TABLE } = await import('../src/sim/gen/index.js');
  const bridge = new SimBridge(await loadSim());
  bridge.boundaryCounters = new F4Counters(); bridge.x = countedF4Exports(bridge.x, bridge.boundaryCounters);
  bridge.register({ ask(args) { return args; } });
  const args = { value: 7, at: { x: 1, y: 2, z: 3 }, count: 1, note: null, events: 0 };
  const command = COMMAND_TABLE.find(c => c?.name === 'relay'), words = command.args.words;
  // The public command table resolves the relay; no hand-numbered command id.
  const before = bridge.boundaryCounters.snapshot();
  const result = bridge.command('relay', args);
  assert.equal(result.value, 7);
  assert.equal(bridge.boundaryCounters.importCalls - before.importCalls, 1);
  // Every command and import result copies its generated block.
  assert.equal(bridge.boundaryCounters.inputWords - before.inputWords, words * 2);
  assert.equal(bridge.boundaryCounters.outputWords - before.outputWords, words * 2);
  assert.ok(bridge.boundaryCounters.exportCalls > before.exportCalls);
});
