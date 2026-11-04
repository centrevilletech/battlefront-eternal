import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

const content = process.env.BFE_CONTENT_DIR ?? 'public';
test('F2 keeps pooled identity through death and respawn and rejects retired handles', {
  skip: !existsSync(`${content}/maps/hot1/map.json`) && 'no game files', timeout: 180000,
}, async () => {
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('?map=hot1&side=all&era=gcw&seed=1&f2=rust&f2Debug=1');
  const vs = game.vehicles, f2 = vs.f2;
  assert.ok(f2);
  assert.equal(vs.sim, undefined, 'old per-call hull bridge is absent');
  const v = vs.vehicles.find(v => v.walking), target = v.aiTarget, position = v.position;
  const generation = target.generation, slot = v.simSlot, gen = v.simGen;
  v.damage(7);
  assert.equal(v.health, v.maxHealth - 7);
  v.setHealth(v.maxHealth);
  assert.equal(v.health, v.maxHealth);
  v.damage(Infinity);
  assert.equal(v.alive, false);
  assert.equal(v.slot.remaining, v.slot.desc.respawnTime);
  for (let tick = 1; tick < 3000 && !v.alive; tick++) vs.update(1 / 60);
  assert.ok(v.alive, 'the native spawn clock respawns the hull');
  assert.equal(v.aiTarget, target);
  assert.equal(v.position, position);
  assert.equal(target.generation, generation + 1);
  assert.equal(v.health, v.maxHealth);
  assert.equal(f2.x.f2_command(slot, gen, generation, 0, 10, 0, 0, 0), 0, 'stale gameplay life rejected');
  const flyer = vs.vehicles.find(v => v.flying), flyerPad = flyer.slot;
  f2.retire(flyer);
  vs.update(1 / 60);
  const fresh = flyerPad.vehicle;
  assert.notEqual(fresh, flyer);
  assert.ok(fresh.alive);
  assert.equal(fresh.flightClass, flyer.flightClass);
  assert.equal(fresh.minSpeed, flyer.minSpeed);
  assert.equal(fresh.power, 100);
  assert.ok(fresh.flightStart?.isVector3, 'a newly constructed flyer has its complete checkpoint');
  v.health--;
  assert.throws(() => f2.check(), /F2 mirror write: health/);
  v.health++;
  f2.retire(v);
  assert.equal(f2.x.f2_command(slot, gen, target.generation, 1, 10, 0, 0, 0), 0);
  vs.update(1 / 60);
  const replacement = v.slot.vehicle;
  assert.ok(replacement.alive);
  assert.notEqual(replacement, v);
  assert.equal(replacement.simSlot, slot);
  assert.notEqual(replacement.simGen, gen);
  assert.equal(f2.pool.at(slot, gen), v, 'retired reads preserve the old identity');
  assert.equal(f2.pool.at(replacement.simSlot, replacement.simGen), replacement);
  // Registry entries can outlive a retired hull. They must remain dead facts,
  // rather than asking targeting to resolve the replacement in the same slot.
  f2.foreignIn();
  const words = f2.bridge.f, start = f2.inputBase + words[f2.inputBase + f2.capacity - 1];
  let oldTarget = null;
  for (let i = 0; i < words[start]; i++) {
    const at = start + 1 + i * 10;
    if (words[at + 1] === target.id) oldTarget = [words[at], words[at + 3]];
  }
  assert.deepEqual(oldTarget, [2, 0], 'retired registry identity stays a dead foreign fact');
  assert.equal(f2.x.f2_command(slot, gen, target.generation, 1, 10, 0, 0, 0), 0, 'slot reuse cannot retarget the old identity');
});
