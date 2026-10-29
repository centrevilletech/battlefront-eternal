// One gameplay clock. Rendering only supplies elapsed time; integer ticks
// determine gameplay time. A task has bounded work and retains catch-up debt.
//
// Two optional limits keep a machine that cannot keep up from spiralling
// (each catch-up batch making the next frame later still):
// - budget: wall-clock seconds of ticks per advance. The batch shrinks to
//   what the measured cost of a tick fits in, never below one tick.
// - maxDebt: ticks of debt kept. Beyond it the excess is dropped: offline,
//   the world runs slower than real time instead of falling further behind.
//   `dropped` counts every tick given up this way.
// Neither changes the tick sequence or a tick's length, only how many run per
// advance, so gameplay results are the same at any speed. An advance with
// `limited` false ignores both (tools and replays that must run every tick).
const now = () => (globalThis.performance ? performance.now() : Date.now());

export class FixedClock {
  constructor({ hz = 60, maxSteps = 8, budget = Infinity, maxDebt = Infinity } = {}) {
    if (!(hz > 0) || !Number.isInteger(maxSteps) || maxSteps < 1 || !(budget > 0) || !(maxDebt >= 0)) throw new Error('invalid fixed clock');
    this.hz = hz; this.dt = 1 / hz; this.maxSteps = maxSteps;
    this.budget = budget; this.maxDebt = maxDebt;
    this.reset();
  }
  reset() {
    this.tick = 0; this.elapsed = 0; this.compensation = 0;
    this.debt = 0; this.alpha = 0; this.overloads = 0;
    this.dropped = 0; this.stepCost = 0;
  }
  advance(seconds, step, limited = true) {
    if (!Number.isFinite(seconds) || seconds < 0) throw new Error('invalid elapsed time');
    const value = seconds - this.compensation;
    const sum = this.elapsed + value;
    this.compensation = (sum - this.elapsed) - value;
    this.elapsed = sum;
    const target = Math.floor(this.elapsed * this.hz + 1e-8);
    const budget = limited ? this.budget : Infinity;
    let limit = this.maxSteps;
    if (budget !== Infinity && this.stepCost > 0) limit = Math.max(1, Math.min(limit, Math.floor(budget * 1000 / this.stepCost)));
    const count = Math.min(Math.max(0, target - this.tick), limit);
    const batchStart = budget !== Infinity ? now() : 0;
    let ran = 0;
    while (ran < count) {
      const start = budget !== Infinity ? now() : 0;
      step(this.dt, this.tick + 1);
      this.tick++; ran++;
      if (budget !== Infinity) {
        // Smoothed milliseconds per tick; and the batch also ends once it
        // has used the budget, before the cost is known.
        const end = now(), cost = end - start;
        this.stepCost = this.stepCost ? this.stepCost + (cost - this.stepCost) * 0.1 : cost;
        if (end - batchStart >= budget * 1000) break;
      }
    }
    this.debt = Math.max(0, target - this.tick);
    if (limited && this.debt > this.maxDebt) {
      const drop = this.debt - this.maxDebt;
      this.elapsed -= drop / this.hz;
      this.compensation = 0;
      this.dropped += drop;
      this.debt = this.maxDebt;
    }
    this.alpha = this.debt ? 1 : Math.max(0, Math.min(1, this.elapsed * this.hz - this.tick));
    if (this.debt) this.overloads++;
    return ran;
  }
  // Explicit suspend/resume rebasing discards wall-clock time, never ticks.
  rebase() { this.elapsed = this.tick / this.hz; this.compensation = this.debt = this.alpha = 0; }
  get time() { return this.tick / this.hz; }
}
