// Counters describe actual crossings and scalar words copied, not estimates.
// Phase exports and explicit scalar copies are counted separately from the
// shared bridge's query and RNG imports. Constructor work and commands are
// visible categories, so a low phase-call count cannot hide lifecycle traffic.
// Reader groups explain the remaining words when comparing boundary costs.
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
    this.inputRegistry = this.inputFiring = this.inputFlight = this.inputWorld = this.inputFlags = this.inputSeats = 0;
    this.outputHulls = this.outputPads = this.outputLives = this.outputPoses = this.outputCollision = 0;
    this.outputLeases = this.outputUnits = this.outputControls = this.outputTurrets = this.outputRiders = this.outputFiring = this.outputEffects = 0;
  }
  snapshot() { return { ...this }; }
}

// Store snapshots rather than references to mutable vectors. The caller supplies
// only the fields owned by F2; foreign seat/weapon/tether state is excluded.
export class F2MirrorCheck {
  constructor() { this.values = new Map(); }
  published(object, fields) {
    let saved = this.values.get(object);
    if (!saved || saved.length !== fields.length) {
      saved = new Array(fields.length);
      for (let i = 0; i < fields.length; i++) saved[i] = [fields[i][0], fields[i][1]];
      this.values.set(object, saved);
    } else for (let i = 0; i < fields.length; i++) {
      saved[i][0] = fields[i][0]; saved[i][1] = fields[i][1];
    }
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
// Keep a plain forwarding table local to this phase. Fixed arguments avoid a
// rest-array allocation on every export. The original bridge table is restored
// after synchronous publication, including nested damage and seat callbacks.
export function countedF2Exports(exports, counters) {
  const counted = {};
  for (const name of Object.keys(exports)) {
    const value = exports[name];
    if (typeof value !== 'function') { counted[name] = value; continue; }
    const words = value.length;
    counted[name] = function(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31) {
      counters.wasmCalls++;
      if (name === 'f2_step') { counters.phaseCalls++; counters.inputFields += 2; }
      else if (name === 'f2_command' || name === 'cmd_f2_seat' || name === 'f2_firing_supply') { counters.commandCalls++; counters.inputFields += words; }
      else counters.lifecycleCalls++;
      if (name === 'sim_import_room') counters.inputFields += a1 * a2 + a3;
      if (name === 'veh_input') counters.inputFields += a0;
      return value(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31);
    };
  }
  return counted;
}
