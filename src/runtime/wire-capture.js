// EntityWorldCapture's native producer borrows the canonical world. Immutable
// protocol maps bind at construction/admission; no actor values cross here.
import { createStoreSource } from './wire-store-source.js';
import { EntityShotCounters } from './network-entity-shots.js';

export class WireEntityCapture {
  constructor(game, registry, { bridge = game.simBridge, shots = null, world, maxEntities = 256 } = {}) {
    if (bridge !== game.simBridge) throw Error('capture requires the shared simulation bridge');
    this.bridge = bridge;
    this.ownShots = shots ? null : new EntityShotCounters(game.simEvents, { bridge }); shots ??= this.ownShots;
    this.source = createStoreSource(game, registry, shots.nativeHandle, { maxEntities, world });
    try { shots.bindCanonical(); } catch (error) { this.source.dispose(); throw error; }
    // Supported canonical classes have no partial actor diagnostic. A real
    // producer failure goes through snapshot-host's existing capture fault.
    this.unsupported = [];
  }
  writeWorldFacts(tick, facts) { this.source.writeWorldFacts(tick, facts); }
  peerId(actor) { return this.source.peerId(actor); }
  get epoch() { return this.source.epoch; }
  // Explicit diagnostic materialization, never used by the live publisher.
  capture(tick) { return this.source.probe().records.map(record => ({ ...structuredClone(record), tick })); }
  refreshContent() { this.source.refreshContent(); }
  get nativeHandle() { return this.source.nativeHandle; }
  dispose() { this.source.dispose(); this.ownShots?.dispose(); }
}
