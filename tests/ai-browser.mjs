// Battles between bots, headless: both teams spawn, move, fight, die,
// respawn, and take command posts. The world is the one the world worker
// runs (tools/headless-world.mjs: createGame headless under Node), stepped
// at 60 Hz with the player idle, so a run measures behavior, not frame rate.
// Requires the extracted maps and their AI data (tools/zero_ai.py,
// zero_ai_mission.py, zero_ai_classes.py) in public/ or BFE_CONTENT_DIR.
//
//   AI_TEST_MAPS=nab2,bes2,kam1  AI_TEST_SECONDS=120  AI_TEST_MODE=classic
//   AI_TEST_TIER=medium  AI_TEST_SEED=1  AI_TEST_HEROES=1
//
// Reports go to AI_TEST_ARTIFACTS (default /tmp/bf-ai): one JSON per map.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { headlessGame, idleCommand } from "../tools/headless-world.mjs";

const out = process.env.AI_TEST_ARTIFACTS || "/tmp/bf-ai";
const maps = (process.env.AI_TEST_MAPS || "nab2,bes2,kam1").split(",");
const seconds = Number(process.env.AI_TEST_SECONDS || 120);
const mode = process.env.AI_TEST_MODE || "classic";
const tier = process.env.AI_TEST_TIER || "medium";
const seed = Number(process.env.AI_TEST_SEED || 1);
const heroes = process.env.AI_TEST_HEROES === "1";
await fs.mkdir(out, { recursive: true });

// What the game reports as an error or a warning while a battle runs.
const errors = [];
const warnings = [];
const consoleError = console.error.bind(console), consoleWarn = console.warn.bind(console);
console.error = (...args) => { errors.push(args.map(String).join(" ")); consoleError(...args); };
console.warn = (...args) => { warnings.push(args.map(String).join(" ")); consoleWarn(...args); };

let failures = 0;
for (const map of maps) {
  errors.length = warnings.length = 0;
  const t0 = Date.now();
  console.log(`${map}: loading`);
  const game = await headlessGame(`?map=${map}&unit=rep_inf_clone_trooper&ai=${mode}&tier=${tier}&seed=${seed}&heroes=${heroes ? 1 : 0}`);
  await Promise.race([game.ready, new Promise((_, reject) => setTimeout(() => reject(new Error("asset readiness timed out")), 60000))]);
  const loadSeconds = (Date.now() - t0) / 1000;
  console.log(`${map}: ready; advancing ${seconds}s`);
  const dt = 1 / 60;
  const samples = [];
  const reader = game.simEvents.reader();
  const counts = {};
  const posts = game.posts.posts.map((p) => p.team).join("");
  let captures = 0;
  let lastPosts = posts;
  // Per bot life: how far it got from where it spawned.
  const travel = new Map(); // unit -> { start, max, since }
  const lives = [];
  const t = performance.now();
  for (let i = 0; i < seconds * 60; i++) {
    game.step(idleCommand(game));
    reader.read((e) => {
      counts[e.kind] = (counts[e.kind] || 0) + 1;
    });
    for (const unit of game.units.all()) {
      if (unit.human || !unit.soldier || !unit.classDef) continue;
      let rec = travel.get(unit);
      if (unit.alive && (!rec || rec.generation !== unit.generation)) {
        if (rec) lives.push(rec);
        rec = { generation: unit.generation, start: unit.position.clone(), max: 0, born: i * dt, team: unit.team, cls: unit.className, mounted: false };
        travel.set(unit, rec);
      }
      if (unit.alive && rec) {
        rec.max = Math.max(rec.max, unit.position.distanceTo(rec.start));
        rec.mounted ||= Boolean(unit.seat);
      }
    }
    const now = game.posts.posts.map((p) => p.team).join("");
    if (now !== lastPosts) {
      captures += [...now].filter((c, k) => c !== lastPosts[k]).length;
      lastPosts = now;
    }
    if (i % 600 === 0) {
      const alive = {};
      for (const u of game.units.all()) if (u.alive && u.soldier) alive[u.team] = (alive[u.team] || 0) + 1;
      samples.push({ t: Math.round(i * dt), alive, reinforcements: { ...game.battle.reinforcements }, posts: now });
    }
  }
  for (const rec of travel.values()) lives.push(rec);
  const old = lives.filter((l) => seconds - l.born > 20);
  const report = {
    msPerTick: ((performance.now() - t) / (seconds * 60)).toFixed(2),
    events: counts,
    captures,
    postsStart: posts,
    postsEnd: lastPosts,
    samples,
    lives: lives.length,
    stuckLives: old.filter((l) => l.max < 3 && !l.mounted).length,
    oldLives: old.length,
    medianTravel: old.map((l) => l.max).sort((a, b) => a - b)[Math.floor(old.length / 2)] ?? 0,
    over: game.battle.over,
  };
  const result = { map, mode, tier, seed, heroes, seconds, loadSeconds, ...report, errors: errors.slice(0, 20), warnings: warnings.slice(0, 20) };
  await fs.writeFile(`${out}/${map}.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ map, msPerTick: report.msPerTick, events: report.events, captures: report.captures, posts: `${report.postsStart} -> ${report.postsEnd}`, stuck: `${report.stuckLives}/${report.oldLives}`, medianTravel: Number(report.medianTravel).toFixed(1), errors: errors.length }));
  try {
    assert.deepEqual(errors, [], "game errors");
    assert.ok((report.events.spawn || 0) > 0, "bots spawned");
    assert.ok((report.events.shot || 0) > 0, "bots fired");
    if (seconds >= 120) assert.ok((report.events.death || 0) > 0, "someone died");
    if (seconds >= 120) assert.ok(report.stuckLives <= report.oldLives * 0.25, "too many bots never left their spawn");
  } catch (error) {
    failures += 1;
    consoleError(`${map}: ${error.message}`);
  }
}
process.exit(failures ? 1 : 0);
