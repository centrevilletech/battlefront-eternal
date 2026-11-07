// The simulation capture, as a tools/world-hash.mjs plugin (run it through
// tools/sim-capture.mjs): while the battle runs exactly as world-hash runs
// it, this records, in the BFECAP format (tools/sim-capture/format.mjs):
//   - every tick: which phase drew how many numbers from which random
//     stream, and the tick's three fingerprints (a "tick" record);
//   - on sampled ticks, for each frame of the chosen specs
//     (tools/sim-capture/specs.mjs): the frame's arguments and state at its
//     start ("enter"), every call across its boundary with its arguments
//     before ("call") and its result and arguments after ("ret"), every draw
//     in order (with the call it came in, when it came inside one), and its
//     state and result at the end ("exit").
// Options (on world-hash's command line, which ignores them):
//   --capture-out FILE      where (required unless --capture-scan)
//   --capture-specs A,B     the specs (default ticks: tick records only)
//   --capture-every N       sample every Nth tick (default 7)
//   --capture-keep FILE     also sample the ticks this JSON array lists
//   --capture-scan FILE     only find the ticks with deaths, spawns,
//                           boardings and explosions (and the chosen
//                           specs' own events), and write them there
// It changes nothing in the battle: the module hook adds markers that only
// call back here, the shims call through, and nothing it writes reads a
// getter or calls game code. tools/sim-capture.mjs checks that the
// capture's fingerprints are a plain run's.
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { register } from 'node:module';
import { fileURLToPath } from 'node:url';
import { Writer, Handle, Omitted, RECORD, MAGIC } from './format.mjs';
import { PHASE_NAMES } from './phases.mjs';
import { ClassicRandom, RandomStreams } from '../../src/ai/random.js';
import { exportState, ExportContext } from '../../src/sim/runtime.js';
import { KIND_TABLE, RECORD_CLASSES } from '../../src/sim/gen/index.js';

register('./hooks.mjs', import.meta.url);
// Specs can import simulation classes, including weapons through vehicles.
// Register first so those modules' seeded draws and phase markers are
// instrumented before any spec causes them to load.
const { SPECS, Expand } = await import('./specs.mjs');

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const outFile = opt('capture-out', null), scanFile = opt('capture-scan', null);
const specNames = String(opt('capture-specs', 'ticks')).split(',').filter(Boolean);
const every = Number(opt('capture-every', 7));
const keep = new Set(opt('capture-keep', null) ? JSON.parse(fs.readFileSync(opt('capture-keep'), 'utf8')) : []);
if (!outFile && !scanFile) throw Error('sim-capture: --capture-out FILE or --capture-scan FILE');
for (const name of specNames) if (!SPECS[name]) throw Error(`sim-capture: no spec ${name} (${Object.keys(SPECS).join(', ')})`);
if (!(Number.isInteger(every) && every >= 1)) throw Error(`sim-capture: --capture-every ${every}`);
const frameSpecs = specNames.flatMap((name) => SPECS[name].frames.map((frame) => ({ ...frame, spec: name, name: frame.phase ?? frame.method })));
for (const frame of frameSpecs) if (frame.phase && !PHASE_NAMES.includes(frame.phase)) throw Error(`sim-capture: ${frame.spec}: no phase ${frame.phase}`);

// ---- What a record names rather than holds.
let game = null, units = new Set(), unitParts = new Map(), vehicles = new Map(), bodies = new Set(), systems = new Map();
function refreshIdentity() {
  units = new Set(game.units.all());
  unitParts = new Map();
  for (const u of units) for (const part of ['soldier', 'combat', 'intent', 'presentation', 'animator']) {
    const v = u[part];
    if (v && typeof v === 'object' && !unitParts.has(v)) unitParts.set(v, new Handle(`unit.${part}`, u.id));
  }
  vehicles = new Map((game.vehicles?.vehicles ?? []).map((v, i) => [v, i]));
  bodies = new Set(game.collision?.bodies ?? []);
}
function buildSystems() {
  systems = new Map([[game, 'game']]);
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors(game))) {
    if ('value' in desc && desc.value && typeof desc.value === 'object' && !systems.has(desc.value)) systems.set(desc.value, key);
  }
  for (const key of ['ai', 'soldier', 'combat', 'networkRemotes']) { const v = game[key]; if (v && typeof v === 'object' && !systems.has(v)) systems.set(v, key); }
  if (game.ordnance?.hitHistory) systems.set(game.ordnance.hitHistory, 'hitHistory');
}
const isDef = (v) => Object.getPrototypeOf(v) === Object.prototype && typeof v.class === 'string' && typeof v.base === 'string';
// The store's kinds a capture names (rust/sim/schema): by class, up the
// prototype chain (the source's names: captures run unminified), or by
// shape for the plain objects among them: a hull's or seat's beam source
// (src/vehicles.js beamSource: getters, read never) and what
// src/ordnance-owner.js froze of a unit.
function isA(v, name) {
  for (let p = Object.getPrototypeOf(v); p && p !== Object.prototype; p = Object.getPrototypeOf(p)) if (p.constructor?.name === name) return true;
  return false;
}
const isEmitter = (v) => Object.getPrototypeOf(v) === Object.prototype && typeof v.viewDirection === 'function' && typeof Object.getOwnPropertyDescriptor(v, 'matrix')?.get === 'function';
const isOwnerSnapshot = (v) => Object.isFrozen(v) && Object.getPrototypeOf(v) === Object.prototype && Number.isSafeInteger(v.id) && Number.isSafeInteger(v.generation) && 'className' in v;
const isBody = (v) => Object.getPrototypeOf(v) === Object.prototype && typeof v.offset === 'number' && 'bvh' in v && 'polygonOf' in v;
const KIND_TESTS = { unit: (v) => isA(v, 'Unit'), vehicle: (v) => isA(v, 'Vehicle'), turret: (v) => isA(v, 'Turret'), weapon: (v) => isA(v, 'Weapon'),
  emitter: isEmitter, ownerSnapshot: isOwnerSnapshot, body: isBody };
// Every other class a schema names (a kind's `class`, a record's `$class`
// in a union of records) by its constructor's name.
for (const k of KIND_TABLE) if (k?.classes.length) KIND_TESTS[k.name] ??= (v) => k.classes.some((c) => isA(v, c));
for (const [name, classes] of Object.entries(RECORD_CLASSES)) KIND_TESTS[name] ??= (v) => classes.some((c) => isA(v, c));
// A store kind's handle: a unit by its registry id, a hull by its place in
// the battle's list, a collision body by its serial, anything else
// numbered by kind in the order the capture meets it. An owner snapshot's
// fields are written once, whole, like a definition (key ownerSnapshot:N).
const numbering = new Map();
function handleOf(kind, v) {
  if (kind === 'unit' && Number.isSafeInteger(v.id)) return new Handle('unit', v.id);
  if (kind === 'vehicle' && vehicles.has(v)) return new Handle('vehicle', vehicles.get(v));
  if (kind === 'body') return new Handle('body', v.offset / 0x1000000);
  let m = numbering.get(kind);
  if (!m) numbering.set(kind, (m = { ids: new WeakMap(), next: 0 }));
  let n = m.ids.get(v);
  if (n === undefined) {
    m.ids.set(v, (n = m.next++));
    if (kind === 'ownerSnapshot') defQueue.push([`ownerSnapshot:${n}`, { ...v }]);
  }
  return new Handle(kind, kind === 'vehicle' ? `gone:${n}` : n);
}
// What a frame's state goes through once its package's schema declares its
// kinds (src/sim/gen): the store's exporter, its references our handles.
function exporter() {
  const cx = new ExportContext({ tests: KIND_TESTS, ref: (kind, object) => handleOf(kind, object), def: (table, def) => new Handle('def', defKey(def)) });
  return { export: (kind, object) => exportState(kind, object, cx) };
}
// Instances of these classes are values, written out where met; an
// instance of any other class (a turret, a weapon, a seat lease, an AI's
// controller) is a handle, numbered by class in the order the capture meets
// them, unless a frame of the capture asks for its class whole (its spec's
// `whole`). A record then holds its own state and names the rest, instead
// of whatever a turret's lease reaches (the whole AI).
const VALUE_CLASSES = new Set(['Vector2', 'Vector3', 'Vector4', 'Quaternion', 'Matrix3', 'Matrix4', 'Euler', 'Box2', 'Box3', 'Sphere', 'Ray',
  'Plane', 'Triangle', 'Line3', 'Color', 'Spherical', 'Cylindrical']);
const wholeClasses = new Set(frameSpecs.flatMap((f) => f.whole ?? []));
const instances = new WeakMap(), instanceCounts = new Map();
function instanceHandle(v, name) {
  let n = instances.get(v);
  if (n === undefined) { n = instanceCounts.get(name) ?? 0; instanceCounts.set(name, n + 1); instances.set(v, n); }
  return new Handle(`object:${name}`, n);
}
function identify(v) {
  if (units.has(v)) return new Handle('unit', v.id);
  const part = unitParts.get(v);
  if (part) return part;
  const vi = vehicles.get(v);
  if (vi !== undefined) return new Handle('vehicle', vi);
  if (bodies.has(v) || isBody(v)) return new Handle('body', v.offset / 0x1000000);
  const system = systems.get(v);
  if (system) return new Handle('system', system);
  if (isOwnerSnapshot(v)) return handleOf('ownerSnapshot', v);
  if (isEmitter(v)) return handleOf('emitter', v);
  if (v.isObject3D) return new Handle('object3d', v.uuid);
  if (v.isBufferGeometry || v.isMaterial || v.isTexture || v.isSkeleton) return new Handle(v.type ?? 'three', v.uuid);
  if (isDef(v)) return new Handle('def', defKey(v));
  if (v instanceof WeakMap || v instanceof WeakSet || v instanceof Promise || v instanceof WeakRef) return new Omitted(v.constructor.name);
  const proto = Object.getPrototypeOf(v);
  if (proto === Object.prototype || proto === null || Array.isArray(v) || ArrayBuffer.isView(v) || v instanceof Map || v instanceof Set) return undefined;
  const name = proto.constructor?.name || '?';
  if (VALUE_CLASSES.has(name) || wholeClasses.has(name)) return undefined;
  for (const kind of ['weapon', 'turret', 'vehicle', 'unit', 'seatLease', 'seatUnit']) if (KIND_TESTS[kind](v)) return handleOf(kind, v);
  return instanceHandle(v, name);
}

// Content definitions a record names are written once each, whole, in a
// "def" record of their own (keyed by class; a second object of the same
// class, were there one, by class#2 and on), so a capture carries the
// content its frames read.
const defKeys = new Map(), defClassCounts = new Map(), defQueue = [];
function defKey(def) {
  let key = defKeys.get(def);
  if (key === undefined) {
    const n = (defClassCounts.get(def.class) ?? 0) + 1;
    defClassCounts.set(def.class, n);
    key = n === 1 ? def.class : `${def.class}#${n}`;
    defKeys.set(def, key);
    defQueue.push([key, def]);
  }
  return key;
}

// ---- The writer and the file.
const writer = new Writer({ identify });
let fd = null, records = 0;
const counts = {};
function flush(force = false) {
  if (!fd || (!force && writer.length < (8 << 20))) return;
  fs.writeSync(fd, writer.take());
}
function write(type, at, name, value) {
  writer.record(type, at, name, value);
  records++;
  if (type === RECORD.frame) counts[name] = (counts[name] ?? 0) + (value.k === 'enter' ? 1 : 0);
  // The definitions it named for the first time (and those they name).
  while (defQueue.length) {
    const [key, def] = defQueue.shift();
    writer.whole.add(def);
    writer.record(RECORD.def, at, key, def);
    writer.whole.clear();
    records++;
  }
  flush();
}
// The units as a frame finds them (their lives, teams and classes), before
// a frame that asks for it (its spec's world: true), for what its replay
// reads of units it only names: whose a round is, what a hit unit is made of.
function writeWorld() {
  const list = game.units.all().map((u) => ({ id: u.id, generation: u.generation, alive: Boolean(u.alive), dead: Boolean(u.dead), removed: Boolean(u.removed),
    team: u.team ?? null, className: typeof u.className === 'string' ? u.className : null, classDef: u.classDef && typeof u.classDef === 'object' ? u.classDef : null }));
  write(RECORD.world, current, 'world', { units: list });
}
// A frame's state: its roots written out, those wrapped in expand() whole.
function stateValue(roots) {
  for (const [key, v] of Object.entries(roots)) if (v instanceof Expand) { roots[key] = v.value; if (v.value && typeof v.value === 'object') writer.whole.add(v.value); }
  return roots;
}

// ---- Random draws: every generator the game draws from, labelled.
const labels = new WeakMap();
let anonymous = 0;
const labelOf = (rng) => { let label = labels.get(rng); if (!label) { label = `anon:${anonymous++}`; labels.set(rng, label); } return label; };
const nextOf = ClassicRandom.prototype.next;
ClassicRandom.prototype.next = function next() { const v = nextOf.call(this); onDraw(labelOf(this), v); return v; };
const getOf = RandomStreams.prototype.get;
RandomStreams.prototype.get = function get(name) { const stream = getOf.call(this, name); if (!labels.has(stream)) labels.set(stream, `stream:${name}`); return stream; };
const mathRandom = Math.random;
Math.random = function random() { const v = mathRandom(); onDraw('Math.random', v); return v; };
globalThis.__bfeDraw = {
  seeded(seed, draws) { return () => { const v = draws(); onDraw(`seeded:${seed}`, v); return v; }; },
  named(random, name) { labels.set(random, name); },
};

let current = 0, ticking = false, sampled = false;
const phaseStack = [];
let tickDraws = new Map(); // phase -> Map(bucket -> count)
function onDraw(label, value) {
  if (!ticking) return;
  const phase = phaseStack[phaseStack.length - 1] ?? 'between';
  let byStream = tickDraws.get(phase);
  if (!byStream) tickDraws.set(phase, byStream = new Map());
  const bucket = label.startsWith('seeded:') ? 'seeded' : label;
  byStream.set(bucket, (byStream.get(bucket) ?? 0) + 1);
  for (const frame of open) (frame.depth ? frame.inner : frame.pending).push([label, value]);
}

// ---- Frames.
const open = [];
let nextFrame = 0;
// A method frame's receiver (`self`: the hull whose tick it is) goes to its
// spec's state(game, args, cap, self) and into the enter record as `t`.
function openFrame(spec, args, self) {
  if (spec.world) writeWorld();
  const frame = { id: nextFrame++, spec, depth: 0, pending: [], inner: null, boundaries: new Set(spec.labels) };
  const parent = open.length ? open[open.length - 1].id : null;
  write(RECORD.frame, current, spec.name, { k: 'enter', f: frame.id, p: parent, ...(self === undefined ? {} : { t: self }), a: args,
    s: stateValue(spec.state(game, args, exporter(), self)) });
  writer.whole.clear();
  open.push(frame);
  return frame;
}
function closeFrame(frame, args, result, self) {
  if (open[open.length - 1] !== frame) throw Error(`sim-capture: frame ${frame.spec.name} closed out of order`);
  open.pop();
  write(RECORD.frame, current, frame.spec.name, { k: 'exit', f: frame.id, d: frame.pending, s: stateValue(frame.spec.state(game, args, exporter(), self)), r: result });
  writer.whole.clear();
}

globalThis.__bfePhase = {
  enter(name) {
    phaseStack.push(name);
    if (!ticking || !sampled) return;
    for (const spec of frameSpecs) if (spec.phase === name) spec.opened = openFrame(spec, []);
  },
  exit(name) {
    const top = phaseStack.pop();
    if (top !== name) throw Error(`sim-capture: phase ${name} ends inside ${top}`);
    if (!ticking || !sampled) return;
    for (let i = frameSpecs.length - 1; i >= 0; i--) {
      const spec = frameSpecs[i];
      if (spec.phase === name && spec.opened) { closeFrame(spec.opened, [], undefined); spec.opened = null; }
    }
  },
};

// A frame on a method of a live object, and a boundary call: shims that call
// straight through unless a sampled frame is open.
const shims = new Map(); // target -> Map(method -> its outermost wrapper)
const boundaryShims = new Set();
function shim(target, method, make) {
  const base = target[method];
  if (typeof base !== 'function') throw Error(`sim-capture: ${method} is not a function`);
  const wrapper = make(base);
  target[method] = wrapper;
  if (!shims.has(target)) shims.set(target, new Map());
  shims.get(target).set(method, wrapper);
}
function installShims() {
  for (const spec of frameSpecs) {
    spec.labels = [];
    for (const [label, target, method, extra, entry] of spec.boundaries?.(game) ?? []) {
      spec.labels.push(label);
      if (extra) (spec.extras ??= new Map()).set(label, extra);
      if (entry) (spec.entries ??= new Map()).set(label, entry);
      if (!target || boundaryShims.has(label)) continue;
      boundaryShims.add(label);
      shim(target, method, (base) => function boundary(...args) {
        if (!open.length) return base.apply(this, args);
        const frames = open.filter((f) => f.depth === 0 && f.boundaries.has(label));
        if (!frames.length) return base.apply(this, args);
        for (const f of frames) {
          const entry = f.spec.entries?.get(label);
          write(RECORD.frame, current, f.spec.name, { k: 'call', f: f.id, n: label, a: args, d: f.pending,
            ...(entry ? { s: stateValue(entry(game, exporter(), args, this)) } : {}) });
          f.pending = []; f.inner = []; f.depth++;
        }
        let result, threw = true;
        try { result = base.apply(this, args); threw = false; return result; }
        finally {
          for (const f of frames) {
            f.depth--;
            const extra = f.spec.extras?.get(label);
            write(RECORD.frame, current, f.spec.name, { k: 'ret', f: f.id, n: label, r: threw ? new Omitted('threw') : result,
              o: args.map((a) => (a && typeof a === 'object' ? a : undefined)), i: f.inner, ...(extra ? { x: stateValue(extra(game, exporter(), args, this)) } : {}) });
            writer.whole.clear();
            f.inner = null;
          }
        }
      });
    }
  }
  for (const spec of frameSpecs) {
    if (!spec.method) continue;
    // An object or a prototype (every hull's tick at once); a battle
    // without one (a map without hulls) has none of these frames.
    const target = spec.target(game), method = spec.method.split('.').pop();
    if (!target) { spec.missing = true; continue; }
    shim(target, method, (base) => function frameShim(...args) {
      if (!ticking || !sampled) return base.apply(this, args);
      const frame = openFrame(spec, args, this);
      let result;
      try { result = base.apply(this, args); return result; } finally { closeFrame(frame, args, result, this); }
    });
  }
}
// A shimmed method the game has since replaced (a callback set again)
// would go unrecorded: the capture stops rather than miss it.
function checkShims() {
  for (const [target, methods] of shims) for (const [method, wrapper] of methods) {
    if (target[method] !== wrapper) throw Error(`sim-capture: ${method} was replaced after the capture shimmed it`);
  }
}

// ---- Events that make a tick worth sampling whole (and its neighbours).
let lastUnits = new Map(), explosions = 0, tickEvents = [];
const eventTicks = new Set(), events = [];
function noteEvents() {
  tickEvents = [];
  const note = (kind, what) => { tickEvents.push([kind, what]); events.push([current, kind, what]); };
  const seen = new Map();
  for (const u of game.units.all()) {
    const state = { alive: Boolean(u.alive), generation: u.generation, removed: Boolean(u.removed), seat: u.seat?.vehicle ?? u.seat?.turret ?? null };
    const before = lastUnits.get(u);
    if (before) {
      if (before.alive && !state.alive) note('death', u.id);
      if (before.generation !== state.generation) note('spawn', u.id);
      if (before.seat !== state.seat) note(state.seat ? 'board' : 'leave', u.id);
    }
    seen.set(u, state);
  }
  lastUnits = seen;
  if (explosions) { note('explosion', explosions); explosions = 0; }
  // A spec's own events (a post changing hands, a respawn's preparation).
  for (const name of specNames) SPECS[name].events?.(game, note);
  if (tickEvents.length) eventTicks.add(current);
}

let header = null;
export function battle({ game: g, index, options }) {
  if (index > 0) throw Error('sim-capture: one battle a capture (no --battles)');
  game = g;
  buildSystems();
  refreshIdentity();
  // Native AI exposes its shared stream facades explicitly. Walking the
  // owner reaches the WASM memory views and cannot discover native draws.
  // These labels cover JS callers of A/B; native per-draw capture is separate.
  const world = game.ai?.worldData;
  if (world) {
    labels.set(world.random, 'A');
    labels.set(world.randomB, 'B');
  }
  // Explosion presentation and item blasts have separate owners. Count their
  // real public boundaries without requiring an obsolete ordnance.explode alias.
  for (const [owner, method] of [[game.ordnance, 'explosion'], [game.items, 'explode']]) shim(owner, method, (base) => function counted(...args) { if (ticking) explosions++; return base.apply(this, args); });
  if (scanFile) return;
  installShims();
  // The commit, from git, or from BFE_COMMIT where the checkout's history
  // is not at hand (a copy of the worktree on another machine).
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const head = process.env.BFE_COMMIT || spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim();
  const dirty = process.env.BFE_COMMIT ? '' : spawnSync('git', ['status', '--porcelain', '--', 'src', 'tools', 'rust'], { cwd: root, encoding: 'utf8' }).stdout.trim();
  fd = fs.openSync(outFile, 'w');
  fs.writeSync(fd, MAGIC);
  header = { format: 1, tool: 'sim-capture', node: process.version, v8: process.versions.v8, commit: dirty ? `${head}+changes` : head,
    battle: { map: options.map, side: options.side, era: options.era, seed: options.seed, ticks: options.ticks, interval: options.interval,
      ack: options.ack, view: options.view, rays: options.rays, movement: options.movement, ai: options.ai, heroes: options.heroes, query: options.query },
    sample: { every, keep: [...keep].sort((a, b) => a - b) }, phases: PHASE_NAMES, specs: specNames,
    frames: frameSpecs.map((f) => ({ name: f.name, spec: f.spec, kind: f.phase ? 'phase' : 'method', boundaries: f.labels, ...(f.missing ? { missing: true } : {}) })),
    streams: { A: 'the AI\'s shared generator (ai.randomA)', B: 'the second (ai.randomB)', C: 'the Enhanced aim generator', 'stream:NAME': 'a named stream (RandomStreams)',
      'Math.random': 'the seeded Math.random world-hash installs', 'seeded:SEED': 'a press\'s spread draws (weapons.js seededRandom)', 'anon:N': 'another ClassicRandom, by first draw' } };
  write(RECORD.header, 0, 'header', header);
}

export function tick(t) {
  current = t;
  ticking = true;
  sampled = !scanFile && (t % every === 0 || keep.has(t));
  tickDraws = new Map();
  if (sampled) { refreshIdentity(); checkShims(); }
}
export function ticked(t, line) {
  if (phaseStack.length) throw Error(`sim-capture: tick ${t} ended inside ${phaseStack.join(' > ')}`);
  if (open.length) throw Error(`sim-capture: tick ${t} ended with frames open`);
  noteEvents();
  ticking = false;
  if (scanFile) return;
  const draws = [...tickDraws].map(([phase, byStream]) => [phase, Object.fromEntries(byStream)]);
  write(RECORD.tick, t, '', { k: 'tick', line, draws, sampled, events: tickEvents });
}

export function finish() {
  if (scanFile) {
    fs.writeFileSync(scanFile, JSON.stringify({ ticks: [...eventTicks].sort((a, b) => a - b), events }) + '\n');
    return;
  }
  flush(true);
  fs.closeSync(fd);
  fs.writeFileSync(`${outFile}.json`, JSON.stringify({ ...header, records, frames: counts, events: events.length, bytes: fs.statSync(outFile).size }, null, 1) + '\n');
}
