// Tick-by-tick fingerprints of an offline battle, for proving that a change
// to the world step leaves every tick's outcome as it was. The local server
// (src/runtime/local-server.js, the world worker's) runs a whole battle with
// its player walking, turning and firing on a script, on a fixed clock: wall
// time reads the tick's time, and Math.random a seeded sequence restarted as
// the battle starts. No gameplay draws Math.random (three draws it for the id
// of every object it makes): with --random native, which leaves it to V8,
// the fingerprints are the same. Each tick prints three hashes: the replay
// parity state (src/runtime/world-state.js), a deeper digest of every unit,
// hull and round in full precision, and the snapshot bytes the server
// published. Run it in two checkouts and compare:
//
//   BFE_CONTENT_DIR=<game-data> node tools/world-hash.mjs \
//     [--map nab2] [--side rep] [--seed 1] [--ticks 2400] > before.txt
//   (the same in the other checkout) > after.txt; cmp before.txt after.txt
//
// tools/world-hash-matrix.mjs runs the standing set of battles this way, in
// parallel, and compares them.
//
// It also reports the mean world step (the simulation without publishing)
// over the ticks after the first 900, while the battle fills, on stderr.
//
// Three options bring in what a page does, each off by default (so the
// fingerprints above stay comparable):
//   --interval N  a snapshot every N ticks, as for a page drawing 60/N
//                 frames a second (the worker's local interval);
//   --ack         the player acknowledges snapshots, with a lag that rises
//                 from 2 to 121 ticks and falls back, so snapshots are sent
//                 as deltas against baselines up to 121 ticks old;
//   --view T      he also pulls his trigger (the edges a page sends; a
//                 held key alone fires nothing for him), and his commands
//                 and presses say his page showed the battle T ticks ago
//                 (fractions allowed), so his shots are judged against the
//                 hit history, rewound and interpolated, as online.
//
// More change the battle itself:
//   --ai enhanced the Enhanced AI instead of the Classic (?ai=);
//   --heroes      the heroes join the battle (?heroes=1);
//   --unit CLASS  the player spawns as this class (?unit=), a droideka say;
//   --mount T     from tick T the player mans the nearest turret he may for
//                 480 ticks, then takes the nearest hull's free armed seat
//                 (between steps, as his use key would within one), so a
//                 network gunner's fire paths run (with --view, his shots);
//   --battles N   the battle N times over in one process, each after the
//                 one before (its clock running on, the server put away):
//                 what a module keeps from one battle to the next shows as
//                 a later battle parting from the first. A later battle's
//                 lines carry its number ("2:1 ..." is the second's tick 1).
//                 --twice is --battles 2.
//
// --combat rust runs a world's combats on the simulation's module
// (?combat=rust, src/combat-sim.js); its fingerprints must match the
// default's. --units rust runs the step's loop over the units there too
// (?units=rust, src/units-sim.js), with combat or without; it brings the
// ray kernel, as the soldiers move through the module's collision world.
// Its fingerprints must match the default's.
// Collision, soldier movement, rounds and items always use Rust.
//
// --plugin FILE (any number of times) loads a module that watches the
// battle (the simulation capture of tools/sim-capture.mjs): it is imported
// once the clock and the draws are fixed and before the game's modules, and
// any of its exports battle({ game, server, world, index, options }),
// tick(tick), ticked(tick, line), ended({ game, index }) and finish() is
// called as its name says (all but finish in step with the battle, never
// awaited). A plugin must leave the battle as it is: the fingerprints with
// it and without it are the same.
import crypto from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const map = opt('map', 'nab2'), side = opt('side', 'rep'), seed = Number(opt('seed', 1)), ticks = Number(opt('ticks', 2400));
const interval = Number(opt('interval', 1)), ack = argv.includes('--ack'), view = opt('view', null) === null ? null : Number(opt('view', 0));
const ai = opt('ai', 'classic'), heroes = argv.includes('--heroes'), battles = argv.includes('--twice') ? 2 : Number(opt('battles', 1));
const unit = opt('unit', null), mount = Number(opt('mount', -1));
// --combat rust: a world's combats on the module (?combat=rust, src/combat-sim.js).
const combat = opt('combat', 'js');
const aiSim = opt('ai-sim', 'js');
if (!['rust', 'js'].includes(aiSim)) throw Error(`--ai-sim ${aiSim}: rust or js`);
const battleRules = opt('battle', 'js');
if (!['rust', 'js'].includes(battleRules)) throw Error(`--battle ${battleRules}: rust or js`);
if (!['rust', 'js'].includes(combat)) throw Error(`--combat ${combat}: rust or js`);
// --hulls rust: the ground hulls' phase on the module (?hulls=rust, src/vehicles-sim.js).
const hulls = opt('hulls', 'js');
const f2 = opt('f2', process.env.BFE_F2 ?? 'js');
if (!['js', 'rust'].includes(f2)) throw Error(`--f2 ${f2}: rust or js`);
// --units rust: the units' phase on the module (?units=rust, src/units-sim.js).
const units = opt('units', 'js');
if (!['rust', 'js'].includes(units)) throw Error(`--units ${units}: rust or js`);
// --seats rust: the hulls' seats' input batch on the module (?seats=rust, src/seats-sim.js).
const seats = opt('seats', 'js');
const nativeRandom = opt('random', 'seeded') === 'native';
const era = ['rep', 'cis'].includes(side) ? 'cw' : 'gcw';
if (!['classic', 'enhanced'].includes(ai)) throw Error(`--ai ${ai}: classic or enhanced`);
if (!(Number.isInteger(battles) && battles >= 1)) throw Error(`--battles ${opt('battles')}: a whole number from 1`);

const wallNow = performance.now.bind(performance);
let now = 1000;
performance.now = () => now;
Date.now = () => 1700000000000 + Math.floor(now);
let draw = 991;
if (!nativeRandom) Math.random = () => { draw = (Math.imul(draw, 1664525) + 1013904223) >>> 0; return draw / 4294967296; };

const plugins = [];
for (let i = 0; i < argv.length; i++) if (argv[i] === '--plugin') plugins.push(await import(pathToFileURL(path.resolve(argv[++i])).href));
// Called in step with the battle, never awaited: an await would let other
// work in flight run at another point of the tick than without a plugin.
const call = (name, ...args) => { for (const plugin of plugins) plugin[name]?.(...args); };

const { headlessGame } = await import('./headless-world.mjs');
const { createLocalServer } = await import('../src/runtime/local-server.js');
const { worldState } = await import('../src/runtime/world-state.js');
const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
const query = `?map=${map}&side=${side}&era=${era}&ai=${ai}&aiSim=${aiSim}&seed=${seed}&battle=${battleRules}${heroes ? '&heroes=1' : ''}${unit ? `&unit=${unit}` : ''}${hulls === 'rust' ? '&hulls=rust' : ''}&f2=${f2}${argv.includes('--f2-debug') ? '&f2Debug=1' : ''}${combat === 'rust' ? '&combat=rust' : ''}${seats === 'rust' ? '&seats=rust' : ''}${units === 'rust' ? '&units=rust' : ''}`;
const options = { map, side, era, seed, ticks, interval, ack, view, hulls, f2, combat, seats, units, battle: battleRules, ai, aiSim, heroes, unit, mount, battles, nativeRandom, query };
const xyz = (v) => (v ? [v.x, v.y, v.z] : null);
const hash = (data) => crypto.createHash('sha1').update(data).digest('hex').slice(0, 16);

async function battle(index) {
  const prefix = index ? `${index + 1}:` : '';
  const game = await headlessGame(query);
  if ((aiSim === 'rust') !== Boolean(game.aiSim)) throw Error('AI navigation does not match the requested module');
  if (aiSim === 'rust' && (!game.aiSim.navigation || game.aiSim.navigation.planner.constructor.name !== 'NativePlanner')) throw Error('native AI navigation did not attach');
  if ((battleRules === 'rust') !== Boolean(game.battleSim)) throw Error('battle rules do not match the requested module');
  if ((battleRules === 'rust') !== Boolean(game.battlePhaseSim)) throw Error('battle phase does not match the requested module');
  if (battleRules === 'rust' && !game.worldSim) throw Error('native world phase did not attach');
  if (battleRules === 'rust' && !game.worldSim.ownsEvents(game.simEvents)) throw Error('native event journal did not attach');
  if (battleRules === 'rust' && game.turretSeats && !game.turretSeatsSim && !game.vehicles?.f2?.seats.crews) throw Error('native turret crews did not attach');
  if (battleRules === 'rust' && (!game.postsSim || !game.battlefieldSim || game.ai?.spawning && !game.spawningSim || game.live && !game.propsSim || game.live?.traps.length && !game.trapsSim)) throw Error('a requested native battle phase was not attached');
  // Collision, movement, rounds and items are required on the battle instance.
  if (!game.collision.rays || !game.collision.movement || !game.ordnance.sim || !game.items.sim) throw Error('required simulation kernels did not attach');
  if ((f2 === 'rust') !== Boolean(game.vehicles?.f2)) throw Error('F2 owner did not match the requested switch');
  if ((units === 'rust') !== Boolean(game.unitsSim)) throw Error('unit owner did not match the requested switch');
  let sent = [];
  const server = await createLocalServer(game, { name: 'Player', send: (channel, payload) => sent.push([channel, payload]) });
  server.welcome();
  sent = [];
  if (interval !== 1) server.host.setLocalInterval(interval);
  const world = server.world, step = world.step.bind(world);
  let stepMs = 0, measured = 0;
  world.step = (...args) => { const start = wallNow(); try { return step(...args); } finally { if (game.worldClock.tick > 900) { stepMs += wallNow() - start; measured++; } } };

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
  let fireEdges = 0, mounted = 0, mountedAt = 0;
  const near = (list, p) => list.map((x) => [x, x.position.distanceToSquared(p)]).sort((a, b) => a[1] - b[1]).map(([x]) => x);
  function mountStep(i) {
    const u = server.actor.unit;
    if (!u?.alive || u.removed) return;
    if (mounted === 0) {
      const turret = near((game.turretSeats?.turrets || []).filter((t) => game.turretSeats.canMan(u, t)), u.position)[0];
      if (turret && game.turretSeats.man(u, turret)) { mounted = 1; mountedAt = i; } else mounted = 2;
    } else if (mounted === 1 && i - mountedAt >= 480) {
      if (u.seat?.turret) game.turretSeats.leave(u, { placement: true });
      mounted = 2;
    } else if (mounted === 2 && !u.seat) {
      for (const v of near((game.vehicles?.vehicles || []).filter((v) => v.alive), u.position)) {
        const k = v.seats.findIndex((s, k) => s.weapons.length && v.seatFree(k));
        if (k >= 0 && game.vehicles.board(u, v, k)) { mounted = 3; break; }
      }
    }
  }
  draw = 991;
  call('battle', { game, server, world, index, options });
  for (let i = 0; i < ticks; i++) {
    now += 1000 / 60;
    if (mount >= 0 && i >= mount) mountStep(i);
    const tick = game.worldClock.tick + 1, phase = i % 240;
    call('tick', tick);
    const down = phase < 160 ? ['KeyW'] : phase < 200 ? ['KeyA', 'Mouse0'] : ['KeyD'];
    const generation = server.actor.unit.generation, viewTick = view === null ? null : Math.max(0, tick - view);
    world.input(server.id, { epoch: 1, sequence: tick, tick, generation, down, actions: [],
      look: [[Math.round(12 * Math.sin(i * 0.05)), Math.round(3 * Math.cos(i * 0.07))]], ...(view === null ? {} : { viewTick }) });
    if (view !== null && (phase === 160 || phase === 200)) world.fire(server.id, { epoch: 1, id: ++fireEdges, tick, generation, button: 0, pressed: phase === 160, ...(phase === 160 ? { viewTick } : {}) });
    server.step();
    // The lag rises a tick a tick (the acknowledged tick holds, the baseline
    // ages) and falls two at a time; of the two ticks named, a page has the
    // one that was published.
    if (ack) { const k = i % 238, lag = 2 + (k < 119 ? k : 238 - k); for (const at of [tick - lag - 1, tick - lag]) if (at > 0) server.host.input(server.id, encodeSnapshotAckKeepalive({ epoch: 1, tick: at })); }
    const published = crypto.createHash('sha1');
    for (const [channel, payload] of sent) { published.update(channel); published.update(payload instanceof ArrayBuffer ? new Uint8Array(payload) : String(payload)); }
    sent = [];
    const line = `${tick} ${hash(JSON.stringify(worldState(game)))} ${hash(digest())} ${published.digest('hex').slice(0, 16)}`;
    // Each line as it comes: a battle that dies keeps the ticks before it.
    process.stdout.write(`${prefix}${line}\n`);
    call('ticked', tick, line);
    // A worker yields between ticks: prepared classes' spawns settle here.
    await new Promise((resolve) => setImmediate(resolve));
  }
  call('ended', { game, index });
  console.error(`${map} ${side} seed ${seed}${ai === 'classic' ? '' : ` (${ai} AI)`}${heroes ? ' (heroes)' : ''}${unit ? ` (unit ${unit})` : ''}${mount >= 0 ? ` (mount ${mount}: ${['none', 'turret', 'none', 'hull'][mounted]})` : ''}${battles > 1 ? ` (battle ${index + 1} of ${battles})` : ''}${nativeRandom ? ' (random native)' : ''}: ${ticks} ticks, ${game.units.all().filter((u) => u.alive && u.soldier).length} soldiers alive at the end, world step ${(stepMs / Math.max(1, measured)).toFixed(2)} ms mean after tick 900`);
  server.dispose?.();
}

for (let index = 0; index < battles; index++) await battle(index);
for (const plugin of plugins) await plugin.finish?.();
process.exit(0);
