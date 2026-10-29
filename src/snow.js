import * as THREE from 'three';
import { EngineRandom } from './atmosphere.js';
import { particleDepth } from './particle-depth.js';
import { RainCover } from './weather.js';

// Snow: a world's Precipitation of Type("Quads") (Hoth), as the 2004 engine
// draws it. `ParticleDensity` flakes fill a 5 m box, and the box repeats over
// the world's 5 m grid out to `Range` in view: the flakes keep their place in
// the world as the camera moves through them. Each falls at `Velocity` with
// the wind, wandering up to `VelocityRange` along each axis; every half
// second its wander, its alpha (one end of AlphaMinMax or the other) and its
// spin (Rotation ± RotationRange / 2 turns a second) ease a fifth of the way
// toward new random ones. A flake is a camera-facing square reaching
// `ParticleSize` from its centre to each side, the texture tinted `Color`;
// boxes fade out over the last third of the range. Roofs stop it, as they
// stop the rain (weather.js's cover cache, standing in for the engine's
// rainshadow regions).
//
// The flakes (a few hundred) move here; one instanced draw repeats them over
// the boxes in view, nearest last.

export const BOX = 5;
const MAX_FLAKES = 32768;
const MAX_BOXES = 1024;

// The wind (the world's Effect("Wind")): its velocity eases from one target
// to the next, a new target every 1 / VelocityChangeRate seconds, each the
// authored velocity give or take VelocityRange.
export class Wind {
  constructor({ velocity = [0, 0], range = 0, rate = 0 } = {}, random = new EngineRandom(5)) {
    this.base = velocity; this.range = range; this.rate = rate; this.random = random;
    this.from = [...velocity]; this.to = this.target(); this.t = 0;
    this.velocity = new THREE.Vector3(velocity[0], 0, velocity[1]);
  }
  target() {
    if (!(this.range > 0)) return [...this.base];
    return [this.base[0] + (this.random.float() * 2 - 1) * this.range, this.base[1] + (this.random.float() * 2 - 1) * this.range];
  }
  update(dt) {
    if (this.rate > 0) {
      this.t += this.rate * dt;
      while (this.t >= 1) { this.t -= 1; this.from = this.to; this.to = this.target(); }
    }
    const t = this.rate > 0 ? this.t : 0;
    this.velocity.set(this.from[0] + (this.to[0] - this.from[0]) * t, 0, this.from[1] + (this.to[1] - this.from[1]) * t);
    return this.velocity;
  }
}

// The flakes of one box: positions in a unit box, each with its wander,
// alpha, spin (turns a second) and turn (0..1).
export class SnowFlakes {
  constructor(p, random = new EngineRandom(3)) {
    const get = (key, fallback) => p[key]?.[0] ?? fallback;
    this.random = random;
    this.count = THREE.MathUtils.clamp(Math.round(get('ParticleDensity', 30) + get('ParticleDensityRange', 0)), 1, 2048);
    this.fall = get('Velocity', 10); this.wander = get('VelocityRange', 1);
    const [alphaMin, alphaMax = alphaMin] = p.AlphaMinMax || [0.5, 0.5];
    this.alpha = [alphaMin, alphaMax];
    this.spin = [get('Rotation', 0), get('RotationRange', 2)];
    this.position = new Float32Array(this.count * 3);
    this.velocity = new Float32Array(this.count * 3);
    this.alphas = new Float32Array(this.count);
    this.spins = new Float32Array(this.count);
    this.turns = new Float32Array(this.count);
    for (let i = 0; i < this.count; i++) {
      for (let k = 0; k < 3; k++) this.position[i * 3 + k] = random.float();
      this.alphas[i] = alphaMin + random.float() * (alphaMax - alphaMin);
      this.spins[i] = this.spin[0] + (random.float() - 0.5) * this.spin[1];
      this.turns[i] = random.float();
      // A random direction, VelocityRange fast.
      const z = random.float() * 2 - 1, a = random.float() * Math.PI * 2, r = Math.sqrt(1 - z * z);
      this.velocity.set([Math.cos(a) * r * this.wander, z * this.wander, Math.sin(a) * r * this.wander], i * 3);
    }
    this.ease = 0;
  }
  // The engine's half-second easing: wander toward (±1, ±1, ±1) x
  // VelocityRange, alpha toward an end of its range, spin toward Rotation
  // ± half RotationRange.
  retarget(t) {
    const r = this.random, pick = () => r.int() & 1;
    for (let i = 0; i < this.count; i++) {
      if (this.wander > 0) for (let k = 0; k < 3; k++) {
        const v = this.velocity[i * 3 + k];
        this.velocity[i * 3 + k] = v + ((pick() * 2 - 1) * this.wander - v) * t;
      }
      if (this.alpha[1] > this.alpha[0]) this.alphas[i] += (this.alpha[0] + pick() * (this.alpha[1] - this.alpha[0]) - this.alphas[i]) * t;
      if (this.spin[1] > 0) this.spins[i] += (this.spin[0] + (pick() - 0.5) * this.spin[1] - this.spins[i]) * t;
    }
  }
  update(dt, wind) {
    this.ease += 0.4 * dt;
    if (this.ease > 0.2) { this.ease = Math.min(this.ease, 1); this.retarget(this.ease); this.ease -= 0.2; }
    const step = dt / BOX, fx = wind.x * step, fy = -this.fall * step, fz = wind.z * step;
    const p = this.position, v = this.velocity;
    for (let i = 0; i < this.count * 3; i += 3) {
      for (let k = 0; k < 3; k++) {
        const value = p[i + k] + (k === 0 ? fx : k === 1 ? fy : fz) + v[i + k] * step;
        p[i + k] = value - Math.floor(value);
      }
    }
    for (let i = 0; i < this.count; i++) { const turn = this.turns[i] + this.spins[i] * dt; this.turns[i] = turn - Math.floor(turn); }
  }
}

// The boxes in view: each 5 m cell of the world grid whose centre lies
// within `far` along the view and whose box the view frustum touches,
// farthest first, with its fade (1, falling to 0 over the last third of the
// range). fill(x, y, z, alpha) for each; returns the count.
const CELL = new THREE.Box3(), FOUND = [];
export function snowBoxes(camera, far, frustum, fill, limit = MAX_BOXES) {
  const e = camera.matrixWorld.elements, ex = e[12], ey = e[13], ez = e[14];
  const length = Math.hypot(e[8], e[9], e[10]) || 1, fx = -e[8] / length, fy = -e[9] / length, fz = -e[10] / length;
  // The frustum's corners at `far` lie this far from the eye at most.
  const tanY = Math.tan(THREE.MathUtils.degToRad(camera.fov ?? 90) / 2), tanX = tanY * (camera.aspect ?? 1);
  const reach = far * Math.sqrt(1 + tanX * tanX + tanY * tanY) + BOX;
  const x0 = Math.floor((ex - reach) / BOX), x1 = Math.floor((ex + reach) / BOX);
  const y0 = Math.floor((ey - reach) / BOX), y1 = Math.floor((ey + reach) / BOX);
  const z0 = Math.floor((ez - reach) / BOX), z1 = Math.floor((ez + reach) / BOX);
  const fadeFrom = far * 0.66, half = BOX / 2;
  let found = 0;
  for (let x = x0; x <= x1; x++) for (let y = y0; y <= y1; y++) for (let z = z0; z <= z1; z++) {
    const cx = (x + 0.5) * BOX, cy = (y + 0.5) * BOX, cz = (z + 0.5) * BOX;
    const depth = (cx - ex) * fx + (cy - ey) * fy + (cz - ez) * fz;
    if (depth >= far) continue;
    CELL.min.set(cx - half, cy - half, cz - half); CELL.max.set(cx + half, cy + half, cz + half);
    if (!frustum.intersectsBox(CELL)) continue;
    const item = FOUND[found] ||= [0, 0, 0, 0, 0];
    item[0] = depth; item[1] = cx; item[2] = cy; item[3] = cz; item[4] = depth > fadeFrom ? (far - depth) / (far - fadeFrom) : 1;
    found++;
  }
  const boxes = FOUND.slice(0, found).sort((a, b) => b[0] - a[0]);
  // Over the limit, the nearest boxes stay.
  const count = Math.min(found, limit);
  for (let i = found - count; i < found; i++) fill(boxes[i][1], boxes[i][2], boxes[i][3], boxes[i][4]);
  return count;
}

const VERTEX = /* glsl */ `
  attribute vec2 corner;
  uniform sampler2D flakes, boxes;
  uniform int flakeCount;
  uniform float size;
  uniform vec3 cameraRight, cameraUp;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying float vAlpha, vDepth;
  void main() {
    int flake = gl_InstanceID % flakeCount, cell = gl_InstanceID / flakeCount;
    vec4 at = texelFetch(flakes, ivec2(flake, 0), 0), spin = texelFetch(flakes, ivec2(flake, 1), 0);
    vec4 box = texelFetch(boxes, ivec2(cell, 0), 0);
    vec3 center = box.xyz + (at.xyz - 0.5) * ${BOX.toFixed(1)};
    float angle = spin.x * 6.2831853, c = cos(angle), s = sin(angle);
    vec2 turned = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y);
    vWorld = center + (cameraRight * turned.x + cameraUp * turned.y) * size;
    vec4 view = viewMatrix * vec4(vWorld, 1.0);
    vUv = corner * 0.5 + 0.5;
    vAlpha = at.w * box.w;
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D map, cover, sceneDepth;
  uniform vec4 tint;
  uniform float coverCell, coverSize, softParticles;
  uniform vec2 depthResolution, cameraRange;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying float vAlpha, vDepth;
  void main() {
    vec2 cell = mod(floor(vWorld.xz / coverCell), coverSize);
    if (vWorld.y < texture2D(cover, (cell + 0.5) / coverSize).r) discard;
    vec4 color = texture2D(map, vUv) * vec4(tint.rgb, tint.a * vAlpha);
    if (softParticles > 0.5) {
      float depth = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
      color.a *= clamp((surface - vDepth) / 0.05, 0.0, 1.0);
    }
    if (color.a < 0.002) discard;
    gl_FragColor = color;
  }
`;

const floatTexture = (width, height) => {
  const texture = new THREE.DataTexture(new Float32Array(width * height * 4), width, height, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.needsUpdate = true;
  return texture;
};

const FRUSTUM = new THREE.Frustum(), VIEW_PROJECTION = new THREE.Matrix4();

export class Snow {
  constructor({ data, scene, collision, water, texture }) {
    const p = data.precipitation;
    this.flakes = new SnowFlakes(p);
    this.wind = new Wind({ velocity: data.wind || [0, 0], ...(data.gusts || {}) });
    this.range = THREE.MathUtils.clamp(p.Range?.[0] ?? 10, 2, 60);
    this.boxLimit = Math.max(1, Math.min(MAX_BOXES, Math.floor(MAX_FLAKES / this.flakes.count)));
    this.cover = new RainCover(collision, this.range, water);
    this.flakeData = floatTexture(this.flakes.count, 2);
    this.boxData = floatTexture(this.boxLimit, 1);
    const c = p.Color || [192, 192, 192, 160];
    this.material = new THREE.ShaderMaterial({
      vertexShader: VERTEX, fragmentShader: FRAGMENT,
      uniforms: {
        map: { value: texture(p.Texture?.[0]) }, flakes: { value: this.flakeData }, boxes: { value: this.boxData },
        flakeCount: { value: this.flakes.count }, size: { value: p.ParticleSize?.[0] ?? 0.02 },
        cameraRight: { value: new THREE.Vector3(1, 0, 0) }, cameraUp: { value: new THREE.Vector3(0, 1, 0) },
        tint: { value: new THREE.Vector4(c[0] / 255, c[1] / 255, c[2] / 255, (c[3] ?? 255) / 255) },
        cover: { value: this.cover.texture }, coverCell: { value: this.cover.cell }, coverSize: { value: this.cover.size },
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true, blending: THREE.NormalBlending,
    });
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute('corner', new THREE.Float32BufferAttribute([-1, -1, 1, -1, 1, 1, -1, 1], 2));
    this.geometry.setIndex([0, 1, 2, 0, 2, 3]);
    this.geometry.instanceCount = 0;
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = 'map-snow';
    this.mesh.frustumCulled = false;
    this.mesh.visible = false;
    this.mesh.renderOrder = 20;
    this.mesh.onBeforeRender = (renderer, s, camera) => particleDepth.capture(renderer, s, camera);
    scene.add(this.mesh);
    this.initial = true;
    this.stats = { boxes: 0, flakes: 0 };
  }
  update(dt, camera) {
    dt = Math.min(Math.max(dt, 0), 0.25);
    this.flakes.update(dt, this.wind.update(dt));
    camera.updateMatrixWorld();
    const e = camera.matrixWorld.elements, u = this.material.uniforms;
    u.cameraRight.value.set(e[0], e[1], e[2]).normalize();
    u.cameraUp.value.set(e[4], e[5], e[6]).normalize();
    const covered = this.cover.update(camera.position, this.initial ? this.cover.heights.length : 96);
    this.initial = false;
    const flakes = this.flakes, f = this.flakeData.image.data, n = flakes.count;
    for (let i = 0; i < n; i++) {
      f[i * 4] = flakes.position[i * 3]; f[i * 4 + 1] = flakes.position[i * 3 + 1]; f[i * 4 + 2] = flakes.position[i * 3 + 2];
      f[i * 4 + 3] = flakes.alphas[i];
      f[(n + i) * 4] = flakes.turns[i];
    }
    this.flakeData.needsUpdate = true;
    FRUSTUM.setFromProjectionMatrix(VIEW_PROJECTION.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    const b = this.boxData.image.data;
    let boxes = 0;
    snowBoxes(camera, this.range, FRUSTUM, (x, y, z, alpha) => { b.set([x, y, z, alpha], boxes * 4); boxes++; }, this.boxLimit);
    this.boxData.needsUpdate = true;
    this.geometry.instanceCount = boxes * n;
    this.mesh.visible = covered && boxes > 0;
    this.stats.boxes = boxes; this.stats.flakes = boxes * n;
  }
  dispose() {
    this.mesh.removeFromParent(); this.geometry.dispose(); this.material.dispose();
    this.flakeData.dispose(); this.boxData.dispose(); this.cover.dispose();
  }
}
