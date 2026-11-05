import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { ExportContext, Heap, SimContext } from '../src/sim/runtime.js';
import { buildModel, parseType } from '../tools/sim-schema.mjs';
import { KIND_TABLE, DEF_TABLES, RECORD_CLASSES, KIND_SHELF, KIND_THING, KIND_PERSON } from './sim-gen/index.js';

// The store's generator (tools/sim-schema.mjs) on its own test schema
// (rust/sim/schema-test/shelf.mjs, generated into tests/sim-gen): the
// JavaScript it makes for every type of field writes rows that read back
// the same, writes a map's entries word for word as the Rust it makes does
// (rust/sim/src/gen_tests.rs, a_map_s_words_are_its_entries), and exports
// entities that name each other once each.

class Person { constructor(id) { this.id = id; } }
class Legs { constructor(stride, side) { this.stride = stride; this.side = side; } }
class Wheels { constructor(count, grip) { this.count = count; this.grip = grip; } }

// A context for the test schema, with a string table of its own (a
// battle's is the module's).
function context() {
  const cx = new SimContext({ kinds: KIND_TABLE, defTables: DEF_TABLES, recordClasses: RECORD_CLASSES, classes: { Person, Legs, Wheels } });
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
  const spot = { at: new THREE.Vector3(1, -0, 3), turn: new THREE.Quaternion(0, 0, 0.7071067811865476, 0.7071067811865476), label: 'b', mark: 'green', near: people[1], keeper: people[0], kept: [] };
  const one = {
    count: -3, size: 4294967295, small: 255, light: Math.fround(0.1), mass: -0, on: true, where: new THREE.Vector3(1.5, 2.5, -3.5), facing: new THREE.Quaternion(),
    pair: [1e-300, 5e-324], name: 'a', colour: 'blue', def: goods[2], owner: people[0], holder: people[1], spot, friends: new Set([people[1], people[0]]),
    next: null, tags: new Map([['a', 'b'], ['b', 'a']]), hidden: 1.25, box: { size: 2.5 },
    marks: [1.5, -0, 1e300], probes: [new THREE.Vector3(1, 2, 3), new THREE.Vector3(-4, 5, -6)], flags: [true, false, true],
    spots: [spot, { ...spot, mark: 'red', label: null }], holders: ['nobody', people[0], null],
    matrix: new THREE.Matrix4().makeTranslation(5, -6, 7.5), boxes: [{ size: -2.5 }],
    shapes: [{ radius: 0.5, half: 0.25 }, { radius: -0, half: new THREE.Vector3(1, 2, 0.5) }], hitSet: null, body: new Wheels(4, new THREE.Vector3(0.1, 0.2, 0.3)),
    gains: new Float32Array([0.5, -0]), maybe: [1.5, undefined, -0], seenBy: new Map([[people[1], 3], [people[0], 255]]), marked: { 2: spot, 0: null },
  };
  one.hitSet = one.shapes[1];
  const two = {
    count: 0, size: 0, small: 0, light: 0, mass: 0, on: false, where: new THREE.Vector3(), facing: new THREE.Quaternion(), pair: [0, 0], name: 'b', colour: 'red',
    def: goods[0], owner: null, holder: one, spot: null, friends: new Set(), next: one, tags: undefined, hidden: 0, box: { size: 0 },
    marks: [], probes: [], flags: [], spots: [], holders: [one], matrix: new THREE.Matrix4(), boxes: [{ size: 0 }],
    shapes: [], hitSet: null, body: new Legs(1.5, [1, -1]),
    gains: null, maybe: [], seenBy: new Map(), marked: {},
  };
  one.next = two;
  const things = [one, two];
  // Set out of order: an object's number keys come out ascending anyway.
  const byTeam = {};
  byTeam[7] = 0.5; byTeam[1] = -0; byTeam[3] = 150;
  // A key deleted and set again goes last; -0 is kept as 0; NaN is a key.
  const counts = new Map([[5, 1], [-0, 2], [NaN, 3], [2, 4]]);
  counts.delete(5); counts.set(5, 6); counts.set(2, 8);
  return {
    people, things,
    shelf: { things, byName: new Map([['a', [two, one]], ['b', []]]), weights: new Map([['a', 1.5], ['b', -0]]), spots: new Map([['b', { ...spot, kept: [two] }]]),
      owners: new Map([['b', people[0]]]), names: new Map([['a', 'b']]),
      byTeam, rules: { 2: [[0, 0.25], [10, 0.5]], 1: [] }, labels: { 2: undefined, 1: 'a' }, counts, spotsAt: new Map([[1.5, spot]]),
      tally: new Map([[two, 1], [one, 2]]), queues: new Map([[people[0], [one]], [two, [two, one]], [people[1], []]]),
      times: new Float64Array([1, -0, 1e300, NaN, 5e-324]), slots: new Int32Array([-1, 2147483647, -2147483648, 0]), ring: [one, null, two, null] },
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
  assert.equal(back[0].boxes[0].size, -2.5);
  assert.ok(Object.is(back[0].marks[1], -0) && back[0].probes[1].isVector3 && back[0].flags[2] === true);
  assert.deepEqual(back[0].matrix.elements.slice(12, 15), [5, -6, 7.5]);
  assert.equal(back[1].holders[0], things[0]);
  assert.equal(back[0].spots[1].mark, 'red');
  // The alias is the very item of the list; a union holds a number or a vector, and a body of either class.
  assert.equal(back[0].hitSet, back[0].shapes[1]);
  assert.equal(back[0].shapes[0].half, 0.25);
  assert.ok(back[0].shapes[1].half.isVector3);
  assert.deepEqual([back[0].body.count, back[1].body.stride, back[1].body.side], [4, 1.5, [1, -1]]);
});

test('numeric mirror blits retain storage and notify quaternion changes once', () => {
  const cx = context(), { things } = shelf(cx), words = block(cx, KIND_THING, things);
  const k = KIND_TABLE[KIND_THING], mirror = new k.Mirror();
  let changes = 0;
  const vector = mirror.where, rotation = mirror.facing;
  rotation._onChange(() => { changes++; });
  k.syncFull(cx, words, 5, words[2], mirror);
  assert.equal(mirror.where, vector); assert.equal(mirror.facing, rotation);
  assert.equal(changes, 1); assert.deepEqual(vector.toArray(), things[0].where.toArray());
  assert.deepEqual(rotation.toArray(), things[0].facing.toArray());
  k.syncFull(cx, words, 5, words[2], mirror);
  assert.equal(changes, 2);
});

test('number keys and object keys keep JavaScript\'s order through the rows', () => {
  const cx = context();
  const { shelf: s, things, people } = shelf(cx);
  block(cx, KIND_THING, things);
  const [m] = readBack(cx, KIND_SHELF, block(cx, KIND_SHELF, [s]));
  // An object's keys ascending however they were set; its values, -0 too.
  assert.deepEqual(Object.keys(m.byTeam), ['1', '3', '7']);
  assert.ok(Object.is(m.byTeam[1], -0));
  assert.deepEqual(Object.keys(m.labels), ['1', '2']);
  assert.ok('2' in m.labels && m.labels[2] === undefined);
  assert.deepEqual(Object.keys(m.rules), ['1', '2']);
  // A Map's insertion order, the key set again after its delete last, -0
  // as 0, NaN found.
  assert.deepEqual([...m.counts.keys()], [0, NaN, 2, 5]);
  assert.ok(Object.is([...m.counts.keys()][0], 0));
  assert.deepEqual([...m.counts.values()], [2, 3, 8, 6]);
  assert.equal(m.counts.get(-0), 2);
  assert.equal(m.counts.get(NaN), 3);
  // Objects as keys: the very objects, of either kind of a union.
  assert.deepEqual([...m.tally.keys()], [things[1], things[0]]);
  assert.deepEqual([...m.queues.keys()], [people[0], things[1], people[1]]);
  assert.deepEqual(m.queues.get(things[1]), [things[1], things[0]]);
  // Typed arrays kept typed, bit for bit; list items that can be null.
  assert.ok(m.times instanceof Float64Array && m.slots instanceof Int32Array);
  assert.ok(Object.is(m.times[1], -0) && Number.isNaN(m.times[3]) && m.times[4] === 5e-324);
  assert.deepEqual([...m.slots], [-1, 2147483647, -2147483648, 0]);
  assert.deepEqual(m.ring, [things[0], null, things[1], null]);
  const [one] = readBack(cx, KIND_THING, block(cx, KIND_THING, things));
  assert.ok(one.gains instanceof Float32Array && Object.is(one.gains[1], -0));
  assert.deepEqual(Object.keys(one.marked), ['0', '2']);
  assert.ok(one.marked[0] === null && one.marked[2].mark === 'green');
  assert.ok(Object.is(one.maybe[2], -0) && 1 in one.maybe && one.maybe[1] === undefined);
});

test('a mirror is synced in place: its objects, Maps, arrays and typed arrays kept', () => {
  const cx = context();
  const { shelf: s, things, people } = shelf(cx);
  block(cx, KIND_THING, things);
  const k = KIND_TABLE[KIND_SHELF];
  const syncInto = (m) => { const f = block(cx, KIND_SHELF, [s]); return k.syncFull(cx, f, 5, f[2], m); };
  const m = syncInto(new k.Mirror());
  const kept = { byTeam: m.byTeam, rules2: m.rules[2], counts: m.counts, spot: m.spotsAt.get(1.5), queues: m.queues, queue: m.queues.get(things[1]), times: m.times, slots: m.slots, ring: m.ring };
  // The battle moves on: a key gone and one new, a list grown, a key set
  // again after its delete (last), a value changed in place.
  delete s.byTeam[3]; s.byTeam[4] = 2;
  s.rules[2].push([20, 1]);
  s.counts.delete(0); s.counts.set(0, 9); s.counts.set(2, -1); s.counts.set(11, 7);
  s.queues.get(things[1]).push(things[0]);
  s.queues.delete(people[0]); s.queues.set(people[0], [things[0]]);
  s.times[0] = 2; s.slots[3] = 42;
  s.ring[1] = things[1];
  assert.equal(syncInto(m), m);
  assert.equal(k.same(s, m), null);
  assert.equal(m.byTeam, kept.byTeam);
  assert.deepEqual(Object.keys(m.byTeam), ['1', '4', '7']);
  assert.equal(m.rules[2], kept.rules2);
  assert.equal(m.rules[2].length, 3);
  assert.equal(m.counts, kept.counts);
  assert.deepEqual([...m.counts], [[NaN, 3], [2, -1], [5, 6], [0, 9], [11, 7]]);
  assert.equal(m.spotsAt.get(1.5), kept.spot);
  assert.equal(m.queues, kept.queues);
  assert.equal(m.queues.get(things[1]), kept.queue);
  assert.deepEqual([...m.queues.keys()], [things[1], people[1], people[0]]);
  assert.ok(m.times === kept.times && m.times[0] === 2 && m.slots === kept.slots && m.slots[3] === 42);
  assert.ok(m.ring === kept.ring && m.ring[1] === things[1]);
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
  // byTeam ({ 1: -0, 3: 150, 7: 0.5 }): each entry its key, then its value.
  const [t, tn] = [f[row + 15], f[row + 16]];
  assert.equal(tn, 3);
  assert.deepEqual(bits(f.subarray(heap + t, heap + t + 6)), bits([1, -0, 3, 150, 7, 0.5]));
  // counts (0: 2, NaN: 3, 2: 8, 5: 6), in its order.
  const [c, cn] = [f[row + 21], f[row + 22]];
  assert.equal(cn, 4);
  const cw = [...f.subarray(heap + c, heap + c + 8)];
  assert.ok(Number.isNaN(cw[2]));
  assert.deepEqual(bits([cw[0], cw[1], cw[3], cw[4], cw[5], cw[6], cw[7]]), bits([0, 2, 3, 2, 8, 5, 6]));
});

test('an entity is written out where it lives, and a handle everywhere else', () => {
  const cx = context();
  const { shelf: s, things } = shelf(cx);
  const numbers = new Map();
  const handle = (kind, o) => { if (!numbers.has(o)) numbers.set(o, numbers.size); return { kind, n: numbers.get(o) }; };
  const ex = new ExportContext({ kinds: KIND_TABLE, defTables: DEF_TABLES, recordClasses: RECORD_CLASSES, classes: { Person, Legs, Wheels }, ref: handle, def: (table, d) => ({ def: d.class }) });
  const value = KIND_TABLE[KIND_SHELF].export(s, ex);
  const [one, two] = value.things;
  assert.deepEqual(Object.keys(one).slice(0, 3), ['$id', 'count', 'size']);
  assert.deepEqual([one.$id, two.$id], [handle('thing', things[0]), handle('thing', things[1])]);
  assert.deepEqual(one.next, two.$id);
  assert.deepEqual(two.next, one.$id);
  assert.deepEqual(two.holder, one.$id);
  assert.deepEqual(one.holder, handle('person', one.holder && things[0].holder));
  assert.deepEqual(value.byName.get('a')[0], two.$id);
  assert.deepEqual(value.spots.get('b').kept[0], two.$id);
  assert.deepEqual(one.def, { def: 'pan' });
  assert.equal(one.boxed, 2.5);
  assert.equal(one.firstSize, -2.5);
  assert.deepEqual([...one.friends].map((p) => p.kind), ['person', 'person']);
  assert.equal(KIND_TABLE[KIND_THING].export(things[0], ex), one);
  assert.equal(one.hitSet, one.shapes[1]);
  assert.deepEqual([one.body.$record, two.body.$record], ['Wheels', 'Legs']);
  // Objects keyed by numbers as plain objects (keys ascending), typed
  // arrays as copies of their class, a Map's object keys as handles.
  assert.deepEqual(Object.keys(value.byTeam), ['1', '3', '7']);
  assert.notEqual(value.byTeam, s.byTeam);
  assert.deepEqual(value.rules[2], [[0, 0.25], [10, 0.5]]);
  assert.ok(value.times instanceof Float64Array && value.times !== s.times && Object.is(value.times[1], -0));
  assert.deepEqual([...value.tally.keys()], [two.$id, one.$id]);
  assert.deepEqual([...value.queues.keys()].map((k) => k.kind), ['person', 'thing', 'person']);
  assert.deepEqual(value.ring, [one.$id, null, two.$id, null]);
  assert.deepEqual([...value.counts.keys()], [0, NaN, 2, 5]);
  assert.equal(one.marked[2], one.spot);
});

test('a schema\'s types are checked as they are read', () => {
  assert.deepEqual(parseType('map:list:thing|null', 'x').value.of, 'thing');
  assert.throws(() => parseType('map:f64|null|null', 'x'), /twice/);
  assert.throws(() => parseType('map:map:f64', 'x'), /no map/);
  assert.throws(() => parseType('vec4', 'x'), /unknown type/);
  assert.throws(() => parseType('f64|maybe', 'x'), /null or undefined/);
  // An item or value that can be null or undefined is written in parentheses.
  const ring = parseType('list:(ref:character|null)', 'x');
  assert.deepEqual([ring.value.base, ring.value.of, ring.value.nullable.null, ring.nullable.null], ['ref', 'character', true, false]);
  assert.equal(parseType('list:(f64|undefined)|null', 'x').nullable.null, true);
  assert.equal(parseType('list:ref:character|null', 'x').nullable.null, true);
  assert.throws(() => parseType('list:(f64)', 'x'), /can be null or undefined/);
  assert.throws(() => parseType('list:(list:f64|null)', 'x'), /no list/);
  // Keyed by numbers or by objects; a Map of string keys whose values are references stays one.
  assert.deepEqual(parseType('map:num:list:thing', 'x').key, { base: 'num' });
  assert.deepEqual(parseType('map:ref:Who:list:thing', 'x').key, { base: 'ref', of: 'Who' });
  assert.equal(parseType('map:ref:person', 'x').key, undefined);
  assert.equal(parseType('obj:(rec:Spot|null)', 'x').value.nullable.null, true);
  assert.throws(() => parseType('obj:map:f64', 'x'), /no map/);
  const ring2 = parseType('Int32Array(100)|null', 'x');
  assert.deepEqual([ring2.base, ring2.elem, ring2.n, ring2.class], ['typed', 'i32', 100, 'Int32Array']);
  assert.throws(() => parseType('list:Float64Array(3)', 'x'), /no object or typed array/);
});

test('two types of one name, or a type named after a comparison, are refused', () => {
  const schema = (s) => [{ file: 'x', schema: s }];
  // A union named as a kind the store only refers to (its empty struct).
  assert.throws(() => buildModel(schema({ unions: { Body: ['null', 'ref:body'] }, kinds: { body: {} } })), /Body is also/);
  // A kind whose comparison would be compare.js's sameEach.
  assert.throws(() => buildModel(schema({ kinds: { each: { singleton: true, fields: { n: 'f64' } } } })), /sameEach/);
  // A Map's keys of a union: every variant a kind.
  assert.throws(() => buildModel(schema({ unions: { Key: ['null', 'ref:body'] }, kinds: { body: {}, shelf: { singleton: true, fields: { m: 'map:ref:Key:f64' } } } })), /each of its variants is a kind/);
  assert.throws(() => buildModel(schema({ kinds: { shelf: { singleton: true, fields: { m: 'map:ref:nobody:f64' } } } })), /no kind or union nobody/);
  assert.throws(() => buildModel(schema({ unions: { U: ['null', 'Float64Array(2)'] } })), /cannot be a union's variant/);
});


test('direct publication names only mutable scalar or vector fields', () => {
  const model = fields => buildModel([{ file: 'publication', schema: { records: { Pose: fields } } }]);
  const pose = model({ $layout: true, $publish: ['position', 'tick'], position: 'vec3', tick: 'f64', fixed: 'f64' }).records.get('Pose');
  assert.deepEqual(pose.publish, ['position', 'tick']);
  assert.equal(pose.fields.length, 3, 'publication metadata adds no words to the record');
  assert.throws(() => model({ $publish: ['missing'], tick: 'f64' }), /distinct fields/);
  assert.throws(() => model({ $publish: ['tick', 'tick'], tick: 'f64' }), /distinct fields/);
  assert.throws(() => model({ $publish: ['history'], history: 'list:f64' }), /scalar and vec3/);
  assert.throws(() => model({ $publish: ['position'], position: 'vec3|null' }), /vectors cannot be optional/);
});
