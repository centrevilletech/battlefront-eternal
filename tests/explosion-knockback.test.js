import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Soldier, ENGINE } from '../src/soldier.js';
import { PlayerCombat } from '../src/combat.js';
import { CollisionWorld } from '../src/collision.js';
import { falloff } from '../src/ordnance.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { captureEntitySoldier, ENTITY_SOLDIER_STATES } from '../src/runtime/network-entity-capture.js';

// An explosion's area damage and push on a soldier (src/combat.js explode),
// the thrown flight through the soldier's own physics, and what the
// multiplayer paths carry of it. The blast's numbers are a thermal
// detonator's: full damage and push out to 4.5 m, so anyone it pushes it
// also kills; the thrown flight must survive that death.
const GRENADE = { Damage: 320, DamageRadiusInner: 4.5, DamageRadiusOuter: 6.5, Push: 10, PushRadiusInner: 4.5, PushRadiusOuter: 4.5 };
const SHOVE = { Damage: 0, Push: 10, PushRadiusInner: 4.5, PushRadiusOuter: 4.5 };
const DT = 1 / 60;
const STILL = { forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false };
const HALF = 1.4626 / 2; // half the standing soldier's blast sphere

// A flat floor 200 m across, and with `wall` an upright wall across x = wall.
function world(wall = null) {
  const positions = [-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100];
  const indices = [0, 2, 1, 0, 3, 2];
  const polygons = [0, 1];
  if (wall !== null) {
    positions.push(wall, 0, -50, wall, 5, -50, wall, 5, 50, wall, 0, 50);
    indices.push(4, 5, 6, 4, 6, 7);
    polygons.push(2, 3);
  }
  return new CollisionWorld(new Float32Array(positions), new Uint32Array(indices), new Uint32Array(polygons));
}

// A soldier standing at the origin facing +z, with the combat a unit runs
// explosions through (PlayerCombat without its model and weapons).
function soldierOn(w) {
  const s = new Soldier(w, {});
  s.spawn(0, 0, 0, 0);
  return s;
}
function combatFor(soldier, { health = 300, chunks = 0, base = 'soldier', random = () => 1 } = {}) {
  const combat = Object.create(PlayerCombat.prototype);
  const log = { deaths: 0, apart: 0, died: 0 };
  Object.assign(combat, {
    soldier, def: { base }, chunks, random, scale: 'PersonScale', health, maxHealth: health, dead: false, deathTime: 0, zoomed: false,
    collision: soldier.world, hit: { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() },
    worldPoint: new THREE.Vector3(), from: new THREE.Vector3(),
    hud: null, player: false, blowApart: false, lastAttacker: null, log,
    onDamaged() {}, onDeath() { log.deaths++; }, onBlowApart() { log.apart++; }, updateHeld() {},
    animator: { die() { log.died++; } },
  });
  return combat;
}
function fixture(options) {
  const soldier = soldierOn(world());
  return { soldier, combat: combatFor(soldier, options) };
}
// Where the blast goes off: `x` metres along x from the feet, `y` above them.
const at = (x, y) => new THREE.Vector3(x, y, 0);
const near = (a, b, tolerance = 1e-6) => assert.ok(Math.abs(a - b) <= tolerance, `${a} is not within ${tolerance} of ${b}`);
// Tick with no input until he lands (air null on the ground), or `ticks` pass.
function fly(soldier, ticks = 300) {
  const feet = soldier.position.y;
  let rise = 0, landed = null;
  for (let i = 1; i <= ticks; i++) {
    soldier.tick(STILL, DT);
    rise = Math.max(rise, soldier.position.y - feet);
    if (landed === null && !soldier.air && soldier.grounded) { landed = i; break; }
  }
  return { rise, landed };
}

test('area damage falls off from full inside the inner radius to nothing at the outer', () => {
  assert.equal(falloff(1, 2, 6), 1);
  assert.equal(falloff(4, 2, 6), 0.5);
  assert.equal(falloff(6, 2, 6), 0);
  assert.equal(falloff(9, 2, 6), 0);
  // Measured to the standing soldier's blast sphere, its centre 1.0 m up,
  // less half its 1.4626 m radius: a blast level with the centre x metres
  // off is x - 0.7313 away.
  const blast = { Damage: 100, DamageRadiusInner: 2, DamageRadiusOuter: 6 };
  const inner = fixture();
  inner.combat.explode(blast, at(2, 1), null, 1);
  assert.equal(inner.combat.health, 200, 'full damage inside the inner radius');
  const between = fixture();
  between.combat.explode(blast, at(4 + HALF, 1), null, 1);
  near(between.combat.health, 250, 1e-9);
  const outside = fixture();
  outside.combat.explode(blast, at(6.8, 1), null, 1);
  assert.equal(outside.combat.health, 300, 'nothing outside the outer radius');
  const friendly = fixture();
  friendly.combat.explode(blast, at(2, 1), null, 0);
  assert.equal(friendly.combat.health, 300, 'the friendly-fire factor scales the damage');
  assert.equal(inner.combat.log.deaths + between.combat.log.deaths, 0);
});

test('a wall between the blast and the soldier shields him from damage and push', () => {
  const soldier = soldierOn(world(1.5)), combat = combatFor(soldier);
  combat.explode(GRENADE, at(3, 0.9), null, 1);
  assert.equal(combat.health, 300);
  assert.deepEqual(soldier.velocity.toArray(), [0, 0, 0]);
  assert.equal(soldier.air, null);
});

test('the push adds (away + 0.6 up) x Push x falloff to the velocity and throws the soldier', () => {
  const { soldier, combat } = fixture();
  combat.explode(SHOVE, at(3, 0.1), null, 1);
  // From the blast to the sphere's centre: (-3, 0.9, 0), 3.13 m, 2.40 m
  // less half the radius, inside 4.5.
  const away = new THREE.Vector3(-3, 0.9, 0).normalize();
  near(soldier.velocity.x, away.x * 10);
  near(soldier.velocity.y, away.y * 10 + 6);
  near(soldier.velocity.z, 0);
  assert.equal(soldier.air, 'thrown');
  assert.equal(soldier.grounded, false);
  assert.equal(soldier.stateTime, 0);
  // Halfway between the push radii, half the push.
  const half = fixture();
  half.combat.explode({ Push: 10, PushRadiusInner: 2, PushRadiusOuter: 6 }, at(4 + HALF, 1), null, 1);
  near(half.soldier.velocity.x, -5, 1e-9);
  near(half.soldier.velocity.y, 3, 1e-9);
  assert.equal(half.soldier.air, 'thrown');
  // A shove that lifts him less than 2 m/s nudges him without a throw.
  const nudge = fixture();
  nudge.combat.explode({ Push: 1, PushRadiusInner: 4, PushRadiusOuter: 4 }, at(3, 1), null, 1);
  near(nudge.soldier.velocity.x, -1, 1e-9);
  near(nudge.soldier.velocity.y, 0.6, 1e-9);
  assert.equal(nudge.soldier.air, null);
  assert.equal(nudge.soldier.grounded, true);
});

test('a thrown soldier flies away from the blast and lands back on the ground', () => {
  const { soldier, combat } = fixture();
  combat.explode(SHOVE, at(3, 0.1), null, 1);
  const lift = soldier.velocity.y;
  const { rise, landed } = fly(soldier);
  // The arc under the thrown gravity, within a tick's rounding.
  near(rise, lift * lift / (2 * ENGINE.thrownGravity), 0.15);
  assert.ok(rise > 2, `rose ${rise} m`);
  assert.ok(landed > ENGINE.landLock * 60, `landed at tick ${landed}`);
  assert.equal(soldier.air, null);
  assert.equal(soldier.grounded, true);
  assert.ok(soldier.position.x < -3, `flew ${soldier.position.x} m along x`);
  near(soldier.position.y, 0, 0.05);
  for (let i = 0; i < 60; i++) soldier.tick(STILL, DT);
  assert.equal(soldier.air, null, 'stays on the ground');
});

test('a soldier the blast kills is thrown all the same and lands dead', () => {
  const { soldier, combat } = fixture();
  combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  assert.equal(combat.health, 0);
  assert.equal(combat.dead, true);
  assert.equal(combat.log.deaths, 1);
  assert.equal(combat.log.died, 1, 'dies whole, with its death clip');
  assert.equal(soldier.air, 'thrown');
  assert.ok(soldier.velocity.y > 6);
  const { rise, landed } = fly(soldier);
  assert.ok(rise > 2 && landed, `rose ${rise} m, landed at ${landed}`);
  assert.equal(combat.dead, true);
  assert.ok(soldier.position.x < -3);
});

test('a body already lying dead is thrown too; a droid that blew apart is not', () => {
  const corpse = fixture();
  corpse.combat.dead = true;
  corpse.combat.health = 0;
  corpse.combat.explode(GRENADE, at(3, 0.1), null, 1);
  assert.equal(corpse.soldier.air, 'thrown');
  assert.equal(corpse.combat.health, 0);
  assert.equal(corpse.combat.log.deaths, 0, 'no second death');
  // A droid with chunks: the blast rolls to blow it apart first (a hit
  // leaves it unpushed), it goes up in pieces at its death, and later blasts
  // find nothing to throw.
  const droid = fixture({ chunks: 0.6, random: () => 0.5 });
  droid.combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  assert.equal(droid.combat.dead, true);
  assert.equal(droid.combat.log.apart, 1);
  assert.equal(droid.combat.log.died, 0);
  assert.deepEqual(droid.soldier.velocity.toArray(), [0, 0, 0]);
  assert.equal(droid.soldier.air, null);
  droid.combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  assert.deepEqual(droid.soldier.velocity.toArray(), [0, 0, 0]);
  // The roll missing, the droid is thrown and dies whole.
  const whole = fixture({ chunks: 0.6, random: () => 0.9 });
  whole.combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  assert.equal(whole.combat.dead, true);
  assert.equal(whole.combat.log.apart, 0);
  assert.equal(whole.combat.log.died, 1);
  assert.equal(whole.soldier.air, 'thrown');
  // A droideka's hull takes the damage but never the launch.
  const droideka = fixture({ base: 'walkerdroid', health: 1000 });
  droideka.combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  assert.equal(droideka.combat.health, 680);
  assert.deepEqual(droideka.soldier.velocity.toArray(), [0, 0, 0]);
});

test('an explosion without Push (older extracted data) hurts without a push or a crash', () => {
  for (const blast of [{ Damage: 50, DamageRadiusInner: 2, DamageRadiusOuter: 6 }, { Damage: 50, DamageRadiusInner: 2, DamageRadiusOuter: 6, Push: 0, PushRadiusInner: 0, PushRadiusOuter: 0 }, { Damage: 50, DamageRadiusInner: 2, DamageRadiusOuter: 6, Push: 10 }]) {
    const { soldier, combat } = fixture();
    combat.explode(blast, at(2, 0.9), null, 1);
    assert.equal(combat.health, 250);
    assert.deepEqual(soldier.velocity.toArray(), [0, 0, 0]);
    assert.equal(soldier.air, null);
    assert.equal(soldier.grounded, true);
  }
});

test("the compact correction carries the thrown flight, and a client's replay lands with the authority", () => {
  const w = world(), authority = soldierOn(w), client = soldierOn(w);
  const combat = combatFor(authority);
  for (let i = 0; i < 10; i++) { authority.tick(STILL, DT); client.tick(STILL, DT); }
  combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  const correction = captureCompactSoldier(authority);
  restoreCompactSoldier(client, correction);
  assert.equal(client.air, 'thrown');
  assert.equal(client.grounded, false);
  assert.deepEqual(client.velocity.toArray(), authority.velocity.toArray());
  let landed = null;
  for (let tick = 1; tick <= 300; tick++) {
    authority.tick(STILL, DT);
    client.tick(STILL, DT);
    assert.deepEqual(captureCompactSoldier(client), captureCompactSoldier(authority), `tick ${tick}`);
    if (landed === null && !authority.air && authority.grounded) landed = tick;
  }
  assert.ok(landed, 'landed');
  assert.equal(client.air, null);
});

test('other players are told a body still flying after its death is thrown, not merely dead', () => {
  const soldier = soldierOn(world()), combat = combatFor(soldier);
  const unit = { id: 7, generation: 2, team: 1, soldier, combat, dead: false };
  const state = () => ENTITY_SOLDIER_STATES[captureEntitySoldier(unit, { tick: 1, classId: 1 }).state];
  assert.equal(state(), 'ground');
  combat.explode(GRENADE, at(2.7, 0.1), null, 1);
  unit.dead = combat.dead;
  assert.equal(state(), 'thrownDead');
  fly(soldier);
  assert.equal(state(), 'dead');
  const living = fixture();
  living.combat.explode(SHOVE, at(3, 0.1), null, 1);
  assert.equal(ENTITY_SOLDIER_STATES[captureEntitySoldier({ id: 8, generation: 1, team: 1, soldier: living.soldier, combat: living.combat, dead: false }, { tick: 1, classId: 1 }).state], 'thrown');
});
