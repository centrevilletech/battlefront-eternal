import * as THREE from "three";
import {sceneNodes} from "./rust-render/scene-changes.js";
import { INPUT as I, LIGHT as P, PACK as K, LIMIT as L } from "./rust-render/camera-layout.js";

// The point lights the map's materials (materials.js) add, packed for the
// scene being drawn: only the lights that are on and can reach what the
// camera draws, in world space, two vec4 a light (position and range, then
// colour times intensity). The materials loop over the packed lights and
// stop at the count.
//
// three's own light uniforms hold every point light in the scene, idle or
// not (ordnance.js keeps a fixed pool so the programs never recompile), and
// its loop is unrolled over all of them. On older GPUs that is the frame:
// on Intel's Sandy Bridge, the lit vertex shader read 56 vec4 of light
// structs before anything else, so the matrices every vertex needs no
// longer fitted in the constant registers and were fetched from memory,
// four reads a matrix; the shader made 61 memory reads a vertex. A lit
// vertex now reads two values for each light that is on, and the rest stay
// in registers. What a light adds is unchanged: a light that is off, or out
// of its range of everything drawn, added exactly nothing.
export const ZERO_LIGHTS = 16;

// What the materials read: the lights of the draw in hand (useLights), two
// vec4 a light (position and range, then colour times intensity), packed
// flat. three uploads a flat array as it is, and only its length: the value
// is a view of the lights in use (at least one light's slots), so a draw
// sends what it reads instead of all of them, unpacked from vectors.
const drawLightData = new Float32Array(ZERO_LIGHTS * 8);
const drawLightViews = Array.from({ length: ZERO_LIGHTS + 1 }, (_, n) => drawLightData.subarray(0, Math.max(n, 1) * 8));
export const pointLighting = {
  drawLights: { value: drawLightViews[0] },
  drawLightCount: { value: 0 },
};

// Each scene's lights packed for each camera that draws it: kept apart, so a
// render made in the middle of another (a fountain's reflection, a volume's
// own pass) leaves the outer one's lights as they were.
const packs = new WeakMap();
const EMPTY = { lights: [], count: 0 };
function packOf(scene, camera) {
  let cameras = packs.get(scene);
  if (!cameras) packs.set(scene, (cameras = new WeakMap()));
  let pack = cameras.get(camera);
  if (!pack) cameras.set(camera, (pack = { lights: Array.from({ length: ZERO_LIGHTS * 2 }, () => new THREE.Vector4()), count: 0, native: -1, owner: null }));
  return pack;
}

// The renderer asks for its light packet after scene admission and placement.
// This explicit publication replaces Scene.prototype.onBeforeRender chaining.
export function packed(scene, camera) {
  const owner=sceneNodes.get(scene)?.owner.core.cameras;
  return owner?packPointLights(scene,camera,owner):EMPTY;
}

// Camera identity and authored layer masks cross once. Native retained scene
// facts supply worlds, colour/range, ordered discovery and view-light metadata.
export function packPointLights(scene,camera,owner=sceneNodes.get(scene)?.owner.core.cameras){
 const pack=packOf(scene,camera),core=owner.core,registry=core.registry;
 owner.acquirePack(pack);
 const root=registry.admitIfNew(scene),source=registry.admitIfNew(core.graphics.scene);
 registry.worlds.flush();core.views();core.cameraInput[I.LIGHT_LAYERS]=camera.layers.mask;
 const role=owner.roles.get(camera)??owner.bindings.get(camera);
 core.rust.camera_scene_lights_frame(root.slot,root.generation,role,source?.slot??0xffffffff,source?.generation??0,scene===core.graphics.viewScene,performance.now(),pack.native);
 core.views();const out=core.cameraPacks,base=(pack.native%L.PACKS)*K.LEN;pack.count=out[base+K.COUNT]||0;
 for(let i=0;i<pack.count*2;i++)pack.lights[i].fromArray(out,base+K.VALUES+i*4);return pack;
}

// Which packed lights can reach what a draw covers (bit i for light i): a
// light whose sphere misses every box adds nothing to any vertex in them.
// boxes: Box3s in world space; visible: which of them to test (all if null).
export function lightMask(pack, boxes, visible = null) {
  const count = pack.count, slots = pack.lights;
  let mask = 0;
  for (let i = 0; i < count; i++) {
    const sphere = slots[2 * i], range = sphere.w;
    if (range <= 0) { mask |= 1 << i; continue; }
    const reach = range * range;
    for (let b = 0; b < boxes.length; b++) {
      if (visible && !visible[b]) continue;
      const { min, max } = boxes[b];
      const dx = Math.max(min.x - sphere.x, 0, sphere.x - max.x);
      const dy = Math.max(min.y - sphere.y, 0, sphere.y - max.y);
      const dz = Math.max(min.z - sphere.z, 0, sphere.z - max.z);
      if (dx * dx + dy * dy + dz * dz <= reach) { mask |= 1 << i; break; }
    }
  }
  return mask;
}

// The same for a sphere in world space (a soldier's pose sphere).
export function sphereLightMask(pack, center, radius) {
  const count = pack.count, slots = pack.lights;
  let mask = 0;
  for (let i = 0; i < count; i++) {
    const sphere = slots[2 * i], range = sphere.w;
    const dx = sphere.x - center.x, dy = sphere.y - center.y, dz = sphere.z - center.z;
    if (range <= 0 || Math.sqrt(dx * dx + dy * dy + dz * dz) <= range + radius) mask |= 1 << i;
  }
  return mask;
}

// The draw's lights: the pack's lights in `mask`, in their order. A light's
// sum is unchanged: those left out add nothing to what the draw covers.
// Each one still in the loop is read and tested at every vertex (or pixel),
// which on an old GPU costs even with nothing lit.
export function useLights(pack, mask) {
  const count = pack.count, from = pack.lights, to = drawLightData;
  let n = 0;
  for (let i = 0; i < count; i++) {
    if (!(mask & (1 << i))) continue;
    const at = from[2 * i], color = from[2 * i + 1], o = 8 * n;
    to[o] = at.x; to[o + 1] = at.y; to[o + 2] = at.z; to[o + 3] = at.w;
    to[o + 4] = color.x; to[o + 5] = color.y; to[o + 6] = color.z; to[o + 7] = color.w;
    n++;
  }
  pointLighting.drawLights.value = drawLightViews[n];
  pointLighting.drawLightCount.value = n;
}
