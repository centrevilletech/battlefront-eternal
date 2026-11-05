import test from 'node:test';
import assert from 'node:assert/strict';
import { mountedFleet } from './fixtures/f2-mounted.js';
import { weaponDefinition } from './fixtures/f2-weapon.js';
import { cycleNetworkSeat } from '../src/runtime/network-seat.js';
import { IntentControls, blankIntent } from '../src/units.js';

test('native network seat cycling swaps bots and preserves other humans', async () => {
  const { vs, v, crew } = await mountedFleet(weaponDefinition('fixture-seat-cycle'), { seats: 3, network: true });
  try {
    const bot = vs.units.add(new crew.constructor(vs.collision));
    const other = vs.units.add(new crew.constructor(vs.collision)); other.networkHuman = true;
    assert.equal(vs.board(bot, v, 1), true);
    assert.equal(vs.board(other, v, 2), true);
    const first = crew.seat;
    assert.equal(cycleNetworkSeat(vs, crew), true);
    assert.equal(crew.seat.index, 1); assert.equal(bot.seat.index, 0);
    assert.equal(other.seat.index, 2);
    assert.equal(cycleNetworkSeat(vs, crew), true);
    assert.equal(crew.seat.index, 0, 'the other human keeps seat 2');
    assert.equal(cycleNetworkSeat(vs, crew, -1), true);
    assert.equal(crew.seat.index, 1);
    assert.equal(vs.vacate(crew.seat, { placement: false }), true, 'disconnect publishes before returning');
    assert.equal(crew.seat, null); assert.equal(v.seats[1].lease, null);
    assert.equal(first.released, true);
    assert.equal(vs.vacate(first, { placement: false }), false, 'an old occupancy cannot release its promoted replacement');
    assert.equal(bot.seat.index, 0); assert.equal(other.seat.index, 2);
  } finally { vs.dispose(); }
});
test('previous weapon cycling passes through network intent once',()=>{
  const unit={intent:blankIntent()}, controls=new IntentControls(unit);
  unit.intent.previousPrimary=true;
  assert.equal(controls.take('previousPrimary'),true);
  assert.equal(controls.take('previousPrimary'),false);
});
