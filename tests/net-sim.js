// Deterministic clock, timers and lossy network for BFE-UDP/1 tests. One
// event queue drives both connections, so loss, delay, jitter, reordering
// and duplication replay identically from a seed.
import { randomBytes } from 'node:crypto';
import { Aead } from '../src/net/crypto.js';
import { Connection } from '../src/net/connection.js';

export function seeded(seed = 1) {
  let state = seed >>> 0 || 1;
  return () => { state ^= state << 13; state >>>= 0; state ^= state >>> 17; state ^= state << 5; state >>>= 0; return state / 0x100000000; };
}

export class Simulation {
  constructor() { this.time = 0; this.sequence = 0; this.heap = []; this.micro = []; }
  now = () => this.time;
  setTimer = (fn, ms) => this.at(this.time + Math.max(0, ms), fn);
  clearTimer = event => { if (event) event.cancelled = true; };
  schedule = fn => { this.micro.push(fn); };
  at(time, fn) {
    const event = { time, sequence: this.sequence++, fn, cancelled: false }, heap = this.heap;
    heap.push(event);
    for (let i = heap.length - 1; i > 0;) {
      const parent = (i - 1) >> 1;
      if (this.before(heap[parent], heap[i])) break;
      [heap[parent], heap[i]] = [heap[i], heap[parent]]; i = parent;
    }
    return event;
  }
  before(a, b) { return a.time < b.time || (a.time === b.time && a.sequence < b.sequence); }
  pop() {
    const heap = this.heap, top = heap[0], last = heap.pop();
    if (heap.length) {
      heap[0] = last;
      for (let i = 0; ;) {
        const left = 2 * i + 1, right = left + 1;
        let smallest = i;
        if (left < heap.length && this.before(heap[left], heap[smallest])) smallest = left;
        if (right < heap.length && this.before(heap[right], heap[smallest])) smallest = right;
        if (smallest === i) break;
        [heap[smallest], heap[i]] = [heap[i], heap[smallest]]; i = smallest;
      }
    }
    return top;
  }
  drain() { while (this.micro.length) this.micro.shift()(); }
  run(until) {
    this.drain();
    while (this.heap.length && this.heap[0].time <= until) {
      const event = this.pop();
      if (event.cancelled) continue;
      this.time = event.time;
      event.fn();
      this.drain();
    }
    this.time = Math.max(this.time, until);
  }
  // Runs until `condition()` holds or `limit` ms of simulated time pass.
  until(condition, limit = 60000, step = 1) {
    const end = this.time + limit;
    while (!condition() && this.time < end) this.run(this.time + step);
    return condition();
  }
}

// A one-way link: each datagram may be dropped, delayed with jitter
// (reordering when jitter exceeds the spacing) or duplicated.
export class Link {
  constructor(sim, { loss = 0, delay = 10, jitter = 0, duplicate = 0, seed = 1, drop = null } = {}) {
    Object.assign(this, { sim, loss, delay, jitter, duplicate, drop });
    this.random = seeded(seed); this.sent = 0; this.dropped = 0; this.bytes = 0; this.blocked = false;
  }
  send(packet, deliver) {
    this.sent++; this.bytes += packet.length;
    if (this.blocked || (this.drop && this.drop(packet, this.sent)) || this.random() < this.loss) { this.dropped++; return; }
    const copies = this.random() < this.duplicate ? 2 : 1;
    for (let i = 0; i < copies; i++) {
      const latency = Math.max(0, this.delay + (this.random() * 2 - 1) * this.jitter);
      const bytes = Buffer.from(packet);
      this.sim.at(this.sim.time + latency, () => deliver(bytes));
    }
  }
}

// Two connected sessions over simulated links (keys negotiated out of band;
// the handshake has its own tests).
export function connectedPair(sim, { up = {}, down = {}, client: clientOptions = {}, server: serverOptions = {} } = {}) {
  const toServer = new Link(sim, { seed: 11, ...up }), toClient = new Link(sim, { seed: 23, ...down });
  const a = randomBytes(32), b = randomBytes(32);
  const received = { client: [], server: [] }, closed = {};
  const common = { now: sim.now, setTimer: sim.setTimer, clearTimer: sim.clearTimer, schedule: sim.schedule };
  let client, server;
  client = new Connection({ role: 'client', localSession: 1, remoteSession: 2, send: new Aead(a), receive: new Aead(b), ...common,
    transmit: packet => toServer.send(packet, bytes => server.receive(bytes, serverPath)),
    onMessage: (lane, payload) => received.client.push([lane, payload, sim.time]), onClose: info => { closed.client = info; }, path: { key: 'server' }, ...clientOptions });
  const serverPath = { key: 'client' };
  server = new Connection({ role: 'server', localSession: 2, remoteSession: 1, send: new Aead(b), receive: new Aead(a), ...common,
    transmit: packet => toClient.send(packet, bytes => client.receive(bytes, { key: 'server' })),
    onMessage: (lane, payload) => received.server.push([lane, payload, sim.time]), onClose: info => { closed.server = info; }, path: { key: 'client' }, ...serverOptions });
  return { client, server, received, closed, toServer, toClient };
}
