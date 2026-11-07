import { KIND_OWNER_SNAPSHOT, KIND_TABLE } from '../gen/index.js';
import { ordnanceTests } from './ordnance-sim.js';
import { prepareOrdnanceOwner, restoreOrdnanceOwner } from '../../ordnance-owner.js';

// OrdnanceOwner snapshots use the bridge's existing Identity for frozen plain
// objects. They are immutable metadata rows, rather than pooled body mirrors.
export class F3Owners {
  constructor(phase) {
    this.phase = phase;
    this.bridge = phase.bridge;
    this.sent = new WeakSet();
    this.one = [null];
    this.frames = [];
    this.bridge.identify({ tests:ordnanceTests });
  }

  // Remote class preparation reserves a full burst of launch identities.
  // The extra half covers every network droid slot before a later outer
  // return can grow the pool. Neither native birth nor its binding allocates.
  reserve(count = 8192) {
    const refs = this.bridge.refs[KIND_OWNER_SNAPSHOT];
    refs.reserve(count);
    while (this.frames.length < count) {
      const frame = prepareOrdnanceOwner();
      this.frames.push(frame);refs.prepare(frame.snapshot);this.sent.add(frame.snapshot);
    }
  }
  reserveFuture() {
    if (!this.frames.length || this.phase.active) return;
    const used = this.bridge.refs[KIND_OWNER_SNAPSHOT].nextSlot;
    if (used + 4096 > this.frames.length) this.reserve(Math.max(this.frames.length * 2,used + 4096));
  }

  // OrdnanceSim.fire can receive a JS-created launch snapshot outside the
  // owned phase. Register that metadata once, without importing its source body.
  prepare(owner) {
    if (!owner || !this.bridge.refs[KIND_OWNER_SNAPSHOT].is(owner) || this.sent.has(owner)) return;
    if (this.phase.active) throw Error('unprepared launch owner inside the owned phase');
    this.one[0] = owner;
    this.bridge.writeSome(KIND_OWNER_SNAPSHOT, this.one);
    this.sent.add(owner);
    this.reserveFuture();
    this.phase.counters.wasmCalls += 2;
    this.phase.counters.lifecycleCalls++;
    this.phase.counters.inputFields += KIND_TABLE[KIND_OWNER_SNAPSHOT].importStride;
  }

  // A native recon launch sampled its old-life metadata already. Bind the
  // ordinary frozen facade before the next notice resolves its owner reference.
  created(event) {
    const refs = this.bridge.refs[KIND_OWNER_SNAPSHOT];
    if (refs.objects[event.slot] && refs.gens[event.slot] === event.generation) return;
    const frame = this.frames[event.slot];
    if (!frame) throw Error('native launch exceeded prepared owner identities');
    const owner = restoreOrdnanceOwner(event.sourceUnit,event,frame);
    refs.bind(event.slot,event.generation,owner);
  }
}
