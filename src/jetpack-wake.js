import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";

const MAX_NOZZLES = 4;
const ATTRIBUTES = { birth: 4, flow: 4, axis: 4, style: 2 };
const TYPES = [
  { name: "combustion", capacity: 96, rate: 36, life: [0.16, 0.27], drag: 7, opacity: 0.8 },
  { name: "smoke", capacity: 128, rate: 15, life: [0.65, 1.05], drag: 3, opacity: 0.12 },
  { name: "embers", capacity: 64, rate: 4.5, life: [0.2, 0.45], drag: 1.8, opacity: 0.75 },
];

const VERTEX = /* glsl */ `
  attribute vec4 birth; // position, exact emission time
  attribute vec4 flow;  // initial world velocity, lifetime
  attribute vec4 axis;  // exhaust axis, initial radius
  attribute vec2 style; // random phase, opacity
  uniform float time;
  uniform float kind;
  uniform float drag;
  varying vec2 wakeUv;
  varying vec4 wakeState;
  varying float viewDepth;
  void main() {
    float seconds = max(0.0, time - birth.w);
    float age = clamp(seconds / flow.w, 0.0, 1.0);
    vec3 direction = normalize(axis.xyz);
    vec3 tangent = cross(direction, abs(direction.y) > 0.9 ? vec3(1,0,0) : vec3(0,1,0));
    tangent = normalize(tangent);
    vec3 bitangent = cross(direction, tangent);
    float seed = style.x;
    float curl = axis.w * (kind > 0.5 && kind < 1.5 ? 1.8 : 0.7) * age;
    float phase = seed + seconds * (3.0 + fract(seed) * 4.0);
    vec3 vortex = tangent * (sin(phase) - sin(seed)) + bitangent * (cos(phase * 1.31) - cos(seed * 1.31));
    vec3 at = birth.xyz + flow.xyz * ((1.0 - exp(-drag * seconds)) / drag) + vortex * curl;
    at.y += seconds * seconds * (kind > 1.5 ? -1.0 : 0.35);
    if (kind > 0.5 && kind < 1.5) at.xz += vec2(0.10, -0.07) * seconds * seconds;
    vec4 view = viewMatrix * vec4(at, 1.0);
    vec2 motion = (viewMatrix * vec4(flow.xyz, 0.0)).xy;
    vec2 along = normalize(motion + vec2(0.0001));
    vec2 across = vec2(along.y, -along.x);
    float radius = axis.w * (kind > 1.5 ? 1.0 : mix(0.65, kind > 0.5 ? 2.0 : 1.5, age));
    float stretch = kind > 1.5 ? 3.8 : (kind > 0.5 ? 1.35 : 2.2);
    // Tilt each wisp around the velocity axis so the wake does not become
    // a stack of identically oriented circles or a single continuous sheet.
    float twist = kind > 1.5 ? 0.0 : sin(seed + age * 2.0) * 0.65;
    vec2 p = mat2(cos(twist), -sin(twist), sin(twist), cos(twist)) * position.xy;
    view.xy += across * p.x * radius + along * p.y * radius * stretch;
    wakeUv = uv;
    wakeState = vec4(age, seed, style.y, seconds);
    viewDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform float kind;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 wakeUv;
  varying vec4 wakeState;
  varying float viewDepth;
  float hash(vec2 p) { return fract(sin(dot(p,vec2(127.1,311.7))) * 43758.5453); }
  float noise(vec2 p) {
    vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);
  }
  void main() {
    float age = wakeState.x, seed = wakeState.y;
    vec2 q = wakeUv * 2.0 - 1.0;
    float radial = max(0.0, 1.0 - dot(q,q));
    float alpha = radial * radial * smoothstep(0.0, 0.08, age) * pow(1.0-age, 1.5);
    vec3 color;
    if (kind > 1.5) {
      color = mix(vec3(1.6,0.75,0.19), vec3(0.7,0.16,0.025), age);
    } else {
      vec2 advect = vec2(seed, -wakeState.w * 2.0);
      vec2 p = wakeUv * vec2(5.0,8.0) + advect;
      float bend = noise(p * 0.53 + seed) - 0.5;
      p.x += bend * 1.4;
      float detail = noise(p) * 0.6 + noise(p * 2.03 + 7.1) * 0.4;
      // Two wandering filaments break up the outline; empty regions remain
      // transparent rather than reading as uniformly filled smoke balloons.
      float filament = 1.0-smoothstep(0.08,0.48,abs(q.x + bend * 0.8 + sin(q.y*4.0+seed+age*2.0)*0.23));
      alpha *= kind < 0.5
        ? smoothstep(0.14,0.65,detail) * mix(0.45,1.0,filament)
        : smoothstep(0.24,0.69,detail) * mix(0.25,1.0,filament);
      if (kind < 0.5) {
        float heat = 1.0-smoothstep(0.20,0.85,age);
        color = mix(vec3(0.28,0.10,0.035),vec3(2.1,0.72,0.08),heat);
        color += vec3(1.2,0.82,0.3) * pow(detail,3.0) * heat;
      } else {
        color = mix(vec3(0.25,0.27,0.29),vec3(0.47,0.45,0.40),detail);
        // A little residual heat at the start, quickly cooling into vapor.
        color += vec3(0.09,0.035,0.006) * pow(1.0-age,5.0);
      }
    }
    alpha *= wakeState.z * smoothstep(0.2,0.8,viewDepth);
    if (softParticles > 0.5) {
      float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).x;
      float surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));
      alpha *= smoothstep(0.0,kind>1.5?0.035:0.14,surface-viewDepth);
    }
    if (alpha < 0.0015) discard;
    float fog=clamp((viewDepth-fogRange.x)/max(1.0,fogRange.y-fogRange.x),0.0,1.0);
    if(kind>0.5&&kind<1.5) color=mix(color,fogColor,fog);
    else alpha*=1.0-fog;
    // Premultiply emissive radiance before the normalized target clamps it.
    // Otherwise hot red (>1) clips first and the fire turns muddy yellow.
    gl_FragColor=vec4(kind>0.5&&kind<1.5 ? color : color*alpha,alpha);
  }
`;

class WakePool {
  constructor(scene, kind, clock) {
    this.kind = kind;
    this.spec = TYPES[kind];
    this.capacity = this.spec.capacity;
    this.count = 0;
    this.dirty = false;
    this.attributes = {};
    const plane = new THREE.PlaneGeometry(2, 2);
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.index = plane.index;
    geometry.attributes.position = plane.attributes.position;
    geometry.attributes.uv = plane.attributes.uv;
    for (const [name, size] of Object.entries(ATTRIBUTES)) {
      const attribute = new THREE.InstancedBufferAttribute(new Float32Array(this.capacity * size), size).setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute(name, attribute);
      this.attributes[name] = attribute;
    }
    this.attributeList = Object.values(this.attributes);
    geometry.instanceCount = 0;
    const material = new THREE.ShaderMaterial({
      uniforms: {
        time: clock, kind: { value: kind }, drag: { value: this.spec.drag },
        fogColor: lighting.fogColor, fogRange: lighting.fogRange,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true,
      depthTest: true, depthWrite: false, toneMapped: false,
      blending: kind === 1 ? THREE.NormalBlending : THREE.AdditiveBlending,
      premultipliedAlpha: kind !== 1,
    });
    this.mesh = new THREE.Mesh(geometry, material);
    this.mesh.name = `jetpack-wake-${this.spec.name}`;
    this.mesh.visible = false;
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = kind === 1 ? 8 : 11;
    this.mesh.onBeforeRender = (...args) => particleDepth.capture(...args);
    scene.add(this.mesh);
  }

  emit(time, position, direction, velocity, radius, throttle, random) {
    if (this.count >= this.capacity) return false;
    const n = this.count++, i = n * 4, j = n * 2;
    const { birth, flow, axis, style } = this.attributes;
    const speed = this.kind === 0 ? 4 + random() * 2 : this.kind === 1 ? 1.8 + random() * 1.1 : 4 + random() * 3;
    const downstream = this.kind === 0 ? 0.12 + random() * 0.08 : this.kind === 1 ? 0.36 + random() * 0.12 : 0.15;
    birth.array[i] = position.x + direction.x * downstream;
    birth.array[i + 1] = position.y + direction.y * downstream;
    birth.array[i + 2] = position.z + direction.z * downstream;
    birth.array[i + 3] = time;
    const spread = this.kind === 2 ? 0.9 : 0.3;
    flow.array[i] = velocity.x * 0.72 + direction.x * speed + (random() - 0.5) * spread;
    flow.array[i + 1] = velocity.y * 0.72 + direction.y * speed + (random() - 0.5) * spread;
    flow.array[i + 2] = velocity.z * 0.72 + direction.z * speed + (random() - 0.5) * spread;
    flow.array[i + 3] = THREE.MathUtils.lerp(this.spec.life[0], this.spec.life[1], random());
    axis.array[i] = direction.x;
    axis.array[i + 1] = direction.y;
    axis.array[i + 2] = direction.z;
    axis.array[i + 3] = this.kind === 0 ? radius * (1.1 + random()) : this.kind === 1 ? radius * (2.0 + random() * 1.1) : 0.004 + random() * 0.003;
    style.array[j] = random() * 100;
    style.array[j + 1] = this.spec.opacity * (0.6 + throttle * 0.4);
    this.dirty = true;
    return true;
  }

  update(time) {
    const birth = this.attributes.birth.array, flow = this.attributes.flow.array;
    for (let n = this.count - 1; n >= 0; n--) {
      if (time - birth[n * 4 + 3] < flow[n * 4 + 3]) continue;
      this.count--;
      for (const attribute of this.attributeList) {
        const size = attribute.itemSize;
        attribute.array.copyWithin(n * size, this.count * size, (this.count + 1) * size);
      }
      this.dirty = true;
    }
    if (this.dirty) {
      for (const attribute of this.attributeList) {
        attribute.clearUpdateRanges();
        if (this.count) {
          attribute.addUpdateRange(0, this.count * attribute.itemSize);
          attribute.needsUpdate = true;
        }
      }
      this.dirty = false;
    }
    this.mesh.geometry.instanceCount = this.count;
    this.mesh.visible = this.count > 0;
  }

  clear() {
    this.count = 0;
    this.mesh.visible = false;
    this.mesh.geometry.instanceCount = 0;
    this.dirty = false;
  }
}

export class JetpackWake {
  constructor({ scene, random = Math.random }) {
    this.random = random;
    this.time = 0;
    this.emitted = 0;
    this.clock = { value: 0 };
    this.pools = TYPES.map((_, kind) => new WakePool(scene, kind, this.clock));
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
    this.clock.value = this.time;
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
          this.position.lerpVectors(start, nozzle.position, fraction);
          this.direction.lerpVectors(oldDirection, nozzle.direction, fraction).normalize();
          if (pool.emit(this.time - dt + fraction * dt, this.position, this.direction, velocity, nozzle.radius || 0.034, throttle, this.random)) this.emitted++;
        }
      }
      previous.position.copy(nozzle.position);
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
      pool.mesh.geometry.dispose();
      pool.mesh.material.dispose();
    }
  }
}
