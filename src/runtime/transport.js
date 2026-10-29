// Transfer ownership is explicit. Snapshot storage never grows while the
// consumer is stalled: simulation may skip a transform publication instead.
export class TransferPool {
  constructor(bytes, count = 3, epoch = 1) {
    this.bytes = bytes; this.epoch = epoch;
    this.free = Array.from({length:count},(_,slot)=>({slot,buffer:new ArrayBuffer(bytes)}));
    this.inFlight = new Set(); this.skipped = 0;
  }
  acquire() {
    const item = this.free.pop();
    if (!item) { this.skipped++; return null; }
    this.inFlight.add(item.slot); return item;
  }
  recycle({epoch, slot, buffer}) {
    if (epoch !== this.epoch || !(buffer instanceof ArrayBuffer) || buffer.byteLength !== this.bytes || !this.inFlight.delete(slot)) return false;
    this.free.push({slot,buffer}); return true;
  }
}

import { humanCommands, HUMAN_COMMAND_LIMIT } from './commands.js';

// Reliable events have separate backpressure from replaceable transforms.
// One bounded batch is in flight; overflow is an explicit host fault.
export class ReliableOutbox {
  constructor(capacity = 2048, batchSize = 128) {
    this.capacity = capacity; this.batchSize = batchSize; this.pending = [];
    this.sequence = 0; this.acked = 0; this.sent = 0;
  }
  push(value) {
    if (this.pending.length >= this.capacity) throw new Error('reliable event backlog exceeded');
    this.pending.push({sequence:++this.sequence,value});
  }
  batch() {
    if (this.sent !== this.acked || !this.pending.length) return null;
    const batch = this.pending.slice(0,this.batchSize);
    this.sent = batch.at(-1).sequence; return batch;
  }
  ack(sequence) {
    if (!Number.isSafeInteger(sequence) || sequence <= this.acked || sequence > this.sent) return false;
    this.pending.splice(0,sequence-this.acked); this.acked = sequence; return true;
  }
}

export class InputMailbox {
  constructor(epoch = 1, capacity = 256) {
    this.epoch = epoch; this.capacity = capacity; this.received = 0; this.ack = 0; this.ackTick = 0; this.discarded = 0;
    this.queue = []; this.down = []; this.generation = 0; this.late = 0; this.view = {};
    this.lookTotal = [0, 0];
    this.stateTick = 0;
  }
  push(command) {
    if (command.epoch !== this.epoch || !Number.isSafeInteger(command.sequence) || command.sequence <= this.received) return false;
    if (!Number.isSafeInteger(command.tick) || command.tick < 1 || !Number.isSafeInteger(command.generation)) throw new Error('invalid input stamp');
    if (!Array.isArray(command.down) || command.down.length > 64 || !command.down.every(x=>typeof x === 'string') ||
        !Array.isArray(command.actions) || command.actions.length > 2048 || !command.actions.every(x=>typeof x === 'string') ||
        !Array.isArray(command.look) || command.look.length > 2048 || !command.look.every(p=>Array.isArray(p) && p.length===2 && p.every(Number.isFinite))) throw new Error('invalid input payload');
    if (command.lookTotal !== undefined && (!Array.isArray(command.lookTotal) || command.lookTotal.length !== 2 ||
      !command.lookTotal.every(n => Number.isFinite(n) && Math.abs(n) <= 2 ** 48))) throw new Error('invalid cumulative look');
    for (const key of ['mouseScale','aspect']) if (command[key] !== undefined && (!Number.isFinite(command[key]) || command[key] <= 0)) throw new Error('invalid view input');
    for (const key of ['invertMouse','firstPerson']) if (command[key] !== undefined && typeof command[key] !== 'boolean') throw new Error('invalid view input');
    if (command.viewTick !== undefined && (!Number.isFinite(command.viewTick) || command.viewTick < 0 || command.viewTick > 0xffffffff)) throw new Error('invalid view tick');
    const fs=command.flightSettings;
    if (fs !== undefined && (!fs || typeof fs !== 'object' || Array.isArray(fs) ||
      fs.sensitivity !== undefined && (!Number.isFinite(fs.sensitivity) || fs.sensitivity<=0 || fs.sensitivity>10) ||
      fs.invert !== undefined && typeof fs.invert !== 'boolean' ||
      fs.curve !== undefined && (!Number.isFinite(fs.curve) || fs.curve<.1 || fs.curve>4) ||
      fs.bindings !== undefined && (!fs.bindings || typeof fs.bindings !== 'object' || Array.isArray(fs.bindings) || Object.keys(fs.bindings).length>32 ||
        !Object.entries(fs.bindings).every(([k,v])=>k.length<=64 && Array.isArray(v) && v.length<=8 && v.every(x=>typeof x==='string' && x.length<=64))))) throw new Error('invalid flight settings');
    const pad=command.gamepad;
    if (pad != null && (!Array.isArray(pad.axes) || pad.axes.length>16 || !pad.axes.every(x=>Number.isFinite(x)&&Math.abs(x)<=1) ||
      !Array.isArray(pad.buttons) || pad.buttons.length>32 || !pad.buttons.every(x=>x && typeof x.pressed==='boolean'))) throw new Error('invalid gamepad input');
    // Trigger edges (WorkerHost.edges): the world names a press's rounds by its id.
    if (command.fire !== undefined && (!Array.isArray(command.fire) || command.fire.length > 16 || !command.fire.every(edge => edge && Number.isSafeInteger(edge.id) && edge.id > 0 &&
      (edge.button === 0 || edge.button === 1) && typeof edge.pressed === 'boolean'))) throw new Error('invalid trigger edges');
    // The spawn screen's choices: validated, and only their values kept.
    const human = humanCommands(command.human);
    if (this.queue.length >= this.capacity) throw new Error('input mailbox overflow');
    if (this.queue.length && command.tick < this.queue.at(-1).tick) throw new Error('input tick moved backwards');
    this.received = command.sequence; this.queue.push(human.length ? {...command,human} : command); return true;
  }
  take(tick, generation) {
    if (generation !== this.generation) { this.down = []; this.generation = generation; this.lookTotal = [0, 0]; this.stateTick = 0; }
    const actions = [], look = [], human = [], fire = [];
    while (this.queue.length && this.queue[0].tick <= tick) {
      const command = this.queue.shift();
      if (command.generation !== generation) { this.discarded = Math.max(this.discarded, command.sequence); continue; }
      this.ack = command.sequence; this.ackTick = tick;
      this.stateTick = command.tick;
      if (command.tick < tick) this.late++;
      this.down = command.down; actions.push(...command.actions);
      if (command.lookTotal) {
        const dx = command.lookTotal[0] - this.lookTotal[0], dy = command.lookTotal[1] - this.lookTotal[1];
        if (dx || dy) look.push([dx, dy]);
        this.lookTotal[0] = command.lookTotal[0]; this.lookTotal[1] = command.lookTotal[1];
      } else look.push(...command.look);
      if (command.human) human.push(...command.human);
      if (command.fire) for (const edge of command.fire) fire.push({id:edge.id, button:edge.button, pressed:edge.pressed});
      for (const key of ['mouseScale','invertMouse','firstPerson','aspect','viewTick','flightSettings','gamepad']) if (command[key] !== undefined) this.view[key] = command[key];
      if (actions.length > 2048 || look.length > 2048 || human.length > HUMAN_COMMAND_LIMIT * this.capacity) throw new Error('input batch overflow');
    }
    const command = {epoch:this.epoch,sequence:tick,tick,generation,down:this.down,actions,look,...this.view};
    if (human.length) command.human = human;
    if (fire.length) command.fire = fire;
    return command;
  }
  clear() { this.discarded = Math.max(this.discarded, this.queue.at(-1)?.sequence || 0); this.queue.length = 0; this.down = []; this.stateTick = 0; delete this.view.gamepad; delete this.view.viewTick; }
}

export const SNAPSHOT_CAPACITY = 512;
export const SNAPSHOT_STRIDE = 16;
export const SNAPSHOT_HEADER = 8;
export const SNAPSHOT_BYTES = (SNAPSHOT_HEADER + SNAPSHOT_CAPACITY * SNAPSHOT_STRIDE) * 8;
export function encodeSnapshot(buffer, game, epoch, ack) {
  const data = new Float64Array(buffer), all = game.units.all();
  let count = 0;
  for (const unit of all) {
    if (unit.removed) continue;
    if (count === SNAPSHOT_CAPACITY) throw new Error('snapshot entity capacity exceeded');
    const s = unit.soldier, v = unit.velocity || s?.velocity;
    const at = SNAPSHOT_HEADER + count++ * SNAPSHOT_STRIDE;
    data.set([unit.id,unit.generation,unit.team,Number(unit.alive),unit.position.x,unit.position.y,unit.position.z,
      v?.x || 0,v?.y || 0,v?.z || 0,s?.aimYaw || 0,s?.aimPitch || 0,unit.health || 0,
      Number(Boolean(unit.seat)),Number(Boolean(s?.grounded)),s?.stateTime || 0],at);
  }
  data.set([epoch,game.worldClock.tick,ack,count,game.playerUnit.id,game.playerUnit.generation,game.worldClock.time,0],0);
  return count;
}

// A shared ring for a streaming world's packets (one binary buffer per
// tick, packet.js), used when the page and the worker can share memory
// (cross-origin isolation in a browser; worker threads under Node). The
// worker appends each packet behind a length and tick header and then
// publishes its position with one atomic store; the page copies out every
// packet published since its last read, in order, with no wait on either
// side. Packets are deltas, so the page must read every one: a page that
// falls a whole ring behind is lapped, skips to the newest position and
// asks the worker for a keyframe (the next packet carries everything).
// The control words are 64-bit byte counts: `reserved` (stored before a
// packet is written, so a reader can tell its copy was not overwritten
// while it was made) and `committed` (stored after).
export const RING_BYTES = 4 << 20;
const RING_CONTROL = 64, SKIP = 0xffffffff;
export const LAPPED = Symbol('lapped');
export class SharedRing {
  static create(bytes = RING_BYTES) { return new SharedArrayBuffer(RING_CONTROL + bytes); }
  constructor(buffer) {
    this.buffer = buffer;
    this.control = new BigInt64Array(buffer, 0, 2); // committed, reserved
    this.words = new Uint32Array(buffer, RING_CONTROL);
    this.bytes = new Uint8Array(buffer, RING_CONTROL);
    this.capacity = this.bytes.length;
    this.written = 0; this.read = 0; this.laps = 0; this.published = 0;
  }
  get committed() { return Number(Atomics.load(this.control, 0)); }
  get reserved() { return Number(Atomics.load(this.control, 1)); }
  // Worker side: `packet` (an ArrayBuffer) for `tick`. Never waits.
  publish(tick, packet) {
    const size = packet.byteLength, need = 8 + ((size + 7) & ~7);
    if (need > this.capacity) throw new Error(`a ${size}-byte packet does not fit the shared ring`);
    let at = this.written % this.capacity, total = need;
    const wrap = at + need > this.capacity;
    if (wrap) total += this.capacity - at;
    Atomics.store(this.control, 1, BigInt(this.written + total));
    if (wrap) { this.words[at >> 2] = SKIP; at = 0; }
    this.words[at >> 2] = size; this.words[(at >> 2) + 1] = tick >>> 0;
    this.bytes.set(new Uint8Array(packet), at + 8);
    this.written += total; this.published++;
    Atomics.store(this.control, 0, BigInt(this.written));
  }
  // Page side: the next packet as { tick, buffer } (its own ArrayBuffer),
  // null when none has arrived, or LAPPED.
  take() {
    for (;;) {
      const committed = Number(Atomics.load(this.control, 0));
      if (committed === this.read) return null;
      if (Number(Atomics.load(this.control, 1)) - this.read > this.capacity) return this.lapped(committed);
      const at = this.read % this.capacity, size = this.words[at >> 2];
      if (size === SKIP) { this.read += this.capacity - at; continue; }
      if (size > this.capacity - at - 8) return this.lapped(committed);
      const tick = this.words[(at >> 2) + 1], start = this.read;
      const buffer = new ArrayBuffer(size);
      new Uint8Array(buffer).set(this.bytes.subarray(at + 8, at + 8 + size));
      // Overwritten while the copy was made: the worker reserved past us.
      if (Number(Atomics.load(this.control, 1)) - start > this.capacity) return this.lapped(Number(Atomics.load(this.control, 0)));
      this.read += 8 + ((size + 7) & ~7);
      return { tick, buffer };
    }
  }
  lapped(committed) { this.read = committed; this.laps++; return LAPPED; }
}
