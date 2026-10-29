import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { RadarUnits, HitRing, radarAlpha, radarSensors, radarRelation, unitColor, findReticleTarget, reticleRange, killMessage } from "../src/hud-units.js";
import { BattleHud } from "../src/hud.js";

const vector = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z);
const unit = (x = 0, z = 0, team = 2) => ({ position: vector(x, 0, z), sphere: { center: vector(x, 0, z), radius: .5 }, kind: 0, classDef: {}, team, alive: true, health: 100, generation: 1 });
const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-10, `${a} != ${b}`);

test("radar palette: a locals team is yellow, friend or foe, as the original's team colour table has it", () => {
  assert.equal(radarRelation(1, 0), "neutral");
  assert.equal(radarRelation(1, 1), "friendly");
  assert.equal(radarRelation(1, 2), "enemy");
  assert.equal(radarRelation(1, 3), "locals");
  // Kashyyyk's Wookiees are the Rebels' friends and still yellow to them.
  assert.equal(radarRelation(2, 3, () => 1), "locals");
  // By the mission's factions: Theed's Naboo Guard (team 2, faction Locals)
  // is yellow to the CIS and friendly to itself; Hoth's unnamed team 4 is red.
  const factions = [0, 4, 5, 5, 0];
  const locals = (team) => factions[team] === 5;
  assert.equal(radarRelation(1, 2, undefined, locals), "locals");
  assert.equal(radarRelation(2, 2, undefined, locals), "friendly");
  assert.equal(radarRelation(1, 4, undefined, locals), "enemy");
  assert.equal(radarRelation(1, 4, () => 0, locals), "neutral");
  close(unitColor("enemy", true)[0], (223 + 64) / 510);
  assert.deepEqual(unitColor("enemy", false, 1, .3), [1, 1, 1, .3]);
});

test("radar visibility prioritizes friendship, dwindling tickets and Easy", () => {
  const target = unit();
  for (const options of [{ friendly: true }, { tickets: 4, living: 4 }, { easy: true }]) assert.equal(radarAlpha(target, options), 1);
  assert.equal(radarAlpha(target, { tickets: 5, living: 4 }), 0);
  close(radarAlpha(target, { flash: .2 }, 1, 1), 223 / 255);
  assert.equal(radarAlpha(target, {}, 1, 8), 0);
});

test("sensor boundary is strict; transmitters must reach the controlled object", () => {
  const viewer = unit(0, 0, 1), controlled = unit(20, 0, 1), transmitter = unit(30, 0, 1);
  controlled.classDef.ScanningRange = 5;
  transmitter.classDef = { ScanningRange: 10, TransmitRange: 10 };
  const sensors = radarSensors([viewer, controlled, transmitter], viewer, controlled);
  assert.equal(sensors.length, 2);
  assert.equal(radarAlpha(unit(40, 0), { sensors }), 0);
  assert.equal(radarAlpha(unit(39.9, 0), { sensors }), 1);
  transmitter.position.x = 30.01;
  assert.equal(radarSensors([transmitter], viewer, controlled).length, 1);
});

test("radar keeps shot memory but clears it for a pooled new life", () => {
  const radar = new RadarUnits(), target = unit(), viewer = unit(0, 0, 1);
  assert.equal(radar.update(0, 0, [target], { viewer }).length, 0);
  radar.shot(target, { ShotDelay: .2 }, 0);
  let icons = radar.update(.01, .01, [target], { viewer });
  assert.equal(icons.length, 1);
  assert.equal(icons[0].texture, "troop_icon");
  close(icons[0].size, 8.4);
  assert.ok(icons[0].color[1] > .9);
  assert.equal(radar.update(1, 1.01, [target], { viewer }).length, 1);
  target.generation++;
  assert.equal(radar.update(0, 1.01, [target], { viewer }).length, 0);
});

test("radar hides seated, removed and unspawned soldiers; deduplicates vehicles", () => {
  const radar = new RadarUnits(), viewer = unit(0, 0, 1), seated = unit(), removed = unit(), unspawned = unit();
  seated.seat = {}; removed.removed = true; unspawned.alive = false;
  const vehicle = unit(); vehicle.kind = 6; vehicle.def = { settings: { MapTexture: "tank", MapScale: 2 } }; delete vehicle.classDef;
  const icons = radar.update(0, 0, [viewer, seated, removed, unspawned, vehicle], { viewer, easy: true, objects: [vehicle] });
  assert.equal(icons.length, 1); assert.equal(icons[0].texture, "tank"); assert.equal(icons[0].size, 12);
});

test("radar dwindling tickets count living non-hero soldiers only", () => {
  const radar = new RadarUnits(), target = unit(), hero = unit(), vehicle = unit();
  hero.hero = true; vehicle.kind = 6;
  assert.equal(radar.update(0, 0, [target, hero, vehicle], { viewerTeam: 1, tickets: () => 2 }).length, 0);
  assert.equal(radar.update(0, 0, [target, hero, vehicle], { viewerTeam: 1, tickets: () => 1 }).length, 2);
});

test("hit ring accumulates each relationship and uses max intensity normalization", () => {
  const ring = new HitRing();
  assert.equal(ring.color()[3], 0);
  ring.hit(-1); ring.hit("locals"); ring.hit(1);
  assert.equal(ring.time.enemy, 1);
  assert.deepEqual(ring.color(), [224 / 255, 224 / 255, 32 / 255, 128 / 255]);
  ring.update(.55);
  assert.deepEqual(ring.color(), [224 / 255, 32 / 255, 32 / 255, 128 / 255]);
  ring.update(.4);
  close(ring.color()[3], 64 / 255);
  ring.update(.1); assert.equal(ring.color()[3], 0);
});

const pick = (units, options = {}) => findReticleTarget({ origin: vector(), dir: vector(0, 0, 1), viewer: unit(0, 0, 1), units,
  weapon: { LockOnAngle: 1, LockOnRange: 100, TargetAll: 1, TargetEnemy: 0, ordnance: { Velocity: 100, LifeSpan: 2 } }, ...options });

test("reticle cone finds an off-ray target and orders by 16d² + t", () => {
  const near = unit(.65, 10), centred = unit(0, 15), farOutside = unit(10, 10);
  assert.equal(pick([near]).unit, near);
  assert.equal(pick([near, centred, farOutside]).unit, centred);
  assert.equal(pick([unit(0, -3), farOutside]), null);
});

test("lockable targets win; blocked locks do not hide a visible fallback", () => {
  const friend = unit(0, 3, 1), enemy = unit(0, 20, 2);
  const weapon = { LockOnAngle: 1, LockOnRange: 100, TargetEnemy: 1, TargetFriendly: 0, TargetPerson: 1 };
  assert.equal(pick([friend, enemy], { weapon }).unit, enemy);
  assert.equal(pick([friend, enemy], { weapon, los: (a, b, viewer, target) => target !== enemy }).unit, friend);
  enemy.seat = {};
  assert.equal(pick([enemy], { weapon }), null);
});

test("reticle range supports exact extracted override and projectile fallback", () => {
  const def = { LockOnRange: 64, ordnance: { Velocity: 150, LifeSpan: .8 } };
  assert.equal(reticleRange(def), 120);
  assert.equal(reticleRange({ ...def, ReticleRange: 64 }), 64);
  assert.equal(pick([unit(0, 121)], { weapon: def }), null);
});

test("reticle rejects dead targets and honours category masks", () => {
  const corpse = unit(0, 1); corpse.health = 0;
  const tank = unit(0, 5); tank.kind = 6;
  const infantry = unit(0, 10);
  const weapon = { LockOnAngle: 1, LockOnRange: 50, TargetAll: 0, TargetPerson: 1 };
  assert.equal(pick([corpse, tank, infantry], { weapon }).unit, infantry);
});

test("kill feed is human/hero gated, ordered and localized", () => {
  const killer = unit(0, 0, 1), victim = unit();
  killer.character = { name: "Alpha" }; victim.character = { name: "Beta" };
  assert.equal(killMessage({ killer, victim }), null);
  killer.hero = true;
  assert.equal(killMessage({ killer, victim }).text, "Alpha KILLED Beta");
  // The player is his Character's name ("Player 1" offline), or that
  // default without one.
  victim.human = true;
  victim.character = { name: "Player 1", human: true };
  assert.equal(killMessage({ victim }).text, "Player 1 DIED");
  assert.equal(killMessage({ killer: victim, victim }).text, "Player 1 KILLED HIMSELF");
  victim.team = 1;
  assert.equal(killMessage({ killer, victim }, { "game.infomsg.teamkill": "%s -> %s" }).text, "Alpha -> Player 1");
  victim.character = null;
  assert.equal(killMessage({ victim }).text, "Player 1 DIED");
});

test("HUD stats baseline is quiet, changes deduct two points per teamkill", () => {
  const hud = { statValues: null, statsLeft: 0, statChanges: [0, 0, 0], statFlash: [0, 0, 0] };
  BattleHud.prototype.stats.call(hud, { kills: 1 });
  assert.equal(hud.statsLeft, 0);
  BattleHud.prototype.stats.call(hud, { kills: 1, teamkills: 1, deaths: 1, captures: 1 });
  assert.deepEqual(hud.statValues, [-1, 1, 1]);
  assert.deepEqual(hud.statChanges, [-1, 1, 1]); assert.equal(hud.statsLeft, 3);
  hud.statFlash = [.3, .3, .3];
  BattleHud.prototype.stats.call(hud, { kills: 1, teamkills: 1, deaths: 2, captures: 1 });
  assert.deepEqual(hud.statFlash, [.3, 1, .3], "another stat change preserves existing flash timers");
});

test("enemy health snapshot survives looking away without following pooled units", () => {
  const hud = { healthFader: { wake() {} } }, target = unit(); target.maxHealth = 200;
  BattleHud.prototype.enemyHealth.call(hud, target);
  assert.equal(hud.enemy.fill, .5); assert.equal(hud.enemyLeft, 2);
  target.health = 200;
  BattleHud.prototype.enemyHealth.call(hud, null);
  assert.equal(hud.enemy.fill, .5);
});

test("map hides out-of-range soldiers without pinning, rotates icons, and draws player last", () => {
  const hud = Object.create(BattleHud.prototype), quads = [];
  Object.assign(hud, { frame: { center: [0, 0], half: 300 }, player: { position: vector(), yaw: 0, team: 1 }, mapImage: "map",
    mapScale: 1, expanded: false, unitSource: [], posts: { posts: [] },
    radar: { icons: [125, 126].map((z) => ({ unit: unit(0, z), texture: "troop", size: 8.4, color: [1, 1, 1, 1], yaw: .7 })) },
    anchor: () => new THREE.Matrix4(), texture: (name) => name,
    batch: { quad: (...args) => quads.push(args), quadCorners() {} } });
  hud.drawMap();
  assert.deepEqual(quads.map((q) => q[5]), ["troop", "player_icon"]);
  close(quads[0][0].elements[0], Math.cos(-.7));
  quads.length = 0; hud.expanded = true; hud.drawMap();
  assert.deepEqual(quads.map((q) => q[5]), ["map", "troop", "troop", "player_icon"]);
  close(quads[1][0].elements[0], Math.cos(Math.PI - .7));
});
