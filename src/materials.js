import { ownedEffectMaterial } from "./rust-render/effect-material.js";
import * as THREE from "three";
import { localLighting } from "./local-lights.js";
import { ZERO_LIGHTS, lightMask, packed, pointLighting, sphereLightMask, useLights } from "./point-lights.js";

// Materials that light like the 2004 renderer: Direct3D 9 shaders working in
// gamma space, lighting per vertex. The light environments:
//
//   lit      props: the flat AmbientColor plus the sun, then the tint:
//            clamp((ambient + max(0, N.-sun) * sunColor + lights) * tint)
//   dynamic  soldiers and "Lighting dynamic" classes: the same with a
//            sky-to-ground ambient, lerp(top, bottom, (1 - N.y) / 2), and the
//            sun scaled by the light region the object stands in
//   baked    STATIC_LIT geometry: the vertex colour is the light (plus
//            lights, when the material is lit)
//   unlit    MTRL bit 0 clear: texture * tint
//   terrain  two layers by weight, the detail texture (0.5 is neutral), times
//            2 x the baked vertex colour
//   sky      the sky dome: the texture as it is
//
// Bump-mapped materials light the sun per pixel from a tangent-space normal
// map, sat(ambient + sat(n.L) * sunColor) * tint, using the model's own
// tangent and binormal (green = +v). Specular materials add a highlight of
// the sun scaled by a gloss alpha.
//
// The near scene dissolves into the far scene's image between the fade start
// and end. The far scene (FAR) is fogged, when the sky's fog is on (setFog),
// and leaves out whatever lies nearer than the fade start, where the near
// scene covers it.
//
// The sun's term is what the port's shadow maps darken. Baked terrain keeps
// its own shadows, and a shadow there only brings it down to the baked shadow
// strength (half), never below what is already baked.

// Shared by every material: the map's lights, the far scene, and the frame.
export const lighting = {
  sunDirection: { value: new THREE.Vector3(0, -1, 0) },
  sunColor: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambient: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  characterAmbient: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambientTop: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambientBottom: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  time: { value: 0 },
  fade: { value: new THREE.Vector2(300, 500) },
  farMap: { value: null },
  resolution: { value: new THREE.Vector2(1, 1) },
  fogColor: { value: new THREE.Vector3() },
  fogRange: { value: new THREE.Vector2(1e9, 2e9) },
  cascades: { value: [new THREE.Vector2(), new THREE.Vector2(), new THREE.Vector2(), new THREE.Vector2()] },
  cascadeNear: { value: 0.5 },
  cascadeFar: { value: 200 },
  // The cascades' depth maps (graphics.js), one a cascade, in the GPU's
  // own comparison mode.
  cascadeMaps: { value: [] },
  // Each cascade's world-to-map matrix as it applies to plane normals (the
  // inverse transpose of its rotation and scale), for the receiver plane.
  cascadePlanes: { value: [] },
  bump: { value: 1 },
  aaSamples: { value: 0 },
  farAaSamples: { value: 0 },
  shadowStrength: { value: 1 },
  // Enhanced terrain shading (Graphics > Terrain); see TERRAIN_RELIGHT.
  terrainShade: { value: 1 },
  // The point lights that reach what the draw covers (point-lights.js).
  zeroLights: pointLighting.drawLights,
  zeroLightCount: pointLighting.drawLightCount,
};

// Map admission may finish before the renderer module. Keep authored facts
// until its light owner arrives; no browser arithmetic prepares uniforms.
let lightOwner=null,lightMap=null,fogMap=null;
export function bindLighting(owner){lightOwner=owner;if(lightMap)owner.mapLighting(lightMap);if(fogMap)owner.mapFog(fogMap);}
export function setLighting(map){lightMap=map;if(lightOwner)lightOwner.mapLighting(map);}

// The fog range that fogs nothing: it starts past any far scene.
export const NO_FOG = Object.freeze([1e9, 2e9]);

// Whether a fog range [start, end] fogs anything, as the 2004 game decides:
// it switches Direct3D's fog on only when 0 <= start < end, and its vertex
// shaders' fog factor is 1 (clear) when end <= start. Kashyyyk: Islands' sky
// sets 9000 and 8000, so its far scene draws unfogged.
export function fogs(range) {
  return Boolean(range) && range[0] >= 0 && range[0] < range[1];
}

// The far scene's fog (map.json distance.lowRes, from the sky's
// LowResTerrain): linear by view depth from FogNear (clear) to FogFar (the
// colour), FogFar already stretched by 255 / alpha at import. None without a
// LowResTerrain, or when its range fogs nothing. The colour is kept either
// way, as the original game keeps its render state.
export function setFog(map){fogMap=map;if(lightOwner)lightOwner.mapFog(map);}


// The sky material for a "sky" record (describe(): kind "sky"): the sky dome
// and the models over it, drawn by the effects family's owned program.
export function skyMaterial(options = {}) {
  return materialFor("sky", options, null);
}

// Static map resources have values and fixed-function state, with source
// supplied by the Rust world catalog rather than a browser shader material.
export function worldMaterial(kind, options = {}) {
  const material = materialFor(kind, options, values => Object.assign(new THREE.Material(), values, { lights: true, uniforms: { ...THREE.UniformsUtils.clone(THREE.UniformsLib.lights), ...values.uniforms } }));
  material.userData.bfeFamily = "world";
  return material;
}

// Actor shells provide values for the closed Rust descriptor selector.
export function actorMaterial(kind, options = {}) {
  return materialFor(kind, options, values => {
    const material = Object.assign(new THREE.Material(), values);
    material.fog = false;
    material.visible = false;
    material.userData.actorShader = "zero";
    return material;
  });
}

function materialFor(kind, options, create) {
  const defines = { ZERO_LIGHTS };
  if (kind === "dynamic") defines.DYNAMIC = "";
  if (kind === "baked") defines.BAKED = "";
  if (kind === "unlit") defines.UNLIT = "";
  if (kind === "terrain") defines.TERRAIN = "";
  if (kind === "sky") defines.SKY = "";
  if (options.colored || kind === "terrain") defines.HAS_COLOR = "";
  if (options.tint) defines.TINT = "";
  if (options.lights === false) defines.NO_POINT = "";
  if (options.bumpMap && kind !== "terrain") defines.BUMP = "";
  if (options.scroll) defines.SCROLL = "";
  if (options.specular && options.specular.slice(1).some((c) => c > 0)) defines.SPECULAR = "";
  if (options.alphaTest) defines.ALPHA_TEST = "";
  if (options.blending) defines.BLENDED = "";
  if (options.blending === "additive") defines.ADDITIVE = "";
  if (options.far) defines.FAR = "";
  if (kind === "sky") {
    const blended = Boolean(options.blending);
    const uniforms = { ...lighting, map: { value: options.map || null }, sunScale: { value: 1 } };
    const material = ownedEffectMaterial("effects/sky", {
      name: "zero:sky", defines, uniforms,
      transparent: blended, depthWrite: false, depthTest: false,
      blending: options.blending === "additive" ? THREE.AdditiveBlending : THREE.NormalBlending,
      side: options.doubleSided ? THREE.DoubleSide : THREE.FrontSide,
      alphaToCoverage: Boolean(options.alphaTest) && !blended,
    });
    material.map = options.map || null;
    material.alphaTest = options.alphaTest ? 0.5 : 0;
    material.shadowSide = material.side;
    material.userData.pointLit = false;
    return material;
  }
  const [exponent = 0, r = 0, g = 0, b = 0] = options.specular || [];
  const uniforms = {
    map: { value: options.map || null },
    bumpMap: { value: options.bumpMap || null },
    scroll: { value: new THREE.Vector2(...(options.scroll || [0, 0])) },
    specular: { value: new THREE.Vector4(THREE.MathUtils.clamp(exponent, 10, 100), r, g, b) },
    sunScale: { value: 1 },
    ambientPull: { value: new THREE.Vector2() },
    brightness: { value: 1 },
    localLightPositions: localLighting.positions,
    localLightColors: localLighting.colors,
    localLightCount: localLighting.count,
  };
  if (kind === "terrain") Object.assign(uniforms, {
    layer1Map: { value: options.layer1Map || options.map || null },
    detailMap: { value: options.detailMap || neutralTexture() },
    layerScales: { value: new THREE.Vector2(...(options.layerScales || [8, 8])) },
    // Where the far scene's low-res texture starts (the terrain's corner).
    farOrigin: { value: new THREE.Vector2(...(options.farOrigin || [0, 0])) },
    layerScale0: { value: new THREE.Vector4(8, 8, 8, 8) },
    layerScale1: { value: new THREE.Vector4(8, 8, 8, 8) },
    bakedLit: { value: new THREE.Vector3(...(options.bakedLit || [145 / 255, 120 / 255, 90 / 255])) },
  });
  // The shared uniforms are shared objects: one change reaches every material.
  Object.assign(uniforms, lighting);
  const blended = Boolean(options.blending);
  const material = create({
    name: `zero:${kind}`,
    defines,
    uniforms,
    forceSinglePass: true,
    transparent: blended,
    depthWrite: !blended && kind !== "sky",
    depthTest: kind !== "sky",
    blending: options.blending === "additive" ? THREE.AdditiveBlending : THREE.NormalBlending,
    side: options.doubleSided ? THREE.DoubleSide : THREE.FrontSide,
    alphaToCoverage: Boolean(options.alphaTest) && !blended,
  });
  // WebGLShadowMap reads these material properties, not shader uniforms or
  // custom defines. Without them fences/leaves cast solid polygon shadows.
  material.map = options.map || null;
  material.alphaTest = options.alphaTest ? 0.5 : 0;
  material.shadowSide = options.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
  // Whether its shaders add point lights at all (unlit, terrain, sky and the
  // far scene's don't).
  material.userData.pointLit = !options.far && options.lights !== false && (kind === "lit" || kind === "dynamic" || kind === "baked");
  return material;
}

// The point lights a zero material's draw reads (every draw of one sets
// them, the derived ones too: destruction-debris.js): the scene's that
// reach the object, by its bounds in the world. A soldier's body has the
// pose sphere actor.js gives it, which any pose stays inside; other meshes
// their geometry's box. Instanced meshes, and skinned ones without such a
// sphere, draw outside their geometry's bounds and keep every light.
const maskBox = new THREE.Box3(), maskCenter = new THREE.Vector3(), maskBoxes = [maskBox];
const IDENTITY = new THREE.Matrix4();
export function selectLights(material, object, geometry, scene, camera) {
  const pack = packed(scene, camera);
  let mask = -1;
  if (object.isSkinnedMesh) {
    const sphere = object.boundingSphere;
    if (sphere && object.userData.poseSphere) {
      maskCenter.copy(sphere.center).applyMatrix4(object.matrixWorld);
      mask = sphereLightMask(pack, maskCenter, sphere.radius * object.matrixWorld.getMaxScaleOnAxis());
    }
  } else if (!object.isInstancedMesh) {
    if (!geometry.boundingBox) geometry.computeBoundingBox();
    maskBox.copy(geometry.boundingBox);
    if (!object.matrixWorld.equals(IDENTITY)) maskBox.applyMatrix4(object.matrixWorld);
    mask = lightMask(pack, maskBoxes);
  }
  setLightMask(material, pack, mask);
}

// The draw's lights. Who sends them decides (a zero material's own hook:
// drawsAgain), so the uniforms do not go up twice.
export function setLightMask(material, pack, mask) {
  useLights(pack, mask);
}

// A draw's own values (its lights, a soldier's local lights) reach the GPU
// with three's upload when the draw before was another material's (setProgram
// refreshes a material on a change of material or program). After a draw of
// the same material three sends nothing, so they are sent again
// (uniformsNeedUpdate): the same material can draw again with other lights.
// Asking for it on every draw would send every uniform twice whenever the
// material changed. Every draw of a zero material passes here, so the last
// one seen is the one three drew last, if three's was a zero material at all.
let lastDrawn = null;
export function drawsAgain(material) {
  const again = lastDrawn === material;
  lastDrawn = material;
  return again;
}

let neutral = null;
function neutralTexture() {
  if (!neutral) {
    neutral = new THREE.DataTexture(new Uint8Array([128, 128, 128, 255]), 1, 1);
    neutral.needsUpdate = true;
  }
  return neutral;
}

// How MTRL flags blend: 0x80 adds, 0x4 lays the texture over by its alpha.
// A hard-edged (0x2) material that doesn't add is opaque: the Normal, Bump,
// Scroll and Detail shaders clear its 0x4 when they load it, so it is cut
// out at half alpha, writes depth and draws with the opaque geometry. Every
// stock hard-edged material uses one of those render types (only Specmap
// would keep the blend).
export function blendingOf(flags) {
  if (flags & 0x80) return "additive";
  return flags & 0x4 && !(flags & 0x2) ? "normal" : null;
}

// The material kind and options for a mesh record from map.json.
export function describe(record) {
  const flags = record.flags || 0;
  let kind = "lit";
  if (record.sky) kind = "sky";
  else if (record.terrain) kind = "terrain";
  else if (record.colorKind === "lit" || flags & 0x40) kind = "baked";
  else if (!(flags & 0x1)) kind = "unlit";
  else if (record.dynamic) kind = "dynamic";
  return {
    kind,
    colored: Boolean(record.colorKind),
    tint: record.colorKind === "tint",
    lights: Boolean(flags & 0x1),
    scroll: record.scroll,
    specular: flags & 0x20 ? record.specular : null,
    alphaTest: Boolean(flags & 0x2),
    blending: blendingOf(flags),
    doubleSided: Boolean(flags & 0x86),
    far: Boolean(record.far),
  };
}
