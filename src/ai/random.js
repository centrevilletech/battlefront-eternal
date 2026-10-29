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

export class ClassicRandom {
  constructor(seed = 0) {
    this.state = seed >>> 0;
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
  }

  get(name) {
    let stream = this.streams.get(name);
    if (!stream) {
      stream = new ClassicRandom(mix(this.seed, name));
      this.streams.set(name, stream);
    }
    return stream;
  }

  reset(seed = this.seed) {
    this.seed = seed >>> 0;
    this.streams.clear();
  }
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
