// Tick-and-epoch-stamped history of snapshots (or per-client own states):
// the newest `capacity` ticks stored, whatever their spacing. Snapshots are
// taken every publication interval (every second tick online), so a ring
// indexed by tick % capacity held only capacity / interval of them; this one
// holds capacity snapshots. A lookup whose stored epoch does not match is a
// miss. Storing a tick older than every held one in a full ring keeps
// nothing, so the ring always holds the newest ticks it was given: a host
// and a client that keep the same capacity agree on what a baseline can be.
// onEvict(value, ring): called with each value the ring lets go to make
// room, the ring already without it.
export class SnapshotRing {
  constructor(capacity = 128, onEvict = null) {
    if (!Number.isSafeInteger(capacity) || capacity < 1) throw Error('invalid snapshot ring capacity');
    this.capacity = capacity; this.onEvict = onEvict;
    this.entries = new Map(); // tick -> { tick, epoch, value }
    this.ticks = []; // held ticks, ascending
    this.newestTick = -1;
  }
  get size() { return this.ticks.length; }
  get oldestTick() { return this.ticks.length ? this.ticks[0] : -1; }
  set(tick, epoch, value) {
    if (!Number.isSafeInteger(tick)) throw Error('invalid snapshot ring tick');
    const entry = this.entries.get(tick);
    if (entry) { entry.epoch = epoch; entry.value = value; return; }
    const ticks = this.ticks;
    if (ticks.length >= this.capacity) {
      if (tick < ticks[0]) return;
      const oldest = ticks.shift(), evicted = this.entries.get(oldest);
      this.entries.delete(oldest);
      this.onEvict?.(evicted.value, this);
    }
    // Ticks arrive in order on the host; a late snapshot on a client is the
    // rare out-of-order insert.
    let at = ticks.length;
    while (at > 0 && ticks[at - 1] > tick) at--;
    ticks.splice(at, 0, tick);
    this.entries.set(tick, { tick, epoch, value });
    if (tick > this.newestTick) this.newestTick = tick;
  }
  get(tick, epoch) {
    const entry = this.entries.get(tick);
    return entry && (epoch === undefined || entry.epoch === epoch) ? entry.value : undefined;
  }
  has(tick, epoch) { return this.get(tick, epoch) !== undefined; }
  clear() { this.entries.clear(); this.ticks.length = 0; this.newestTick = -1; }
}
