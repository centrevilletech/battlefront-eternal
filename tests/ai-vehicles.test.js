import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
import { Units } from '../src/registry.js';
import { blankIntent } from '../src/units.js';
import { Turret } from '../src/props.js';
import { Gunner } from '../src/gunner.js';
import { TurretSeats, classicSeatExit, exitCandidates, exitSupportOffsets, highestSupport, KIND } from '../src/seats.js';

// Synthetic fixtures only: a flat floor, box hulls, made-up weapons.

// Identity checks that never inspect their operands: a failing assert.equal
// on a unit, a lease or a vehicle would print its whole object graph (scene,
// collision trees, the AI), which can exhaust memory.
const show = (x) => (x === null || typeof x !== 'object' ? String(x) : `[${x.constructor?.name ?? 'object'}]`);
const same = (actual, expected, message) => assert.ok(actual === expected, message ?? `${show(actual)} !== ${show(expected)}`);

const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
const floor = () => { const b = box(0, -1, 0, 400, 2, 400); return new CollisionWorld(b.positions, b.indices, b.polygons); };
const identity = () => new THREE.Matrix4();

const cannon = (extra = {}) => ({ class: 'test_cannon', ammo: 0, aimers: [], def: { class: 'test_cannon', base: 'cannon', ShotDelay: 0.2, RoundsPerClip: 0, ReloadTime: 0, TriggerSingle: 0, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 2, Damage: 50 }, ...extra } });

function fixture({ base = 'hover', single = false, seats = 2, extra = {} } = {}) {
  const c = floor();
  const hull = box(0, 1, 0, 2, 2, 4);
  const settings = { MaxHealth: 1000, ForwardSpeed: 20, ReverseSpeed: 8, StrafeSpeed: 4, Acceleration: 10, Deceleration: 10, SetAltitude: 0.5, TurnRate: 1.7, PitchRate: 0.5, ...extra };
  const seatDefs = [{ name: 'BODY', settings: {}, weapons: [cannon(single ? { TriggerSingle: 1 } : {})] }];
  for (let i = 1; i < seats; i++) seatDefs.push({ name: `TURRET${i}`, settings: { TurnRate: 1.5, PitchRate: 0.5 }, weapons: [cannon(single ? { TriggerSingle: 1 } : {})] });
  const def = { model: 'tank', base, label: 'Test tank', settings, seats: seatDefs, explosion: null };
  const spawns = [0, 40].map((x, i) => ({ name: `pad${i}`, matrix: identity().makeTranslation(x, 0, 0).toArray(), team: 1, post: '', respawnTime: 2, classes: { 1: 'tank' } }));
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: hull }, classes: { tank: def }, spawns };
  const fired = [];
  const ordnance = { fire: (def, origin, dir, owner, visual, ignoreBody, emitter) => fired.push({ def, origin: origin.clone(), dir: dir.clone(), owner, ignoreBody, emitter }), explosion() {}, lights: { flash() {} } };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision: c, texture: () => null, camera: new THREE.PerspectiveCamera(), posts: { posts: [] }, ordnance, effects: { play() { return null; } } });
  return { c, system, fired, a: system.vehicles[0], b: system.vehicles[1] };
}

let nextId = 100;
function bot(team = 1, { classDef = {} } = {}) {
  const soldier = {
    position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, air: null, fuel: 0.3, spawns: 0,
    spawn(x, y, z, yaw) { this.position.set(x, y, z); this.aimYaw = yaw; this.velocity.set(0, 0, 0); this.fuel = 0.5; this.spawns += 1; },
  };
  const unit = {
    id: nextId++, generation: 1, team, alive: true, dead: false, human: false, walker: false, seat: null, controller: null, character: { kills: 0 },
    classDef, intent: blankIntent(), soldier,
    actor: { root: new THREE.Group(), byName: {} },
    animator: { clips: new Map(), gunning: null, gunner(clip) { this.gunning = clip; }, reset() {} },
    presentation: { reset() {} },
    combat: { mounted: false, dead: false, team, deaths: [], damage(amount, onShield, point, attacker) {
      if (this.dead) return;
      this.dead = true; unit.alive = false; unit.dead = true; this.deaths.push(attacker);
      this.onDeath?.();
    } },
  };
  return unit;
}

const run = (system, seconds, dt = 1 / 60) => { for (let t = 0; t < seconds - 1e-9; t += dt) system.update(dt); };

test('two hulls driven at once by two bot pilots, each with its own gunner', () => {
  const { system, fired, a, b } = fixture();
  const [pilotA, gunnerA, pilotB, gunnerB] = [bot(), bot(), bot(), bot()];
  assert.ok(system.board(pilotA, a, 0));
  assert.ok(system.board(gunnerA, a, 1));
  assert.ok(system.board(pilotB, b, 0));
  assert.ok(system.board(gunnerB, b, 1));
  same(pilotA.seat.vehicle, a);
  same(gunnerB.seat.index, 1);
  same(gunnerB.seat.role, 'gunner');
  same(pilotA.combat.mounted, true);
  pilotA.intent.forward = 1;
  pilotB.intent.forward = 1;
  pilotB.intent.turn = 1; // right: yaw goes down
  gunnerA.intent.turn = -1; // its turret left
  gunnerA.intent.buttons[0] = 5;
  gunnerB.intent.pitch = 1;
  gunnerB.intent.aim = new THREE.Vector3(1, 0, 0);
  gunnerB.intent.buttons[0] = 5;
  const startA = a.position.clone(), startB = b.position.clone();
  run(system, 2);
  assert.ok(a.position.z - startA.z > 10, 'A drives straight ahead');
  assert.ok(Math.abs(a.position.x - startA.x) < 1e-6 && Math.abs(a.yaw) < 1e-9, 'A does not turn');
  assert.ok(b.yaw < -1, 'B turns right');
  assert.ok(b.position.distanceTo(startB) > 10, 'B moves too');
  assert.ok(a.seats[1].aimYaw > 1.5, "A's gunner turns its own turret");
  same(a.seats[0].aimYaw, a.yaw, "A's pilot aim is the hull's");
  assert.ok(b.seats[1].pitch > 0.4, "B's gunner raises its gun");
  const fromA = fired.filter((f) => f.ignoreBody === a.body), fromB = fired.filter((f) => f.ignoreBody === b.body);
  assert.ok(fromA.length >= 5 && fromB.length >= 5, `${fromA.length} / ${fromB.length}`);
  assert.ok(fromA.every((f) => f.owner === gunnerA), "A's rounds are its gunner's");
  assert.ok(fromB.every((f) => f.owner === gunnerB), "B's rounds are its gunner's");
  assert.ok(fromB.every((f) => f.dir.distanceTo(new THREE.Vector3(1, 0, 0)) < 1e-6), 'a bot fires along its intent.aim');
  assert.ok(fromA.every((f) => f.emitter === a.seats[1].emitter), 'a beam would follow the seat it was fired from');
  // The bodies ride along: at their seats, hidden inside an enclosed hull.
  assert.ok(pilotA.soldier.position.distanceTo(a.position) < 3);
  same(pilotA.actor.root.visible, false);
});

test('boarding and destruction in the same tick: seats cleared at once, crew dead and credited', () => {
  const { system, a } = fixture();
  const pilot = bot(), gunner = bot(), shooter = bot(2);
  assert.ok(system.board(pilot, a, 0));
  assert.ok(system.board(gunner, a, 1));
  pilot.intent.forward = 1;
  let seatsAtFirstDeath = null;
  pilot.combat.onDeath = () => { seatsAtFirstDeath = a.seats.map((s) => s.lease); };
  a.damage(5000, shooter);
  same(a.alive, false);
  assert.ok(seatsAtFirstDeath && seatsAtFirstDeath.every((l) => l === null), 'every seat is let go before anyone dies');
  assert.ok(!pilot.alive && !gunner.alive);
  assert.ok(pilot.combat.deaths[0] === shooter && gunner.combat.deaths[0] === shooter, 'deaths credited to the destroyer');
  same(pilot.seat, null);
  same(pilot.combat.mounted, false);
  const at = a.position.clone();
  system.update(1 / 60);
  assert.ok(a.position.equals(at), 'a destroyed hull takes no more input');
  same(system.board(bot(), a, 0), false, 'no boarding a wreck');
  // Same tick the other way round: a seat taken as the hull dies.
  const { system: s2, a: v2 } = fixture();
  const late = bot();
  v2.damage(Infinity, shooter);
  same(s2.board(late, v2, 0), false);
  same(late.seat, null);
});

test('one lease per seat; the player takes a bot-held seat and the bot moves on', () => {
  const { system, a, b, c } = fixture();
  const one = bot(), two = bot(), three = bot();
  assert.ok(system.board(one, a, 0));
  same(system.board(two, a, 0), false, 'seat 0 is taken');
  same(system.board(one, b, 0), false, 'a seated unit cannot board again');
  assert.ok(system.board(two, a));
  same(two.seat.index, 1);
  same(system.board(three, a), false, 'full');
  same(system.eligibleSeat(three, a), -1);
  same(system.eligibleSeat(three, b), 0);
  same(system.eligibleSeat(bot(2), b), -1, "a team's hull takes only its own team's bots");
  same(system.eligibleSeat(bot(1, { classDef: { NoEnterVehicles: 1 } }), b), -1);
  same(system.board(bot(1, { classDef: { NoEnterVehicles: 1 } }), b), false, 'direct boarding honors the class prohibition too');
  const droideka = bot(); droideka.walker = true;
  same(system.board(droideka, b, 0), false, 'droidekas do not board');
  // The player boards B, held by a pilot bot: he takes seat 0, the bot the next free seat.
  assert.ok(system.board(three, b, 0));
  const soldier = { position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, spawn(x, y, z, yaw) { this.position.set(x, y, z); this.aimYaw = yaw; } };
  const player = { team: 1, generation: 1, alive: true, seat: null };
  const combat = { dead: false, mounted: false, team: 1, owner: player };
  const driver = new VehicleDriver({ vehicles: system, soldier, combat, actor: { root: new THREE.Group() }, animator: {}, clips: new Map(), camera: system.camera, collision: c, hud: { message() {} } });
  assert.ok(driver.enter(b));
  same(b.seatIndex, 0);
  same(b.seats[0].lease.human, true);
  same(player.seat, b.seats[0].lease, "the player's unit holds the seat");
  same(three.seat.index, 1, 'the displaced pilot took the gunner seat');
  // F swaps him with the bot.
  driver.update(1 / 60, { take: (k) => k === 'nextPrimary', held: () => false });
  same(b.seatIndex, 1);
  same(b.seats[1].lease, player.seat);
  same(three.seat.index, 0);
  // Both A seats bot-held: as the original game seats a player, the first seat
  // a bot holds; its pilot has nowhere to go but out.
  driver.release();
  same(player.seat, null);
  a.seatIndex = 1;
  assert.ok(driver.enter(a));
  same(a.seatIndex, 0);
  same(one.seat, null, 'no free seat: the displaced pilot got out');
  assert.ok(one.soldier.spawns > 0);
  same(two.seat.index, 1);
});

test('edges are taken once per batch, whatever the substeps; held values hold', () => {
  const { system, fired, a } = fixture({ single: true });
  const gunner = bot();
  assert.ok(system.board(gunner, a, 1));
  gunner.intent.buttons[0] = 1; // pressed and held
  system.update(1 / 30); // four 120 Hz substeps, one batch
  for (let i = 0; i < 30; i++) system.update(1 / 60);
  same(fired.length, 1, 'a single-shot trigger held down fires once');
  gunner.intent.buttons[0] = -1;
  system.update(1 / 60);
  gunner.intent.buttons[0] = 1;
  for (let i = 0; i < 30; i++) system.update(1 / 60);
  same(fired.length, 2, 'release and press again: one more');
  gunner.intent.reload = true;
  gunner.intent.jump = true;
  system.update(1 / 60);
  same(gunner.intent.reload, false, 'reload taken');
  same(gunner.intent.jump, false, 'a jump means nothing in a seat and is dropped');
  // The pilot's held throttle reaches every substep of a batch.
  const pilot = bot();
  assert.ok(system.board(pilot, a, 0));
  pilot.intent.forward = 1;
  system.update(1 / 60);
  const speed = a.velocity.length();
  assert.ok(speed > 0);
  // A use edge leaves the vehicle exactly once.
  pilot.intent.use = true;
  system.update(1 / 60);
  same(pilot.seat, null);
  same(pilot.intent.use, false);
  same(gunner.seat.index, 0, "the gunner took the pilot's seat (promotion)");
  same(system.board(pilot, a, 1), true, 'back in through the free seat');
});

test('stale leases steer nothing: a respawned or dead occupant is let go', () => {
  const { system, a } = fixture();
  const pilot = bot();
  assert.ok(system.board(pilot, a, 0));
  pilot.intent.forward = 1;
  pilot.generation += 1; // died and respawned elsewhere; the integrator's spawn reset left seat as is
  const start = a.position.clone();
  const where = pilot.soldier.position.clone();
  system.update(1 / 60);
  same(a.seats[0].lease, null);
  same(pilot.seat, null);
  assert.equal(a.position.x, start.x, 'no lateral steering from a stale life');
  assert.equal(a.position.z, start.z, 'no forward steering from a stale life; suspension may settle vertically');
  assert.ok(pilot.soldier.position.equals(where), 'a stale body is not moved by the exit');
  // A unit that died in its seat.
  const gunner = bot();
  assert.ok(system.board(gunner, a, 1));
  gunner.alive = false; gunner.dead = true;
  system.update(1 / 60);
  same(a.seats[1].lease, null);
  // A lease from a past life never matches the unit's current seat.
  const again = bot();
  assert.ok(system.board(again, a, 0));
  const old = again.seat;
  system.unboard(again);
  assert.ok(system.board(again, a, 1));
  same(old.live, false);
  same(again.seat.live, true);
});

test('vehicles and turrets are registry targets without hit spheres', () => {
  const { system, a, b } = fixture();
  const units = new Units();
  system.attachUnits(units);
  system.attachUnits(units); // idempotent
  const entries = units.all();
  same(entries.length, 2);
  const entry = a.aiTarget;
  same(entry.kind, KIND.hover);
  same(entry.vehicle, a);
  same(entry.hitSpheres, undefined);
  same(entry.team, 1);
  assert.ok(entry.alive);
  same(entry.combat.health, a.health);
  assert.ok(Math.abs(entry.sphere.radius - Math.sqrt(2 * 2 + 2 * 2 + 4 * 4) / 2) < 1e-5);
  const near = units.query(a.position, 5);
  assert.ok(near.length === 1 && near[0].vehicle === a);
  const ray = units.raycast(new THREE.Vector3(0, 1, -10), new THREE.Vector3(0, 0, 1), 100);
  same(ray, null, 'soldier raycasts pass vehicles by');
  const ref = units.ref(entry);
  const enemy = bot(2);
  enemy.team = 2;
  b.team = 2;
  same(b.aiTarget.team, 2);
  a.team = 0;
  same(entry.team, 0);
  assert.ok(system.board(enemy, a, 0));
  same(entry.team, 2, 'controlled by its occupant');
  a.damage(Infinity);
  same(units.resolve(ref), null);
  system.updateSpawns(5);
  assert.ok(a.alive);
  same(units.resolve(ref), null, 'a respawned hull is a new life');
  assert.ok(units.resolve(units.ref(entry)));
  // Kinds of the other bases.
  same(fixture({ base: 'flyer' }).a.aiTarget.kind, KIND.flyer);
  same(fixture({ base: 'walker' }).a.aiTarget.kind, KIND.walker);
  same(fixture({ base: 'commandwalker' }).a.aiTarget.kind, KIND.walker);
});

test('a bot pilot uses the common takeoff and landing states', () => {
  // A snowspeeder's lift: 2.5 m up at TakeoffSpeed 3, over to flight in 1 s;
  // its landing out of flight in 2 s, sinking at LandingSpeed 5.
  const { system, a } = fixture({ base: 'flyer', extra: { TakeoffHeight: '2.5', TakeoffSpeed: '3.0', TakeoffTime: '1.0', LandingTime: '2.0', LandingSpeed: '5.0' } }); const pilot=bot();
  assert.ok(system.board(pilot,a,0)); pilot.intent.up=1; run(system,2.5,1/60);
  same(a.flightState,'Flying'); pilot.intent.up=0;pilot.intent.forward=-1;run(system,3,1/60);
  pilot.intent.down=1;run(system,8,1/60);same(a.flightState,'Landed');
});

test('exits: the original game placement for bots, the player\'s search kept', () => {
  const { system, a, c } = fixture();
  const pilot = bot();
  assert.ok(system.board(pilot, a, 0));
  pilot.soldier.velocity.set(9, 9, 9);
  assert.ok(system.unboard(pilot));
  // Candidate 0 is behind the hull, R = bounding radius + 0.684 out, on the floor.
  const R = Math.fround(Math.fround(a.boundRadius) + Math.fround(0.6840001));
  assert.ok(Math.abs(pilot.soldier.position.z - (a.position.z - R)) < 1e-4, `${pilot.soldier.position.toArray()}`);
  assert.ok(Math.abs(pilot.soldier.position.y) < 1e-4, 'on the floor');
  same(pilot.soldier.velocity.length(), 0, 'exit velocity is zero');
  same(pilot.soldier.fuel, 0.3, 'jet fuel kept');
  same(pilot.combat.mounted, false);
  same(pilot.actor.root.visible, true);
  // Boxed in by a wall right behind: the line from the centre is blocked, the next candidate (left) is used.
  const wall = box(0, 3, -3.2, 10, 6, 0.2);
  c.addBody(wall.positions, wall.indices, wall.polygons);
  assert.ok(system.board(pilot, a, 0));
  assert.ok(system.unboard(pilot));
  assert.ok(Math.abs(pilot.soldier.position.x - (a.position.x - R)) < 1e-4, 'second candidate: -R along the frame right');
  // The safe policy refuses when the player's search finds nothing, unless forced.
  assert.ok(system.board(pilot, a, 0));
  const cast = c.cast;
  c.cast = () => null;
  same(system.unboard(pilot, { policy: 'safe' }), false);
  assert.ok(pilot.seat);
  same(system.unboard(pilot, { policy: 'safe', force: true }), true);
  c.cast = cast;
});

test('classic seat-exit primitives match the reference fixture', { skip: !existsSync('extract/ai-research/round9/root/exit-native.json') }, () => {
  const native = JSON.parse(readFileSync('extract/ai-research/round9/root/exit-native.json', 'utf8'));
  for (const kase of native.candidateCases) {
    const ours = exitCandidates(kase.radius);
    kase.candidates.forEach((p, i) => p.forEach((v, j) => assert.ok(Math.abs(ours[i][j] - v) < 1e-6, `r=${kase.radius} #${i}: ${ours[i]} vs ${p}`)));
  }
  const offsets = exitSupportOffsets();
  for (const kase of native.worldSampleCases) {
    kase.samples.forEach((p, i) => p.forEach((v, j) => assert.ok(Math.abs(kase.center[j] + offsets[i][j] - v) < 2e-6)));
  }
  for (const kase of native.heightSelections) same(highestSupport(kase.heights), kase.chosen);
});

test('classic seat-exit: corner support, fallback to the origin, water is no floor', () => {
  // A stub world: rays down hit only where `ground(x, z)` says.
  const world = (ground, blocked = () => false) => ({
    cast(origin, dir, far, hit) {
      if (dir.y > 0.5) return null;
      if (dir.y < -0.5) {
        const h = ground(origin.x, origin.z);
        if (h === null || origin.y - h > far || origin.y < h) return null;
        hit.distance = origin.y - h;
        return hit;
      }
      return blocked(origin, dir, far) ? (hit.distance = 1, hit) : null;
    },
  });
  const frame = { origin: new THREE.Vector3(0, 0, 0), forward: new THREE.Vector3(0, 0, 1), center: new THREE.Vector3(0, 1, 0), radius: 2 };
  // Only a corner sample of the first candidate finds a floor: X/Z from the candidate, Y from the corner.
  const corner = classicSeatExit(world((x, z) => (x > 0.5 && z < -3 ? 2 : null)), frame);
  same(corner.candidate, 0);
  assert.ok(Math.abs(corner.position.x) < 1e-9 && Math.abs(corner.position.y - 2) < 1e-6);
  // No support anywhere (open water has none): the vehicle's origin.
  const none = classicSeatExit(world(() => null), frame);
  same(none.candidate, -1);
  assert.ok(none.position.equals(frame.origin));
  // Every radial line blocked: only the centre candidate (8) remains.
  const centre = classicSeatExit(world(() => 0, () => true), frame);
  same(centre.candidate, 8);
});

function turretFixture(weaponSettings = {}) {
  const collision = floor();
  const rig = { root: new THREE.Group(), byName: {}, bind: new Map() };
  for (const name of ['drum', 'gun', 'seat', 'hp_fire']) { const o = new THREE.Object3D(); o.name = name; rig.byName[name] = o; }
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.gun.add(rig.byName.hp_fire); rig.byName.drum.add(rig.byName.seat);
  rig.byName.drum.position.y = 1.5; rig.byName.hp_fire.position.z = 1;
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  const desc = { base: 'armedbuilding', class: 'test_turret', matrix: identity().toArray(), settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'hp_fire', MaxTurnSpeed: 2, MaxHealth: 300 }, weapon: { class: 'test_turret_gun', base: 'cannon', ShotDelay: 0.25, RoundsPerClip: 0, ReloadTime: 0, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 2, Damage: 20 } } };
  Object.assign(desc.weapon, weaponSettings);
  const turret = new Turret(desc, { built: rig }, { built: null }, null);
  const fired = [];
  const ordnance = { fire: (def, origin, dir, owner, visual, ignoreBody, emitter) => fired.push({ owner, dir: dir.clone(), emitter }) };
  const crews = new TurretSeats({ live: { turrets: [turret] }, ordnance, collision });
  const units = new Units();
  crews.attachUnits(units);
  return { collision, rig, turret, fired, ordnance, crews, units };
}

test('bots man turrets: drum from the analogs, rounds credited, death with the turret', () => {
  const { turret, crews, rig, fired, units } = turretFixture();
  same(crews.man(bot(1, { classDef: { NoEnterVehicles: 1 } }), turret), false, 'heroes cannot man turrets');
  same(turret.aiTarget.kind, KIND.armedBuilding);
  same(turret.aiTarget.team, 0, 'unmanned turrets belong to no one');
  const gunner = bot(2);
  assert.ok(crews.man(gunner, turret));
  same(crews.man(bot(), turret), false, 'one gunner');
  same(turret.gunner, gunner.soldier);
  same(turret.aiTarget.team, 2);
  gunner.intent.turn = -1;
  gunner.intent.buttons[0] = 3;
  for (let i = 0; i < 60; i++) crews.update(1 / 60);
  assert.ok(turret.yaw > 0.5, `left turn raises yaw: ${turret.yaw}`);
  assert.ok(fired.length >= 3 && fired.every((f) => f.owner === gunner));
  assert.ok(gunner.soldier.position.distanceTo(rig.byName.seat.getWorldPosition(new THREE.Vector3())) < 1e-9, 'body at the seat');
  const killer = bot(1);
  const oldTarget = units.ref(turret.aiTarget);
  turret.damage(1000, killer);
  same(turret.alive, false, 'a turret without a wreck model is dead too');
  same(gunner.alive, false);
  same(gunner.combat.deaths[0], killer);
  same(turret.lease, null);
  same(turret.gunner, null);
  // A use edge gets a gunner out, onto the floor beside it.
  turret.setHealth(300);
  same(turret.alive, true);
  same(units.resolve(oldTarget), null, 'rebuilding cannot revive a stale target reference');
  same(turret.lastAttacker, null);
  const next = bot(1);
  assert.ok(crews.man(next, turret));
  next.intent.use = true;
  crews.update(1 / 60);
  same(next.seat, null);
  same(turret.lease, null);
  assert.ok(next.soldier.position.distanceTo(turret.position) > 2);
});

test('bot turret spread is seeded and its beam emitter ends when the gunner leaves', () => {
  const fire = () => {
    const env = turretFixture({ SpreadThreshold: 2, SpreadPerShot: 1, MaxSpread: 5, YawSpread: 2, PitchSpread: 1 });
    const unit = bot();
    env.crews.man(unit, env.turret);
    same(env.turret.weapon.floor, 0);
    const random = new ClassicRandom(31);
    env.turret.weapon.random = () => random.uniform();
    unit.intent.buttons[0] = 1;
    for (let i = 0; i < 30; i++) env.crews.update(1 / 30);
    return { ...env, unit };
  };
  const a = fire(), b = fire();
  assert.ok(a.fired.length > 2);
  assert.deepEqual(a.fired.map((s) => s.dir.toArray()), b.fired.map((s) => s.dir.toArray()));
  assert.ok(a.fired.some((s) => Math.abs(s.dir.x) > 0.001), 'spread reaches the projectile direction');
  const emitter = a.fired[0].emitter;
  same(emitter.alive, true);
  same(emitter.matrix, a.turret.gun.matrixWorld);
  a.crews.leave(a.unit, { placement: false });
  same(emitter.alive, false);
  a.crews.man(bot(), a.turret);
  same(emitter.alive, false, 'a new occupant cannot revive an old beam');
});

test('a human taking a bot seat restores player weapon settings and owns turret shots', () => {
  const env = turretFixture({ SpreadThreshold: 2 });
  const botUnit = bot();
  env.crews.man(botUnit, env.turret);
  const weapon = env.turret.weapon;
  Object.assign(weapon, { spreadScale: 9, sniperScale: 2, yawSpreadOverride: 5, random: () => 0.5 });
  env.crews.leave(botUnit, { placement: false });
  const unit = bot(); unit.human = true;
  const gunner = new Gunner({ camera: new THREE.PerspectiveCamera(), collision: env.collision, ordnance: env.ordnance, combat: { owner: unit, team: 1 }, animator: unit.animator, clips: new Map() });
  gunner.enter(env.turret, unit.soldier);
  same(weapon.floor, -2);
  same(weapon.spreadScale, 1);
  same(weapon.sniperScale, 0);
  same(weapon.yawSpreadOverride, null);
  same(weapon.random, null);
  same(gunner.combat.mounted, true);
  gunner.update(1 / 30, unit.soldier, { take: () => true, held: () => true });
  same(env.fired[0].owner, unit);
  same(env.fired[0].emitter, unit.seat.emitter);
  gunner.exit(unit.soldier);
  same(gunner.combat.mounted, false);
  same(env.fired[0].emitter.alive, false);

  const { a } = fixture();
  const hullWeapon = a.seats[0].weapons[0].state;
  Object.assign(hullWeapon, { spreadScale: 9, sniperScale: 2, yawSpreadOverride: 5, random: () => 0.5 });
  a.setSeatPolicy(0, true);
  same(hullWeapon.spreadScale, 1);
  same(hullWeapon.sniperScale, 0);
  same(hullWeapon.yawSpreadOverride, null);
  same(hullWeapon.random, null);
});

test('human vehicle rounds carry the human unit and physical seat emitter', () => {
  const { a, fired } = fixture();
  const unit = bot(); unit.human = true;
  a.seats[0].lease = { unit, human: true };
  a.muzzle.set(0, 1, 0);
  a.direction.set(0, 0, 1);
  a.emitShot({ weapon: a.seats[0].weapons[0].state }, 0, null);
  same(fired[0].owner, unit);
  same(fired[0].emitter, a.seats[0].emitter);
  same(fired[0].ignoreBody, a.body);
  a.seats[0].lease = null;
  a.emitShot({ weapon: a.seats[0].weapons[0].state }, 0, null);
  same(fired[1].owner, a, 'standalone vehicle fixtures retain the hull fallback');
});

// ---------------------------------------------------------------------------------------------
// Classic vehicle AI (Phase B).

import { ClassicRandom } from '../src/ai/random.js';
import { Character } from '../src/battle.js';
import { createClassicAI } from '../src/ai/classic/index.js';
import { GOAL, SourceGoal } from '../src/ai/classic/controller.js';
import * as vehicleAI from '../src/ai/classic/vehicles/index.js';
import { Reservations, freeCapacity } from '../src/ai/classic/vehicles/reservations.js';
import { VehicleNotifier } from '../src/ai/classic/vehicles/notify.js';
import { vehicleDispatch, _test as patternTest, resetAttackRun, sidestep, enterPattern, tickPattern, exitPattern } from '../src/ai/classic/vehicles/patterns.js';
import { nonlinearYaw, orbitInput, translate, walkerYields } from '../src/ai/classic/vehicles/steering.js';
import { orbitResult, orbitSteer } from '../src/ai/classic/vehicles/orbit.js';
import { FireControl, harpoonReach } from '../src/ai/classic/weapons-ai.js';

// A soldier body the registry, the AI and the seats can all use.
function soldierUnit(units, team, x, z, classDef = { UnitType: 'trooper' }) {
  const u = bot(team, { classDef });
  u.kind = 0;
  u.soldier.position.set(x, 0, z);
  u.soldier.maxSpeed = 7; u.soldier.maxStrafe = 6; u.soldier.maxTurn = 3; u.soldier.maxPitch = 1;
  u.soldier.posture = 'stand';
  Object.defineProperty(u, 'position', { get() { return u.soldier.position; } });
  Object.defineProperty(u, 'velocity', { get() { return u.soldier.velocity; } });
  u.bounds = { center: new THREE.Vector3(), radius: 0.9 };
  Object.defineProperty(u, 'sphere', { get() { u.bounds.center.copy(u.soldier.position); u.bounds.center.y += 0.9; return u.bounds; } });
  u.aimPoint = (out) => out.copy(u.sphere.center);
  u.origin = (out) => out.set(u.position.x, u.position.y + 1.7, u.position.z);
  u.forward = (out) => out.set(Math.sin(u.soldier.aimYaw), 0, Math.cos(u.soldier.aimYaw));
  u.hitSpheres = () => [u.sphere];
  units.add(u);
  return u;
}

function aiWorld({ base = 'hover', hard = false, extra = {} } = {}) {
  const fx = fixture({ base, extra });
  const units = new Units();
  fx.system.attachUnits(units);
  const world = {
    time: 0, random: new ClassicRandom(7), randomB: new ClassicRandom(9), units,
    relation: (a, b) => units.relation(a, b),
    battle: { reinforcements: { 1: 100, 2: 100 }, characters: [] },
    posts: { posts: [] },
    collision: fx.c,
    vehicleSystem: fx.system,
    missionControls: { stayInTurrets: false, stationaryWait: 10, vehicleNotifyRadius: 50, dense: { value: false }, flyHeight: { min: -200, max: 200 } },
  };
  const handle = createClassicAI(world, { modules: { vehicles: vehicleAI }, hard, quiet: true, characters: () => world.battle.characters });
  vehicleAI.adaptClassicAI(handle.ai);
  return { ...fx, units, world, ai: handle.ai, handle, service: vehicleAI.serviceOf(handle.ai) };
}

function enlist(env, unit) {
  const character = new Character(env.world.battle.characters.length, unit.team, false);
  character.unit = unit;
  unit.character = character;
  env.world.battle.characters.push(character);
  env.ai.attach(character);
  env.ai.spawned(character);
  return character;
}

const step = (env, h = 1 / 30) => { env.world.time += h; env.ai.update(h); env.system.update(h); };

test('reservations: three seconds, refresh in place, capacity counts players and reservations only', () => {
  let now = 0;
  const r = new Reservations(() => now);
  const vehicle = { alive: true, generation: 1 };
  const a = { alive: true, generation: 1, team: 1 }, b = { alive: true, generation: 1, team: 1 }, enemy = { alive: true, generation: 1, team: 2 };
  r.reserve(vehicle, a);
  r.reserve(vehicle, a);
  same(r.records.length, 1, 'same owner refreshes');
  r.reserve(vehicle, b);
  r.reserve(vehicle, enemy);
  same(r.count(vehicle, 1), 2);
  same(freeCapacity(r, vehicle, 1, 1, 2, 0), 0);
  same(freeCapacity(r, vehicle, 2, 1, 2, 1), 0, 'a player aboard counts against seats');
  same(freeCapacity(r, vehicle, 2, 1, 3, 1), 1);
  now = 2.999;
  same(r.count(vehicle, 1), 2);
  now = 3;
  same(r.count(vehicle, 1), 0, 'expired at exactly 3 s (strict now < time + 3)');
  r.reserve(vehicle, a);
  a.generation += 1;
  same(r.count(vehicle, 1), 0, 'a stale owner drops its reservation');
});

test('the notifier: 3G buckets, one empty pass, slow and not full, own team, not low on tickets', () => {
  const env = aiWorld();
  const { service, a, b, units, world } = env;
  const friend = soldierUnit(units, 1, 10, 0);
  const hero = soldierUnit(units, 1, 10, 0, { NoEnterVehicles: 1 });
  same(service.canEnter({ unit: hero }, a.aiTarget), false, 'commands cannot bypass the notifier class restriction');
  const foe = soldierUnit(units, 2, 12, 0);
  const notes = [];
  friend.controller = { isUnitController: true, addVehicleNote: (ref) => notes.push(ref.unit) };
  foe.controller = { isUnitController: true, addVehicleNote: () => assert.fail('an enemy hull is not offered') };
  const notifier = new VehicleNotifier();
  const G = service.cyclesPerSecond();
  same(G, 2, 'Easy/Medium: 2 cycles per second');
  const seen = [];
  for (let pass = 0; pass < 3 * G + 1; pass++) {
    notes.length = 0;
    notifier.run(service);
    seen.push(notes.map((e) => e.vehicle));
  }
  same(notifier.counter, 3 * G, 'the counter visits 0..N');
  same(seen.filter((n) => n.includes(a)).length, 1, 'each hull once per N + 1 passes');
  same(seen.filter((n) => n.length === 0).length, 3 * G + 1 - 2, 'the other buckets are empty');
  // A moving hull is skipped; a full one too; low tickets silence it.
  const bucket = seen.findIndex((n) => n.includes(a));
  const runTo = (i) => { notifier.counter = i === 0 ? 3 * G : i - 1; notes.length = 0; notifier.run(service); return notes.slice(); };
  a.velocity.set(4, 0, 0);
  assert.ok(runTo(bucket).length === 0);
  a.velocity.set(0, 0, 0);
  world.battle.reinforcements[1] = 25;
  assert.ok(runTo(bucket).length === 0);
  world.battle.reinforcements[1] = 26;
  same(runTo(bucket).length, 1);
  assert.ok(env.system.board(bot(), a, 0) && env.system.board(bot(), a, 1));
  assert.ok(runTo(bucket).length === 0, 'every seat taken');
});

test('a bot is told of a hull, boards it, and its controller drives the seat; out again on an unboard goal', () => {
  const env = aiWorld();
  const { a, units, world, ai, service } = env;
  const unit = soldierUnit(units, 1, 2.5, 0);
  const character = enlist(env, unit);
  const controller = character.controller;
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  let boarded = false;
  for (let i = 0; i < 400 && !boarded; i++) {
    step(env);
    boarded = Boolean(unit.seat);
  }
  assert.ok(boarded, 'notified, interrupted (priority 6), walked and boarded');
  same(unit.seat.vehicle, a);
  same(unit.seat.index, 0);
  step(env);
  const seat = unit.seat.controllable;
  same(controller.unit, seat, 'the controller now drives the seat');
  same(seat.kind, 6);
  same(controller.policy, vehicleAI.MOBILE_POLICY);
  same(service.boarded(controller), true);
  same(service.currentControllable(character), seat);
  // The seat's controller steers the hull through the vehicle analogs. (A
  // goal starts at the controller's next service: up to a cycle away.)
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  for (let i = 0; i < 40 && controller.commands.latch; i++) step(env);
  assert.ok(!controller.commands.latch, 'the wait goal started');
  controller.request.valid = true;
  controller.request.steeringTarget.set(0, 0, 60);
  controller.request.waypoint.set(0, 0, 60);
  const start = a.position.clone();
  for (let i = 0; i < 30; i++) {
    ai.scheduler.executeInput(controller, 1 / 30);
    env.system.update(1 / 30);
    world.time += 1 / 30;
  }
  assert.ok(a.position.z - start.z > 3, `hull driven by the seat's intent: ${a.position.toArray()}`);
  // An unboard goal gets it out; its controller is the soldier's again.
  assert.ok(controller.commands.prependOne(SourceGoal.of(GOAL.UNBOARD)), 'the unboard goal is queued');
  for (let i = 0; i < 60 && unit.seat; i++) step(env);
  same(unit.seat, null, 'out of the seat');
  step(env);
  same(controller.unit, unit);
  same(controller.policy, null);
});

test('a bot at a turret leaves after the stationary wait with nothing to shoot', () => {
  const env = aiWorld();
  const { units, world, ai } = env;
  const rig = { root: new THREE.Group(), byName: {}, bind: new Map() };
  for (const name of ['drum', 'gun', 'seat', 'hp_fire']) rig.byName[name] = new THREE.Object3D();
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.gun.add(rig.byName.hp_fire); rig.byName.drum.add(rig.byName.seat);
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  const desc = { base: 'armedbuilding', class: 't', matrix: identity().makeTranslation(-30, 0, 0).toArray(), settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', MaxTurnSpeed: 2, MaxHealth: 300, YawLimits: '-180 180', PitchLimits: '-45 45' }, weapon: { class: 'g', base: 'cannon', ShotDelay: 0.25, RoundsPerClip: 0, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 2 } } };
  const turret = new Turret(desc, { built: rig }, { built: null }, null);
  const crews = new TurretSeats({ live: { turrets: [turret] }, ordnance: { fire() {} }, collision: env.c, units });
  world.turretSeats = crews;
  const unit = soldierUnit(units, 1, -30, 2);
  const character = enlist(env, unit);
  assert.ok(crews.man(unit, turret));
  const controller = character.controller;
  for (let i = 0; i < 3; i++) { world.time += 1 / 30; ai.update(1 / 30); crews.update(1 / 30); }
  same(controller.unit, unit.seat.controllable);
  same(controller.unit.kind, 7);
  same(controller.policy, vehicleAI.STATIONARY_POLICY);
  same(controller.homeFacing, true, 'a building control looks around +-30 degrees');
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  let left = -1;
  for (let i = 0; i < 30 * 14; i++) {
    world.time += 1 / 30; ai.update(1 / 30); crews.update(1 / 30);
    if (!unit.seat) { left = world.time; break; }
  }
  assert.ok(left > 10 && left < 12, `left after the 10 s stationary wait: ${left}`);
});

test('low reinforcements make a seated bot get out', () => {
  const env = aiWorld();
  const { a, units, world } = env;
  const unit = soldierUnit(units, 1, 0, 6);
  const character = enlist(env, unit);
  assert.ok(env.system.board(unit, a, 0));
  for (let i = 0; i < 3; i++) step(env);
  same(character.controller.unit, unit.seat.controllable);
  world.battle.reinforcements[1] = 25;
  for (let i = 0; i < 40 && unit.seat; i++) step(env);
  same(unit.seat, null);
});

test('vehicle dispatch: hover passes, walker and flyer rows, the AT-AT orbit', () => {
  const env = aiWorld();
  const { a, b, units, world } = env;
  const pilot = bot(); assert.ok(env.system.board(pilot, a, 0));
  const seat = pilot.seat.controllable;
  const weapon = seat.combat.arsenal.weapons[0];
  weapon.def.MinRange = 0; weapon.def.OptimalRange = 30; weapon.def.MaxRange = 200;
  const enemyHull = b.aiTarget;
  b.team = 2;
  const soldier = soldierUnit(units, 2, 5, 40);
  const controller = { unit: seat, rangeScale: 1, perception: null };
  const out = {};
  // ForwardSpeed 20 >= 14, StrafeSpeed 4 < 5, open terrain: pattern 7 at a hull.
  vehicleDispatch(controller, seat, enemyHull, false, world, out, { dense: false });
  same(out.pattern, 7);
  vehicleDispatch(controller, seat, enemyHull, false, world, out, { dense: true });
  assert.ok(out.pattern === 2 || out.pattern === 0, 'dense: stand and shoot in band');
  vehicleDispatch(controller, seat, soldier, false, world, out, { dense: false });
  same(out.pattern, 0, 'a hover fights infantry only opportunistically');
  // A flyer against an AT-AT with two aboard: 15; otherwise the attack run.
  const fly = aiWorld({ base: 'flyer' });
  const flyPilot = bot(); assert.ok(fly.system.board(flyPilot, fly.a, 0));
  const flyer = flyPilot.seat.controllable;
  same(flyer.kind, 5);
  const atat = { kind: 2, team: 2, alive: true, position: new THREE.Vector3(0, 0, 50), aimPoint: (o) => o.set(0, 5, 50), sphere: { center: new THREE.Vector3(0, 5, 50), radius: 8 }, classDef: {}, vehicle: { name: 'imp_walk_atat', def: { label: 'AT-AT' } } };
  vehicleDispatch({ unit: flyer }, flyer, atat, false, fly.world, out);
  same(out.pattern, out.aux ? 13 : 0, 'one aboard: the attack run when a weapon can hit it');
  assert.ok(fly.system.board(bot(), fly.a, 1));
  vehicleDispatch({ unit: flyer }, flyer, atat, false, fly.world, out);
  same(out.pattern, 15, 'two aboard against the AT-AT: the tow orbit');
});

// Pattern 15's run (src/ai/classic/vehicles/orbit.js), against a walker at
// the origin facing +z: a craft that turns at most 1.5 rad/s toward the
// orbit's point, at 23 m/s with the throttle closed and up to 72 m/s open.
const walkerAt = (x = 0, y = 0, z = 0) => ({ alive: true, position: new THREE.Vector3(x, y, z), matrix: new THREE.Matrix4().makeTranslation(x, y, z) });
const runOrbit = () => ({ offset: new THREE.Vector3(12.5, 0, 25), radius: 25, factor: 2.5 });
function flyOrbit(hull, orbit, start, yaw, seconds, each) {
  const P = start.clone();
  const out = orbitResult();
  const fwd = new THREE.Vector3();
  for (let t = 0; t < seconds; t += 1 / 30) {
    fwd.set(Math.sin(yaw), 0, Math.cos(yaw));
    orbitSteer(P, fwd, hull, orbit, out);
    const want = Math.atan2(out.point.x - P.x, out.point.z - P.z);
    yaw += THREE.MathUtils.clamp(Math.atan2(Math.sin(want - yaw), Math.cos(want - yaw)), -1.5 / 30, 1.5 / 30);
    const speed = 23 + (72 - 23) * out.throttle;
    P.x += (Math.sin(yaw) * speed) / 30;
    P.z += (Math.cos(yaw) * speed) / 30;
    P.y = out.point.y;
    each?.(t, P, yaw, out);
  }
  return P;
}

test('pattern 15 orbit: the ellipse 7 m over the walker, its height share and throttle, and the tow pass', () => {
  const hull = walkerAt(0, 3, 0);
  const out = orbitResult();
  // Far off to the side: the run's centre is 25 m ahead of the walker, 12.5 m
  // aside and 7 m up; the climb taken in full; the throttle (300 - 12.5) /
  // 62.5 - 1 = 3.6 -> (3.6 - 1) / 5.
  orbitSteer(new THREE.Vector3(300, 40, 25), new THREE.Vector3(-1, 0, 0), hull, runOrbit(), out);
  assert.ok(out.centre.distanceTo(new THREE.Vector3(12.5, 10, 25)) < 1e-9, `centre ${out.centre.toArray()}`);
  same(out.point.y, 10, 'at the centre\'s height');
  same(out.blend, 1);
  assert.ok(Math.abs(out.throttle - 0.52) < 1e-9, `throttle ${out.throttle}`);
  // Coming in, the point is ahead of the tangent point on the near edge.
  assert.ok(out.point.x < 12.5 && out.point.z < 25, `the near side: ${out.point.toArray()}`);
  // On the ellipse, beside the walker: no climb, the throttle shut.
  orbitSteer(new THREE.Vector3(-12.5, 10, 20), new THREE.Vector3(0, 0, 1), hull, runOrbit(), out);
  same(out.blend, 0); same(out.throttle, 0);
  // A negative radius: t is below zero everywhere, so slow and low all the way in.
  orbitSteer(new THREE.Vector3(300, 40, 25), new THREE.Vector3(-1, 0, 0), hull, { ...runOrbit(), radius: -25 }, out);
  same(out.blend, 0); same(out.throttle, 0);
  // The tow pass (offset +x, radius +): flying the walker's way, past the centre.
  orbitSteer(new THREE.Vector3(-12.5, 10, 40), new THREE.Vector3(0, 0, 1), hull, runOrbit(), out);
  same(out.towPass, true, 'past the centre, the walker\'s way');
  orbitSteer(new THREE.Vector3(-12.5, 10, 10), new THREE.Vector3(0, 0, 1), hull, runOrbit(), out);
  same(out.towPass, false, 'short of the centre');
  orbitSteer(new THREE.Vector3(-12.5, 10, 40), new THREE.Vector3(0, 0, -1), hull, runOrbit(), out);
  same(out.towPass, false, 'against the walker\'s way');
});

test('pattern 15 orbit: the run passes the legs with the walker in the rear gun\'s arc on the tow pass; once the cable holds the circle stays inside the cable and winds three circuits', () => {
  const hull = walkerAt();
  // The run, from 300 m off: it closes, then passes along the walker's flank.
  let nearest = Infinity, passes = 0, behind = 0;
  flyOrbit(hull, runOrbit(), new THREE.Vector3(300, 0, -100), -Math.PI / 2, 60, (t, P, yaw, out) => {
    if (t < 20) return;
    const d = Math.hypot(P.x, P.z);
    nearest = Math.min(nearest, d);
    if (!out.towPass) return;
    passes++;
    // The walker's bearing from the craft's heading: aft (the rear gun's
    // YawLimits 80 280) and within the harpoon's 70 m.
    const rel = Math.atan2(Math.sin(Math.atan2(-P.x, -P.z) - yaw), Math.cos(Math.atan2(-P.x, -P.z) - yaw));
    if (Math.abs(rel) > (80 * Math.PI) / 180 && d < 70) behind++;
  });
  assert.ok(nearest < 20, `the run passes within 20 m of the walker: ${nearest.toFixed(1)}`);
  assert.ok(passes > 0 && behind / passes > 0.5, `on the tow pass the walker is mostly aft within reach: ${behind}/${passes}`);
  // Towing: the 25 m circle 8.75 m ahead of the walker, factor 1.
  const towed = { offset: new THREE.Vector3(0, 0, 8.75), radius: 25, factor: 1 };
  let wound = 0, last = null, farthest = 0;
  flyOrbit(hull, towed, new THREE.Vector3(-12.5, 0, 40), 0, 40, (t, P) => {
    const a = Math.atan2(P.x, P.z);
    if (last !== null) wound += Math.atan2(Math.sin(a - last), Math.cos(a - last));
    last = a;
    if (t > 5) farthest = Math.max(farthest, Math.hypot(P.x, P.z));
  });
  assert.ok(Math.abs(wound) > 6 * Math.PI, `three circuits in 40 s: ${(Math.abs(wound) / (2 * Math.PI)).toFixed(2)}`);
  assert.ok(farthest < 60, `inside 1.2 x CableLength (72 m) from the walker: ${farthest.toFixed(1)}`);
});

test('pattern 15 orbit: the pilot\'s input steers by it and, on the tow pass, every bot seat with a tow cable selects it (a person\'s seat keeps his own)', () => {
  const towGun = { def: { base: 'towcableweapon' }, channel: 0 }, cannonGun = { def: { base: 'cannon' }, channel: 1 };
  const gunner = () => {
    const controller = { perception: { fire: new FireControl() } };
    controller.perception.fire.channel = 1;
    const control = { controller, combat: { arsenal: { weapons: [towGun, cannonGun], channels: [[towGun], [cannonGun]], current(c) { return this.channels[c ?? 0]?.[0] ?? null; }, select() { return true; } } } };
    return { control, controller };
  };
  const bot = gunner(), person = gunner();
  const hull = walkerAt();
  const target = { generation: 3, vehicle: hull };
  const craft = { position: new THREE.Vector3(-12.5, 10, 40), quaternion: new THREE.Quaternion(), seats: [{ lease: null }, { lease: { human: false, unit: {}, controllable: bot.control } }, { lease: { human: true, unit: {}, controllable: person.control } }] };
  const pilot = { vehicle: craft, steer: { orbit: { target: { unit: target, generation: 3 }, ...runOrbit() } } };
  const request = orbitInput(pilot);
  same(request.mode, 3);
  const out = orbitSteer(craft.position, new THREE.Vector3(0, 0, 1), hull, pilot.steer.orbit, orbitResult());
  assert.ok(request.steeringTarget.distanceTo(out.point) < 1e-9, 'toward the orbit\'s point');
  same(request.blend, out.blend); same(request.throttle, out.throttle);
  same(bot.controller.perception.fire.channel, 0, 'the bot gunner takes the tow cable');
  same(person.controller.perception.fire.channel, 1, 'a person keeps his weapon');
  // Short of the centre: no pass, no switch.
  bot.controller.perception.fire.channel = 1;
  craft.position.set(-12.5, 10, 10);
  orbitInput(pilot);
  same(bot.controller.perception.fire.channel, 1);
  // A dead walker, or another life of it, steers nothing.
  target.generation = 4;
  same(orbitInput(pilot), null);
});

test('harpoon reach: a bot fires the tow cable only inside the harpoon\'s cast (LockOnRange, at least 20 m)', () => {
  same(harpoonReach({ base: 'towcableweapon', LockOnRange: 70, ordnance: { base: 'towcable' } }), 70);
  same(harpoonReach({ base: 'towcableweapon', ordnance: { base: 'towcable' } }), 20);
  same(harpoonReach({ base: 'cannon', LockOnRange: 100, ordnance: { base: 'bolt' } }), Infinity);
});

test('pattern geometry: the hover pass point and the once-set attack run', () => {
  const env = aiWorld();
  const pilot = bot(); assert.ok(env.system.board(pilot, env.a, 0));
  const seat = pilot.seat.controllable;
  const target = { sphere: { radius: 3 } };
  const patterns = { agent: { tracker: { point: new THREE.Vector3(0, 0, 50), freshTarget: target } }, controller: { unit: seat }, world: env.world };
  const st = patternTest.stateOf(patterns);
  st.toggle = false;
  const away = patternTest.hoverPassPoint(patterns, seat, st, new THREE.Vector3());
  assert.ok(Math.abs(away.z - 3 * 20) < 1e-6, 'away leg: 3 x ForwardSpeed ahead');
  st.toggle = true;
  env.a.yaw = 0.3; // heading not straight at the target
  const pass = patternTest.hoverPassPoint(patterns, seat, st, new THREE.Vector3());
  const r = seat.sphere.radius + 3 + 2;
  assert.ok(pass.distanceTo(new THREE.Vector3(0, 0, 50)) > r - 1e-6, 'beside the target, at least r out');
  resetAttackRun();
  const fly = aiWorld({ base: 'flyer' });
  const p = bot(); assert.ok(fly.system.board(p, fly.a, 0));
  const flyer = p.seat.controllable;
  const attack = { agent: { tracker: { point: new THREE.Vector3(0, 20, 300) } }, controller: { unit: flyer }, world: fly.world };
  const path = patternTest.attackRunPath(attack, flyer, 200);
  const A = Math.max(160, 4 * fly.a.flightConfig.maxSpeed), B = Math.max(40, fly.a.flightConfig.maxSpeed);
  assert.deepEqual(path[1].map(Math.round), [0, 110, Math.round(300 - A)]);
  assert.deepEqual(path[2].map(Math.round), [0, 20, 300 - B]);
  assert.deepEqual(path[3].map(Math.round), [0, 50, 300 + B]);
  same(patternTest.run.A, A, 'set once for the process');
  const capped = patternTest.attackRunPath(attack, flyer, 60);
  same(capped[1][1], 60, 'the attack points stay under the maximum fly height');
});

test('steering primitives: nonlinear yaw, the shared translation, walker right of way', () => {
  const unit = { kind: 6, position: new THREE.Vector3(), bodyYaw: 0, classDef: { TurnRate: 1.7, ForwardSpeed: 20, StrafeSpeed: 4 } };
  const right = new THREE.Vector3(-10, 0, 10); // ahead and to the right (port right is -x)
  const yaw = nonlinearYaw(unit, right);
  const g = 4.5 - 1.7, q = Math.SQRT1_2, t = Math.min(g * q, 1);
  assert.ok(Math.abs(yaw - t * t) < 1e-9 && yaw > 0, 'positive: turn right');
  same(nonlinearYaw(unit, new THREE.Vector3(-1, 0, -10), false), 1, 'behind: full turn its way');
  const out = {};
  translate(unit, { valid: true, steeringTarget: new THREE.Vector3(0, 0, 100) }, 20, 4, true, out);
  assert.ok(Math.abs(out.forward - 1) < 1e-9 && Math.abs(out.right) < 1e-9);
  translate(unit, { valid: true, steeringTarget: new THREE.Vector3(0, 0, 5) }, 20, 4, false, out);
  assert.ok(out.near && Math.abs(out.forward - 0.5) < 1e-9, 'inside half ForwardSpeed with no path: slowed');
  const big = { kind: 2, position: new THREE.Vector3(), velocity: new THREE.Vector3(1, 0, 0), classDef: { bounds: [[-5, 0, -5], [5, 10, 5]] } };
  const small = { kind: 6, position: new THREE.Vector3(10, 0, 0), velocity: new THREE.Vector3(-1, 0, 0), classDef: { bounds: [[-1, 0, -1], [1, 1, 1]] } };
  same(walkerYields(big, small), false, 'the far larger walker keeps going');
  same(walkerYields({ ...big, classDef: small.classDef }, { ...small, classDef: big.classDef }), true);

});

test('mounted reach is angular only, inclusive, in the body frame', () => {
  const env = aiWorld();
  const { service } = env;
  const pivot = new THREE.Vector3();
  const lease = (yaw, pitch) => ({ turret: { facing: 0, desc: { settings: { YawLimits: `${yaw[0] * 180 / Math.PI} ${yaw[1] * 180 / Math.PI}`, PitchLimits: `${pitch[0] * 180 / Math.PI} ${pitch[1] * 180 / Math.PI}` } } }, index: 0, origin: (o) => o.copy(pivot) });
  const l = lease([-0.5, 0.5], [-0.25, 0.25]);
  const reach = (p) => service.mountReach({ lease: l, seat: l }, p);
  // The executed native fixture's cases (identity transforms).
  same(reach(new THREE.Vector3(0, 0, 10)), true);
  same(reach(new THREE.Vector3(1, 1, 10)), true);
  same(reach(new THREE.Vector3(10, 0, 0)), false);
  same(reach(new THREE.Vector3(-10, 0, 0)), false);
  same(reach(new THREE.Vector3(0, 10, 10)), false);
  same(reach(new THREE.Vector3(0, -10, 10)), false);
  const zero = lease([0, 0], [0, 0]);
  same(service.mountReach({ lease: zero, seat: zero }, new THREE.Vector3(0, 0, 10)), true, 'zero width: straight ahead');
  same(service.mountReach({ lease: zero, seat: zero }, new THREE.Vector3(0.01, 0, 10)), false);
});

test('sidestep: a slow strafing hull, about once in 13 s, one to two radii aside', () => {
  const env = aiWorld();
  const pilot = bot(); assert.ok(env.system.board(pilot, env.a, 0));
  const seat = pilot.seat.controllable;
  seat.classDef = { ...seat.classDef, ForwardSpeed: 12, StrafeSpeed: 4 };
  const goals = [];
  const patterns = { controller: { unit: seat }, random: new ClassicRandom(3), agent: { helper: { goto: (p) => goals.push(p.clone()) } } };
  let n = 0;
  for (let i = 0; i < 26000; i++) n += sidestep(patterns, 2) ? 1 : 0;
  assert.ok(n > 600 && n < 1400, `about 1 in 26 at 2 cycles per second: ${n}`);
  const r = seat.sphere.radius;
  for (const g of goals) {
    const d = Math.hypot(g.x - seat.position.x, g.z - seat.position.z);
    assert.ok(d >= r - 1e-6 && d <= 2 * r + 1e-6);
  }
});

test('the flyer pilot: waits on the pad, flies to a post, finds a landing hint, lands, and leaves', () => {
  const env = aiWorld({ base: 'flyer' });
  const { a, units, world, ai } = env;
  const post = { name: 'cp1', team: 0, position: new THREE.Vector3(0, 0, 300) };
  world.posts.posts.push(post);
  const hint = { kind: 'land', bit: 32, position: new THREE.Vector3(0, 0, 40), forward: new THREE.Vector3(0, 0, 1), up: new THREE.Vector3(0, 1, 0), commandPost: post, reservedAt: -100 };
  const queries = [];
  // Nav's hint query, the only Nav service this test needs.
  ai.nav = { ...(ai.nav || {}), queryHints: (q) => { queries.push(q); return [hint]; } };
  const unit = soldierUnit(units, 1, 4, 0);
  const character = enlist(env, unit);
  assert.ok(env.system.board(unit, a, 0));
  character.objective = post;
  for (let i = 0; i < 5; i++) step(env);
  const controller = character.controller;
  same(controller.policy, vehicleAI.FLY_POLICY);
  // The objective goal maps to the flyer agent's state 7. (The crew-wait
  // interrupt, which a flyer pilot accepts, may come first and replace the
  // agent; it saves the objective and resumes it.)
  const agent = () => (controller.state === 7 ? controller.agent : null);
  for (let i = 0; i < 40 && !agent(); i++) step(env);
  same(agent()?.constructor.name, 'UnitFlyAgent', 'objective goal -> UnitFlyAgent');
  same(agent().flag, true, 'state 1 sets the alternating flag');
  // On the ground at match start: wait (state 3) until the literal time 4, then travel (4).
  const fly = (s) => agent()?.state === s;
  for (let i = 0; i < 30 * 12 && !(fly(4) || fly(5)); i++) step(env);
  assert.ok(fly(4) || fly(5), `on to the post: ${controller.state}/${agent()?.state}`);
  // A hint 80 m around, at most every 3 s, below the flyer: the approach.
  for (let i = 0; i < 30 * 8 && !fly(5); i++) step(env);
  assert.ok(fly(5), 'landing hint found');
  assert.ok(queries.every((q) => q.mask === 32 && q.halfExtent === 80 && q.cap === 1));
  assert.ok(!unit.seat.controllable.steer?.land, 'without Nav (no flyer helper) no landing request is made');
  // Grounded: landed (6). A fighter closes the hint for 20 s, its pilot gets out, no reboarding for 15 s.
  // (Without Nav it never flies the approach: set it down by hand.)
  a.tick = () => { a.grounded = true; a.flightState = 'Landed'; };
  a.grounded = true;
  const before = world.time;
  for (let i = 0; i < 40 && unit.seat; i++) step(env);
  same(unit.seat, null, 'the pilot left');
  assert.ok(Math.abs(hint.reservedAt + 4 - (before + 20)) < 2, 'hint closed for 20 s');
  assert.ok(controller.reboardAt > before + 14);
});

test('patrol in a seat: a slow hull with an enemy near keeps its seat; otherwise out, no reboarding for 5 s', () => {
  const env = aiWorld();
  const { a, units, service, world } = env;
  const unit = soldierUnit(units, 1, 0, 5);
  enlist(env, unit);
  const gunner = soldierUnit(units, 1, 2, 5);
  const gunnerCharacter = enlist(env, gunner);
  assert.ok(env.system.board(gunner, a, 1));
  for (let i = 0; i < 3; i++) step(env);
  const controller = gunnerCharacter.controller;
  same(controller.unit.kind, 7);
  let done = 0;
  let asked = 0;
  controller.requestObjective = () => { asked += 1; };
  const patrol = { agent: { controller }, requestState: () => { done += 1; } };
  const enemy = soldierUnit(units, 2, 10, 5);
  enemy.character = { team: 2 };
  same(service.patrolMounted(patrol), true, 'an enemy with a Character within 30 m: stay');
  same(asked, 1);
  same(done, 1);
  enemy.alive = false;
  same(service.patrolMounted(patrol), false);
  same(controller.exitRequest, true, 'nobody near: out');
  assert.ok(Math.abs(controller.reboardAt - (world.time + 5)) < 1e-9);
});

test('patrol in a seat at a support point (port): stands three times while the post is not held, then another post; never out', () => {
  const env = aiWorld();
  const { a, units, service } = env;
  const pilot = soldierUnit(units, 1, 0, 5);
  const character = enlist(env, pilot);
  assert.ok(env.system.board(pilot, a, 0));
  for (let i = 0; i < 3; i++) step(env);
  const controller = character.controller;
  let asked = 0;
  controller.requestObjective = () => { asked += 1; };
  const states = [];
  let stopped = 0;
  const post = { team: 2 };
  const patrol = { post, expires: 0, agent: { controller, support: true }, helper: { stop: () => { stopped += 1; } }, requestState: (s) => states.push(s) };
  for (let i = 0; i < 3; i++) same(service.patrolMounted(patrol), true);
  assert.deepEqual(states, [6, 6, 6], 'stands');
  same(stopped, 3);
  assert.ok(patrol.expires >= env.world.time + 20 && patrol.expires <= env.world.time + 30);
  same(asked, 0);
  same(service.patrolMounted(patrol), true, 'a fourth time: on to another post');
  same(asked, 1);
  patrol.agent.supportHolds = 0;
  post.team = 1;
  same(service.patrolMounted(patrol), true, 'its team holds the post: on, aboard');
  same(asked, 2);
  assert.ok(!controller.exitRequest, 'never out');
});

test('unattended decay: outside the boundary, one sample a second after 20, a near player pauses it', () => {
  const env = aiWorld();
  const { a, service, world } = env;
  world.nav = { outside: () => true };
  const max = a.maxHealth;
  for (let i = 0; i < 20 * 30; i++) service.updateDecay(1 / 30);
  same(a.health, max, 'twenty idle samples cost nothing');
  for (let i = 0; i < 2 * 30; i++) service.updateDecay(1 / 30);
  assert.ok(a.health < max && Math.abs(a.health - (max - 2 * max / 10)) < 1e-6, `MaxHealth / DecayTime a sample past 20: ${a.health}`);
  const pilot = bot();
  assert.ok(env.system.board(pilot, a, 0));
  service.updateDecay(1);
  same(a.decay.idle, 0, 'anyone aboard clears it');
  env.system.unboard(pilot, { placement: false });
  world.nav = { outside: () => false };
  for (let i = 0; i < 40; i++) service.updateDecay(1);
  same(a.decay.idle, 0, 'inside the map, still on its team: no decay');
});

test('boarding acceptance: moving hulls always; turrets need a reachable memory or StayInTurrets', () => {
  const env = aiWorld();
  const { a, service, world, ai } = env;
  const agent = { controller: { unit: { classDef: { UnitType: 'trooper' } } } };
  same(service.boardPolicy(agent, a.aiTarget), true, 'ForwardSpeed > 0');
  const rig = { root: new THREE.Group(), byName: {}, bind: new Map() };
  for (const name of ['drum', 'gun', 'seat']) rig.byName[name] = new THREE.Object3D();
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.drum.add(rig.byName.seat);
  rig.byName.gun.position.y = 1.5;
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  const turret = new Turret({ base: 'armedbuilding', matrix: identity().toArray(), settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', MaxHealth: 300, YawLimits: '-180 180', PitchLimits: '-45 45' }, weapon: null }, { built: rig }, { built: null }, null);
  const crews = new TurretSeats({ live: { turrets: [turret] }, collision: env.c, units: env.units });
  world.turretSeats = crews;
  let memory = null;
  ai.combat = { memoryPoint: () => memory };
  same(service.boardPolicy(agent, turret.aiTarget), false, 'no memory, no StayInTurrets');
  memory = new THREE.Vector3(0, 1.5, 30);
  same(service.boardPolicy(agent, turret.aiTarget), true, 'a remembered point the gun can bear on');
  memory = new THREE.Vector3(0, 60, 1);
  same(service.boardPolicy(agent, turret.aiTarget), false, 'above its pitch limits');
  world.missionControls.stayInTurrets = true;
  same(service.boardPolicy(agent, turret.aiTarget), true, 'StayInTurrets');
  same(service.boardPolicy({ controller: { unit: { classDef: { UnitType: 'pilot' } } } }, turret.aiTarget), false, 'not for the repair roles');
});

test('hover pattern 7: legs alternate on arrival or crossing; ten seconds without progress falls back to a path', () => {
  const env = aiWorld();
  const pilot = bot();
  assert.ok(env.system.board(pilot, env.a, 0));
  const seat = pilot.seat.controllable;
  const gotos = [];
  const helper = { arrived: false, goto: (p, o) => gotos.push({ p: p.clone(), mode: o?.mode ?? 'route' }) };
  let delay = null;
  const patterns = { agent: { helper, tracker: { point: new THREE.Vector3(0, 0, 80), freshTarget: { sphere: { radius: 2 } }, setDelay: (s) => { delay = s; } } }, controller: { unit: seat }, world: env.world, face() {} };
  enterPattern(patterns, 7, {});
  same(delay, 12, 'target kept for 12 s');
  same(gotos.length, 1);
  same(gotos[0].mode, 'direct');
  const st = patternTest.stateOf(patterns);
  same(st.toggle, true, 'the first leg goes past the target');
  helper.arrived = true;
  tickPattern(patterns, 7, 1 / 30, {});
  same(st.toggle, false, 'arrived: the away leg');
  helper.arrived = false;
  const legs = gotos.length;
  env.world.time += 11;
  tickPattern(patterns, 7, 1 / 30, {});
  same(gotos.length, legs + 1);
  same(gotos[gotos.length - 1].mode, 'route', 'no progress for 10 s: an ordinary path to the target');
  same(st.fallback, true);
  assert.ok(seat.steer.movingTurnAt === env.world.time, 'every tick asks to keep moving while turning');
});

test('pattern 15: the orbit is on the tracker\'s target even when the scanner does not see it this cycle; once the cable holds it closes in; the exit clears it', () => {
  const fly = aiWorld({ base: 'flyer' });
  const pilot = bot();
  assert.ok(fly.system.board(pilot, fly.a, 0));
  const seat = pilot.seat.controllable;
  const atat = { generation: 2, alive: true, vehicle: { alive: true } };
  let delay = null;
  // Circling it, the walker is off the nose: the tracker holds it, not fresh.
  const tracker = { target: atat, freshTarget: null, setDelay: (s) => { delay = s; }, defaultDelay: () => { delay = 'default'; } };
  const patterns = { agent: { helper: { stop() {} }, tracker }, controller: { unit: seat }, world: fly.world, random: new ClassicRandom(3), face() {} };
  enterPattern(patterns, 15, {});
  same(delay, 10, 'target kept for 10 s');
  same(seat.steer.orbit.target.unit, atat, 'the tracker\'s target, fresh or not');
  same(seat.steer.orbit.target.generation, 2);
  same(Math.abs(seat.steer.orbit.offset.x), 12.5); same(seat.steer.orbit.offset.z, 25);
  same(Math.abs(seat.steer.orbit.radius), 25); same(seat.steer.orbit.factor, 2.5);
  fly.a.towTarget = 1;
  tickPattern(patterns, 15, 1 / 30, {});
  same(seat.steer.orbit.offset.x, 0); same(seat.steer.orbit.offset.z, 8.75);
  same(seat.steer.orbit.factor, 1, 'the cable holds: the 25 m circle');
  exitPattern(patterns, 15);
  same(seat.steer.orbit, null);
  same(delay, 'default');
});

test('the flyer input: aims its nose at the steering point and keeps at least its minimum throttle', () => {
  const env = aiWorld({ base: 'flyer' });
  const pilot = bot();
  assert.ok(env.system.board(pilot, env.a, 0));
  const seat = pilot.seat.controllable;
  env.a.grounded = false;
  env.a.position.y = 50;
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  const request = { valid: true, steeringTarget: new THREE.Vector3(-100, 50, 100), waypoint: new THREE.Vector3(-100, 50, 100), segmentDirection: new THREE.Vector3(-1, 0, 1).normalize(), mode: 0, lower: -1, upper: 1 };
  env.service.analogs(seat, request, null, 1 / 30, out, { controller: { navControl: null, executorStatus: 0 } });
  assert.ok(out.turn > 0, 'the point is to its right: turn right');
  assert.ok(out.forward >= -1 && out.forward <= 1, `bounded virtual throttle: ${out.forward}`);
  same(seat.intent.down, 0);
  // Asked to land with no path: down.
  seat.steer.land = true; env.a.flightState='Flying';env.a.landingReason='Press SPACE to land';
  env.service.analogs(seat, { valid: false }, null, 1 / 30, out, { controller: { navControl: null, executorStatus: 0 } });
  same(seat.intent.down, 1);
});

test('the flyer helper follows an external path through Nav\'s executor, and the seat flies it', async () => {
  const { ClassicNav } = await import('../src/ai/classic/nav/index.js');
  // A snowspeeder's speeds and lift.
  const env = aiWorld({ base: 'flyer', extra: { MinSpeed: '20.0', MidSpeed: '40.0', MaxSpeed: '72.0', Acceleraton: '30.0', TakeoffHeight: '2.5', TakeoffSpeed: '3.0', TakeoffTime: '1.0' } });
  const { a, units, world, ai } = env;
  world.nav = ClassicNav.empty('test');
  const unit = soldierUnit(units, 1, 4, 0);
  const character = enlist(env, unit);
  assert.ok(env.system.board(unit, a, 0));
  for (let i = 0; i < 3; i++) step(env);
  const controller = character.controller;
  const helper = env.service.createHelper({ controller });
  same(helper?.constructor.name, 'FlyGotoHelper');
  const P = a.position;
  assert.ok(helper.followPath([[P.x, P.y, P.z], [P.x, P.y + 30, P.z + 150], [P.x, P.y + 30, P.z + 300]], true, true));
  same(unit.seat.controllable.steer.pathOverride, true, 'the path flags reach the flight steering');
  const start = P.clone();
  for (let i = 0; i < 120; i++) {
    ai.scheduler.executeInput(controller, 1 / 30);
    env.system.update(1 / 30);
    world.time += 1 / 30;
  }
  assert.ok(a.position.z - start.z > 20, `it flew the path: ${a.position.toArray().map((v) => v.toFixed(1))}`);
  assert.ok(a.position.y > start.y + 2, 'and climbed');
});

test('with every Classic module loaded: board, crew wait, then drive a Nav route from the seat', async () => {
  const { ClassicNav } = await import('../src/ai/classic/nav/index.js');
  const fx = fixture();
  const units = new Units();
  fx.system.attachUnits(units);
  const world = {
    time: 0, random: new ClassicRandom(7), randomB: new ClassicRandom(9), units,
    relation: (a, b) => units.relation(a, b),
    battle: { reinforcements: { 1: 100, 2: 100 }, characters: [] },
    posts: { posts: [], owned: () => [] },
    collision: fx.c, vehicleSystem: fx.system, nav: ClassicNav.empty('test'), los: () => true,
    missionControls: { stayInTurrets: false, stationaryWait: 10, vehicleNotifyRadius: 50, dense: { value: false }, flyHeight: { min: -200, max: 200 } },
  };
  const handle = createClassicAI(world, { quiet: true, characters: () => world.battle.characters });
  await handle.ready;
  vehicleAI.adaptClassicAI(handle.ai);
  const env = { ...fx, units, world, ai: handle.ai };
  const unit = soldierUnit(units, 1, 2.5, 0);
  unit.combat.arsenal = { weapons: [], current: () => null, channels: [[], []], selected: [0, 0] };
  unit.combat.maxHealth = 300;
  const character = enlist(env, unit);
  const controller = character.controller;
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  for (let i = 0; i < 400 && !unit.seat; i++) step(env);
  assert.ok(unit.seat, 'boarded');
  for (let i = 0; i < 30 * 6; i++) step(env); // the pilot's four-second crew wait
  same(controller.unit, unit.seat.controllable);
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  for (let i = 0; i < 40 && controller.commands.latch; i++) step(env);
  same(controller.agent?.helper?.constructor.name, 'GroundGotoHelper');
  const start = fx.a.position.clone();
  controller.agent.helper.goto(new THREE.Vector3(0, 0, 80));
  for (let i = 0; i < 90; i++) step(env);
  assert.ok(fx.a.position.distanceTo(start) > 5, `the hull followed Nav's route: ${fx.a.position.toArray().map((v) => v.toFixed(1))}`);
});
