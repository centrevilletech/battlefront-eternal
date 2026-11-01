import * as THREE from "three";

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
// vec4 a light (position and range, then colour times intensity).
export const pointLighting = {
  drawLights: { value: Array.from({ length: ZERO_LIGHTS * 2 }, () => new THREE.Vector4()) },
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
  if (!pack) cameras.set(camera, (pack = { lights: Array.from({ length: ZERO_LIGHTS * 2 }, () => new THREE.Vector4()), count: 0 }));
  return pack;
}

// The lights packed for this scene and camera (the scene's render packs
// them as it starts: installPointLights).
export function packed(scene, camera) {
  return packs.get(scene)?.get(camera) ?? EMPTY;
}

// Each scene's point lights in scene order, as three would find them;
// looked for again every half second (lights come with the map's command
// posts and the ordnance pool, and hardly ever change).
const found = new WeakMap();
const REFRESH_MS = 500;

function lightsOf(scene) {
  const now = performance.now();
  let entry = found.get(scene);
  if (!entry || now - entry.at > REFRESH_MS) {
    const lights = entry?.lights || [];
    lights.length = 0;
    scene.traverse((object) => { if (object.isPointLight) lights.push(object); });
    entry = { lights, at: now };
    found.set(scene, entry);
  }
  return entry.lights;
}

// Whether three would light with it: it and every parent shown.
function shown(object, scene) {
  for (let o = object; o && o !== scene; o = o.parent) if (!o.visible) return false;
  return true;
}

const at = new THREE.Vector3();
const eye = new THREE.Vector3();

// The scene's lights that are on and can reach what the camera draws.
export function packPointLights(scene, camera) {
  const pack = packOf(scene, camera), slots = pack.lights;
  let count = 0;
  const lights = lightsOf(scene);
  if (lights.length) eye.setFromMatrixPosition(camera.matrixWorld);
  for (let i = 0; i < lights.length && count < ZERO_LIGHTS; i++) {
    const light = lights[i];
    if (!shown(light, scene) || !light.layers.test(camera.layers)) continue;
    const intensity = light.intensity, color = light.color;
    const r = color.r * intensity, g = color.g * intensity, b = color.b * intensity;
    if (!(r > 0 || g > 0 || b > 0)) continue;
    at.setFromMatrixPosition(light.matrixWorld);
    const range = light.distance;
    // Nothing drawn lies past the far plane, so nothing there is in range.
    if (range > 0 && at.distanceTo(eye) > camera.far + range) continue;
    slots[2 * count].set(at.x, at.y, at.z, range);
    slots[2 * count + 1].set(r, g, b, 0);
    count++;
  }
  pack.count = count;
  return pack;
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
  const count = pack.count, from = pack.lights, to = pointLighting.drawLights.value;
  let n = 0;
  for (let i = 0; i < count; i++) {
    if (!(mask & (1 << i))) continue;
    to[2 * n].copy(from[2 * i]);
    to[2 * n + 1].copy(from[2 * i + 1]);
    n++;
  }
  pointLighting.drawLightCount.value = n;
}

// Every scene packs its own lights for the camera as it starts to draw.
// Scenes that add their own handler chain to this one (terrain-patches.js,
// tree-canopies.js).
let installed = false;
export function installPointLights() {
  if (installed) return;
  installed = true;
  const previous = THREE.Scene.prototype.onBeforeRender;
  THREE.Scene.prototype.onBeforeRender = function (renderer, scene, camera, target) {
    previous.call(this, renderer, scene, camera, target);
    packPointLights(this, camera);
  };
}
