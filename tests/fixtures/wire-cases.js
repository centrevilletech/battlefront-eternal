import assert from 'node:assert/strict';
import { emptySnapshot, encodeEntities, encodeWordDelta, encodeWorldSection, decodeEntities, cloneSnapshot, applyEntities } from '../../src/runtime/snapshot-codec.js';
import { encodeOwnDatagram } from '../../src/runtime/snapshot-own.js';
import { entryFromRecord } from '../../src/runtime/snapshot-records.js';

// Synthetic protocol data only. Every class, group, word-boundary and
// packing boundary participates, including extended part counters.
export function wireCases(native, { count = 600, result = () => {} } = {}) {
  let seed = 7823;
  const random = n => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % n; };
  const bytes = n => Uint8Array.from({ length: n }, () => random(256));
  const check = (actual, expected) => { assert.deepEqual(actual, expected); result(expected); };
  const classes = ['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote'];
  function entry(cls, id) {
    const r = { kind: cls, id, generation: random(4), classId: random(200), team: random(4), position: [random(2000)-1000, random(500)-250, random(2000)-1000],
      velocity: [random(100)-50, random(10), random(100)-50], yaw: random(628)/100, pitch: random(300)/100-1.5,
      health: random(5000), posture: random(4), charge: random(64), state: random(4), weapon: random(200), fireCounter: random(5000),
      mode: random(4), reverse: !!random(2), dead: !!random(2), shieldOn: !!random(2), time: random(1000)/70, roll: -0, lean: 0.3, legs: 1/3, shield: 10,
      quaternion: [0.5,-0.5,0.5,-0.5], power: random(65536), building: random(50), gunYaw: -0.2, gunPitch: 0.3,
      active: !!random(2), definitionId: random(600), up: [0,1,0] };
    if (cls === 'turret') r.state = ['built','unbuilt','destroyed'][random(3)];
    return entryFromRecord(r, { mount: { slot: null, tilt: 0.3, roll: -0.1, walkPhase: 10, walkBlend: 0.4, seats: [{ yaw: 0.2, pitch: 0.8 }] } });
  }
  let previous = null;
  for (let i = 1; i <= count; i++) {
    const current = emptySnapshot(i, 7);
    const n = i % 60 === 0 ? 512 : 1 + random(80);
    for (let s = 0; s < n; s++) {
      const old = previous?.slots.get(s);
      let e = old && random(3) ? structuredClone(old) : entry(classes[s % 6], s + 100);
      if (old && random(2)) {
        if (e.pos) e.pos = e.pos.map(x => Math.max(0, Math.min(65535, x + random(255) - 127)));
        e.health = random(65536); e.disc = !!random(7);
        if (e.cls === 'droideka') { e.anim[1] = i % 2 ? 0 : -0; e.anim[2] += 1e-12; }
      }
      // Noncontiguous riders exercise grouping from both slot directions.
      if (s % 6 < 2 && s + 2 < n) e.seatSlot = s - s % 6 + 2;
      current.slots.set(s,e);
    }
    const fieldCount = i % 60 === 0 ? 30 : random(12);
    for (let f = 0; f < fieldCount; f++) {
      const key = `7:${f}`, base = previous?.world.get(key);
      const payload = base && random(3) ? base.payload.slice() : bytes(i % 60 === 0 ? 900 : random(300));
      if (payload.length && random(2)) payload[random(payload.length)] ^= 255;
      current.world.set(key, { id: f, schema: 7, generation: 1, payload });
    }
    const base = random(4) ? previous : null, interval = 1 + random(3);
    const expected = encodeEntities(current, base, { interval });
    check(native.encodeEntities(current, base, { interval }), expected);
    // Every part remains independently consumable by the existing decoder.
    const decoded = cloneSnapshot(base);
    for (const part of expected) applyEntities(decoded, decodeEntities(part, base));
    assert.equal(decoded.slots.size,current.slots.size);
    check([native.encodeWorldSection(current.world, base?.world)], [encodeWorldSection(current.world, base?.world)]);
    previous = current;
  }
  for (let n = 0; n < 1050; n++) {
    const current = bytes(n), base = current.slice();
    if (n) { current[random(n)] ^= 255; if (n % 4 === 0) current[random(n)] ^= 255; }
    const baseline = n % 5 === 0 ? null : n % 7 === 0 ? base.subarray(1) : base;
    check([native.encodeWordDelta(current, baseline)], [encodeWordDelta(current, baseline)]);
  }
  for (let i = 0; i < 200; i++) {
    const second = i % 3 === 0;
    const own = { epoch: 4, tick: i, baselineTick: Math.max(0,i-2), interval: 2, ackInput: 0xffffffff-i, second, ownParts: second || i % 2 ? 2 : 1,
      correction: second ? null : { id: 3, schema: 11, generation: 9, payload: bytes(random(600)) },
      summaries: second ? null : new Map([['3:2',{id: 2, schema: 3, generation: 1, payload: bytes(60)}]]), cosmeticBytes: bytes(random(150)) };
    check([native.encodeOwnDatagram(own)], [encodeOwnDatagram(own)]);
  }
}
