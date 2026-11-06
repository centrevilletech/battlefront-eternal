import {BlockHeaderLayout as B} from './gen/boundary.js';
// Shared ownership-boundary instrumentation. Count copied scalar words at the
// copy site; phase bridges count their export arguments at the call site.
export class PhaseCounters {
  constructor() { this.reset(); }
  reset() {
    this.ticks = 0;
    this.wasmCalls = 0;
    this.importCalls = 0;
    this.phaseCalls = 0;
    this.commandCalls = 0;
    this.lifecycleCalls = 0;
    this.captureCalls = 0;
    this.debugCalls = 0;
    this.inputFields = 0;
    this.outputFields = 0;
    this.unitRows = 0;
    this.unitWords = 0;
    // Native owners snapshot the actual allocator around their phase call.
    // Requested bytes include reallocations; these are not heap-live sizes.
    this.rustAllocations = 0;
    this.rustReallocations = 0;
    this.rustAllocationBytes = 0;
  }
  snapshot() { return { ...this }; }
  // store/block.rs is the shared five-word ABI: rows, stride, heap offset,
  // heap length and retired count. Count encoded rows and their heap once
  // where a publication is decoded, including empty block headers.
  outputBlock(floats, at) {
    if (at) this.outputFields += B.words + floats[at + B.rows] * floats[at + B.stride] + floats[at + B.heapWords];
  }
}

// Ordinary values, copied on publication, with no property traps. The owner
// declares its fields and excludes foreign state before recording a snapshot.
export class MirrorWriteCheck {
  constructor(label) { this.label = label; this.values = new Map(); }
  published(object, fields) {
    let values = this.values.get(object);
    if (!values) { values = []; this.values.set(object, values); }
    for (let i = 0; i < fields.length; i++) {
      let pair = values[i];
      if (!pair) { pair = []; values[i] = pair; }
      pair[0] = fields[i][0]; pair[1] = fields[i][1];
    }
    values.length = fields.length;
  }
  check(object, read) {
    const values = this.values.get(object);
    if (!values) return;
    for (let i = 0; i < values.length; i++) {
      const path = values[i][0];
      if (!Object.is(read(object, path), values[i][1])) throw Error(`${this.label} mirror write: ${path}`);
    }
  }
  retire(object) { this.values.delete(object); }
}
