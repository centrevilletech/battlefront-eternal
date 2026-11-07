import { contentPath } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

// The world's determinism census. A world step (the world worker's, a
// server's) takes every random draw from a named stream and every time from
// its host, so that a run with the same seed and inputs is the same run, and
// a port of the step has nothing hidden to reproduce. Two checks:
// - every Math.random, performance.now and Date.now the world worker's
//   modules hold is classified below (a new one fails until it is), and
//   every legacy stream (src/ai/random.js legacyRandom) is listed with what
//   still reaches it;
// - in a battle, nothing inside a world step or a publication reads a clock,
//   and the only Math.random there is three.js's, for the ids of the objects
//   it makes (which no gameplay reads): no legacy stream reaches a step.

const root = fileURLToPath(new URL('..', import.meta.url));
const content = contentPath();

// Where a module's clock reads and Math.random calls run:
// page: only on a page (presentation, interface, audio, the page's own
//   clocks); never in a world step;
// host: around a world step, not in it: pacing, transport, input arrival,
//   profiling, and the times a host hands the step and its publication;
// legacy: the legacy streams' one draw (src/ai/random.js).
const CENSUS = {
  'Math.random': {
    'src/ai/random.js': { legacy: 1 },
    'src/arc-lightning.js': { page: 58 },
    'src/presentation-pkg/bfe_presentation.js': { page: 1 }, // native sniper cosmetic random primitive
    'src/presentation/cameras.js': { page: 1 }, // injectable cosmetic shake stream
    'src/sim/module.js': { page: 1 }, // binds the camera's standard random primitive
    'src/commandposts.js': { page: 2 }, // the post holograms' flicker (CommandPosts.present)
    'src/presentation/effects.js': { page: 1 }, // a displayed effect's recorded birth seed
    'src/game.js': { page: 3 }, // the sky's flyers, a droid's death blasts (presentEvent)
    'src/launch.js': { page: 3 },
    'src/lightning.js': { page: 1 },
    'src/player-body.js': { page: 2 }, // the spawn screen's preview fidgets
    'src/props.js': { page: 3 }, // build holograms' flicker, the power droids' idle turn (LiveObjects.present)
    'src/vehicle-effects.js': { page: 1 },
  },
  'performance.now': {
    'src/game.js': { page: 20 }, // frames, loading and the page's own clocks
    'src/local-lights.js': { page: 1 },
    'src/point-lights.js': { page: 1 },
    'src/render-resources.js': { page: 2 }, // upload-fence deadline while loading, never a world step
    'src/runtime-profiler.js': { host: 1 },
    'src/runtime/fixed-clock.js': { host: 1 }, // pacing; a world's step runs unlimited and reads none
    'src/runtime/input-latency-trace.js': { page: 1 },
    'src/runtime/local-message-batch.js': { host: 1 },
    'src/runtime/local-server.js': { host: 3 }, // the step's and the publication's time, a page's frame spacing
    'src/runtime/local-world-pump.js': { host: 1 },
    'src/runtime/multiplayer-world.js': { host: 6 }, // input arrival; step()'s default time
    'src/runtime/network-entity-budget.js': { host: 4 },
    'src/runtime/predicted-shots.js': { page: 1 },
    'src/runtime/snapshot-host.js': { host: 1 },
    'src/runtime/wire-host.js': { host: 3 }, // entity budget availability, admission and refunds use transport time
    'src/rust-render/programs.js': { page: 2 }, // the page's own shader registration timing
    'src/vehicles.js': { page: 1 }, // a page's barrel clock (a world's is its host's)
  },
  'Date.now': {
    'src/chat.js': { page: 2 },
    'src/runtime/fixed-clock.js': { host: 1 },
    'src/runtime/local-server.js': { host: 1 },
    'src/runtime/snapshot-host.js': { host: 1 },
    'src/vehicles.js': { page: 1 },
  },
};

// The legacy streams, and what reaches each in a world step today.
const LEGACY_STREAMS = {
  'grenade-spin': 'none in a world (Ordnance.spinRandom is the grenade-spin stream)',
  'missile-waver': 'none in a world (Ordnance.random is the gameplay stream)',
  'item-drop': 'none in a world (Items.random is the gameplay stream)',
  'spawn-post': 'none in a world step (the spawn screen\'s pick of a default post)',
};

const KEYWORDS = new Set(['return', 'typeof', 'case', 'do', 'else', 'in', 'of', 'new', 'delete', 'void', 'throw', 'yield', 'await', 'instanceof']);
// The source with its comments, strings, template text and regular
// expressions blanked out, newlines kept: the same length, positions line up.
function codeOf(text) {
  let out = '', i = 0, last = '', word = '', depth = 0;
  const n = text.length, subs = [];
  const blank = (from, to) => { out += text.slice(from, to).replace(/[^\n]/g, ' '); };
  const regexStarts = () => last === '' || '(,=:[!&|?{};+-*%<>~^'.includes(last) || (last === 'w' && KEYWORDS.has(word));
  // From a backquote or a substitution's closing brace to the template's
  // end or its next substitution.
  const template = () => {
    for (let j = i + 1; j < n; j++) {
      if (text[j] === '\\') { j++; continue; }
      if (text[j] === '`') { blank(i, j + 1); i = j + 1; last = 'w'; word = ''; return; }
      if (text[j] === '$' && text[j + 1] === '{') { blank(i, j + 2); i = j + 2; subs.push(depth); depth++; last = '{'; return; }
    }
    blank(i, n); i = n;
  };
  while (i < n) {
    const c = text[i], d = text[i + 1];
    if (c === '/' && d === '/') { const j = text.indexOf('\n', i); const end = j < 0 ? n : j; blank(i, end); i = end; continue; }
    if (c === '/' && d === '*') { const j = text.indexOf('*/', i + 2); const end = j < 0 ? n : j + 2; blank(i, end); i = end; continue; }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < n && text[j] !== c && text[j] !== '\n') j += text[j] === '\\' ? 2 : 1;
      blank(i, j + 1); i = j + 1; last = 'w'; word = ''; continue;
    }
    if (c === '`') { template(); continue; }
    if (c === '}' && subs.length && depth - 1 === subs[subs.length - 1]) { subs.pop(); depth--; template(); continue; }
    if (c === '/' && regexStarts()) {
      let j = i + 1, inClass = false;
      for (; j < n && text[j] !== '\n'; j++) {
        if (text[j] === '\\') { j++; continue; }
        if (text[j] === '[') inClass = true;
        else if (text[j] === ']') inClass = false;
        else if (text[j] === '/' && !inClass) break;
      }
      j++;
      while (j < n && /[a-z]/i.test(text[j])) j++;
      blank(i, j); i = j; last = 'w'; word = ''; continue;
    }
    if (c === '{') depth++;
    else if (c === '}') depth--;
    if (/[A-Za-z0-9_$]/.test(c)) {
      let j = i;
      while (j < n && /[A-Za-z0-9_$]/.test(text[j])) j++;
      word = text.slice(i, j); out += word; i = j; last = 'w'; continue;
    }
    out += c;
    if (!/\s/.test(c)) { last = c === ')' || c === ']' ? 'w' : c; word = ''; }
    i++;
  }
  return out;
}

// The world worker's modules: static imports from its entry, and the
// kernels a headless world loads (a dynamic import() is otherwise a page's).
function worldModules() {
  const seen = new Set();
  const queue = ['src/runtime/local-server-worker.js', 'src/ray-kernel.js', 'src/movement-kernel.js'].map((f) => join(root, f));
  const imports = /\b(?:import|export)\s[^;]*?\bfrom\s*(['"])([^'"]+)\1|\bimport\s+(['"])([^'"]+)\3/g;
  while (queue.length) {
    const file = queue.pop();
    if (seen.has(file)) continue;
    seen.add(file);
    const text = readFileSync(file, 'utf8');
    const code = codeOf(text);
    for (const m of text.matchAll(imports)) {
      if (code.slice(m.index, m.index + 6) !== text.slice(m.index, m.index + 6)) continue;
      const spec = m[2] ?? m[4];
      if (spec.startsWith('.') && /\.m?js$/.test(spec)) queue.push(join(dirname(file), spec));
    }
  }
  return [...seen].sort();
}

const PATTERNS = {
  'Math.random': /\bMath\s*\??\.\s*random\b/g,
  'performance.now': /\bperformance\s*\??\.\s*now\b/g,
  'Date.now': /\bDate\s*\??\.\s*now\b/g,
};

test('every random draw and clock read in the world worker\'s modules is classified', () => {
  const files = worldModules();
  assert.ok(files.length > 150 && files.some((f) => f.endsWith('src/game.js')) && files.some((f) => f.endsWith('src/ordnance.js')), 'the world worker\'s modules were found');
  const found = { 'Math.random': {}, 'performance.now': {}, 'Date.now': {} };
  const streams = [];
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    const code = codeOf(text);
    const name = relative(root, file).split('\\').join('/');
    for (const [kind, pattern] of Object.entries(PATTERNS)) {
      const n = code.match(pattern)?.length ?? 0;
      if (n) found[kind][name] = n;
    }
    for (const m of text.matchAll(/\blegacyRandom\(\s*(["'])([^"']+)\1\s*\)/g)) if (code.slice(m.index, m.index + 12) === 'legacyRandom') streams.push(m[2]);
  }
  for (const kind of Object.keys(PATTERNS)) {
    const expected = Object.fromEntries(Object.entries(CENSUS[kind]).map(([file, by]) => [file, Object.values(by).reduce((a, b) => a + b, 0)]));
    assert.deepEqual(found[kind], expected, `${kind}: classify each new site in CENSUS (a draw in a world step goes through a named stream; a time comes from the host)`);
  }
  assert.equal(new Set(streams).size, streams.length, 'each legacy stream has one call site');
  assert.deepEqual(streams.sort(), Object.keys(LEGACY_STREAMS).sort(), 'every legacy stream is listed in LEGACY_STREAMS');
});

// The world-hash battle (tools/world-hash.mjs, --view): its player walks,
// turns and fires on a script, against Classic bots who throw grenades;
// Math.random a seeded sequence and the clock the tick's, as there.
test('a world step reads no clock and draws no Math.random but three.js\'s object ids', { skip: !existsSync(`${content}/maps/tat1/map.json`) }, async (t) => {
  process.env.BFE_CONTENT_DIR ||= content;
  let now = 1000, draw = 991, phase = null, routed = null;
  const clocks = [], stray = [], legacy = new Map();
  const where = () => new Error().stack.split('\n').slice(2).filter((line) => !line.includes('node:internal')).slice(1, 5).map((line) => line.trim()).join(' < ');
  t.mock.method(performance, 'now', () => { if (phase) clocks.push(`performance.now in ${phase}: ${where()}`); return now; });
  t.mock.method(Date, 'now', () => { if (phase) clocks.push(`Date.now in ${phase}: ${where()}`); return 1700000000000 + Math.floor(now); });
  t.mock.method(Math, 'random', () => {
    if (phase === 'step') {
      if (routed) legacy.set(routed, (legacy.get(routed) || 0) + 1);
      else if (!/\bgenerateUUID\b/.test(new Error().stack)) stray.push(where());
    }
    routed = null;
    draw = (Math.imul(draw, 1664525) + 1013904223) >>> 0;
    return draw / 4294967296;
  });
  const { tapLegacyRandom } = await import('../src/ai/random.js');
  const untap = tapLegacyRandom((name) => { routed = name; });
  t.after(() => tapLegacyRandom(untap));
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createLocalServer } = await import('../src/runtime/local-server.js');
  const game = await headlessGame('?map=tat1&side=all&era=gcw&ai=classic&seed=2');
  const server = await createLocalServer(game);
  server.welcome();
  for (const [object, method, label] of [[server.world, 'step', 'step'], [server.host, 'publish', 'publish']]) {
    const original = object[method].bind(object);
    object[method] = (...args) => { phase = label; try { return original(...args); } finally { phase = null; } };
  }
  let edges = 0;
  for (let i = 0; i < 2400; i++) {
    now += 1000 / 60;
    const tick = game.worldClock.tick + 1, cycle = i % 240, generation = server.actor.unit.generation, viewTick = Math.max(0, tick - 2.5);
    server.world.input(server.id, { epoch: 1, sequence: tick, tick, generation, down: cycle < 160 ? ['KeyW'] : cycle < 200 ? ['KeyA', 'Mouse0'] : ['KeyD'], actions: [],
      look: [[Math.round(12 * Math.sin(i * 0.05)), Math.round(3 * Math.cos(i * 0.07))]], viewTick });
    if (cycle === 160 || cycle === 200) server.world.fire(server.id, { epoch: 1, id: ++edges, tick, generation, button: 0, pressed: cycle === 160, ...(cycle === 160 ? { viewTick } : {}) });
    server.step();
    await new Promise((resolve) => setImmediate(resolve));
  }
  assert.ok(game.units.all().some((u) => u.alive && u.soldier && !u.human), 'the bots fought');
  const drawn = game.randomState().streams;
  assert.ok(['grenade-spin', 'network-weapons', 'seat-weapons'].every((name) => name in drawn), 'grenades flew, the player\'s trigger repeated, a seat fired unbound');
  assert.deepEqual(clocks.slice(0, 5), [], 'no clock read in a world step or a publication');
  assert.deepEqual(stray.slice(0, 5), [], 'no Math.random in a world step but three.js object ids');
  assert.deepEqual(Object.fromEntries(legacy), {}, 'no legacy stream reaches a world step');
});
