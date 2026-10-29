import test from 'node:test';
import assert from 'node:assert/strict';
import { Arsenal } from '../src/weapons.js';
import { prepareMelee } from '../src/heroes.js';
import { captureArsenalWeapon, validateArsenalWeapon } from '../src/runtime/network-arsenal-state.js';

const definition = (name, channel = 0, extra = {}) => ({ class: name, base: 'cannon', channel, RoundsPerClip: 10, ammo: 4, ShotDelay: .1, ReloadTime: .1, InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 }, ...extra });
function weapons() {
  return prepareMelee(new Arsenal([definition('rifle'), definition('blade', 1, { base: 'melee', swings: [{ duration: 1, hit: .3, sound: null }] })], { random: () => .5 })).weapons;
}

test('repeated arsenal captures stay byte-stable and track every mutation', () => {
  const [gun, blade] = weapons();
  const first = captureArsenalWeapon(gun);
  assert.deepEqual(captureArsenalWeapon(gun), first, 'unchanged weapon re-captures identically');

  gun.heat = .375;
  const heated = captureArsenalWeapon(gun);
  assert.notDeepEqual(heated, first, 'a number change must reach the checkpoint');
  assert.equal(validateArsenalWeapon(gun, heated).state.heat, .375);
  gun.heat = 0;
  assert.deepEqual(captureArsenalWeapon(gun), first, 'reverting restores the original bytes');

  gun.overheated = true;
  assert.equal(validateArsenalWeapon(gun, captureArsenalWeapon(gun)).state.overheated, true, 'a flag change must reach the checkpoint');
  gun.overheated = false;
  assert.deepEqual(captureArsenalWeapon(gun), first);

  const bladeFirst = captureArsenalWeapon(blade);
  blade.update(1 / 60, true, { pressed: true, held: true });
  const swung = captureArsenalWeapon(blade);
  assert.notDeepEqual(swung, bladeFirst, 'melee timeline changes must reach the checkpoint');
  assert.deepEqual(validateArsenalWeapon(blade, swung).melee, blade.melee.captureState());
});

test('arsenal capture still rejects invalid state after a good capture', () => {
  const [gun] = weapons();
  captureArsenalWeapon(gun);
  gun.timer = Number.NaN;
  assert.throws(() => captureArsenalWeapon(gun), /invalid weapon state number/);
  gun.timer = 0; gun.firing = 'yes';
  assert.throws(() => captureArsenalWeapon(gun), /invalid weapon state flag/);
});
