import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import * as THREE from 'three';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, Identity, SimBridge, bridgeFor, exportState, undeclared } from '../src/sim/runtime.js';
import { KIND_BEAM, KIND_BODY, KIND_BOLT, KIND_EMITTER, KIND_GRENADE, KIND_ORDNANCE, KIND_OWNER_SNAPSHOT, KIND_ROUND, KIND_TABLE, KIND_UNIT, KIND_VEHICLE, BOLT_KEYS } from '../src/sim/gen/index.js';
import { generate } from '../tools/sim-schema.mjs';

// The entity store (docs/sim-store.md): what JavaScript writes into a
// battle's instance of the simulation's module comes back out bit for bit
// (every number by Object.is, -0 and NaN included, null apart from
// undefined, every reference the same object), its mirrors keep their
// identity as JavaScript's objects did, and the code generated from the
// schemas is the code that is committed.

class Unit { constructor(id) { this.id = id; this.generation = 3; } }
class Vehicle { constructor(name) { this.name = name; } }
class Turret { constructor(name) { this.name = name; } }

// A battle's worth of made-up ordnance, every kind of value its schema
// allows at least once.
function battle() {
  const units = [new Unit(4), new Unit(9), new Unit(12)];
  const vehicles = [new Vehicle('rep_hover_fightertank'), new Vehicle('cis_tread_snailtank')];
  const turrets = [new Turret('rep_bldg_chaingun_tripod')];
  const bodies = [{ offset: 0 }, { offset: 1 << 24 }];
  const emitters = [{ emitter: 1 }];
  const snapshot = Object.freeze({ id: 9, generation: 2, team: 1, human: false, className: 'cis_inf_rifleman' });
  const rifle = { class: 'rep_weap_inf_rifle_ord', base: 'bolt', Velocity: 150, LifeSpan: 0.8 };
  const rocket = { class: 'rep_weap_inf_rocket_launcher_ord', base: 'bullet' };
  const grenade = { class: 'rep_weap_inf_thermaldetonator_ord', base: 'sticky' };
  const v = (x, y, z) => new THREE.Vector3(x, y, z);
  const bolts = [
    { def: rifle, owner: units[0], networkFireId: 17, networkFireChannel: 0, networkFireUnit: 4, networkFireGeneration: 3, ignoreBody: null,
      position: v(1.5, -0, 2), direction: v(0, 0, 1), speed: 150, life: 0.8, drawn: 0, flown: 0, offset: v(0.1, 0.2, 0.3), merge: 8,
      impact: { point: v(3, 4, 5), normal: v(0, 1, 0), terrain: true, body: bodies[1] } },
    { def: rifle, owner: 'player', networkFireId: 0, networkFireChannel: 1, networkFireUnit: 0, networkFireGeneration: 0, ignoreBody: bodies[0],
      position: v(NaN, Infinity, -Infinity), direction: v(-0, 1e-300, 5e-324), speed: 150, life: -0, drawn: 1, flown: 2, offset: null, merge: 1e-3,
      impact: { point: v(1, 2, 3), normal: v(0, 0, -1), terrain: false, body: null, unit: units[2], part: 'head' }, previousFlown: 0.25, previousDrawn: -0 },
    { def: rifle, owner: snapshot, networkFireId: 2 ** 40, networkFireChannel: 2, networkFireUnit: 9, networkFireGeneration: 2, ignoreBody: null,
      position: v(0, 0, 0), direction: v(1, 0, 0), speed: 0.1 + 0.2, life: 1 / 3, drawn: 0, flown: 0, offset: null, merge: 8, impact: null },
  ];
  const beams = [
    { networkFireChannel: 0, historyTick: 1204.5, def: rifle, owner: vehicles[0], source: emitters[0], ignoreBody: bodies[0], life: 0.4, duration: 0.4,
      effectTime: -0.01, position: v(1, 1, 1), end: v(2, 2, 2), direction: v(0, 0, 1), localOrigin: v(0, 0.5, 2), damageDef: { ...rifle, Damage: 12.5 } },
    { networkFireChannel: 3, historyTick: null, def: rifle, owner: null, source: 'player', ignoreBody: null, life: 0, duration: 0.01,
      effectTime: 0.12, position: v(0, 0, 0), end: v(0, 0, 600), direction: v(0, 0, 1), localOrigin: null, damageDef: { ...rifle, Damage: -0 } },
  ];
  const grenades = [
    { networkFireChannel: 0, def: grenade, owner: snapshot, position: v(1, 2, 3), previous: v(1, 2.1, 3), launchPosition: v(0, 2, 0), velocity: v(0, -9.8, 0),
      gravity: -9.8, slow: 2, resting: false, flight: 1.25, fuse: null, spin: v(0.6, 0.8, 0), quaternion: new THREE.Quaternion(0, 0.7071067811865476, 0, 0.7071067811865476),
      sinking: true, touched: true },
    { networkFireChannel: 1, def: grenade, owner: units[1], position: v(5, 0, 5), previous: v(5, 0, 5), launchPosition: v(5, 1, 5), velocity: v(0, 0, 0),
      gravity: -9.8, slow: 4, resting: true, flight: 3, fuse: 2.5, spin: v(0, 1, 0), quaternion: new THREE.Quaternion(), gone: false },
  ];
  const rounds = [
    { kind: 'missile', networkFireChannel: 0, ignoreBody: bodies[1], def: rocket, owner: vehicles[1], position: v(1, 2, 3), previous: v(1, 2, 2.5),
      launchPosition: v(1, 2, 0), velocity: v(0, 0, 35), life: 2.75, impact: null, dead: false, gravity: 0, ramp: 0.5, desired: v(0, 0, 1),
      targetId: 12, targetGeneration: 3, phase: [0.25, 6.0], quaternion: new THREE.Quaternion(), flightMissile: true, flightClass: 'fighter',
      mount: vehicles[1], mountSeat: 0, mountWeapon: 1, mountBarrel: -1 },
    { kind: 'fatray', networkFireChannel: 0, ignoreBody: null, def: rocket, owner: units[0], position: v(0, 0, 0), previous: v(0, 0, 0),
      launchPosition: v(0, 0, 0), velocity: v(100, 0, 0), life: 0.45, impact: null, dead: false, gravity: 0, ramp: 0, desired: v(1, 0, 0),
      phase: [0, 0], quaternion: new THREE.Quaternion(), struck: new Set([units[2], units[1]]), weapon: undefined, muzzleGeneration: null,
      muzzleIndex: -1, muzzleBarrel: -1, mount: turrets[0], mountSeat: -1, mountWeapon: 0, mountBarrel: 2 },
    { kind: 'bullet', networkFireChannel: 0, ignoreBody: null, def: rocket, owner: units[2], position: v(0, 0, 0), previous: v(0, 0, 0),
      launchPosition: v(0, 0, 0), velocity: v(150, 0, 0), life: 0.5, dead: true, gravity: 0, ramp: 0, desired: v(1, 0, 0), phase: [1, 2],
      quaternion: new THREE.Quaternion(), targetId: -1, targetGeneration: -1, gone: true,
      impact: { point: v(75, 0, 0), normal: v(-1, 0, 0), terrain: false, body: undefined, unit: undefined, part: undefined } },
  ];
  // (A round's mount and muzzle are the firing code's own, on the mirror:
  // the store keeps none of them.)
  return { units, vehicles, turrets, bodies, emitters, snapshot, ordnance: { bolts, beams, grenades, rounds, start: v(0.5, -0, 2) } };
}

// A bridge whose identities tell the kinds a union holds apart.
async function bridge(world) {
  const b = new SimBridge(await loadSim());
  b.setRefs(KIND_UNIT, new Identity(KIND_UNIT, { test: (o) => o instanceof Unit }));
  b.setRefs(KIND_VEHICLE, new Identity(KIND_VEHICLE, { test: (o) => o instanceof Vehicle }));
  b.setRefs(KIND_EMITTER, new Identity(KIND_EMITTER, { test: (o) => world.emitters.includes(o) }));
  b.setRefs(KIND_OWNER_SNAPSHOT, new Identity(KIND_OWNER_SNAPSHOT, { test: (o) => o === world.snapshot }));
  return b;
}

const LISTS = [[KIND_BOLT, 'bolts'], [KIND_BEAM, 'beams'], [KIND_GRENADE, 'grenades'], [KIND_ROUND, 'rounds']];

function writeAll(b, ordnance, full) {
  for (const [kind, list] of LISTS) b.write(kind, ordnance[list], full);
  b.write(KIND_ORDNANCE, [ordnance], full);
}

test('every field of every round written into the store comes back bit for bit', async () => {
  const world = battle();
  const b = await bridge(world);
  writeAll(b, world.ordnance, true);
  for (const [kind, list] of LISTS) {
    const back = b.read(kind);
    const original = world.ordnance[list];
    assert.equal(back.length, original.length, list);
    back.forEach(({ slot, gen, object }, i) => {
      // In the home list's order, each row the slot JavaScript's identity gave it.
      assert.equal(b.refs[kind].at(slot, gen), original[i], `${list}[${i}] is not in its place`);
      assert.equal(KIND_TABLE[kind].same(original[i], object), null, `${list}[${i}]`);
    });
  }
  const [{ object: system }] = b.read(KIND_ORDNANCE, () => ({}));
  for (const [, list] of LISTS) assert.deepEqual(system[list].map((x) => world.ordnance[list].indexOf(x)), world.ordnance[list].map((_, i) => i));
  assert.ok(Object.is(system.start.y, -0) && system.start.x === 0.5 && system.start.z === 2);
  // Every reference came back as the very object written.
  const [first, second, third] = b.read(KIND_BOLT).map((r) => r.object);
  assert.equal(first.owner, world.units[0]);
  assert.equal(first.def, world.ordnance.bolts[0].def);
  assert.equal(first.impact.body, world.bodies[1]);
  assert.equal(second.owner, 'player');
  assert.equal(second.impact.unit, world.units[2]);
  assert.ok(Object.is(second.previousDrawn, -0) && Object.is(second.life, -0) && Number.isNaN(second.position.x));
  assert.equal(third.owner, world.snapshot);
  assert.equal(third.previousFlown, undefined);
  const rounds = b.read(KIND_ROUND).map((r) => r.object);
  assert.deepEqual([...rounds[1].struck], [world.units[2], world.units[1]]);
  assert.equal(rounds[0].flightClass, 'fighter');
  assert.equal(rounds[2].impact.body, undefined);
  assert.equal('muzzleGeneration' in rounds[1], false);
  const beams = b.read(KIND_BEAM).map((r) => r.object);
  assert.ok(Object.is(beams[1].damageDef.Damage, -0));
});

// A kind with a home list (a bolt's is ordnance.bolts) is listed in that
// list's order, so its home goes in with it.
const system = (lists) => ({ bolts: [], beams: [], grenades: [], rounds: [], start: new THREE.Vector3(), ...lists });

test('a kind JavaScript still owns comes in by its import fields, and one that leaves its list leaves the store', async () => {
  const world = battle();
  const b = await bridge(world);
  b.write(KIND_BOLT, world.ordnance.bolts);
  b.write(KIND_ORDNANCE, [world.ordnance]);
  assert.equal(b.read(KIND_BOLT).length, 3);
  world.ordnance.bolts.splice(1, 1);
  b.write(KIND_BOLT, world.ordnance.bolts);
  b.write(KIND_ORDNANCE, [world.ordnance]);
  const back = b.read(KIND_BOLT);
  assert.equal(back.length, 2);
  back.forEach(({ object }, i) => assert.equal(KIND_TABLE[KIND_BOLT].same(world.ordnance.bolts[i], object), null));
});

test('a partial write changes only the listed entities and an empty partial write keeps them all', async () => {
  const world = battle(), b = await bridge(world);
  writeAll(b, world.ordnance, true);
  const before = b.read(KIND_BOLT);
  const changed = world.ordnance.bolts[1];
  changed.position.set(19, -0, 7);
  b.writeSome(KIND_BOLT, [changed]);
  b.writeSome(KIND_BOLT, []);
  const after = b.read(KIND_BOLT);
  assert.equal(after.length, 3);
  for (const i of [0, 2]) {
    assert.equal(KIND_TABLE[KIND_BOLT].same(before[i].object, after[i].object), null);
  }
  assert.deepEqual(after[1].object.position, changed.position);
  assert.equal(after[1].object.owner, changed.owner);
});

test('mirrors keep their identity and their Vector3s, and a new life of a slot gets a new mirror', async () => {
  const world = battle();
  const b = await bridge(world);
  b.write(KIND_BEAM, world.ordnance.beams);
  b.write(KIND_ORDNANCE, [system({ beams: world.ordnance.beams })]);
  const pool = b.own(KIND_BEAM);
  assert.equal(b.sync(KIND_BEAM), 2);
  const [mirror, other] = pool.objects;
  const position = mirror.position;
  // Every mirrored field is the beam's (damage is not mirrored).
  assert.equal(KIND_TABLE[KIND_BEAM].same(world.ordnance.beams[0], mirror), '.damage');
  // The store's entity changes (written here as Rust would change it, by
  // slot and generation): the next sync updates the same mirror and the same
  // Vector3 in place, so whatever holds them sees the change.
  const moved = { ...world.ordnance.beams[0], position: new THREE.Vector3(7, 8, 9), simSlot: mirror.simSlot, simGen: mirror.simGen };
  const kept = { ...world.ordnance.beams[1], simSlot: other.simSlot, simGen: other.simGen };
  b.write(KIND_BEAM, [moved, kept]);
  b.write(KIND_ORDNANCE, [system({ beams: [moved, kept] })]);
  b.sync(KIND_BEAM);
  assert.equal(pool.objects[mirror.simSlot], mirror);
  assert.equal(mirror.position, position);
  assert.deepEqual(position.toArray(), [7, 8, 9]);
  // The slot's next life (its generation moved on) is another object, as
  // JavaScript's next beam was.
  const next = { ...moved, simGen: mirror.simGen + 1 };
  b.write(KIND_BEAM, [next]);
  b.write(KIND_ORDNANCE, [system({ beams: [next] })]);
  assert.equal(b.sync(KIND_BEAM), 1);
  assert.notEqual(pool.objects[mirror.simSlot], mirror);
  assert.equal(pool.objects[mirror.simSlot].simGen, mirror.simGen + 1);
});

test('a retired entity\'s last state reaches the mirror JavaScript still holds, and its slot\'s next life is a new mirror', async () => {
  const world = battle();
  const b = await bridge(world);
  b.write(KIND_GRENADE, world.ordnance.grenades);
  b.write(KIND_ORDNANCE, [system({ grenades: world.ordnance.grenades })]);
  const pool = b.own(KIND_GRENADE);
  b.sync(KIND_GRENADE);
  const [first, second] = pool.objects;
  // A sim event kept the first grenade's position, as one keeps g.position.
  const kept = first.position;
  // The store moves it (as Rust would, by slot) and retires it: it went off.
  const last = { ...world.ordnance.grenades[0], position: new THREE.Vector3(9, 8, 7), gone: true, simSlot: first.simSlot, simGen: first.simGen };
  const other = { ...world.ordnance.grenades[1], simSlot: second.simSlot, simGen: second.simGen };
  b.write(KIND_GRENADE, [last, other]);
  b.write(KIND_ORDNANCE, [system({ grenades: [last, other] })]);
  b.retire(KIND_GRENADE, first.simSlot, first.simGen);
  b.write(KIND_ORDNANCE, [system({ grenades: [other] })]);
  // The detector leaves the retired for the sync; the sync hands them over once.
  assert.deepEqual(b.check(KIND_GRENADE), []);
  assert.equal(b.sync(KIND_GRENADE), 2);
  assert.deepEqual(kept.toArray(), [9, 8, 7]);
  assert.equal(first.gone, true);
  assert.equal(pool.objects[first.simSlot], undefined);
  assert.equal(b.sync(KIND_GRENADE), 1);
  assert.throws(() => b.retire(KIND_GRENADE, first.simSlot, first.simGen), /no entity/);
});

test('the mirror write detector names a field JavaScript changed without a command', async () => {
  const world = battle();
  const b = await bridge(world);
  b.write(KIND_BOLT, world.ordnance.bolts);
  b.write(KIND_ORDNANCE, [system({ bolts: world.ordnance.bolts })]);
  b.own(KIND_BOLT);
  b.sync(KIND_BOLT);
  assert.deepEqual(b.check(KIND_BOLT), []);
  b.refs[KIND_BOLT].objects[2].life = 5;
  b.refs[KIND_BOLT].objects[0].position.x = -0;
  assert.deepEqual(b.check(KIND_BOLT), ['bolt[0].position', 'bolt[2].life']);
});

test('views are made again when the memory grows, and what was written is still there', async () => {
  const world = battle();
  const b = await bridge(world);
  writeAll(b, world.ordnance, true);
  const before = b.memory.buffer.byteLength, grown = b.grown;
  b.scratch(new Uint8Array(before + (8 << 20)));
  assert.ok(b.memory.buffer.byteLength > before);
  for (const [kind, list] of LISTS) b.read(kind).forEach(({ object }, i) => assert.equal(KIND_TABLE[kind].same(world.ordnance[list][i], object), null));
  assert.equal(b.grown, grown + 1);
  // Strings interned before and after the growth keep their indices.
  const id = b.strings.id('fighter');
  b.scratch(new Uint8Array(b.memory.buffer.byteLength + (1 << 20)));
  assert.equal(b.strings.id('fighter'), id);
  b.strings.list.length = 0; b.strings.ids.clear();
  assert.equal(b.strings.at(id), 'fighter');
});

test('a command runs at once on its arguments, and its events come out in order', async () => {
  const world = battle();
  const b = await bridge(world);
  const at = new THREE.Vector3(1, -0, 3);
  const result = b.command('echo', { value: 1.5, at, count: 7, note: 'hi', events: 3 });
  assert.equal(result.value, 2.5);
  assert.equal(result.count, 7);
  assert.equal(result.note, 'hi');
  assert.ok(Object.is(result.at.y, -0) && result.at !== at);
  const seen = [];
  assert.equal(b.drain({ echoed: (e) => seen.push(e) }), 3);
  assert.deepEqual(seen.map((e) => [e.value, e.note, e.at.z]), [[0, 'hi', 3], [1, 'hi', 3], [2, 'hi', 3]]);
  assert.equal(b.drain({}), 0);
  assert.equal(b.command('echo', { value: -0, at, count: 0, note: null, events: 0 }).note, null);
  assert.throws(() => b.command('nothing', {}), /no command nothing/);
});

test('room made as a battle loads keeps the memory from growing in play', async () => {
  const world = battle();
  const b = await bridge(world);
  const pages = b.reserve(8 << 20);
  assert.ok(pages * 65536 >= 8 << 20);
  const grown = b.grown;
  for (let i = 0; i < 50; i++) {
    writeAll(b, world.ordnance, true);
    for (const [kind] of LISTS) b.read(kind);
    b.command('fill', { kind: KIND_BOLT, count: 20 });
    b.write(KIND_BOLT, world.ordnance.bolts);
  }
  assert.equal(b.x.sim_memory_pages(), pages);
  assert.equal(b.grown, grown);
});

test('an import asks JavaScript in the middle of a command and takes its answer', async () => {
  const world = battle();
  const b = await bridge(world);
  const asked = [];
  b.handlers.ask = (args) => { asked.push(args.value); return { ...args, value: args.value * 3, note: 'answered' }; };
  const at = new THREE.Vector3(1, 2, 3);
  const result = b.command('relay', { value: 2.5, at, count: 1, note: null, events: 0 });
  assert.deepEqual(asked, [2.5]);
  assert.equal(result.value, 7.5);
  assert.equal(result.note, 'answered');
  assert.deepEqual(result.at.toArray(), [1, 2, 3]);
  // The command's own call block was not disturbed by the import's.
  assert.equal(b.command('echo', { value: 1, at, count: 2, note: null, events: 0 }).value, 2);
  delete b.handlers.ask;
  assert.throws(() => b.command('relay', { value: 1, at, count: 0, note: null, events: 0 }), /nothing serves/);
});

test('an instance has one bridge, which every phase joins and serves its own imports through', async () => {
  const instance = await loadSim();
  const b = bridgeFor(instance, { tests: { emitter: () => false } });
  assert.equal(bridgeFor(instance, { tests: { ownerSnapshot: () => false } }), b);
  assert.throws(() => new SimBridge(instance), /has a bridge already/);
  assert.ok(b.refs[KIND_EMITTER].test && b.refs[KIND_OWNER_SNAPSHOT].test);
  b.register({ ask: (args) => args });
  assert.throws(() => b.register({ ask: (args) => args }), /served already/);
  assert.throws(() => b.register({ noSuchImport: () => null }), /no schema declares/);
  // A tool that empties the store and starts over may replace it.
  assert.notEqual(new SimBridge(instance, { replace: true }), b);
});

test('a module whose kinds were made from other schemas is refused', async () => {
  const { exports } = await loadSim();
  const stale = { exports: { ...exports, sim_kind_hash: (k) => (k === KIND_BOLT ? 1 : exports.sim_kind_hash(k)) } };
  assert.throws(() => new SimBridge(stale), /lays bolt out otherwise/);
});

// The export context a test of the exporters uses: the union's kinds told
// apart by their classes and tests, references and definitions as tagged
// values (the replay harness makes its handles here).
function exporting(world) {
  return new ExportContext({
    classes: { Unit, Vehicle },
    tests: { emitter: (o) => world.emitters.includes(o), ownerSnapshot: (o) => o === world.snapshot },
    ref: (kind, object) => ({ kind, object }),
    def: (table, def) => ({ table, def }),
  });
}

test('the replay harness gets plain values in the schema\'s order, one value an entity, references as the harness names them', () => {
  const world = battle();
  const cx = exporting(world);
  const value = exportState('ordnance', world.ordnance, cx);
  assert.deepEqual(Object.keys(value), ['bolts', 'beams', 'grenades', 'rounds', 'start']);
  const bolt = value.bolts[0];
  assert.deepEqual(Object.keys(bolt).slice(0, 4), ['$id', 'def', 'owner', 'networkFireId']);
  assert.deepEqual(bolt.$id, { kind: 'bolt', object: world.ordnance.bolts[0] });
  assert.deepEqual(bolt.owner, { kind: 'unit', object: world.units[0] });
  assert.deepEqual(bolt.def, { table: 'ordnance', def: world.ordnance.bolts[0].def });
  // A copy of the vector, so a capture keeps no alias the store cannot.
  assert.notEqual(bolt.position, world.ordnance.bolts[0].position);
  assert.deepEqual(bolt.position.toArray(), world.ordnance.bolts[0].position.toArray());
  assert.notEqual(bolt.impact, world.ordnance.bolts[0].impact);
  assert.deepEqual(bolt.impact.body, { kind: 'body', object: world.bodies[1] });
  assert.equal(value.bolts[1].owner, 'player');
  assert.deepEqual(value.bolts[2].owner, { kind: 'ownerSnapshot', object: world.snapshot });
  assert.deepEqual(value.beams[0].source, { kind: 'emitter', object: world.emitters[0] });
  assert.equal('mount' in value.rounds[1], false);
  assert.deepEqual([...value.rounds[1].struck].map((r) => r.object), [world.units[2], world.units[1]]);
  assert.equal(value.beams[0].damage, 12.5);
  assert.equal('damageDef' in value.beams[0], false);
  // The same bolt met again is the same value.
  assert.equal(KIND_TABLE[KIND_BOLT].export(world.ordnance.bolts[0], cx), bolt);
});

test('the completeness check names a field no schema lists', () => {
  const { ordnance } = battle();
  assert.deepEqual(undeclared(ordnance.bolts[0], BOLT_KEYS), []);
  ordnance.bolts[0].surprise = 1;
  ordnance.bolts[0].visualMuzzle = () => null;
  assert.deepEqual(undeclared(ordnance.bolts[0], BOLT_KEYS), ['surprise']);
});

test('the generated code is what the schemas make', async () => {
  const { out } = await generate();
  for (const [path, text] of out) assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node tools/sim-schema.mjs`);
});
