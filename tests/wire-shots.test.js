import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { EntityShotCounters } from '../src/runtime/network-entity-shots.js';
test('native counters retain bounded generation state through live events and disposal', async () => {
  const instance = await loadSim(), listeners = new Set(), events = { listen(fn) { listeners.add(fn); return () => listeners.delete(fn); } };
  const js = new EntityShotCounters(events, {maxEntities:8}), native = new EntityShotCounters(events, {maxEntities:8,bridge:bridgeFor(instance)});
  const units = Array.from({length:12}, (_,id) => ({id,generation:1}));
  for (let n = 0; n < 1200; n++) {
    const unit = units[n % units.length]; if (n % 7 === 0) unit.generation++;
    const event = {kind:n%13?'shot':'damage', source:{unit, generation:unit.generation-(n%5===0?1:0)}};
    for (const fn of listeners) fn(event);
    for (const u of units) assert.equal(native.counter(u),js.counter(u));
    assert.equal(native.overflow,js.overflow);
    if (n===350) instance.exports.memory.grow(2);
    if (n===700) {native.reset();js.reset();}
  }
  for (const value of [NaN, Infinity, -Infinity, 1.1, 2**53, '1', undefined, null]) {
    const event={kind:'shot',source:{unit:{id:value,generation:value},generation:value}};
    for (const fn of listeners) fn(event);
    assert.equal(native.overflow,js.overflow);
  }
  native.dispose();js.dispose();assert.equal(listeners.size,0);assert.equal(native.overflow,0);assert.equal(native.counter(units[0]),0);
});
