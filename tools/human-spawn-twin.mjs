// Differential gate for rust/sim/src/battle/human.rs. Record the actual
// HumanSpawn state machine and its synchronous world callbacks, including
// event order, buffered commands, timer boundaries and server summaries.
// node tools/human-spawn-twin.mjs --seed 7 --out DIR
// cargo run --release -p sim --bin human-spawn-twin -- DIR/ops.json DIR/rs.txt
// Compare DIR/js.txt with DIR/rs.txt byte for byte.
import fs from 'node:fs';
import path from 'node:path';
import { HumanSpawn, EVENTS } from '../src/human-spawn.js';
import { ClassicRandom } from '../src/ai/random.js';

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i < 0 ? fallback : args[i + 1]; };
const seed = Number(opt('seed', 1));
const out = opt('out', null);
if (!out) throw Error('human-spawn-twin: --out DIR');
const random = new ClassicRandom(seed);
const pick = a => a[Math.floor(random.uniform() * a.length)];
const buf = new DataView(new ArrayBuffer(8));
const bits = n => { buf.setFloat64(0, n); return buf.getBigUint64(0).toString(16).padStart(16, '0'); };
const number = n => { buf.setBigUint64(0, BigInt(`0x${n}`)); return buf.getFloat64(0); };
const trace = [];
const line = (...parts) => trace.push(parts.map(x => typeof x === 'number' ? bits(x) : typeof x === 'boolean' ? (x ? '1' : '0') : x == null ? '-' : String(x)).join(' '));
const rosters = { 1: ['a', 'b', 'c'], 2: ['d', 'e'] };
const choices = ['a', 'b', 'c', 'd', 'e', 'absent'];
const posts = [null, 'cp1', 'cp2'];
const phase = ['unspawned', 'alive', 'dying', 'awaiting'];
const runs = [];

for (let r = 0; r < 24; r++) {
  const config = { classic: Boolean(r & 1), auto: Boolean(r & 2), className: 'b', post: r & 4 ? 'cp1' : null };
  const ops = [];
  let k = -1;
  const w = { teamNow: 1, deadNow: true, overNow: false, classic: config.classic, present: true, unit: false, timer: 0,
    can: true, accept: true, spawnOk: true };
  let spawn;
  const call = (...a) => line('call', r, k, ...a);
  Object.assign(w, {
    team: () => w.teamNow, dead: () => w.deadNow, over: () => w.overNow,
    roster: team => rosters[team] || [],
    character: () => w.present ? { unit: w.unit, spawn: { timer: w.timer } } : null,
    canSpawn: () => w.can,
    setTeam(team) { call('team', team); const previous = w.teamNow; w.teamNow = team; spawn.teamChanged(team, previous); return true; },
    spawn(className, post) { call('spawn', className, post); if (!w.spawnOk) return false; w.deadNow = false; w.unit = true; spawn.spawned(className, post || 'cp1'); return true; },
    ready(className, post) { call('ready', className, post); return w.accept; },
    kill() { call('kill'); w.deadNow = true; w.unit = false; w.timer = 4; spawn.died(); },
    cancel() { call('cancel'); },
  });
  spawn = new HumanSpawn({ world: w, policy: { className: config.className, auto: config.auto ? { team: 1, className: 'b', post: config.post } : null }, post: config.post });
  const state = s => [s.phase, s.team, s.className, s.classIndex, s.post, s.canSwitchTeams, s.over];
  const event = (prefix, name, d) => {
    if (name === 'awaitingSpawn') line(prefix, r, k, name, ...state(d));
    else if (name === 'spawned') line(prefix, r, k, name, d.team, d.className, d.post);
    else if (name === 'teamChanged') line(prefix, r, k, name, d.team, d.previous);
    else line(prefix, r, k, name, d.team, d.className);
  };
  for (const name of EVENTS) spawn.on(name, d => event('event', name, d));
  const dump = () => {
    line('state', r, k, ...state(spawn.state()), spawn.spawnedOnce, spawn.deadTime, spawn.ended,
      spawn.auto?.team, spawn.auto?.className, spawn.auto?.post, spawn.pending?.className, spawn.pending?.post);
    line('world', r, k, w.teamNow, w.deadNow, w.overNow, w.classic, w.present, w.unit, w.timer, w.can, w.accept, w.spawnOk);
    line('queue', r, k, spawn.queue.length);
    for (const [name, d] of spawn.queue) event('queued', name, d);
  };
  const run = op => {
    k++; ops.push(op);
    if (op.set) {
      Object.assign(w, op.set);
      if (typeof op.set.timer === 'string') w.timer = number(op.set.timer);
    }
    if (op.switchable !== undefined) spawn.canSwitchTeams = op.switchable;
    if (op.kind === 'update') {
      const warn = console.warn;
      console.warn = () => call('rejected', spawn.pending.className, w.teamNow);
      try { spawn.update(number(op.dt), op.commands); } finally { console.warn = warn; }
    } else if (op.kind === 'spawned') {
      w.deadNow = false; w.unit = true;
      spawn.spawned(op.className, op.post);
    } else if (op.kind === 'died') {
      w.deadNow = true; w.unit = false; w.timer = 4;
      spawn.died();
    } else if (op.kind === 'authority') {
      spawn.applyAuthoritative({ ...op.next, deadTime: number(op.next.deadTime) });
    }
    if (op.flush !== false) spawn.flush();
    dump();
  };
  dump();
  // Exercise the exact 240-step respawn boundary in every mode, both
  // with a spawn manager and with its Character absent.
  run({ kind: 'spawned', className: 'b', post: 'cp1', flush: false });
  run({ kind: 'died' });
  for (let i = 0; i < 241; i++) run({ kind: 'update', dt: bits(1 / 60), commands: [], set: { present: false } });
  run({ kind: 'update', dt: bits(1 / 60), commands: [{ kind: 'spawn', className: 'c', post: 'cp2' }], set: { present: true, timer: bits(0) } });
  for (let i = 0; i < 350; i++) {
    const kind = pick(['update', 'update', 'update', 'update', 'died', 'spawned', 'authority']);
    let op;
    if (kind === 'update') {
      const commands = Array.from({ length: pick([0, 1, 2, 4]) }, () => {
        const kind = pick(['team', 'spawn', 'suicide', 'cancel', 'bad']);
        if (kind === 'team') return { kind, team: pick([1, 2, 7, 0, 1.5, 8, '2']) };
        if (kind === 'spawn') return { kind, className: pick([...choices, '', 'x'.repeat(65), '😀'.repeat(33)]), post: pick([...posts, '', 3]) };
        return { kind };
      });
      op = { kind, dt: bits(pick([1 / 60, 0, 0.1, 4, -0, -1])), commands, set: {
        overNow: random.uniform() < 0.15, present: random.uniform() < 0.8,
        timer: bits(pick([0, -1, 4, Number.NaN])), can: random.uniform() < 0.8,
        accept: random.uniform() < 0.6, spawnOk: random.uniform() < 0.8,
      }, switchable: random.uniform() < 0.8 };
    } else if (kind === 'spawned') op = { kind, className: pick(choices), post: pick([...posts, '']) };
    else if (kind === 'died') op = { kind };
    else op = { kind, next: { phase: pick(phase), className: pick([...choices, null, '']), post: pick([...posts, '']),
      spawnedOnce: random.uniform() < 0.6, ended: random.uniform() < 0.2, deadTime: bits(pick([0, 4, 4 - 1e-6, 3.99])),
      auto: random.uniform() < 0.2 ? { team: 1, className: 'a', post: null } : null,
      pending: random.uniform() < 0.2 ? { className: pick(choices), post: pick(posts) } : null } };
    op.flush = random.uniform() < 0.7;
    run(op);
  }
  runs.push({ config, ops });
}
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'ops.json'), JSON.stringify({ seed, runs }));
fs.writeFileSync(path.join(out, 'js.txt'), trace.join('\n') + '\n');
console.log(`human-spawn-twin: ${runs.length} runs, ${trace.length} trace lines`);
