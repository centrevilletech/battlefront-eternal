// How fast Classic AI bots fire, per weapon: whole battles headless (the
// world worker's and the server's code), every round a bot or a seat it
// holds sends off counted by its ordnance class.
//
//   BFE_CONTENT_DIR=<game-data> node tools/fire-pace-probe.mjs \
//     [--maps nab2,tat2] [--seeds 1,2] [--seconds 90] [--tier medium] [--json out.json]
//
// Without BFE_CONTENT_DIR it reads public/, or the desktop app's imported
// game data when public/ has no maps. The player spawns and stays idle.
// Per map and seed it prints the rounds per second over the whole battle,
// then per ordnance class: rounds, fire cycles (a salvo's rounds leave in
// one update and play one FireSound), shooters, the cycles per second a
// soldier fires while its fire request stands (from its controller), the
// median gap between one shooter's cycles, the bursts (cycles under 0.5 s
// apart) and every gap in bins, long ones included. A gap of 12 updates is
// a 0.2 s ShotDelay when the bot's input runs every update; 16 is the same
// weapon on a bot the distance/visibility gate runs every other update
// (docs/ai-combat.md, "Fire pace").
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const maps = opt('maps', 'nab2').split(',');
const seeds = opt('seeds', '1').split(',').map(Number);
const seconds = Number(opt('seconds', 90));
const tier = opt('tier', 'medium');
const jsonOut = opt('json', null);

const desktop = path.join(os.homedir(), '.config/Battlefront Eternal/game-data');
if (!process.env.BFE_CONTENT_DIR && !fs.existsSync(new URL('../public/maps', import.meta.url)) && fs.existsSync(desktop)) process.env.BFE_CONTENT_DIR = desktop;
const { headlessGame, content } = await import('./headless-world.mjs');

// The player's side for single player: a side of team 1.
function sideOf(map) {
  const mission = JSON.parse(fs.readFileSync(path.join(content, 'maps', map, 'map.json'))).mission ?? {};
  const first = mission.teams?.[1]?.classes?.[0]?.[0] ?? mission.teams?.[1]?.classes?.[0];
  const prefix = typeof first === 'string' ? first.split('_')[0] : null;
  return prefix && (mission.sides ?? []).includes(prefix) ? prefix : (mission.sides?.[0] ?? 'rep');
}

const BINS = [0.15, 0.25, 0.35, 0.6, 1.5, 3, Infinity];
const LABELS = ['<.15', '<.25', '<.35', '<.6', '<1.5', '<3', '>=3'];

async function run(map, seed) {
  const game = await headlessGame(`map=${map}&side=${sideOf(map)}&ai=classic&seed=${seed}&tier=${tier}`);
  await game.ready;
  game.begin?.();
  const classes = new Map(); // ordnance class -> stats
  const last = new Map(); // shooter|class -> { tick, run }
  const stat = (cls) => {
    let s = classes.get(cls);
    if (!s) classes.set(cls, (s = { rounds: 0, cycles: 0, gaps: [], bursts: [], shooters: new Set() }));
    return s;
  };
  const shooterOf = (owner) => (owner?.id ?? owner?.lease?.unit?.id ?? '?') + ':' + (owner?.generation ?? owner?.lease?.unit?.generation ?? 0);
  const ordnance = game.ordnance;
  for (const kind of ['bolt', 'round', 'beam', 'throw', 'fatray']) {
    const original = ordnance[kind]?.bind(ordnance);
    if (!original) continue;
    ordnance[kind] = (def, origin, direction, owner, ...rest) => {
      if (owner && owner !== game.playerUnit && !owner.human) {
        const cls = def?.class || '?';
        const s = stat(cls);
        const shooter = shooterOf(owner);
        s.rounds++;
        s.shooters.add(shooter);
        const key = shooter + '|' + cls;
        const tick = game.worldClock.tick;
        const prev = last.get(key);
        if (!prev) {
          s.cycles++;
          last.set(key, { tick, run: 1 });
        } else if (prev.tick !== tick) {
          s.cycles++;
          const gap = (tick - prev.tick) / 60;
          s.gaps.push(gap);
          if (gap < 0.5) prev.run++;
          else {
            s.bursts.push(prev.run);
            prev.run = 1;
          }
          prev.tick = tick;
        }
      }
      return original(def, origin, direction, owner, ...rest);
    };
  }
  // A shooter's fire request, charged to the ordnance of the weapon in its
  // hands (bots on foot; a seat's guns are not charged).
  const requestTicks = new Map(); // ordnance class -> ticks
  let seq = 0;
  for (let i = 0; i < seconds * 60; i++) {
    const tick = game.worldClock.tick + 1;
    seq = Math.max(seq + 1, tick);
    game.step({ epoch: 1, sequence: seq, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    for (const controller of game.ai?.classic?.controllers ?? []) {
      if (!controller.unit?.alive || controller.unit.seat || !(controller.fireA || controller.fireB)) continue;
      const weapon = controller.unit.combat?.arsenal?.current(controller.perception?.fire?.channel ?? 0);
      const cls = weapon?.def?.ordnance?.class;
      if (cls) requestTicks.set(cls, (requestTicks.get(cls) ?? 0) + 1);
    }
  }
  for (const [key, v] of last) classes.get(key.split('|')[1])?.bursts.push(v.run);
  let total = 0;
  for (const s of classes.values()) total += s.rounds;
  const rows = [...classes].sort((a, b) => b[1].rounds - a[1].rounds).map(([cls, s]) => {
    const gaps = s.gaps.slice().sort((a, b) => a - b);
    const histogram = BINS.map(() => 0);
    for (const g of gaps) histogram[BINS.findIndex((b) => g < b)]++;
    const requestSeconds = (requestTicks.get(cls) ?? 0) / 60;
    return {
      ordnance: cls, rounds: s.rounds, cycles: s.cycles, shooters: s.shooters.size,
      cyclesPerRequestSecond: requestSeconds > 0 ? +(s.cycles / requestSeconds).toFixed(2) : null,
      medianGap: gaps.length ? +gaps[gaps.length >> 1].toFixed(3) : null,
      meanBurst: s.bursts.length ? +(s.bursts.reduce((a, b) => a + b, 0) / s.bursts.length).toFixed(1) : null,
      longestBurst: s.bursts.length ? Math.max(...s.bursts) : 0,
      histogram: Object.fromEntries(LABELS.map((label, i) => [label, histogram[i]])),
    };
  });
  return { map, seed, tier, seconds, roundsPerSecond: +(total / seconds).toFixed(1), rows };
}

const results = [];
for (const map of maps) for (const seed of seeds) {
  const r = await run(map, seed);
  results.push(r);
  console.log(`${r.map} seed ${r.seed} ${r.tier}: ${r.roundsPerSecond} bot rounds/s over ${r.seconds} s`);
  for (const row of r.rows) {
    console.log('  ' + row.ordnance.padEnd(38), 'rounds', String(row.rounds).padStart(5), 'cycles', String(row.cycles).padStart(5),
      'shooters', String(row.shooters).padStart(3), 'per request-s', String(row.cyclesPerRequestSecond ?? '-').padStart(5),
      'median gap', String(row.medianGap ?? '-').padStart(6), 'bursts', `${row.meanBurst ?? '-'}/${row.longestBurst}`,
      '|', Object.entries(row.histogram).map(([k, v]) => `${k}:${v}`).join(' '));
  }
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, null, 1));
process.exit(0);
