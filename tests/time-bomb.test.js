import test from 'node:test';
import assert from 'node:assert/strict';
import { Scene, Vector3, Object3D } from 'three';
import { Items } from '../src/items.js';

// The time bomb (the Wookiee smuggler's remote charge dispenser throws a
// "mine" item with LifeSpan 5 and an ExplosionExpire): when its time runs
// out it sets off its explosion with its thrower's credit (src/items.js
// expire). It used to vanish without a blast. Numbers are the retail
// all_weap_inf_remote_charge_dispenser_ord's.
const BLAST = { class: 'timebomb_exp', base: 'explosion', Damage: 2000, DamageRadiusInner: 2.5, DamageRadiusOuter: 3.5, Effect: 'timebomb', PersonScale: 1, VehicleScale: 1.5 };
const POP = { class: 'pop', base: 'explosion', Effect: 'smaller_explosion' };
const BOMB = { class: 'timebomb', base: 'mine', GeometryName: 'bomb', LifeSpan: 5, MaxHealth: 100, Gravity: 1, Rebound: 0, Friction: 1,
  ExplosionTrigger: BLAST, ExplosionExpire: BLAST, ExplosionDeath: POP };
const LANDMINE = { class: 'landmine', base: 'mine', GeometryName: 'mine', TriggerRadius: 1, MaxHealth: 200, ExplosionTrigger: BLAST, ExplosionDeath: POP };
const PICKUP = { class: 'pickup', base: 'powerupitem', GeometryName: 'pickup', LifeSpan: 30 };
const DT = 1 / 60;

function fixture() {
  const blasts = [];
  // A floor at y = 0 for anything falling onto it.
  const collision = { cast(from, dir, far, hit) {
    if (dir.y >= 0 || from.y < 0) return null;
    const t = from.y / -dir.y;
    if (t > far) return null;
    hit.distance = t; hit.point.copy(from).addScaledVector(dir, t); hit.normal.set(0, 1, 0);
    return hit;
  } };
  const items = new Items({ scene: new Scene(), collision, effects: {}, lights: { add() {} }, model: () => () => new Object3D(),
    explode: (explosion, position, owner) => blasts.push({ explosion, position: position.clone(), owner }) });
  return { items, blasts };
}
const run = (items, seconds) => { for (let i = 0; i < Math.round(seconds * 60); i++) items.update(DT, []); };

test('a time bomb goes off with its ExplosionExpire when its LifeSpan runs out, its thrower credited', () => {
  const { items, blasts } = fixture(), thrower = { team: 1, generation: 1 };
  const bomb = items.dispense('dispenser', 3, BOMB, new Vector3(0, 1.5, 0), new Vector3(0, 0, 4), 0, thrower, 1);
  run(items, 4.9);
  assert.equal(blasts.length, 0, 'still ticking at 4.9 s');
  assert.ok(bomb.landed, 'it came to rest on the floor');
  run(items, 0.2);
  assert.equal(blasts.length, 1, 'gone off once its 5 s are up');
  assert.equal(blasts[0].explosion, BLAST);
  assert.equal(blasts[0].owner, thrower);
  assert.ok(blasts[0].position.distanceTo(bomb.position) < 1e-9, 'where it lay');
  assert.equal(bomb.gone, true);
  assert.equal(items.items.length, 0);
  run(items, 1);
  assert.equal(blasts.length, 1, 'once only');
});

test('a time bomb destroyed before its time pops harmlessly and never goes off', () => {
  const { items, blasts } = fixture();
  const bomb = items.dispense('dispenser', 3, BOMB, new Vector3(0, 0.2, 0), new Vector3(), 0, null, 1);
  run(items, 1);
  items.hurt(bomb, 150);
  assert.deepEqual(blasts.map((b) => b.explosion), [POP]);
  run(items, 6);
  assert.deepEqual(blasts.map((b) => b.explosion), [POP]);
});

test('items without an expiry explosion still just go away; a land mine has no timer', () => {
  const { items, blasts } = fixture();
  const pickup = items.spawn(PICKUP, new Vector3(0, 0.2, 0), new Vector3(), 0);
  const mine = items.spawn(LANDMINE, new Vector3(3, 0.2, 0), new Vector3(), 0);
  run(items, 31);
  assert.equal(pickup.gone, true);
  assert.notEqual(mine.gone, true);
  assert.equal(blasts.length, 0);
});

test('a thrower over MaxItems loses his oldest bomb as a pop, not a blast', () => {
  const { items, blasts } = fixture(), thrower = { team: 1, generation: 1 };
  const first = items.dispense('dispenser', 1, BOMB, new Vector3(0, 0.2, 0), new Vector3(), 0, thrower, 1);
  items.dispense('dispenser', 1, BOMB, new Vector3(2, 0.2, 0), new Vector3(), 0, thrower, 1);
  assert.equal(first.gone, true);
  assert.deepEqual(blasts.map((b) => b.explosion), [POP]);
});
