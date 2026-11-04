// Replicas can arrive every simulation tick (worker) or every third tick
// (20 Hz server). Draw between actual samples, including one future sample.
const nativeModels = new WeakMap(), weakGet = WeakMap.prototype.get, weakSet = WeakMap.prototype.set, invoke = Reflect.apply;
const nativeModel = value => invoke(weakGet, nativeModels, [value]);
export class SnapshotInterval {
  constructor({ native = null } = {}) {
    if (native) invoke(weakSet, nativeModels, [this, native]);
    this.reset();
  }
  reset() {
    if (nativeModel(this)?.intervalReset(this)) return;
    this.previous = null; this.current = null;
  }
  needsSample(clock) {
    const result = nativeModel(this)?.needsSample(this, clock);
    return result ? result.value : this.current === null || this.current <= clock;
  }
  apply(tick) {
    if (nativeModel(this)?.intervalApply(this, tick)) return;
    this.previous = this.current ?? tick - 1;
    this.current = tick;
  }
  alpha(clock) {
    const result = nativeModel(this)?.alpha(this, clock);
    if (result) return result.value;
    if (this.current === null) return 1;
    return Math.max(0, Math.min(1, (clock - this.previous) / Math.max(1, this.current - this.previous)));
  }
}

export function renderedViewTick(clock, latest, { native = null } = {}) {
  const result = native?.renderedViewTick(clock, latest);
  if (result) return result.value;
  return Number.isFinite(clock) && Number.isFinite(latest) ? Math.max(0, Math.min(Math.floor(latest), Math.floor(clock))) : 0;
}
