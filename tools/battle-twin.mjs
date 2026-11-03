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

fs.writeFileSync(path.join(outDir, "ops.json"), JSON.stringify({ seed, regions, kills, posts: postRuns, stats: statRuns }));
fs.writeFileSync(path.join(outDir, "js-trace.txt"), trace.join("\n") + "\n");
console.log(`battle-twin: ${trace.length} trace lines in ${outDir}`);
