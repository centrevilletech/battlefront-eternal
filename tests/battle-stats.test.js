import test from "node:test";
import assert from "node:assert/strict";
import { BattleStats, recordOf, personalStats, awards, clock, observe, RING_SIZE } from "../src/battle-stats.js";
import { Battle, Character } from "../src/battle.js";

const character = (team, name, index = 0) => Object.assign(new Character(index, team, false), { name });

test("kills: per victim, team kills apart, this life's best and the survivalist's low health", () => {
  const stats = new BattleStats();
  const me = character(1, "ME");
  const friend = character(1, "FRIEND");
  const foe = character(2, "FOE");
  stats.kill(me, foe, -1);
  stats.kill(me, foe, -1);
  stats.kill(me, friend, 1);
  stats.kill(me, me, 1);
  const own = recordOf(me).slots[0];
  assert.equal(own.victims.get(foe), 2);
  assert.equal(own.victims.get(friend), 1);
  assert.equal(own.victims.get(me), 1, "a suicide is in his own row too");
  assert.equal(own.teamKills, 2, "TRAITOR: the friend and himself");
  assert.equal(own.lifeKills, 2, "only enemies count for PUBLIC ENEMY");
  stats.death(me);
  assert.deepEqual([own.lifeKills, own.lifeKillsBest, own.deaths], [0, 2, 1]);
  stats.kill(me, foe, 0);
  assert.equal(own.victims.get(foe), 2, "a neutral's death credits nothing");
  recordOf(me).health = 63;
  stats.kill(me, foe, -1);
  recordOf(me).health = 64;
  stats.kill(me, foe, -1);
  assert.equal(own.survivalist, 1, "under 64/255 only");
  assert.equal(recordOf(foe).slots[1].teamKills, 0, "team 2 keeps slot 1");
});

test("KILLING SPREE: kills inside 5 s, counted only while the ring's next free entry holds the killer's slot", () => {
  const stats = new BattleStats();
  const rep = character(1, "REP");
  const cis = character(2, "CIS");
  for (let i = 0; i < 3; i++) {
    stats.kill(rep, cis, -1);
    stats.tick(1, []);
  }
  assert.equal(recordOf(rep).slots[0].spree, 3, "team 1: three in 3 s");
  stats.tick(10, []);
  stats.kill(rep, cis, -1);
  assert.equal(recordOf(rep).slots[0].spree, 3, "the old ones fell out of the window");
  for (let i = 0; i < 4; i++) stats.kill(cis, rep, -1);
  assert.equal(recordOf(cis).slots[1].spree, 0, "team 2's go uncounted: the next entry's stale slot is 0 (the original's bug)");
  // Once the ring has wrapped, an entry team 2 wrote can be the next one.
  const late = new BattleStats();
  for (let i = 0; i < RING_SIZE - 1; i++) {
    late.kill(cis, rep, -1);
    late.tick(5.01, []);
  }
  late.kill(cis, rep, -1);
  late.kill(cis, rep, -1);
  assert.ok(recordOf(cis).slots[1].spree >= 1, "counted after the wrap");
});

test("head shots and destroyed vehicles go to whoever is credited; nothing is u8-unlimited", () => {
  const stats = new BattleStats();
  const me = character(2, "ME");
  stats.headshot(me);
  stats.headshot(null);
  stats.vehicleDestroyed(me);
  assert.deepEqual([recordOf(me).slots[1].headshots, recordOf(me).slots[1].vehiclesDestroyed], [1, 1]);
  for (let i = 0; i < 255; i++) stats.vehicleDestroyed(me);
  assert.equal(recordOf(me).slots[1].vehiclesDestroyed, 0, "a byte: it wraps");
});

test("a destroyed vehicle leaves the vehicle time table alone (it once replaced it with a count)", () => {
  const stats = new BattleStats();
  const me = character(1, "ME");
  me.className = "rep_inf_trooper";
  stats.vehicleDestroyed(me);
  me.unit = body({ x: 0, y: 0, z: 0 }, { vehicle: "rep_hover_fightertank" });
  assert.doesNotThrow(() => stats.tick(1, [me]));
  const own = recordOf(me).slots[0];
  assert.equal(own.vehiclesDestroyed, 1);
  assert.deepEqual(own.vehicles.map((e) => [e.key, e.time]), [["rep_hover_fightertank", 1]]);
});

// A body for the tallies.
function body(position, { health = 1, weapon = null, vehicle = null } = {}) {
  return {
    removed: false, alive: health > 0, position,
    combat: { dead: health <= 0, health: health * 300, maxHealth: 300, arsenal: { current: () => (weapon ? { name: weapon, def: { displayName: weapon.toUpperCase() } } : null) } },
    seat: vehicle ? { released: false, index: 0, vehicle: { name: vehicle, label: vehicle.toUpperCase(), position, seats: [{ weapons: [{ state: { name: `${vehicle}_gun`, def: { label: "CANNON" } } }] }] } } : null,
    classDef: { displayName: "TROOPER" },
  };
}

test("tallies: class time and the life clock (even without a body), weapons, vehicles, camping, narrow escapes", () => {
  const stats = new BattleStats();
  const bot = character(1, "BOT");
  bot.className = "rep_inf_trooper";
  stats.tick(2, [bot]);
  const own = recordOf(bot).slots[0];
  assert.deepEqual(own.classes.map((e) => [e.key, e.time]), [["rep_inf_trooper", 2]], "waiting to spawn: class time runs");
  assert.equal(own.life, 2);
  const at = { x: 0, y: 0, z: 0 };
  bot.unit = body(at, { weapon: "rep_weap_rifle" });
  stats.tick(1, [bot]);
  stats.tick(1, [bot]);
  assert.equal(own.camp, 1, "the first tick sets the spot");
  at.x = 9.9;
  stats.tick(1, [bot]);
  assert.equal(own.camp, 2, "within 10 m");
  at.x = 10.1;
  stats.tick(1, [bot]);
  assert.deepEqual([own.camp, own.campBest], [0, 2], "out of it: a new spot");
  assert.equal(own.weapons[0].time, 4);
  assert.equal(own.weapons[0].label, "REP_WEAP_RIFLE");
  bot.unit = body(at, { vehicle: "rep_hover_fightertank" });
  stats.tick(3, [bot]);
  assert.deepEqual(own.vehicles.map((e) => [e.key, e.label, e.time]), [["rep_hover_fightertank", "REP_HOVER_FIGHTERTANK", 3]]);
  assert.equal(own.weapons.find((e) => e.key === "rep_hover_fightertank_gun").time, 3, "the seat's weapon");
  // Down under 10% and back to full: a narrow escape.
  bot.unit = body(at, { health: 0.05 });
  stats.tick(0.1, [bot]);
  bot.unit = body(at, { health: 0.99 });
  stats.tick(0.1, [bot]);
  assert.equal(own.escapes, 0);
  bot.unit = body(at, { health: 1 });
  stats.tick(0.1, [bot]);
  assert.equal(own.escapes, 1);
  assert.equal(recordOf(bot).health, 255);
  // Not ticked: nothing.
  stats.tick(5, [bot], () => false);
  assert.equal(own.vehicles[0].time, 3);
});

test("the time tables keep 5 classes, 10 weapons, 5 vehicles; a new one replaces the least used", () => {
  const stats = new BattleStats();
  const bot = character(1, "BOT");
  const at = { x: 0, y: 0, z: 0 };
  for (let i = 0; i < 6; i++) {
    bot.className = `class${i}`;
    bot.unit = body(at, { vehicle: `v${i}` });
    stats.tick(10 - i, [bot]);
  }
  const own = recordOf(bot).slots[0];
  assert.equal(own.classes.length, 5);
  assert.deepEqual(own.classes.map((e) => e.key).sort(), ["class0", "class1", "class2", "class3", "class5"]);
  assert.equal(own.classes.find((e) => e.key === "class5").time, 5, "started again at its own time");
  assert.equal(own.vehicles.length, 5);
});

test("DETAILS: head shots, favourites with m:ss, the living streak with its leading space, nemesis and bait", () => {
  const stats = new BattleStats();
  const me = character(1, "ME");
  const foe = character(2, "UNIT 538");
  const other = character(2, "UNIT 182");
  me.className = "rep_inf_trooper";
  me.unit = body({ x: 0, y: 0, z: 0 }, { weapon: "rep_weap_rifle" });
  stats.tick(187.9, [me]);
  stats.headshot(me);
  stats.headshot(me);
  stats.kill(foe, me, -1);
  stats.kill(foe, me, -1);
  stats.kill(other, me, -1);
  stats.kill(me, other, -1);
  const rows = personalStats(me, [me, foe, other], { label: () => null });
  const values = Object.fromEntries(rows.map((r) => [r.label, r.value]));
  assert.deepEqual(values, {
    "HEAD SHOTS": "2",
    "FAVORITE CLASS": "TROOPER 3:07",
    "FAVORITE WEAPON": "REP_WEAP_RIFLE 3:07",
    "FAVORITE VEHICLE": "",
    "LONGEST LIVING STREAK": " 3:07",
    "NARROW ESCAPES": "0",
    "VEHICLES DESTROYED": "0",
    NEMESIS: "UNIT 538 (2)",
    BAIT: "UNIT 182 (1)",
  });
  assert.equal(clock(59.99), "0:59");
  assert.equal(clock(3600), "60:00", "minutes unpadded");
  // A class seen only without a body takes the port's name for it.
  const ghost = character(1, "GHOST");
  ghost.className = "rep_inf_pilot";
  stats.tick(5, [ghost]);
  assert.equal(personalStats(ghost, [ghost], { label: (kind, key) => (kind === "class" ? key.toUpperCase() : null) })[1].value, "REP_INF_PILOT 0:05");
  assert.equal(personalStats(ghost, [ghost])[1].value, "", "no name: the label alone");
});

test("AWARDS: the most over everyone, slot 0 then slot 1 in roster order; ties to the first; nothing for 0", () => {
  const stats = new BattleStats();
  const a = character(1, "A");
  const b = character(2, "B");
  const c = character(1, "C");
  const all = [a, b, c];
  stats.vehicleDestroyed(b);
  stats.vehicleDestroyed(c);
  for (let i = 0; i < 3; i++) stats.death(b);
  stats.death(a);
  stats.headshot(c);
  recordOf(a).slots[0].campBest = 125.7;
  const result = Object.fromEntries(awards(all).map((w) => [w.label, w.text]));
  assert.deepEqual(result, {
    "TANK BUSTER": "C (1)", // a tie: slot 0 (team 1) before slot 1
    "KILLING SPREE": "",
    CAMPER: "A (2:05)",
    SURVIVALIST: "",
    TRAITOR: "",
    "PUBLIC ENEMY": "",
    "BANTHA FODDER": "B (3)",
    "DEAD EYE": "C (1)",
  });
});

test("the battle feeds the record: its credits, its ticks, and nothing once the game is over", () => {
  const posts = { posts: [{ name: "a", team: 1, desc: {} }], owned: (team) => (team === 1 ? [{ desc: {} }] : []) };
  const battle = new Battle({ teams: { 1: { units: 2, reinforcements: 5 }, 2: { units: 1, reinforcements: 5 } }, posts, own: 1, foe: 2 });
  battle.populate(1);
  const [human, bot] = battle.team(1);
  const [foe] = battle.team(2);
  bot.className = "rep_inf_trooper";
  battle.credit(foe, human, -1);
  battle.credit(human, foe, -1);
  battle.headshot(foe);
  battle.vehicleDestroyed(human);
  battle.update(1);
  assert.equal(recordOf(foe).slots[1].victims.get(human), 1);
  assert.equal(recordOf(human).slots[0].deaths, 1);
  assert.equal(recordOf(bot).slots[0].life, 1, "bots tally every tick");
  assert.equal(recordOf(human).slots[0].classes.length, 0, "the player only with a body");
  battle.reinforcements[2] = 0;
  for (let i = 0; i < 200; i++) battle.update(1 / 60);
  assert.equal(battle.gameOver, true);
  const life = recordOf(bot).slots[0].life;
  battle.update(1);
  battle.headshot(foe);
  battle.vehicleDestroyed(human);
  assert.equal(recordOf(bot).slots[0].life, life, "frozen");
  assert.deepEqual([recordOf(foe).slots[1].headshots, recordOf(human).slots[0].vehiclesDestroyed], [1, 1]);
});

test("observe: a seat's vehicle and its first weapon, a turret's weapon, a body's own", () => {
  const at = { x: 1, y: 2, z: 3 };
  const seen = observe({ unit: body(at, { vehicle: "cis_tread_snailtank" }) });
  assert.equal(seen.vehicle.key, "cis_tread_snailtank");
  assert.equal(seen.weapon.key, "cis_tread_snailtank_gun");
  const turret = { removed: false, alive: true, position: at, combat: { dead: false, health: 1, maxHealth: 1 }, seat: { released: false, turret: { position: { x: 9, y: 9, z: 9 }, weapon: { name: "tur_gun", def: { label: "TURRET" } } } } };
  const mounted = observe({ unit: turret });
  assert.deepEqual([mounted.position.x, mounted.weapon.key, mounted.vehicle], [9, "tur_gun", null]);
  assert.equal(observe({ unit: null }), null);
});
