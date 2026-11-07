import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";
import { sheathColor, vivid } from "./blaster-bolts.js";

// The Enhanced look of a sniper shot (Video > Sniper Shots). Presentation
// only: the round is the world's own bolt (a class with a FadeOutTime: the
// sniper rifles' 2000 m/s rounds), flown and cut by ordnance.js, a worker's
// or a server's records, or the page's predicted cosmetic. Where it goes,
// what it hits and when it hits are untouched. Classic keeps the 2004 streak
// (the ordnance phase's draw_bolt).
//
// A shot is drawn as a tracer, in the round's own LaserGlowColor (kept
// saturated: the Rebels' pale yellow stays yellow):
//
//   - a puff of vapour at the muzzle: the bolt flashes the water in the air
//     there, a small cloud that blooms out, throws wisps and drifts off.
//   - a short, hot bolt, drawn flying at TRACER.speed from the gun as drawn
//     to where the round ends (2000 m/s crosses 50 m inside two frames, too
//     fast to see). The world's impact effect and sound, and a head shot's
//     flourish, wait for it to arrive (defer, headshot).
//   - a thin tracer line along its path that cools from the gun outward, so
//     nothing is left by a gun that kicks and moves on, and a wisp of smoke
//     that stays where it was made, rises and tears apart.
//   - at the impact a flash, sparks thrown back off the surface, embers and
//     a light; a head shot adds a gold star and more sparks.
//
// In the shooter's own eye view the bolt leaves his first-person gun as it
// is drawn each frame, fades in over its first metre, and his smoke starts
// a little way out.
//
// Bolts, tracers, vapour and sparks are one instanced draw of capsules,
// shaded per pixel from the closest distance between the view ray and the
// segment (as blaster-bolts.js draws plasma); flares are a second instanced
// draw of quads.

export const sniperLook = { enhanced: false };

const MAX_RECORDS = 48;
const NEAR_EYE = 2.5; // m: a muzzle this near the camera is the shooter's own eye view
const OWN_CLEAR = 0.6; // m of his own shot left out right in front of his eye
const OWN_FADE_IN = 1.2; // m his own bolt and tracer fade in over from the gun
const FADE_IN = 0.5; // m anyone else's do
const OWN_SMOKE_CLEAR = 2.5; // m of his own smoke left out in front of his gun
const HEAD_SPARKS = 26;
const SPARK_GRAVITY = 9.8;
const UP = new THREE.Vector3(0, 1, 0);
const SIDE = new THREE.Vector3(1, 0, 0);
const MERGE_SAME = 0.6; // m: a new source this close to a fresh record's ends is the same shot
const WHITE = [1, 1, 1];
const GOLD = [1, 0.82, 0.45];
const EMBER = [1, 0.5, 0.15];

// The look: a short, hot bolt drawn flying at
// `speed` (the round's 2000 m/s crosses 50 m inside two frames, too fast to
// be seen), `length` m long with radii `core` and `glow`; its tail runs in to
// the impact over `collapse` s. It leaves a thin tracer line along its path
// for `after` s, cooling from the gun outward at `retreat` m/s, and a wisp of smoke that rises and tears apart over
// smoke.life. Its muzzle puffs: the bolt flashes the water in the air there
// to vapour, a small cloud that blooms out and drifts off over PUFF_LIFE.
const TRACER = { retreat: 140, speed: 650, length: 6, core: 0.045, glow: 0.17, bright: 2.0, collapse: 0.05, after: 0.6, afterGlow: 0.03, afterBright: 0.6,
  smoke: { life: 2.2, radius: 0.22, opacity: 0.5, rise: 0.12, glow: 0.4 }, sparks: 12, embers: 6, flash: 0.8, light: 4 };
const BURN = 0.03; // s the tracer burns whole before it cools from the gun
const PUFF_LIFE = 0.9; // s
const PUFF_WISPS = 7;
const VAPOUR = [0.85, 0.9, 1];

const QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]);
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
// Each batch is one interleaved instance buffer (one upload a frame): name,
// size, offset in floats.
const STREAK_ATTRIBUTES = [["iHead", 3, 0], ["iTail", 3, 3], ["iColor", 4, 6], ["iSize", 4, 10], ["iMisc", 4, 14]];
const STREAK_STRIDE = 18;
const FLARE_ATTRIBUTES = [["iCenter", 3, 0], ["iAxis", 3, 3], ["iColor", 4, 6], ["iParam", 4, 10]];
const FLARE_STRIDE = 14;











function instanced(attributes, stride, material, capacity, name, order) {
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
  geometry.setIndex(QUAD_INDEX);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = name;
  mesh.frustumCulled = false;
  mesh.renderOrder = order;
  mesh.visible = false;
  const batch = { geometry, mesh, data: null, buffer: null, range: { start: 0, count: 0 }, capacity: 0, count: 0, attributes, stride };
  grow(batch, capacity);
  return batch;
}

function grow(batch, capacity) {
  const data = new Float32Array(capacity * batch.stride);
  if (batch.data) data.set(batch.data);
  batch.data = data;
  batch.buffer = new THREE.InstancedInterleavedBuffer(data, batch.stride).setUsage(THREE.DynamicDrawUsage);
  for (const [name, size, offset] of batch.attributes) batch.geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(batch.buffer, size, offset));
  // three keeps the instance limit of the buffers it first bound until the
  // geometry is disposed: grown, the new instances would not be drawn.
  delete batch.geometry._maxInstanceCount;
  batch.capacity = capacity;
}

// warm: an empty batch draws one dark instance (all zero: no brightness,
// which its fragment shader lets go) so the program's pipeline is built.
function upload(batch, warm = false) {
  let drawn = batch.count;
  if (!drawn && warm && !batch.external) {
    batch.data.fill(0, 0, batch.stride);
    drawn = 1;
  }
  batch.geometry.instanceCount = drawn;
  // Drawn by another renderer (external: src/rust-render/effects.js reads
  // its data), it is shown to three never and uploads it nothing.
  batch.mesh.visible = drawn > 0 && !batch.external;
  if (!drawn || batch.external) return;
  // What was drawn, through one range kept for it (three empties the list
  // once it has uploaded).
  const ranges = batch.buffer.updateRanges;
  ranges.length = 0;
  batch.range.count = drawn * batch.stride;
  ranges.push(batch.range);
  batch.buffer.needsUpdate = true;
}

const premultiplied = (uniforms, key) => ownedEffectMaterial(key, {
  uniforms,
  transparent: true, depthWrite: false,
  blending: THREE.CustomBlending, blendEquation: THREE.AddEquation,
  blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
  side: THREE.DoubleSide, forceSinglePass: true,
});

const ease = (x) => 1 - (1 - x) * (1 - x);

// A sheath colour (largest channel 1) at least MIN_SATURATION saturated, its
// hue kept: the Rebels' pale yellow (224, 224, 160) would otherwise draw
// nearly white beside the Republic's blue, the CIS's red and the Empire's
// green.
const MIN_SATURATION = 0.65;
export function saturated(rgb) {
  const least = Math.min(...rgb), saturation = 1 - least;
  if (saturation >= MIN_SATURATION || saturation <= 1e-3) return rgb;
  const k = MIN_SATURATION / saturation;
  return rgb.map((c) => 1 - (1 - c) * k);
}

// A ParticleRenderer batch of its own kind (ParticleRenderer.custom): begin()
// and end() bracket each frame's bolts, and the ordnance phase hands it each
// sniper round it draws (touch). A shot outlives its bolt here: its vapour
// and sparks run on after the world has let the round go.
export class SniperTrace {
  // lights: the frame's LightPool; view: the render camera, when known (the
  // shooter's own eye view is told apart by how near it is to the muzzle).
  // muzzle: his first-person gun's muzzle as drawn now, or null.
  constructor(lights = null, view = null, muzzle = null) {
    this.lights = lights;
    this.view = view;
    this.muzzle = muzzle;
    this.waiting = [];
    this.puffs = [];
    this.records = [];
    this.bySource = new WeakMap();
    this.sparks = [];
    this.sparkPool = []; // spent sparks, reused
    this.bursts = [];
    this.time = null;
    // Frames left over which an empty batch still draws one dark instance
    // (upload): the driver builds a program's pipeline at its first real
    // draw, which would otherwise be the first shot.
    this.warming = 90;
    this.camera = new THREE.Vector3(0, 1e6, 0);
    this.pixelAngle = { value: 0.002 };
    const uniforms = {
      fade: lighting.fade, time: lighting.time, pixelAngle: this.pixelAngle,
      sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
      depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
    };
    // Sized for a busy fight (a dozen rifles' smoke, vapour and sparks at
    // once): growing mid-battle makes new buffers.
    this.streaks = instanced(STREAK_ATTRIBUTES, STREAK_STRIDE, premultiplied(uniforms, 'effects/sniper-streak'), 512, "sniper-trace", 11);
    this.flares = instanced(FLARE_ATTRIBUTES, FLARE_STRIDE, premultiplied({}, 'effects/sniper-flare'), 64, "sniper-flares", 12);
    this.flares.mesh.material.depthTest = true;
    this.mesh = new THREE.Group();
    this.mesh.name = "sniper-shots";
    this.mesh.add(this.streaks.mesh, this.flares.mesh);
    const size = new THREE.Vector2();
    this.streaks.mesh.onBeforeRender = (renderer, scene, camera) => {
      particleDepth.capture(renderer, scene, camera);
      if (!camera.isPerspectiveCamera) return;
      this.camera.setFromMatrixPosition(camera.matrixWorld);
      const target = renderer.getRenderTarget();
      const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
      this.pixelAngle.value = (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / Math.max(height, 1) / (camera.zoom || 1);
    };
    this.tmp = new THREE.Vector3();
    this.tmp2 = new THREE.Vector3();
    this.endPoint = new THREE.Vector3();
  }

  now() {
    return lighting.time.value;
  }

  eye() {
    const camera = this.view?.();
    return camera ? camera.getWorldPosition(this.tmp) : this.camera;
  }

  begin() {
    for (const record of this.records) record.touched = false;
  }

  // A sniper round as the ordnance phase's draw_bolt draws it this frame: from `start`
  // (the gun as drawn) to `head`, `ended` once its flight is over, `fade`
  // (1 to 0) what is left of it after.
  touch(source, def, start, head, direction, ended, fade, origin = start) {
    const t = this.now();
    let record = this.bySource.get(source);
    if (!record) {
      // One shot can be drawn from a new object (a replicated round rebuilt
      // from its terminal, or the authority's copy of a cosmetic): keep the
      // shot that is already there.
      record = this.records.find((r) => !r.touched && t - r.born < 0.5 && r.direction.dot(direction) > 0.999
        && (r.start.distanceTo(start) < MERGE_SAME || r.head.distanceTo(head) < MERGE_SAME));
      if (!record) record = this.add(def, start, direction, t, origin);
      this.bySource.set(source, record);
    }
    record.touched = true;
    record.source = source;
    record.target.copy(head);
    record.fade = fade;
    // The impact plays when the drawn bolt gets there (end).
    if (ended) record.ended = true;
  }

  add(def, start, direction, t, origin = start) {
    if (this.records.length >= MAX_RECORDS) this.records.shift();
    const color = saturated(vivid(sheathColor(def.LaserGlowColor || def.LightColor || [120, 160, 255, 255]), 1.4));
    const style = TRACER;
    const record = {
      def, color, style, born: t, landed: null, ended: false, touched: true, fade: 1, gone: null,
      start: start.clone(), head: start.clone(), target: start.clone(), direction: direction.clone().normalize(),
      origin: origin.clone(), drawn: direction.clone().normalize(), fired: start.clone(),
      reach: (def.Velocity || 0) * (def.LifeSpan || 0),
      own: start.distanceTo(this.eye()) < NEAR_EYE,
      seed: Math.random(), headshot: false,
    };
    // His own view: the gun is drawn with the camera, so the shot's start
    // rides with the camera too, staying on the barrel as he moves and the
    // view kicks; anywhere else it stays where it was fired.
    const camera = record.own ? this.view?.() : null;
    if (camera) { camera.updateMatrixWorld(); record.cameraLocal = camera.worldToLocal(start.clone()); }
    this.records.push(record);
    if (!record.own) this.burst({ kind: 0, at: record.start, color, radius: 0.45, life: 0.07, brightness: 1.2 });
    this.puff(record.start, record.direction, color);
    return record;
  }

  // Vapour flashed out of the air at the muzzle: a cloud along the shot's
  // first half metre, and wisps thrown out round it.
  puff(at, direction, color) {
    if (this.puffs.length > 24) this.puffs.shift();
    const across = new THREE.Vector3().crossVectors(direction, Math.abs(direction.y) < 0.9 ? UP : SIDE).normalize();
    const wisps = [];
    for (let i = 0; i < PUFF_WISPS; i++) {
      const angle = (i + Math.random() * 0.6) / PUFF_WISPS * Math.PI * 2;
      wisps.push(across.clone().applyAxisAngle(direction, angle).addScaledVector(direction, 0.3 + Math.random() * 0.4).normalize().multiplyScalar(1.2 + Math.random() * 1.2));
    }
    this.puffs.push({ at: at.clone(), direction: direction.clone(), color, born: this.now(), seed: Math.random(), wisps });
  }

  // The flight is over: an impact, unless it simply ran out of life in the air.
  land(record, t) {
    record.ended = true;
    record.landed = t;
    record.head.copy(record.target ?? record.head);
    for (const play of record.onLand?.splice(0) || []) play();
    const flown = record.head.distanceTo(record.start);
    if (record.reach && flown > record.reach - 1 && !record.headAt) return;
    record.impact = true;
    const { color, style } = record, at = record.head;
    this.burst({ kind: 0, at, color: WHITE, radius: 0.6 * style.flash, life: 0.12, brightness: 1.3, turn: record.seed * 3 });
    this.burst({ kind: 0, at, color, radius: 0.9 * style.flash, life: 0.45, brightness: 0.55, turn: record.seed * 3 });
    this.spray(at, record.direction, color, style.sparks, 1);
    this.embers(at, record.direction, style.embers);
    this.lights?.flash?.(at.clone().addScaledVector(record.direction, -0.4), color.map((c) => c * 1.5), 6 + 2 * style.flash, 0.4);
    if (record.headAt) { this.flourish(record.headAt, record.direction); record.headAt = null; }
  }

  // The world's "headshot" effect at `point`: the shot that made it, if
  // this page drew one there, gets the head shot's flourish.
  headshot(point) {
    const t = this.now();
    // Once a hit: the page's own round shows it as it lands, and the
    // world's effect for the same hit arrives later.
    this.heads = (this.heads || []).filter((h) => t - h.t < 1.2);
    if (this.heads.some((h) => h.point.distanceTo(point) < 1.5)) return;
    this.heads.push({ t, point: point.clone() });
    // The shot whose line passes it (its drawn head, or the target it has
    // been told of so far, may still be short of it).
    let record = null, best = 1.5;
    for (const r of this.records) {
      if (r.headshot || t - r.born > 1) continue;
      const d = this.offLine(r, point);
      if (d < best) { best = d; record = r; }
    }
    if (record) record.headshot = true;
    // Its drawn bolt still on the way: the flourish waits for it (land).
    if (record && record.landed === null) { record.headAt = point.clone(); return; }
    // No shot drawn there yet (the page's own round reports its hit before
    // the frame draws it): wait a moment for one.
    if (!record) { this.waiting.push({ point: point.clone(), until: t + 0.15 }); return; }
    this.flourish(point, record.direction);
  }

  // Head shots waiting for their shot: on the first record whose line
  // passes them, or on their own once the wait is over.
  claimWaiting(t) {
    this.waiting = this.waiting.filter((w) => {
      const record = this.records.find((r) => !r.headshot && r.landed === null && this.offLine(r, w.point) < 1.5);
      if (record) { record.headshot = true; record.headAt = w.point; return false; }
      if (t < w.until) return true;
      this.flourish(w.point, this.tmp.set(0, 0, 1));
      return false;
    });
  }

  flourish(point, direction) {
    this.burst({ kind: 0, at: point, color: GOLD, radius: 1.4, life: 0.16, brightness: 1.9, turn: 0.785 });
    this.spray(point, direction, GOLD, HEAD_SPARKS, 1.3);
    this.embers(point, direction, 10);
    this.lights?.flash?.(point.clone(), [1.6, 1.4, 1.1], 8, 0.45);
  }

  // The world's impact for a shot whose drawn bolt is still on its way:
  // `play` runs when it lands. False when no such shot is drawn here.
  defer(point, play) {
    const t = this.now();
    for (const r of this.records) {
      if (r.landed !== null || t - r.born > 1 || this.offLine(r, point) > 1.5) continue;
      (r.onLand ||= []).push(play);
      return true;
    }
    return false;
  }

  // How far `point` lies from a shot's line, ahead of its gun.
  offLine(r, point) {
    const v = this.tmp2.subVectors(point, r.origin), along = v.dot(r.direction);
    return along < 0 ? Infinity : v.addScaledVector(r.direction, -along).length();
  }

  burst(spec) {
    if (this.bursts.length > 64) this.bursts.shift();
    this.bursts.push({ ...spec, at: spec.at.clone(), axis: spec.axis?.clone() ?? null, born: this.now() + (spec.delay || 0) });
  }

  // Sparks thrown back off the surface, against the way the round came.
  spray(at, direction, color, count, speed) {
    for (let i = 0; i < count; i++) {
      const s = this.spark(at, false);
      const v = s.v.randomDirection();
      if (v.dot(direction) > 0) v.addScaledVector(direction, -2 * v.dot(direction));
      v.addScaledVector(direction, -0.6).normalize().multiplyScalar((3 + Math.random() * 11) * speed);
      s.life = 0.2 + Math.random() * 0.45;
      s.color = Math.random() < 0.5 ? WHITE : color;
      this.addSpark(s);
    }
  }

  // Slow, hot embers that fall and cool from white to orange.
  embers(at, direction, count) {
    for (let i = 0; i < count; i++) {
      const s = this.spark(at, true);
      const v = s.v.randomDirection().addScaledVector(direction, -0.8).normalize().multiplyScalar(1 + Math.random() * 3);
      v.y += 1.5;
      s.life = 0.6 + Math.random() * 0.7;
      s.color = EMBER;
      this.addSpark(s);
    }
  }

  // A spark at `at`, from the spent ones when there are any.
  spark(at, ember) {
    const s = this.sparkPool.pop() || { at: new THREE.Vector3(), from: new THREE.Vector3(), v: new THREE.Vector3(), life: 0, age: 0, color: null, ember: false };
    s.at.copy(at);
    s.from.copy(at);
    s.age = 0;
    s.ember = ember;
    return s;
  }

  addSpark(s) {
    if (this.sparks.length > 160) this.sparkPool.push(this.sparks.shift());
    this.sparks.push(s);
  }

  streak(head, tail, color, brightness, core, glow, kind, seed, age = 0, opacity = 0, offset = 0, near = 0) {
    const b = this.streaks;
    if (b.count === b.capacity) grow(b, b.capacity * 2);
    const d = b.data, o = b.count++ * STREAK_STRIDE;
    d[o] = head.x; d[o + 1] = head.y; d[o + 2] = head.z;
    d[o + 3] = tail.x; d[o + 4] = tail.y; d[o + 5] = tail.z;
    d[o + 6] = color[0]; d[o + 7] = color[1]; d[o + 8] = color[2]; d[o + 9] = brightness;
    d[o + 10] = core; d[o + 11] = glow; d[o + 12] = kind; d[o + 13] = seed;
    d[o + 14] = age; d[o + 15] = opacity; d[o + 16] = offset; d[o + 17] = near;
  }

  flare(center, axis, color, brightness, radius, width, kind, turn = 0) {
    const b = this.flares;
    if (b.count === b.capacity) grow(b, b.capacity * 2);
    const d = b.data, o = b.count++ * FLARE_STRIDE;
    d[o] = center.x; d[o + 1] = center.y; d[o + 2] = center.z;
    d[o + 3] = axis?.x || 0; d[o + 4] = axis?.y || 0; d[o + 5] = axis?.z || 0;
    d[o + 6] = color[0]; d[o + 7] = color[1]; d[o + 8] = color[2]; d[o + 9] = brightness;
    d[o + 10] = radius; d[o + 11] = width; d[o + 12] = kind; d[o + 13] = turn;
  }

  end() {
    const t = this.now();
    const dt = this.time === null ? 0 : Math.min(Math.max(t - this.time, 0), 0.1);
    this.time = t;
    this.streaks.count = 0;
    this.flares.count = 0;
    if (this.waiting.length) this.claimWaiting(t);
    // Each list is kept in place, the spent left out (no new array a
    // frame).
    const records = this.records;
    let kept = 0;
    for (let i = 0; i < records.length; i++) if (this.drawRecord(records[i], t)) records[kept++] = records[i];
    records.length = kept;
    const puffs = this.puffs;
    kept = 0;
    for (let i = 0; i < puffs.length; i++) if (this.drawPuff(puffs[i], t)) puffs[kept++] = puffs[i];
    puffs.length = kept;
    const bursts = this.bursts;
    kept = 0;
    for (let i = 0; i < bursts.length; i++) {
      const b = bursts[i];
      const age = t - b.born;
      if (age >= 0) {
        const k = Math.min(age / b.life, 1);
        if (b.kind === 0) this.flare(b.at, null, b.color, b.brightness * (1 - k) * (1 - k), b.radius * (0.7 + 0.5 * ease(k)), 0, 0, b.turn || 0);
        else this.flare(b.at, b.axis, b.color, b.brightness * (1 - k), b.radius[0] + (b.radius[1] - b.radius[0]) * ease(k), b.width * (1 - 0.5 * k), 1);
      }
      if (age < b.life) bursts[kept++] = b;
    }
    bursts.length = kept;
    const sparks = this.sparks, tail = this.tmp2;
    kept = 0;
    for (let i = 0; i < sparks.length; i++) {
      const s = sparks[i];
      s.age += dt;
      s.from.copy(s.at);
      s.v.y -= SPARK_GRAVITY * dt;
      s.v.multiplyScalar(Math.exp(-dt * 2.5));
      s.at.addScaledVector(s.v, dt);
      const k = 1 - s.age / s.life;
      if (k > 0) {
        tail.copy(s.at).addScaledVector(s.v, s.ember ? -0.012 : -0.03);
        if (s.ember) this.streak(s.at, tail, k > 0.6 ? WHITE : s.color, 1.1 * k, 0.01, 0.035, 2, 0);
        else this.streak(s.at, tail, s.color, 1.4 * k, 0.012, 0.05, 2, 0);
      }
      if (s.age < s.life) sparks[kept++] = s;
      else this.sparkPool.push(s);
    }
    sparks.length = kept;
    const warm = this.warming > 0;
    if (warm) this.warming--;
    upload(this.streaks, warm);
    upload(this.flares, warm);
  }

  // A shot's frame; false once nothing of it is left.
  drawRecord(r, t) {
    const lift = this.tmp, tail = this.tmp2;
    const age = t - r.born, style = r.style;
    // Its bolt gone (or never touched again): it has landed.
    if (!r.touched) { r.gone ??= t; r.ended = true; }
    // The bolt is drawn flying at the style's speed, slower than the
    // round (2000 m/s crosses 30 m inside one frame); the hit itself is
    // the world's, already decided. It lands when the drawn bolt arrives.
    // Drawn from the gun as drawn to where the round ends (or, still in
    // flight, the far end of its line): the round itself leaves from the
    // gun origin beside the eye, so running the drawn bolt along its line
    // from the muzzle put it beside the real path, at an angle to the gun.
    // His own shot leaves his first-person gun as it is drawn this frame
    // (it kicks and settles after the shot), else rides with the camera.
    const muzzle = r.own ? this.muzzle?.() : null;
    const camera = r.cameraLocal ? this.view?.() : null;
    if (muzzle && muzzle.distanceTo(r.start) < 1.5) r.start.copy(muzzle);
    else if (camera) r.start.copy(r.cameraLocal).applyMatrix4(camera.matrixWorld);
    if (r.ended) this.endPoint.copy(r.target);
    else this.endPoint.copy(r.origin).addScaledVector(r.direction, r.reach || 300);
    r.drawn.subVectors(this.endPoint, r.start);
    if (r.drawn.lengthSq() > 1e-8) r.drawn.normalize(); else r.drawn.copy(r.direction);
    const reach = r.ended ? r.target.distanceTo(r.start) : Infinity;
    const flown = Math.min(reach, style.speed * age);
    if (r.landed === null) {
      r.head.copy(r.start).addScaledVector(r.drawn, flown);
      if (r.ended && flown >= reach) this.land(r, t);
    }
    const after = r.landed === null ? 0 : t - r.landed;
    const smoke = style.smoke;
    if (r.landed !== null && after > style.collapse && after > style.after && (!smoke || age > smoke.life)) return false;
    const length = r.head.distanceTo(r.start);
    // Not from inside the shooter's own eye, and faded in from the gun.
    const clear = r.own ? Math.min(OWN_CLEAR, length) : 0;
    const fadeIn = r.own ? OWN_FADE_IN : FADE_IN;
    // The bolt: a streak `length` long racing to the target, its tail
    // running in to the impact once it has struck.
    const back = Math.max(0, length - style.length);
    const k = r.landed === null ? 0 : Math.min(1, after / style.collapse);
    const tailAt = Math.max(clear, back + (length - back) * k);
    if (k < 1 && length - tailAt > 0.05) {
      tail.copy(r.start).addScaledVector(r.drawn, tailAt);
      const thin = r.own ? 0.6 : 1;
      this.streak(r.head, tail, r.color, style.bright * (1 - 0.5 * k), style.core * thin, style.glow * thin, 2, r.seed, age, fadeIn, tailAt);
      if (!r.own) this.lights?.add?.(r.head, r.color, style.light);
    }
    // What it leaves for a moment along its whole path.
    const glowLeft = r.landed === null ? 1 : 1 - after / style.after;
    // It cools from the gun outward: its near end runs off down the path,
    // so nothing is left by the gun to swing with its kick.
    const cooled = Math.max(clear, (age - BURN) * style.retreat);
    if (glowLeft > 0 && length > cooled + 0.5) {
      tail.copy(r.start).addScaledVector(r.drawn, cooled);
      this.streak(r.head, tail, r.color, style.afterBright * glowLeft * glowLeft, style.afterGlow * 0.3, style.afterGlow * (0.6 + 0.4 * glowLeft), 0, r.seed, age, fadeIn, cooled);
    }
    // Smoke, for the styles that leave it; it rises as it ages.
    if (smoke) {
      const left = Math.max(0, 1 - age / smoke.life);
      if (left > 0 && length > clear + OWN_SMOKE_CLEAR * Number(r.own)) {
        const rise = smoke.rise * age;
        const radius = 0.03 + smoke.radius * ease(Math.min(age / (smoke.life * 0.6), 1));
        const opacity = smoke.opacity * Math.pow(left, 1.3) * Math.min(1, age / 0.04 + 0.2);
        // Smoke stays where it was made: from where the gun was when it
        // fired, not on the gun as it moves on.
        const from = r.fired, reachSmoke = r.head.distanceTo(from), skip = r.own ? Math.min(OWN_SMOKE_CLEAR, reachSmoke) : 0;
        lift.copy(r.head).y += rise;
        tail.subVectors(r.head, from).multiplyScalar(skip / Math.max(reachSmoke, 1e-6)).add(from).y += rise;
        this.streak(lift, tail, r.color, (smoke.glow || 0.5) * Math.exp(-age * 4), 0, radius * 2, 1, r.seed, age, opacity * (r.own ? 0.75 : 1), skip);
      }
    }
    return true;
  }

  // Muzzle puffs: they bloom out fast, then slow, swell and rise as they
  // thin. Drawn right in front of the shooter's own eye too. False once it
  // is gone.
  drawPuff(f, t) {
    const lift = this.tmp, tail = this.tmp2;
    const age = t - f.born;
    if (age > PUFF_LIFE) return false;
    const k = age / PUFF_LIFE, out = 1 - Math.exp(-age * 6), opacity = 0.42 * Math.pow(1 - k, 1.6);
    const glow = 0.35 * Math.exp(-age * 16);
    lift.copy(f.at).addScaledVector(f.direction, 0.15 + 0.5 * out).y += 0.12 * age;
    tail.copy(f.at).addScaledVector(f.direction, 0.05 * out).y += 0.12 * age;
    this.streak(lift, tail, VAPOUR, glow, 0, (0.05 + 0.24 * ease(Math.min(age / 0.5, 1))) * 2, 1, f.seed, age * 0.5, opacity, 0, 1);
    for (let i = 0; i < f.wisps.length; i++) {
      const w = f.wisps[i];
      lift.copy(f.at).addScaledVector(w, 0.3 * out).y += 0.15 * age;
      tail.copy(lift).addScaledVector(w, -0.08);
      this.streak(lift, tail, VAPOUR, glow * 0.3, 0, (0.03 + 0.1 * ease(Math.min(age / 0.4, 1))) * 2, 1, f.seed + i, age * 0.5, opacity * 0.7, 0, 1);
    }
    return true;
  }
}
