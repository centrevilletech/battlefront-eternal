import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";
import { particleDepth } from "./particle-depth.js";

const MAX_NOZZLES = 4;
const ATTRIBUTES = { birth: 4, flow: 4, axis: 4, style: 2 };
const TYPES = [
  { name: "combustion", capacity: 96, rate: 36, life: [0.16, 0.27], drag: 7, opacity: 0.8 },
  { name: "smoke", capacity: 128, rate: 15, life: [0.65, 1.05], drag: 3, opacity: 0.12 },
  { name: "embers", capacity: 64, rate: 4.5, life: [0.2, 0.45], drag: 1.8, opacity: 0.75 },
];




// The pools by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a pool is `external`, three is shown
// it never and uploads it nothing.
export const jetpackWakes = new WeakMap();

// A pool of one kind of wake particle. Their clock is the wake's own
// (wakeTime, not the frame's time).
class WakePool {
  constructor(scene, kind) {
    this.kind = kind;
    this.spec = TYPES[kind];
    this.capacity = this.spec.capacity;
    this.count = 0;
    this.external = false;
    this.dirty = false;
    // WakePool's capacities and birth/flow records are gameplay presentation
    // facts. Native descriptors own the common plane and each kind's blend.
    this.nativeKind=EFFECT.WAKE.ID;this.family='wake';this.variant=kind;this.texture=null;
    this.values=new Float64Array(EFFECT.WAKE.LENGTH);
    this.values[EFFECT.WAKE.KIND]=kind;this.values[EFFECT.WAKE.DRAG]=this.spec.drag;
    this.arrays={};
    for(const [name,size] of Object.entries(ATTRIBUTES))this.arrays[name]=new Float32Array(this.capacity*size);
    this.streams=Object.values(this.arrays);this.widths=Object.values(ATTRIBUTES);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name = `jetpack-wake-${this.spec.name}`;
    sceneProperty(this.mesh,"visible",false);
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",kind === 1 ? 8 : 11);
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    jetpackWakes.set(this.mesh, this);
    scene.add(this.mesh);
  }

  emit(time, position, direction, velocity, radius, throttle, random) {
    if (this.count >= this.capacity) return false;
    const n = this.count++, i = n * 4, j = n * 2;
    const { birth, flow, axis, style } = this.arrays;
    const speed = this.kind === 0 ? 4 + random() * 2 : this.kind === 1 ? 1.8 + random() * 1.1 : 4 + random() * 3;
    const downstream = this.kind === 0 ? 0.12 + random() * 0.08 : this.kind === 1 ? 0.36 + random() * 0.12 : 0.15;
    birth[i] = position.x + direction.x * downstream;
    birth[i + 1] = position.y + direction.y * downstream;
    birth[i + 2] = position.z + direction.z * downstream;
    birth[i + 3] = time;
    const spread = this.kind === 2 ? 0.9 : 0.3;
    flow[i] = velocity.x * 0.72 + direction.x * speed + (random() - 0.5) * spread;
    flow[i + 1] = velocity.y * 0.72 + direction.y * speed + (random() - 0.5) * spread;
    flow[i + 2] = velocity.z * 0.72 + direction.z * speed + (random() - 0.5) * spread;
    flow[i + 3] = THREE.MathUtils.lerp(this.spec.life[0], this.spec.life[1], random());
    axis[i] = direction.x;
    axis[i + 1] = direction.y;
    axis[i + 2] = direction.z;
    axis[i + 3] = this.kind === 0 ? radius * (1.1 + random()) : this.kind === 1 ? radius * (2.0 + random() * 1.1) : 0.004 + random() * 0.003;
    style[j] = random() * 100;
    style[j + 1] = this.spec.opacity * (0.6 + throttle * 0.4);
    this.dirty = true;
    return true;
  }

  update(time) {
    const birth = this.arrays.birth, flow = this.arrays.flow;
    for (let n = this.count - 1; n >= 0; n--) {
      if (time - birth[n * 4 + 3] < flow[n * 4 + 3]) continue;
      this.count--;
      for(let k=0;k<this.streams.length;k++){
        const size=this.widths[k];this.streams[k].copyWithin(n*size,this.count*size,(this.count+1)*size);
      }
      this.dirty = true;
    }
    sceneProperty(this.mesh,"visible",this.count > 0);
  }

  clear() {
    this.count = 0;
    sceneProperty(this.mesh,"visible",false);
    this.dirty = false;
  }
}

export class JetpackWake {
  constructor({ scene, random = Math.random }) {
    this.random = random;
    this.time = 0;
    this.emitted = 0;
    this.pools = TYPES.map((_, kind) => new WakePool(scene, kind));
    [this.wisps, this.smoke, this.embers] = this.pools;
    this.emitters = Array.from({ length: MAX_NOZZLES }, () => ({
      position: new THREE.Vector3(), direction: new THREE.Vector3(), ready: false,
      phase: new Float64Array(3),
    }));
    this.position = new THREE.Vector3();
    this.direction = new THREE.Vector3();
  }

  update(dt, { nozzles, velocity, throttle = 1, active }) {
    dt = THREE.MathUtils.clamp(dt, 0, 0.25);
    this.time += dt;
    for(const pool of this.pools)pool.values[EFFECT.WAKE.WAKE_TIME]=this.time;
    throttle = THREE.MathUtils.clamp(throttle, 0, 1);
    const count = Math.min(nozzles.length, MAX_NOZZLES);
    // One teleport invalidates the entire old wake, including the other
    // nozzle, rather than drawing a bridge between remote world positions.
    for (let i = 0; i < count; i++) {
      if (this.emitters[i].ready && this.emitters[i].position.distanceToSquared(nozzles[i].position) > 64) {
        this.clear();
        break;
      }
    }
    for (let i = 0; i < MAX_NOZZLES; i++) {
      const previous = this.emitters[i], nozzle = nozzles[i];
      if (!active || throttle < 0.01 || i >= count) {
        previous.ready = false;
        previous.phase.fill(0);
        continue;
      }
      const suppliedPrevious = nozzle.previous?.position || nozzle.previous;
      const usablePrevious = suppliedPrevious?.isVector3 && suppliedPrevious.distanceToSquared(nozzle.position) <= 64;
      const start = usablePrevious ? suppliedPrevious : previous.ready ? previous.position : nozzle.position;
      const oldDirection = previous.ready ? previous.direction : nozzle.direction;
      for (let kind = 0; kind < this.pools.length; kind++) {
        const pool = this.pools[kind], rate = pool.spec.rate * throttle;
        const phase = previous.phase[kind];
        const total = phase + dt * rate;
        const births = Math.floor(total + 1e-9);
        previous.phase[kind] = Math.max(0, total - births);
        for (let n = 0; n < births; n++) {
          const fraction = dt > 0 ? THREE.MathUtils.clamp((n + 1 - phase) / (dt * rate), 0, 1) : 1;
          sceneTransform(this.position.lerpVectors(start, nozzle.position, fraction));
          this.direction.lerpVectors(oldDirection, nozzle.direction, fraction).normalize();
          if (pool.emit(this.time - dt + fraction * dt, this.position, this.direction, velocity, nozzle.radius || 0.034, throttle, this.random)) this.emitted++;
        }
      }
      sceneTransform(previous.position.copy(nozzle.position));
      previous.direction.copy(nozzle.direction);
      previous.ready = true;
    }
    for (const pool of this.pools) pool.update(this.time);
  }

  clear() {
    for (const pool of this.pools) pool.clear();
    for (const emitter of this.emitters) {
      emitter.ready = false;
      emitter.phase.fill(0);
    }
  }

  dispose() {
    for (const pool of this.pools) {
      pool.mesh.removeFromParent();
    }
  }
}
