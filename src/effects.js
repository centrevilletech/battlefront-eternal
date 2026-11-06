import * as THREE from "three";
import { ParticleRenderer } from "./particles.js";
import { GpuEffects } from "./effects-gpu.js";
const UP = new THREE.Vector3(0, 1, 0);
// Browser resource shells consumed by native draw records. This class has
// no frame update, emission, particle storage or analytic evaluation.
export class EffectResources {
  // library: effect name -> emitter definitions. texture(name) gives a
  // texture; model(name) gives a function making a mesh of that model.
  constructor(scene, library, { texture, model, gpu = true }) {
    this.scene = scene;
    // LightningBolt blocks are drawn by lightning.js, not as particles.
    this.library = {};
    this.lightning = {}; // effect name -> its LightningBolt settings
    this.shields = {}; // effect name -> its Shield settings
    for (const [name, defs] of Object.entries(library || {})) {
      const bolt = defs.find((def) => def.lightning);
      if (bolt) this.lightning[name] = bolt.lightning;
      const shield = defs.find((def) => def.shield);
      if (shield) this.shields[name] = shield.shield;
      this.library[name] = defs.filter((def) => !def.lightning && !def.shield);
    }
    this.texture = texture;
    this.emitterTextures = new WeakMap();
    this.model = model;
    this.renderer = new ParticleRenderer(scene);
    this.gpu = gpu ? new GpuEffects(this.library, this.renderer) : null;
    this.effects = [];
    this.meshes = new Map(); // model name -> pooled meshes
  }

  // Load every texture the effects draw, so none appears late on first use.
  preload() {
    const visit = (defs) => {
      for (const def of defs || []) {
        if (def.geometry.texture) this.texture(def.geometry.texture);
        if (def.geometry.model) this.model?.(def.geometry.model);
        visit(def.geometry.emitters);
      }
    };
    for (const defs of Object.values(this.library)) visit(defs);
  }

  async warmup(renderer, camera) {
    if (!this.gpu) return;
    for (const def of this.gpu.definitions.keys()) {
      const texture = this.texture(def.geometry.texture);
      if (texture) {
        await this.gpu.warmup(renderer, camera, texture);
        break;
      }
    }
  }

  // Start an effect at a point, its up axis along `normal`, or turned by
  // `quaternion` when given.
  play(name, position, normal = UP, velocity = null, quaternion = null) {
    const defs = this.library[name];
    if (!defs) return null;
    const turn = quaternion ? quaternion.clone() : new THREE.Quaternion().setFromUnitVectors(UP, normal || UP);
    return this.start(defs, position, turn, velocity);
  }

  // An emitter definition's texture, resolved once: the name lookup lowercases
  // through every archive's table, and it was asked per emitter per frame.
  emitterTexture(def) {
    let texture = this.emitterTextures.get(def);
    if (texture === undefined) this.emitterTextures.set(def, (texture = this.texture(def.geometry.texture) || null));
    return texture;
  }
}

// Dedicated servers expose effect definitions to gameplay hooks but never
// construct a GPU or advance a displayed effect. This is a headless host.
export class HeadlessEffects {
  constructor(library) {
    this.lightning = {};
    this.shields = {};
    for (const [name, defs] of Object.entries(library))
      for (const def of defs) {
        if (def.lightning) this.lightning[name] = def.lightning;
        if (def.shield) this.shields[name] = def.shield;
      }
  }
  play() {
    return null;
  }
  update() {}
}
