import * as THREE from "three";
import { BILLBOARD, FLAT, ParticleRenderer, STREAK } from "./particles.js";
import { GpuEffects } from "./effects-gpu.js";

// Particle effects from the game's .fx data (tools/zero_fx.py), run the way the
// 2004 game runs them.
//
// An emitter waits StartDelay, then emits a burst of BurstCount particles
// every BurstDelay seconds until it has emitted MaxParticles (-1 never stops;
// such effects run until their owner stops them). Each particle takes a
// direction from the Spawner's Spread box (per-axis ranges) or Circle (a
// random direction mapped into those ranges), starts at Offset + direction x
// PositionScale moving at direction x VelocityScale (plus the emitter's own
// velocity x InheritVelocityFactor), all in the emitter's frame, and picks its
// size, colour, alpha, and rotation from the Spawner's ranges.
//
// Particles are stateless: every frame each one is rebuilt from its spawn
// values and its age. An emitter normally records the path it moves along,
// so its particles stay where they were emitted (a trail); one marked
// NoRegisterStep keeps no path, so its particles are placed from where the
// emitter is now and travel with it (the glow around a flying grenade). The Transformer's keys run in turn for their Lifetime,
// from where the last key left off: position keys Accelerate (the velocity
// changes by the amount over the key), Reach a velocity, or Scale it, and the
// particle coasts after the last key; size keys Scale, Move, or Reach; colour
// keys Reach or Move. The particle dies at the Transformer's Lifetime.

const DEG = Math.PI / 180;
const UP = new THREE.Vector3(0, 1, 0);
const MAX_BURST = 128;

const rand = (range) => range[0] + Math.random() * (range[1] - range[0]);

// HSV in 0..1 to RGB in 0..1.
function hsv(h, s, v, out) {
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const p = v * (1 - s);
  const q = v * (1 - f * s);
  const t = v * (1 - (1 - f) * s);
  const k = ((i % 6) + 6) % 6;
  const rgb = [
    [v, t, p],
    [q, v, p],
    [p, v, t],
    [p, q, v],
    [t, p, v],
    [v, p, q],
  ][k];
  out[0] = rgb[0];
  out[1] = rgb[1];
  out[2] = rgb[2];
  return out;
}

// Where a particle has got to after `age` seconds of its position keys, from
// its spawn velocity: the displacement goes into `p`, the velocity into `v`.
function travel(keys, age, v, p) {
  p.set(0, 0, 0);
  let left = age;
  for (const key of keys || []) {
    if (left <= 0) break;
    const span = key.lifetime;
    if (span <= 0) {
      if (key.accelerate) {
        v.x += key.accelerate[0];
        v.y += key.accelerate[1];
        v.z += key.accelerate[2];
      } else if (key.reach) {
        v.fromArray(key.reach);
      } else if (key.scale !== undefined) {
        v.multiplyScalar(key.scale);
      }
      continue;
    }
    const t = Math.min(left, span);
    const f = t / span;
    if (key.accelerate) {
      const [ax, ay, az] = key.accelerate;
      p.x += v.x * t + (ax * t * f) / 2;
      p.y += v.y * t + (ay * t * f) / 2;
      p.z += v.z * t + (az * t * f) / 2;
      v.x += ax * f;
      v.y += ay * f;
      v.z += az * f;
    } else if (key.reach) {
      const [rx, ry, rz] = key.reach;
      p.x += v.x * t + ((rx - v.x) * t * f) / 2;
      p.y += v.y * t + ((ry - v.y) * t * f) / 2;
      p.z += v.z * t + ((rz - v.z) * t * f) / 2;
      v.x += (rx - v.x) * f;
      v.y += (ry - v.y) * f;
      v.z += (rz - v.z) * f;
    } else if (key.scale !== undefined) {
      const s = key.scale;
      p.addScaledVector(v, t + ((s - 1) * t * f) / 2);
      v.multiplyScalar(1 + (s - 1) * f);
    } else {
      p.addScaledVector(v, t);
    }
    left -= t;
  }
  if (left > 0) p.addScaledVector(v, left);
}

// A scalar track (size) after `age` seconds, from its spawn value.
function scalarTrack(keys, age, value) {
  let left = age;
  for (const key of keys || []) {
    if (left < 0) break;
    const span = key.lifetime;
    const f = span > 0 ? Math.min(left / span, 1) : 1;
    if (key.scale !== undefined) value *= 1 + (key.scale - 1) * f;
    else if (key.move !== undefined) value += (Array.isArray(key.move) ? key.move[0] : key.move) * f;
    else if (key.reach !== undefined) value += ((Array.isArray(key.reach) ? key.reach[0] : key.reach) - value) * f;
    left -= span;
    if (f < 1) break;
  }
  return value;
}

// The colour track after `age` seconds, from the spawn colour (0..1 each).
function colorTrack(keys, age, c) {
  let left = age;
  for (const key of keys || []) {
    const span = key.lifetime;
    const f = span > 0 ? Math.min(left / span, 1) : 1;
    if (key.reach) {
      for (let i = 0; i < 4; i++) c[i] += (key.reach[i] / 255 - c[i]) * f;
    } else if (key.move) {
      for (let i = 0; i < 4; i++) c[i] = Math.min(Math.max(c[i] + (key.move[i] / 255) * f, 0), 1);
    }
    left -= span;
    if (f < 1) break;
  }
  return c;
}

class Emitter {
  constructor(def) {
    this.def = def;
    this.delay = rand(def.startDelay);
    this.nextBurst = 0;
    this.limit = Math.round(rand(def.maxParticles));
    this.emitted = 0;
    this.particles = [];
    this.size = rand(def.size);
    this.scale = def.color.channels.map((c) => rand(c) / 255);
  }

  get emitting() {
    return this.limit < 0 || this.emitted < this.limit;
  }
}

export class Effect {
  constructor(defs, position, quaternion, velocity) {
    this.emitters = defs.map((def) => new Emitter(def));
    this.position = position.clone();
    this.previous = position.clone();
    this.quaternion = quaternion.clone();
    this.velocity = velocity ? velocity.clone() : new THREE.Vector3();
    this.time = 0;
    this.stopped = false;
    this.done = false;
    this.tint = 1; // multiplies every particle's colour and alpha
    // Multiplies every particle's drawn size, as the original game's emitter
    // scale does (a vehicle's ThrustEffectMinScale..MaxScale, its
    // DamageEffectScale); sub-emitters take it when they start.
    this.scale = 1;
    // A share of the emitter's velocity every particle takes on top of its
    // spawner's InheritVelocity (a vehicle's DamageInheritVelocity).
    this.inherit = 0;
  }

  // A moving effect (a trail) follows its owner.
  move(position, quaternion, velocity) {
    this.position.copy(position);
    if (quaternion) this.quaternion.copy(quaternion);
    if (velocity) this.velocity.copy(velocity);
  }

  // Anchor each retail jet emitter to the nearest skinned outlet. Keep
  // this state on the live emitter; definitions are shared by all actors.
  attachNozzles(frames) {
    if (!frames?.length) return;
    for (const em of this.emitters) {
      const offset = new THREE.Vector3(...em.def.spawner.offset.map(v => (v[0] + v[1]) * .5)).applyQuaternion(this.quaternion);
      const authored = offset.clone().add(this.position);
      const frame = frames.reduce((a, b) => authored.distanceToSquared(a.position) < authored.distanceToSquared(b.position) ? a : b);
      const axis = new THREE.Vector3(-1, 0, 0).applyQuaternion(this.quaternion);
      em.nozzleTurn ||= new THREE.Quaternion();
      em.nozzleTurn.setFromUnitVectors(axis, frame.direction);
      em.nozzleOffset ||= new THREE.Vector3();
      em.nozzleOffset.copy(frame.position).sub(this.position).sub(offset.applyQuaternion(em.nozzleTurn));
    }
  }

  // Stop emitting; what is out already lives on.
  stop() {
    this.stopped = true;
  }

  // Gone at once, particles and all.
  kill() {
    this.stopped = true;
    this.killed = true;
  }
}

const scratch = {
  dir: new THREE.Vector3(),
  pos: new THREE.Vector3(),
  vel: new THREE.Vector3(),
  disp: new THREE.Vector3(),
  axis: new THREE.Vector3(),
  axis2: new THREE.Vector3(),
  color: [0, 0, 0, 0],
  rgb: [0, 0, 0],
  frame: [0, 0, 1, 1],
};

export class Effects {
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
      if (texture) { await this.gpu.warmup(renderer, camera, texture); break; }
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

  start(defs, position, quaternion, velocity) {
    const effect = new Effect(defs, position, quaternion, velocity);
    this.effects.push(effect);
    return effect;
  }

  update(dt) {
    this.renderer.begin();
    this.gpu?.begin();
    for (const pool of this.meshes.values()) pool.used = 0;
    // In place: a filtered copy of every list, every frame, was garbage.
    let kept = 0;
    for (let i = 0; i < this.effects.length; i++) {
      const effect = this.effects[i];
      this.step(effect, dt);
      if (!effect.done) this.effects[kept++] = effect;
    }
    this.effects.length = kept;
    this.renderer.end();
    this.gpu?.end();
    for (const pool of this.meshes.values()) {
      for (let i = pool.used; i < pool.list.length; i++) pool.list[i].visible = false;
    }
  }

  // An emitter definition's texture, resolved once: the name lookup lowercases
  // through every archive's table, and it was asked per emitter per frame.
  emitterTexture(def) {
    let texture = this.emitterTextures.get(def);
    if (texture === undefined) this.emitterTextures.set(def, (texture = this.texture(def.geometry.texture) || null));
    return texture;
  }

  step(effect, dt) {
    if (effect.killed) {
      for (const em of effect.emitters) for (const p of em.particles) p.child?.kill();
      effect.done = true;
      return;
    }
    const from = effect.time;
    effect.time += dt;
    const gpuSlot = this.gpu?.register(effect) ?? -1;
    let alive = false;
    for (const em of effect.emitters) {
      // Bursts due this frame, placed along the path the emitter moved.
      while (!effect.stopped && em.emitting && effect.time - em.delay >= em.nextBurst) {
        const at = em.delay + em.nextBurst;
        const share = dt > 0 ? THREE.MathUtils.clamp((at - from) / dt, 0, 1) : 1;
        const count = Math.min(Math.round(rand(em.def.burstCount)), MAX_BURST);
        for (let i = 0; i < count && em.emitting; i++) this.spawn(effect, em, at, em.def.registerStep ? share : 1);
        em.nextBurst += Math.max(rand(em.def.burstDelay), 1e-3);
        if (count <= 0 && em.limit < 0 && em.def.burstDelay[1] <= 0) break;
      }
      const definition = this.gpu?.definitions.get(em.def);
      const map = definition !== undefined && gpuSlot >= 0 ? this.emitterTexture(em.def) : null;
      let batch = null;
      const particles = em.particles;
      let live = 0;
      for (let i = 0; i < particles.length; i++) {
        const p = particles[i];
        const age = effect.time - p.birth;
        if (age >= p.life) {
          if (p.child) p.child.stop();
          continue;
        }
        if (map) {
          batch ||= this.gpu.batch(em.def, map);
          batch.pushParticle(p, gpuSlot, definition, this.gpu.epochs[gpuSlot]);
        }
        else this.draw(effect, em, p, age);
        particles[live++] = p;
      }
      particles.length = live;
      if (em.particles.length || (!effect.stopped && em.emitting)) alive = true;
    }
    effect.previous.copy(effect.position);
    effect.done = !alive;
  }

  spawn(effect, em, birth, share) {
    const sp = em.def.spawner;
    const { dir } = scratch;
    const [bx, by, bz] = sp.box;
    if (sp.shape === "circle") {
      // A random direction, each component mapped into its axis's range.
      dir.randomDirection();
      dir.set(
        bx[0] + ((dir.x + 1) / 2) * (bx[1] - bx[0]),
        by[0] + ((dir.y + 1) / 2) * (by[1] - by[0]),
        bz[0] + ((dir.z + 1) / 2) * (bz[1] - bz[0]),
      );
    } else {
      dir.set(rand(bx), rand(by), rand(bz));
    }
    const positionScale = rand(sp.positionScale);
    const velocityScale = rand(sp.velocityScale);
    const [ox, oy, oz] = sp.offset;
    const position = new THREE.Vector3(rand(ox), rand(oy), rand(oz))
      .addScaledVector(dir, positionScale)
      .applyQuaternion(effect.quaternion);
    if (em.nozzleTurn) position.applyQuaternion(em.nozzleTurn).add(em.nozzleOffset);
    // Recorded paths fix the particle in the world; otherwise it stays an
    // offset from the emitter.
    if (em.def.registerStep) position.add(new THREE.Vector3().lerpVectors(effect.previous, effect.position, share));
    const velocity = dir.clone().multiplyScalar(velocityScale).applyQuaternion(effect.quaternion);
    if (em.nozzleTurn) velocity.applyQuaternion(em.nozzleTurn);
    velocity.addScaledVector(effect.velocity, rand(sp.inheritVelocity) + effect.inherit);

    const c = sp.color.channels.map(rand);
    const color = [c[0] / 255, c[1] / 255, c[2] / 255, c[3] / 255];
    if (sp.color.hsv) hsv(color[0], color[1], color[2], color);
    for (let i = 0; i < 4; i++) color[i] *= em.scale[i];
    if (sp.fadeInTime > 0 && birth - em.delay < sp.fadeInTime) color[3] *= (birth - em.delay) / sp.fadeInTime;

    const geometry = em.def.geometry;
    const particle = {
      birth,
      life: em.def.transformer.lifetime,
      position,
      velocity,
      size: rand(sp.size) * em.size,
      color,
      rotation: rand(sp.startRotation) * DEG,
      spin: rand(sp.rotationVelocity) * DEG,
      turn: geometry.type === "geometry" ? new THREE.Quaternion().random() : null,
      child: null,
    };
    if (geometry.type === "emitter" && geometry.emitters?.length) {
      const at = em.def.registerStep ? position : position.clone().add(effect.position);
      particle.child = this.start(geometry.emitters, at, effect.quaternion, velocity);
      particle.child.scale = effect.scale;
    }
    em.particles.push(particle);
    em.emitted++;
  }

  draw(effect, em, p, age) {
    const def = em.def;
    const geometry = def.geometry;
    const { pos, vel, disp, color } = scratch;
    vel.copy(p.velocity);
    travel(def.transformer.position, age, vel, disp);
    pos.copy(p.position).add(disp);
    if (!def.registerStep) pos.add(effect.position);
    const size = scalarTrack(def.transformer.size, age, p.size) * effect.scale;
    for (let i = 0; i < 4; i++) color[i] = p.color[i];
    colorTrack(def.transformer.color, age, color);
    if (effect.tint !== 1) for (let i = 0; i < 4; i++) color[i] *= effect.tint;
    const rotation = p.rotation + p.spin * age;
    if (p.child) p.child.move(pos, null, vel);
    if (geometry.type === "emitter" || geometry.blend === "blur" || size <= 0) return;

    if (geometry.type === "geometry") {
      this.drawModel(geometry, pos, size, p.turn, rotation, color);
      return;
    }
    const map = geometry.texture ? this.emitterTexture(em.def) : null;
    if (!map) return;
    const batch = this.renderer.batch(map, geometry.blend === "additive" ? "additive" : "normal");
    if (geometry.type === "spark" || geometry.type === "streak") {
      const speed = vel.length();
      const length = speed * (geometry.sparkLength || 0.1);
      if (speed < 1e-4) return;
      scratch.axis.copy(vel).divideScalar(speed);
      batch.push(pos.x, pos.y, pos.z, color[0], color[1], color[2], color[3], size, 0, STREAK, length, scratch.axis.toArray(), [1, 0, 0]);
    } else if (geometry.type === "billboard") {
      // Flat in the emitter's ground plane, turned by its rotation.
      const c = Math.cos(rotation);
      const s = Math.sin(rotation);
      scratch.axis.set(c, 0, s).applyQuaternion(effect.quaternion);
      scratch.axis2.set(-s, 0, c).applyQuaternion(effect.quaternion);
      batch.push(pos.x, pos.y, pos.z, color[0], color[1], color[2], color[3], size, 0, FLAT, 0, scratch.axis.toArray(), scratch.axis2.toArray());
    } else {
      let frame = null;
      if (geometry.type === "animated" && geometry.timePerFrame > 0) {
        const cells = Math.max(Math.round(geometry.frameSize), 1);
        const total = Math.max(Math.round(geometry.totalFrames), 1);
        const index = Math.min(Math.floor(age / geometry.timePerFrame), total - 1);
        frame = scratch.frame;
        frame[0] = (index % cells) / cells;
        frame[1] = Math.floor(index / cells) / cells;
        frame[2] = 1 / cells;
        frame[3] = 1 / cells;
      }
      batch.push(pos.x, pos.y, pos.z, color[0], color[1], color[2], color[3], size, rotation, BILLBOARD, 0, null, null, frame);
    }
  }

  // GEOMETRY particles draw a model, turned and scaled, in the particle's colour.
  drawModel(geometry, pos, size, turn, rotation, color) {
    const make = this.model?.(geometry.model);
    if (!make) return;
    let pool = this.meshes.get(geometry.model);
    if (!pool) {
      pool = { list: [], used: 0 };
      this.meshes.set(geometry.model, pool);
    }
    let mesh = pool.list[pool.used];
    if (!mesh) {
      mesh = make(geometry.blend === "additive");
      pool.list.push(mesh);
      this.scene.add(mesh);
    }
    pool.used++;
    mesh.visible = true;
    mesh.position.copy(pos);
    mesh.quaternion.copy(turn);
    mesh.rotateY(rotation);
    mesh.scale.setScalar(size);
    mesh.userData.material.color.setRGB(color[0], color[1], color[2]);
    mesh.userData.material.opacity = color[3];
  }
}
