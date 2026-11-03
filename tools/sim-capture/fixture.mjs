// Writes a small made-up capture (no game data) with every kind of value
// and record, for rust/sim-replay's format test: node fixture.mjs OUT
// The values are the test's expectations: change them together.
import fs from 'node:fs';
import * as THREE from 'three';
import { Writer, MAGIC, RECORD, Handle, Omitted } from './format.mjs';

const out = process.argv[2];
if (!out) throw Error('usage: node tools/sim-capture/fixture.mjs OUT');
const w = new Writer();
w.record(RECORD.header, 0, 'header', { format: 1, tool: 'sim-capture fixture', node: process.version, v8: process.versions.v8,
  battle: { map: 'test', side: 'rep', seed: 7, ticks: 3 }, phases: ['ordnance'], specs: ['fixture'] });
const def = { class: 'test_ord', base: 'bolt', Velocity: 150, LaserLength: 0.5, ImpactEffectTerrain: 'spark' };
w.whole.add(def);
w.record(RECORD.def, 1, 'test_ord', def);
w.whole.clear();
w.record(RECORD.world, 2, 'world', { units: [{ id: 3, generation: 2, alive: true, dead: false, removed: false, team: 1, className: 'rep_inf_test', classDef: new Handle('def', 'rep_inf_test') }] });

// Every kind of value, in one frame's arguments.
const shared = new THREE.Vector3(1.5, -0, -2.25);
const list = ['x', shared];
const values = [0, -0, 0.1, -1, 2 ** 52 - 1, -(2 ** 52) + 1, 2 ** 52, 2 ** 53 + 2, NaN, Infinity, -Infinity, Number.MIN_VALUE,
  'plain', 'déjà', '\ud800', true, false, null, undefined, shared, shared, list,
  new THREE.Quaternion(0.5, 0.5, 0.5, 0.5), new THREE.Matrix4().makeTranslation(1, 2, 3), new THREE.Euler(0.1, 0.2, 0.3, 'YXZ'),
  new THREE.Box3(new THREE.Vector3(-1, -2, -3), new THREE.Vector3(1, 2, 3)),
  new Float32Array([0.1, -0]), new Uint16Array([65535]), new Int32Array([-5]), new Map([['k', list], [2, 'two']]), new Set([list, 3]),
  new Handle('unit', 3), new Handle('def', 'test_ord'), new Omitted('getter'), function named() {}, { z: 1, a: 2, 10: 3, 2: 4 }];
// A frame (id 0) holding another (id 1): enter, a draw, a call with a draw
// inside it and an out parameter, the inner frame, the end.
const hit = { unit: null, distance: 0, point: new THREE.Vector3() };
w.record(RECORD.frame, 2, 'test.frame', { k: 'enter', f: 0, p: null, a: values, s: { list: [1, 2], hit } });
w.record(RECORD.frame, 2, 'test.inner', { k: 'enter', f: 1, p: 0, a: [], s: { n: 1 } });
w.record(RECORD.frame, 2, 'test.frame', { k: 'call', f: 0, n: 'units.raycast', a: [new THREE.Vector3(0, 1, 0), 25.5, hit], d: [['A', 4294967295], ['stream:gameplay', 12]] });
hit.unit = new Handle('unit', 3); hit.distance = 12.25; hit.point.set(0, 1, 12.25);
w.record(RECORD.frame, 2, 'test.frame', { k: 'ret', f: 0, n: 'units.raycast', r: hit, o: [undefined, undefined, hit], i: [['B', 7]], x: { striking: { owner: null } } });
w.record(RECORD.frame, 2, 'test.inner', { k: 'exit', f: 1, d: [], s: { n: 2 }, r: undefined });
w.record(RECORD.frame, 2, 'test.frame', { k: 'exit', f: 0, d: [['Math.random', 0.25], ['seeded:99', 0.5]], s: { list: [1, 2, 3], hit }, r: 'done' });
w.record(RECORD.tick, 2, '', { k: 'tick', line: '2 a b c', draws: [['ordnance', { A: 1 }]], sampled: true, events: [['death', 3]] });
const body = w.take();
fs.writeFileSync(out, Buffer.concat([Buffer.from(MAGIC), Buffer.from(body)]));
