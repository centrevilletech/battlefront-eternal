import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { Writer, readRecords, MAGIC, RECORD, Handle, Omitted, FunctionName } from '../tools/sim-capture/format.mjs';
import { EDITS, edit } from '../tools/sim-capture/hooks.mjs';
import { PHASES, LIVE_PHASES } from '../tools/sim-capture/phases.mjs';

// The simulation capture (tools/sim-capture.mjs) records a battle's phases
// for the Rust replays (rust/sim-replay): its format must give back every
// value exactly, and its markers must still find the statements they wrap.
// No game files are needed.
const root = fileURLToPath(new URL('../', import.meta.url));

test('boundary tuples preserve old afterstate and optionally record entry state before mutation', () => {
  const script = `
    import assert from 'node:assert/strict';
    import fs from 'node:fs';
    import os from 'node:os';
    import path from 'node:path';
    import * as THREE from 'three';
    import { SPECS } from './tools/sim-capture/specs.mjs';
    import { readRecords } from './tools/sim-capture/format.mjs';
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'capture-boundary-'));
    try {
      const file = path.join(dir, 'fixture.bfecap'), point = new THREE.Vector3(1, 2, 3);
      let entries = 0;
      const world = { old(delta) { point.x += delta; return point.x; }, fresh(delta) { this.geometry(); point.x += delta; return point.x; }, geometry() { return point.length(); } };
      const fixture = { step(delta) { return world.old(delta) + world.fresh(delta); } };
      SPECS.boundaryFixture = { frames: [{ method: 'fixture.step', target: () => fixture, state: () => ({ point }), boundaries: () => [
        ['old', world, 'old', () => ({ point })],
        ['fresh', world, 'fresh', (_game, _cap, args, receiver) => { assert.equal(receiver, world); return { point, delta: args[0] }; },
          (_game, _cap, args, receiver) => { entries++; assert.equal(receiver, world); return { point, delta: args[0] }; }],
        ['geometry', world, 'geometry', null, () => { throw Error('nested geometry must belong to its outer boundary'); }],
      ] }] };
      process.argv = ['node', 'capture-test', '--capture-out', file, '--capture-specs', 'boundaryFixture', '--capture-every', '1'];
      const capture = await import('./tools/sim-capture/plugin.mjs');
      capture.battle({ game: { units: { all: () => [] }, collision: { bodies: [] }, ordnance: { explosion() {} }, items: { explode() {} } }, index: 0, options: {} });
      capture.tick(1); assert.equal(fixture.step(2), 8); capture.ticked(1, 'fixture'); capture.finish();
      const records = [...readRecords(new Uint8Array(fs.readFileSync(file)))].map(r => r.value);
      const oldCall = records.find(r => r.k === 'call' && r.n === 'old'), freshCall = records.find(r => r.k === 'call' && r.n === 'fresh');
      const oldRet = records.find(r => r.k === 'ret' && r.n === 'old'), freshRet = records.find(r => r.k === 'ret' && r.n === 'fresh');
      assert.equal(Object.hasOwn(oldCall, 's'), false); assert.equal(oldRet.x.point.x, 3);
      assert.equal(freshCall.s.point.x, 3); assert.equal(freshCall.s.delta, 2); assert.equal(freshRet.x.point.x, 5);
      assert.equal(freshRet.x.delta, 2); assert.equal(records.some(r => r.n === 'geometry'), false);
      world.fresh(1); assert.equal(entries, 1, 'no entry snapshot outside a sampled frame');
    } finally { fs.rmSync(dir, { recursive: true, force: true }); }
  `;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', script], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

function roundTrip(values, identify) {
  const writer = new Writer({ identify });
  const bytes = new Uint8Array([...new TextEncoder().encode(MAGIC)]);
  for (const [i, value] of values.entries()) writer.record(RECORD.frame, i + 1, `frame${i}`, value);
  const body = writer.take();
  const all = new Uint8Array(bytes.length + body.length);
  all.set(bytes); all.set(body, bytes.length);
  return [...readRecords(all)];
}

test('numbers keep their 64 bits, -0, NaN and the infinities included', () => {
  const numbers = [0, -0, 1, -1, 0.1, -2.5, 2 ** 52 - 1, -(2 ** 52) + 1, 2 ** 52, 2 ** 53 + 2, -(2 ** 60), Number.MAX_VALUE, Number.MIN_VALUE,
    -Number.MIN_VALUE, Number.EPSILON, NaN, Infinity, -Infinity, 1 / 3, Math.fround(0.1), 123456789.123, 2 ** -1074];
  const [record] = roundTrip([numbers]);
  assert.equal(record.value.length, numbers.length);
  for (const [i, n] of numbers.entries()) assert.ok(Object.is(record.value[i], n), `${n} came back as ${record.value[i]}`);
});

test('strings keep their code units, lone surrogates included; keys keep their order', () => {
  const strings = ['', 'a', 'déjà vu', '\u{1f600}', 'x\ud800y', '\udfff', 'a'.repeat(300)];
  const object = { z: 1, a: 2, 10: 3, 2: 4, m: strings };
  const [record] = roundTrip([object]);
  assert.deepEqual(Object.keys(record.value), Object.keys(object));
  for (const [i, s] of strings.entries()) assert.equal(record.value.m[i], s);
});

test('objects met twice come back as one object; three.js values, typed arrays, Maps and Sets come back whole', () => {
  const shared = new THREE.Vector3(1, -0, 3.5);
  const q = new THREE.Quaternion(0.1, 0.2, 0.3, Math.sqrt(1 - 0.14));
  const m = new THREE.Matrix4().makeRotationY(0.3).setPosition(1, 2, 3);
  const e = new THREE.Euler(0.1, 0.2, 0.3, 'YXZ');
  const box = new THREE.Box3(new THREE.Vector3(-1, -2, -3), new THREE.Vector3(4, 5, 6));
  const list = [1, 2];
  const value = { a: shared, b: shared, q, m, e, box, f32: new Float32Array([0.1, -0, NaN]), u16: new Uint16Array([1, 65535]),
    map: new Map([['k', list], [7, shared]]), set: new Set([list, 'x']), list, nothing: undefined, none: null, yes: true, no: false,
    handle: new Handle('unit', 17), name: new Handle('def', 'rep_inf_clone_trooper'), left: new Omitted('getter'), fn: function named() {} };
  const [record] = roundTrip([value]);
  const v = record.value;
  assert.equal(v.a, v.b, 'a shared vector is one object');
  assert.ok(Object.is(v.a.y, -0));
  assert.deepEqual([v.q.x, v.q.y, v.q.z, v.q.w], [q.x, q.y, q.z, q.w]);
  assert.deepEqual(v.m.elements, m.elements);
  assert.deepEqual([v.e.x, v.e.y, v.e.z, v.e.order], [e.x, e.y, e.z, 'YXZ']);
  assert.deepEqual([v.box.min, v.box.max], [[-1, -2, -3], [4, 5, 6]]);
  assert.ok(v.f32 instanceof Float32Array && Object.is(v.f32[1], -0) && Number.isNaN(v.f32[2]) && v.f32[0] === Math.fround(0.1));
  assert.deepEqual([...v.u16], [1, 65535]);
  assert.equal(v.map.get('k'), v.list, 'a list in a Map is the same list');
  assert.equal(v.map.get(7), v.a);
  assert.ok(v.set.has(v.list) && v.set.has('x'));
  assert.equal(v.nothing, undefined);
  assert.ok('nothing' in v);
  assert.deepEqual([v.none, v.yes, v.no], [null, true, false]);
  assert.ok(v.handle instanceof Handle && v.handle.kind === 'unit' && v.handle.key === 17);
  assert.equal(v.name.key, 'rep_inf_clone_trooper');
  assert.ok(v.left instanceof Omitted && v.left.why === 'getter');
  assert.ok(v.fn instanceof FunctionName && v.fn.name === 'named');
});

test('a getter is left out, never read; a class keeps its name; identify names what it knows', () => {
  let reads = 0;
  class Thing { constructor() { this.x = 1; Object.defineProperty(this, 'lazy', { get: () => ++reads, enumerable: true }); } }
  const known = { big: 'not written' };
  const [record] = roundTrip([{ thing: new Thing(), known }], (o) => (o === known ? new Handle('system', 'known') : undefined));
  assert.equal(reads, 0);
  assert.equal(record.value.thing.$class, 'Thing');
  assert.ok(record.value.thing.lazy instanceof Omitted);
  assert.ok(record.value.known instanceof Handle && record.value.known.key === 'known');
});

test('records are counted afresh and skippable by their length', () => {
  const s = 'a string both records hold', o = { s };
  const records = roundTrip([{ s, o, again: o }, { s, o }]);
  assert.equal(records.length, 2);
  assert.deepEqual(records.map((r) => [r.tick, r.name]), [[1, 'frame0'], [2, 'frame1']]);
  assert.equal(records[0].value.again, records[0].value.o);
  assert.equal(records[1].value.s, s);
});

test('every capture marker still finds the one statement it wraps', () => {
  for (const [file, edits] of Object.entries(EDITS)) {
    const source = fs.readFileSync(`${root}${file.slice(1)}`, 'utf8');
    const edited = edit(source, edits, file);
    for (const phase of edited.matchAll(/__bfePhase\?\.(enter|exit)\("([a-zA-Z]+)"\)/g)) assert.ok(PHASES.some((p) => p.name === phase[2]), `${file}: ${phase[2]} is no phase`);
  }
  // Each phase is marked in and out once, in the order phases.mjs lists.
  const marks = Object.entries(EDITS).flatMap(([file, edits]) => [...edit(fs.readFileSync(`${root}${file.slice(1)}`, 'utf8'), edits, file)
    .matchAll(/__bfePhase\?\.(enter|exit)\("([a-zA-Z]+)"\)/g)].map((m) => `${m[1]} ${m[2]}`));
  for (const phase of LIVE_PHASES) {
    assert.equal(marks.filter((m) => m === `enter ${phase.name}`).length, 1, `enter ${phase.name}`);
    assert.equal(marks.filter((m) => m === `exit ${phase.name}`).length, 1, `exit ${phase.name}`);
  }
  const game = edit(fs.readFileSync(`${root}src/game.js`, 'utf8'), EDITS['/src/game.js'], 'game.js');
  const order = [...game.matchAll(/__bfePhase\?\.enter\("([a-zA-Z]+)"\)/g)].map((m) => m[1]);
  assert.deepEqual(order, PHASES.map((p) => p.name).filter((name) => order.includes(name)));
});

test('the phases are the ones rust/sim-replay names, in the same order', { skip: !fs.existsSync(`${root}rust/sim-replay/src/phase.rs`) && 'no rust/sim-replay' }, () => {
  const rust = fs.readFileSync(`${root}rust/sim-replay/src/phase.rs`, 'utf8');
  const named = [...rust.matchAll(/^\s*\(Phase::([A-Za-z]+), "([a-zA-Z]+)"\),/gm)].map((m) => [m[1], m[2]]);
  assert.deepEqual(named, PHASES.map((p) => [p.rust, p.name]));
});

// A scan must work without the deleted ordnance.explode method. It observes
// both real owners, preserving their return values and receiver identity.
test('capture scans count ordnance explosions and item blasts on their real owners', () => {
  const script = `
    import assert from 'node:assert/strict';
    import fs from 'node:fs';
    import os from 'node:os';
    import path from 'node:path';
    const dir=fs.mkdtempSync(path.join(os.tmpdir(),'capture-explosions-'));
    try {
      const file=path.join(dir,'scan.json');
      process.argv=['node','capture-test','--capture-scan',file,'--capture-specs','ticks'];
      const capture=await import('./tools/sim-capture/plugin.mjs');
      const ordnance={explosion(){assert.equal(this,ordnance);return 'round';}};
      const items={explode(){assert.equal(this,items);return 'item';}};
      capture.battle({game:{units:{all:()=>[]},collision:{bodies:[]},ordnance,items},index:0,options:{}});
      capture.tick(1);
      assert.equal(ordnance.explosion(),'round');assert.equal(items.explode(),'item');
      capture.ticked(1,'fixture');capture.finish();
      const scan=JSON.parse(fs.readFileSync(file,'utf8'));
      assert.deepEqual(scan.events.filter(e=>e[1]==='explosion'),[[1,'explosion',2]]);
    } finally {fs.rmSync(dir,{recursive:true,force:true});}
  `;
  const result=spawnSync(process.execPath,['--input-type=module','-e',script],{cwd:root,encoding:'utf8'});
  assert.equal(result.status,0,result.stdout+result.stderr);
});
