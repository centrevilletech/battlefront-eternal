// The 2004 game's random numbers, drawn the way the game draws them.
//
// A generator is one 32-bit state. Each draw steps it twice through the
// linear congruential map x' = x * 1664525 + 1013904223 (mod 2^32) and
// returns the high half of the second step over the high half of the first;
// the second step is the new state. The game keeps one shared generator for
// AI decisions (roulettes, route picks, timers), so every draw anywhere moves
// every later result: callers draw in the order the original does.
//
// uniform() is the game's [0, 1) float: the draw's top 23 bits become the
// mantissa of a float in [1, 2), less 1. Results are exact float32 values in
// steps of 2^-23. int(lo, hi) is its inclusive integer pick:
// trunc(float32(U * (hi - lo + 1) + lo)), or lo when hi <= lo.
//
// Nothing here is seeded from the clock: a match seeds its streams, so a
// replay with the same seed and inputs draws the same numbers.

const MUL = 1664525;
const ADD = 1013904223;
const f32 = Math.fround;
const generators = new WeakMap();
const uniformDraws = new WeakMap();
// A boundary can skip scanning unchanged JS streams. This stamp observes
// draws and explicit restores without changing any generator or draw order.
export let randomRevision = 0;

// A phase transfers the generator's state, not its callback. Keep the binding
// beside the function so existing AI and weapon callers retain ordinary draws.
export function randomBinding(draw) { return generators.get(draw); }
export function uniformRandom(random) {
  let draw = uniformDraws.get(random);
  if (!draw) {
    draw = () => random.uniform();
    uniformDraws.set(random, draw);
    generators.set(draw, { random, streams: null, name: null });
  }
  return draw;
}

export class ClassicRandom {
  constructor(seed = 0) {
    this._state = seed >>> 0;
    randomRevision = (randomRevision + 1) >>> 0;
  }

  get state() { return this._state; }
  set state(value) {
    this._state = value;
    randomRevision = (randomRevision + 1) >>> 0;
  }

  // A raw 32-bit draw, as an unsigned integer.
  next() {
    const a = (Math.imul(this.state, MUL) + ADD) >>> 0;
    const b = (Math.imul(a, MUL) + ADD) >>> 0;
    this.state = b;
    return ((b & 0xffff0000) | (a >>> 16)) >>> 0;
  }

  // [0, 1) in float32 steps.
  uniform() {
    return (this.next() >>> 9) / 8388608;
  }

  // An inclusive integer in [lo, hi], with the game's float32 rounding.
  int(lo, hi) {
    if (!(hi > lo)) return lo;
    const u = this.uniform();
    return Math.trunc(f32(f32(u * f32(hi - lo + 1)) + lo));
  }

  // A float in [lo, hi): lo + U * (hi - lo), in float32. Call sites whose
  // original arithmetic differs should spell theirs out with uniform().
  range(lo, hi) {
    const u = this.uniform();
    return f32(f32(lo) + f32(u * f32(hi - lo)));
  }

  // A raw draw modulo n (n > 0), unsigned, as the roster and route pickers
  // use it.
  mod(n) {
    return n > 0 ? this.next() % n : 0;
  }

  // Save and restore, for replays and tests.
  get seed() {
    return this.state;
  }

  set seed(value) {
    this.state = value >>> 0;
  }
}

// Independent seeded streams for systems that the original did not share
// with AI decisions (presentation, test harnesses). Each name gets its own
// ClassicRandom seeded from the match seed and the name, so adding a stream
// never shifts another's sequence.
export class RandomStreams {
  constructor(seed = 0) {
    this.seed = seed >>> 0;
    this.streams = new Map();
    this.draws = new Map();
  }

  get(name) {
    let stream = this.streams.get(name);
    if (!stream) {
      stream = new ClassicRandom(mix(this.seed, name));
      this.streams.set(name, stream);
      // Native phases retain this same generator binding. Update it on first
      // use, rather than looking up a stream name at every aimer boundary.
      const draw = this.draws.get(name);
      if (draw) generators.get(draw).random = stream;
    }
    return stream;
  }

  // createGame's named draw callbacks stay lazy: preparing a native binding
  // must not insert a stream into the world-hash's observed first-use order.
  draw(name) {
    let draw = this.draws.get(name);
    if (!draw) {
      const binding = { random: this.streams.get(name) ?? null, streams: this, name };
      draw = () => (binding.random ?? this.get(name)).uniform();
      generators.set(draw, binding);
      this.draws.set(name, draw);
    }
    return draw;
  }

  initialState(name) { return mix(this.seed, name); }

  reset(seed = this.seed) {
    this.seed = seed >>> 0;
    this.streams.clear();
    randomRevision = (randomRevision + 1) >>> 0;
    // Existing draw callbacks survive reset, while their old generators do
    // not. The next use still establishes the new registry insertion order.
    for (const draw of this.draws.values()) generators.get(draw).random = null;
  }
}

// The draws that still come from Math.random, each through a name. A legacy
// stream is Math.random itself: its values, in the order its call sites draw
// them. The name is for the world's determinism census
// (tests/determinism-census.test.js), which tells these draws from stray
// ones inside a world step, and so each can move to a seeded stream on its
// own. Every gameplay draw in a world step comes from a seeded generator or
// from one of these.
let legacyTap = null;
export function legacyRandom(name) {
  return () => {
    if (legacyTap) legacyTap(name);
    return Math.random();
  };
}

// The census's hook: tap(name) runs before each legacy draw (null: none).
// Returns the hook it replaces.
export function tapLegacyRandom(tap) {
  const previous = legacyTap;
  legacyTap = tap;
  return previous;
}

// FNV-1a over the name, folded with the seed.
function mix(seed, name) {
  let h = 0x811c9dc5 ^ seed;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
