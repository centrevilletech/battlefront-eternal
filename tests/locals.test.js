// Locals (teams 3 and up: Tatooine's Tusken Raiders and Jawas, Naboo's
// Gungans, Endor's Ewoks, Kashyyyk's Wookiees, Geonosis' Geonosians): the
// mission's relations, factions and capture teams; command posts; the
// battle's team rules; the HUD's colours; and, with an extract, a battle.
//
//   node --test tests/locals.test.js
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as THREE from "three";
import { missionTeams } from "../src/ai/classic/spawning.js";
import { Units } from "../src/registry.js";
import { CommandPosts } from "../src/commandposts.js";
import { Battle } from "../src/battle.js";
import { RadarUnits, radarRelation } from "../src/hud-units.js";
import { BattleHud } from "../src/hud.js";

const call = (name, ...args) => ({ name, args });
const named = (team, name) => call("SetTeamName", team, name);
const armies = [named(1, "Republic"), named(2, "CIS")];
// The calls that decide each map's locals, as the missions make them.
const MISSIONS = {
  tusken: [...armies, named(3, "locals"), call("AddUnitClass", 3, "tat_inf_tuskenraider", 5), call("SetUnitCount", 3, 14)],
  jawa: [...armies, call("SetTeamAsFriend", 1, 3), call("SetTeamAsFriend", 2, 3), named(3, "locals"),
    call("SetTeamAsFriend", 3, 1), call("SetTeamAsFriend", 3, 2)],
  gungan: [...armies, call("SetTeamAsEnemy", 1, 3), call("SetTeamAsFriend", 2, 3), named(3, "Locals"), call("SetTeamAsFriend", 3, 2)],
  ewok: [...armies, call("SetTeamAsFriend", 1, 3), call("SetTeamAsEnemy", 2, 3), named(3, "locals"), call("SetTeamAsFriend", 3, 1)],
  probe: [...armies, named(3, "Local"), call("AddUnitClass", 4, "imp_droid_probe", 1), call("SetUnitCount", 4, 1),
    call("SetTeamAsEnemy", 3, 2), call("SetTeamAsFriend", 3, 1), call("SetTeamAsFriend", 1, 3), call("SetTeamAsEnemy", 2, 3)],
};

test("the mission decides who fights the locals and whom they capture for", () => {
  const relations = (table, team) => [1, 2, 3].map((other) => table.relations[team][other]);
  const tusken = missionTeams(MISSIONS.tusken);
  assert.deepEqual([relations(tusken, 1), relations(tusken, 3)], [[1, -1, -1], [-1, -1, 1]], "the Tusken Raiders fight everybody");
  assert.equal(tusken.capture[3], 3, "and take posts for themselves");
  assert.equal(tusken.factions[3], 5);
  const jawa = missionTeams(MISSIONS.jawa);
  assert.deepEqual([relations(jawa, 1), relations(jawa, 2), relations(jawa, 3)], [[1, -1, 1], [-1, 1, 1], [1, 1, 1]]);
  assert.equal(jawa.capture[3], 0, "friends of both armies, the Jawas take no post");
  const gungan = missionTeams(MISSIONS.gungan);
  assert.deepEqual(relations(gungan, 3), [-1, 1, 1]);
  assert.equal(gungan.capture[3], 2, "the Gungans capture for the army they fight beside");
  assert.equal(missionTeams(MISSIONS.ewok).capture[3], 1);
  const probe = missionTeams(MISSIONS.probe);
  assert.deepEqual([probe.capture[3], probe.capture[4], probe.factions[3], probe.factions[4]], [1, 4, 5, 0]);
  assert.deepEqual([1, 2].map((army) => probe.capture[army]), [1, 2]);
});

test("the registry answers with the mission's table", () => {
  const units = new Units();
  assert.equal(units.relation(1, 3), -1, "without a mission, every other team is an enemy");
  assert.equal(units.locals(3), true);
  units.setTeams(missionTeams(MISSIONS.jawa));
  assert.deepEqual([units.relation(1, 3), units.relation(3, 2), units.relation(1, 2), units.relation(0, 3)], [1, 1, -1, 0]);
  assert.deepEqual([units.captureTeam(1), units.captureTeam(3)], [1, 0]);
  units.setTeams(missionTeams(MISSIONS.probe));
  assert.deepEqual([units.locals(3), units.locals(4), units.relation(4, 1)], [true, false, -1]);
  // Allied or hostile, a locals team is drawn in the locals' colour; Hoth's
  // unnamed probe droid team is an enemy.
  const look = (viewer, team) => radarRelation(viewer, team, (a, b) => units.relation(a, b), (t) => units.locals(t));
  assert.deepEqual([look(1, 3), look(2, 3), look(1, 4), look(1, 1), look(1, 0)], ["locals", "locals", "enemy", "friendly", "neutral"]);
});

const at = (x, z) => new THREE.Matrix4().makeTranslation(x, 0, z).toArray();
const desc = (name, team, x, { capture = true, map = true, holo = true } = {}) => ({
  name, team, displayName: name.toUpperCase(), matrix: at(x, 0), hologram: [x, 3, 0], valueBleed: map ? 10 : 0,
  capture: capture ? { type: "box", size: [4, 4, 4], matrix: at(x, 0) } : null,
  mapTexture: map ? "hud_flag_icon" : null, mapScale: map ? 2.5 : 0, holoOdf: holo ? "com_holo_controlzone" : null,
});
const LOOK = { height: 3.5, size: 2.5, beamIntensity: 1, lightIntensity: 1, imageIntensity: .5, flareIntensity: 1, lightRadius: 8,
  rotateRate: .2, flickerRate: .01, flickerAlpha: [.4, .8], fadeInTime: 2, visibleDistance: 200,
  neutral: [255, 255, 255], friendly: [30, 220, 30], enemy: [240, 30, 30], locals: [240, 220, 30] };

function battlefield(mission, { headless = true } = {}) {
  const units = new Units();
  units.setTeams(missionTeams(mission));
  const map = {
    commandPosts: [desc("cp1", 1, 0), desc("cp2", 2, 100), desc("cp3", 0, 200), desc("ts1", 3, 300, { capture: false, map: false, holo: false })],
    commandPost: { neutralizeTime: 12, captureTime: 10, holoTurnOnTime: 2.2 },
    hologram: LOOK,
    mission: { teams: { 1: { name: "Republic" }, 2: { name: "CIS" }, 3: { name: "locals" } } },
  };
  const posts = new CommandPosts(new THREE.Scene(), map, { logos: new Map(), textures: {}, playerTeam: 1, teams: units, headless });
  return { units, posts, post: (name) => posts.posts.find((p) => p.name === name) };
}
const soldier = (team, x) => ({ team, position: new THREE.Vector3(x, 0, 0) });
const run = (posts, soldiers, seconds) => {
  for (let t = 0; t < seconds * 60; t++) posts.update(1 / 60, soldiers);
};

test("Jawas in a post count for nobody; Gungans count for their army", () => {
  const jawas = battlefield(MISSIONS.jawa);
  run(jawas.posts, [soldier(3, 0), soldier(3, 0), soldier(3, 200)], 40);
  assert.equal(jawas.post("cp1").team, 1, "friends of both, they don't neutralize the Republic's post");
  assert.equal(jawas.post("cp3").team, 0, "nor take the neutral one");
  const gungans = battlefield(MISSIONS.gungan);
  run(gungans.posts, [soldier(3, 100), soldier(3, 100)], 30);
  assert.deepEqual([gungans.post("cp2").team, gungans.post("cp2").neutralize], [2, 0], "Gungans in the CIS post don't attack it");
  run(gungans.posts, [soldier(3, 100), soldier(1, 100)], 6);
  assert.ok(Math.abs(gungans.post("cp2").neutralize - 0.5) < 0.01, "nor slow the Republic soldier neutralizing it");
  const helped = battlefield(MISSIONS.gungan);
  run(helped.posts, [soldier(3, 200)], 12);
  assert.equal(helped.post("cp3").team, 2, "a Gungan alone takes the neutral post for the CIS");
});

test("Tusken Raiders take posts for themselves; two attacking teams add their rates", () => {
  const raid = battlefield(MISSIONS.tusken);
  run(raid.posts, [soldier(3, 100)], 12.1);
  assert.equal(raid.post("cp2").team, 0, "one raider neutralizes the CIS post in 12 s");
  run(raid.posts, [soldier(3, 100)], 10.1);
  assert.equal(raid.post("cp2").team, 3, "and captures it in 10 more");
  const both = battlefield(MISSIONS.tusken);
  both.posts.update(1 / 60, [soldier(1, 100), soldier(3, 100)]);
  assert.ok(Math.abs(both.post("cp2").remaining() - (12 * (1 - both.post("cp2").neutralize)) / 2) < 1e-9, "1 + 1, not the rate of 2 (1.5)");
  run(both.posts, [soldier(1, 300), soldier(2, 300), soldier(3, 300)], 60);
  assert.equal(both.post("ts1").team, 3, "a post without a capture region never changes hands");
});

test("campfires and drops aren't on the maps and have no hologram; locals' posts are yellow", () => {
  const { posts, post } = battlefield(MISSIONS.tusken, { headless: false });
  assert.deepEqual(posts.posts.map((p) => p.mapped), [true, true, true, false]);
  assert.deepEqual(posts.holograms.map((h) => h.post.name), ["cp1", "cp2", "cp3"]);
  assert.equal(posts.relation(3), "locals");
  assert.equal(posts.look(2, 3), "locals");
  posts.present(1 / 60, new THREE.PerspectiveCamera());
  const raided = battlefield(MISSIONS.tusken, { headless: false });
  raided.post("cp3").team = 3;
  raided.posts.present(1 / 60, new THREE.PerspectiveCamera());
  const color = raided.posts.holograms[2].color;
  assert.deepEqual([color.r, color.g, color.b].map((v) => Math.round(v * 255)), [240, 220, 30], "the hologram takes LocalsColor");
  assert.equal(post("ts1").mapped, false);
  // An extract without the class fields draws every post, as before.
  const old = new CommandPosts(null, { commandPosts: [{ ...desc("cp9", 1, 0), mapTexture: undefined, mapScale: undefined, holoOdf: undefined }].map((d) => {
    delete d.mapTexture; delete d.mapScale; delete d.holoOdf; return d;
  }) }, { logos: new Map(), textures: {}, playerTeam: 1, headless: true });
  assert.equal(old.posts[0].mapped, true);
});

test("the maps leave the campfires out and draw the locals' posts yellow", () => {
  const { posts, post } = battlefield(MISSIONS.tusken);
  post("cp3").team = 3;
  const quads = [], labels = [];
  const hud = Object.create(BattleHud.prototype);
  Object.assign(hud, {
    frame: { center: [150, 0], half: 300 }, mapImage: "map", mapScale: 1, time: 0,
    player: { position: new THREE.Vector3(150, 0, 0), yaw: 0, team: 1 }, unitSource: [], radar: new RadarUnits(), posts,
    anchor: () => new THREE.Matrix4(), texture: (name) => name,
    batch: { quad: (...args) => quads.push(args), quadCorners() {} }, fonts: { draw: (batch, m, x, y, text) => labels.push(text) },
  });
  const rgb = (quad) => quad[7].slice(0, 3).map((v) => Math.round(v * 255)).join(" ");
  const flags = () => quads.filter((quad) => quad[5] === "hud_flag_icon").map(rgb);
  hud.drawMap();
  assert.deepEqual(flags(), ["32 223 32", "223 32 32", "223 223 32"], "the minimap: CP1 ours, CP2 theirs, CP3 the raiders'; no campfire");
  assert.deepEqual(labels, ["CP1", "CP2", "CP3"]);
  quads.length = labels.length = 0;
  hud.expanded = true;
  hud.drawMap();
  assert.deepEqual(flags(), ["32 223 32", "223 32 32", "223 223 32"]);
  assert.deepEqual(labels, ["CP1", "CP2", "CP3"], "the M map names no campfire");
  quads.length = 0;
  hud.spawnMapPlace = () => ({ x: 0, y: 0, size: 500 });
  hud.drawSpawnMap({ team: 2, rings: [] });
  assert.deepEqual(flags(), ["223 32 32", "32 223 32", "223 223 32"], "the CIS spawn screen's map, as the CIS sees it");
});

const living = (counts) => (team) => counts[team] ?? 0;
function battle(mission, posts, counts = {}) {
  const units = new Units();
  units.setTeams(missionTeams(mission));
  const teams = { 1: { name: "Republic", reinforcements: 100, bleed: [[0, 1]] }, 2: { name: "CIS", reinforcements: 100 }, 3: { name: "locals" } };
  return new Battle({ teams, posts: { posts, owned: (team) => posts.filter((p) => p.team === team) }, own: 1, foe: 2,
    relation: (a, b) => units.relation(a, b), living: living(counts) });
}
const held = (team, valueBleed = 10) => ({ team, desc: { valueBleed } });

test("locals keep coming: no bleed, no countdown, unlimited reinforcements", () => {
  const b = battle(MISSIONS.jawa, [held(1), held(2)]);
  for (let t = 0; t < 30 * 60; t++) b.update(1 / 60);
  assert.equal(b.reinforcements[3], 2147483647, "holding no post, the Jawas still have every reinforcement after 30 s");
  assert.equal(b.over, false);
});

test("a local team friendly with one army runs out with it", () => {
  const ewoks = battle(MISSIONS.ewok, [held(1), held(2)], { 1: 12 });
  ewoks.reinforcements[1] = 13;
  ewoks.update(1 / 60);
  assert.equal(ewoks.reinforcements[3], 2147483647);
  ewoks.reinforcements[1] = 12;
  ewoks.update(1 / 60);
  assert.equal(ewoks.reinforcements[3], 0, "the Rebels' reinforcements are down to their living: no more Ewoks");
  assert.equal(ewoks.start[3], 2147483647, "their starting count stays");
  const jawas = battle(MISSIONS.jawa, [held(1), held(2)], { 1: 12 });
  jawas.reinforcements[1] = 0;
  jawas.update(1 / 60);
  assert.equal(jawas.reinforcements[3], 2147483647, "friends of both armies follow neither");
});

test("an army whose posts are worth nothing counts down; a neutral post pauses it", () => {
  // Hoth's probe droid drop: the Empire's, worth nothing.
  const b = battle(MISSIONS.probe, [held(1, 0), held(2), held(2)]);
  for (let t = 0; t < 21 * 60; t++) b.update(1 / 60);
  assert.equal(b.reinforcements[1], 0);
  assert.equal(b.loser, 1);
  const paused = battle(MISSIONS.probe, [held(1, 0), held(2), held(0)]);
  for (let t = 0; t < 21 * 60; t++) paused.update(1 / 60);
  assert.equal(paused.over, false);
  assert.equal(paused.noPost[1], 20);
});

// A battle under Node with the extract: the locals are fielded, fight, and
// keep coming. Needs an extract with Tatooine's locals (sides/des).
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, "public");
const desSide = (() => {
  try { return Object.keys(JSON.parse(fs.readFileSync(path.join(CONTENT, "sides/des/units.json"), "utf8")).units).length > 0; } catch { return false; }
})();
const tat1 = fs.existsSync(path.join(CONTENT, "maps/tat1/map.json"));
test("Dune Sea: the Tusken Raiders spawn at their camp and fight both armies", { skip: !(desSide && tat1) && "no tat1 extract with sides/des" }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import("../tools/headless-world.mjs");
  const game = await headlessGame("map=tat1&side=rep&ai=classic&seed=1");
  await game.ready;
  const deaths = [];
  game.simEvents.listen((e) => e.kind === "death" && deaths.push([e.attackerTeam, e.team]));
  const raiders = () => game.units.all().filter((u) => u.soldier && u.alive && u.team === 3);
  for (let i = 0; i < 40 * 60; i++) {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
  }
  // Many of the 14 are up (how many have fallen by then is the battle's:
  // the walkers' guns holding fire past their aimers' limits changed it, and
  // the bots' objective offsets spanning a cylinder's built radius did again:
  // 6 to 9 across seeds 1 to 5).
  assert.ok(raiders().length >= 6, `${raiders().length} raiders alive at 40 s`);
  assert.ok(raiders().every((u) => u.character?.name === "TUSKEN RAIDER"));
  assert.deepEqual([game.units.relation(1, 3), game.units.relation(2, 3), game.units.captureTeam(3)], [-1, -1, 3]);
  assert.equal(game.posts.relation(3), "locals");
  assert.ok(deaths.some(([killer, victim]) => killer === 3 || victim === 3), "they fight");
  assert.equal(game.battle.reinforcements[3], 2147483647 - deaths.filter(([, victim]) => victim === 3).length, "each death spends one of their unlimited reinforcements");
  const campfires = game.posts.posts.filter((p) => p.name.startsWith("ts"));
  assert.ok(campfires.every((p) => !p.capture && p.team === 3));
  if (campfires.some((p) => "mapTexture" in p.desc)) assert.ok(campfires.every((p) => p.mapped === false), "the campfires aren't drawn");
});
