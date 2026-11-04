import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3, Quaternion } from 'three';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { EntityWorldCapture } from '../src/runtime/network-entity-capture.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';

function fixture(bridge = null, nested = null) {
  const definitionCalls = [], weapon = { name: 'weapon', charging: true, held: 0.1, levels: [{ MaxPressedTime: 1 }] };
  const registry = { classes: new Map([[1, 'unit'], [2, 'vehicle']]), weapons: new Map([[3, 'weapon']]),
    combat: { definitionId(def) { nested?.(); definitionCalls.push(def.id); return def.id; } } };
  const unit = id => ({ id, generation: 1, className: 'unit', team: 1, dead: false, removed: false,
    soldier: { position: new Vector3(id, 2, 3), velocity: new Vector3(0, -0, 0), aimYaw: -0, aimPitch: 0.3,
      posture: 'stand', air: null, dive: 0, mode: 0, reverse: false, dead: false, shield: { on: true, value: 10 }, time: 0, roll: -0, lean: 0.1, legs: 0.2 },
    combat: { health: 200, arsenal: { current() { return weapon; } } }, seat: null });
  const units = Array.from({ length: 5 }, (_, i) => unit(i + 1)); units[4].walker = true;
  const vehicle = { aiTarget: { id: 100, generation: 2 }, name: 'vehicle', team: 2, alive: true, dying: 0,
    position: new Vector3(20, -10, 3), velocity: new Vector3(0, 0.2, 31.75), quaternion: new Quaternion(), health: 500,
    flightState: '', power: 3.5, slot: {}, tilt: -0, roll: -0, walkPhase: 0, walkBlend: NaN, seats: [{ aimYaw: 0, pitch: -0 }] };
  const turret = { aiTarget: { id: 200, generation: 3, team: 2 }, state: 'built', health: 80, yaw: NaN, pitch: 0.2, gunYaw: -0.2, gunPitch: -0 };
  const item = id => ({ def: { id }, gone: false, position: new Vector3(0, 0, 0), velocity: new Vector3(0.01, -0.02, 0), up: new Vector3(0, 1, 0), yaw: 0.3 });
  const items = [item(4), item(5)];
  const droid = { def: { id: 6 }, dead: false, team: 2, health: 20.5, position: new Vector3(2, 3, 4), velocity: new Vector3(), yaw: 0.2, pitch: -0.5 };
  const remote = { slots: [{ droid, generation: 1 }], droids() { return this.slots.filter(s => s.droid); }, identityFor(droid) { return this.slots.find(s => s.droid === droid); } };
  const game = { units: { all: () => units }, items: { pools: { mine: 2, powerupitem: 1 }, items }, vehicles: { vehicles: [vehicle], slots: [vehicle.slot] },
    turretSeats: { turrets: [turret] }, live: { buildings: [turret] }, networkRemotes: remote };
  const capture = new EntityWorldCapture(game, registry, { bridge, fireCounter: u => u.id * 3 });
  return { game, capture, units, vehicle, turret, items, item, droid, remote, weapon, definitionCalls };
}
function step(f, tick) {
  for (let i = 0; i < f.units.length; i++) {
    const u = f.units[i], s = u.soldier;
    u.dead = tick % 19 === i; u.removed = tick % 23 === i; s.air = tick % 3 === 0 ? 'thrown' : tick % 2 ? '' : 'jump';
    s.dive = tick % 13 === i ? -1 : 0; s.posture = ['stand', 'crouch', 'prone'][tick % 3];
    u.combat.health = tick % 7 ? 40.5 : -5; s.position.x += 0.002;
    s.time = tick / 60; s.shield.on = !!(tick % 2); s.shield.value = tick % 13; s.dead = u.dead;
    u.seat = i < 2 && tick % 7 ? { vehicle: f.vehicle, index: i } : null;
    if (tick === 81) u.generation++;
  }
  f.weapon.held = tick / 50; f.weapon.charging = tick % 11 !== 0;
  f.vehicle.alive = tick % 17 !== 0; f.vehicle.dying = tick % 17 === 0 ? 0.231 : 0;
  f.vehicle.walkPhase = tick / 4; f.vehicle.seats[0].aimYaw = tick / 100;
  f.turret.health = tick % 5 ? 25.5 : NaN; f.turret.state = tick % 31 ? 'built' : 'unknown';
  for (const item of f.items) item.position.x += 0.00001;
  f.items[0].gone = tick % 9 === 0;
  if (tick === 40) f.items[1] = f.item(7);
  if (tick % 15 === 0) f.items[1].velocity.x += 0.1;
  f.droid.dead = tick % 8 < 3; f.droid.health = tick % 11 ? 21.5 : -1;
  if (tick === 75) f.remote.slots[0].generation++;
}

test('native semantic capture keeps entity, seat, item and remote publication decisions', async () => {
  const native = fixture(bridgeFor(await loadSim())), js = fixture();
  for (let tick = 1; tick <= 180; tick++) {
    step(native, tick); step(js, tick);
    assert.deepEqual(native.capture.capture(tick), js.capture.capture(tick), `capture ${tick}`);
    assert.deepEqual(native.definitionCalls, js.definitionCalls, `definition callbacks ${tick}`);
    assert.equal(native.capture.leaseSerial, js.capture.leaseSerial);
  }
  native.capture.dispose(); assert.throws(() => native.capture.capture(181), /disposed/);
});

test('native item raw shortcuts retain old samples and survive reentrant registry lookups', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  let count = 0;
  const native = fixture(bridge, () => { wire.encodeWordDelta(new Uint8Array([3, 7, 9])); if (count++ === 0) instance.exports.memory.grow(1); }), js = fixture();
  const initial = native.capture.capture(1), reference = js.capture.capture(1);
  assert.deepEqual(initial, reference);
  for (const f of [native, js]) f.items[0].position.x += 0.000001;
  assert.deepEqual(native.capture.capture(2), js.capture.capture(2));
  assert.equal(native.capture.items.capture(3)[0].tick, 1);
  assert.equal(native.definitionCalls.length, js.definitionCalls.length, 'the unchanged item capture does not look up definitions');
  native.capture.dispose();
});
