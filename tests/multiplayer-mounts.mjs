// Owned-assets semantic lifecycle diagnostic, not a transport/prediction gate.
// Each complete world lives in its own sequential process: capture authority,
// exit it, then replay Alice and Bob independently. No three-world memory peak.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { resolve, sep, join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { EntitySeats } from '../src/runtime/entity-seats.js';
import { EntityWorldCapture, tryCapturePlayerCorrection } from '../src/runtime/network-entity-capture.js';
import { createEntityRegistry, entityRegistryDescriptor, verifyEntityRegistry } from '../src/runtime/network-entity-registry.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES } from '../src/runtime/network-entity-input.js';
import { EntityScheduler } from '../src/runtime/network-entity-stream.js';
import { EntityShotCounters } from '../src/runtime/network-entity-shots.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';
const [phase, archive] = process.argv.slice(2);
if (!phase) {
  const dir = await mkdtemp(join(tmpdir(), 'bfe-mount-replay-'));
  try {
    for (const stage of ['authority', 'alice', 'bob']) {
      const result = spawnSync(process.execPath, [fileURLToPath(import.meta.url), stage, join(dir, 'records.json')],
        { stdio: 'inherit', env: process.env, timeout: 180000 });
      if (result.error || result.status !== 0) throw Error(`Mount diagnostic ${stage} failed: ${result.error?.message || result.signal || result.status}; archive ${dir}`);
    }
    await rm(dir, { recursive: true });
  } catch (error) { console.error(`Preserved mount diagnostic archive: ${dir}`); throw error; }
  process.exit(0);
}
assert.ok(['authority', 'alice', 'bob'].includes(phase));
const root = resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch = async path => {
  assert.match(path, /^\/(maps|sides|common|audio)\//);
  const filename = resolve(root, `.${path}`); assert.ok(filename.startsWith(root + sep));
  try { return new Response(await readFile(filename)); } catch (error) { if (error.code === 'ENOENT') return new Response('', { status: 404 }); throw error; }
};
const query = '?map=nab2&side=rep&ai=classic&seed=1';
const options = game => ({ contentEpoch: 1, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES, posts: game.posts.posts });
const turn = (a, b) => Math.abs(Math.atan2(Math.sin(a - b), Math.cos(a - b)));
const ANGLE = Math.PI / 32767;
console.log(JSON.stringify({ stage: phase, state: 'loading' }));
const game = await createGame({ headless: true, query });
console.log(JSON.stringify({ stage: phase, state: 'loaded', rss: process.memoryUsage().rss }));
if (phase === 'authority') {
  const world = createMultiplayerWorld(game), registry = createEntityRegistry(game.data, options(game));
  const shots = new EntityShotCounters(game.simEvents);
  const capture = new EntityWorldCapture(game, registry, { fireCounter: unit => shots.counter(unit) });
  const joined = { alice: await world.addPlayer('alice', { team: game.playerTeam, separateActions: true }), bob: await world.addPlayer('bob', { team: game.playerTeam, separateActions: true }) };
  const au = game.units.byId(joined.alice.unitId), bu = game.units.byId(joined.bob.unitId);
  assert.notEqual(au.id, bu.id);
  const result = { descriptor: entityRegistryDescriptor(registry), players: { alice: au.id, bob: bu.id }, packets: { alice: [], bob: [] }, checkpoints: [] };
  const schedulers = Object.fromEntries(['alice', 'bob'].map(id => [id, new EntityScheduler({ epoch: 1, maxEntities: 768 })]));
  let archiveBytes = 0;
  function publish() {
    const tick = game.worldClock.tick, records = capture.capture(tick);
    assert.equal(capture.unsupported.filter(e => !/walker/.test(e.message)).length, 0);
    for (const id of ['alice', 'bob']) {
      const actor = world.players.get(id), scheduler = schedulers[id];
      const own = tryCapturePlayerCorrection(game, world, id, { registry, bindingProfileId: 0, epoch: 1 });
      for (const record of records) {
        if (own && record.kind === 'soldier' && record.id === actor.unit.id) continue;
        scheduler.update(record, record.kind === 'seat' || record.kind === 'tombstone' ? 16 : 6);
      }
      const fields = { tick, ackInput: actor.mailbox.ack };
      let packet = scheduler.packet({ ...fields, own: own || undefined }), sent = 0;
      while (packet && sent++ < 16) {
        scheduler.admitted(packet.packetId);
        archiveBytes += packet.buffer.length; assert.ok(archiveBytes <= 8 * 1024 * 1024, 'bounded replay archive');
        result.packets[id].push(Buffer.from(packet.buffer).toString('base64'));
        scheduler.ack({ epoch: 1, packetId: packet.packetId });
        packet = scheduler.packet({ ...fields, maxBytes: 1100 });
        if (packet && packet.buffer.length <= 32) break;
      }
    }
  }
  const totals = { alice: [0, 0], bob: [0, 0] };
  function step(ticks, inputs = {}) {
    for (let n = 0; n < ticks; n++) {
      const tick = game.worldClock.tick + 1;
      for (const [id, make] of Object.entries(inputs)) {
        const { look = [], ...rest } = make(tick);
        for (const [dx, dy] of look) { totals[id][0] += dx; totals[id][1] += dy; }
        if (rest.actions?.length) assert.equal(world.actions(id, { epoch:1, id:tick, tick, generation:world.players.get(id).unit.generation, actions:rest.actions }), true);
        world.input(id, { epoch: 1, sequence: tick, tick, generation: world.players.get(id).unit.generation,
          down: [], actions: [], look: [], lookTotal: [...totals[id]], mouseScale: 1, invertMouse: false, ...rest });
      }
      world.step(); if (tick % 3 === 0) publish();
    }
    if (game.worldClock.tick % 3 !== 0) publish();
  }
  let hull, turret;
  function checkpoint(name) {
    result.checkpoints.push({ name, tick: game.worldClock.tick,
      counts: Object.fromEntries(['alice', 'bob'].map(id => [id, result.packets[id].length])),
      alive: { alice: au.alive, bob: bu.alive },
      hull: hull ? { id: hull.aiTarget.id, generation: hull.aiTarget.generation, alive: hull.alive,
        crew: hull.seats.map(s=>s.lease?.unit?.id??null), position: hull.position.toArray(), yaw: hull.seats[1].aimYaw, pitch: hull.seats[1].pitch } : null,
      turret: turret ? { index: game.live.buildings.indexOf(turret), id: turret.aiTarget.id, yaw: turret.yaw, pitch: turret.pitch } : null });
    console.log(JSON.stringify({ stage: phase, checkpoint: name, tick: game.worldClock.tick, rss: process.memoryUsage().rss }));
  }
  step(6); checkpoint('on-foot');
  hull = game.vehicles.vehicles.find(v => v.alive && v.seats.length > 1); assert.ok(hull);
  for (const unit of game.units.all()) if (unit.seat?.vehicle === hull) game.vehicles.unboard(unit, { force: true, placement: false });
  assert.ok(game.vehicles.board(au, hull, 0)); assert.ok(game.vehicles.board(bu, hull, 1));
  step(6, { bob: () => ({ look: [[400, -150]] }) });
  assert.ok(Math.abs(hull.seats[1].aimYaw - hull.yaw) > .01); checkpoint('crewed');
  console.log(JSON.stringify({stage:phase,checkpoint:'exit-request',seat:bu.seat?.index,vehicle:hull.className}));
  step(3, { bob: tick => ({ actions: tick === 13 ? ['use'] : [] }) }); console.log(JSON.stringify({stage:phase,checkpoint:'exit-result',seated:Boolean(bu.seat)})); assert.ok(bu.seat === null, 'passenger remains seated after one use edge'); step(3); checkpoint('passenger-exited');
  turret = game.turretSeats.turrets.find(t => game.turretSeats.canMan(bu, t));
  if (turret) {
    assert.ok(game.turretSeats.man(bu, turret)); step(6, { bob: () => ({ look: [[200, 0]] }) }); checkpoint('turret-manned');
    step(3, { bob: () => ({ actions: ['use'] }) }); step(3); assert.ok(bu.seat === null, 'passenger seat must be clear'); checkpoint('turret-exited');
  }
  hull.setHealth(0); assert.equal(hull.alive, false); assert.ok(au.seat === null, 'pilot seat must be clear');
  step(3); checkpoint('destroyed');
  for (let n = 0; n < 180; n++) { game.vehicles.updateSpawns?.(1 / 60); world.step(); if (game.worldClock.tick % 3 === 0) publish(); if (hull.alive) break; }
  if (hull.alive) { step(3); checkpoint('respawned'); }
  result.turretCovered = Boolean(turret); result.respawnCovered = hull.alive;
  await writeFile(archive, JSON.stringify(result));
  console.log(JSON.stringify({ stage: phase, status: 'captured', archiveBytes, turretCovered: result.turretCovered, respawnCovered: result.respawnCovered }));
} else {
  const recorded = JSON.parse(await readFile(archive, 'utf8'));
  // This semantic fixture bypasses reliable bootstrap. Prepare its provisional
  // own class explicitly; the actual browser gate covers production bootstrap.
  await game.factory.prepare(game.playerUnit.className);
  const local = Object.create(game); Object.defineProperty(local, 'client', { value: { seats: new EntitySeats(local) } });
  // No browser client closure in this semantic-only fixture. Both humans use
  // the loaded mission team; real cross-team application is browser-gated.
  Object.defineProperty(local, 'validateAuthoritativeHumanTeam', { value: team => assert.equal(team, game.playerTeam) });
  const registry = verifyEntityRegistry(createEntityRegistry(game.data, options(game)), recorded.descriptor);
  const adapter = new EntityGameState(local, registry); adapter.baseline();
  let packets = 0;
  for (const point of recorded.checkpoints) {
    while (packets < point.counts[phase]) adapter.receive(new Uint8Array(Buffer.from(recorded.packets[phase][packets++], 'base64')));
    assert.equal(local.playerUnit.id, recorded.players[phase]);
    const hull = point.hull ? local.vehicles.vehicles.find(v => v.aiTarget.id === point.hull.id) : null;
    if (point.name === 'on-foot') { assert.ok(adapter.previous?.tick > 0, 'exact on-foot correction applied'); assert.ok(local.playerUnit.seat === null, 'expected empty local.playerUnit.seat'); }
    if (point.name === 'crewed') {
      assert.ok(hull); assert.ok(local.driver.vehicle === hull, 'local driver references expected hull'); assert.equal(hull.seatIndex, phase === 'alice' ? 0 : 1);
      assert.equal(hull.seats[0].lease?.unit?.id, recorded.players.alice); assert.equal(hull.seats[1].lease?.unit?.id, recorded.players.bob);
      assert.equal(hull.seats[phase === 'alice' ? 0 : 1].lease.human, true);
      assert.ok(turn(hull.seats[1].aimYaw, point.hull.yaw) <= ANGLE * 2);
      assert.ok(turn(hull.seats[1].pitch, point.hull.pitch) <= ANGLE * 2);
      assert.ok(Math.hypot(...hull.position.toArray().map((n, i) => n - point.hull.position[i])) < .02);
    }
    if (point.name === 'passenger-exited') {
      assert.notEqual(hull.seats[1].lease?.unit?.id, recorded.players.bob, 'exiting passenger leaves the lease');
      assert.deepEqual(hull.seats.map(s=>s.lease?.unit?.id??null), point.hull.crew, 'later AI boarding matches authority');
      if (phase === 'bob') { assert.ok(local.driver.vehicle === null, 'expected empty local.driver.vehicle'); assert.ok(local.playerUnit.seat === null, 'expected empty local.playerUnit.seat'); assert.equal(local.playerUnit.combat.mounted, false); }
    }
    if (point.name === 'turret-manned' || point.name === 'turret-exited') {
      const turret = local.live.buildings[point.turret.index]; assert.equal(turret.aiTarget.id, point.turret.id);
      if (point.name === 'turret-manned') {
        assert.equal(turret.lease?.unit?.id, recorded.players.bob); assert.equal(turret.lease.human, phase === 'bob');
        assert.ok(turn(turret.yaw, point.turret.yaw) <= ANGLE * 2 && turn(turret.pitch, point.turret.pitch) <= ANGLE * 2);
        if (phase === 'bob') assert.equal(local.gunner.turret, turret);
      } else { assert.ok(turret.lease === null, 'expected empty turret.lease'); if (phase === 'bob') assert.ok(local.gunner.turret === null, 'expected empty local.gunner.turret'); }
    }
    if (point.name === 'destroyed') {
      assert.equal(hull.alive, false); assert.equal(hull.root.visible, false); assert.equal(hull.body.enabled, false);
      assert.ok(hull.seats.every(s => s.lease === null));
      assert.ok(local.driver.vehicle === null, 'expected empty local.driver.vehicle'); assert.ok(local.playerUnit.seat === null, 'expected empty local.playerUnit.seat');
      assert.equal(local.playerUnit.alive, point.alive[phase]);
    }
    if (point.name === 'respawned') {
      assert.equal(hull.alive, true); assert.equal(hull.aiTarget.generation, point.hull.generation); assert.ok(hull.seats.every(s => s.lease === null));
    }
    adapter.present(point.tick); local.vehicles.present(1); local.vehicles.presentRiders(1 / 60);
    console.log(JSON.stringify({ stage: phase, checkpoint: point.name, status: 'passed', rss: process.memoryUsage().rss }));
  }
  assert.equal(adapter.deferred, 0);
  console.log(JSON.stringify({ stage: phase, status: 'passed', packets, turretCovered: recorded.turretCovered, respawnCovered: recorded.respawnCovered }));
}
