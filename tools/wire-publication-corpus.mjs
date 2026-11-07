// Record complete snapshot-host publications, including ordered non-datagram
// outputs. A comparison reads an existing corpus and never updates its bytes.
// Run with game content: --out DIR [--against DIR] [--map nab2] [--ticks 900]
// [--local] [--budget 500000]. Clocks and impairment are scripted.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { headlessGame } from './headless-world.mjs';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createSnapshotHost, publicationBuffersFor } from '../src/runtime/snapshot-host.js';
import { createCombatRegistry } from '../src/runtime/network-combat-registry.js';
import { SnapshotClient } from '../src/runtime/snapshot-client.js';
import { encodeEntityInput, encodeSnapshotAckKeepalive } from '../src/runtime/network-entity-input.js';
import { Vector3 } from 'three';
import { WireCensusLayout as WireCensus } from '../src/sim/gen/wire-census.js';

const args = process.argv.slice(2);
const option = (key, fallback) => { const at = args.indexOf(`--${key}`); return at < 0 ? fallback : args[at + 1]; };
const out = option('out', null), against = option('against', null), map = option('map', 'nab2');
if (!out) throw Error('Expected --out directory');
const ticks = Number(option('ticks', 900)), local = args.includes('--local'), stress = args.includes('--stress');
const wire = true, budget = Number(option('budget', 500000));
const lateHost = args.includes('--late-host'), flyer = args.includes('--flyer');
const validOrdnance = args.includes('--ordnance-valid');
const ordnance = args.includes('--ordnance') || validOrdnance, spectator = args.includes('--spectator'), remote = args.includes('--remote'), edges = args.includes('--edges');
const label = `${map}-${local ? 'local' : 'online'}-${budget}${stress ? '-stress' : ''}${ordnance ? '-ordnance' : ''}${validOrdnance ? '-valid' : ''}${spectator ? '-spectator' : ''}${remote ? '-remote' : ''}${edges ? '-edges-clock' : ''}${lateHost ? '-late-host' : ''}${flyer ? '-flyer' : ''}`;
const bytes = value => Buffer.from(value.buffer ?? value, value.byteOffset ?? 0, value.byteLength).toString('base64');
function normalize(value) {
  if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) return { bytes: bytes(value) };
  if (value instanceof Error) return { name: value.name, message: value.message };
  if (Array.isArray(value)) return value.map(normalize);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, field]) => [key, normalize(field)]));
  return value;
}
function difference(actual, expected, path = '') {
  if (actual === expected) return null;
  if (path.endsWith('.bytes') && typeof actual === 'string' && typeof expected === 'string') {
    const a = Buffer.from(actual, 'base64'), b = Buffer.from(expected, 'base64'); let at = 0;
    while (at < Math.min(a.length, b.length) && a[at] === b[at]) at++;
    return `${path}[${at}] (${a.length}/${b.length} bytes)`;
  }
  if (actual && expected && typeof actual === 'object' && typeof expected === 'object') {
    const keys = Object.keys(actual), before = Object.keys(expected);
    if (keys.join('\0') !== before.join('\0')) return `${path}: keys ${keys.join(',')}/${before.join(',')}`;
    for (const key of keys) { const found = difference(actual[key], expected[key], `${path}.${key}`); if (found) return found; }
    return null;
  }
  return `${path}: ${String(actual).slice(0, 120)}/${String(expected).slice(0, 120)}`;
}

const game = await headlessGame(`?map=${map}&side=rep&ai=classic&seed=1`);
const world = createMultiplayerWorld(game, { armySize: 32 });
const bridge = game.simBridge, exports = bridge.x, calls = Object.create(null);
let counting = false;
const manual = { inputWords: 0, outputWords: 0, inputBytes: 0, outputBytes: 0 };
// Wrap only this diagnostic bridge. Count actual exports and generated words;
// the simulation still calls the same module with the same owned inputs.
bridge.x = new Proxy({ ...exports }, { get(target, key) {
  const value = target[key];
  if (typeof value !== 'function') return value;
  return (...values) => {
    if (counting) {
      calls[key] = (calls[key] ?? 0) + 1;
      if (key === 'wire_semantic_room' || key === 'wire_presentation_room' || key === 'wire_cosmetics_room') manual.inputWords += values[1];
      if (key === 'wire_cosmetics_payload') manual.inputBytes += values[1];
    }
    const result = value(...values);
    if (counting) {
      if (key === 'wire_semantic_len' || key === 'wire_presentation_len' || key === 'wire_cosmetics_byte_count') manual.outputBytes += result;
      if (key === 'wire_cosmetics_word_count') manual.outputWords += result;
    }
    return result;
  };
} });
let at = 0;
const host = createSnapshotHost({ game, world, local, stateBytesPerSecond: budget, now: () => at });
const ids = ['steady', 'partial', 'second', 'held', 'silent', 'rejoin', ...(spectator ? ['observer'] : [])];
const clients = new Map(), pending = new Map();
const remoteClass = remote ? game.spawnPort.roster(1).find(entry => Object.values(game.data.sides).some(side => side.units?.[entry.className]?.weapons?.some(w => w.base === 'remote')))?.className : null;
if (remote) assert.ok(remoteClass, 'mission roster has a remote launcher');
let remoteButton = 'Mouse2';
for (const id of ids) {
  await world.addPlayer(id, { spectator: id === 'observer', reliableFire: !(remote && id === 'steady'), ...(remote && id === 'steady' ? { team: 1, className: remoteClass } : {}), separateActions: true, ...(stress ? { name: `fixture-\u03bb\ud83d\ude80-${id}` } : {}) });
  world.players.get(id).autoRespawn = true;
  world.players.get(id).lastInputAt = 0;
  // Bodies can predate the first connection. Their initial identity is the
  // publication's observation, not an invented earlier launch callback.
  if (lateHost && id === 'steady') {
    const owner = world.players.get(id).unit;
    const definitions = [...createCombatRegistry(game.data).definitions.values()].filter(d => ['grenade', 'shell', 'missile', 'bullet', 'beam', 'fatray'].includes(d.base) && Number.isFinite(d.Velocity));
    for (const def of definitions.slice(0, 6)) game.ordnance.fire(def, owner.position.clone().add(new Vector3(0, 3, 0)), new Vector3(.1, 1, .2).normalize(), owner, null, owner.body);
    assert.ok(game.ordnance.grenades.length + game.ordnance.rounds.length + game.ordnance.beams.length, 'bodies predate host admission');
  }
  host.addClient(id); host.setStarted(id, true); host.setReady(id);
  clients.set(id, new SnapshotClient()); pending.set(id, []);
}
const totals = { inputWords: 0, outputWords: 0, allocations: 0, reallocations: 0, allocationBytes: 0, inputBytes: 0, outputBytes: 0, sharedBytes: 0, ownBytes: 0, secondOwn: 0, multipart: 0, reliable: 0, faults: 0, decoded: 0 };
const coverage = { flyerTicks: 0, remoteTicks: 0, dynamicTicks: 0, turret: 0, hull: 0, death: 0, reconnect: 0, secondOwnLost: 0, partialLost: 0 };
const lines = [], expected = against ? readFileSync(join(against, `${label}.jsonl`), 'utf8').trimEnd().split('\n') : null;
const sequences = new Map(ids.map(id => [id, 0]));
try {
  for (let frame = 0; frame < ticks; frame++) {
    at = frame * 1000 / 60;
    if (stress) {
      const actor = world.players.get('steady'), unit = actor.unit;
      if (frame === 40 && unit.alive) {
        const turret = game.turretSeats?.turrets.find(t => game.turretSeats.canMan(unit, t));
        if (turret && game.turretSeats.man(unit, turret)) coverage.turret++;
      }
      if (frame === 100 && unit.seat?.turret) game.turretSeats.leave(unit, { placement: true });
      if (frame === 120 && unit.alive && !unit.seat) {
        for (const vehicle of game.vehicles?.vehicles ?? []) {
          if (!vehicle.alive || flyer && !vehicle.flightClass) continue;
          const seat = flyer ? (vehicle.seatFree(0) ? 0 : -1) : vehicle.seats.findIndex((s, index) => vehicle.seatFree(index));
          if (seat >= 0 && game.vehicles.board(unit, vehicle, seat)) { coverage.hull++; break; }
        }
      }
      if (frame === 180 && unit.alive) { unit.combat.damage(Infinity); coverage.death++; }
      if (frame === 360) { host.removeClient('partial'); world.removePlayer('partial'); }
      if (frame === 380) {
        await world.addPlayer('partial', { reliableFire: true, separateActions: true });
        host.addClient('partial'); host.setStarted('partial', true); host.setReady('partial');
        clients.set('partial', new SnapshotClient()); pending.get('partial').length = 0; coverage.reconnect++;
      }
    }
    for (const [i, id] of ids.entries()) {
      if (id === 'silent' && frame > 10 && frame < 400) continue;
      const queue = pending.get(id), receiver = clients.get(id);
      queue.push(receiver.snapshotAck.tick);
      const ack = queue.length > i + 1 ? queue.shift() : 0;
      // One client holds its first acknowledgement through legacy age expiry;
      // another never acknowledges, retaining a full-snapshot recipient.
      const allowed = id !== 'second' && (id !== 'held' || frame < 8 || frame > 300);
      if (allowed && ack) host.input(id, encodeSnapshotAckKeepalive({ epoch: 1, tick: ack }));
      else host.input(id, encodeSnapshotAckKeepalive({ epoch: 1, tick: 0 }));
      const actor = world.players.get(id);
      const sequence = sequences.get(id) + 1; sequences.set(id, sequence);
      if (remote && id === 'steady' && frame === 10) {
        const launcher = actor.unit.combat.arsenal.weapons.find(w => w.def.base === 'remote');
        assert.ok(actor.unit.combat.arsenal.select(launcher), 'select canonical remote launcher'); remoteButton = launcher.channel === 0 ? 'Mouse0' : 'Mouse2';
      }
      if (id === 'steady') host.input(id, encodeEntityInput({ epoch: 1, generation: actor.unit.generation,
        commands: [{ epoch: 1, generation: actor.unit.generation, sequence, tick: game.worldClock.tick + 1, down: [...(frame % 90 < 45 ? ['KeyW'] : []), ...(remote && frame >= 10 && frame < 15 ? [remoteButton] : []), ...(remote && frame >= 60 && frame < 100 ? ['Mouse0'] : [])], lookTotal: [0, 0] }], edges: edges && frame % 19 === 5 ? [{epoch:1,generation:actor.unit.generation,id:frame+1,tick:game.worldClock.tick+1,kind:'action',action:'reload'}] : [], snapshotAck: null }, host.registry));
      actor.lastInputAt = at;
    }
    if (ordnance && frame % 80 === 20) {
      const owner = world.players.get('steady').unit;
      const definitions = [...host.registry.combat.definitions.values()].filter(d => ['grenade', 'shell', 'missile', 'bullet', 'beam', 'fatray'].includes(d.base) && (!validOrdnance || Number.isFinite(d.Velocity)));
      assert.ok(definitions.length, 'dynamic projectile content');
      for (const def of definitions.slice(0, 6)) game.ordnance.fire(def, owner.position.clone().add(new Vector3(0, 3, 0)), new Vector3(.1, 1, .2).normalize(), owner, null, owner.body);
    }
    assert.deepEqual(host.advanceEdges(), []);
    // An executed action refreshes lastInputAt from performance.now(). Supply
    // the scripted clock after that callback too, so CPU speed cannot decide
    // whether the very same queued input is cleared as stale.
    if (edges) for (const actor of world.players.values()) actor.lastInputAt = at;
    world.step(at);
    assert.deepEqual(host.consumedEdges(game.worldClock.tick), []);
    if (world.players.get('steady').unit.seat?.index === 0 && world.players.get('steady').unit.seat?.vehicle?.flightClass) coverage.flyerTicks++;
    if (remote && world.players.get('steady').unit.combat.remotes?.piloted) coverage.remoteTicks++;
    if (ordnance && (game.ordnance.grenades.length || game.ordnance.rounds.length || game.ordnance.beams.length)) coverage.dynamicTicks++;
    // Force enough valid presentation notices for the second own packet.
    // Unicode and signed-zero fields exercise JSON/UTF-8 at the live boundary.
    if (frame === 90 || frame === 330) {
      const own = world.players.get('steady').unit;
      for (let i = 0; i < 24; i++) game.simEvents.emit('presentation', { event: { kind: 'effect', name: `fixture-${i}-\u03bb\ud83d\ude80`, ownerId: own.id, ownerGeneration: own.generation, position: [-0, 1e-7, 1e21], normal: [0, 1, 0] } });
      game.simEvents.emit('damage', { target: { unit: own, generation: own.generation }, attacker: { unit: own, generation: own.generation }, position: [-0, 1e-7, 1e21], source: [0, 1, 0], team: own.team, attackerTeam: own.team });
    }
    // A late host/catch-up publication must follow distance from the previous
    // capture, including when its next snapshot lands on an odd world tick.
    if (frame >= 160 && frame < 165) continue;
    if (local && frame === 200) host.setLocalInterval(3);
    if (local && frame === 400) host.setLocalInterval(1);
    bridge.boundaryCounters = { inputWords: 0, outputWords: 0 };
    const faultViews = [];
    if (stress && (frame === 71 || frame === 73)) for (const id of ['steady', 'held']) {
      const view = world.players.get(id).mailbox.view;
      faultViews.push({ view, value: view.mouseScale, present: Object.hasOwn(view, 'mouseScale') });
      view.mouseScale = NaN;
    }
    exports.wire_census_begin(); counting = true;
    let publication;
    try { publication = host.publish({ at, unixMs: 1700000000000 + Math.floor(at), droppedTicks: frame === 165 ? 4 : 0 }); }
    finally { counting = false; }
    const ptr = exports.wire_census_end(); bridge.views();
    for (const saved of faultViews) { if (saved.present) saved.view.mouseScale = saved.value; else delete saved.view.mouseScale; }
    const census = bridge.f.subarray(ptr / 8, ptr / 8 + WireCensus.words);
    totals.allocations += census[WireCensus.allocations]; totals.reallocations += census[WireCensus.reallocations]; totals.allocationBytes += census[WireCensus.allocationBytes];
    totals.inputWords += bridge.boundaryCounters.inputWords; totals.outputWords += bridge.boundaryCounters.outputWords;
    totals.inputBytes += bridge.boundaryCounters.inputBytes ?? 0; totals.outputBytes += bridge.boundaryCounters.outputBytes ?? 0;
    bridge.boundaryCounters = null;
    for (const group of publication.shared) { totals.sharedBytes += group.bytes; if (group.parts.length > 1) totals.multipart++; }
    for (const packet of publication.ownPackets) totals.ownBytes += packet.buffer.byteLength;
    totals.secondOwn += publication.ownPackets.length - new Set(publication.ownPackets.map(p => p.client)).size;
    totals.reliable += publication.reliableEvents.reduce((n, event) => n + event.records.length, 0); totals.faults += publication.faults.length;
    const line = JSON.stringify(normalize(publication));
    if (expected && line !== expected[lines.length]) {
      mkdirSync(out, { recursive: true });
      writeFileSync(join(out, `${label}-mismatch.json`), JSON.stringify({ frame, actual: JSON.parse(line), expected: JSON.parse(expected[lines.length]) }, null, 2));
      throw Error(`publication ${label} frame ${frame}: ${difference(JSON.parse(line), JSON.parse(expected[lines.length])) ?? 'serialization order'}`);
    }
    lines.push(line);
    for (const [i, id] of ids.entries()) {
      const packets = publicationBuffersFor(publication, id);
      if (frame >= 220 && frame < 250 || (frame + i) % (i + 3) === 0) continue;
      for (let part = 0; part < packets.length; part++) {
        if (id === 'partial' && part === 0 && frame % 7 === 0) { coverage.partialLost++; continue; }
        if ((id === 'steady' || id === 'second') && frame % 7 < 3 && part === packets.length - 1 && publication.ownPackets.filter(p => p.client === id).length === 2) { coverage.secondOwnLost++; continue; }
        if (clients.get(id).receive(new Uint8Array(packets[part]))) totals.decoded++;
      }
    }
  }
  if (expected) assert.equal(lines.length, expected.length, 'complete corpus length');
  if (flyer) assert.ok(coverage.flyerTicks, 'live flyer pilot coverage');
  if (ordnance) assert.ok(coverage.dynamicTicks, 'live dynamic projectile coverage');
  if (remote) assert.ok(coverage.remoteTicks, 'actual remote control coverage');
  mkdirSync(out, { recursive: true });
  const transcript = `${lines.join('\n')}\n`;
  writeFileSync(join(out, `${label}.jsonl`), transcript);
  let commit = process.env.BFE_COMMIT ?? null;
  if (!commit) try { commit = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { /* Packaged runtimes need an explicit revision fact. */ }
  const summary = { label, commit, node: process.version, ticks, publications: lines.length, wire, totals, manual, coverage, calls,
    sha256: createHash('sha256').update(transcript).digest('hex'), matched: expected ? true : null };
  writeFileSync(join(out, `${label}.json`), `${JSON.stringify(summary, null, 2)}\n`);
  console.log(JSON.stringify(summary));
} finally { host.dispose(); }
