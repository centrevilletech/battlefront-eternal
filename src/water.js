import { ownedEffectMaterial } from './rust-render/effect-material.js';
import { COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT } from "./render-resources.js";
import { WaterState } from "./runtime/water-state.js";
import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";
import { WATER_QUALITIES } from "./settings.js";
import { OceanSurface } from "./ocean-surface.js";
import { SurfaceHost } from "./environment/surfaces.js";
import { Surface as LIMITS, SurfaceConfig as CONFIG, SurfaceFrame as FRAME, SurfaceResult as RESULT } from "./environment/layout.js";

// Terrain water, as the 2004 game draws it on PC: a flat surface over the
// map's water cells (32 m squares on Theed) at the layer's height, bobbing
// 0.06 m at 0.3 Hz.
//
// Two passes over the same surface, after the opaque scene:
//   1. what lies under the water is multiplied by RefractionColor;
//   2. a reflection, at ReflectionColor's alpha, plus white sun glints
//      (N.H)^8, blended by a Fresnel term from FresnelMin looking straight
//      down to FresnelMax at grazing angles.
// The surface normal comes from the NormalMapTextures frames, cross-faded at
// their frame rate and tiled Tile times per cell; the reflection is jittered
// by the BumpMapTextures frames (0.03 of the screen, turned with the view).
//
// Low uses MainTexture and scrolling specular masks. High reflects nearby
// geometry in screen space; Ultra uses a full mirrored scene. Enhanced water resolves opaque color/depth once for
// depth-aware refraction/absorption, with animated world-space normal detail.
//
// Rings spread where things meet the water (radius growing 1.5 m/s, fading
// in over the first 15% of their life and out over the rest), and under the
// surface the screen takes UnderwaterColor.

const RIPPLE_MAX = LIMITS.RIPPLES;
const RIPPLE_SEGMENTS = LIMITS.SEGMENTS;
const DISTORT = 0.03;
const REFLECTION_SIZE = [512, 256];
// The layer the mirror draws the sky's models on, alone (reflect()).
const MIRROR_SKY_LAYER = 5;
const rgba = (c, fallback = [255, 255, 255, 255]) => {
  const [r, g, b, a = 255] = c || fallback;
  return [r / 255, g / 255, b / 255, a / 255];
};



// The far scene's copy of a pass (FAR_WATER, Water.farPass) draws from where
// the near scene ends (surfaceRange.x) out to the water's reach
// (surfaceRange.y), in the far scene's fog.




// Native low quality uses the authored scrolling colour texture and animated
// specular mask. It needs no mirrored scene or opaque framebuffer copy.


// Wading soldiers (xz, depth, activity) for contact foam and ripples.
const WADERS = 8;











// Whether the water is a sea rather than a pool or canal: its cells reach
// all four edges of the water grid (the map ends in water on every side).
export function isSea(data) {
  const n = data.grid;
  if (!n || !data.cells?.length) return false;
  let west = false, east = false, north = false, south = false;
  for (const [i, j] of data.cells) {
    if (i === 0) west = true;
    if (i === n - 1) east = true;
    if (j === 0) north = true;
    if (j === n - 1) south = true;
  }
  return west && east && north && south;
}

export class Water extends WaterState {
  // data: the map's water (tools/zero_water.py); texture(name) gives a map
  // texture; graphics draws the scene; weather is the map's (an Enhanced
  // ocean storms only where it rains, ocean-surface.js).
  constructor({ data, texture, graphics, weather = null, surfaceOwner = null }) {
    super(data);
    this.data = data;
    this.graphics = graphics;
    const look = data.look;
    this.host = surfaceOwner ? new SurfaceHost(surfaceOwner) : new SurfaceHost();
    this.host.config[CONFIG.HEIGHT] = data.height;
    this.host.config[CONFIG.BOBBING] = (look.OscillationEnable?.[0] ?? 1) > 0 ? 1 : 0;
    // The original draws an ocean where the look asks (OceanEnable). A sea
    // the look leaves flat (Kashyyyk's: water out to every edge of its
    // grid) takes the Enhanced ocean too, while Enhanced ocean effects are
    // on; Classic keeps the flat layer the 2004 game drew there.
    const oceanLook = (look.OceanEnable?.[0] || 0) > 0;
    const sea = !oceanLook && isSea(data);
    const ocean = oceanLook || sea;
    this.height = data.height;
    this.cell = data.cell;
    this.offset = data.offset;
    this.grid = data.grid;
    this.cells = new Set(data.cells.map(([i, j]) => j * data.grid + i));
    this.bobbing = (look.OscillationEnable?.[0] ?? 1) > 0;
    this.time = 0;
    this.wave = 0;
    const scale = data.scale || 1;
    const tile = look.Tile || [1, 1];
    this.frames = (key) => {
      const set = look[key];
      if (!set || set.length < 3) return null;
      const textures = Array.from({ length: Math.round(set[1]) }, (_, k) => texture(`${set[0]}${k}`)).filter(Boolean);
      return textures.length ? { textures, fps: set[2] } : null;
    };
    this.normals = this.frames("NormalMapTextures");
    this.bumps = this.frames("BumpMapTextures");
    this.masks = this.frames("SpecularMaskTextures");
    const config = this.host.config;
    config[CONFIG.NORMAL_COUNT] = this.normals?.textures.length || 0; config[CONFIG.NORMAL_FPS] = this.normals?.fps || 0;
    config[CONFIG.BUMP_COUNT] = this.bumps?.textures.length || 0; config[CONFIG.BUMP_FPS] = this.bumps?.fps || 0;
    config[CONFIG.MASK_COUNT] = this.masks?.textures.length || 0; config[CONFIG.MASK_FPS] = this.masks?.fps || 0;

    // The surface: a grid of PatchDivisions x scale quads per cell, and the
    // patched cells' own triangles, in world space at the layer's height.
    const divisions = look.PatchDivisions || [16, 16];
    const geometry = this.surface(ocean ? 1 : Math.max(1, Math.round(divisions[0] * scale)),
      ocean ? 1 : Math.max(1, Math.round((divisions[1] ?? divisions[0]) * scale)));
    const c0 = rgba(look.RefractionColor);
    const c1 = rgba(look.ReflectionColor);
    const fresnel = look.FresnelMinMax || [0.2, 0.9];

    this.target = new THREE.WebGLRenderTarget(REFLECTION_SIZE[0], REFLECTION_SIZE[1], { type: THREE.UnsignedByteType });
    this.mirror = new THREE.PerspectiveCamera();
    // A mirror: the Rust renderer draws what it took from three for it too
    // (src/rust-render: the near scene's passes from this camera).
    this.mirror.userData.mirror = true;
    this.plainProjection = new THREE.Matrix4();
    this.obliqueProjection = new THREE.Matrix4();
    this.reflectionMatrix = new THREE.Matrix4();
    this.tintPass = new THREE.Mesh(
      geometry,
      ownedEffectMaterial('effects/water-refraction', {
        uniforms: {
          tint: { value: new THREE.Vector3(c0[0], c0[1], c0[2]) },
          quality: { value: 0 }, time: { value: 0 }, underwater: { value: 0 },
          sceneColor: { value: null }, sceneDepth: { value: null },
          resolution: { value: new THREE.Vector2(1, 1) },
          cameraRange: { value: new THREE.Vector2(0.5, 200) },
          waders: { value: Array.from({ length: WADERS }, () => new THREE.Vector4()) },
        },
        transparent: true,
        depthWrite: true,
        side: THREE.DoubleSide,
        forceSinglePass: true,
        blending: THREE.CustomBlending,
        blendEquation: THREE.AddEquation,
        blendSrc: THREE.DstColorFactor,
        blendDst: THREE.OneMinusSrcAlphaFactor,
      }),
    );
    const blank = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
    blank.needsUpdate = true;
    this.mirrorPass = new THREE.Mesh(
      geometry,
      ownedEffectMaterial('effects/water-reflection', {
        uniforms: {
          reflection: { value: this.target.texture },
          quality: { value: 0 },
          time: { value: 0 },
          sunColor: lighting.sunColor,
          cameraRange: this.tintPass.material.uniforms.cameraRange,
          // Shared with the tint pass: its opaque depth copy and the waders.
          sceneDepth: this.tintPass.material.uniforms.sceneDepth,
          sceneColor: this.tintPass.material.uniforms.sceneColor,
          screenSpace: { value: 0 },
          resolution: this.tintPass.material.uniforms.resolution,
          waders: this.tintPass.material.uniforms.waders,
          reflectionMatrix: { value: this.reflectionMatrix },
          normalA: { value: this.normals?.textures[0] || blank },
          normalB: { value: this.normals?.textures[0] || blank },
          normalMix: { value: 0 },
          bumpA: { value: this.bumps?.textures[0] || blank },
          bumpB: { value: this.bumps?.textures[0] || blank },
          bumpMix: { value: 0 },
          distort: { value: this.bumps ? DISTORT : 0 },
          viewYaw: { value: 0 },
          tile: { value: new THREE.Vector2(tile[0] * scale, (tile[1] ?? tile[0]) * scale) },
          toSun: { value: new THREE.Vector3(0, 1, 0) },
          reflectAmount: { value: c1[3] },
          fresnel: { value: new THREE.Vector2(fresnel[0], fresnel[1]) },
        },
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        forceSinglePass: true,
      }),
    );
    // After the opaque scene, ahead of everything else that blends.
    this.tintPass.renderOrder = -20;
    this.mirrorPass.renderOrder = -19;
    this.group = new THREE.Group();
    this.group.add(this.tintPass, this.mirrorPass);
    const mainMap = look.MainTexture?.[0] ? texture(look.MainTexture[0]) : null;
    this.lowPass = null;
    if (mainMap) {
      const black = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
      black.needsUpdate = true;
      const pair = (values, multiplier = 1) => new THREE.Vector2(
        (values?.[0] ?? 0) * multiplier, (values?.[1] ?? values?.[0] ?? 0) * multiplier);
      this.lowPass = new THREE.Mesh(geometry, ownedEffectMaterial('effects/water-textured', {
        uniforms: {
          mainMap: { value: mainMap }, tint: { value: new THREE.Vector4(...c0) },
          tile: { value: pair(tile, scale) }, velocity: { value: pair(look.Velocity, 1 / scale) },
          maskTile: { value: pair(look.SpecularMaskTile || [1, 1], scale) },
          maskVelocity: { value: pair(look.SpecularMaskScrollSpeed, 1 / scale) },
          maskA: { value: this.masks?.textures[0] || black },
          maskB: { value: this.masks?.textures[0] || black }, maskMix: { value: 0 },
          time: this.mirrorPass.material.uniforms.time,
          toSun: this.mirrorPass.material.uniforms.toSun,
          cameraRange: this.tintPass.material.uniforms.cameraRange,
        },
        transparent: true, depthWrite: true, side: THREE.DoubleSide, forceSinglePass: true,
      }));
      this.lowPass.frustumCulled = false;
      this.lowPass.renderOrder = -20;
      this.group.add(this.lowPass);
    }
    for (const mesh of [this.tintPass, this.mirrorPass]) mesh.frustumCulled = false;
    graphics.scene.add(this.group);
    // The Rust renderer's water pass finds it here (src/rust-render/water.js).
    graphics.scene.userData.water = this;

    // The far scene's water. The original draws the surface again in the far
    // scene, after the low-res terrain and in its fog (the sky's draw hands
    // the water's draw the far flag), out to the look's own FarSceneRange,
    // or else the sky's, when that reaches past the near
    // scene. Without it the far terrain's sea bed showed bare past the near
    // scene (both Kashyyyks). The same passes on a coarse grid, drawn only
    // from the eye's far camera (graphics.eyeLayer): mirrors leave it out.
    this.farReach = look.FarSceneRange?.[0] || 0;
    const farGeometry = this.surface(1, 1);
    this.farUniforms = {
      cameraRange: { value: new THREE.Vector2(2, 1500) },
      surfaceRange: { value: new THREE.Vector2(1e9, 0) },
      fogColor: lighting.fogColor,
      fogRange: lighting.fogRange,
    };
    this.farView = new THREE.PerspectiveCamera();
    this.farFrustum = new THREE.Frustum();
    this.farTint = this.farPass(this.tintPass, farGeometry);
    this.farMirror = this.farPass(this.mirrorPass, farGeometry);
    this.farLow = this.lowPass ? this.farPass(this.lowPass, farGeometry) : null;
    this.farGroup = new THREE.Group();
    this.farGroup.name = "far-water";
    this.farGroup.add(...[this.farTint, this.farMirror, this.farLow].filter(Boolean));
    this.farGroup.visible = false;
    graphics.farScene?.add(this.farGroup);
    this.bounds = new THREE.Box3().setFromBufferAttribute(geometry.getAttribute("position"));
    this.bounds.min.y -= 1;
    this.bounds.max.y += 1;
    // Each water cell's own box: one box around a whole canal is "in view"
    // from most of the city, and with it the scene's second (mirrored)
    // render ran nearly every frame.
    this.cellBounds = data.cells.map(([i, j]) => new THREE.Box3(
      new THREE.Vector3(i * data.cell - data.offset[0], data.height - 1, j * data.cell - data.offset[1]),
      new THREE.Vector3((i + 1) * data.cell - data.offset[0], data.height + 1, (j + 1) * data.cell - data.offset[1])));

    // Rings.
    this.rings = this.ringMesh(texture("FXrippl1"));
    graphics.scene.add(this.rings);

    // The tint under the surface.
    const u = rgba(look.UnderwaterColor, [255, 255, 255, 0]);
    this.underwater = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      ownedEffectMaterial('effects/water-underwater-tint', {
        uniforms: { color: { value: new THREE.Vector4(...u) } },
        transparent: true,
        depthTest: false,
        depthWrite: false,
      }),
    );
    this.underwater.frustumCulled = false;
    this.underwater.material.userData.bfeNoShadows = true;
    this.underwater.material.visible = true;
    this.underwaterScene = new THREE.Scene();
    this.underwaterScene.add(this.underwater);
    this.passCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

    // Enhanced submerged views are composited inside the scene, after other
    // transparency and before the final AA. Capture here even when the surface
    // is out of view (e.g. looking at the riverbed), so depth never goes stale.
    const surfaceUniforms = this.tintPass.material.uniforms;
    this.underwaterFog = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      ownedEffectMaterial('effects/water-underwater-fog', {
        uniforms: {
          sceneColor: surfaceUniforms.sceneColor, sceneDepth: surfaceUniforms.sceneDepth,
          cameraRange: surfaceUniforms.cameraRange, tint: surfaceUniforms.tint, time: surfaceUniforms.time,
        },
        transparent: true, blending: THREE.NoBlending, depthTest: false, depthWrite: false,
      }),
    );
    this.underwaterFog.frustumCulled = false;
    this.underwaterFog.renderOrder = 1e8;
    this.underwaterFog.visible = false;
    this.underwaterFog.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    graphics.scene.add(this.underwaterFog);

    this.frustum = new THREE.Frustum();
    this.matrix = new THREE.Matrix4();
    this.forward = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.at = new THREE.Vector3();
    this.plane = new THREE.Plane();
    this.clip = new THREE.Vector4();
    this.q = new THREE.Vector4();
    this.reflectionFar = null;
    this.surfaceBuffer = null;
    // Where the far reflection was last drawn from (held every other frame).
    this.reflectedEye = new THREE.Vector3();
    this.reflectedForward = new THREE.Vector3();
    this.reflectedSize = new THREE.Vector2();
    this.reflectedLevel = NaN;
    this.reflectFrames = 0;
    this.tintPass.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    this.oceanSurface = ocean ? new OceanSurface({ data, texture, graphics, weather, surfaceHost: this.host }) : null;
    this.oceanAlways = oceanLook;
    this.applyQuality();
    graphics.settings.onChange((key) => { if (key === "waterQuality" || key === "oceanEffects") this.applyQuality(); });
  }

  // A near pass's copy for the far scene: its shader as FAR_WATER, its
  // uniforms shared but for the far camera's range, the span it draws in and
  // the far scene's fog.
  farPass(pass, geometry) {
    const source = pass.material;
    const material = ownedEffectMaterial(source.userData.bfeProgram + '-far', {
      uniforms: { ...source.uniforms, ...this.farUniforms },
      defines: { FAR_WATER: "" },
      transparent: source.transparent, depthWrite: source.depthWrite, side: source.side, forceSinglePass: true,
      blending: source.blending, blendEquation: source.blendEquation, blendSrc: source.blendSrc, blendDst: source.blendDst,
    });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = "far-water";
    mesh.renderOrder = pass.renderOrder;
    mesh.frustumCulled = false;
    mesh.layers.set(this.graphics.eyeLayer ?? 4);
    mesh.onBeforeRender = (renderer, scene, camera) => this.placeFar(camera);
    return mesh;
  }

  // Where the far water draws, in view depth: from the near scene's end out
  // to the look's FarSceneRange (scaled with the view distance, as the far
  // scene's own reach is) or else the far scene's reach. None when the far
  // scene doesn't reach past the near scene's fade start, as the sky's draw
  // decides for the low-res terrain and the water together (Yavin 4:
  // Temple, both Rhen Vars, Jabba's Palace).
  farSpan(farCamera = this.graphics.farCamera) {
    const end = farCamera?.far || 0;
    const [fadeStart, fadeEnd] = lighting.fade.value.toArray();
    if (!(end > fadeStart)) return [fadeEnd, 0];
    const reach = this.graphics.farReach?.[1];
    const scale = reach > 0 ? end / reach : 1;
    return [fadeEnd, this.farReach > 0 ? this.farReach * scale : end];
  }

  placeFar(camera) {
    const [start, end] = this.farSpan(camera);
    this.farUniforms.cameraRange.value.set(camera.near, camera.far);
    this.farUniforms.surfaceRange.value.set(start, end);
  }

  // Whether any water cell lies in the eye's view between the far span's ends.
  farInView(camera) {
    const [start, end] = this.farSpan();
    if (!(end > start)) return false;
    const view = this.farView;
    view.fov = camera.fov;
    view.aspect = camera.aspect;
    view.near = Math.max(camera.near, 0.1);
    view.far = end;
    view.updateProjectionMatrix();
    this.farFrustum.setFromProjectionMatrix(this.matrix.multiplyMatrices(view.projectionMatrix, camera.matrixWorldInverse));
    if (!this.farFrustum.intersectsBox(this.bounds)) return false;
    return this.cellBounds.some((box) => this.farFrustum.intersectsBox(box));
  }

  // The ocean drawn in place of the flat surface: always where the look
  // asks for one, and on an Enhanced sea while ocean effects are enhanced.
  get ocean() {
    const surface = this.oceanSurface;
    if (!surface) return null;
    return this.oceanAlways || this.graphics.settings.get("oceanEffects") === "enhanced" ? surface : null;
  }

  applyQuality() {
    const name = this.graphics.settings.get("waterQuality");
    this.quality = WATER_QUALITIES[name] || WATER_QUALITIES.high;
    if (this.oceanSurface) {
      const active = Boolean(this.ocean);
      this.oceanSurface.near.visible = this.oceanSurface.far.visible = active;
      if (active) return; // textured ocean does not need planar reflection targets
    }
    const enhanced = this.quality.scale > 0;
    if (this.lowPass) this.lowPass.visible = !enhanced;
    this.tintPass.visible = enhanced || !this.lowPass;
    this.mirrorPass.visible = enhanced || !this.lowPass;
    if (!enhanced) this.underwaterFog.visible = false;
    this.mirrorPass.material.uniforms.quality.value = enhanced ? 1 : 0;
    this.mirrorPass.material.uniforms.screenSpace.value = enhanced && !this.quality.planarNear ? 1 : 0;
    this.tintPass.material.uniforms.quality.value = enhanced ? 1 : 0;
    this.tintPass.material.blending = enhanced ? THREE.NoBlending : THREE.CustomBlending;
    // The far scene's passes follow (their quality uniforms are these).
    if (this.farLow) this.farLow.visible = this.lowPass.visible;
    this.farTint.visible = this.tintPass.visible;
    this.farMirror.visible = this.mirrorPass.visible;
    this.farTint.material.blending = this.tintPass.material.blending;
    if (enhanced && !this.surfaceBuffer) {
      this.surfaceBuffer = new THREE.WebGLRenderTarget(1, 1, {
        depthTexture: new THREE.DepthTexture(1, 1, THREE.UnsignedIntType),
      });
      this.tintPass.material.uniforms.sceneColor.value = this.surfaceBuffer.texture;
      this.tintPass.material.uniforms.sceneDepth.value = this.surfaceBuffer.depthTexture;
    }
    if (!enhanced && this.surfaceBuffer) {
      this.surfaceBuffer.dispose();
      this.surfaceBuffer = null;
      this.tintPass.material.uniforms.sceneColor.value = null;
      this.tintPass.material.uniforms.sceneDepth.value = null;
    }
    this.mirrorPass.material.uniforms.distort.value = this.bumps ? (enhanced ? DISTORT * 0.18 : DISTORT) : 0;
    const planar = enhanced && Boolean(this.quality.planarNear);
    if (planar && !this.reflectionFar) this.reflectionFar = new THREE.WebGLRenderTarget(1, 1);
    if (!planar && this.reflectionFar) {
      this.reflectionFar.dispose();
      this.reflectionFar = null;
    }
    this.resizeReflection();
  }

  captureSurface(renderer, camera) {
    if (!this.surfaceBuffer) return;
    const source = renderer.getRenderTarget();
    if (source !== this.graphics.sceneTarget) return;
    const target = this.surfaceBuffer;
    target.setSize(source.width, source.height);
    // Colour and depth are resolved together before the first water reader.
    // The Rust bank preserves the source's multisample framebuffer binding.
    renderer.resources.copy(source,target,COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
    const u = this.tintPass.material.uniforms;
    u.resolution.value.set(source.width, source.height);
    u.cameraRange.value.set(camera.near, camera.far);
  }

  resizeReflection() {
    const { renderer } = this.graphics;
    // The 3D frame's resolution (smaller than the canvas under an upscaler).
    const size = this.graphics.renderSize?.clone() ?? renderer.getDrawingBufferSize(new THREE.Vector2());
    const q = this.quality;
    const scale = Math.min(q.scale, q.maxSize / Math.max(size.x, size.y));
    const width = q.scale ? Math.max(1, Math.round(size.x * scale)) : REFLECTION_SIZE[0];
    const height = q.scale ? Math.max(1, Math.round(size.y * scale)) : REFLECTION_SIZE[1];
    this.target.setSize(width, height);
    this.reflectionFar?.setSize(width, height);
  }

  surface(divisions, rows = divisions) {
    const { cell, offset, height } = this;
    const positions = [];
    const uvs = [];
    const indices = [];
    for (const [i, j, patched] of this.data.cells) {
      if (patched) continue;
      const x0 = i * cell - offset[0];
      const z0 = j * cell - offset[1];
      const base = positions.length / 3;
      for (let b = 0; b <= rows; b++) {
        for (let a = 0; a <= divisions; a++) {
          positions.push(x0 + (a * cell) / divisions, height, z0 + (b * cell) / rows);
          uvs.push(a / divisions, b / rows);
        }
      }
      const row = divisions + 1;
      for (let b = 0; b < rows; b++) {
        for (let a = 0; a < divisions; a++) {
          const k = base + b * row + a;
          indices.push(k, k + row, k + 1, k + 1, k + row, k + row + 1);
        }
      }
    }
    for (const patch of this.data.patches || []) {
      const base = positions.length / 3;
      for (const p of patch.positions) positions.push(p[0], p[1], p[2]);
      for (const uv of patch.uvs) uvs.push(uv[0], uv[1]);
      for (const k of patch.indices) indices.push(base + k);
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
    geometry.setIndex(indices);
    return geometry;
  }

  ringMesh(map) {
    const quads = RIPPLE_MAX * RIPPLE_SEGMENTS;
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(this.host.positions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("uv", new THREE.BufferAttribute(this.host.uvs, 2).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("alpha", new THREE.BufferAttribute(this.host.alpha, 1).setUsage(THREE.DynamicDrawUsage));
    const index = new Uint32Array(quads * 6);
    for (let q = 0; q < quads; q++) index.set([q * 4, q * 4 + 1, q * 4 + 2, q * 4 + 2, q * 4 + 1, q * 4 + 3], q * 6);
    geometry.setIndex(new THREE.BufferAttribute(index, 1));
    // Retain update-range objects too: BufferAttribute.addUpdateRange creates
    // a fresh object on every frame, although these prefixes have one range.
    for (const attribute of Object.values(geometry.attributes)) attribute.surfaceRange = { start: 0, count: 0 };
    geometry.setDrawRange(0, 0);
    const mesh = new THREE.Mesh(
      geometry,
      ownedEffectMaterial('effects/water-rings', {
        uniforms: { map: { value: map } },
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        forceSinglePass: true,
      }),
    );
    mesh.renderOrder = -18;
    mesh.frustumCulled = false;
    return mesh;
  }

  // The gameplay step reports each soldier in the water every tick; note the
  // wet ones for the surface's contact foam and ripples (High/Ultra).
  soldierStep(dt, contact, feet, head, gun, velocity, splash) {
    const damage = super.soldierStep(dt, contact, feet, head, gun, velocity, splash);
    if (this.level > feet.y && this.over(feet.x, feet.z)) this.host.wader(contact, feet, velocity, this.level - feet.y);
    return damage;
  }

  // Water.ripple's birth is a scalar command; the phase owns its lifetime,
  // oldest-ring replacement, wake shape and all ring vertex construction.
  ripple(position, life, radius, velocity = null) {
    if (this.over(position.x, position.z)) this.host.ripple(position, life, radius, velocity);
  }

  prepareFrame(dt, camera) {
    this.host.prepare(dt, camera);
    const f = this.host.frame, sun = lighting.sunDirection.value;
    f[FRAME.LEVEL] = this.level;
    f[FRAME.SUN] = sun.x; f[FRAME.SUN + 1] = sun.y; f[FRAME.SUN + 2] = sun.z;
    f[FRAME.OCEAN] = this.ocean ? 1 : 0;
    f[FRAME.STORM] = this.ocean?.enhanced && this.ocean.waves.storm?.enabled ? 1 : 0;
  }

  // Only resource publication remains from Water.update and updateRings.
  // Texture indices, mixing, sun/yaw, contacts and vertices are native outputs.
  publishFrame(camera) {
    const r = this.host.result;
    this.time = r[RESULT.TIME]; this.wave = r[RESULT.WAVE];
    this.group.position.y = this.farGroup.position.y = this.wave;
    this.publishFrames(this.normals, this.mirrorPass.material.uniforms, 'normalA', 'normalB', 'normalMix', RESULT.NORMAL_A);
    this.publishFrames(this.bumps, this.mirrorPass.material.uniforms, 'bumpA', 'bumpB', 'bumpMix', RESULT.BUMP_A);
    if (this.lowPass) this.publishFrames(this.masks, this.lowPass.material.uniforms, 'maskA', 'maskB', 'maskMix', RESULT.MASK_A);
    const u = this.mirrorPass.material.uniforms;
    u.time.value = this.time; this.tintPass.material.uniforms.time.value = this.time;
    u.toSun.value.fromArray(r, RESULT.SUN); u.viewYaw.value = r[RESULT.YAW];
    const waders = this.tintPass.material.uniforms.waders.value;
    for (let i = 0; i < WADERS; i++) waders[i].fromArray(r, RESULT.WADERS + i * 4);
    const geometry = this.rings.geometry, quads = r[RESULT.QUADS];
    if (quads) {
      this.publishRingAttribute(geometry.attributes.position, quads);
      this.publishRingAttribute(geometry.attributes.uv, quads);
      this.publishRingAttribute(geometry.attributes.alpha, quads);
    }
    geometry.setDrawRange(0, quads * 6);
    if (this.ocean) this.ocean.publishFrame(r, camera);
  }
  publishFrames(frames, uniforms, a, b, mix, at) {
    if (!frames) return;
    const r = this.host.result;
    uniforms[a].value = frames.textures[r[at]]; uniforms[b].value = frames.textures[r[at + 1]];
    uniforms[mix].value = r[at + 2];
  }
  publishRingAttribute(attribute, quads) {
    attribute.surfaceRange.count = quads * 4 * attribute.itemSize;
    attribute.updateRanges[0] = attribute.surfaceRange; attribute.updateRanges.length = 1; attribute.needsUpdate = true;
  }
  update(dt, camera) {
    camera.updateMatrixWorld();
    this.prepareFrame(dt, camera); this.host.update(); this.publishFrame(camera);
  }

  // Render the mirror image into the reflection target: the far scene from
  // the camera mirrored in the surface, cut at the surface. Skipped when no
  // water is in view or the camera is under it.
  reflect(camera) {
    this.farGroup.visible = false;
    if (this.ocean) { this.group.visible = false; this.underwaterFog.visible = false; return; }
    camera.updateMatrixWorld();
    this.matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.matrix);
    const level = this.level;
    this.tintPass.material.uniforms.cameraRange.value.set(camera.near, camera.far);
    let visible = false;
    if (this.frustum.intersectsBox(this.bounds)) for (const box of this.cellBounds) if (this.frustum.intersectsBox(box)) { visible = true; break; }
    const below = camera.position.y < level;
    this.underwaterFog.visible = Boolean(this.quality.scale && below && this.over(camera.position.x, camera.position.z));
    this.tintPass.material.uniforms.underwater.value = below ? 1 : 0;
    this.group.visible = visible;
    // The far scene's water, seen from above (it takes the same mirror).
    const farVisible = !below && this.farInView(camera);
    this.farGroup.visible = farVisible;
    // A mirror camera on/below the plane has no valid above-water reflection.
    // In particular, do not draw yesterday's texture/projective matrix here.
    this.mirrorPass.visible = this.farMirror.visible = false;
    if (!(visible || farVisible) || this.lowPass?.visible || camera.position.y - level < 0.03) return;
    const graphics = this.graphics;
    const renderer = graphics.renderer;
    const far = graphics.farCamera;
    const mirror = this.mirror;
    // The far scene's mirror image (High, or the textureless fallback) is held every other frame
    // while the eye stays within 0.3 m and 2 degrees of where it was drawn
    // from: the sky and the far buildings are 100 m and more away, so a
    // frame's parallax is a fraction of a texel, and at rest the image is
    // the same one. Ultra mirrors the near scene (soldiers) every frame.
    this.resizeReflection();
    camera.getWorldDirection(this.forward);
    const planarNear = Boolean(this.reflectionFar && this.quality.planarNear);
    const held = !planarNear && this.reflectFrames++ % 2 === 1 && this.reflectedLevel === level
      && this.reflectedSize.x === this.target.width && this.reflectedSize.y === this.target.height
      && this.reflectedEye.distanceToSquared(camera.position) < 0.09 && this.reflectedForward.dot(this.forward) > 0.9994;
    if (held) { this.mirrorPass.visible = this.farMirror.visible = true; return; }
    mirror.fov = camera.fov;
    mirror.aspect = camera.aspect;
    mirror.near = this.reflectionFar ? camera.near : far.near || 2;
    mirror.far = far.far || camera.far;
    mirror.updateProjectionMatrix();
    mirror.position.copy(camera.position);
    mirror.position.y = 2 * level - camera.position.y;
    camera.getWorldDirection(this.forward);
    this.forward.y = -this.forward.y;
    this.up.set(0, 1, 0).applyQuaternion(camera.quaternion);
    this.up.y = -this.up.y;
    mirror.up.copy(this.up);
    mirror.lookAt(this.at.copy(mirror.position).add(this.forward));
    mirror.updateMatrixWorld();
    // The sky is drawn through this one (below).
    this.plainProjection.copy(mirror.projectionMatrix);

    // Clip at the surface by tilting the near plane onto it.
    this.plane.set(new THREE.Vector3(0, 1, 0), -level).applyMatrix4(mirror.matrixWorldInverse);
    this.clip.set(this.plane.normal.x, this.plane.normal.y, this.plane.normal.z, this.plane.constant);
    const p = mirror.projectionMatrix;
    const e = p.elements;
    this.q.set((Math.sign(this.clip.x) + e[8]) / e[0], (Math.sign(this.clip.y) + e[9]) / e[5], -1, (1 + e[10]) / e[14]);
    const clipDenominator = this.clip.dot(this.q);
    if (Math.abs(clipDenominator) < 1e-6 || !Number.isFinite(clipDenominator)) return;
    this.clip.multiplyScalar(2 / clipDenominator);
    e[2] = this.clip.x;
    e[6] = this.clip.y;
    e[10] = this.clip.z + 1;
    e[14] = this.clip.w;

    // World point -> the target's 0..1 square.
    this.reflectionMatrix.set(0.5, 0, 0, 0.5, 0, 0.5, 0, 0.5, 0, 0, 0.5, 0.5, 0, 0, 0, 1);
    this.reflectionMatrix.multiply(p).multiply(mirror.matrixWorldInverse);

    this.reflectedEye.copy(camera.position);
    camera.getWorldDirection(this.reflectedForward);
    this.reflectedSize.set(this.target.width, this.target.height);
    this.reflectedLevel = level;
    const previous = renderer.getRenderTarget();
    const sky = graphics.sky;
    const skyAt = sky ? sky.position.clone() : null;
    // The sky's other models (graphics.skyLayers) are mirrored with it.
    const layersAt = (graphics.skyLayers || []).map(({ mesh }) => mesh.position.clone());
    const clearColor = renderer.getClearColor(new THREE.Color());
    const clearAlpha = renderer.getClearAlpha();
    const autoClear = renderer.autoClear;
    const autoShadows = renderer.shadowMap.autoUpdate;
    const backgroundMap = graphics.background.material.uniforms.map.value;
    const farMap = lighting.farMap.value;
    const resolution = lighting.resolution.value.clone();
    const softParticles = particleDepth.enabled.value;
    const ringsVisible = this.rings.visible;
    try {
      if (sky) sky.position.copy(mirror.position);
      for (const { mesh } of graphics.skyLayers || []) if (mesh !== sky) mesh.position.copy(mirror.position);
      renderer.autoClear = true;
      renderer.shadowMap.autoUpdate = false;
      const near = Boolean(this.reflectionFar && this.quality.planarNear);
      renderer.setRenderTarget(near ? this.reflectionFar : this.target);
      renderer.setClearColor(0x000000, 0);
      // The sky first, through the plain mirror: its models stand around the
      // mirrored eye, under the surface, where the oblique near plane cut
      // every one of them away and left the mirror black above the horizon.
      // Then the rest of the far scene over it (the sky writes no depth).
      const skyMeshes = (graphics.skyLayers || []).map(({ mesh }) => mesh).filter((mesh) => mesh.visible);
      const oblique = this.obliqueProjection.copy(mirror.projectionMatrix);
      // Rust's farMirror phase draws both reflected far-scene views.
      const drawFar = () => { graphics.rust.cover("farMirror", renderer, graphics.farScene, mirror); };
      try {
        if (skyMeshes.length) {
          mirror.projectionMatrix.copy(this.plainProjection);
          mirror.layers.set(MIRROR_SKY_LAYER);
          for (const mesh of skyMeshes) mesh.layers.enable(MIRROR_SKY_LAYER);
          drawFar();
          for (const mesh of skyMeshes) mesh.visible = false;
          mirror.layers.set(0);
          mirror.projectionMatrix.copy(oblique);
          renderer.autoClear = false;
        }
        drawFar();
      } finally {
        for (const mesh of skyMeshes) { mesh.layers.disable(MIRROR_SKY_LAYER); mesh.visible = true; }
        mirror.layers.set(0);
        mirror.projectionMatrix.copy(oblique);
        renderer.autoClear = true;
      }
      if (near) {
        // Full detail architecture and characters share the same mirrored
        // camera. Hide the water to avoid sampling its own render target.
        this.group.visible = false;
        this.rings.visible = false;
        particleDepth.enabled.value = 0;
        lighting.resolution.value.set(this.target.width, this.target.height);
        lighting.farMap.value = this.reflectionFar.texture;
        graphics.background.material.uniforms.map.value = this.reflectionFar.texture;
        // The near scene's matrices, once a frame (Graphics.updateScene):
        // only this mirror draws it before render() does.
        graphics.updateScene?.();
        renderer.setRenderTarget(this.target);
        // Rust draws the complete reflected near scene.
        graphics.rust.coverMirror(renderer, graphics.scene, mirror);
      }
    } finally {
      this.group.visible = visible;
      this.mirrorPass.visible = this.farMirror.visible = true;
      this.rings.visible = ringsVisible;
      lighting.farMap.value = farMap;
      lighting.resolution.value.copy(resolution);
      particleDepth.enabled.value = softParticles;
      graphics.background.material.uniforms.map.value = backgroundMap;
      renderer.shadowMap.autoUpdate = autoShadows;
      renderer.autoClear = autoClear;
      renderer.setClearColor(clearColor, clearAlpha);
      renderer.setRenderTarget(previous);
      if (sky) sky.position.copy(skyAt);
      (graphics.skyLayers || []).forEach(({ mesh }, i) => mesh.position.copy(layersAt[i]));
    }
  }

  // The screen's tint with the eye under the surface over water (the Rust
  // renderer's underwater phase where it draws it: src/rust-render).
  drawUnderwater(renderer, camera) {
    if (this.quality.scale) return;
    if (camera.position.y >= this.level || !this.over(camera.position.x, camera.position.z)) return;
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    renderer.setRenderTarget(null);
    this.graphics.rust.cover("underwater", renderer, this.underwaterScene, this.passCamera);
    renderer.autoClear = autoClear;
  }
}
