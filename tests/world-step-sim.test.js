import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import * as THREE from 'three';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

const source = readFileSync(new URL('../src/game.js', import.meta.url), 'utf8');
const begin = source.indexOf('  const stepSquads = '), end = source.indexOf('  // Soldiers push each other apart');
const code = source.slice(begin, end);
assert.ok(code.includes('function step(dt, tick, recordedCommand = null)'));

function fixture(world, flags = 0, changed = () => {}) {
  const trace = [], down = new Set(['squadMoveOut', 'squadFollow', 'use']);
  let api;
  const position = new THREE.Vector3(1, 2, 3);
  const env = { THREE, game: { worldSim: world, serverWorld: Boolean(flags & 16) },
    walker: Boolean(flags & 8), firstPerson: false, activeCommand: null, tickHuman: null, tickLookYaw: 0,
    previewYaw: 2, previewPitch: 3, simTime: 0, simulationZoom: 0, simulationEye: new THREE.Vector3(),
    input: { forward: 0, right: 0, turn: 0 }, droidInput: { forward: 0, right: 0, turn: 0 },
    playerUnit: { id: 1, generation: 3, tickCount: 4, removed: Boolean(flags & 16384), walker: Boolean(flags & 8), position },
    soldier: { position, aimYaw: 0.4, aimPitch: 2, posture: 'crouch', air: null, velocity: new THREE.Vector3() },
    combat: { dead: Boolean(flags & 2), usesEyeView: true, zoom: 2, pivot: position, origin: position },
    presentation: { previousYaw: 0.5 }, FLIGHT_CLASSES: { fighter: { acceleration: 2 } }, simEvents: { time: 0 },
    served: [], supplied: { id: 'player-supply' }, mineTargets: [], itemTargets: [{ id: 'player-target' }],
    bodyMiddle: new THREE.Vector3(), BODY_MIDDLE: 0.9, waterContact: {}, battle: { characters: [] },
  };
  const view = () => ({ input: { ...env.input }, tick: env.playerUnit.tickCount, dead: env.combat.dead,
    aim: [env.soldier.aimYaw, env.soldier.aimPitch], remote: env.remotes.active, driver: env.driver.active,
    gunner: env.gunner.active, time: env.simEvents.time, lexical: api?.inspect() });
  const call = (name, ...args) => { trace.push([name, args, view()]); changed(name, env, api); };
  const cmd = { id: 17, look: [[3, -4], [-2, 1]], fire: [{ pressed: true, id: 3 }, { pressed: false, id: 4 }],
    firstPerson: true, aspect: 1.5, human: [{ kind: 'spawn' }], flightSettings: { sensitivity: 0.75, invert: true },
    flightTuning: { fighter: { acceleration: 4, unknown: 10 } } };
  const commands = new Set([cmd]);
  env.commandHistory = { epoch: 1, put(command) { assert.ok(commands.has(command)); call('history', command.id); } };
  env.controls = { command(tick, epoch, generation) { call('command', tick, epoch, generation); return cmd; } };
  env.runtimeProfile = { step: dt => call('profile', dt) };
  env.worldControls = { accept(command) { assert.ok(commands.has(command)); call('accept', command.id); return !env.stale; },
    axis(name) { call(`axis:${name}`); return { forward: -0.75, right: 0.25, turn: 0.125 }[name]; },
    take(name) { call(`take:${name}`); return down.delete(name); } };
  env.applyLook = (dx, dy) => { call('look', dx, dy); env.soldier.aimYaw += dx / 2048; env.soldier.aimPitch += dy / 2048; };
  env.setPerspective = value => { call('perspective', value); api.perspective(value); };
  env.simulationCamera = { aspect: 1, updateMatrixWorld: () => call('matrix') };
  env.simulationView = { applyFov: aspect => call('fov', aspect),
    update(dt, at, yaw, pitch, posture, zoom, eyes, eye) { call('camera', dt, at.toArray(), yaw, pitch, posture, zoom, eyes, eye?.toArray() ?? null); } };
  env.combat.eye = (at, out) => { call('eye'); out.copy(at); };
  env.combat.update = (dt, controls, movement) => { assert.equal(controls, env.worldControls); call('combat', dt, movement); };
  env.combat.flushMuzzles = () => call('flushMuzzles');
  env.combat.damage = amount => { call('damage', amount); env.combat.dead = true; };
  env.soldier.sampleMuzzles = dt => call('muzzles', dt);
  env.soldier.pitchRange = () => { call('pitchRange'); return [-0.6, 0.8]; };
  env.remotes = { active: Boolean(flags & 4), updateCamera: (...args) => call('remoteCamera', ...args),
    update(dt, input, controls) { assert.equal(controls, env.worldControls); call('remotes', dt, input ? { ...input } : null); } };
  env.deathView = { active: Boolean(flags & 32), update: (dt, at, killer) => call('deathCamera', dt, at?.toArray() ?? null, killer), apply: () => call('deathApply') };
  env.killerEyes = () => { call('killer'); return 'killer'; };
  const hull = { alive: true, seatIndex: 0, itemTarget: { id: 'hull-target' } };
  env.driver = { active: Boolean(flags & 64), vehicle: flags & 64 ? hull : null,
    aimView: { update: (v, dt, first) => call('driverCamera', v === hull, dt, first) }, update: dt => call('driver', dt) };
  env.gunner = { active: Boolean(flags & 128), updateCamera: (...args) => call('gunnerCamera', ...args), update: dt => call('gunner', dt) };
  env.vehicles = flags & 2048 ? null : { vehicles: [hull, { alive: false }],
    sim: { retune: (...args) => call('retune', ...args) }, restoreTickPose: () => call('restorePose'),
    update(dt, v, keys) { call('vehicles', dt, v === hull, keys ?? null); } };
  env.seatInput = (v, index, controls, command) => { assert.ok(commands.has(command)); call('seatInput', index, command.id); return { forward: 1 }; };
  env.hud = { squadCommand: label => call('squadHud', label) };
  env.ai = flags & 1024 ? null : { update: dt => call('ai', dt), playerCommand: kind => call('squad', kind),
    grenadeUpdate: (g, owner) => call(`grenade:${g.id}`, owner === env.playerUnit ? 'player' : owner) };
  env.toggleTurret = () => call('toggleTurret'); env.stepPlayer = dt => call('player', dt);
  env.water = flags & 1 ? { step: dt => call('water', dt), soldierStep(dt, contact, at) {
    call('waterSoldier', dt, at.toArray()); contact.gunUnder = true; return 0.125;
  } } : null;
  env.presentEffect = (name, at) => call('effect', name, at.toArray());
  env.droidekaWater = (body, combat, dt) => call('waterDroideka', dt, body.position.toArray());
  env.inDeathRegion = at => { call('region', at.toArray()); return at.x === 9; };
  env.factory = { built: [0, 1].map(i => ({ alive: true, seat: null, held: false, walker: Boolean(i),
    position: new THREE.Vector3(i ? 9 : 8, 0, 0), velocity: new THREE.Vector3(),
    soldier: { position: new THREE.Vector3(i ? 9 : 8, 0, 0) },
    combat: { pivot: position, origin: position, damage: amount => call(`unitDamage:${i}`, amount) },
    tick: dt => call(`unit:${i}`, dt), supply: { id: `supply:${i}` }, itemTarget: { id: `target:${i}` } })) };
  env.unitsSim = flags & 256 ? { phase: dt => call('nativeUnits', dt) } : null;
  env.turretSeats = flags & 4096 ? null : { update: dt => call('turretSeats', dt) };
  env.networkRemotes = flags & 8192 ? null : { finish: (tick, dt) => call('networkRemotes', tick, dt) };
  env.separateSoldiers = () => call('separate');
  env.live = flags & 32768 ? null : { update: (dt, served, tick) => call('live', dt, served.map(x => x.id), tick) };
  env.ordnance = { update: (dt, predicted) => call('ordnance', dt, predicted),
    grenades: [{ id: 1, owner: 'player', gone: false }, { id: 2, owner: 'bot', gone: true }] };
  env.items = { update: (dt, targets) => call('items', dt, targets.map(x => x.id)) };
  env.battlefield = { update: dt => call('battlefield', dt) };
  env.updateBattle = dt => call('battle', dt); env.updateTargetHud = dt => call('target', dt);
  env.humanSpawn = { flush: () => call('spawnFlush') };
  api = Function(...Object.keys(env), `${code}\nreturn { step, inspect: () => ({ firstPerson, tickHuman, tickLookYaw,
    previousYaw: presentation.previousYaw, previewYaw, previewPitch, simTime, simulationZoom }), perspective: value => { firstPerson = value; } };`)(...Object.values(env));
  return { env, api, trace, command: cmd, commands, view };
}

const run = (f, command = f.command, tick = 17) => { try { f.api.step(1 / 60, tick, command); return null; } catch (e) { return e.message; } };

test('native phase order twins the actual game step across camera, actor, seat and server guards', async () => {
  const world = createWorldSim(await loadSim());
  for (let flags = 0; flags < 65536; flags += 113) {
    const js = fixture(null, flags), native = fixture(world, flags);
    assert.equal(run(native), run(js), `error ${flags}`);
    assert.deepEqual(native.trace, js.trace, `trace ${flags}`);
    assert.deepEqual(native.view(), js.view(), `state ${flags}`);
  }
});

test('native phase guards observe mutations and newly appended grenades from earlier callbacks', async () => {
  const world = createWorldSim(await loadSim());
  const mutations = [
    (op, e) => { if (op === 'take:squadMoveOut') e.combat.dead = true; },
    (op, e) => { if (op === 'player') e.remotes.active = true; },
    (op, e) => { if (op === 'eye') { e.deathView.active = true; e.playerUnit.removed = true; } },
    (op, e) => { if (op === 'ai') { e.driver.active = true; e.gunner.active = true; } },
    (op, e) => { if (op === 'vehicles') e.driver.active = false; },
    (op, e) => { if (op === 'combat') e.game.serverWorld = true; },
    (op, e) => { if (op === 'pitchRange') e.soldier.aimPitch = -8; },
    (op, e) => { if (op === 'items') { e.combat.dead = false; e.gunner.active = e.driver.active = false; } },
    (op, e) => { if (op === 'grenade:1') e.ordnance.grenades.push({ id: 3, owner: 'bot', gone: false }); },
  ];
  for (const changed of mutations) for (const flags of [0, 1, 64, 8 + 1, 256 + 16, 256 + 1]) {
    const js = fixture(null, flags, changed), native = fixture(world, flags, changed);
    assert.equal(run(native, null), run(js, null));
    assert.deepEqual(native.trace, js.trace); assert.deepEqual(native.view(), js.view());
  }
});

test('native world phases retain partial state and iterator cleanup on thrown callbacks', async () => {
  const world = createWorldSim(await loadSim());
  for (const failed of ['accept', 'look', 'water', 'take:use', 'eye', 'ai', 'nativeUnits', 'vehicles', 'combat', 'pitchRange', 'items', 'battle', 'target', 'grenade:1', 'spawnFlush']) {
    const changed = (op, env) => { if (op === failed) { env.soldier.aimPitch = 0.25; throw Error(`failed:${op}`); } };
    const js = fixture(null, 257, changed), native = fixture(world, 257, changed);
    const closed = [];
    for (const [i, f] of [js, native].entries()) {
      const records = f.env.ordnance.grenades;
      records[Symbol.iterator] = function () { let at = 0; return { next: () => at < this.length ? { value: this[at++], done: false } : { done: true },
        return: () => { closed.push(i); throw Error('close also failed'); } }; };
    }
    assert.equal(run(native), run(js)); assert.deepEqual(native.trace, js.trace); assert.deepEqual(native.view(), js.view());
    assert.equal(closed.length, failed === 'grenade:1' ? 2 : 0);
  }
});

test('native world steps preserve per-call commands through nested ticks and memory growth', async () => {
  const instance = await loadSim(), world = createWorldSim(instance);
  const exercise = native => {
    let nested = false;
    const f = fixture(native ? world : null, 64 + 256, (op, env, api) => {
      if (op === 'ai' && !nested) {
        nested = true;
        const command = { ...f.command, id: 19, look: [[10, 1]], firstPerson: false };
        f.commands.add(command); api.step(1 / 120, 19, command);
        if (native) instance.exports.memory.grow(1);
      }
    });
    assert.equal(run(f), null);
    assert.deepEqual(f.trace.filter(([name]) => name === 'seatInput').map(([, args]) => args[1]), [19, 17]);
    return { trace: f.trace, state: f.view() };
  };
  assert.deepEqual(exercise(true), exercise(false));
});
