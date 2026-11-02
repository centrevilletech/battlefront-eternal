// Tick-by-tick fingerprints of an offline battle, for proving that a change
// to the world step leaves every tick's outcome as it was. The local server
// (src/runtime/local-server.js, the world worker's) runs a whole battle with
// its player walking, turning and firing on a script, on a fixed clock: wall
// time reads the tick's time, and Math.random a seeded sequence restarted as
// the battle starts (a few gameplay paths still draw it, three draws it for
// every object it makes while loading, and a vehicle's barrel recoil reads
// the clock, so two unseeded runs part after a while). Each tick prints three
// hashes: the replay parity state (src/runtime/world-state.js), a deeper
// digest of every unit, hull and round in full precision, and the snapshot
// bytes the server published. Run it in two checkouts and compare:
//
//   BFE_CONTENT_DIR=<game-data> node tools/world-hash.mjs \
//     [--map nab2] [--side rep] [--seed 1] [--ticks 2400] > before.txt
//   (the same in the other checkout) > after.txt; cmp before.txt after.txt
//
// It also reports the mean world step (the simulation without publishing)
// over the ticks after the first 900, while the battle fills, on stderr.
import crypto from 'node:crypto';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const map = opt('map', 'nab2'), side = opt('side', 'rep'), seed = Number(opt('seed', 1)), ticks = Number(opt('ticks', 2400));
const era = ['rep', 'cis'].includes(side) ? 'cw' : 'gcw';

const wallNow = performance.now.bind(performance);
let now = 1000;
performance.now = () => now;
Date.now = () => 1700000000000 + Math.floor(now);
let draw = 991;
Math.random = () => { draw = (Math.imul(draw, 1664525) + 1013904223) >>> 0; return draw / 4294967296; };

const { headlessGame } = await import('./headless-world.mjs');
const { createLocalServer } = await import('../src/runtime/local-server.js');
const { worldState } = await import('../src/runtime/world-state.js');
const game = await headlessGame(`?map=${map}&side=${side}&era=${era}&ai=classic&seed=${seed}`);
let sent = [];
const server = await createLocalServer(game, { name: 'Player', send: (channel, payload) => sent.push([channel, payload]) });
server.welcome();
sent = [];
const world = server.world, step = world.step.bind(world);
let stepMs = 0, measured = 0;
world.step = () => { const start = wallNow(); try { return step(); } finally { if (game.worldClock.tick > 900) { stepMs += wallNow() - start; measured++; } } };

const xyz = (v) => (v ? [v.x, v.y, v.z] : null);
function digest() {
  const units = game.units.all().map((u) => {
    const s = u.soldier, i = u.intent;
    return [u.id, u.generation, u.alive, u.removed, u.team, xyz(u.position), u.health,
      s ? [xyz(s.velocity), s.posture, s.air, s.aimYaw, s.aimPitch, s.fuel] : null,
      i ? [i.forward, i.right, i.turn, i.pitch, i.buttons?.[0], i.buttons?.[1], xyz(i.aim)] : null,
      u.seat ? [u.seat.vehicle?.name ?? null, u.seat.index] : null];
  });
  const hulls = (game.vehicles?.vehicles || []).map((v) => [v.name, v.alive, v.health, xyz(v.position), xyz(v.velocity), v.yaw, v.tilt, v.roll,
    v.tiltRate, v.rollRate, v.yawRate, v.support, v.sleeping, v.forwardInput, v.rightInput,
    v.seats.map((s) => [s.aimYaw, s.pitch, xyz(s.target), s.yawRate, s.weapons.map((w) => [w.barrel, w.aimers.map((a) => [a.yaw, a.pitch, a.onTarget, a.inLimits, xyz(a.facing)])])])]);
  const o = game.ordnance;
  const rounds = [o.bolts, o.grenades, o.rounds].map((list) => list.map((r) => [xyz(r.position), xyz(r.velocity), r.life]));
  return JSON.stringify({ units, hulls, rounds });
}
const hash = (data) => crypto.createHash('sha1').update(data).digest('hex').slice(0, 16);
const lines = [];
draw = 991;
for (let i = 0; i < ticks; i++) {
  now += 1000 / 60;
  const tick = game.worldClock.tick + 1, phase = i % 240;
  const down = phase < 160 ? ['KeyW'] : phase < 200 ? ['KeyA', 'Mouse0'] : ['KeyD'];
  world.input(server.id, { epoch: 1, sequence: tick, tick, generation: server.actor.unit.generation, down, actions: [],
    look: [[Math.round(12 * Math.sin(i * 0.05)), Math.round(3 * Math.cos(i * 0.07))]] });
  server.step();
  const published = crypto.createHash('sha1');
  for (const [channel, payload] of sent) { published.update(channel); published.update(payload instanceof ArrayBuffer ? new Uint8Array(payload) : String(payload)); }
  sent = [];
  lines.push(`${tick} ${hash(JSON.stringify(worldState(game)))} ${hash(digest())} ${published.digest('hex').slice(0, 16)}`);
  // A worker yields between ticks: prepared classes' spawns settle here.
  await new Promise((resolve) => setImmediate(resolve));
}
process.stdout.write(lines.join('\n') + '\n');
console.error(`${map} ${side} seed ${seed}: ${ticks} ticks, ${game.units.all().filter((u) => u.alive && u.soldier).length} soldiers alive at the end, world step ${(stepMs / Math.max(1, measured)).toFixed(2)} ms mean after tick 900`);
process.exit(0);
