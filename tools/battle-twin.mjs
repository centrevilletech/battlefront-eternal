// The JavaScript side of the battle package's twin gate (V1): random
// world regions and points, destructible posts' kill regions, command
// posts' ticks under shifting crowds, and the end-of-match records under
// kills, deaths and ticks, run through src/regions.js, src/objectives.js,
// src/commandposts.js and src/battle-stats.js. Writes the operations
// (every number as its bit pattern) and the trace rust/sim's battle-twin
// binary must reproduce byte for byte.
//
//   node tools/battle-twin.mjs --seed 7 --out DIR
//   cargo run -p sim --release --bin battle-twin -- DIR/ops.json DIR/rs-trace.txt
//   diff DIR/js-trace.txt DIR/rs-trace.txt
import fs from "node:fs";
import path from "node:path";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import { buildRegion, inRegion } from "../src/regions.js";
import { killRegion } from "../src/objectives.js";
import { CommandPosts } from "../src/commandposts.js";
import { BattleStats } from "../src/battle-stats.js";
import { Character } from "../src/battle.js";
import { ClassicSpawning } from "../src/ai/classic/spawning.js";
import { createMultiplayerRoster } from "../src/runtime/multiplayer-roster.js";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const seed = Number(opt("seed", "1")) >>> 0;
const outDir = opt("out", null);
if (!outDir) throw new Error("battle-twin: --out DIR");
fs.mkdirSync(outDir, { recursive: true });

const random = new ClassicRandom(seed);
const u = () => random.uniform();
const between = (lo, hi) => lo + (hi - lo) * u();
const pick = (list) => list[Math.min(Math.floor(u() * list.length), list.length - 1)];

// Every number crosses as its bit pattern.
const view = new DataView(new ArrayBuffer(8));
const bits = (x) => { view.setFloat64(0, x); return view.getBigUint64(0).toString(16).padStart(16, "0"); };
const trace = [];
const line = (...parts) => trace.push(parts.map((p) => (typeof p === "number" ? bits(p) : typeof p === "boolean" ? (p ? "1" : "0") : p === null || p === undefined ? "-" : String(p))).join(" "));
const n = (x) => bits(x);

// ---------------------------------------------------------------- regions
const TYPES = ["sphere", "box", "cylinder", "other"];
function randomMatrix() {
  const q = new THREE.Quaternion(between(-1, 1), between(-1, 1), between(-1, 1), between(-1, 1)).normalize();
  const s = new THREE.Vector3(between(0.2, 3), between(0.2, 3), between(0.2, 3));
  const p = new THREE.Vector3(between(-100, 100), between(-20, 20), between(-100, 100));
  return new THREE.Matrix4().compose(p, q, s).toArray();
}
function randomRegion() {
  const kind = pick(TYPES);
  const how = u();
  const region = { type: kind, size: [between(0.1, 12), between(0.1, 12), between(0.1, 12)] };
  if (how < 0.6) region.matrix = randomMatrix();
  else if (how < 0.9) region.center = [between(-100, 100), between(-20, 20), between(-100, 100)];
  return region;
}
const regionOp = (r) => ({ type: r.type, matrix: r.matrix ? r.matrix.map(n) : null, center: r.center ? r.center.map(n) : null, size: r.size.map(n) });
function nearPoint(built) {
  const c = built.center;
  const reach = Math.max(built.radius, ...built.half) * 1.5 + 1;
  return new THREE.Vector3(c.x + between(-reach, reach), c.y + between(-reach, reach), c.z + between(-reach, reach));
}

const regions = [];
for (let i = 0; i < 400; i++) {
  const region = randomRegion();
  const built = buildRegion(region);
  const points = Array.from({ length: 16 }, () => nearPoint(built));
  regions.push({ region: regionOp(region), points: points.map((p) => [n(p.x), n(p.y), n(p.z)]) });
  line("region", i, built.type, ...built.half, built.radius, built.center.x, built.center.y, built.center.z, ...built.inverse.elements);
  points.forEach((p, j) => line("in", i, j, inRegion(built, p)));
}

// ---------------------------------------------------------------- kill regions
const kills = [];
for (let k = 0; k < 60; k++) {
  const region = randomRegion();
  if (region.type === "other") region.type = "box";
  const built = buildRegion(region);
  const victims = Array.from({ length: 80 }, () => nearPoint(built));
  const draws = [];
  const effects = [];
  const killed = [];
  const count = killRegion(region, {
    random: () => { const v = u(); draws.push(v); return v; },
    play: (name, point) => effects.push([name, point]),
    victims: () => victims.map((position, i) => ({ position, kill: () => killed.push(i) })),
  });
  kills.push({ region: regionOp(region), draws: draws.map(n), victims: victims.map((p) => [n(p.x), n(p.y), n(p.z)]) });
  effects.forEach(([name, p], e) => line("blast", k, e, name, p.x, p.y, p.z));
  line("killed", k, count, ...killed);
}

// ---------------------------------------------------------------- command posts
const postRuns = [];
for (let r = 0; r < 40; r++) {
  const descs = Array.from({ length: 1 + Math.floor(u() * 4) }, (_, i) => {
    const capture = u() < 0.85 ? randomRegion() : null;
    if (capture && capture.type === "other") capture.type = "cylinder";
    return { name: `cp${i}`, team: pick([0, 0, 1, 2, 3]), matrix: new THREE.Matrix4().toArray(), hologram: [0, 0, 0], capture };
  });
  const rules = { neutralizeTime: between(4, 20), captureTime: between(4, 20), holoTurnOnTime: 2 };
  const capture = Array.from({ length: 8 }, (_, t) => (t === 0 ? 0 : pick([t, t, 0, 1, 2])));
  const posts = new CommandPosts(null, { commandPost: rules, commandPosts: descs, mission: { teams: {} } }, {
    logos: new Map(), textures: {}, playerTeam: 1, settings: null, headless: true,
    teams: { captureTeam: (team) => capture[team] ?? team, relation: () => 0, locals: () => false },
  });
  const ticks = [];
  for (let t = 0; t < 150; t++) {
    const dt = pick([1 / 60, 1 / 60, 1 / 30, between(0, 0.2)]);
    const soldiers = [];
    for (const post of posts.posts) {
      if (!post.capture) continue;
      for (let s = Math.floor(u() * 5); s > 0; s--) soldiers.push({ team: pick([1, 2, 3, 4, 5]), position: nearPoint(post.capture) });
    }
    posts.update(dt, soldiers);
    ticks.push({ dt: n(dt), soldiers: soldiers.map((s) => ({ team: s.team, position: [n(s.position.x), n(s.position.y), n(s.position.z)] })) });
    posts.posts.forEach((post, i) => line("post", r, t, i, post.team, post.neutralize, post.progress, post.toward, post.contested, post.remaining(), ...[...post.counts].flat()));
    for (const e of posts.events.splice(0)) line("event", r, t, posts.posts.indexOf(e.post), e.kind, e.from ?? null, e.by ?? null);
  }
  postRuns.push({ posts: descs.map((d) => ({ team: d.team, capture: d.capture ? regionOp(d.capture) : null })), rules: { neutralizeTime: n(rules.neutralizeTime), captureTime: n(rules.captureTime) }, capture, ticks });
}

// ---------------------------------------------------------------- records
const statRuns = [];
const KEYS = ["rifle", "pistol", "launcher", "", null];
for (let r = 0; r < 10; r++) {
  const characters = Array.from({ length: 8 }, (_, i) => ({ index: i, team: i < 4 ? 1 : 2, className: u() < 0.8 ? pick(["a", "b", "c", "d", "e", "f"]) : null, unit: null, record: null, human: false }));
  const stats = new BattleStats();
  const classes = characters.map((c) => c.className);
  const ops = [];
  for (let k = 0; k < 400; k++) {
    const op = pick(["kill", "kill", "death", "head", "vehicle", "tick", "tick", "tick", "class"]);
    if (op === "kill") {
      const killer = u() < 0.9 ? Math.floor(u() * 8) : null, victim = u() < 0.9 ? Math.floor(u() * 8) : null, relation = pick([-1, -1, 0, 1]);
      ops.push({ op, killer, victim, relation });
      stats.kill(killer === null ? null : characters[killer], victim === null ? null : characters[victim], relation);
    } else if (op === "death" || op === "head" || op === "vehicle") {
      const who = u() < 0.9 ? Math.floor(u() * 8) : null;
      ops.push({ op, who });
      const c = who === null ? null : characters[who];
      if (op === "death") stats.death(c); else if (op === "head") stats.headshot(c); else stats.vehicleDestroyed(c);
    } else if (op === "class") {
      const who = Math.floor(u() * 8), name = u() < 0.85 ? pick(["a", "b", "c", "d", "e", "f", "g"]) : null;
      ops.push({ op, who, name });
      characters[who].className = name;
    } else {
      const dt = pick([1 / 60, between(0, 3)]);
      const active = characters.map(() => u() < 0.9);
      const seen = characters.map(() => (u() < 0.15 ? null : {
        position: u() < 0.95 ? new THREE.Vector3(between(-15, 15), between(-2, 2), between(-15, 15)) : null,
        health: u() < 0.8 ? pick([1, 0.05, between(0, 1), 1]) : null,
        weapon: u() < 0.8 ? { key: pick(KEYS), label: pick(["Rifle", null, ""]) } : null,
        vehicle: u() < 0.3 ? { key: pick(["atst", "speeder", null]), label: pick(["AT-ST", null]) } : null,
        classLabel: pick(["Trooper", null, "Sniper"]),
      }));
      ops.push({ op, dt: n(dt), active, seen: seen.map((s) => s && { position: s.position ? [n(s.position.x), n(s.position.y), n(s.position.z)] : null, health: s.health === null ? null : n(s.health), weapon: s.weapon, vehicle: s.vehicle, classLabel: s.classLabel }) });
      stats.tick(dt, characters, (c) => active[c.index], (c) => seen[c.index]);
    }
    if (k % 5 !== 4) continue;
    for (const c of characters) {
      const rec = c.record;
      if (!rec) { line("record", r, k, c.index, "-"); continue; }
      line("record", r, k, c.index, rec.health, rec.low, ...(rec.anchor ? [rec.anchor.x, rec.anchor.y, rec.anchor.z] : ["-"]));
      rec.slots.forEach((s, i) => {
        line("slot", r, k, c.index, i, s.teamKills, s.vehiclesDestroyed, s.spree, s.survivalist, s.lifeKills, s.lifeKillsBest, s.headshots, s.deaths, s.life, s.lifeBest, s.camp, s.campBest, s.escapes);
        line("victims", r, k, c.index, i, ...[...s.victims].flatMap(([v, m]) => [v.index, m]));
        for (const table of ["classes", "weapons", "vehicles"]) line(table, r, k, c.index, i, ...s[table].flatMap((e) => [e.key, e.label, e.time]));
      });
    }
    const ring = stats.ring;
    line("ring", r, k, stats.time, ring.head, ring.tail);
  }
  statRuns.push({ classes, ops });
}

// ---------------------------------------------------------------- rosters
// The spawn manager's match start: the mission's calls, the teams, the
// humans and the bots (named by their teams' generators), padded class
// targets, and each Character's class, post and queue place.
const rosterRuns = [];
const SIDES = ["all", "IMP", "rep", "cis", "neutral", "locals", "", "republic"];
const CLASS_NAMES = ["trooper", "sniper", "engineer", "officer", "pilot", "heavy", "special"];
const arg = () => (u() < 0.85 ? Math.floor(between(0, 30)) : pick(["12", " 7 ", "x", "", "0x10", 3.7]));
for (let r = 0; r < 30; r++) {
  const calls = [];
  for (let t = 1; t <= 4; t++) {
    calls.push({ name: "SetTeamName", args: [t, pick(SIDES)] });
    for (let c = Math.floor(between(0, 7)); c > 0; c--) calls.push({ name: "AddUnitClass", args: [t, pick(CLASS_NAMES), arg()] });
    calls.push({ name: "SetUnitCount", args: [t, Math.floor(between(0, 24))] });
    if (u() < 0.7) calls.push({ name: "SetReinforcementCount", args: [t, Math.floor(between(0, 300))] });
    if (u() < 0.3) calls.push({ name: "SetHeroClass", args: [t, pick(["hero_a", "hero_b"])] });
    if (u() < 0.2) calls.push({ name: "ActivateBonus", args: [t, pick(["hero", "sabotage", "training", "other"])] });
  }
  for (let k = Math.floor(between(0, 5)); k > 0; k--) calls.push({ name: pick(["SetTeamAsFriend", "SetTeamAsEnemy", "SetTeamAsNeutral"]), args: [Math.floor(between(0, 8)), Math.floor(between(0, 8))] });
  if (u() < 0.6) calls.push({ name: "SetSpawnDelay", args: [between(5, 20), between(0.2, 2)] });
  const posts = Array.from({ length: 1 + Math.floor(u() * 6) }, () => {
    const path = () => (u() < 0.7 ? { present: u() < 0.9, points: Array.from({ length: Math.floor(between(0, 6)) }, () => [between(-50, 50), 0, between(-50, 50), between(-3, 3)]) } : null);
    return {
      team: pick([0, 1, 1, 2, 2, 3, 4]), active: u() < 0.9, position: new THREE.Vector3(between(-100, 100), between(-5, 5), between(-100, 100)),
      ai: { spawnPath: path(), allyPath: path(), allyCount: Math.floor(between(0, 4)), classSpawnPoints: u() < 0.2 ? 2 : 0, classSpawnWorld: [], yaw: between(-3, 3) },
      desc: {},
    };
  });
  const humans = Array.from({ length: Math.floor(between(0, 3)) }, (_, i) => ({ team: pick([1, 2, 0, 9]), ...(u() < 0.5 ? { id: 100 + i } : {}), ...(u() < 0.5 ? { name: `P${i}` } : {}) }));
  const names = u() < 0.7 ? { 0: { first: ["ZERO"], last: [] }, 1: { first: ["LUKE", "HAN"], last: ["SOLO", "ONE"] }, 3: { first: [], last: ["CLONE"] }, local: pick(["EWOK", ""]) } : null;
  const heroes = u() < 0.3, unitScale = pick([1, 1, 0.5, 1.5]), rseed = Math.floor(u() * 0xffffffff);
  const characters = [];
  const spawning = new ClassicSpawning({
    unitScale, calls, posts, random: new ClassicRandom(rseed), humans, heroes, names, heroName: (name) => name.toUpperCase(),
    makeCharacter: (slot, team, human) => new Character(slot, team, human), objectives: null, start: true,
  });
  characters.push(...spawning.characters);
  line("delays", r, spawning.A, spawning.B, spawning.Q, spawning.waveCountdown, spawning.random.state);
  spawning.teams.forEach((t) => line("team", r, t.index, t.name, t.faction, t.unitCount, t.tickets, t.heroCount, t.heroClass, t.bonus, t.aggressiveness, ...t.relations, t.capacity,
    ...t.classes.flatMap((c) => [c.name, c.target, c.hero]), "|", ...t.characters.map((c) => characters.indexOf(c))));
  characters.forEach((c, i) => line("character", r, i, c.team, c.human, c.slot, c.managerIndex, c.playerId, c.name, c.classIndex, c.className, c.hero,
    c.spawn.timer, c.spawn.wave, c.spawn.ordinal, c.spawn.post ? posts.indexOf(c.spawn.post) : null, c.spawn.ready));
  for (const [post, q] of spawning.queues) line("queue", r, posts.indexOf(post), ...q);
  // The multiplayer roster on that battle (src/runtime/multiplayer-roster.js):
  // the local player dormant, the armies trimmed and filled, then players
  // joining, leaving and switching teams. Some bots stand for bodies (alive
  // or dead), which a retire takes out of the battle.
  const idx = (c) => spawning.characters.indexOf(c);
  const units = [];
  spawning.characters.forEach((c, i) => {
    if (c.human || u() < 0.5) return;
    const alive = u() < 0.6;
    units.push([i, alive]);
    c.unit = { alive, seat: null, human: false, networkHuman: false, combat: { player: true }, onRemoved: null, remove: () => line("remove", r, idx(c)) };
  });
  const game = {
    battle: { characters: spawning.characters, get human() { return spawning.characters.find((c) => c.human) || null; } },
    ai: { spawning, classic: { attach: (c) => line("attach", r, idx(c)), detach: (c) => line("detach", r, idx(c)) } },
    factory: { release: () => {} },
  };
  const armySize = pick([null, 32, 8, 3, 1]);
  const state = (k) => {
    for (const t of [1, 2]) { const team = spawning.teams[t]; line("rteam", r, k, t, team.capacity, team.unitCount, team.living, ...team.characters.map(idx)); }
    spawning.characters.forEach((c, i) => line("rchar", r, k, i, c.team, c.human, c.playerId, c.name, c.index, c.slot, c.classIndex, c.className, c.hero,
      c.spawn.timer, c.spawn.wave, c.spawn.ordinal, c.spawn.post ? posts.indexOf(c.spawn.post) : null, c.spawn.ready, c.networkDormant, c.unit ? idx(c) : null, c.kills, c.record === null ? "null" : "-"));
    for (const [post, q] of spawning.queues) line("rqueue", r, k, posts.indexOf(post), ...q);
    line("rrandom", r, k, spawning.random.state);
  };
  const roster = createMultiplayerRoster(game, armySize);
  state(-1);
  const ops = [];
  const players = [];
  for (let k = 0; k < 12; k++) {
    const kind = players.length ? pick(["reserve", "reserve", "release", "switch"]) : "reserve";
    if (kind === "reserve") {
      const team = pick([1, 2]), id = u() < 0.5 ? 1000 + k : `net${k}`, name = pick(["Ace", "a-very-long-player-name-over-thirty-two-units", `P${k}`]);
      ops.push({ op: kind, team, id: typeof id === "number" ? { n: n(id) } : { s: id }, name });
      let ready = null;
      try {
        const c = roster.reserve(team, id, name);
        players.push(c);
        // His choice on the spawn screen, most of the time (onHumanReady).
        if (u() < 0.7) {
          const names = spawning.teams[c.team].classes.map((x) => x.name);
          const by = u();
          const choice = by < 0.4 ? { className: pick([...names, "outsider"]) } : by < 0.8 ? { classIndex: pick([-1, 0, 2, 9, 1.7]) } : {};
          const p = u();
          if (p < 0.3) choice.post = null;
          else if (p < 0.7) choice.post = pick(posts);
          ready = { className: choice.className ?? null, classIndex: choice.classIndex === undefined ? null : n(choice.classIndex), post: "post" in choice ? (choice.post ? posts.indexOf(choice.post) : -1) : null };
          spawning.onHumanReady(c, choice);
        }
        line("rop", r, k, kind, idx(c));
      } catch (e) { line("rop", r, k, kind, e.message); }
      ops.at(-1).ready = ready;
    } else if (kind === "release") {
      const c = pick(players);
      players.splice(players.indexOf(c), 1);
      ops.push({ op: kind, who: idx(c) });
      roster.release(c);
      line("rop", r, k, kind, idx(c));
    } else {
      const c = pick(players), target = pick([1, 2]);
      ops.push({ op: kind, who: idx(c), target });
      line("rop", r, k, kind, roster.switchTeam(c, target));
    }
    state(k);
  }
  rosterRuns.push({
    multiplayer: { armySize: armySize === null ? null : n(armySize), units, ops },
    calls: calls.map((c) => ({ name: c.name, args: c.args.map((a) => (typeof a === "number" ? { n: n(a) } : { s: a })) })),
    posts: posts.map((p) => ({ team: p.team, active: p.active, position: [n(p.position.x), n(p.position.y), n(p.position.z)],
      ai: { ...p.ai, yaw: n(p.ai.yaw), spawnPath: p.ai.spawnPath && { present: p.ai.spawnPath.present, points: p.ai.spawnPath.points.map((q) => q.map(n)) },
        allyPath: p.ai.allyPath && { present: p.ai.allyPath.present, points: p.ai.allyPath.points.map((q) => q.map(n)) } } })),
    humans, names, heroes, unitScale: n(unitScale), seed: rseed,
  });
}

fs.writeFileSync(path.join(outDir, "ops.json"), JSON.stringify({ seed, regions, kills, posts: postRuns, stats: statRuns, rosters: rosterRuns }));
fs.writeFileSync(path.join(outDir, "js-trace.txt"), trace.join("\n") + "\n");
console.log(`battle-twin: ${trace.length} trace lines in ${outDir}`);
