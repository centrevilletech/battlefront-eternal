#!/usr/bin/env node
// Sniper hit registration against latency, without a browser or a network.
// The authoritative world (createMultiplayerWorld) runs here; the shooter's
// client is modelled from the server's own hit history: at each press it
// sees the target as recorded `round trip + input lead + playout delay`
// ticks before the press runs, aims the rifle's gun origin at that head (or
// chest), and sends the press as EntityInputSender does (its tick, its view
// tick, its aim), leading it by the round's flight. The target strafes 7 m/s, turning every 0.4 to 1 s; the
// shooter stands, or strafes 3 m/s himself (--shooter-moves=1). Prints the
// share of presses that struck, and struck the head, for each round trip.
//
//   node tools/sniper-lag-probe.mjs [--map=geo1] [--shots=40] [--range=60]
//     [--rtt=0,50,80,120,160] [--lead=3] [--delay=6] [--jitter=0]
//     [--shooter-moves=0] [--aim=head|body]
//
// Presentation is taken to equal history (the page draws the remote body
// where the server recorded it at the view tick): this measures the
// server's lag compensation, not interpolation error.
import { headlessGame } from './headless-world.mjs';

const arg = (name, fallback) => { const found = process.argv.find((value) => value.startsWith(`--${name}=`)); return found === undefined ? fallback : found.slice(name.length + 3); };
const map = arg('map', 'geo1'), shots = Number(arg('shots', 40)), range = Number(arg('range', 60));
const rtts = arg('rtt', '0,50,80,120,160').split(',').map(Number);
const lead = Number(arg('lead', 3)), delay = Number(arg('delay', 6)), jitter = Number(arg('jitter', 0));
const shooterMoves = arg('shooter-moves', '0') === '1', aimAt = arg('aim', 'head');

const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
const game = await headlessGame(`?map=${map}&side=rep&ai=classic&seed=1`);
const world = createMultiplayerWorld(game, { maxPlayers: 4, armySize: 1 });
const T = game.THREE ?? (await import('three'));
await world.addPlayer('shooter', { team: 1, className: 'rep_inf_clone_sharpshooter', reliableFire: true });
await world.addPlayer('target', { team: 2, reliableFire: true });
const shooter = game.units.byId(world.player('shooter').unitId), target = game.units.byId(world.player('target').unitId);
const history = game.ordnance.hitHistory;

// Open, level ground: a line `range` m long from the shooter's spawn.
const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3() }, down = new T.Vector3(0, -1, 0);
const ground = (x, z, y) => { const h = game.collision.cast(new T.Vector3(x, y + 6, z), down, 20, hit); return h ? y + 6 - h.distance : null; };
const base = shooter.position.clone();
let lane = null;
for (let i = 0; i < 72 && !lane; i++) {
  const yaw = i * Math.PI / 36, ok = [0.25, 0.5, 0.75, 1].every((f) => {
    const x = base.x + Math.sin(yaw) * range * f, z = base.z + Math.cos(yaw) * range * f, g = ground(x, z, base.y);
    if (g === null || Math.abs(g - base.y) > 1.2) return false;
    const eye = new T.Vector3(base.x, base.y + 1.7, base.z), to = new T.Vector3(x, g + 1.6, z).sub(eye), length = to.length();
    return !game.collision.cast(eye, to.divideScalar(length), length, hit);
  });
  if (ok) lane = yaw;
}
if (lane === null) throw Error('no open lane');
const ahead = new T.Vector3(Math.sin(lane), 0, Math.cos(lane)), across = new T.Vector3(Math.cos(lane), 0, -Math.sin(lane));
const far = base.clone().addScaledVector(ahead, range);
far.y = ground(far.x, far.z, base.y) ?? base.y;
target.combat.maxHealth = target.combat.health = 1e9;
shooter.combat.maxHealth = shooter.combat.health = 1e9;
let random = 1;
const rand = () => ((random = (random * 1103515245 + 12345) % 2147483648) / 2147483648);

// Who was struck, where: the world's own hit callback.
const struck = [];
const onHit = game.ordnance.onHitUnit;
game.ordnance.onHitUnit = (unit, def, point, owner, part, direction) => { if (owner === shooter && unit === target) struck.push(part); return onHit(unit, def, point, owner, part, direction); };

let edgeId = 0, sequence = 0, strafe = 1, turnAt = 0, shooterSide = 1;
function drive() {
  const tick = game.worldClock.tick;
  if (tick >= turnAt) { strafe = -strafe; turnAt = tick + 24 + Math.floor(rand() * 36); }
  const offset = target.position.clone().sub(far).dot(across);
  const next = Math.max(-4, Math.min(4, offset + strafe * 7 / 60));
  if (Math.abs(next) >= 4) strafe = -Math.sign(next);
  target.soldier.position.copy(far).addScaledVector(across, next);
  target.soldier.position.y = ground(target.position.x, target.position.z, far.y) ?? far.y;
  target.soldier.velocity.set(0, 0, 0); // placed each tick; its own step must not move it again
  target.soldier.aimYaw = lane + Math.PI;
  // A moving shooter strafes on his own keys (input below), turning about
  // every 0.75 s.
  if (shooterMoves && tick % 45 === 0) shooterSide = -shooterSide;
}
function input(id, unit, view, down = []) {
  world.input(id, { epoch: 1, sequence: ++sequence, tick: game.worldClock.tick + lead, viewTick: view, generation: unit.generation, down, actions: [], look: [] });
}
function step(view = null) {
  drive();
  input('shooter', shooter, view ?? Math.max(0, game.worldClock.tick - delay), shooterMoves ? [shooterSide > 0 ? 'KeyD' : 'KeyA'] : []);
  input('target', target, game.worldClock.tick);
  world.step();
}

for (let n = 0; n < 90; n++) step();
const results = [];
for (const rtt of rtts) {
  const rttTicks = Math.round(rtt * 60 / 1000);
  let fired = 0, any = 0, heads = 0;
  for (let s = 0; s < shots; s++) {
    for (let n = 0; n < 70; n++) step();
    const exec = game.worldClock.tick + 1;
    const view = Math.max(0, exec - (rttTicks + lead + delay + Math.round((rand() * 2 - 1) * jitter)));
    // What his page drew: the target as history has it at his view tick,
    // led by the round's flight (2000 m/s: a tick per 33 m), as a player
    // who leads perfectly aims where it will be when the round gets there.
    const seen = history.sample(view + Math.ceil(range / (2000 / 60)))?.entries.find((e) => e.unit === target);
    if (!seen) continue;
    const head = seen.shapes.find((shape) => shape.part === (aimAt === 'head' ? 'head' : 'body')).center;
    // His page fires from his predicted body (the server's now) along that aim.
    const s0 = shooter.soldier, c = shooter.combat;
    s0.aimYaw = lane; s0.aimPitch = 0;
    for (let i = 0; i < 6; i++) {
      c.aimBasis();
      const dx = head.x - c.origin.x, dy = head.y - c.origin.y, dz = head.z - c.origin.z;
      s0.aimYaw = Math.atan2(dx, dz); s0.aimPitch = Math.atan2(dy, Math.hypot(dx, dz));
    }
    // A full clip every press: ammunition and reloads are not under test.
    const rifle = shooter.combat.arsenal.current(0);
    rifle.reloadLeft = 0; rifle.clip = 1; rifle.reserve = 99;
    const generation = shooter.generation, before = struck.length;
    world.fire('shooter', { epoch: 1, id: ++edgeId, tick: exec, generation, button: 0, pressed: true, viewTick: view, aimYaw: s0.aimYaw, aimPitch: s0.aimPitch });
    world.fire('shooter', { epoch: 1, id: ++edgeId, tick: exec + 1, generation, button: 0, pressed: false, viewTick: view });
    for (let n = 0; n < 12; n++) step(view + n);
    fired++;
    if (struck.length > before) { any++; if (struck.slice(before).includes('head')) heads++; }
  }
  results.push({ rtt, fired, hit: +(any / fired).toFixed(2), head: +(heads / fired).toFixed(2) });
}
console.log(JSON.stringify({ map, range, lead, delay, jitter, shooterMoves, aim: aimAt, results }));
process.exit(0);
