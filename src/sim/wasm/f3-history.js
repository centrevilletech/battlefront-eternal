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

}
