// Differential gate for the native log-trap state machine. It exercises
// the actual Trap methods, callback order, both sphere paths, and deaths
// that change other hulls while a crush is being dispatched.
// node tools/traps-twin.mjs --seed 7 --out DIR
// cargo run --release -p sim --bin traps-twin -- DIR/ops.json DIR/rs.txt
// Compare js.txt and rs.txt byte for byte.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Trap } from '../src/traps.js';
import { observeSounds } from '../src/audio/events.js';
import { ClassicRandom } from '../src/ai/random.js';

const args = process.argv.slice(2);
const opt = (name, fallback) => { const at = args.indexOf(`--${name}`); return at < 0 ? fallback : args[at + 1]; };
const out = opt('out', null);
const module = opt('module', 'js');
if (!['js', 'rust'].includes(module)) throw Error('--module must be js or rust');
const attach = module === 'rust' ? (await import('../src/traps-sim.js')).attachTrapsSim : null;
const load = attach ? (await import('../src/sim/module.js')).loadSim : null;
if (!out) throw Error('traps-twin: --out DIR');
const random = new ClassicRandom(Number(opt('seed', 1)));
const pick = values => values[Math.floor(random.uniform() * values.length)];
const view = new DataView(new ArrayBuffer(8));
const bits = n => { view.setFloat64(0, n); return view.getBigUint64(0).toString(16).padStart(16, '0'); };
const number = n => { view.setBigUint64(0, BigInt(`0x${n}`)); return view.getFloat64(0); };
const trace = [], runs = [];
const line = (...parts) => trace.push(parts.map(v => typeof v === 'number' ? bits(v) : typeof v === 'boolean' ? (v ? '1' : '0') : v == null ? '-' : String(v)).join(' '));

for (let r = 0; r < 32; r++) {
  const base = r < 8;
  const buildFrames = base ? 141 : pick([null, 0, 1, 21, 141, NaN]);
  const triggerFrames = base ? 161 : pick([null, 0, 1, 30, 161, NaN]);
  const settings = { build: 'reset', trigger: 'trigger', triggerTeam: base ? 'imp' : pick([null, '', 'imp', 'all']),
    radius: base ? 3 : pick([3, 0, -1, Infinity, NaN]), minRadius: base ? 4 : pick([0, -0, 4, NaN, Infinity, -1]),
    deathTime: base ? 0.666 : pick([0, -1, 1 / 60, 0.666, 2, Infinity, NaN]),
    offset: [10, -3.5, 20], deathEffectOffset: [10, 4, 20], sound: r & 1 ? 'spring' : '', deathEffect: r & 2 ? '' : 'dust' };
  const clips = new Map();
  if (buildFrames !== null) clips.set('reset', { frames: buildFrames });
  if (triggerFrames !== null) clips.set('trigger', { frames: triggerFrames });
  const bodies = { built: { enabled: false }, unbuilt: { enabled: true } };
  const rig = { root: new THREE.Group(), bones: [], byName: {} };
  const t = new Trap({ base: 'trap', class: 'trap', model: 'trap', matrix: new THREE.Matrix4().toArray(),
    settings: { MaxHealth: '1000' }, team: 0, trap: settings }, { built: rig, pad: null }, bodies, null, clips);
  if (!base) t.maxHealth = pick([0, -0, 10, 1000, -5, Infinity, NaN]);
  const config = { maxHealth: bits(t.maxHealth), reach: bits(t.reach), minRadius: bits(t.minRadius), deathTime: bits(t.deathTime),
    buildFrames: buildFrames === null ? null : bits(buildFrames), triggerFrames: triggerFrames === null ? null : bits(triggerFrames),
    triggerTeam: settings.triggerTeam, sound: settings.sound, deathEffect: settings.deathEffect,
    point: t.point.toArray().map(bits), effectAt: t.effectAt.toArray().map(bits) };
  const ops = [];
  let k = -1, hulls = [];
  const call = (...parts) => line('call', r, k, ...parts);
  const originalShow = t.show;
  t.show = function (state) { originalShow.call(this, state); call('show', state === 'built'); };
  t.sideOf = team => { call('side', team); return ({ 1: 'all', 2: 'imp', 3: 'ewk' })[team] ?? null; };
  t.onEffect = (name, at) => call('effect', name, ...at.toArray());
  const unobserve = observeSounds((type, name, at) => call(type, name, ...at.toArray()));
  if (attach) attach(await load(), [t]);
  function makeHull(h, i) {
    const v = { id: i, alive: h.alive, team: number(h.team), position: new THREE.Vector3(...h.position.map(number)),
      radius: number(h.radius), _dying: number(h.dying), _attacker: true, _visible: h.root ? true : null,
      setHealth(value) {
        call('health', i, value);
        if (!(value > 0)) { this.alive = false; this._dying = 3; }
        // A death callback can invalidate another selected hull. The
        // filter has already finished, so that hull must still be killed.
        if (h.cascade !== null) { hulls[h.cascade].alive = false; hulls[h.cascade].position.x += 10000; }
      } };
    Object.defineProperty(v, 'lastAttacker', { get: () => v._attacker, set: x => { v._attacker = x; call('attacker', i); } });
    Object.defineProperty(v, 'dying', { get: () => v._dying, set: x => { v._dying = x; call('dying', i, x); } });
    if (h.root) {
      v.root = {};
      Object.defineProperty(v.root, 'visible', { get: () => v._visible, set: x => { v._visible = x; call('visible', i, x); } });
    }
    if (h.sphere) v.blastSphere = at => { call('sphere', i); at.copy(v.position); at.y += number(h.up); return v.radius; };
    return v;
  }
  const dump = () => {
    line('state', r, k, t.state === 'built', t.health, t.phase, t.started, t.time, t.sprung, t.tick, t.crushed, ...t.stateRecord());
    line('bodies', r, k, bodies.built.enabled, bodies.unbuilt.enabled);
    for (let i = 0; i < hulls.length; i++) {
      const h = hulls[i];
      line('hull', r, k, i, h.alive, h._dying, h._attacker, h._visible, ...h.position.toArray());
    }
  };
  const run = op => {
    k++; ops.push(op);
    if (op.kind === 'health') t.setHealth(number(op.value));
    else if (op.kind === 'step') t.step(number(op.dt), number(op.tick), hulls);
    else if (op.kind === 'begin') t.begin(op.phase, op.tick === null ? null : number(op.tick));
    else if (op.kind === 'crush') t.crush(hulls);
    else if (op.kind === 'hulls') hulls = op.values.map(makeHull);
    else if (op.kind === 'catches') for (let i = 0; i < hulls.length; i++) line('catches', r, k, i, t.catches(hulls[i]));
    dump();
  };
  const fleet = () => Array.from({ length: 8 }, (_, i) => ({ alive: i !== 7, team: bits(i % 3 + 1),
    position: [10 + (i === 4 ? 100 : i === 5 ? 8.53 : i === 6 ? 8.531 : i * 0.1), -3.5, 20].map(bits),
    radius: bits(i === 3 ? 4 : i === 6 ? NaN : 5.53), dying: bits(0), root: i !== 2,
    sphere: i !== 5 && i !== 6, up: bits(4.24), cascade: r & 4 && i === 0 ? 2 : null }));
  dump();
  run({ kind: 'hulls', values: fleet() });
  run({ kind: 'catches' });
  run({ kind: 'health', value: bits(1000) });
  for (let tick = 1; tick <= 666; tick++) run({ kind: 'step', dt: bits(1 / 60), tick: bits(tick) });
  run({ kind: 'hulls', values: fleet() });
  run({ kind: 'crush' });
  if (base) {
    // Binary-exact radius + reach = 8: each positive and negative face
    // is included, while the point just beyond it is excluded.
    const faces = [];
    for (let axis = 0; axis < 3; axis++) for (const sign of [-1, 1]) for (const extra of [0, 2 ** -40]) {
      const position = [10, -3.5, 20];
      position[axis] += sign * (8 + extra);
      faces.push({ alive: true, team: bits(2), position: position.map(bits), radius: bits(5), dying: bits(0),
        root: true, sphere: axis !== 1, up: bits(0), cascade: null });
    }
    run({ kind: 'hulls', values: faces });
    run({ kind: 'catches' });
    run({ kind: 'crush' });
  }
  for (let tick = 667; tick < 867; tick++) {
    const kind = pick(['step', 'step', 'health', 'begin', 'hulls', 'crush', 'catches']);
    if (kind === 'step') run({ kind, dt: bits(pick([1 / 60, 0, -0, 0.1, 4, -1, NaN])), tick: bits(tick) });
    else if (kind === 'health') run({ kind, value: bits(pick([0, -0, 30, 1000, -10, Infinity, -Infinity, NaN])) });
    else if (kind === 'begin') run({ kind, phase: pick(['down', 'reset', 'armed', 'swing']), tick: pick([null, bits(tick)]) });
    else if (kind === 'hulls') run({ kind, values: fleet() });
    else run({ kind });
  }
  unobserve();
  runs.push({ config, ops });
}
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'ops.json'), JSON.stringify(runs));
fs.writeFileSync(path.join(out, 'js.txt'), `${trace.join('\n')}\n`);
console.log(`${runs.length} traps, ${runs.reduce((n, r) => n + r.ops.length, 0)} operations, ${trace.length} trace lines`);
