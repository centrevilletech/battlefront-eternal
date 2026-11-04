// Counters describe actual crossings and scalar words copied, not estimates.
export class F2Counters {
  constructor() { this.reset(); }
  reset() {
    this.ticks = 0;
    this.phaseCalls = 0;
    this.commandCalls = 0;
    this.lifecycleCalls = 0;
    this.debugCalls = 0;
    this.inputFields = 0;
    this.outputFields = 0;
  }
  snapshot() { return { ...this }; }
}

// Store snapshots rather than references to mutable vectors. The caller supplies
// only the fields owned by F2; foreign seat/weapon/tether state is excluded.
export class F2MirrorCheck {
  constructor() { this.values = new Map(); }
  published(object, fields) {
    this.values.set(object, fields.map(([path, value]) => [path, value]));
  }
  check(object, read) {
    for (const [path, value] of this.values.get(object) ?? []) {
      if (!Object.is(read(object, path), value)) throw Error(`F2 mirror write: ${path}`);
    }
  }
  retire(object) { this.values.delete(object); }
}
