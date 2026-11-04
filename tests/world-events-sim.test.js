import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { SimEvents } from '../src/sim-events.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

const snapshot = (journal, readers = []) => ({ capacity: journal.capacity, seq: journal.seq, time: journal.time,
  ring: Object.assign([], journal.ring), readers: readers.map(reader => [reader.cursor, reader.dropped]) });
const caught = run => { try { return run(); } catch (error) { return error.message; } };

test('native event journals twin bounded rings and independent readers over mixed operations', async () => {
  const world = createWorldSim(await loadSim());
  for (const capacity of [0, 1, 3, 7, 64, 4096]) {
    const js = new SimEvents(capacity), native = new SimEvents(capacity);
    const a = [js.reader(), js.reader(), js.reader()], b = [native.reader(), native.reader(), native.reader()];
    const ring = native.ring, listeners = native.listeners;
    world.events(native);
    assert.equal(world.events(native), native); assert.equal(world.ownsEvents(native), true);
    assert.equal(native.ring, ring); assert.equal(native.listeners, listeners);
    let seed = 12345;
    for (let i = 0; i < 2100; i++) {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      const op = seed % 13, n = (seed >>> 8) % 3;
      if (op < 9) {
        js.time = native.time = i / 60;
        assert.deepEqual(native.emit(op, { n: i, position: new Vector3(i, -i, i / 3) }), js.emit(op, { n: i, position: new Vector3(i, -i, i / 3) }));
      } else if (op < 12) {
        const expected = [], actual = [];
        a[n].read(record => expected.push(record)); b[n].read(record => actual.push(record));
        assert.deepEqual(actual, expected);
      } else if (i % 5 === 0) { js.reset(); native.reset(); }
      else { a[n] = js.reader(); b[n] = native.reader(); }
      assert.deepEqual(snapshot(native, b), snapshot(js, a));
    }
  }
});

test('native journal attachment preserves shallow payloads, inherited fields and readers already in use', async () => {
  const world = createWorldSim(await loadSim());
  const run = native => {
    const journal = new SimEvents(4), reader = journal.reader(), shared = { later: 1 }, vector = new Vector3(1, 2, 3);
    const first = journal.emit('first', { shared });
    if (native) world.events(journal);
    let notifications = 0;
    journal.listen(record => { notifications++; assert.equal(journal.ring[(journal.seq - 1) % journal.capacity], record); });
    const fields = Object.create({ inherited: shared });
    Object.defineProperty(fields, 'nested', { enumerable: true, get() {
      journal.emit('inside', { shared }); journal.time = 12; return shared;
    } });
    Object.assign(fields, { position: vector, seq: 99, kind: 'overridden', time: -5 });
    const outer = journal.emit('outer', fields);
    shared.later = 2; vector.set(9, 9, 9);
    assert.equal(outer.shared, undefined); assert.equal(outer.inherited, shared); assert.equal(outer.nested, shared);
    assert.deepEqual(outer.position, [1, 2, 3]);
    const records = []; reader.read(record => records.push(record));
    assert.equal(records[0], first); assert.equal(records[2], outer);
    return { state: snapshot(journal, [reader]), records, notifications };
  };
  assert.deepEqual(run(true), run(false));
});

test('native event listeners keep live Set iteration through nested emissions, resets and memory growth', async () => {
  const instance = await loadSim(), world = createWorldSim(instance);
  const run = native => {
    const journal = new SimEvents(3), other = new SimEvents(2), reader = journal.reader();
    if (native) { world.events(journal); world.events(other); }
    const trace = []; let nested = false;
    const late = record => trace.push(['late', record.kind, journal.seq]);
    const removed = record => trace.push(['removed', record.kind]);
    journal.listen(record => {
      trace.push(['first', record.kind, journal.seq]);
      if (!nested) {
        nested = true; journal.listeners.delete(removed); journal.listen(late);
        other.emit('other'); journal.emit('nested');
        if (native) instance.exports.memory.grow(1);
        reader.read(value => trace.push(['read', value.kind, reader.cursor]));
      }
    });
    journal.listen(removed);
    journal.emit('outer'); journal.reset(); journal.emit('after');
    reader.read(value => trace.push(['after-reset', value.kind]));
    return { trace, state: snapshot(journal, [reader]), other: snapshot(other) };
  };
  assert.deepEqual(run(true), run(false));
});

test('native readers preserve nested cursor edits, ring replacement and a single overflow check per read', async () => {
  const world = createWorldSim(await loadSim());
  const run = native => {
    const journal = new SimEvents(3), reader = journal.reader(), other = journal.reader();
    if (native) world.events(journal);
    for (let i = 0; i < 5; i++) journal.emit(i);
    const trace = []; let nested = false;
    reader.read(record => {
      trace.push(['outer', record.kind, reader.cursor, reader.dropped]);
      if (!nested) {
        nested = true;
        for (let i = 5; i < 9; i++) journal.emit(i);
        other.read(value => trace.push(['other', value.kind, other.cursor, other.dropped]));
        reader.read(value => trace.push(['same', value.kind, reader.cursor, reader.dropped]));
        journal.ring = journal.ring.slice(); journal.ring[0] = null;
        reader.cursor = journal.seq - 2;
      }
    });
    return { trace, state: snapshot(journal, [reader, other]) };
  };
  assert.deepEqual(run(true), run(false));
});

test('native journal callbacks preserve partial state on throws and recover with the same dispatcher', async () => {
  const world = createWorldSim(await loadSim());
  const run = native => {
    const journal = new SimEvents(2), reader = journal.reader();
    if (native) world.events(journal);
    const errors = [], trace = [];
    const stop = journal.listen(record => { journal.time = 7; throw Error(`listener ${record.kind}`); });
    errors.push(caught(() => journal.emit('failed'))); stop();
    journal.emit('next');
    errors.push(caught(() => reader.read(record => {
      trace.push([record.kind, reader.cursor]); journal.emit('inside'); throw Error('reader failed');
    })));
    reader.read(record => trace.push([record.kind, reader.cursor]));
    errors.push(caught(() => journal.emit('fields', { get fail() { journal.emit('before getter failed'); throw Error('getter failed'); } })));
    journal.reset(); journal.emit('reset');
    return { errors, trace, state: snapshot(journal, [reader]) };
  };
  assert.deepEqual(run(true), run(false));
});

test('native journal loads a ring slot before advancing its cursor and retains that exact record', async () => {
  const world = createWorldSim(await loadSim());
  const run = native => {
    const journal = new SimEvents(3), reader = journal.reader(), trace = [];
    journal.emit('a'); journal.emit('b');
    if (native) world.events(journal);
    const record = journal.ring[0]; let loaded = false;
    Object.defineProperty(journal.ring, '0', { configurable: true, get() {
      trace.push(['load', reader.cursor]);
      if (!loaded) { loaded = true; reader.cursor += 1; }
      return record;
    }, set(value) { trace.push(['write', value.kind, journal.seq]); } });
    reader.read(value => { trace.push(['deliver', value.kind, reader.cursor]); assert.equal(value, record); });
    journal.seq = 3;
    const emitted = journal.emit('c');
    assert.equal(emitted.kind, 'c');
    return { trace, cursor: reader.cursor, dropped: reader.dropped, seq: journal.seq };
  };
  assert.deepEqual(run(true), run(false));
});
