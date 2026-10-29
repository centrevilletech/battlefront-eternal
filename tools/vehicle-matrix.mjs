// The vehicle matrix: every vehicle class on every imported map's battles
// (each era fields its own: src/mission.js), every
// seat, through the real command path in the headless world (the engine the
// world worker, a dedicated server and a desktop host run; tools/
// headless-world.mjs). Once as the player (single player) and once as a
// network human (src/runtime/multiplayer-world.js): E (use) boards the
// hull, the seat key reaches each seat, the throttle drives a driver's seat
// (a craft takes off first), the mouse turns a gunner's aim, both triggers
// fire, and E gets out, bailing out of a craft in flight. Prints one row per
// path, map, class and seat; exits 1 if any row fails (an exception, not
// boarded, a seat not reached, no control, no way out).
//
//   BFE_CONTENT_DIR=<game data> node tools/vehicle-matrix.mjs [map ...] [--sp | --mp] [--json <file>]
import fs from 'node:fs';
import path from 'node:path';
import { content, headlessGame } from './headless-world.mjs';
import { battlesOf, selectMission } from '../src/mission.js';

const args = process.argv.slice(2);
const option = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
const jsonOut = option('--json');
const only = args.includes('--sp') ? 'sp' : args.includes('--mp') ? 'mp' : null;
const wanted = args.filter((a) => !a.startsWith('--'));
const mapsDir = path.join(content, 'maps');
// Each era's battle of each map that has vehicles: { map, era, unit }, the
// unit being its team 1's first class (the boarder, and the URL's unit). An
// import holding one battle per map has era null.
const battles = [];
for (const map of wanted.length ? wanted : fs.readdirSync(mapsDir).sort()) {
  let world;
  try { world = JSON.parse(fs.readFileSync(path.join(mapsDir, map, 'map.json'), 'utf8')); } catch { continue; }
  const eras = world.missions ? [...new Set(battlesOf(world).filter((b) => b.playable && !b.historical).map((b) => b.era))] : [null];
  for (const era of eras) {
    const played = selectMission(world, era ? { era } : '');
    const file = played.vehicles && path.join(mapsDir, map, played.vehicles);
    if (file && fs.existsSync(file) && JSON.parse(fs.readFileSync(file, 'utf8')).spawns.length) {
      battles.push({ map, era, unit: played.mission.teams[1].classes[0][0] });
    }
  }
}
const battleQuery = ({ map, era, unit }) => `map=${map}${era ? `&era=${era}` : ''}&unit=${unit}`;

const TICK = 1 / 60;
// What a seat does to the hull (src/boarding.js seatControl), inline so the
// tool also runs against an engine from before that module.
const seatControl = (v, index) => (index > 0 ? 'turret' : v.flying ? 'flight' : 'drive');
const PRESS_LIMIT = 8; // seat key presses to reach a seat

// Someone who boards: the player of a single-player battle, or a network
// human on a server's battle. step(extra) is one tick with that command.
async function playerBoarder(battle) {
  const game = await headlessGame(`${battleQuery(battle)}&ai=off&seed=1`);
  await game.ready; game.begin?.();
  let sequence = 0;
  const boarder = {
    path: 'sp', game,
    get unit() { return game.playerUnit; },
    get soldier() { return game.soldier; },
    get alive() { return !game.combat.dead; },
    get seat() { const v = game.driver.vehicle; return v ? { vehicle: v, index: v.seatIndex } : null; },
    step(extra = {}) {
      const tick = game.worldClock.tick + 1;
      sequence = Math.max(sequence + 1, tick);
      game.step({ epoch: 1, sequence, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [], ...extra });
    },
    revive() {
      if (game.combat.dead || game.playerUnit.removed) { game.respawn(0); game.combat.reset(); game.actor.root.visible = true; }
      boarder.step();
    },
    release() { game.driver.release(); },
  };
  boarder.revive();
  return boarder;
}

async function networkBoarder(battle) {
  const game = await headlessGame(`${battleQuery(battle)}&ai=classic&seed=1`);
  await game.ready; game.begin?.();
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const world = createMultiplayerWorld(game, { armySize: 2 });
  // No bots: the battle holds still around the one network human.
  game.ai.update = () => {};
  await world.addPlayer('matrix', { team: 1 });
  const player = world.players.get('matrix');
  let sequence = 0;
  const boarder = {
    path: 'mp', game,
    get unit() { return player.unit; },
    get soldier() { return player.unit.soldier; },
    get alive() { return player.unit.alive; },
    get seat() { const lease = player.unit.seat; return lease?.vehicle ? { vehicle: lease.vehicle, index: lease.index } : null; },
    step(extra = {}) {
      world.input('matrix', { epoch: 1, sequence: ++sequence, tick: game.worldClock.tick + 1, generation: player.unit.generation, down: [], actions: [], look: [], ...extra });
      world.step();
    },
    revive() {
      for (let i = 0; i < 900 && !player.unit.alive; i++) boarder.step({ human: [{ kind: 'spawn', className: player.className, post: null }] });
      boarder.step();
    },
    release() { if (player.unit.seat?.vehicle) game.vehicles.unboard(player.unit, { force: true, placement: false }); },
  };
  return boarder;
}

// The class's hull at its first pad, new, nobody aboard (another class's
// hull on that pad is put away without a blast).
function freshHull(game, slot, name, team) {
  for (const other of slot.variants.values()) if (other.name !== name && other.alive) {
    for (const seat of other.seats) if (seat.lease) game.vehicles.vacate(seat.lease, { placement: false, promote: false, reason: 'removed' });
    other.alive = other.root.visible = other.body.enabled = false;
  }
  const v = game.vehicles.variant(slot, name);
  for (const seat of v.seats) if (seat.lease) game.vehicles.vacate(seat.lease, { placement: false, promote: false, reason: 'removed' });
  slot.vehicle = v;
  v.spawn(team);
  return v;
}

// Stand beside the hull, as a player walks up to it: on a floor near its
// base, with room, in sight of its middle; then face it and press E.
function boardHull(b, v) {
  const { game } = b, T = game.THREE;
  const center = new T.Vector3(); v.sphere(center);
  const hit = { point: new T.Vector3(), normal: new T.Vector3() }, down = new T.Vector3(0, -1, 0), toward = new T.Vector3();
  const base = v.position.y + v.bounds.min.y;
  let spot = null, best = Infinity;
  for (const reach of [0.4, 0.6, 0.9]) for (let k = 0; k < 24; k++) {
    const a = v.yaw + Math.PI / 2 + k * Math.PI / 12;
    const at = new T.Vector3(center.x + Math.sin(a) * (v.radius * reach + 1.5), base + 3, center.z + Math.cos(a) * (v.radius * reach + 1.5));
    const floor = game.collision.cast(at, down, 12, hit, v.body);
    if (!floor || floor.normal.y < .65 || Math.abs(floor.point.y - base) > 4) continue;
    const floorY = floor.point.y;
    at.y = floorY + 1;
    if (game.collision.overlapsSphere(at, .6, v.body)) continue;
    const eye = at.clone(); eye.y += .7;
    toward.subVectors(center, eye);
    const span = toward.length();
    if (span > v.radius + 3.2) continue;
    const seen = game.collision.cast(eye, toward.normalize(), span, hit);
    if (seen && hit.body !== v.body) continue;
    const score = Math.abs(floorY - base) + span * .1;
    if (score < best) { best = score; spot = at.setY(floorY); }
  }
  if (!spot) spot = new T.Vector3(v.position.x + v.radius * 0.6 + 1.5, v.position.y, v.position.z);
  b.soldier.spawn(spot.x, spot.y, spot.z, 0);
  b.unit.presentation?.reset?.();
  for (let i = 0; i < 6; i++) b.step();
  const eye = b.soldier.position.clone(); eye.y += 1.7;
  b.soldier.aimYaw = Math.atan2(center.x - eye.x, center.z - eye.z);
  b.soldier.aimPitch = Math.max(-1.2, Math.min(1.2, Math.atan2(center.y - eye.y, Math.hypot(center.x - eye.x, center.z - eye.z))));
  b.step({ actions: ['use'] });
  return b.seat?.vehicle === v;
}

const groundBelow = (game, p) => {
  const T = game.THREE, hit = { point: new T.Vector3(), normal: new T.Vector3() };
  return game.collision.cast(new T.Vector3(p.x, p.y + .5, p.z), new T.Vector3(0, -1, 0), 400, hit)?.point.y ?? -Infinity;
};

// The matrix's flight off the pad: take off, throttle up, climb gently. The
// lift takes as long as the craft's own takeoff keys make it (a gunship's
// some 4 s, src/flight/model.js), up to 10 s, and leaves it a few metres up
// and slow, flying on level: the climb starts as it ends.
function flyOff(b, v) {
  b.step({ actions: ['takeoff'] });
  const warning = v.flightWarning || undefined;
  for (let i = 0; i < 600 && v.alive && v.flightState === 'TakingOff'; i++) b.step({ down: ['KeyW'] });
  for (let i = 0; i < 150 && v.alive; i++) b.step({ down: ['KeyW'], look: i < 50 ? [[0, 5]] : [] });
  return warning;
}

// A craft's pads: where E and a takeoff get it off the ground (the takeoff
// is the flight model's: src/flight/model.js), and whether the matrix's
// straight climb out clears what lies ahead (a hangar wall does not). The
// seats are tried from the first pad it flies out of. { slot, team, pads,
// blocked, walled }.
function surveyPads(b, name) {
  const { game } = b, pads = [];
  for (const slot of game.vehicles.slots) for (const [team, each] of Object.entries(slot.desc.classes)) if (each === name) pads.push({ slot, team: Number(team) });
  const blocked = [], walled = [];
  let first = null;
  for (const pad of pads) {
    if (!b.alive) b.revive();
    if (b.seat) b.release();
    const v = freshHull(game, pad.slot, name, pad.team);
    if (!boardHull(b, v)) { blocked.push(`${pad.slot.desc.name} (not boarded)`); continue; }
    flyOff(b, v);
    if (v.alive && v.flightState === 'Flying') first ??= pad;
    else if (v.alive) blocked.push(pad.slot.desc.name);
    else walled.push(pad.slot.desc.name);
    if (b.seat) b.release();
  }
  return { ...(first ?? pads[0]), pads: `${pads.length - blocked.length}/${pads.length}`, blocked, walled };
}

function testSeat(b, slot, name, team, index, shots, survey = null) {
  const { game } = b;
  const row = { path: b.path, map: [game.data.world.name, game.data.world.mission?.era].filter(Boolean).join(':'), vehicle: name, seat: index, control: null, pass: false };
  if (survey) {
    row.pads = survey.pads;
    if (survey.blocked.length) row.blockedPads = survey.blocked;
    if (survey.walled.length) row.crashedAhead = survey.walled;
  }
  try {
    if (!b.alive) b.revive();
    if (b.seat) b.release();
    const v = freshHull(game, slot, name, team);
    row.control = seatControl(v, index);
    row.boarded = boardHull(b, v);
    if (!row.boarded) { row.why = 'not boarded'; return row; }
    const start = v.position.clone();
    // A craft flies before anyone moves to a gunner's seat (its guns fire in flight).
    if (v.flying) {
      row.warning = flyOff(b, v);
      row.flight = v.flightState;
    }
    for (let n = 0; n < PRESS_LIMIT && b.seat?.index !== index && b.seat; n++) b.step({ actions: ['nextPrimary'] });
    row.reached = b.seat?.vehicle === v && b.seat.index === index;
    if (!row.reached) { row.why = 'seat not reached'; return row; }
    if (row.control === 'drive') {
      for (let i = 0; i < 120; i++) b.step({ down: ['KeyW'] });
      row.moved = +start.distanceTo(v.position).toFixed(1);
      row.controllable = row.moved > 0.5;
    } else if (row.control === 'flight') {
      row.moved = +start.distanceTo(v.position).toFixed(1);
      row.controllable = (v.flightState === 'Flying' || v.flightState === 'TakingOff') && row.moved > 5;
    } else {
      // Both ways across the arc: the seat may start at one of its ends.
      const seat = v.seats[index], before = [seat.aimYaw, seat.pitch];
      let turned = 0;
      for (let i = 0; i < 30; i++) {
        b.step({ look: [[i < 10 ? 60 : -60, i < 10 ? -30 : 30]] });
        turned = Math.max(turned, Math.abs(seat.aimYaw - before[0]), Math.abs(seat.pitch - before[1]));
      }
      row.aim = +turned.toFixed(3);
      const view = seat.view, fixed = view.yawLimits && view.yawLimits[0] === view.yawLimits[1] && view.pitchLimits[0] === view.pitchLimits[1];
      row.controllable = fixed || row.aim > 0.005;
    }
    const key = `${v.aiTarget.id}:${index}`;
    shots.set(key, [0, 0]);
    for (let i = 0; i < 60; i++) b.step({ down: ['Mouse0'] });
    for (let i = 0; i < 60; i++) b.step({ down: ['Mouse2'] });
    row.shots = shots.get(key);
    row.weapons = v.seats[index].weapons.length;
    const airborne = v.flying && v.flightState !== 'Landed';
    b.step({ actions: ['use'] });
    row.exited = !b.seat;
    if (!row.exited) { row.why = 'no way out'; return row; }
    if (airborne) {
      // Bailed out: in the air beside the craft, at rest; the empty craft comes down.
      const p = b.soldier.position;
      row.bailHeight = +(p.y - groundBelow(game, p)).toFixed(1);
      row.bailClear = +Math.hypot(p.x - v.position.x, p.z - v.position.z).toFixed(1);
      let t = 0;
      for (; t < 1800 && (v.alive || (b.alive && b.soldier.air)); t++) b.step();
      row.craftDown = !v.alive;
      row.craftSeconds = +(t * TICK).toFixed(1);
      row.landed = b.alive ? 'alive' : 'dead';
    }
    row.pass = row.boarded && row.reached && row.controllable && row.exited && (!airborne || row.craftDown);
    if (!row.pass && !row.why) row.why = v.flying && row.flight === 'Landed' ? 'takeoff blocked at every pad' : !row.controllable ? 'no control' : 'craft did not come down';
  } catch (error) {
    row.error = String(error?.stack || error).split('\n').slice(0, 3).join(' | ');
    row.why = 'exception';
  }
  return row;
}

const rows = [];
for (const battle of battles) {
  const map = [battle.map, battle.era].filter(Boolean).join(':');
  for (const kind of only ? [only] : ['sp', 'mp']) {
    const started = performance.now();
    let b;
    try { b = kind === 'sp' ? await playerBoarder(battle) : await networkBoarder(battle); }
    catch (error) { rows.push({ path: kind, map, vehicle: '*', seat: '*', pass: false, why: 'load', error: String(error).slice(0, 200) }); console.log(JSON.stringify(rows.at(-1))); continue; }
    const { game } = b;
    const shots = new Map(), fired = game.vehicles.onFired;
    game.vehicles.onFired = (object, seat, shot, ...rest) => {
      const count = shots.get(`${object.aiTarget.id}:${seat}`);
      if (count) count[Math.max(0, object.seats[seat].weapons.findIndex((w) => w.state === shot.weapon))] += 1;
      fired?.(object, seat, shot, ...rest);
    };
    const done = new Set();
    for (const slot of game.vehicles.slots) for (const [team, name] of Object.entries(slot.desc.classes)) {
      if (done.has(name)) continue;
      done.add(name);
      const def = game.data.vehicles.classes[name], count = def.seats.length;
      let survey = null;
      // An exception here shows again, row by row, in the seat tests.
      if (def.base === 'flyer') try { survey = surveyPads(b, name); } catch { survey = null; }
      for (let index = 0; index < count; index++) {
        const row = testSeat(b, survey?.slot ?? slot, name, survey?.team ?? Number(team), index, shots, survey);
        rows.push(row);
        console.log(JSON.stringify(row));
      }
    }
    console.error(`${kind} ${map}: ${((performance.now() - started) / 1000).toFixed(1)} s`);
  }
}
const failed = rows.filter((r) => !r.pass);
console.log(`\n${rows.length} rows, ${rows.length - failed.length} pass, ${failed.length} fail`);
for (const r of rows) console.log(`${r.pass ? 'PASS' : 'FAIL'} ${r.path} ${r.map} ${r.vehicle} seat ${r.seat} ${r.control ?? ''}${r.why ? ` (${r.why})` : ''}${r.pads ? ` [takeoff pads ${r.pads}${r.blockedPads ? `, blocked ${r.blockedPads.join(' ')}` : ''}]` : ''}`);
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(rows, null, 1));
process.exit(failed.length ? 1 : 0);
