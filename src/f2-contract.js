// Counters describe actual crossings and scalar words copied, not estimates.
export class F2Counters {
  constructor() { this.reset(); }
  reset() {
    this.ticks = 0;
    this.wasmCalls = 0;
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
// only the declared mirror fields; leases and presentation objects are excluded.
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

// A plain forwarding table, scoped to this subsystem's bridge. No global
// intrinsic or exported function is replaced.
export function countedF2Exports(exports, counters) {
  return Object.fromEntries(Object.entries(exports).map(([name, value]) => [name,
    typeof value !== 'function' ? value : (...args) => {
      counters.wasmCalls++;
      if (name === 'f2_step') { counters.phaseCalls++; counters.inputFields += 2; }
      else if (name === 'f2_command' || name === 'f2_firing_supply') { counters.commandCalls++; counters.inputFields += args.length; }
      else counters.lifecycleCalls++;
      if (name === 'sim_import_room') counters.inputFields += args[1] * args[2] + args[3];
      if (name === 'veh_input') counters.inputFields += args[0];
      return value(...args);
    }]));
}
