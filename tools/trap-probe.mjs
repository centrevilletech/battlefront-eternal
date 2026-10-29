// Endor's Ewok log traps, headless (src/traps.js, docs/live-objects.md
// "Ewok log traps").
//
//   node tools/trap-probe.mjs [--trap 0]          a walk-in check
//   node tools/trap-probe.mjs --battle [--minutes 10] [--seeds 1,2]
//
// The walk-in check builds a trap with the Rebel pilot's fusion cutter (its
// BuildingBuild ticks through LiveObjects.repair), waits for it to be
// hauled up, walks an Imperial AT-ST through it at walking pace, and checks
// that it swings, that the walker is destroyed DeathTime into the swing,
// that the trap is unbuilt again when the swing has played, and that it can
// be built again. --battle runs Classic AI battles and reports each trap's
// builds (and by whom), swings and what they crushed.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const { headlessGame, content } = await import('./headless-world.mjs');

function stepper(game) {
  return () => {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
  };
}

function cutter() {
  const weapons = JSON.parse(fs.readFileSync(path.join(content, 'sides', 'all', 'weapons.json')));
  const find = (o) => {
    if (!o || typeof o !== 'object') return null;
    if (o.BuildingBuild !== undefined && o.TargetBuildingUnbuilt !== undefined || o.BuildingBuild !== undefined && /fusion/i.test(o.name || o.class || '')) return o;
    for (const v of Object.values(o)) { const f = find(v); if (f) return f; }
    return null;
  };
  return find(weapons) || { BuildingBuild: 30, TargetFriendly: 1, TargetNeutral: 1, TargetEnemy: 0 };
}

async function walkIn() {
  const game = await headlessGame('map=end1&side=all&ai=off&seed=1');
  await game.ready;
  game.begin?.();
  const step = stepper(game);
  const live = game.live;
  const trap = live.traps[Number(opt('trap', 0))];
  if (!trap) throw new Error('no trap on end1 (prepare maps generation 12)');
  const report = { trap: live.traps.indexOf(trap), team: trap.team, at: trap.origin.toArray(), pad: trap.position.toArray().map((n) => +n.toFixed(2)).map((n) => +n.toFixed(2)), state: trap.state };
  // Build it as a Rebel pilot does: the fusion cutter's ticks, twice a second.
  const def = cutter();
  let ticks = 0;
  while (trap.state !== 'built' && ticks < 400) {
    if (!live.repair(def, trap, 1)) throw new Error(`the Rebels' cutter cannot build it (${trap.healthType}, team ${trap.team})`);
    ticks++;
    for (let i = 0; i < 30; i++) step();
  }
  report.buildTicks = ticks;
  report.imperialCanBuild = live.repairs(def, { ...trap, healthType: 'build', health: 0, maxHealth: 1000, team: trap.team }, 2);
  let n = 0;
  while (trap.phase !== 'armed' && n++ < 1200) step();
  report.armedAfter = +(n / 60).toFixed(2);
  // An Imperial AT-ST 40 m out, walking at 3 m/s through the trap's middle.
  const walker = game.vehicles.vehicles.find((v) => v.alive && /atst/.test(v.name) && v.team === 2)
    || game.vehicles.vehicles.find((v) => /atst/.test(v.name));
  if (!walker.alive) walker.spawn(2);
  const dir = new THREE.Vector3(1, 0, 0.3).normalize();
  const from = trap.point.clone().addScaledVector(dir, -40);
  walker.position.copy(from);
  walker.yaw = Math.atan2(dir.x, dir.z);
  walker.syncBody?.();
  let triggeredAt = null, killedAt = null, unbuiltAt = null, distanceAtTrigger = null;
  for (let tick = 0; tick < 60 * 30 && unbuiltAt === null; tick++) {
    if (walker.alive) {
      walker.position.addScaledVector(dir, 3 / 60);
      walker.position.y = from.y;
      walker.syncBody?.();
    }
    step();
    if (triggeredAt === null && trap.phase === 'swing') {
      triggeredAt = tick;
      distanceAtTrigger = +Math.hypot(walker.position.x - trap.point.x, walker.position.z - trap.point.z).toFixed(2);
    }
    if (killedAt === null && !walker.alive) killedAt = tick;
    if (triggeredAt !== null && unbuiltAt === null && trap.state === 'unbuilt') unbuiltAt = tick;
  }
  Object.assign(report, {
    triggered: triggeredAt !== null, distanceAtTrigger, walkerDestroyed: !walker.alive, walkerDying: walker.dying,
    crushSecondsIntoSwing: killedAt !== null && triggeredAt !== null ? +((killedAt - triggeredAt + 1) / 60).toFixed(3) : null,
    unbuiltSecondsAfterTrigger: unbuiltAt !== null ? +((unbuiltAt - triggeredAt + 1) / 60).toFixed(3) : null,
    crushed: trap.crushed, stateAfter: trap.state, healthAfter: trap.health, padShown: trap.pad?.root.visible ?? null,
  });
  ticks = 0;
  while (trap.state !== 'built' && ticks < 400) { live.repair(def, trap, 1); ticks++; for (let i = 0; i < 30; i++) step(); }
  report.rebuilt = trap.state === 'built';
  report.phaseAfterRebuild = trap.phase;
  return report;
}

async function battle(seed, minutes) {
  const game = await headlessGame(`map=end1&side=all&ai=classic&seed=${seed}`);
  await game.ready;
  game.begin?.();
  const step = stepper(game);
  const traps = game.live.traps;
  const out = traps.map(() => ({ builds: 0, swings: 0, crushed: 0, builtBy: {} }));
  const was = traps.map((t) => [t.state, t.phase]);
  const repair = game.live.repair.bind(game.live);
  game.live.repair = (def, building, team) => {
    const i = traps.indexOf(building);
    const before = building?.state;
    const done = repair(def, building, team);
    if (i >= 0 && done && before === 'unbuilt' && building.state === 'built') out[i].builtBy[team] = (out[i].builtBy[team] ?? 0) + 1;
    return done;
  };
  for (let tick = 0; tick < minutes * 3600; tick++) {
    step();
    traps.forEach((t, i) => {
      if (t.state === 'built' && was[i][0] !== 'built') out[i].builds++;
      if (t.phase === 'swing' && was[i][1] !== 'swing') out[i].swings++;
      out[i].crushed = t.crushed;
      was[i] = [t.state, t.phase];
    });
    if (game.battle?.over) break;
  }
  return { seed, minutes: +(game.worldClock.tick / 3600).toFixed(2), traps: out };
}

if (argv.includes('--battle')) {
  const minutes = Number(opt('minutes', 10));
  for (const seed of String(opt('seeds', '1')).split(',').map(Number)) console.log(JSON.stringify(await battle(seed, minutes)));
} else console.log(JSON.stringify(await walkIn(), null, 1));
process.exit(0);
