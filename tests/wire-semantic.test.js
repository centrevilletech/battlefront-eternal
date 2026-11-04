import test from 'node:test';
import assert from 'node:assert/strict';
import { Quaternion, Vector3 } from 'three';
import { loadSim } from '../src/sim/module.js';
import { createWireSemantic } from '../src/runtime/wire-semantic.js';
import { Weapon, Arsenal } from '../src/weapons.js';
import { recordOf, SHARED_TALLIES, FAVOURITES } from '../src/battle-stats.js';
import { captureWeaponState, decodeWeaponState, WEAPON_STATE_FLAGS, WEAPON_STATE_NUMBERS } from '../src/runtime/network-weapon-state.js';
import { captureArsenalWeapon } from '../src/runtime/network-arsenal-state.js';
import { captureCompactSoldier, captureCompactSoldierInto } from '../src/runtime/network-entity-soldier-compact.js';
import { captureCompactFlight } from '../src/runtime/network-entity-flight-compact.js';
import { captureGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { captureDroidekaState } from '../src/runtime/network-droideka-state.js';
import { captureMountedControl } from '../src/runtime/network-mounted-control.js';
import { captureOwnPlayer, OwnPlayerPrerequisiteError } from '../src/runtime/network-entity-player.js';
import { encodeRemoteCorrection } from '../src/runtime/network-remote-codec.js';
import { captureBattleSummary, capturePostSummary, captureSpawnSummary, captureRosterSummaries, captureRecordSummaries } from '../src/runtime/network-entity-summary.js';

const random = initial => { let seed = initial; return () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296); };
const definition = (channel = 0) => ({ class: `gun${channel}`, base: 'cannon', channel, RoundsPerClip: 10, ammo: 4,
  ShotDelay: 0.1, ReloadTime: 0.2, InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0,
  charges: [{ at: 0.25 }, { at: 0.75 }], ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 } });
const seat = () => ({ aimYaw: -0, pitch: 0.25, lookYaw: null, lookPitch: null, yawRate: undefined, lastYaw: -2 });
function soldier() {
  return { contactCount: 0, contactMean: new Vector3(), position: new Vector3(), velocity: new Vector3(), normal: new Vector3(0, 1, 0), settled: new Vector3(),
    posture: 'stand', air: null, grounded: true, diveFrom: 'stand', fuel: 1, aimYaw: 0, aimPitch: 0, dive: 0, stateTime: 0, lock: 0 };
}
const flightNumbers = 'health team yaw tilt roll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickCooldown trickPending gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS trickTime trickDirection'.split(' ');
const flightFlags = 'alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld'.split(' ');
function flight() {
  const v = { flying: true, flightConfig: {}, seats: [seat()], flightState: 'Flying', trick: '', flightLockState: '', flightWarning: '', landingMessage: '', landingReason: '' };
  for (const key of flightNumbers) v[key] = 0;
  for (const key of flightFlags) v[key] = false;
  for (const key of ['position', 'velocity', 'flightStart', 'flightEnd', 'flightTrickRight']) v[key] = new Vector3();
  for (const key of ['quaternion', 'flightStartQ', 'flightTrickLoopQ', 'flightTrickEndQ']) v[key] = new Quaternion();
  return v;
}
function ground() {
  const v = { flying: false, seats: [seat()], position: new Vector3(), velocity: new Vector3(), quaternion: new Quaternion(), alive: true, sleeping: false };
  for (const key of 'health team yaw tilt roll yawRate forwardInput rightInput tiltRate rollRate support'.split(' ')) v[key] = 0;
  return v;
}
function droideka() {
  const d = { clips: new Map(), clip() {}, toggleShield() {}, mode: 0, grounded: false, reverse: false, dead: false, guns: [{}, {}], gunBarrels: [[{}, {}], [{}]] };
  for (const key of 'aimYaw aimPitch stateTime time legs spin roll lean blastClock lookX lookY gun'.split(' ')) d[key] = 0;
  for (const key of ['position', 'velocity', 'avoidance', 'previous', 'settled']) d[key] = new Vector3();
  return d;
}
function ownFixture() {
  const arsenal = new Arsenal([definition(0), definition(1)]);
  const unit = { id: 42, generation: 3, team: 1, className: 'soldier', soldier: soldier(), walker: false, deadTime: 0, removed: false, seat: null,
    combat: { arsenal, health: 100, dead: false, deathTime: 0, kick: { rate: 0, hold: 0, offset: 0 }, zoomStep: 0, zoom: 1, firstPerson: false, gunUnderwater: false, remotes: null } };
  const registry = { contentEpoch: 8, classes: new Map([[1, 'soldier'], [2, 'pilot']]), classIds: new Map([['soldier', 1], ['pilot', 2]]),
    posts: new Map([[1, 'alpha'], [2, 'beta']]), weapons: new Map([[1, 'gun0'], [2, 'gun1']]), weaponIds: new Map([['gun0', 1], ['gun1', 2]]),
    inputCodes: ['KeyW', 'KeyA', 'Space', 'ShiftLeft', 'Mouse0'], bindingProfiles: new Map([[0, {}]]) };
  return { unit, registry, classId: 1, epoch: 12, tick: 38 };
}
function sameError(reference, native) {
  let expected;
  try { reference(); } catch (error) { expected = error; }
  assert.ok(expected, 'reference rejects input');
  assert.throws(native, error => error.message === expected.message && error.constructor === expected.constructor);
}

test('weapon and arsenal native bytes preserve defaults, level masks, signed zero, timers and melee', async () => {
  const instance = await loadSim(), native = createWireSemantic(instance), w = new Weapon(definition()), u = random(2003);
  try {
    for (let i = 0; i < 1100; i++) {
      for (const key of WEAPON_STATE_FLAGS) w[key] = u() < 0.5;
      for (const key of WEAPON_STATE_NUMBERS) w[key] = i % 3 ? (u() - 0.5) * 100 : (u() < 0.5 ? -0 : 0);
      w.reserve = i % 5 ? w.reserve : Infinity; w.yawSpreadOverride = i % 7 ? null : w.yawSpreadOverride;
      w.def = w.levels[i % w.levels.length];
      w.melee = i % 4 ? null : { captureState: () => ({ time: i % 12 ? -0 : NaN, index: i * 257, active: i % 2 === 0 }) };
      if (i % 149 === 0) instance.exports.memory.grow(1);
      assert.deepEqual(captureWeaponState(w, native), captureWeaponState(w), `full weapon ${i}`);
      const bytes = captureArsenalWeapon(w, native);
      assert.deepEqual(bytes, captureArsenalWeapon(w), `arsenal ${i}`);
      assert.equal(captureArsenalWeapon(w, native), bytes);
    }
    for (const key of WEAPON_STATE_FLAGS) { const old = w[key]; w[key] = 0; sameError(() => captureWeaponState(w), () => captureWeaponState(w, native)); w[key] = old; }
    for (const key of WEAPON_STATE_NUMBERS) {
      const old = w[key];
      for (const bad of [undefined, NaN, '0', -Infinity]) { w[key] = bad; sameError(() => captureWeaponState(w), () => captureWeaponState(w, native)); }
      w[key] = old;
    }
    w.def = {}; sameError(() => captureArsenalWeapon(w), () => captureArsenalWeapon(w, native));
  } finally { native.dispose(); }
  assert.throws(() => native.weapon(w), /disposed/);
});

test('compact body twins cover conditional posture, flight histories, seat tags and walker flags', async () => {
  const native = createWireSemantic(await loadSim()), u = random(912), s = soldier(), f = flight(), g = ground(), d = droideka();
  const into = { bytes: new Uint8Array(149) }; into.view = new DataView(into.bytes.buffer);
  try {
    for (let i = 0; i < 900; i++) {
      s.position.set((u() - 0.5) * 100, -0, u()); s.settled.copy(s.position); if (i % 3) s.settled.y = 0;
      s.velocity.set(u(), u(), u()); s.dive = i % 2 ? 0 : u(); s.diveFrom = ['stand', 'crouch', 'prone'][i % 3];
      s.posture = ['stand', 'crouch', 'prone'][i % 3]; s.air = [null, 'jump', 'fall', 'thrown', 'jet'][i % 5]; s.grounded = i % 2 === 0;
      s.fuel = u(); s.aimYaw = -u(); s.aimPitch = u(); s.stateTime = u() * 20; s.lock = i % 5;
      const expected = captureCompactSoldier(s);
      assert.deepEqual(captureCompactSoldier(s, native), expected, `soldier ${i}`);
      assert.deepEqual(captureCompactSoldierInto(s, into, native), expected);
      assert.equal(captureCompactSoldierInto(s, into, native).buffer, into.bytes.buffer);
      f.flightState = ['Landed', 'TakingOff', 'Flying', 'Landing', 'Destroyed'][i % 5]; f.trick = ['', 'barrel', 'loop', 'immelmann'][Math.floor(i / 5) % 4];
      f.flightLockState = ['', 'Locking', 'Locked'][i % 3];
      for (const key of flightNumbers) f[key] = (u() - 0.5) * 50;
      f.floorY = i % 3 ? NaN : -0;
      for (const key of flightFlags) f[key] = [true, false, undefined][Math.floor(u() * 3)];
      f.seats[0].lookYaw = i % 3 ? null : -0; f.seats[0].lookPitch = i % 2 ? undefined : u();
      assert.deepEqual(captureCompactFlight(f, native), captureCompactFlight(f), `flight ${i}`);
      g.yaw = f.yaw; g.position.copy(s.position); g.velocity.copy(s.velocity); g.seats[0] = f.seats[0];
      g.alive = i % 2 === 0; g.sleeping = i % 3 === 0;
      assert.deepEqual(captureGroundCorrection(g, native), captureGroundCorrection(g), `ground ${i}`);
      d.mode = i % 14; d.gun = i * 13; d.grounded = i % 2 === 0; d.dead = i % 3 === 0; d.reverse = i % 5 === 0;
      d.shield = i % 3 ? { on: i % 2 === 0, value: u() * 100, max: 100 } : null;
      d.lastGun = i % 4 ? { index: i % 2, barrel: i % 2 ? 0 : i % 2 } : undefined;
      d.position.copy(s.position); d.velocity.copy(s.velocity); d.aimYaw = f.yaw; d.time = u() * 100;
      assert.deepEqual(captureDroidekaState(d, native), captureDroidekaState(d), `walker ${i}`);
    }
    f.trick = ''; f.flightState = 'Flying'; f.flightStart = null; f.flightTrickRight = null; f.flightTrickLoopQ = null;
    assert.deepEqual(captureCompactFlight(f, native), captureCompactFlight(f), 'unused histories are not required');
    for (const bad of [undefined, null, '0', Infinity]) { f.floorY = bad; sameError(() => captureCompactFlight(f), () => captureCompactFlight(f, native)); }
    f.floorY = NaN;
    for (const bad of [undefined, '0', 1]) { s.contactCount = bad; sameError(() => captureCompactSoldier(s), () => captureCompactSoldier(s, native)); }
    s.contactCount = 0; s.posture = 'unknown'; sameError(() => captureCompactSoldier(s), () => captureCompactSoldier(s, native));
    d.gun = Number.MAX_SAFE_INTEGER + 1; sameError(() => captureDroidekaState(d), () => captureDroidekaState(d, native));
    g.alive = 1; sameError(() => captureGroundCorrection(g), () => captureGroundCorrection(g, native));
  } finally { native.dispose(); }
});

test('optional semantic hooks preserve legacy callback and ignored-argument use', () => {
  const weapons = [new Weapon(definition(0)), new Weapon(definition(1))], soldiers = [soldier(), soldier()];
  assert.deepEqual(weapons.map(captureWeaponState), weapons.map(w => captureWeaponState(w)));
  assert.deepEqual(weapons.map(captureArsenalWeapon), weapons.map(w => captureArsenalWeapon(w)));
  assert.deepEqual(soldiers.map(captureCompactSoldier), soldiers.map(s => captureCompactSoldier(s)));
  assert.deepEqual(captureCompactSoldier(soldiers[0], { previous: true }), captureCompactSoldier(soldiers[0]));
});

test('mounted pilot, passenger and turret weapon checkpoints match their live encoders', async () => {
  const native = createWireSemantic(await loadSim()), w = new Weapon(definition());
  const vehicle = { aiTarget: { id: 91, generation: 4 }, seats: [{ weapons: [{ state: w, barrel: 12 }] }, { ...seat(), weapons: [{ state: w, barrel: 9 }] }] };
  const turret = { aiTarget: { id: 92, generation: 5 }, yaw: -0, pitch: 0.1, yawVel: 0.2, pitchVel: 0.3, gunYaw: 0.4, gunPitch: 0.5, weapon: w, barrel: 19 };
  try {
    for (const lease of [{ vehicle, index: 0 }, { vehicle, index: 1 }, { turret }]) {
      const unit = { seat: lease };
      for (let i = 0; i < 100; i++) {
        vehicle.seats[1].lookYaw = i % 2 ? null : -0; vehicle.seats[1].lookPitch = i % 3 ? null : -0.8;
        w.wait = i / 100; w.heat = i / 300; w.hidden = i % 2 === 0;
        assert.deepEqual(captureMountedControl(unit, native), captureMountedControl(unit));
      }
    }
    vehicle.seats[1].weapons[0].barrel = -1; const unit = { seat: { vehicle, index: 1 } };
    sameError(() => captureMountedControl(unit), () => captureMountedControl(unit, native));
  } finally { native.dispose(); }
});

test('own-player native envelope composes bodies, mounted weapons, input view and cached bytes', async () => {
  const native = createWireSemantic(await loadSim()), base = ownFixture(), { unit } = base;
  try {
    for (let i = 0; i < 600; i++) {
      unit.team = i % 2 + 1; unit.combat.health = 100 - i / 100; unit.combat.dead = i % 7 === 0; unit.deadTime = i % 9 / 5;
      unit.combat.arsenal.current(0).wait = i % 10 / 60; unit.combat.arsenal.current(1).reserve = Infinity;
      unit.soldier.dive = i % 3 ? 0 : 0.7; unit.soldier.settled.y = i % 2 ? -0 : 0;
      const held = i % 3 ? ['KeyW', 'Mouse0', 'KeyW'] : [];
      const view = i % 2 ? {} : { invertMouse: true, firstPerson: false, mouseScale: 0.002, aspect: 16 / 9, viewTick: i,
        flightSettings: { invert: false, sensitivity: 0.7, curve: 1.4, bindings: {} },
        gamepad: { axes: [0.4, -0, -0.9], buttons: [{ pressed: true }, {}, { pressed: true }] } };
      const options = { ...base, tick: i, discardedInput: i * 7, held, view };
      const actual = captureOwnPlayer({ ...options, native });
      assert.deepEqual(actual, captureOwnPlayer(options), `own ${i}`);
      assert.equal(captureOwnPlayer({ ...options, native }).payload, actual.payload);
    }
    for (const vehicle of [ground(), flight()]) {
      vehicle.aiTarget = { id: 512, generation: 4 }; vehicle.seats[0].weapons = [{ state: new Weapon(definition()), barrel: 0 }];
      unit.seat = { vehicle, index: 0 }; const options = { ...base, vehicle, vehicleId: 512, vehicleGeneration: 4 };
      assert.deepEqual(captureOwnPlayer({ ...options, native }), captureOwnPlayer(options));
    }
    unit.seat = null; unit.walker = true; unit.soldier = droideka();
    assert.deepEqual(captureOwnPlayer({ ...base, native }), captureOwnPlayer(base));
    sameError(() => captureOwnPlayer({ ...base, held: ['missing'] }), () => captureOwnPlayer({ ...base, held: ['missing'], native }));
    assert.throws(() => captureOwnPlayer({ ...base, held: ['missing'], native }), OwnPlayerPrerequisiteError);
  } finally { native.dispose(); }
});

test('recon native correction preserves selected weapons, salvo and camera with error parity', async () => {
  const native = createWireSemantic(await loadSim()), arsenal = new Arsenal([definition(0), { ...definition(1), strike: {}, SalvoCount: 3 }]), droid = { arsenal };
  const state = { selected: [0, 0], zoomStep: 1, dead: false, piloted: true, firstPerson: false, yaw: -0, pitch: 0.5, health: 1, countdown: 42, lookX: 0, lookY: 0,
    randomSeed: 0xffffffff, position: [-0, 2, 3], velocity: [1, 0, -3], weapons: arsenal.weapons.map(w => decodeWeaponState(captureWeaponState(w))), salvo: null, camera: null };
  try {
    for (let i = 0; i < 300; i++) {
      state.weapons[0].wait = -i / 100; state.firstPerson = i % 2 === 0;
      state.salvo = i % 3 ? null : { def: arsenal.weapons[1].def, left: i % 4, wait: -0, at: new Vector3(1, 2, 3), x: new Vector3(1, 0, 0), y: new Vector3(0, 1, 0), z: new Vector3(0, 0, 1) };
      state.camera = i % 2 ? null : { position: [1, 2, -0], quaternion: [0, 0, 0, 1], aspect: 16 / 9, fov: 50, reach: 99, fresh: i % 3 === 0 };
      assert.deepEqual(encodeRemoteCorrection(state, droid, native), encodeRemoteCorrection(state, droid), `recon ${i}`);
    }
    for (const change of [{ zoomStep: 2 }, { selected: [99, 0] }, { dead: true }, { position: [0, 0] },
      { camera: { position: [1, 2, 3], quaternion: [0, 0, 1], aspect: 1, fresh: false } },
      { camera: { position: [1, 2, 3], quaternion: [0, 0, 0, 1], aspect: 1, fresh: 2 } }]) {
      sameError(() => encodeRemoteCorrection({ ...state, ...change }, droid), () => encodeRemoteCorrection({ ...state, ...change }, droid, native));
    }
  } finally { native.dispose(); }
});

test('own recon mode composes the live captured controller and exact nested weapon checkpoint', async () => {
  const native = createWireSemantic(await loadSim()), base = ownFixture(), { unit, registry } = base;
  const arsenal = new Arsenal([definition(0), { ...definition(1), strike: {}, SalvoCount: 2 }]);
  const droid = { def: { base: 'droid' }, arsenal, yaw: -0, pitch: 0.6, health: 0.8, dead: false, countdown: 37 };
  for (const key of ['position', 'velocity', 'eye', 'aimDir', 'target']) droid[key] = new Vector3(0, 2, -3);
  droid.salvo = { def: arsenal.current(1).def, left: 2, wait: 0.5, at: new Vector3(1, 0, 1), x: new Vector3(1, 0, 0), y: new Vector3(0, 1, 0), z: new Vector3(0, 0, 1) };
  const remote = { random: { seed: 0xf0123456 }, piloted: droid, lookX: -0, lookY: 0.07, zoomStep: 1, firstPerson: true,
    simulationView: { fov: 60, reach: 300, fresh: true }, aimCamera: { position: new Vector3(0, 2, 0), quaternion: new Quaternion(), aspect: 1.6 },
    identityFor: () => ({ id: 17, generation: 5 }) };
  unit.combat.remotes = remote; registry.combat = { definitionId: () => 23 };
  try {
    for (let i = 0; i < 160; i++) {
      remote.random.seed = i * 671; remote.firstPerson = i % 2 === 0; remote.simulationView.fresh = i % 3 === 0;
      arsenal.current(0).heat = i / 300; arsenal.current(1).wait = -i / 60; droid.salvo.left = i % 3;
      const options = { ...base, tick: i, held: ['Mouse0'], view: { gamepad: { axes: [-0, 0, 0.5, -0.2] } } };
      assert.deepEqual(captureOwnPlayer({ ...options, native }), captureOwnPlayer(options), `nested own recon ${i}`);
    }
    remote.identityFor = () => ({ id: 4096, generation: 5 });
    sameError(() => captureOwnPlayer(base), () => captureOwnPlayer({ ...base, native }));
  } finally { native.dispose(); }
});

test('battle, post and spawn native summaries preserve scalar order and nullable fields', async () => {
  const native = createWireSemantic(await loadSim()), { registry } = ownFixture(), u = random(632);
  const battle = { reinforcements: { 2: 200, 1: 100 }, start: { 1: 100, 2: 200 }, noPost: {}, bleed: {}, over: false, gameOver: false, finished: false, losing: false, loser: null, overTime: 0, victoryClock: null, countdown: 1 };
  const post = { name: 'alpha', team: 1, toward: 2, progress: 0, neutralize: 0, contested: false, active: true, counts: new Map([[2, 5], [1, 17]]) };
  const player = { unit: { id: 19, generation: 3 }, team: 1, spawn: { phase: 'alive', className: 'soldier', post: 'alpha', spawnedOnce: true, ended: false, deadTime: 0 } };
  try {
    for (let i = 0; i < 700; i++) {
      const options = { registry, tick: i, epoch: 31, eligiblePosts: i % 2 ? ['alpha', 'beta'] : [], eligibleClasses: ['soldier', 'pilot'], slot: [0, 254, 255, -1, undefined][i % 5] };
      battle.reinforcements[1] = u() * 100; battle.overTime = -0; battle.victoryClock = i % 2 ? null : u(); battle.countdown = i % 3 ? 10 : null;
      battle.over = i % 2 === 0; battle.loser = i % 3; post.progress = u(); post.contested = i % 2 === 0; post.active = i % 4 ? undefined : false;
      player.spawn.phase = ['unspawned', 'alive', 'dying', 'awaiting'][i % 4]; player.spawn.deadTime = u() * 10;
      player.spawn.pending = i % 3 ? null : { className: 'pilot', post: null }; player.spawn.auto = i % 2 ? null : { team: 2, className: 'soldier', post: 'beta' };
      for (const [capture, value] of [[captureBattleSummary, battle], [capturePostSummary, post], [captureSpawnSummary, player]]) {
        const actual = capture(value, { ...options, native }); assert.deepEqual(actual, capture(value, options));
        assert.equal(capture(value, { ...options, tick: i + 1, native }).payload, actual.payload);
      }
    }
    post.contested = 1; sameError(() => capturePostSummary(post, { registry }), () => capturePostSummary(post, { registry, native }));
    battle.countdown = undefined; sameError(() => captureBattleSummary(battle, {}), () => captureBattleSummary(battle, { native }));
    player.spawn.phase = 'unknown'; const options = { registry, eligiblePosts: [], eligibleClasses: [] };
    sameError(() => captureSpawnSummary(player, options), () => captureSpawnSummary(player, { ...options, native }));
  } finally { native.dispose(); }
});

test('native roster chunks preserve coercions, RTT rounding and truncated Unicode bytes', async () => {
  const native = createWireSemantic(await loadSim()), u = random(273), players = new Map(), characters = [], names = ['雪😀\ud800\udfff', '', 'a'.repeat(31) + '😀', 'é'.repeat(23), null, 427];
  for (let i = 0; i < 151; i++) {
    const c = { name: names[i % names.length], team: i % 3 ? i % 256 : 4294967297, human: i % 4 === 0 };
    for (const key of ['kills', 'teamKills', 'deaths', 'captures']) c[key] = [undefined, -1, 65536, 1.5, 4294967298, '23', NaN][Math.floor(u() * 7)];
    if (i % 5) players.set(c, { link: { rtt: [0, -0, -3, 1.5, 65535, Infinity, NaN, '4'][i % 8], path: { kind: ['relay', 'direct', 'peer', 1, { toString: () => 'relay' }][i % 5], family: ['IPv4', 'IPv6', 6, ''][i % 4] } } });
    characters.push(c);
  }
  try {
    const options = { tick: 19, epoch: 8 }, expected = captureRosterSummaries({ characters }, players, options);
    const actual = captureRosterSummaries({ characters }, players, { ...options, native });
    assert.deepEqual(actual, expected); assert.equal(actual.length, 13);
    const again = captureRosterSummaries({ characters }, players, { ...options, tick: 20, native });
    actual.forEach((record, i) => assert.equal(again[i].payload, record.payload));
    sameError(() => captureRosterSummaries({ characters: Array(2401).fill(characters[0]) }, players, options),
      () => captureRosterSummaries({ characters: Array(2401).fill(characters[0]) }, players, { ...options, native }));
  } finally { native.dispose(); }
});

test('native record summaries select favourites, filter victims and quantize bounded chunks', async () => {
  const native = createWireSemantic(await loadSim()), { registry } = ownFixture(), u = random(3309), characters = Array.from({ length: 107 }, (_, i) => ({ name: `person${i}` }));
  const missing = {}, battle = { characters };
  for (let i = 0; i < characters.length; i++) {
    if (i % 7 === 0) continue;
    const record = recordOf(characters[i]);
    for (const [slot, s] of record.slots.entries()) {
      if (i % 3 === slot) continue;
      for (const key of SHARED_TALLIES) s[key] = key.endsWith('Best') ? u() * 10000 : Math.floor(u() * 80000);
      for (const victim of [missing, characters[0], characters[i], characters[(i + 1) % characters.length]]) s.victims.set(victim, i % 3 ? i : 350);
      for (const key of FAVOURITES) {
        const ids = key === 'weapons' ? ['gun0', 'gun1'] : ['soldier', 'pilot'];
        s[key].push({ key: ids[0], label: 'first', time: 13.0009 }, { key: ids[1], label: 'tied', time: 13.0009 });
        if (i % 2) s[key].push({ key: 'unknown', label: 'missing id', time: 15 });
      }
    }
  }
  try {
    for (const quantum of [1, 1000, '1000']) {
      const options = { tick: 123, epoch: 7, quantum }, expected = captureRecordSummaries(battle, registry, options), actual = captureRecordSummaries(battle, registry, { ...options, native });
      assert.deepEqual(actual, expected); assert.ok(actual.length > 2);
      for (const record of actual) assert.ok(record.payload.length <= 900);
      for (const only of [characters[0], characters[1], characters[106], missing]) {
        const own = { ...options, only, id: 79999 };
        assert.deepEqual(captureRecordSummaries(battle, registry, { ...own, native }), captureRecordSummaries(battle, registry, own));
      }
    }
    const all = Array.from({ length: 260 }, () => ({})), large = recordOf(all[0]);
    for (const s of large.slots) for (const victim of all) s.victims.set(victim, 255);
    const options = { tick: 1, epoch: 1 };
    assert.deepEqual(captureRecordSummaries({ characters: all }, registry, { ...options, native }), captureRecordSummaries({ characters: all }, registry, options), 'oversized entries are omitted');
    const coerced = [{}, {}, {}];
    recordOf(coerced[0]).slots[0].teamKills = '0';
    recordOf(coerced[1]).slots[0].lifeBest = '13.9999';
    recordOf(coerced[2]).slots[1].deaths = true;
    recordOf(coerced[2]).slots[1].victims.set(coerced[0], '17');
    assert.deepEqual(captureRecordSummaries({ characters: coerced }, registry, { ...options, native }), captureRecordSummaries({ characters: coerced }, registry, options), 'raw truthiness precedes numeric tally coercion');
    recordOf(characters[1]).slots[0].headshots = -1;
    sameError(() => captureRecordSummaries(battle, registry, options), () => captureRecordSummaries(battle, registry, { ...options, native }));
  } finally { native.dispose(); }
});
