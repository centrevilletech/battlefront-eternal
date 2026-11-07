import * as THREE from "three";
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

function copyLight(rows,count,light,proxy,scene){
    if(count===L.RAW_LIGHTS)return count;
    const at=count++*P.LEN,e=light.matrixWorld.elements;
    rows[at+P.POSITION]=e[12];rows[at+P.POSITION+1]=e[13];rows[at+P.POSITION+2]=e[14];
    rows[at+P.COLOR]=light.color.r;rows[at+P.COLOR+1]=light.color.g;rows[at+P.COLOR+2]=light.color.b;
    rows[at+P.INTENSITY]=light.intensity;rows[at+P.RANGE]=light.distance;rows[at+P.LAYERS]=light.layers.mask;
    rows[at+P.SHOWN]=+(proxy?light.visible:shown(light,scene));rows[at+P.PROXY]=+!!proxy;
    rows[at+P.INITIAL_SHOWN]=+!!proxy?.shown;rows[at+P.INITIAL_LAYERS]=proxy?.layers??0;
  return count;
}


// Discovery and parent visibility remain graph facts. Native packing owns
// colour/intensity, range rejection and transformed first-person proxies.
export function packPointLights(scene, camera, owner=scene.userData.nativeCameraOwner) {
  const pack=packOf(scene,camera);
  if(pack.owner!==owner){pack.owner=owner;pack.native=owner.nextPack++;}
  const core=owner.core;if(pack.native>=L.PACKS){pack.count=0;core.rust.camera_lights_frame(pack.native);return pack;}core.views();const a=core.cameraInput,rows=core.cameraLights;
  const lights=lightsOf(scene),proxies=scene.userData.nativeViewLights||EMPTY.lights;let count=0;
  for(let i=0;i<lights.length;i++)count=copyLight(rows,count,lights[i],null,scene);
  for(let i=0;i<proxies.length;i++)count=copyLight(rows,count,proxies[i].source,proxies[i],scene);
  a[I.LIGHT_COUNT]=count;const eye=camera.matrixWorld.elements;
  a[I.LIGHT_EYE]=eye[12];a[I.LIGHT_EYE+1]=eye[13];a[I.LIGHT_EYE+2]=eye[14];a[I.LIGHT_FAR]=camera.far;a[I.LIGHT_LAYERS]=camera.layers.mask;a.set(scene.matrixWorld.elements,I.VIEW_WORLD);
  core.rust.camera_lights_frame(pack.native);const out=core.cameraPacks,base=pack.native*K.LEN;pack.count=out[base+K.COUNT];
  for(let i=0;i<pack.count*2;i++)pack.lights[i].fromArray(out,base+K.VALUES+i*4);
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
    this.userData.nativeCameraOwner=renderer.nativeCameraOwner;
    packPointLights(this, camera);
  };
}
