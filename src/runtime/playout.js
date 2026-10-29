// The page's playout clock for a streamed world (a worker's today, a
// server's later): the tick it shows, held behind the newest tick that has
// arrived by a delay measured from how unevenly packets arrive, so bodies
// move evenly however the packets bunch up.
//
// - The delay is a high percentile (95th) of recent arrival gaps, plus a
//   margin, within [minDelay, maxDelay] ticks. One isolated stall must not
//   widen the cushion for the whole window the way the maximum did. A page drawing at 20 fps
//   sees packets three at a time, so its delay settles around 4 to 5. Packets that arrive in bursts
//   (a tick that ran long, a garbage-collection pause) widen it; a calm
//   stream lets it shrink again.
// - The clock runs at wall speed, trimmed by at most `trim` (10%) toward
//   its target. It never passes the newest tick: starved, it waits.
// - Far from its target (a stall, a resume, the first packets), it jumps.
export class PlayoutClock {
  constructor({ hz = 60, minDelay = 2, maxDelay = 10, margin = 1, window = 120, trim = 0.1, snap = 12 } = {}) {
    this.hz = hz; this.minDelay = minDelay; this.maxDelay = maxDelay; this.margin = margin;
    this.window = window; this.trim = trim; this.snap = snap;
    this.reset();
  }
  reset() {
    this.clock = 0; this.latest = 0; this.delay = this.minDelay; this.rate = 1;
    this.gaps = new Float64Array(this.window); this.gapAt = 0; this.gapCount = 0;
    this.arrivedAt = null; this.arrivedTick = 0; this.started = false; this.waits = 0; this.measuredKey = -1;
  }
  // A packet for `tick` arrived at wall time `now` (seconds). `interval`
  // is the stream's snapshot spacing in ticks (1 for per-tick streams).
  arrive(tick, now, interval = 1) {
    if (tick > this.latest) this.latest = tick;
    if (this.arrivedAt !== null && tick > this.arrivedTick) {
      // How late this arrival ran against one interval after the previous
      // one. A missing snapshot makes the next arrival a whole interval
      // late by this measure, so sustained loss widens the cushion and
      // holds a spare interval in the buffer. (The old formula subtracted
      // the ticks the packet jumped, so a lost snapshot measured on time.)
      const gap = Math.max(0, (now - this.arrivedAt) * this.hz - interval + 1);
      this.gaps[this.gapAt] = gap; this.gapAt = (this.gapAt + 1) % this.window;
      if (this.gapCount < this.window) this.gapCount++;
    }
    this.arrivedAt = now; this.arrivedTick = Math.max(this.arrivedTick, tick);
  }
  // The delay to hold: the 95th percentile recent arrival gap plus a
  // margin, so sustained jitter widens the cushion and one stall does not.
  measure() {
    const key = this.gapCount * 4096 + this.gapAt;
    if (key !== this.measuredKey) {
      this.measuredKey = key;
      let gap = 0;
      if (this.gapCount) {
        const sorted = this.gaps.slice(0, this.gapCount).sort();
        gap = sorted[Math.min(this.gapCount - 1, Math.floor(this.gapCount * 0.95))];
      }
      this.delay = Math.min(this.maxDelay, Math.max(this.minDelay, gap + this.margin));
    }
    return this.delay;
  }
  // Wall seconds passed: the clock moves on. Returns the tick to show
  // (fractional: the integer part is the newest tick applied, the fraction
  // the way to the next).
  advance(dt) {
    this.measure();
    const target = this.latest - this.delay;
    if (!this.started) {
      if (this.latest <= 0) return this.clock;
      this.started = true; this.clock = Math.max(0, target); this.rate = 1;
      return this.clock;
    }
    const error = target - this.clock;
    if (Math.abs(error) > this.snap) { this.clock = Math.max(0, target); this.rate = 1; return this.clock; }
    // Trim toward the target: the whole error over about a second.
    this.rate = Math.min(1 + this.trim, Math.max(1 - this.trim, 1 + error / this.hz));
    let next = this.clock + dt * this.hz * this.rate;
    if (next > this.latest) { next = this.latest; this.waits++; }
    this.clock = Math.max(this.clock, next);
    return this.clock;
  }
  // The newest tick the page should have applied for the clock's position.
  get shownTick() { return Math.floor(this.clock) + 1; }
}
