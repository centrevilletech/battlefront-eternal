// Compare native timing/window decisions in the actual Soldier, mailbox and
// replay loop. Deterministic network scenarios are correctness checks only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, basename, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { measurePrediction } from './prediction-timing.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { selectMission } from '../src/mission.js';
import { loadSim } from '../src/sim/module.js';
import { createReconcileSim } from '../src/runtime/reconcile-sim.js';
import { PredictionWindow, retainPredictionCommands } from '../src/runtime/prediction-window.js';
import { InputLead, advanceInputClock, replayTicks } from '../src/runtime/reconcile.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { InputMailbox } from '../src/runtime/transport.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { Soldier } from '../src/soldier.js';
import { CollisionWorld } from '../src/collision.js';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'explicit source revision required');
const requestedCorpus = option('corpus') ? resolve(option('corpus')) : null;
const corpusPath = requestedCorpus ? join(realpathSync(dirname(requestedCorpus)), basename(requestedCorpus)) : null;
if (corpusPath) {
  const within = relative(realpathSync(root), corpusPath);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'client corpus must stay outside the checkout');
  for (let directory = dirname(corpusPath); ; directory = dirname(directory)) {
    assert.equal(existsSync(join(directory, '.git')), false, 'client corpus must stay outside repositories');
    if (directory === dirname(directory)) break;
  }
  assert.equal(existsSync(corpusPath), false); assert.equal(existsSync(corpusPath + '.manifest.json'), false);
}
const descriptor = corpusPath ? openSync(corpusPath, 'wx') : null;
const corpusHash = createHash('sha256'), opcodes = {}, cases = [];
let label = '', corpusBytes = 0, operations = 0;
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
const native = createReconcileSim(await loadSim(), { trace(event) {
  assert.equal(event.error, null, 'valid client timing operation');
  const line = JSON.stringify({ label, op: event.op, input: words(event.input), output: words(event.output), error: null }) + '\n';
  corpusHash.update(line); corpusBytes += Buffer.byteLength(line); operations++;
  opcodes[event.op] = (opcodes[event.op] || 0) + 1;
  if (descriptor !== null) writeSync(descriptor, line);
} });
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function plane() {
  return new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]),
    new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
}
function measured(options, native) {
  const commands = createHash('sha256'), bodies = createHash('sha256'); let commandCount = 0, physicsTicks = 0;
  const result = measurePrediction({ ...options, native, observe(kind, value, context) {
    if (kind === 'command') { commands.update(JSON.stringify([context, value])); commandCount++; }
    else { bodies.update(captureCompactSoldier(value)); physicsTicks++; }
  } });
  assert.ok(commandCount > 200 && physicsTicks > 200, 'actual clocks and bodies advanced');
  return { result, commandCount, physicsTicks, commandSha256: commands.digest('hex'), bodySha256: bodies.digest('hex') };
}
function blackout({ fps, stall }, native) {
  const world = plane(), authority = new Soldier(world, {}), local = new Soldier(world, {}), checker = new Soldier(world, {});
  for (const soldier of [authority, local, checker]) soldier.spawn(0, 0, 0, 0);
  const window = new PredictionWindow(120, { native }), clock = new FixedClock(), lead = new InputLead({ maxLead: 24, extrapolate: true }, { native });
  let mailbox = new InputMailbox(), serverEpoch = 1, serverGeneration = 1, serverIdentity = 17, epoch = 1, generation = 1, identity = 17;
  let pending = [], held = [], sequence = 0, latest = 0, applied = 0, serverTick = 0, nextFrame = 1 / fps, lastFrame = 0;
  let saved = null, oldState = null, predicted = false, staleInjected = false, maxPending = 0, maxReplay = 0, frames = 0, exactCalls = 0, exactMatches = 0;
  const toServer = [], toClient = [], commands = createHash('sha256'), bodies = createHash('sha256'), checkpoints = createHash('sha256');
  let physicsTicks = 0;
  const step = (soldier, command) => {
    soldier.tick({ forward: command.down.includes('KeyW') ? 1 : 0, right: 0, turn: 0, jump: false, crouch: false, prone: false }, 1 / 60);
    bodies.update(captureCompactSoldier(soldier)); physicsTicks++;
  };
  const queue = (list, item) => { list.push(item); list.sort((a, b) => a.at - b.at); };
  lead.arrive(0, 0);
  for (let micro = 1; micro <= 10 * 7200; micro++) {
    const now = micro / 7200;
    while (toServer.length && toServer[0].at <= now) mailbox.push(toServer.shift().command);
    if (micro % 120 === 0) {
      serverTick++;
      if (serverTick === 430) { serverGeneration = 2; authority.spawn(10, 0, 0, 0); }
      if (serverTick === 500) serverIdentity = 90;
      if (serverTick === 540) { serverEpoch = 2; mailbox = new InputMailbox(serverEpoch); }
      step(authority, mailbox.take(serverTick, serverGeneration));
      if (serverTick === 60) oldState = captureCompactSoldier(authority);
      if (serverTick % 3 === 0 && (serverTick < 60 || serverTick >= 360)) queue(toClient, {
        at: now + .05, tick: serverTick, epoch: serverEpoch, generation: serverGeneration, identity: serverIdentity,
        state: captureCompactSoldier(authority), ack: mailbox.ack, ackTick: mailbox.ackTick,
      });
    }
    if (now + 1e-12 < nextFrame) continue;
    if (stall && now >= 3.2 && now < 3.7) { nextFrame = 3.7; continue; }
    nextFrame += 1 / fps;
    const dt = now - lastFrame; lastFrame = now; frames++;
    if (predicted) restoreCompactSoldier(local, saved);
    while (toClient.length && toClient[0].at <= now) {
      const packet = toClient.shift(); restoreCompactSoldier(local, packet.state);
      latest = applied = packet.tick; epoch = packet.epoch; generation = packet.generation; identity = packet.identity;
      lead.arrive(packet.tick, now);
      while (pending.length && pending[0].sequence <= packet.ack) {
        const command = pending.shift(); held = command.down;
        if (command.sequence === packet.ack) lead.acknowledge(packet.ackTick - command.tick, 1, command.capped);
      }
    }
    if (serverTick >= 300 && !staleInjected) { restoreCompactSoldier(local, oldState); applied = 60; staleInjected = true; }
    const target = lead.target(now, latest, .1);
    advanceInputClock(clock, target, dt, (_dt, tick) => {
      const command = { epoch, generation, sequence: ++sequence, tick, down: now >= 1.5 && now < 5 ? ['KeyW'] : [], actions: [], look: [], capped: lead.capped };
      commands.update(JSON.stringify([now, command])); pending.push(command);
      const before = pending;
      pending = retainPredictionCommands(pending, tick, generation, epoch, 120, { native });
      assert.ok(pending.every(command => before.includes(command)), 'retained command identities');
      queue(toServer, { at: now + .05, command });
    }, target + 7, { lock: .9 });
    maxPending = Math.max(maxPending, pending.length);
    saved = captureCompactSoldier(local);
    const context = { source: local, identity, epoch, generation, authorityTick: applied, pending, held };
    const previousCheckpoint = window.checkpoint;
    const selected = window.select(context, checkpoint => {
      exactCalls++; restoreCompactSoldier(checker, saved);
      const last = replayTicks(pending.filter(c => c.tick <= checkpoint.tick), applied, generation, held,
        (_tick, command) => checker.tick({ forward: command.down.includes('KeyW') ? 1 : 0, right: 0, turn: 0, jump: false, crouch: false, prone: false }, 1 / 60));
      const bytes = captureCompactSoldier(checker);
      const same = last === checkpoint.tick && bytes.length === checkpoint.state.length && bytes.every((byte, i) => byte === checkpoint.state[i]);
      if (same) exactMatches++;
      return same;
    });
    if (selected.checkpoint) {
      assert.equal(selected.checkpoint, previousCheckpoint, 'retained checkpoint identity');
      assert.equal(selected.held, selected.checkpoint.held, 'held-key alias');
      restoreCompactSoldier(local, selected.checkpoint.state);
    } else {
      assert.equal(selected.commands, pending, 'authority retains pending array identity');
      assert.equal(selected.held, held, 'authority retains held-key identity');
    }
    assert.ok(selected.commands.every(command => pending.includes(command)), 'selected command identities');
    let work = 0;
    const end = replayTicks(selected.commands, selected.from, generation, selected.held, (_tick, command) => { work++; step(local, command); });
    maxReplay = Math.max(maxReplay, work);
    const state = captureCompactSoldier(local);
    window.save({ ...context, tick: end, commands: selected.commands, held: selected.held, state });
    assert.equal(window.checkpoint.state, state, 'checkpoint state identity');
    checkpoints.update(state); checkpoints.update(JSON.stringify([end, applied, epoch, generation, identity, selected.commands.map(c => c.sequence)]));
    predicted = true;
  }
  assert.equal(serverTick, 600); assert.equal(epoch, 2); assert.equal(generation, 2); assert.equal(identity, 90);
  assert.ok(sequence > 500 && maxPending <= 120 && maxReplay <= 120 && exactCalls > 0 && exactMatches > 0);
  return { fps, stall, sequence, frames, physicsTicks, maxPending, maxReplay, exactCalls, exactMatches, epoch, generation, identity,
    commandSha256: commands.digest('hex'), bodySha256: bodies.digest('hex'), checkpointSha256: checkpoints.digest('hex'),
    finalBody: Array.from(captureCompactSoldier(local)), finalAuthority: Array.from(captureCompactSoldier(authority)) };
}
let report, completed = false;
const phaseCounts = () => ({ ...opcodes });
const requirePhases = (before, phases) => {
  for (const phase of phases) assert.ok((opcodes[phase] || 0) > (before[phase] || 0), `${label} native phase ${phase} delegated`);
};
try {
  for (const fps of [20, 60, 90, 144]) for (const network of [{}, { jitterMs: 30, lossEvery: 7, snapshotLossEvery: 11 }]) {
    label = `plane-${fps}-${network.lossEvery || 0}`;
    const options = { fps, ...network }, first = native.calls, before = phaseCounts(), expected = measured(options, null), actual = measured(options, native);
    assert.deepEqual(actual, expected, label); assert.ok(native.calls > first + 100, `${label} native timing delegated`);
    requirePhases(before, [1, 8, 11, 12]);
    cases.push({ label, ...actual, nativeCalls: native.calls - first });
  }
  for (const fps of [20, 59.94, 60, 90, 144]) for (const stall of [false, true]) {
    label = `blackout-${fps}-${stall}`;
    const first = native.calls, before = phaseCounts(), expected = blackout({ fps, stall }, null), actual = blackout({ fps, stall }, native);
    assert.deepEqual(actual, expected, label); assert.ok(native.calls > first + 100, `${label} native windows delegated`);
    requirePhases(before, [1, 8, 13, 14, 15]);
    cases.push({ label, ...actual, nativeCalls: native.calls - first });
  }
  const files = [], loaded = new Map(), query = '?map=nab2&side=rep';
  const manifest = await contentManifest(query, async path => {
    const response = await fetch(path); if (!response.ok) return response;
    const bytes = await response.arrayBuffer();
    if (/\/(map\.json|(terrain_)?collision(_index|_poly)?\.bin)$/.test(path)) {
      loaded.set(path, bytes); files.push([path, bytes.byteLength, sha256(new Uint8Array(bytes))]);
    }
    return { ok: true, status: 200, arrayBuffer: async () => bytes };
  });
  const geometry = [['collision.bin', Float32Array], ['collision_index.bin', Uint32Array], ['collision_poly.bin', Uint32Array],
    ['terrain_collision.bin', Float32Array], ['terrain_collision_index.bin', Uint32Array]].map(([name, Type]) => {
      const bytes = loaded.get('/maps/nab2/' + name); assert.ok(bytes, `authenticated ${name}`); return new Type(bytes);
    });
  const map = JSON.parse(new TextDecoder().decode(loaded.get('/maps/nab2/map.json'))), mission = selectMission(map, new URLSearchParams(query));
  const spawn = Object.values(mission.spawns).flat().find(value => Array.isArray(value) && value.length >= 4 && value.slice(0, 4).every(Number.isFinite));
  assert.ok(spawn, 'authored map spawn');
  for (const fps of [60, 144]) {
    label = `content-nab2-${fps}`;
    const options = { fps, jitterMs: 30, lossEvery: 7, snapshotLossEvery: 11, start: spawn.slice(0, 3), yaw: spawn[3] };
    const first = native.calls, before = phaseCounts(), expected = measured({ ...options, world: new CollisionWorld(...geometry) }, null), actual = measured({ ...options, world: new CollisionWorld(...geometry) }, native);
    assert.deepEqual(actual, expected, label); assert.ok(native.calls > first + 100, `${label} authenticated timing delegated`);
    requirePhases(before, [1, 8, 11, 12]);
    cases.push({ label, ...actual, nativeCalls: native.calls - first });
  }
  requirePhases({}, [10]);
  report = { revision, closed: true, cases, caseCount: cases.length,
    authentication: { query, manifest, files, fingerprint: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)) },
    operations, opcodes, corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { native.dispose(); if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
