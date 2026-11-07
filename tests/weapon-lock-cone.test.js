import test from 'node:test';
import assert from 'node:assert/strict';
import {botLockTarget,seatLockTarget} from '../src/weapon-targeting.js';
test('a bot hands its controller target to its launcher; a human, local or networked, locks by view', () => {
  const target = {id:3,generation:1,alive:true};
  const controller = { perception: { fire: { weaponTarget: { unit: target, generation: 1 } } } };
  assert.equal(botLockTarget({ controller }), target);
  assert.equal(botLockTarget({ controller: { perception: { fire: { weaponTarget: { unit: target, generation: 0 } } } } }), null);
  assert.equal(botLockTarget({ controller: null }), null);
  for (const human of [{ human: true }, { networkHuman: true }, { combat: { player: true } }]) assert.equal(botLockTarget({ ...human, controller }), undefined);
  assert.equal(botLockTarget(null), undefined);

});

test('a seated bot hands over its seat controller\'s target, and only what the launcher may lock', () => {
  const tank = {id:3,generation:1,alive:true};
  // In a seat the bot's own controller is idle; the seat's (SeatUnit) picks targets.
  const seat = { perception: { fire: { weaponTarget: { unit: tank, generation: 1 } } } };
  const lease = { team: 1, unit: { controller: null }, controllable: { controller: seat } };
  assert.equal(seatLockTarget(lease), tank);
  assert.equal(seatLockTarget({ ...lease, controllable: null, unit: { controller: seat } }), tank);
  assert.equal(seatLockTarget({ ...lease, human: true }), undefined);
  assert.equal(seatLockTarget({ ...lease, unit: { networkHuman: true } }), undefined);

});
