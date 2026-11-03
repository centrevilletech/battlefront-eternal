import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { ExportContext, Heap, SimContext } from '../src/sim/runtime.js';
import { parseType } from '../tools/sim-schema.mjs';
import { KIND_TABLE, DEF_TABLES, KIND_SHELF, KIND_THING, KIND_PERSON } from './sim-gen/index.js';

// The store's generator (tools/sim-schema.mjs) on its own test schema
// (rust/sim/schema-test/shelf.mjs, generated into tests/sim-gen): the
// JavaScript it makes for every type of field writes rows that read back
// the same, writes a map's entries word for word as the Rust it makes does
// (rust/sim/src/gen_tests.rs, a_map_s_words_are_its_entries), and exports
// entities that name each other once each.

class Person { constructor(id) { this.id = id; } }

// A context for the test schema, with a string table of its own (a
// battle's is the module's).
function context() {
  const cx = new SimContext({ kinds: KIND_TABLE, defTables: DEF_TABLES, classes: { Person } });
  const ids = new Map(), list = [];
  cx.strings = { id: (s) => { if (!ids.has(s)) { ids.set(s, list.length); list.push(s); } return ids.get(s); }, at: (i) => list[i] };
  return cx;
}

// The shelf rust/sim/src/gen_tests.rs makes, as JavaScript objects (the
// strings "a" and "b" interned first, so they are 0 and 1 there too).
function shelf(cx) {
  cx.strings.id('a'); cx.strings.id('b');
  const people = [new Person(4), new Person(9)];
  const goods = [{ class: 'cup' }, { class: 'jug' }, { class: 'pan' }];
  for (const d of goods) cx.defs.goods.register(d);
  const spot = { at: new THREE.Vector3(1, -0, 3), turn: new THREE.Quaternion(0, 0, 0.7071067811865476, 0.7071067811865476), label: 'b', mark: 'green', near: people[1], kept: [] };
  const one = {
    count: -3, size: 4294967295, small: 255, light: Math.fround(0.1), mass: -0, on: true, where: new THREE.Vector3(1.5, 2.5, -3.5), facing: new THREE.Quaternion(),
    pair: [1e-300, 5e-324], name: 'a', colour: 'blue', def: goods[2], owner: people[0], holder: people[1], spot, friends: new Set([people[1], people[0]]),
    next: null, tags: new Map([['a', 'b'], ['b', 'a']]), hidden: 1.25, box: { size: 2.5 },
  };
  const two = {
    count: 0, size: 0, small: 0, light: 0, mass: 0, on: false, where: new THREE.Vector3(), facing: new THREE.Quaternion(), pair: [0, 0], name: 'b', colour: 'red',
    def: goods[0], owner: null, holder: one, spot: null, friends: new Set(), next: one, tags: undefined, hidden: 0, box: { size: 0 },
  };
  one.next = two;
  const things = [one, two];
  return {
    people, things,
    shelf: { things, byName: new Map([['a', [two, one]], ['b', []]]), weights: new Map([['a', 1.5], ['b', -0]]), spots: new Map([['b', { ...spot, kept: [two] }]]),
      owners: new Map([['b', people[0]]]), names: new Map([['a', 'b']]) },
  };
}

// Writes a kind's objects as full rows and makes the block an export of
// them would be: header, rows, heap.
function block(cx, kind, objects) {
  const k = KIND_TABLE[kind], stride = k.fullStride, heap = new Heap();
  const rows = new Float64Array(objects.length * stride);
  objects.forEach((o, i) => {
    if (!k.singleton) cx.refs[kind].put(o, rows, i * stride);
    k.putFull(cx, o, rows, i * stride, heap);
  });
  const f = new Float64Array(5 + rows.length + heap.words.length);
  f.set([objects.length, stride, 5 + rows.length, heap.words.length, 0]);
  f.set(rows, 5);
  f.set(heap.words, 5 + rows.length);
  return f;
}

function readBack(cx, kind, f) {
  const k = KIND_TABLE[kind], out = [];
  for (let i = 0; i < f[0]; i++) out.push(k.syncFull(cx, f, 5 + i * f[1], f[2], new k.Mirror()));
  return out;
}

test('every type of field written as a row reads back the same', () => {
  const cx = context();
  const { shelf: s, things } = shelf(cx);
  const back = readBack(cx, KIND_THING, block(cx, KIND_THING, things));
  back.forEach((m, i) => assert.equal(KIND_TABLE[KIND_THING].same(things[i], m), null, `thing ${i}`));
  const [system] = readBack(cx, KIND_SHELF, block(cx, KIND_SHELF, [s]));
  assert.equal(KIND_TABLE[KIND_SHELF].same(s, system), null);
  assert.equal(system.byName.get('a')[0], things[1]);
  assert.ok(Object.is(system.weights.get('b'), -0));
  assert.equal(back[1].holder, things[0]);
  assert.equal(back[0].box.size, 2.5);
});

test('a map\'s words are its entries, as the Rust writes them', () => {
  const cx = context();
  const { shelf: s, things } = shelf(cx);
  block(cx, KIND_THING, things); // the things' slots: 0 and 1, generation 1
  const f = block(cx, KIND_SHELF, [s]), heap = f[2], row = 5;
  const bits = (xs) => [...new Float64Array(xs)].map((x) => new BigUint64Array(new Float64Array([x]).buffer)[0]);
  // weights: { a: 1.5, b: -0 }.
  const [at, n] = [f[row + 6], f[row + 7]];
  assert.equal(n, 2);
  assert.deepEqual(bits(f.subarray(heap + at, heap + at + 4)), bits([0, 1.5, 1, -0]));
  // byName: { a: [two, one], b: [] }, its lists after its entries.
  const [by, m] = [f[row + 4], f[row + 5]];
  assert.equal(m, 2);
  const e = f.subarray(heap + by, heap + by + 6);
  assert.deepEqual([e[0], e[2], e[3], e[5]], [0, 2, 1, 0]);
  assert.deepEqual([...f.subarray(heap + e[1], heap + e[1] + 4)], [1, 1, 0, 1]);
});

test('entities that name each other are exported once each', () => {
  const cx = context();
  const { shelf: s, things } = shelf(cx);
  const ex = new ExportContext({ kinds: KIND_TABLE, defTables: DEF_TABLES, classes: { Person }, ref: (kind, o) => ({ kind, id: o.id }), def: (table, d) => ({ def: d.class }) });
  const value = KIND_TABLE[KIND_SHELF].export(s, ex);
  const [one, two] = value.things;
  assert.equal(one.next, two);
  assert.equal(two.next, one);
  assert.equal(two.holder, one);
  assert.deepEqual(one.holder, { kind: 'person', id: 9 });
  assert.equal(value.byName.get('a')[0], two);
  assert.deepEqual(one.def, { def: 'pan' });
  assert.equal(one.boxed, 2.5);
  assert.equal(value.spots.get('b').kept[0], two);
  assert.deepEqual([...one.friends].map((p) => p.id), [9, 4]);
});

test('a schema\'s types are checked as they are read', () => {
  assert.deepEqual(parseType('map:list:thing|null', 'x').value.of, 'thing');
  assert.throws(() => parseType('map:f64|null|null', 'x'), /twice/);
  assert.throws(() => parseType('map:map:f64', 'x'), /no map/);
  assert.throws(() => parseType('vec4', 'x'), /unknown type/);
  assert.throws(() => parseType('f64|maybe', 'x'), /null or undefined/);
});
