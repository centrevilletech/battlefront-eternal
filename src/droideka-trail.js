import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from "three";
import { makeHit } from "./collision.js";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";

const CAPACITY = 160;
const UP = new THREE.Vector3(0, 1, 0), DOWN = new THREE.Vector3(0, -1, 0);



function dustTint(name) {
  if (/hot/i.test(name)) return [0.78, 0.83, 0.86];
  if (/tat|geo/i.test(name)) return [0.66, 0.46, 0.28];
  if (/end|kas|yav/i.test(name)) return [0.43, 0.37, 0.25];
  if (/bes|kam/i.test(name)) return [0.59, 0.63, 0.65];
  return [0.68, 0.61, 0.45];
}

// The trails by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a trail is `external`, three is shown
// it never and uploads it nothing.
export const droidekaTrails = new WeakMap();

// A fixed-size pool of dust wisps. Physics only emits particles; their motion,
// expansion and turbulent breakup happen on the GPU. Roof/floor probes are
// cached, and paved outdoor courtyards produce a lighter trail than terrain.
// Its clock is its own (trailTime, not the frame's time).
export class DroidekaTrail {
  constructor({ scene, collision, water = null, settings, mapName = "", random = Math.random }) {
    this.collision = collision;
    this.water = water;
    this.settings = settings;
    this.random = random;
    this.time = 0;
    this.count = 0;
    this.external = false;
    this.emitted = 0;
    this.carry = 0;
    this.probeAt = -Infinity;
    this.outdoor = false;
    this.terrain = false;
    this.probePosition = new THREE.Vector3(Infinity, Infinity, Infinity);
    this.previous = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.hit = makeHit();
    this.lastX = this.lastZ = 0;
    this.ready = false;
    this.records = new Float32Array(CAPACITY * 12);
    const plane = new THREE.PlaneGeometry(2, 2);
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.index = plane.index;
    this.geometry.attributes.position = plane.attributes.position;
    this.geometry.attributes.uv = plane.attributes.uv;
    this.attributes = ["birth", "drift", "shape"].map(name => {
      const a = new THREE.InstancedBufferAttribute(new Float32Array(CAPACITY * 4), 4).setUsage(THREE.DynamicDrawUsage);
      this.geometry.setAttribute(name, a);
      return a;
    });
    this.geometry.instanceCount = 0;
    this.material = ownedEffectMaterial('effects/trail', {
      uniforms: {
        trailTime: { value: 0 }, tint: { value: new THREE.Vector3(...dustTint(mapName)) },
        fogColor: lighting.fogColor, fogRange: lighting.fogRange,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true, depthWrite: false, depthTest: true, toneMapped: false,
    });
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = "droideka-rolling-dust";
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 9;
    this.mesh.visible = false;
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    droidekaTrails.set(this.mesh, this);
    scene.add(this.mesh);
  }

  probe(position) {
    this.probeAt = this.time;
    this.probePosition.copy(position);
    this.outdoor = false;
    this.origin.copy(position).y += 0.6;
    const floor = this.collision.cast(this.origin, DOWN, 1.3, this.hit);
    if (!floor || floor.normal.y < 0.45) return;
    this.floor = floor.point.y;
    this.terrain = floor.terrain;
    this.origin.set(position.x, this.floor + 0.25, position.z);
    this.outdoor = !this.collision.cast(this.origin, UP, 80, this.hit);
  }

  clear() {
    this.count = this.carry = 0;
    this.geometry.instanceCount = 0;
    this.mesh.visible = false;
  }

  update(dt, soldier, { dead = soldier.dead, position = soldier.position } = {}) {
    dt = Math.max(0, Math.min(dt, 0.1));
    this.time += dt;
    this.material.uniforms.trailTime.value = this.time;
    if ((this.settings?.get("droidekaEffects") ?? "enhanced") !== "enhanced") {
      this.clear(); this.ready = false; return;
    }
    const jumped = this.ready && this.previous.distanceToSquared(position) > 100;
    if (jumped) this.clear();
    for (let i = this.count - 1; i >= 0; i--) {
      const at = i * 12;
      if (this.time - this.records[at + 3] >= this.records[at + 7]) {
        this.count--;
        this.records.copyWithin(at, this.count * 12, this.count * 12 + 12);
      }
    }
    const vx = soldier.velocity.x, vz = soldier.velocity.z;
    const speed = Math.hypot(vx, vz);
    const rolling = soldier.mode === 12 && !dead && !soldier.air && soldier.grounded !== false && speed > 2.2;
    if (rolling && (this.time - this.probeAt > 0.08 || this.probePosition.distanceToSquared(position) > 0.25)) this.probe(position);
    const wet = this.water?.over(position.x, position.z) && position.y < this.water.level + 0.12;
    if (rolling && this.outdoor && !wet && dt > 0) {
      const dx = vx / speed, dz = vz / speed;
      const turn = this.ready ? Math.min(1, Math.abs(dx * this.lastZ - dz * this.lastX) / Math.max(dt * 2, 0.01)) : 0;
      const strength = THREE.MathUtils.clamp((speed - 2.2) / 8, 0, 1);
      this.carry += dt * (18 + strength * 38 + turn * 22);
      const spawn = Math.min(Math.floor(this.carry), CAPACITY - this.count);
      this.carry -= Math.floor(this.carry);
      for (let n = 0; n < spawn; n++) {
        const at = this.count++ * 12, f = (n + 1) / Math.max(spawn, 1);
        const spread = (this.random() - 0.5) * (0.55 + turn * 0.4);
        const r = this.records;
        r[at] = (this.ready && !jumped ? THREE.MathUtils.lerp(this.previous.x, position.x, f) : position.x) - dx * 0.4 + dz * spread;
        r[at + 1] = this.floor + 0.14 + this.random() * 0.12;
        r[at + 2] = (this.ready && !jumped ? THREE.MathUtils.lerp(this.previous.z, position.z, f) : position.z) - dz * 0.4 - dx * spread;
        r[at + 3] = this.time;
        r[at + 4] = -dx * (0.4 + strength * 0.8) + dz * spread * 2;
        r[at + 5] = 0.1 + this.random() * 0.25;
        r[at + 6] = -dz * (0.4 + strength * 0.8) - dx * spread * 2;
        r[at + 7] = 1.2 + this.random() * 0.8;
        r[at + 8] = 0.3 + strength * 0.2 + this.random() * 0.17;
        r[at + 9] = this.random() * 30;
        r[at + 10] = (this.terrain ? 0.7 : 0.42) * (0.55 + strength * 0.45 + turn * 0.25);
        r[at + 11] = this.terrain ? 1 : 0;
        this.emitted++;
      }
      this.lastX = dx; this.lastZ = dz;
    } else this.carry = 0;
    this.previous.copy(position);
    this.ready = true;
    for (let i = 0; i < this.count; i++) {
      for (let a = 0; a < 3; a++) {
        const array = this.attributes[a].array, from = i * 12 + a * 4, to = i * 4;
        array[to] = this.records[from]; array[to + 1] = this.records[from + 1];
        array[to + 2] = this.records[from + 2]; array[to + 3] = this.records[from + 3];
      }
    }
    for (const a of this.attributes) {
      a.clearUpdateRanges();
      if (this.count && !this.external) { a.addUpdateRange(0, this.count * 4); a.needsUpdate = true; }
    }
    this.geometry.instanceCount = this.count;
    this.mesh.visible = this.count > 0 && !this.external;
  }

  dispose() {
    this.mesh.removeFromParent();
    this.geometry.dispose();
    this.material.dispose();
  }
}
