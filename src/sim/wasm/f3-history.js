// HitHistory.capture at the multiplayer world's end-of-tick boundary. F1
// consumes the retained native samples directly; JS keeps only the tick label.
// This capture remains after ordnance, items and separation, so a death never
// makes an earlier pose disappear or samples a body before its final movement.
export class F3History {
  constructor(phase) {
    this.phase = phase;
    this.bridge = phase.bridge;
    this.tick = -1;
    // Snapshot hosts can come and go while the battle continues. Releasing
    // their borrowed view must not discard the authority's rewind window.
    this.lease = {
      bridge: this.bridge,
      source: this,
      dispose() {},
    };
  }

  capture(tick) {
    const p = this.phase;
    // F4 can have changed a prop since the unit span. Reuse the generated
    // foreign block beside this capture, with no unit-body or shape imports.
    // A deferred write with zero intents places those facts at the block start.
    p.registry.foreign(0);
    p.x.f3_history_capture(tick);
    this.tick = tick;
    p.counters.wasmCalls++;
    p.counters.captureCalls++;
    p.counters.inputFields++;
    p.readLayout();
    // The allocator census includes lazy native shape observation as well as
    // writing the ring. Construction reserved every frame's shape capacity.
    p.counters.rustAllocations += p.layout.allocations;
    p.counters.rustReallocations += p.layout.reallocations;
    p.counters.rustAllocationBytes += p.layout.allocationBytes;
  }
}
