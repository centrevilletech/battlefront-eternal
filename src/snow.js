import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { SnowHost, loaded } from './environment/module.js';
import { Snow as LIMITS, SnowConfig as CONFIG, SnowResult as RESULT, View as VIEW } from './environment/layout.js';

const BOX = LIMITS.BOX;

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
// The flakes (a few hundred), the wind, the cover cache and the choice of
// boxes in view are a Rust phase's (rust/bfe-environment, snow.rs); one
// instanced draw repeats the flakes over the boxes, nearest last. What is
// here is the shell around them: the mesh, the material and the textures
// the shader reads the phase's records from.

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




// A float texture over records the Rust phase keeps.
const floatTexture = (data, width, height) => {
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.needsUpdate = true;
  return texture;
};

export class Snow {
  constructor({ data, scene, collision, water, texture }) {
    const p = data.precipitation;
    const get = (key, fallback) => p[key]?.[0] ?? fallback;
    // The flakes of one box: how many, and how they fall and ease.
    const count = THREE.MathUtils.clamp(Math.round(get('ParticleDensity', 30) + get('ParticleDensityRange', 0)), 1, 2048);
    const [alphaMin, alphaMax = alphaMin] = p.AlphaMinMax || [0.5, 0.5];
    const range = THREE.MathUtils.clamp(p.Range?.[0] ?? 10, 2, 60);
    const boxLimit = Math.max(1, Math.min(LIMITS.MAX_BOXES, Math.floor(LIMITS.MAX_FLAKES / count)));
    // A roof or floor over a cell: what a cast straight down first hits, or
    // open water over it if higher (the cover cache's query).
    const down = new THREE.Vector3(0, -1, 0), from = new THREE.Vector3(), hit = makeHit();
    const cover = (x, y, z) => {
      const found = collision.cast(from.set(x, y, z), down, 512, hit);
      let height = found ? found.point.y : -100000;
      if (water?.over(x, z)) height = Math.max(height, water.level);
      return height;
    };
    this.host = new SnowHost(loaded(), cover);
    const config = this.host.config;
    config[CONFIG.COUNT] = count;
    config[CONFIG.FALL] = get('Velocity', 10); config[CONFIG.WANDER] = get('VelocityRange', 1);
    config[CONFIG.ALPHA_MIN] = alphaMin; config[CONFIG.ALPHA_MAX] = alphaMax;
    config[CONFIG.SPIN_BASE] = get('Rotation', 0); config[CONFIG.SPIN_RANGE] = get('RotationRange', 2);
    const wind = data.wind || [0, 0];
    config[CONFIG.WIND] = wind[0]; config[CONFIG.WIND + 1] = wind[1];
    config[CONFIG.GUST_RANGE] = data.gusts?.range ?? 0; config[CONFIG.GUST_RATE] = data.gusts?.rate ?? 0;
    config[CONFIG.RANGE] = range;
    config[CONFIG.BOX_LIMIT] = boxLimit;
    this.host.start();
    this.flakeCount = count;
    this.range = range;
    this.boxLimit = boxLimit;
    // The cache of cover heights the shader clips snow against.
    const size = Math.sqrt(this.host.heights.length);
    const heights = new THREE.DataTexture(this.host.heights, size, size, THREE.RedFormat, THREE.FloatType);
    heights.minFilter = heights.magFilter = THREE.NearestFilter;
    heights.needsUpdate = true;
    this.cover = { heights: this.host.heights, texture: heights, cell: this.host.cell, size, queries: 0 };
    this.flakeData = floatTexture(this.host.flakes, count, 2);
    this.boxData = floatTexture(this.host.boxes, boxLimit, 1);
    const c = p.Color || [192, 192, 192, 160];
    this.material = new THREE.ShaderMaterial({
      vertexShader: VERTEX, fragmentShader: FRAGMENT,
      uniforms: {
        map: { value: texture(p.Texture?.[0]) }, flakes: { value: this.flakeData }, boxes: { value: this.boxData },
        flakeCount: { value: count }, size: { value: p.ParticleSize?.[0] ?? 0.02 },
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
    this.stats = { boxes: 0, flakes: 0 };
  }
  // The frame: the camera and the time go to the phase, which moves the
  // flakes and the wind, refreshes the cover cache and picks the boxes in
  // view; what it changed is marked for upload.
  update(dt, camera) {
    camera.updateMatrixWorld();
    const view = this.host.view;
    view[VIEW.DT] = dt;
    view.set(camera.matrixWorld.elements, VIEW.CAMERA);
    view.set(camera.matrixWorldInverse.elements, VIEW.INVERSE);
    view.set(camera.projectionMatrix.elements, VIEW.PROJECTION);
    view[VIEW.FOV] = camera.fov ?? NaN;
    view[VIEW.ASPECT] = camera.aspect ?? NaN;
    view[VIEW.POSITION] = camera.position.x; view[VIEW.POSITION + 1] = camera.position.y; view[VIEW.POSITION + 2] = camera.position.z;
    this.host.update();
    const r = this.host.result, u = this.material.uniforms;
    u.cameraRight.value.fromArray(r, RESULT.CAMERA_RIGHT);
    u.cameraUp.value.fromArray(r, RESULT.CAMERA_UP);
    if (r[RESULT.CHANGED]) this.cover.texture.needsUpdate = true;
    this.cover.queries = r[RESULT.QUERIES];
    this.flakeData.needsUpdate = true;
    this.boxData.needsUpdate = true;
    const boxes = r[RESULT.BOXES];
    this.geometry.instanceCount = r[RESULT.INSTANCES];
    this.mesh.visible = r[RESULT.COVERED] === 1 && boxes > 0;
    this.stats.boxes = boxes; this.stats.flakes = r[RESULT.INSTANCES];
  }
  dispose() {
    this.mesh.removeFromParent(); this.geometry.dispose(); this.material.dispose();
    this.flakeData.dispose(); this.boxData.dispose(); this.cover.texture.dispose();
  }
}
