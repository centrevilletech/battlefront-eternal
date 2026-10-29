// Deterministic virtual-time duplex link for exercising the actual protocol.
// Milliseconds throughout. Reliable delivery models ordered retransmission;
// state delivery intentionally permits loss, duplication and reordering.
export class NetworkSimulator {
  constructor({ seed = 1, rtt = 0, jitter = 0, loss = 0, duplicate = 0,
    reorder = 0, retry = 100, capacity = 4096 } = {}) {
    for (const [name, value] of Object.entries({ rtt, jitter, retry }))
      if (!Number.isFinite(value) || value < 0) throw new Error(`invalid ${name}`);
    for (const [name, value] of Object.entries({ loss, duplicate, reorder }))
      if (!Number.isFinite(value) || value < 0 || value > 1) throw new Error(`invalid ${name}`);
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error('invalid capacity');
    Object.assign(this, { rtt, jitter, loss, duplicate, reorder, retry: Math.max(1, retry), capacity });
    this.seed = seed >>> 0 || 1; this.now = 0; this.epoch = 1; this.connected = true;
    this.queue = []; this.serial = 0; this.receivers = new Map(); this.ordered = new Map();
    this.stats = { sent: 0, delivered: 0, lost: 0, duplicated: 0, retransmitted: 0, rejected: 0 };
  }
  random() {
    let x = this.seed; x ^= x << 13; x ^= x >>> 17; x ^= x << 5;
    this.seed = x >>> 0; return this.seed / 4294967296;
  }
  receive(side, callback) {
    if (!['client', 'server'].includes(side) || typeof callback !== 'function') throw new Error('invalid receiver');
    this.receivers.set(side, callback);
  }
  send(from, payload, { reliable = false, channel = reliable ? 'events' : 'state' } = {}) {
    if (!['client', 'server'].includes(from)) throw new Error('invalid sender');
    if (!this.connected || this.queue.length >= this.capacity) { this.stats.rejected++; return false; }
    this.stats.sent++;
    const copy = structuredClone(payload), key = `${from}:${channel}`;
    let at = this.now + Math.max(0, this.rtt / 2 + (this.random() * 2 - 1) * this.jitter);
    if (this.random() < this.reorder) at += this.rtt + this.jitter + 1;
    const lost = this.random() < this.loss;
    if (lost && !reliable) { this.stats.lost++; return true; }
    // Retry at delivery time, so 100% loss stays bounded rather than spinning.
    if (reliable) at = Math.max(at, this.ordered.get(key)?.at ?? 0);
    const item = { at, serial: ++this.serial, epoch: this.epoch, to: from === 'client' ? 'server' : 'client',
      payload: copy, reliable, lost, previous: reliable ? this.ordered.get(key) : null, done: false };
    this.queue.push(item);
    if (reliable) this.ordered.set(key, item);
    if (!reliable && this.random() < this.duplicate && this.queue.length < this.capacity) {
      this.queue.push({ ...item, at: at + this.random() * (this.jitter + 1), serial: ++this.serial, payload: structuredClone(copy) });
      this.stats.duplicated++;
    }
    return true;
  }
  advance(milliseconds) {
    if (!Number.isFinite(milliseconds) || milliseconds < 0) throw new Error('invalid time step');
    const until = this.now + milliseconds;
    for (;;) {
      this.queue.sort((a, b) => a.at - b.at || a.serial - b.serial);
      const item = this.queue[0];
      if (!item || item.at > until) break;
      this.queue.shift(); this.now = item.at;
      if (item.epoch !== this.epoch || !this.connected) continue;
      if (item.previous && !item.previous.done) {
        item.at = Math.max(item.at + 1, item.previous.at + .001); this.queue.push(item); continue;
      }
      if (item.lost) {
        this.stats.lost++; this.stats.retransmitted++;
        item.at += this.retry; item.lost = this.random() < this.loss; this.queue.push(item); continue;
      }
      item.done = true; item.previous = null; this.stats.delivered++;
      this.receivers.get(item.to)?.(item.payload, { now: this.now, epoch: item.epoch });
    }
    this.now = until;
  }
  disconnect() {
    this.connected = false; this.epoch++; this.queue.length = 0; this.ordered.clear();
  }
  reconnect() { this.connected = true; return this.epoch; }
}
