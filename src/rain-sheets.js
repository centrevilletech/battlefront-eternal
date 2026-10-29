import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// Enhanced rain's middle distance: three open cylinders around the eye (4, 9
// and 18 m) scrolling a streak texture down and across with the wind, as in
// CryEngine's and Lagarde's rain. Individual drops (weather.js) stay near the
// camera; these give heavy rain its density and its sheets in the distance
// at a fixed cost: one draw, at most three thin layers. They keep to the
// world's axes, so turning doesn't drag them, stop under roofs (the drops'
// cover map), fade into geometry and take the scene's light, lightning
// included.
const RADII = [4, 9, 18], SEGMENTS = 48, ABOVE = 16, BELOW = 10;

// A tileable sheet of falling streaks: thin, varied in length and strength,
// pre-blurred across so they stay soft when minified.
function streakTexture(size = 256, count = 520) {
  const data = new Uint8Array(size * size * 4);
  let state = 2741;
  const random = () => ((state = Math.imul(state, 1664525) + 1013904223 >>> 0) / 4294967296);
  for (let n = 0; n < count; n++) {
    const x = random() * size, y0 = random() * size, length = 6 + random() * random() * 44, strength = 0.35 + random() * 0.65;
    for (let k = 0; k < length; k++) {
      const y = Math.floor(y0 + k) % size, fade = Math.sin(Math.PI * (k + 0.5) / length) * strength;
      for (let dx = -1; dx <= 1; dx++) {
        const px = ((Math.floor(x) + dx) % size + size) % size, w = dx === 0 ? 1 : 0.28;
        const at = (y * size + px) * 4;
        data[at + 3] = Math.min(255, data[at + 3] + Math.round(255 * fade * w));
      }
    }
  }
  for (let i = 0; i < data.length; i += 4) data[i] = data[i + 1] = data[i + 2] = 255;
  const texture = new THREE.DataTexture(data, size, size);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  return texture;
}

function cylinders() {
  const sheet = [], index = [];
  RADII.forEach((radius, layer) => {
    const base = sheet.length / 3;
    for (let j = 0; j <= SEGMENTS; j++) for (const k of [0, 1]) sheet.push(layer, j / SEGMENTS, k);
    for (let j = 0; j < SEGMENTS; j++) {
      const a = base + j * 2;
      index.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
    }
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('sheet', new THREE.Float32BufferAttribute(sheet, 3));
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(sheet.length), 3));
  geometry.setIndex(index);
  return geometry;
}

const vertexShader = /* glsl */`
  attribute vec3 sheet; // layer, around, bottom/top
  uniform vec3 eye;
  uniform vec3 radii;
  varying vec3 vWorld;
  varying vec2 vAround;
  varying float vLayer, vDepth;
  void main() {
    float radius = sheet.x < 0.5 ? radii.x : sheet.x < 1.5 ? radii.y : radii.z;
    float turn = sheet.y * 6.2831853;
    vec3 world = vec3(eye.x + cos(turn) * radius, eye.y + mix(-${BELOW.toFixed(1)}, ${ABOVE.toFixed(1)}, sheet.z), eye.z + sin(turn) * radius);
    vWorld = world;
    vAround = vec2(sheet.y * 6.2831853 * radius, sheet.z);
    vLayer = sheet.x;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;

const fragmentShader = /* glsl */`
  uniform sampler2D streaks, cover, sceneDepth;
  uniform float time, speed, coverCell, coverSize, softParticles, strength;
  uniform vec2 wind, depthResolution, cameraRange;
  uniform vec3 color, ambient;
  varying vec3 vWorld;
  varying vec2 vAround;
  varying float vLayer, vDepth;
  void main() {
    vec2 cell = mod(floor(vWorld.xz / coverCell), coverSize);
    if (vWorld.y < texture2D(cover, (cell + 0.5) / coverSize).r + 0.05) discard;
    // Farther sheets: finer, denser streaks moving more slowly on screen.
    float scale = vLayer < 0.5 ? 2.2 : vLayer < 1.5 ? 3.4 : 5.5;
    float slant = dot(wind, vec2(0.7, 0.7)) * 0.04;
    vec2 uv = vec2(vAround.x / scale + vWorld.y * slant, (vWorld.y + time * speed) / (scale * 2.4));
    float a = texture2D(streaks, uv).a * 0.65 + texture2D(streaks, uv * vec2(1.7, 1.3) + vec2(0.31, time * 0.21)).a * 0.35;
    float layer = vLayer < 0.5 ? 0.17 : vLayer < 1.5 ? 0.13 : 0.1;
    a *= layer * strength * smoothstep(0.0, 0.25, vAround.y) * (1.0 - smoothstep(0.75, 1.0, vAround.y));
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).r;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      a *= clamp((surface - vDepth) / 1.5, 0.0, 1.0);
    }
    if (a < 0.003) discard;
    // Drops are lit by the whole sky around them and pass most of it on: a
    // little brighter than the scene's ambient light, much more in a flash.
    float sky = dot(ambient, vec3(0.3, 0.59, 0.11));
    gl_FragColor = vec4(color * clamp(0.45 + sky * 1.6, 0.0, 1.4), a);
  }
`;

export class RainSheets {
  constructor(scene, uniforms) {
    this.texture = streakTexture();
    this.material = new THREE.ShaderMaterial({
      vertexShader, fragmentShader, transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true,
      uniforms: {
        streaks: { value: this.texture }, eye: uniforms.eye, time: uniforms.time, wind: uniforms.wind, color: uniforms.color,
        speed: { value: Math.max(6, uniforms.velocity.value * 1.4) }, radii: { value: new THREE.Vector3(...RADII) }, strength: { value: 1 },
        cover: uniforms.cover, coverCell: uniforms.coverCell, coverSize: uniforms.coverSize,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
        ambient: lighting.ambient,
      },
    });
    this.mesh = new THREE.Mesh(cylinders(), this.material);
    this.mesh.name = 'rain-sheets';
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 19;
    this.mesh.visible = false;
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    scene.add(this.mesh);
  }
  dispose() { this.mesh.removeFromParent(); this.mesh.geometry.dispose(); this.material.dispose(); this.texture.dispose(); }
}
