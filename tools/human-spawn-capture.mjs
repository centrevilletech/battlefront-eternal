// Exercise the offline player's spawn screen in a real headless battle.
// With --capture-out FILE, record HumanSpawn.update for the native replay;
// without it, print the same fingerprints to prove recording is inert.
// The regular world-hash runner uses a server world, whose local player is
// dormant, so it does not run this phase.
// BFE_CONTENT_DIR=<data> node tools/human-spawn-capture.mjs --map nab2 --side rep
//   --ticks 2400 --capture-out FILE --capture-specs humanSpawn --capture-every 1
import crypto from 'node:crypto';

const args = process.argv.slice(2);
const opt = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const map = opt('map', 'nab2'), side = opt('side', 'rep'), ai = opt('ai', 'classic');
const seed = Number(opt('seed', 1)), ticks = Number(opt('ticks', 2400));
let random = seed >>> 0;
Math.random = () => { random = (Math.imul(random, 1664525) + 1013904223) >>> 0; return random / 4294967296; };
console.log = (...a) => console.error(...a);
const capture = opt('capture-out', null) ? await import('./sim-capture/plugin.mjs') : null;
const { headlessGame, idleCommand } = await import('./headless-world.mjs');
const { worldState } = await import('../src/runtime/world-state.js');
const query = `?map=${map}&side=${side}&seed=${seed}&ai=${ai}&spawnscreen=1&sim=js`;
const game = await headlessGame(query);
capture?.battle({ game, index: 0, options: { map, side, ai, seed, ticks, query, interval: 1, ack: false, view: false, rays: 'js', movement: 'js' } });
const h = game.humanSpawn;
for (let t = 1; t <= ticks; t++) {
  const command = idleCommand(game), within = (t - 1) % 1200;
  const roster = h.world.roster(h.world.team());
  if ([0, 149, 369, 999].includes(within)) command.human = [{ kind: 'spawn', className: roster[within === 149 ? roster.length - 1 : 0], post: null }];
  if ([120, 720].includes(within)) command.human = [{ kind: 'suicide' }];
  if (within === 169) command.human = [{ kind: 'cancel' }];
  if (within === 219) command.human = [{ kind: 'team', team: 3 - h.world.team() }];
  capture?.tick(t);
  game.step(command);
  const digest = crypto.createHash('sha256').update(JSON.stringify(worldState(game))).digest('hex');
  const line = `${t} ${digest}`;
  process.stdout.write(`${line}\n`);
  capture?.ticked(t, line);
}
capture?.finish();
