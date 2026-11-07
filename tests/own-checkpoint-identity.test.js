import test from 'node:test';
import assert from 'node:assert/strict';
import { Soldier } from './helpers/body.js';
import { PlayerCombat } from '../src/combat.js';
import { Arsenal } from '../src/weapons.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { prepareMelee } from '../src/heroes.js';
import { captureArsenalWeapon } from '../src/runtime/network-arsenal-state.js';
import { captureCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { captureOwnPlayer } from '../src/runtime/network-entity-player.js';
import { mountFixture } from './mount-fixture.js';
import { encodeOwnDatagram } from '../src/runtime/snapshot-own.js';
import { encodeWordDelta, encodeWordDeltaView, encodeWorldSection, encodeWorldSectionView, encodeEntities, worldFieldKey } from '../src/runtime/snapshot-codec.js';
import * as reference from './fixtures/own-checkpoint-reference.js';

// The own checkpoint is captured for every player every snapshot. Its
// writer, its weapon fields and its soldier vectors are now written out by
// name, but every checkpoint must be exactly the frozen reference's
// (fixtures/own-checkpoint-reference.js), byte for byte, and every invalid
// state must fail with the same error.
function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const pick = (r, list) => list[Math.floor(r() * list.length)];
const outcome = capture => { try { const out = capture(); return { bytes: [...(out.payload ?? out)] }; } catch (error) { return { error: `${error.constructor.name}: ${error.message}` }; } };

const defs = [
  { class: 'rifle', channel: 0, RoundsPerClip: 20, ammo: 3, ZoomMin: 2, ZoomMax: 4, ShotDelay: .2, base: 'cannon', charges: [{ ShotDelay: .4 }] },
  { class: 'grenade', channel: 1, RoundsPerClip: 1, ammo: 4, base: 'grenade' },
  { class: 'blade', channel: 1, base: 'melee', RoundsPerClip: 1, ammo: 1, swings: [{ duration: 1, hit: .3, sound: null }, { duration: .5, hit: .2, sound: null }] },
];
const NUMBERS = ['timer', 'clip', 'reserve', 'heat', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime', 'held', 'charge', 'pattern', 'salvosLeft', 'salvoWait', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'limit'];
const FLAGS = ['deployed', 'firing', 'overheated', 'released', 'hidden', 'charging', 'lockComplete'];
// Values each field may take, valid or not: -0 differs from 0 on the wire.
const ODD = [0, -0, 1, 0.1, 1e-300, -2.5, 1e9, Infinity, -Infinity, NaN, null, undefined, '1'];

test('weapon checkpoints: the reference bytes for every value, and its errors for bad ones', () => {
  const r = rng(7);
  for (let round = 0; round < 4; round++) {
    const weapons = prepareMelee(new Arsenal(defs, { random: () => .5 })).weapons;
    for (let i = 0; i < 1500; i++) {
      const weapon = pick(r, weapons), roll = r();
      // A bad value is put back once compared, as the world would never leave it.
      const restore = [];
      const set = (key, value) => { restore.push([key, weapon[key]]); weapon[key] = value; };
      if (roll < 0.5) set(pick(r, NUMBERS), r() < 0.7 ? Math.fround(r() * 40 - 10) : pick(r, ODD));
      else if (roll < 0.8) set(pick(r, FLAGS), r() < 0.9 ? r() < 0.5 : pick(r, ODD));
      else if (roll < 0.9 && weapon.levels.length > 1) weapon.def = pick(r, weapon.levels);
      else if (weapon.melee) weapon.melee.time = r();
      // Twice: a second capture of an unchanged weapon comes from each one's cache.
      for (let n = 0; n < 2; n++) assert.deepEqual(outcome(() => captureArsenalWeapon(weapon)), outcome(() => reference.captureArsenalWeapon(weapon)), `round ${round} step ${i}`);
      if (restore.some(([key]) => !Number.isFinite(weapon[key]) && typeof weapon[key] !== 'boolean')) for (const [key, value] of restore.reverse()) weapon[key] = value;
    }
  }
});

const plane = () => new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
test('soldier checkpoints: the reference bytes, settled or not, diving or not, -0 and all', () => {
  const r = rng(8), soldier = new Soldier(plane(), {});
  soldier.spawn(0, 0, 0, 0);
  const value = () => r() < 0.8 ? (r() - 0.5) * 50 : pick(r, [0, -0, 1e-300, NaN, Infinity]);
  for (let i = 0; i < 3000; i++) {
    const roll = r();
    if (roll < 0.3) soldier.position.set(value(), value(), value());
    else if (roll < 0.45) soldier.settled.copy(soldier.position);
    else if (roll < 0.55) { soldier.settled.copy(soldier.position); const axis = pick(r, ['x', 'y', 'z']); soldier.settled[axis] = r() < 0.5 ? -soldier.settled[axis] : soldier.settled[axis] + 1; }
    else if (roll < 0.65) soldier.velocity.set(value(), value(), value());
    else if (roll < 0.7) soldier.normal.set(value(), 1, value());
    else if (roll < 0.85) soldier[pick(r, ['fuel', 'aimYaw', 'aimPitch', 'stateTime', 'lock'])] = value();
    else if (roll < 0.9) { soldier.dive = pick(r, [0, 0, 1, -1]); soldier.diveFrom = pick(r, ['stand', 'crouch', 'prone', 'lying']); }
    else if (roll < 0.95) soldier.posture = pick(r, ['stand', 'crouch', 'prone', 'sit']);
    else soldier.air = pick(r, [null, 'jump', 'fall', 'thrown', 'jet', 'swim']);
    assert.deepEqual(outcome(() => captureCompactSoldier(soldier)), outcome(() => reference.captureCompactSoldier(soldier)), `step ${i}`);
    // A bad value is put back once compared.
    for (const key of ['fuel', 'aimYaw', 'aimPitch', 'stateTime', 'lock']) if (!Number.isFinite(soldier[key])) soldier[key] = 0;
    for (const vector of [soldier.position, soldier.velocity, soldier.normal, soldier.settled]) for (const axis of ['x', 'y', 'z']) if (!Number.isFinite(vector[axis])) vector[axis] = 0;
    if (!['stand', 'crouch', 'prone'].includes(soldier.posture)) soldier.posture = 'stand';
    if (![null, 'jump', 'fall', 'thrown', 'jet'].includes(soldier.air)) soldier.air = null;
    if (!['stand', 'crouch', 'prone'].includes(soldier.diveFrom)) soldier.diveFrom = 'stand';
  }
});

const registry = { contentEpoch: 42, classes: new Map([[1, 'trooper']]), weapons: new Map([[1, 'rifle'], [2, 'grenade'], [3, 'blade']]), inputCodes: ['KeyW', 'KeyS', 'Mouse0', 'ShiftLeft'], bindingProfiles: new Map([[0, {}]]) };
function unit() {
  const soldier = new Soldier(plane(), {}); soldier.spawn(0, 0, 0, 0);
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { arsenal: prepareMelee(new Arsenal(defs, { player: true })), health: 97, zoomStep: 1, firstPerson: true, soldier, kick: { rate: .4, hold: .8, offset: .1 }, dead: false, deathTime: 0 });
  return { id: 1, generation: 7, team: 1, className: 'trooper', soldier, combat };
}
test('player checkpoints: the reference bytes on foot and in a gunner seat, with every view option', () => {
  const r = rng(9), u = unit(), hull = mountFixture().v;
  hull.aiTarget = { id: 20, generation: 2 };
  for (let i = 0; i < 1500; i++) {
    const roll = r(), c = u.combat;
    if (roll < 0.2) u.soldier.position.set((r() - 0.5) * 99, r() * 3, (r() - 0.5) * 99);
    else if (roll < 0.3) c.health = pick(r, [97, 0, -0, 12.5, 1e6]);
    else if (roll < 0.35) { c.dead = r() < 0.3; c.deathTime = r() * 4; u.deadTime = pick(r, [undefined, 0, 2.5]); u.removed = r() < 0.1; }
    else if (roll < 0.45) c.kick[pick(r, ['rate', 'hold', 'offset'])] = pick(r, [0, -0, r(), NaN]);
    else if (roll < 0.5) { c.firstPerson = r() < 0.5; c.gunUnderwater = r() < 0.2; }
    else if (roll < 0.6) c.arsenal.weapons[Math.floor(r() * 3)].heat = r() < 0.9 ? r() : NaN;
    else if (roll < 0.7) c.arsenal.selected[1] = Math.floor(r() * 2);
    else if (roll < 0.8) u.seat = u.seat ? null : { vehicle: hull, index: 1 };
    else if (roll < 0.85) u.team = pick(r, [1, 2, 1, 3]);
    else if (roll < 0.9) c.zoomStep = pick(r, [0, 1, 0, 1, 256, -1, 1.5]);
    const view = { invertMouse: r() < 0.5, firstPerson: r() < 0.5 };
    if (r() < 0.5) Object.assign(view, { mouseScale: 1.25, aspect: 16 / 9, viewTick: Math.floor(r() * 1000) });
    if (r() < 0.4) view.flightSettings = { sensitivity: r() + 0.5, invert: r() < 0.5, curve: 1.35, bindings: {} };
    if (r() < 0.3) view.gamepad = { axes: [r() - 0.5, -.3, .4, pick(r, [0, -0, .5])], buttons: Array.from({ length: 16 }, (_, b) => ({ pressed: b === Math.floor(r() * 16) })) };
    const held = ['KeyW', 'KeyS', 'Mouse0', 'ShiftLeft'].filter(() => r() < 0.4).concat(r() < 0.02 ? ['KeyZ'] : []);
    const options = { unit: u, tick: 100 + i, epoch: 3, registry, classId: 1, held, view, discardedInput: r() < 0.95 ? Math.floor(r() * 3) : pick(r, [-1, 0.5, 2 ** 32, 2 ** 32 - 1]) };
    assert.deepEqual(outcome(() => captureOwnPlayer(options)), outcome(() => reference.captureOwnPlayer(options)), `step ${i}`);
    // A bad value is put back once compared.
    for (const key of ['rate', 'hold', 'offset']) if (!Number.isFinite(c.kick[key])) c.kick[key] = 0;
    for (const weapon of c.arsenal.weapons) if (!Number.isFinite(weapon.heat)) weapon.heat = 0;
    if (u.team === 3) u.team = 1;
    if (!(c.zoomStep >= 0 && c.zoomStep <= 1 && Number.isInteger(c.zoomStep))) c.zoomStep = 1;
  }
});

// The host passes the correction's delta and the summaries' section as views
// of the codec's scratch (encodeWordDeltaView, encodeWorldSectionView); the
// datagram is written into scratch of its own and copied out once.
test('own datagrams: the reference bytes for every mix of sections, from views or fresh encodings', () => {
  const r = rng(10), bytes = n => Uint8Array.from({ length: n }, () => Math.floor(r() * 256));
  const world = () => { const map = new Map(); for (let id = 1; id <= Math.floor(r() * 6); id++) { const field = { id, schema: 2 + (id % 3), generation: 1, payload: bytes(1 + Math.floor(r() * 120)) }; map.set(worldFieldKey(field), field); } return map; };
  for (let i = 0; i < 2000; i++) {
    const payload = bytes(80 + Math.floor(r() * 500)), baseline = r() < 0.6 ? payload.map(b => r() < 0.1 ? b ^ 1 : b) : r() < 0.5 ? null : bytes(payload.length + 8);
    const correction = r() < 0.8 ? { id: Math.floor(r() * 100), generation: pick(r, [1, 7, 2 ** 32 - 1, 2 ** 32]), schema: 11, payload } : null;
    const summaries = r() < 0.7 ? world() : null, baselineSummaries = summaries && r() < 0.7 ? world() : null;
    const options = { epoch: 1, tick: 100 + i, interval: pick(r, [1, 2]), baselineTick: Math.floor(r() * 100), ackInput: Math.floor(r() * 1000), correction,
      cosmeticBytes: r() < 0.5 ? bytes(Math.floor(r() * (r() < 0.1 ? 1200 : 400))) : null, second: r() < 0.2 };
    if (!options.second) options.ownParts = pick(r, [1, 2]);
    const fresh = { ...options, baselinePayload: baseline, summaries, baselineSummaries };
    const expected = outcome(() => reference.encodeOwnDatagram(fresh));
    assert.deepEqual(outcome(() => encodeOwnDatagram(fresh)), expected, `step ${i}: fresh`);
    // As the host builds it: delta and section precomputed as views.
    const viewed = { ...options, correctionDelta: correction ? encodeWordDeltaView(payload, baseline) : null, summariesSection: summaries ? encodeWorldSectionView(summaries, baselineSummaries) : null };
    const precomputed = { ...options, correctionDelta: correction ? encodeWordDelta(payload, baseline) : null, summariesSection: summaries ? encodeWorldSection(summaries, baselineSummaries) : null };
    assert.deepEqual(outcome(() => encodeOwnDatagram(viewed)), outcome(() => reference.encodeOwnDatagram(precomputed)), `step ${i}: views`);
  }
  // A view holds until the next call of the same one, whatever else encodes
  // in between (the host encodes entity parts and cosmetics meanwhile).
  const a = world(), b = world(), payload = bytes(300), delta = encodeWordDeltaView(payload, null), section = encodeWorldSectionView(a, null);
  encodeWordDelta(bytes(200), null); encodeWorldSection(b, null);
  encodeEntities({ tick: 2, epoch: 1, slots: new Map(), world: b }, null);
  assert.deepEqual([...delta], [...encodeWordDelta(payload, null)]);
  assert.deepEqual([...section], [...encodeWorldSection(a, null)]);
});
