import test from 'node:test';
import assert from 'node:assert/strict';
import { Turret } from '../src/props.js';
import { VehicleDriver } from '../src/vehicles.js';
import { previewSteer } from '../src/gunner.js';
import { Weapon } from '../src/weapons.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
import { mountFixture } from './mount-fixture.js';
import { captureMountedControl, decodeMountedControl, restoreMountedControl } from '../src/runtime/network-mounted-control.js';

function passenger() {
  const { v } = mountFixture();
  v.aiTarget = { id: 7, generation: 3 };
  v.seatIndex = 1;
  return { unit: { seat: { vehicle: v, index: 1 } }, v, seat: v.seats[1] };
}
function stationary() {
  const turret = { aiTarget: { id: 9, generation: 2 }, yaw: .32, pitch: -.12,
    yawVel: .64, pitchVel: -.16, gunYaw: .03, gunPitch: -.11,
    turnRate: 2.85, filter: 10, pitchLimits: [-.7, .7], steer: Turret.prototype.steer };
  return { unit: { seat: { turret } }, turret };
}

test('mounted weapon heat, salvo eligibility and next barrel resume from exact shared Weapon state', () => {
  const source = passenger(), client = passenger();
  const definition = { class: 'mounted-cannon', base: 'cannon', channel: 0, RoundsPerClip: 25,
    ammo: 0, ShotDelay: .13, SalvoCount: 3, ShotsPerSalvo: 1, SalvoDelay: .07, HeatPerShot: .2, HeatRecoverRate: .1 };
  const make = () => [0, 1].map(channel => ({ state: new Weapon({ ...definition, channel }, { random: () => .5 }), barrel: 0 }));
  source.seat.weapons = make(); client.seat.weapons = make();
  for (let tick = 0; tick < 13; tick++) for (const entry of source.seat.weapons) {
    entry.barrel += entry.state.update(1 / 60, true, { held: true, pressed: tick === 0 }, false, { stick: 0, posture: 'stand', zoomed: false }).length;
  }
  client.seat.weapons[0].state.heat = .99;
  const payload = captureMountedControl(source.unit);
  assert.ok(payload.length <= 352, 'two full mutable weapons and nullable aim fit a bounded block');
  restoreMountedControl(client.unit, payload);
  for (let tick = 0; tick < 40; tick++) for (let channel = 0; channel < 2; channel++) {
    const a = source.seat.weapons[channel], b = client.seat.weapons[channel];
    const input = { held: tick < 25, pressed: false };
    const shots = a.state.update(1 / 60, true, input, false, { stick: 0, posture: 'stand', zoomed: false });
    const replayed = b.state.update(1 / 60, true, input, false, { stick: 0, posture: 'stand', zoomed: false });
    assert.ok(replayed.every(shot => shot.weapon === b.state));
    const values = list => list.map(({ weapon, ...shot }) => shot);
    assert.deepEqual(values(replayed), values(shots));
    a.barrel += shots.length; b.barrel += shots.length;
    assert.deepEqual(captureWeaponState(b.state), captureWeaponState(a.state)); assert.equal(b.barrel, a.barrel);
  }
  const before = captureMountedControl(client.unit), malformed = payload.slice();
  // The second weapon's unsupported charge level must not apply the first.
  malformed[malformed.length - decodeMountedControl(payload).weapons.at(-1).payload.length] = 255;
  assert.throws(() => restoreMountedControl(client.unit, malformed), /definition mismatch/);
  assert.deepEqual(captureMountedControl(client.unit), before);
});

test('passenger baseline preserves nullable look overrides and preview follows shared driver aim', () => {
  const source = passenger(), client = passenger();
  Object.assign(source.seat, { aimYaw: .12, pitch: .04, lookYaw: .14, lookPitch: .05 });
  const payload = captureMountedControl(source.unit);
  assert.equal(payload.length, 44);
  restoreMountedControl(client.unit, payload);
  const driver = new VehicleDriver({}); driver.vehicle = source.v;
  const page = new VehicleDriver({}); page.vehicle = client.v;
  const commands = [{ look: [[12, 8]], mouseScale: .5, invertMouse: false },
    { look: [[-4, 20]], mouseScale: .25, invertMouse: true }];
  for (const command of commands) {
    for (const [dx, dy] of command.look) driver.look(dx, dy, command.mouseScale, command.invertMouse);
    source.v.aimPlayerSeat(1 / 60);
  }
  const before = captureMountedControl(client.unit);
  page.preview(commands, null, .5, false);
  assert.equal(page.previewYaw, source.seat.aimYaw);
  assert.equal(page.previewPitch, source.seat.pitch);
  assert.deepEqual(captureMountedControl(client.unit), before, 'preview leaves authoritative aim unchanged');
  for (const [lookYaw, lookPitch] of [[null, null], [null, .1], [.2, null]]) {
    Object.assign(source.seat, { lookYaw, lookPitch });
    restoreMountedControl(client.unit, captureMountedControl(source.unit));
    assert.equal(client.seat.lookYaw, lookYaw); assert.equal(client.seat.lookPitch, lookPitch);
  }
});

test('turret baseline restores filtered steering velocity before shared pending-input replay', () => {
  const source = stationary(), client = stationary();
  Object.assign(client.turret, { yaw: -1, yawVel: 0, pitchVel: 0, gunPitch: 0 });
  const payload = captureMountedControl(source.unit);
  assert.equal(payload.length, 60);
  restoreMountedControl(client.unit, payload);
  const commands = [{ look: [[8, 3]], mouseScale: .5, invertMouse: false },
    { look: [], mouseScale: .5, invertMouse: false },
    { look: [[-6, 4]], mouseScale: .25, invertMouse: true }];
  for (const command of commands) {
    let x = 0, y = 0;
    for (const [dx, dy] of command.look) {
      x += dx * command.mouseScale; y += (command.invertMouse ? dy : -dy) * command.mouseScale;
    }
    source.turret.steer(1 / 60, -x * 60 / 2048, y * 60 / 2048, 1);
  }
  const before = captureMountedControl(client.unit);
  const predicted = previewSteer(client.turret, { ...client.turret }, commands);
  for (const key of ['yaw', 'pitch', 'yawVel', 'pitchVel']) assert.equal(predicted[key], source.turret[key]);
  assert.equal(client.turret.gunPitch, -.11);
  assert.deepEqual(captureMountedControl(client.unit), before);
});

test('stale lease, malformed float, truncation and trailing bytes cannot partly restore mounted aim', () => {
  const source = passenger(), client = passenger(), payload = captureMountedControl(source.unit);
  client.seat.aimYaw = .75;
  const before = captureMountedControl(client.unit);
  for (const offset of [1, 5, 9]) {
    const bad = payload.slice(); bad[offset]++;
    assert.throws(() => restoreMountedControl(client.unit, bad), /lease mismatch/);
    assert.deepEqual(captureMountedControl(client.unit), before);
  }
  for (let length = 0; length < payload.length; length++) assert.throws(() => restoreMountedControl(client.unit, payload.slice(0, length)));
  const nonfinite = payload.slice(); new DataView(nonfinite.buffer).setFloat64(11, NaN, true);
  assert.throws(() => restoreMountedControl(client.unit, nonfinite), /nonfinite/);
  assert.throws(() => restoreMountedControl(client.unit, new Uint8Array([...payload, 0])), /length/);
  assert.deepEqual(captureMountedControl(client.unit), before);
  client.unit.seat.index = 99;
  assert.throws(() => captureMountedControl(client.unit), /prerequisite/);
});

test('turret mode rejects unknown flags, passenger index, and nonfinite final fields atomically', () => {
  const { unit } = stationary(), bytes = captureMountedControl(unit), before = captureMountedControl(unit);
  for (const [offset, value] of [[0, 5], [9, 1], [10, 1]]) {
    const bad = bytes.slice(); bad[offset] = value;
    assert.throws(() => restoreMountedControl(unit, bad), /invalid mounted/);
  }
  const bad = bytes.slice(); new DataView(bad.buffer).setFloat64(bad.length - 9, Infinity, true);
  assert.throws(() => restoreMountedControl(unit, bad), /nonfinite/);
  assert.deepEqual(captureMountedControl(unit), before);
  const padded = new Uint8Array(bytes.length + 7); padded.set(bytes, 3);
  assert.deepEqual(decodeMountedControl(padded.subarray(3, bytes.length + 3)), decodeMountedControl(bytes));
});
