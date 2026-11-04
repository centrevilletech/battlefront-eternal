import { WorldTickEstimator, InputLead, CorrectionSmoother } from '../../src/runtime/reconcile.js';
import { PredictionWindow, retainPredictionCommands } from '../../src/runtime/prediction-window.js';

const pick = (value, names) => Object.fromEntries(names.map(name => [name, value[name]]));
export const ringBytes = ring => Buffer.from(ring.buffer, ring.byteOffset, ring.byteLength).toString('hex');
export const estimatorState = value => ({ ...pick(value, ['hz', 'window', 'at', 'count', 'offset', 'dropped']), bytes: ringBytes(value.offsets) });
export const leadState = value => ({ ...pick(value, ['hz', 'margin', 'maxLead', 'cooldownAcks', 'calmAcks', 'recover', 'extrapolate', 'gapAt', 'gapCount', 'arrivedAt', 'arrivedTick', 'latest', 'lead', 'late', 'worstGap', 'extra', 'cooldown', 'calm', 'measuredKey', 'capped']), gaps: ringBytes(value.gaps), estimator: estimatorState(value.estimator) });
export const smootherState = value => pick(value, ['snap', 'tau', 'x', 'y', 'z', 'count', 'snaps', 'worst']);
export const attempt = fn => {
  try { return { value: fn(), error: null }; }
  catch (error) { return { error: { name: error.name, message: error.message, marker: error.marker ?? null } }; }
};
const fail = name => Object.assign(new Error(name), { marker: name });
const options = native => ({ native });

function seedRing(ring) {
  const bits = [0x7ff0000000000041n, 0xfff0000000000042n, 0x8000000000000000n, 0x0000000000000001n, 0x7ff8000000000043n];
  const view = new DataView(ring.buffer, ring.byteOffset, ring.byteLength);
  for (let i = 0; i < ring.length; i++) view.setBigUint64(i * 8, bits[i % bits.length], true);
}

function estimatorCase(window, hz) {
  return { name: `estimator:ring-${window}-hz-${hz}`, group: 'estimator', native: true, run(native) {
    const e = new WorldTickEstimator({ window, hz }, options(native)), ring = e.offsets, observations = [];
    seedRing(ring); const seeded = ringBytes(ring);
    e.reset(); observations.push({ state: estimatorState(e), seeded, same: e.offsets === ring, estimate: e.estimate(-0) });
    const samples = [[-0, 0, 0], [0, 0, -0], [NaN, 1, 0], [-Infinity, 0, 0], [Infinity, 0, 0], [1, NaN, NaN], [2, 1, NaN]];
    for (let i = 0; i < 140; i++) {
      const args = samples[i] ?? [i * 3, i / 20 + (i % 11) / 1000, Math.floor(i / 37)];
      const result = attempt(() => e.arrive(...args));
      observations.push({ result, state: estimatorState(e), estimate: e.estimate(i / 23), same: e.offsets === ring });
      if (i === 82) { e.reset(); observations.push({ reset: estimatorState(e), same: e.offsets === ring }); }
    }
    return observations;
  } };
}

function leadCase(name, config) {
  return { name: `lead:${name}`, group: 'lead', native: true, run(native) {
    const lead = new InputLead(config, options(native)), ring = lead.gaps, offsets = lead.estimator.offsets, observations = [];
    const targets = [0, -0, -1, 1 / 60, .018, .5, 1, 5, Infinity, -Infinity, NaN];
    const elapsed = [1, 0, -4, 1.5, 60, 61, Infinity, NaN];
    seedRing(offsets); seedRing(ring);
    lead.reset(); observations.push({ state: leadState(lead), same: lead.gaps === ring && lead.estimator.offsets === offsets });
    observations.push({ before: lead.target(0), state: leadState(lead) });
    for (let i = 0; i < 240; i++) {
      const tick = i % 17 === 0 ? i - 2 : i, now = i / 60 + (i % 13 > 9 ? .15 : .005);
      lead.arrive(tick, now, Math.floor(i / 97));
      lead.acknowledge(i % 19 === 0 ? 2.25 : i % 31 === 0 ? NaN : 0, elapsed[i % elapsed.length], i % 38 === 0);
      const target = lead.target(now + .001, i % 29 === 0 ? undefined : tick, targets[i % targets.length]);
      observations.push({ target, state: leadState(lead), same: lead.gaps === ring && lead.estimator.offsets === offsets });
      if (i === 156) lead.reset();
    }
    return observations;
  } };
}

function smootherCase(name, config) {
  return { name: `smoother:${name}`, group: 'smoother', native: true, run(native) {
    const s = new CorrectionSmoother(config, options(native)), observations = [];
    const sizes = [0, -0, Number.MIN_VALUE, 1e-6 * (1 - Number.EPSILON), 1e-6, 1e-6 * (1 + Number.EPSILON), 1e-4, .1, .75, 1, 1 + Number.EPSILON, 1e100, 1e-100];
    const times = [0, -0, 1 / 60, 1 / 120, -.1, .001, 2, Infinity, NaN];
    for (let i = 0; i < 130; i++) {
      if (i % 11 === 0) s.reset();
      const n = sizes[i % sizes.length];
      const result = s.note(n, -n * .125, n * .0625, 0, 0, 0);
      observations.push({ result, state: smootherState(s), length: s.length });
      s.decay(times[i % times.length]);
      observations.push({ state: smootherState(s), length: s.length });
    }
    for (const values of [[NaN, Infinity, 0], [-Infinity, 0, NaN], [NaN, 1, 0], [-0, 0, -0]]) {
      s.reset(); const result = s.note(...values, 0, 0, 0);
      observations.push({ result, state: smootherState(s), length: s.length });
    }
    return observations;
  } };
}

export function windowFixture(native, limit = 120) {
  const window = new PredictionWindow(limit, options(native)), source = {}, state = new Uint8Array([1, 2]), held = ['KeyA'];
  const context = { source, identity: 3, epoch: 4, generation: 5, authorityTick: 20, pending: [], held };
  const save = (extra = {}) => window.save({ ...context, tick: 30, commands: [{ sequence: 10, down: held }], state, ...extra });
  save();
  return { window, context, save, state, held };
}

function windowCase(limit) {
  return { name: `window:decisions-${limit}`, group: 'window', native: true, run(native) {
    const observations = [];
    for (const tick of [-Infinity, -0, 0, -100, -90, -89, 19, 20, 21, 29, 30, 31, Infinity, NaN]) {
      for (const exactResult of [false, true, null]) {
        const { window, context, held, state } = windowFixture(native, limit), old = window.checkpoint;
        const commands = [0, 9, 10, 11, 12, NaN, Infinity, -Infinity].map(sequence => ({ sequence, down: ['KeyW'] }));
        const pending = context.pending = commands, calls = [];
        const result = window.select({ ...context, authorityTick: tick }, exactResult === null ? null : c => { calls.push(c === old); return exactResult; });
        observations.push({ tick, exactResult, calls, checkpoint: result.checkpoint === old, current: window.checkpoint === old,
          from: result.from, held: result.held === held, samePending: result.commands === pending,
          indices: result.commands.map(command => commands.indexOf(command)), authorityTick: old.authorityTick, state: old.state === state });
      }
    }
    return observations;
  } };
}

function retainCase(limit) {
  return { name: `retain:limits-${String(limit)}${Object.is(limit, -0) ? '-negative-zero' : ''}`, group: 'retain', native: true, run(native) {
    const observations = [];
    for (const tick of [-Infinity, -0, 0, .5, 12, 120, Infinity, NaN]) {
      const commands = Array.from({ length: 48 }, (_, i) => ({ generation: i % 5 ? 5 : 6, epoch: i % 7 ? 4 : 3, tick: i - 12, sequence: i, down: [i] }));
      commands.push({ generation: 5, epoch: 4, tick: Infinity }, { generation: NaN, epoch: 4, tick: Infinity });
      const result = retainPredictionCommands(commands, tick, 5, 4, limit, options(native));
      observations.push({ tick, indices: result.map(c => commands.indexOf(c)), fresh: result !== commands, nested: result.every(c => commands.includes(c)) });
    }
    return observations;
  } };
}

export function reconcileNumericCases() {
  const cases = [];
  for (const window of [0, 1, 2, 7, 60]) for (const hz of [0, 60, 120]) cases.push(estimatorCase(window, hz));
  for (const [name, config] of [['defaults', {}], ['short-rings', { window: 3, gapWindow: 5 }], ['network', { recover: true, extrapolate: true, maxLead: 24, cooldown: 2, calm: 4 }], ['empty-gap', { gapWindow: 0 }], ['zero-rate', { hz: 0 }], ['zero-cap', { maxLead: 0, margin: -0 }]]) cases.push(leadCase(name, config));
  for (const [name, config] of [['defaults', {}], ['small-snap', { snap: 1e-4 }], ['zero-tau', { tau: 0 }], ['negative-tau', { tau: -.03 }], ['infinite-tau', { tau: Infinity }], ['nan-tau', { tau: NaN }], ['negative-zero-tau', { tau: -0 }]]) cases.push(smootherCase(name, config));
  for (const limit of [0, 1, 2.5, 120, -2, Infinity, NaN]) cases.push(windowCase(limit));
  for (const limit of [0, -0, 1, 2.5, 120, -2, -5.5, Infinity, -Infinity, NaN]) cases.push(retainCase(limit));
  return cases;
}

export function reconcileCallbackCases() {
  return [
    { name: 'estimator:untouched-ring-payload-and-zero-tie', group: 'callback', native: true, run(native) {
      const e = new WorldTickEstimator({ hz: 0, window: 5 }, options(native)), ring = e.offsets;
      seedRing(ring); const before = ringBytes(ring); e.reset(); const reset = ringBytes(ring);
      e.arrive(-0, 0); const first = estimatorState(e); e.arrive(0, 0); const second = estimatorState(e);
      return { before, reset, first, second, identity: e.offsets === ring, zeroTie: Object.is(e.offset, -0) };
    } },
    { name: 'estimator:noncanonical-ring-configurations', group: 'callback', run(native) {
      return [2.5, NaN, -1, Infinity, '3', null].map(window => attempt(() => {
        const e = new WorldTickEstimator({ window }, options(native));
        const out = []; for (let i = 0; i < 7; i++) { e.arrive(i, i / 60); out.push(estimatorState(e)); } return out;
      }));
    } },
    { name: 'lead:reset-live-estimator-and-fill-boundaries', group: 'callback', run(native) {
      const lead = new InputLead({ gapWindow: 3 }, options(native)), events = [], replacement = new Float64Array([7, 8, 9]);
      lead.extra = 11; lead.latest = 12;
      lead.estimator.reset = () => { events.push(['reset', lead.extra, lead.latest]); lead.gaps = replacement; lead.margin = 3; };
      replacement.fill = function (value) { events.push(['fill', value, lead.extra, lead.latest, this === replacement]); lead.margin = 4; return Float64Array.prototype.fill.call(this, value); };
      lead.reset(); const good = leadState(lead);
      replacement.fill = () => { events.push(['throw', lead.extra, lead.latest]); throw fail('fill-stop'); };
      lead.extra = 20; const error = attempt(() => lead.reset());
      return { events, good, error, final: leadState(lead), identity: lead.gaps === replacement };
    } },
    { name: 'lead:arrival-callback-live-fields-and-overlap', group: 'callback', run(native) {
      const lead = new InputLead({ window: 4, gapWindow: 4 }, options(native)), events = [];
      lead.gaps = lead.estimator.offsets;
      lead.arrive(1, .1); const shared = leadState(lead);
      const arrive = lead.estimator.arrive;
      lead.estimator.arrive = function (...args) { events.push(['before', ...args, lead.latest]); arrive.apply(this, args); lead.arrivedAt = .25; lead.arrivedTick = 2; lead.hz = 30; lead.gapAt = 3; events.push(['after', lead.gaps[0]]); };
      lead.arrive(5, .5, 1);
      return { shared, events, final: leadState(lead), alias: lead.gaps === lead.estimator.offsets };
    } },
    { name: 'lead:measure-publishes-key-before-live-slice-sort', group: 'callback', native: true, run(native) {
      const lead = new InputLead({ gapWindow: 5 }, options(native)), events = [];
      lead.gaps.set([8, -0, 2, NaN, 4]); lead.gapCount = 5; lead.gapAt = 3;
      lead.gaps.slice = function (start, end) {
        events.push(['slice', start, end, lead.measuredKey, lead.worstGap]);
        return { sort() { events.push(['sort', lead.measuredKey]); lead.gapCount = 2; return [4, 8, 12]; } };
      };
      lead.measure(); const measured = leadState(lead);
      lead.gapCount = 5; lead.measure();
      lead.gapAt = 4; lead.gaps.slice = () => { events.push(['throw', lead.measuredKey]); throw fail('slice-stop'); };
      const error = attempt(() => lead.measure()); lead.measure();
      return { events, measured, error, final: leadState(lead) };
    } },
    { name: 'lead:measure-cache-signed-zero-nan-and-key-collision', group: 'callback', native: true, run(native) {
      const lead = new InputLead({ gapWindow: 7 }, options(native)), out = [];
      for (const values of [[-0, 0, -0, 0, NaN, Infinity, -Infinity], [1, 2, 3, 4, 5, 6, 7]]) {
        lead.gaps.set(values); lead.gapCount = 7; lead.gapAt++; lead.measure(); out.push(leadState(lead));
        lead.gaps.fill(99); lead.measure(); out.push(leadState(lead));
      }
      lead.gapCount = 1; lead.gapAt = 4096; lead.measure(); out.push(leadState(lead));
      lead.gapCount = 2; lead.gapAt = 0; lead.gaps[0] = -100; lead.measure(); out.push(leadState(lead));
      return out;
    } },
    { name: 'lead:target-captures-default-latest-before-callbacks', group: 'callback', native: true, run(native) {
      const lead = new InputLead({}, options(native)), events = []; lead.latest = 10;
      lead.measure = () => { events.push(['measure', lead.latest]); lead.latest = 100; lead.margin = 2; lead.estimator = { estimate(now) { events.push(['estimate', now, lead.latest]); lead.margin = 3; lead.worstGap = -.5; lead.extra = 1; lead.extrapolate = true; return 13; } }; };
      const result = lead.target(.25); return { result, events, state: pick(lead, ['latest', 'lead', 'capped', 'margin', 'extra', 'worstGap']) };
    } },
    { name: 'smoother:reset-sees-published-note-and-increment-is-late', group: 'callback', native: true, run(native) {
      const s = new CorrectionSmoother({ snap: .5 }, options(native)), events = [];
      s.reset = function () { events.push(smootherState(this)); this.snaps = 40; this.x = .25; this.y = -.25; this.z = 0; };
      const result = s.note(1, 2, 3, 0, 0, 0), first = smootherState(s);
      s.reset = function () { events.push(smootherState(this)); this.snaps = 70; throw fail('reset-stop'); };
      const error = attempt(() => s.note(2, 0, 0, 0, 0, 0));
      return { result, first, error, events, final: smootherState(s) };
    } },
    { name: 'smoother:decay-reset-callback-can-reenter', group: 'callback', native: true, run(native) {
      const s = new CorrectionSmoother({}, options(native)), events = []; s.note(.01, 0, 0, 0, 0, 0);
      s.reset = function () { events.push(smootherState(this)); this.x = this.y = this.z = 0; this.note(.002, .001, 0, 0, 0, 0); };
      s.decay(1); return { events, state: smootherState(s), length: s.length };
    } },
    { name: 'smoother:setter-publication-and-reset-chain', group: 'callback', run(native) {
      const s = new CorrectionSmoother({}, options(native)), events = [], backing = { x: .25, y: .5, z: .75 };
      for (const key of ['x', 'y', 'z']) Object.defineProperty(s, key, { configurable: true, get() { events.push(['get', key]); return backing[key]; }, set(value) { events.push(['set', key, value]); backing[key] = value; if (key === 'x') backing.y += 1; } });
      s.reset(); const first = { ...backing }; events.push(['note']); s.note(.1, .2, .3, 0, 0, 0);
      return { first, events, backing, count: s.count, snaps: s.snaps, worst: s.worst };
    } },
    ...windowCallbackCases(),
    ...retentionCallbackCases(),
  ];
}

function windowCallbackCases() {
  return [
    { name: 'window:exact-keeps-old-checkpoint-after-nested-save', group: 'callback', native: true, run(native) {
      const { window, context, save } = windowFixture(native), old = window.checkpoint, replacementHeld = ['KeyD'], events = [];
      const commands = context.pending = [11, 12, 13].map(sequence => ({ sequence, down: [sequence] }));
      const result = window.select({ ...context, authorityTick: 25 }, c => {
        events.push(['exact', c === old]); save({ tick: 90, commands: [{ sequence: 100, down: replacementHeld }] });
        const inner = window.select(context); events.push(['inner', inner.checkpoint === window.checkpoint, inner.from]);
        c.tick = 31; c.held = replacementHeld; c.sequence = 12; return {};
      });
      return { events, old: result.checkpoint === old, currentDifferent: window.checkpoint !== old, currentTick: window.checkpoint.tick,
        from: result.from, held: result.held === replacementHeld, indices: result.commands.map(c => commands.indexOf(c)), authorityTick: old.authorityTick };
    } },
    { name: 'window:exact-false-reset-throw-and-live-authority-setter', group: 'callback', run(native) {
      const out = [];
      for (const mode of ['false', 'throw', 'setter', 'setter-throw']) {
        const { window, context, save } = windowFixture(native), old = window.checkpoint, events = []; let authorityTick = old.authorityTick;
        window.reset = function () { events.push(['reset', this.checkpoint === old]); this.checkpoint = null; };
        const result = attempt(() => window.select({ ...context, authorityTick: 25 }, c => {
          events.push(['exact', c === old]); save({ tick: 88, commands: [] });
          if (mode === 'throw') throw fail('exact-stop');
          if (mode.startsWith('setter')) Object.defineProperty(c, 'authorityTick', { get: () => authorityTick, set(value) { events.push(['set', value]); authorityTick = value; c.tick = 37; c.held = null; if (mode === 'setter-throw') throw fail('authority-stop'); } });
          return mode !== 'false';
        }));
        out.push({ mode, events, error: result.error, from: result.value?.from, held: result.value?.held === context.held,
          oldResult: result.value?.checkpoint === old, current: window.checkpoint?.tick, authorityTick });
      }
      return out;
    } },
    { name: 'window:filter-getters-see-live-checkpoint-and-saved-length', group: 'callback', run(native) {
      const { window, context } = windowFixture(native), c = window.checkpoint, events = [];
      const pending = context.pending = new Array(5), commands = [{ sequence: 11 }, { sequence: 12 }, { sequence: 13 }, { sequence: 14 }];
      pending[0] = commands[0]; pending[2] = commands[2]; pending[3] = commands[3];
      Object.defineProperty(commands[0], 'sequence', { get() { events.push('first'); c.sequence = 12; pending[1] = commands[1]; delete pending[3]; pending.push({ sequence: 100 }); return 11; } });
      Object.defineProperty(commands[1], 'sequence', { get() { events.push('second'); c.sequence = 9; return 12; } });
      const result = window.select(context);
      return { events, indices: result.commands.map(c => commands.indexOf(c)), pendingLength: pending.length, checkpoint: result.checkpoint === c };
    } },
    { name: 'window:save-at-and-late-property-reads', group: 'callback', run(native) {
      const { window, context, state } = windowFixture(native), events = [], old = window.checkpoint, held = ['KeyS'];
      const last = {};
      Object.defineProperties(last, { sequence: { get() { events.push(['sequence', window.checkpoint.sequence]); window.checkpoint = { sequence: -0 }; return null; } }, down: { get() { events.push(['down', Object.is(window.checkpoint.sequence, -0)]); return held; } } });
      const commands = { at(index) { events.push(['at', index]); window.checkpoint = { sequence: 52 }; return last; } };
      window.save({ ...context, commands, tick: 44, state }); const saved = window.checkpoint;
      const error = attempt(() => window.save({ ...context, commands: { at() { events.push('at-throw'); throw fail('at-stop'); } }, tick: 50, state }));
      return { events, error, oldReplaced: old !== saved, same: saved === window.checkpoint, state: saved.state === state, held: saved.held === held, sequence: saved.sequence, tick: saved.tick };
    } },
    { name: 'window:custom-filter-species-and-nullish-continuation', group: 'callback', run(native) {
      const { window, context } = windowFixture(native), events = [], old = window.checkpoint;
      class Commands extends Array { static get [Symbol.species]() { events.push('species'); return Array; } }
      const commands = new Commands({ sequence: 11 }, { sequence: 12 }); context.pending = commands;
      const first = window.select(context);
      old.held = null; old.tick = undefined;
      commands.filter = function (predicate) { events.push(['filter', this === commands, predicate(this[0])]); return { tag: 'custom' }; };
      const second = window.select(context);
      return { events, first: first.commands.map(c => commands.indexOf(c)), second: { from: second.from, held: second.held === context.held, commands: second.commands }, current: window.checkpoint === old };
    } },
    { name: 'window:identity-and-exact-callability-boundaries', group: 'callback', native: true, run(native) {
      const out = [];
      for (const change of [{}, { source: {} }, { identity: 90 }, { epoch: NaN }, { generation: NaN }, { authorityTick: 25 }]) {
        const { window, context } = windowFixture(native); const c = window.checkpoint;
        const result = attempt(() => window.select({ ...context, ...change }, 3));
        out.push({ error: result.error, retained: result.value?.checkpoint === c, samePending: result.value?.commands === context.pending, current: window.checkpoint === c });
      }
      return out;
    } },
  ];
}

function retentionCallbackCases() {
  return [
    { name: 'retain:getter-short-circuit-and-coercion-per-command', group: 'callback', run(native) {
      const events = [], commands = [];
      for (let i = 0; i < 5; i++) {
        const c = {}; Object.defineProperties(c, {
          generation: { get() { events.push([i, 'generation']); return i === 0 ? 2 : 1; } },
          epoch: { get() { events.push([i, 'epoch']); return i === 1 ? 2 : 1; } },
          tick: { get() { events.push([i, 'tick']); if (i === 2) commands.push({ generation: 1, epoch: 1, tick: 100 }); return 10 + i; } },
        }); commands.push(c);
      }
      const tick = { valueOf() { events.push('tick-value'); return 15; } }, limit = { valueOf() { events.push('limit-value'); return 4.5; } };
      const result = retainPredictionCommands(commands, tick, 1, 1, limit, options(native));
      return { events, indices: result.map(c => commands.indexOf(c)), length: commands.length };
    } },
    { name: 'retain:holes-inherited-elements-species-and-custom-result', group: 'callback', run(native) {
      const events = [];
      class Commands extends Array { static get [Symbol.species]() { events.push('species'); return Array; } }
      const commands = new Commands(4), inherited = { generation: 1, epoch: 1, tick: 10 };
      Object.defineProperty(Commands.prototype, 1, { configurable: true, value: inherited, writable: true });
      commands[2] = { generation: 1, epoch: 1, tick: 11 }; commands[3] = { generation: 2, epoch: 1, tick: 12 };
      const result = retainPredictionCommands(commands, 12, 1, 1, 3, options(native));
      commands.filter = function (fn) { events.push(['filter', fn(inherited)]); return { slice(start) { events.push(['slice', start]); return 'custom'; } }; };
      const custom = retainPredictionCommands(commands, 12, 1, 1, 3, options(native));
      return { events, result: result.map(c => c === inherited ? 'inherited' : c.tick), custom };
    } },
  ];
}

export const reconcileCases = () => [...reconcileNumericCases(), ...reconcileCallbackCases()];
export function runReconcileCase(scenario, native = null) {
  const before = native?.calls ?? 0;
  const result = scenario.run(native);
  return { result, calls: (native?.calls ?? 0) - before };
}
