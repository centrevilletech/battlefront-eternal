// Replicas can arrive every simulation tick (worker) or every third tick
// (20 Hz server). Draw between actual samples, including one future sample.
export class SnapshotInterval {
  constructor() { this.reset(); }
  reset() { this.previous = null; this.current = null; }
  needsSample(clock) { return this.current === null || this.current <= clock; }
  apply(tick) {
    this.previous = this.current ?? tick - 1;
    this.current = tick;
  }
  alpha(clock) {
    if (this.current === null) return 1;
    return Math.max(0, Math.min(1, (clock - this.previous) / Math.max(1, this.current - this.previous)));
  }
}

export function renderedViewTick(clock, latest) {
  return Number.isFinite(clock) && Number.isFinite(latest) ? Math.max(0, Math.min(Math.floor(latest), Math.floor(clock))) : 0;
}
