// Operator-only performance check using locally imported gameplay data.
// BFE_DATA_DIR=public node tools/multiplayer-benchmark.mjs [players] [ticks]
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { deflateSync } from 'node:zlib';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
const players = Number(process.argv[2] || 8), ticks = Number(process.argv[3] || 600);
if (!Number.isInteger(players) || players < 1 || players > 64 || !Number.isInteger(ticks) || ticks < 120 || ticks > 36000) throw Error('Use 1–64 players and 120–36000 ticks');
const root = resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch = async name => {
  const file = resolve(root, `.${name}`);
  if (!file.startsWith(root + sep)) throw Error('Invalid content path');
  try { return new Response(await readFile(file)); }
  catch (error) { if (error.code === 'ENOENT') return new Response('', { status: 404 }); throw error; }
};
const game = await createGame({ headless: true, query: '?map=nab2&side=rep&ai=classic&seed=1' });
// Keep the workload alive for the full measurement rather than benchmarking
// an ended match or a room whose test players all died in the first seconds.
game.battle.reinforcements[1] = game.battle.reinforcements[2] = 1000000;
const world = createMultiplayerWorld(game, { maxPlayers: players,linearProjectiles:process.env.BFE_LINEAR_PROJECTILES==='1' });
for (let i = 0; i < players; i++) await world.addPlayer(`bench-${i}`, {reliableFire:true});
const simulation = [], replication = [], total = [], bytes = Array(players).fill(0);
const livePopulation = [];
const captureDirectory=process.env.BFE_BENCH_CAPTURE_DIR,captured=[];
if(captureDirectory)await mkdir(captureDirectory,{recursive:true});
let baselineBytes = 0;
for (let n = 1; n <= ticks + 120; n++) {
  const started = performance.now();
  for (let i = 0; i < players; i++) {
    const p = world.player(`bench-${i}`);
    const state = world.players.get(`bench-${i}`);
    world.input(`bench-${i}`, { epoch: 1, generation: p.generation, sequence: n, tick: n,
      down: n % 120 < 60 ? ['KeyW', 'Mouse0'] : ['KeyD'], actions: [], look: [[.2, 0]], viewTick:Math.max(0,n-7),
      human:state.spawn.phase==='awaiting'?[{kind:'spawn',className:p.className,post:null}]:[] });
    if (n % 120 === 1 || n % 120 === 61) world.fire(`bench-${i}`, {
      epoch:1, id:n, tick:n, generation:p.generation, button:0, pressed:n%120===1, viewTick:Math.max(0,n-7),
    });
  }
  world.step();
  if (n>120 && n%60===0) livePopulation.push(game.units.all().filter(u=>u.alive&&!u.removed&&u.character).length);
  const simulated = performance.now();
  let sharedId, compressed;
  if (n % 3 === 0) for (let i = 0; i < players; i++) {
    const packet = world.snapshot(`bench-${i}`, {sharedPacket:true});
    if (packet.sharedId !== sharedId) {
      sharedId=packet.sharedId;
      if(captureDirectory)captured.push([n,packet.buffer]);
      compressed=deflateSync(Buffer.from(packet.buffer),{level:3}).toString('base64');
    }
    // Matches the server's shared compressed body and small per-player tail.
    const wire = Buffer.byteLength(JSON.stringify({type:'ticks',encoding:'deflate',buffers:[compressed],metadata:packet.metadata}));
    if (n === 3) baselineBytes += wire;
    if (n > 120) bytes[i] += wire;
  }
  if (n > 120) { simulation.push(simulated - started); replication.push(performance.now() - simulated); total.push(performance.now() - started); }
}
for(const [tick,buffer] of captured)await writeFile(resolve(captureDirectory,`${players}-${tick}.bin`),new Uint8Array(buffer));
const stats = values => { const sorted = [...values].sort((a, b) => a - b); return { mean: values.reduce((a,b) => a+b,0)/values.length, p95: sorted[Math.floor(sorted.length*.95)], p99: sorted[Math.floor(sorted.length*.99)], max: sorted.at(-1) }; };
console.log(JSON.stringify({ players, armies:world.roster.counts(), liveInfantry:game.units.all().filter(u=>u.alive&&!u.removed&&u.character).length, registryBodies: game.units.all().length, ticks, simulationMs: stats(simulation), replicationMs: stats(replication), totalTickMs: stats(total),
  livePopulation:{min:Math.min(...livePopulation),mean:livePopulation.reduce((a,b)=>a+b,0)/livePopulation.length,max:Math.max(...livePopulation)},
  baselineBytesPerClient: baselineBytes / players, wireBytesPerSecond: bytes.map(n => n / (ticks / 60)), rssMiB: process.memoryUsage().rss / 1048576 }, null, 2));
