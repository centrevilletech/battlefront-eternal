// buildWorld's authored terrain enters the same native geometry/material
// compiler as opaque buckets. Rust owns weights, atlas policy, cells and LODs;
// this adapter publishes scene visibility until the scene registry owns it.
import { staticFeatures } from "./geometry-types.js";

class TerrainPass {
  constructor(core) {
    this.core = core;
    this.name = "terrain";
    this.phase = "near";
    this.grounds = [];
    this.pending = [];
    this.entries = new WeakMap();
  }

  admit(group, record, vertices, index, weights, samplers, source = null) {
    this.core.registry.claim(group,true);
    const previous = this.entries.get(group);
    if (previous) { previous.stale = true; this.core.rust.world_shown(previous.id, false); }
    source ??= this.core.rust.static_source(JSON.stringify(record), vertices, index);
    this.core.rust.static_source_weights(source, weights);
    this.pending.push({ group, source, samplers });
    return true;
  }

  prepareNative() {
    const core = this.core;
    for (const { group, source, samplers } of this.pending) {
      const id = core.rust.static_world(source, false, 1, samplers,
        staticFeatures(core), group.userData.worldOrder, 0, group.id);
      const entry = { group, id, shown: true, stale: false };
      this.grounds.push(entry); this.entries.set(group, entry);
    }
    this.pending.length = 0;
    core.rust.sort(core.byName.world.byProgram ?? false);
  }

  draw(ctx) {
    if (this.pending.length) this.prepareNative();
    for (const entry of this.grounds) {
      if (entry.stale) continue;
      const shown = this.core.registry.shown(entry.group);
      if (shown !== entry.shown) { entry.shown = shown; this.core.rust.world_shown(entry.id, shown); }
    }
  }
}

export function terrainPass(core) { return new TerrainPass(core); }
