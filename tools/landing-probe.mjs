// Landing and taking off again across a battle's map, through the real
// command path in the headless world (tools/headless-world.mjs). For each
// point of a grid over the battlefield the player's craft is put in level
// flight at its slowest, `height` m over what lies below, Space is pressed,
// and once it is down, Space again. Prints a tally of the outcomes: whether
// TO LAND showed, how the landing went (or why it was refused, or stayed on
// its way down) and how the takeoff went. --json writes every point.
//
//   BFE_CONTENT_DIR=<game data> node tools/landing-probe.mjs [map] [era] [craft] [spacing] [height] [--yaw rad] [--json <file>]
//   node tools/landing-probe.mjs tat1 gcw all_fly_xwing 60 20
import fs from 'node:fs';
import { headlessGame } from './headless-world.mjs';

const args = process.argv.slice(2);
const option = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
const jsonOut = option('--json'), yaw = Number(option('--yaw') ?? 0);
const [map = 'tat1', era = 'gcw', craft = 'all_fly_xwing', spacing = '60', height = '20'] = args;
const units = { gcw: 'all_inf_pilot', cw: 'rep_inf_clone_pilot' };
const game = await headlessGame(`map=${map}&era=${era}&unit=${units[era] || units.gcw}&ai=off&seed=1`);
await game.ready; game.begin?.();
const T = game.THREE;
let sequence = 0;
const step = (down = [], actions = []) => {
  const tick = game.worldClock.tick + 1;
  sequence = Math.max(sequence + 1, tick);
  game.step({ epoch: 1, sequence, tick, generation: game.playerUnit.generation, down, actions, look: [] });
};
if (game.combat.dead || game.playerUnit.removed) { game.respawn(0); game.combat.reset(); }
step();
const v = game.vehicles.vehicles.find((x) => x.name === craft && x.alive);
if (!v) throw new Error(`no ${craft} in this battle`);
// The probe's craft never dies: what it hits is noted, not fatal.
let hit = false;
v.damage = (amount) => { if (amount > 0) hit = true; };
const env = v.system.flightEnvironment, b = env.bounds;
const DOWN = new T.Vector3(0, -1, 0), ray = { point: new T.Vector3(), normal: new T.Vector3() };
const seat = () => {
  if (game.driver.vehicle === v) return true;
  if (game.driver.vehicle) game.driver.exit(true);
  if (game.combat.dead) { game.respawn(0); game.combat.reset(); }
  return game.driver.enter(v);
};
// Steps until done() or n ticks: the states gone through and the warnings.
const follow = (n, done) => {
  const states = [v.flightState], warnings = new Set();
  for (let i = 0; i < n && !done(); i++) {
    step();
    if (states.at(-1) !== v.flightState) states.push(v.flightState);
    if (v.flightWarning) warnings.add(v.flightWarning);
  }
  return { states: states.join('>'), warnings: [...warnings].join('|') };
};
const rows = [];
for (let x = b.min[0] + spacing / 2; x < b.max[0]; x += Number(spacing)) for (let z = b.min[2] + spacing / 2; z < b.max[2]; z += Number(spacing)) {
  if (!seat()) throw new Error('cannot board the craft');
  const under = game.collision.cast(new T.Vector3(x, (env.ceiling ?? 300) - 1, z), DOWN, 2000, ray, v.body);
  // Over the battlefield only: ten seconds outside it destroy the craft.
  if (!under || game.battlefield?.outside?.(new T.Vector3(x, ray.point.y + 2, z))) continue;
  const y = ray.point.y + Number(height) - v.bounds.min.y;
  if (y > (env.ceiling ?? Infinity)) continue;
  v.flightState = 'Flying'; v.stateTime = 0; v.trick = ''; v.gear = 0; v.flightActionHeld = false; v.landingMessageTime = 0;
  v.position.set(x, y, z); v.quaternion.setFromAxisAngle(new T.Vector3(0, 1, 0), yaw);
  v.airspeed = Math.max(0, v.flightConfig.minSpeed); v.velocity.set(0, 0, v.airspeed).applyQuaternion(v.quaternion);
  v.yawRate = v.tiltRate = v.flightRollRate = v.bank = 0;
  v.syncBody(); step();
  const row = { x: Math.round(x), z: Math.round(z), ground: +ray.point.y.toFixed(1), prompt: v.landingReason === 'Press SPACE to land' };
  hit = false;
  step(['Space'], ['takeoff']);
  if (v.flightState === 'Flying') { row.landing = `refused (${v.flightWarning})`; rows.push(row); continue; }
  const landing = follow(60 * 60, () => v.flightState === 'Landed');
  row.landing = landing.states + (landing.warnings ? ` (${landing.warnings})` : '');
  if (v.flightState === 'Landed') {
    for (let i = 0; i < 10; i++) step();
    step(['Space'], ['takeoff']);
    const takeoff = follow(10 * 60, () => v.flightState === 'Flying');
    row.takeoff = takeoff.states + (takeoff.warnings ? ` (${takeoff.warnings})` : '');
  }
  row.hit = hit;
  rows.push(row);
}
const tally = new Map();
for (const r of rows) {
  const key = `${r.prompt ? 'TO LAND' : 'no prompt'} | land ${r.landing}${r.takeoff ? ` | take off ${r.takeoff}` : ''}${r.hit ? ' | hit something' : ''}`;
  tally.set(key, (tally.get(key) || 0) + 1);
}
console.log(JSON.stringify({ map, era, craft, spacing: Number(spacing), height: Number(height), yaw, points: rows.length }));
for (const [key, n] of [...tally].sort((a, b) => b[1] - a[1])) console.log(String(n).padStart(5), key);
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(rows, null, 1));
process.exit(0);
