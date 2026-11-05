import { writeCompactSoldier, COMPACT_SOLDIER_MAX_BYTES, COMPACT_SOLDIER_LENGTHS } from './network-entity-soldier-compact.js';
import { writeDroidekaState, DROIDEKA_STATE_MAX_BYTES, DROIDEKA_STATE_LENGTHS } from './network-droideka-state.js';

// game.js replayPrediction borrows these rows for one synchronous prediction.
// restorePrediction consumes the body checkpoint before the next capture;
// neither Rust nor presentation retains these command facts after the call.
export class PredictionFrame {
  constructor() {
    const bytes = new Uint8Array(Math.max(COMPACT_SOLDIER_MAX_BYTES, DROIDEKA_STATE_MAX_BYTES));
    this.writer = { bytes, view: new DataView(bytes.buffer), at: 0 };
    this.views = [];
    // Existing compact wire lengths: soldier's optional settled/dive tails,
    // droideka's optional shield/gun tails. Make their views once per battle.
    for (const length of COMPACT_SOLDIER_LENGTHS) this.views[length] = bytes.subarray(0, length);
    for (const length of DROIDEKA_STATE_LENGTHS) this.views[length] = bytes.subarray(0, length);
    this.crowd = []; this.fireEdges = []; this.edgePool = []; this.empty = [];
    this.options = { epoch: 1, tick: 0, held: this.empty, view: null, mouseScale: 1,
      fireEdges: this.fireEdges, crowd: this.crowd, checkTick: 0, presentedThrough: 0, cosmeticSlots: 0 };
    // Map.forEach can reuse this callback; values() would make an iterator on
    // every frame. Pool size follows the largest outstanding outcome set.
    this.storeEdge = item => {
      const index = this.fireEdges.length, edge = item.edge;
      let row = this.edgePool[index];
      if (!row) row = this.edgePool[index] = { sequence: 0, kind: null, pressed: false, button: null, id: 0 };
      // Overwrite absent fields too: a reused slot may now contain a shield
      // action rather than the fire press which occupied it on the last frame.
      row.sequence = item.sequence; row.kind = edge.kind; row.pressed = edge.pressed;
      row.button = edge.button; row.id = edge.id;
      this.fireEdges[index] = row;
    };
  }
  clear() {
    // A long mounted/recon interval must not retain the last on-foot roster.
    // The checkpoint bytes stay valid until restorePrediction has used them.
    this.crowd.length = 0; this.fireEdges.length = 0;
    this.options.held = this.empty; this.options.view = null;
  }
  captureBody(body, walker) {
    const length = walker ? writeDroidekaState(body, this.writer) : writeCompactSoldier(body, this.writer);
    return this.views[length];
  }
  captureEdges(outcomes) {
    this.fireEdges.length = 0;
    outcomes.forEach(this.storeEdge);
  }
  captureCrowd(units, own) {
    this.crowd.length = 0;
    // Preserve the registry order and the old separatePredictedSoldier filter.
    // Dead bodies still separate; seated, held and removed bodies do not.
    for (let i = 0; i < units.length; i++) {
      const unit = units[i];
      if (unit !== own && !unit.seat && !unit.held && unit.soldier && !unit.removed && (unit.alive || unit.dead)) this.crowd.push(unit);
    }
  }
}
