import { STATIC } from "./static-layout.js";
import { staticFeatures } from "./geometry-types.js";
// Colour and depth share native geometry/material descriptors. The page
// keeps only scene anchors and loader images for cold resource admission.

class WorldPass {
  constructor(core) {
    this.core = core;
    this.name = "world";
    this.phase = "near";
    this.count = 0;
    this.meshes = []; // scene node and numeric native descriptor id
    this.lampNode = null;
    this.nativePending = [];this.nativeAt=0;
    this.entries = new WeakMap();
    this.settingsDirty = false;
    // The settings owner holds a weak listener, so a closed battle can collect
    // its native descriptors even when the settings object outlives the page.
    const weak = new WeakRef(this);
    core.graphics.settings?.onChange?.((key, value) => {
      const owner = weak.deref(); if (!owner) return;
      if (key === "shadows" || key === "lampLighting") owner.settingsDirty = true;
      if (key === "anisotropy") owner.core.resources.staticAnisotropy(Number(value));
    });

  }

  register() {
    this.core.addView("worldLampInput", Float32Array, () => this.core.rust.world_input_ptr(), () => this.core.rust.world_input_len());
  }

  // buildWorld provides authored bytes directly. A scene anchor carries no
  // geometry or material; depth admission refers to the same native source.
  admit(mesh, record, vertices, index, map, bump) {
    const previous = this.entries.get(mesh);
    if (previous) { previous.stale = true; this.core.rust.world_shown(previous.id, false); }
    const source = this.core.rust.static_source(JSON.stringify(record), vertices, index);
    // Replacing a material or stream keeps its depth reference and scene id.
    // Rebuild cold depth resources together, invalidating cached cascades once.
    if (mesh.userData.nativeDepthSource !== undefined) {
      const core=this.core, shadow=core.byName.shadow;
      const texture=map == null ? -1 : core.texture(core.resources.staticSample(map));
      core.rust.static_depth_replace(mesh.userData.nativeDepthSource,source,texture);
      shadow.nativeRoots.delete(mesh.userData.nativeSource);
      shadow.nativeRoots.set(source,mesh);
      shadow.queueNative(shadow.nativeRoots);
    }
    mesh.userData.nativeSource=source;
    this.core.registry.claim(mesh);
    this.nativePending.push({ mesh, source, tree: Boolean(record.treeCanopies), map, bump });
    return source;
  }

  prepareNative() {
    const core = this.core;
    for (;this.nativeAt<this.nativePending.length;this.nativeAt++) {
      const cold=this.nativePending[this.nativeAt];
      const { mesh, source, tree, map, bump } = cold;
      const samplers = new Int32Array(STATIC.SAMPLERS.LENGTH).fill(-1);
      samplers[STATIC.SAMPLERS.MAP] = map == null ? -1 : core.texture(core.resources.staticSample(map));
      samplers[STATIC.SAMPLERS.BUMP] = bump == null ? -1 : core.texture(core.resources.staticSample(bump));
      if (tree) samplers[STATIC.SAMPLERS.TREE] = core.texture(core.graphics.treeCanopies.stateMap);
      const id = core.rust.static_world(source, false, 1, samplers, staticFeatures(core), mesh.userData.worldOrder, 0, mesh.id);
      const entry = { mesh, id, native: true, stale: false };
      this.meshes.push(entry);
      this.entries.set(mesh, entry);
      if (!this.lampNode && core.rust.world_uses_lamps(id)) this.lampNode = mesh;
      mesh.userData.nativeWorld = id;
      this.count++;
    }
    this.nativePending.length = 0;this.nativeAt=0;
    core.rust.sort(false);
  }

  updateSettings() {
    if (!this.settingsDirty) return;
    this.settingsDirty = false;
    this.core.rust.static_world_settings(staticFeatures(this.core), staticFeatures(this.core, true));
  }

  draw() {
    if (this.nativePending.length) this.prepareNative();
    this.updateSettings();
    // Emissive lamp occlusion remains a lighting owner. It receives only the
    // identity scene anchor, never a geometry/material descriptor to inspect.
    if (this.lampNode) {
      const core = this.core, lamps = core.lamps(this.lampNode, false);
      core.views();
      core.worldLampInput.set(lamps.indices); core.worldLampInput[STATIC.LAMPS.COUNT] = lamps.count;
    }
  }


}

export function worldPass(core) {
  return new WorldPass(core);
}
