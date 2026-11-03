// The effects pass (effects.rs): the battle's effects, their simulation
// left here in the page, drawn as items of the near scene's transparent
// list (transparent.js: the renderer sorts every source's items with what
// three still draws, as three would sort them all). So far:
//
// - The particle batches: particles.js's (the Classic bolts and their glow,
//   the effects' CPU particles) and effects-gpu.js's analytic ones, every
//   particle renderer's of the near scene (particles.js particleRenderers:
//   the ordnance's, the effects', the map's environment effects').
// - The Classic arc caster's lightning (lightning.js): one mesh rebuilt each
//   frame, drawn twice as three draws a double-sided transparent material.
// - Glow models (game.js glowModel: the muzzle flashes, the effects' model
//   particles): additive basic materials on models, each part a mesh with a
//   world matrix, found in three's transparent list as it first draws them.
// - The droidekas' shields (droideka-shield.js): a shader material on a
//   sphere, its values read from its uniforms each frame (after its
//   onBeforeRender, as three would call it), found the same way.
// - The map's haze and weather: atmosphere.js's square batches (dust, the
//   FogCloud), drawn like particle batches with values of their own;
//   snow.js's flakes, a batch with no instance attributes (its program
//   finds each flake and box by instance number in data textures, which
//   three uploads as they change and the renderer samples); weather.js's
//   rain, a batch of fixed seeds on a streak plane of its own.
// - The Enhanced environment: the rain's ground mist (the rain's own batch
//   with other values), its splashes (rain-impacts.js: instances rewritten
//   as drops land) and its sheets (rain-sheets.js: a shader model around the
//   eye); the steam's wisps (steam-wisps.js) and the fountains' wake
//   splashes (fountain-spray.js), batches of fixed seeds placed by their
//   programs; and the steam's volumes as three composites them
//   (environment-volume.js: a shader model over the screen, its image drawn
//   by three into its own target by the model's onBeforeRender, as three
//   would call it).
// - The fountains' jets (fountain-jets.js): batches of fixed seeds on bases
//   of their own (quads, ribbons, the column, the tier's overflow), which
//   read the frame drawn so far, copied by an object three still draws just
//   before them (its own list then splits the renderer's there); and their
//   two halves of soft spray, each drawn by three into a target of its own
//   and composited by the renderer (shader models, as the steam's volumes).
//
// Each is registered the first time three has drawn it (which gives its
// material a program), then marked external: three is shown it no more and
// uploads it nothing, and each frame this pass writes what it draws into the
// renderer's memory (a CPU batch's instances, an analytic batch's from the
// first that changed, the lightning's vertices and indices as far as they
// are used), with the key three would have sorted it by: group order,
// render order, the centre of its bounding sphere (through its world
// matrix, as three's projectObject takes it) and its id.
//
// Of what the Enhanced looks draw, the bolts (blaster-bolts.js), the
// sniper's trace (sniper-trace.js), the arc caster's lightning
// (arc-lightning.js: its capsules; the glow over a shocked body stays
// three's), the droideka's rolling dust (droideka-trail.js), the
// jetpack's wake (jetpack-wake.js), the destruction's fire and smoke
// (destruction-fx.js) and the death camera's trace of the killing shot
// (death-trace.js: a mesh rebuilt each frame with a shader material of its
// own) are drawn here too; the destruction's arcs and its pieces, and the
// jetpack's flame (jetpack-volume.js), stay with three for now.
import * as THREE from "three";
import { particleRenderers } from "../particles.js";
import { squareBatches } from "../atmosphere.js";
import { classicLightning } from "../lightning.js";
import { arcBatches } from "../arc-lightning.js";
import { droidekaTrails } from "../droideka-trail.js";
import { jetpackWakes } from "../jetpack-wake.js";
import { destructionPools } from "../destruction-fx.js";
import { rainImpacts } from "../rain-impacts.js";
import { fountainJets } from "../fountain-jets.js";
import { particleDepth } from "../particle-depth.js";
import { lighting } from "../materials.js";
import { blendOf, stateOf, sidesOf, programForSide } from "./transparent.js";

// add_program's kinds (web/mod.rs): the world's (a model's model-view
// matrix set per draw), and the effects' (the identity for a model matrix,
// read as modelViewMatrix).
const WORLD = 0, EFFECT = 4;
// The material values a batch's program reads, set by the renderer from
// the frame (soft particles) and the batch's owner (its rows).
const MATERIAL = new Set(["softParticles", "depthResolution", "cameraRange", "trackRows", "emitterRows"]);
const POLICY = { uniforms: new Set(["modelViewMatrix", "map", "sceneDepth", "tracks", "emitters", ...MATERIAL]), material: MATERIAL };
const policyOf = (kind) => ({ uniforms: new Set([...POLICY.uniforms, ...kind.values, ...kind.samplers]), material: MATERIAL });
// A basic material's values (three's refreshUniformsCommon), as the page
// writes them for a mesh: name, size.
const BASIC = [["diffuse", 3], ["opacity", 1], ["mapTransform", 9]];
const MESH_POLICY = { uniforms: new Set(["modelViewMatrix", "map", ...BASIC.map(([name]) => name)]), material: new Set() };
const MODEL_POLICY = MESH_POLICY;
// Each kind of batch: its instance attributes, in the order this pass
// writes them; the material values of its own the page writes after them
// (names; ints among them by `ints`); the textures it samples on units of
// their own (SAMPLER_UNITS). Shown to three never by `external` (it then
// uploads nothing either), or by its material (snow: nothing to upload).
const KINDS = {
  cpu: { attributes: [["iPosition", 3], ["iColor", 4], ["iShape", 4], ["iAxis", 3], ["iAxis2", 3], ["iFrame", 4]], values: [], samplers: [] },
  analytic: { attributes: [["spawnPosition", 3], ["spawnVelocity", 3], ["spawnColor", 4], ["spawnShape", 4], ["spawnMeta", 4]], values: [], samplers: [] },
  square: { attributes: [["center", 4], ["spin", 2]], values: ["tint", "axisU", "axisV"], samplers: [] },
  snow: { attributes: [], values: ["flakeCount", "size", "cameraRight", "cameraUp", "tint", "coverCell", "coverSize"], ints: ["flakeCount"], samplers: ["flakes", "boxes", "cover"], hiddenBy: "material" },
  // The Enhanced bolts (blaster-bolts.js): capsules fitted on the image
  // plane, their pixel's angle set by their onBeforeRender.
  blaster: { attributes: [["iHead", 3], ["iTail", 3], ["iColor", 4], ["iSize", 4]], values: ["pixelAngle"], samplers: [], beforeRender: true },
  // The Enhanced sniper's streaks and flares (sniper-trace.js): batches
  // kept interleaved, one buffer each; the streaks' pixel angle (and the
  // camera the trace aims by) set by their onBeforeRender.
  sniperStreaks: { attributes: [["iHead", 3], ["iTail", 3], ["iColor", 4], ["iSize", 4], ["iMisc", 4]], interleaved: true, values: ["pixelAngle"], samplers: [], beforeRender: true },
  sniperFlares: { attributes: [["iCenter", 3], ["iAxis", 3], ["iColor", 4], ["iParam", 4]], interleaved: true, values: [], samplers: [] },
  // The Enhanced arc caster's lightning (arc-lightning.js ArcBatch): a
  // capsule a segment, kept interleaved; its pixel's angle and the near
  // plane's depth set by its onBeforeRender.
  arc: { attributes: [["iA", 3], ["iB", 3], ["iNA", 3], ["iNB", 3], ["iColor", 4], ["iSize", 4]], interleaved: true,
    values: ["eyeFade", "closeRange", "pixelAngle", "nearDepth"], samplers: [], beforeRender: true },
  // The droideka's rolling dust (droideka-trail.js): wisps on a quad of
  // their own, rewritten each frame as they are born and die, placed by
  // their program from their births and the trail's clock.
  trail: { attributes: [["birth", 4], ["drift", 4], ["shape", 4]], base: [["position", 3], ["uv", 2]], values: ["trailTime", "tint"], samplers: [] },
  // The Enhanced jetpack's wake (jetpack-wake.js): its pools of fire,
  // smoke and embers, particles on a quad of their own placed by their
  // program from their births and the wake's clock.
  wake: { attributes: [["birth", 4], ["flow", 4], ["axis", 4], ["style", 2]], base: [["position", 3], ["uv", 2]], values: ["wakeTime", "kind", "drag"], samplers: [] },
  // The Enhanced destruction's fire and smoke (destruction-fx.js): pools of
  // records on a quad of their own, as many attributes as each keeps,
  // placed by their program from their births and the effects' clock.
  pool: { attributes: "instanced", base: "rest", values: ["fxTime", "wind"], samplers: [] },
  // The rain's drops (weather.js): streaks on a plane of their own (its
  // base: position, uv), each drop's seed fixed (uploaded as it grows).
  rain: { attributes: [["seed", 4]], base: [["position", 3], ["uv", 2]], static: true, hiddenBy: "material",
    values: ["eye", "cameraMotion", "wind", "rainTime", "range", "velocity", "velocityRange", "width", "streak", "enhanced", "effect", "coverCell", "coverSize", "color", "alpha"], samplers: ["cover"] },
  // The steam's wisps (steam-wisps.js) and the fountains' wake splashes
  // (fountain-spray.js): fixed seeds on a quad of their own, single-sided,
  // placed by their programs from the vents' and wakes' values; their
  // onBeforeRender takes the scene's depth whatever the setting (`depth`).
  wisps: { attributes: [["seed", 4]], base: [["position", 3], ["uv", 2]], static: true, hiddenBy: "material", beforeRender: true, depth: true,
    values: ["wispTime", "sources", "directions", "wakes", "motion", "hasDepth"], samplers: [] },
  spray: { attributes: [["seed", 4]], base: [["position", 3], ["uv", 2]], static: true, hiddenBy: "material", beforeRender: true, depth: true,
    values: ["sprayTime", "sources", "wakes", "motion", "hasDepth"], samplers: [] },
  // The rain's splashes (rain-impacts.js): rings and droplets where drops
  // land, rewritten as they land (all of them uploaded when any changed).
  impacts: { attributes: [["impact", 4], ["surfaceNormal", 3], ["kind", 1]], base: [["position", 3], ["uv", 2]], versioned: true,
    values: ["impactTime"], samplers: [] },
  // The fountains' jets (fountain-jets.js): each its instance attributes
  // (fixed: a block of seeds or ropes for each fountain) and its base (what
  // else its program reads) as its geometry has them; their values set by
  // the jets' prepare() rather than their onBeforeRender, which would copy
  // the frame at once (three's copier draws just before them).
  jets: { attributes: "instanced", base: "rest", static: true, hiddenBy: "material", depth: true,
    values: ["jetTime", "side", "perFountain", "sources", "levels", "lows", "visible", "wind", "hasFoam", "hasDepth", "hasScene", "sceneResolution", "fragScale"],
    samplers: ["atlas", "foamMap", "sceneColor"] },
};
// The batches of fixed seeds found in three's list by their meshes' names,
// and the shader models (registerShader).
const SEEDED = { "map-precipitation": "rain", "rain-ground-mist": "rain", "chamber-steam-wisps": "wisps", "fountain-wake-spray": "spray" };
const SHADER_MODELS = new Set(["rain-sheets", "environment-volumes", "fountain-spray-beyond", "fountain-spray-before"]);
// The fountains' jets' meshes (fountain-jets.js).
const JETS = new Set(["fountain-pool", "fountain-overflow", "fountain-ropes", "fountain-jets", "fountain-jets-core", "fountain-jets-near", "fountain-overflow-near", "fountain-ropes-near"]);
// add_batch's corner for a batch with a base of its own (effects.rs NO_CORNER).
const NO_CORNER = 0xffffffff;
const SAMPLER_UNITS = { flakes: 7, boxes: 8, cover: 9, atlas: 10, foamMap: 11, sceneColor: 12 };
// A shader model's units for what it samples besides its map and the scene's depth.
const MODEL_UNITS = [9, 10, 11];
const LIGHTNING = [["position", 3], ["uv", 2], ["color", 4]];
// The frame's inputs (effects.rs IN_*, OWNER_*, DRAW_*).
const IN_DRAWS = 0, IN_OWNERS = 1, IN_SOFT = 2;
const OWNER_AT = 8, OWNER_STRIDE = 4, MAX_OWNERS = 8;
const DRAW_AT = OWNER_AT + MAX_OWNERS * OWNER_STRIDE, DRAW_STRIDE = 12, DRAW_KEY = 6, MAX_DRAWS = 512;
const KIND_BATCH = 0, KIND_MESH = 1, KIND_MODEL = 2;
// Floats of an emitter row (effects-gpu.js).
const EMITTER_FLOATS = 12;
const center = new THREE.Vector4();

// What kind of batch a particle renderer's batch is (null: one this pass
// doesn't draw, such as the Enhanced bolts').
function kindOf(batch) {
  const a = batch.arrays, m = batch.material;
  if (!a || !m?.isShaderMaterial || !batch.mesh || !batch.geometry?.isInstancedBufferGeometry) return null;
  if (a.iShape && a.iPosition) return "cpu";
  if (a.spawnMeta && a.spawnPosition) return "analytic";
  if (a.iHead && a.iSize && !a.iMisc) return "blaster";
  return null;
}

// A batch as this pass reads one, for those that keep their instances
// otherwise: atmosphere.js's squares, snow.js's flakes (no instances of its
// own: its draw's instance count).
function squares(batch) {
  return {
    mesh: batch.mesh, material: batch.material, geometry: batch.geometry, source: batch,
    get count() { return batch.geometry.instanceCount; }, capacity: batch.center.count,
    arrays: { center: batch.center.array, spin: batch.spin.array },
    get external() { return batch.external; }, set external(on) { batch.external = on; },
  };
}
function flakes(mesh) {
  return { mesh, material: mesh.material, geometry: mesh.geometry, get count() { return mesh.geometry.instanceCount; }, capacity: 0, arrays: {}, external: false };
}
// The rain's splashes (rain-impacts.js): their instances as the splashes
// keep them, none while it doesn't rain; hidden from three by `external`.
function splashes(impacts) {
  const mesh = impacts.mesh, g = mesh.geometry;
  return {
    mesh, material: mesh.material, geometry: g, source: impacts,
    get count() { return impacts.shown ? g.instanceCount : 0; }, capacity: g.getAttribute("impact").count,
    arrays: { impact: g.getAttribute("impact").array, surfaceNormal: g.getAttribute("surfaceNormal").array, kind: g.getAttribute("kind").array },
    get external() { return impacts.external; }, set external(on) { impacts.external = on; },
  };
}
// A mesh of instances whatever its attributes (a fountain's jets): its
// instance attributes' arrays by name.
function instancedOf(mesh) {
  const g = mesh.geometry, arrays = {};
  let capacity = 0;
  for (const [name, a] of Object.entries(g.attributes)) if (a.isInstancedBufferAttribute) { arrays[name] = a.array; capacity = a.count; }
  return { mesh, material: mesh.material, geometry: g, get count() { return g.instanceCount; }, capacity, arrays, external: false };
}
// A sniper trace's batch (sniper-trace.js instanced): its instances in one
// interleaved array, `stride` floats each.
function interleavedBatch(batch) {
  return {
    mesh: batch.mesh, material: batch.mesh.material, geometry: batch.geometry, source: batch,
    get count() { return batch.count; }, get capacity() { return batch.capacity; },
    get arrays() { return { interleaved: batch.data }; },
    get external() { return batch.external; }, set external(on) { batch.external = on; },
  };
}
// The droideka's rolling dust (droideka-trail.js): its wisps as the trail
// keeps them; hidden from three by `external`.
function trailOf(trail) {
  const g = trail.geometry, arrays = {};
  for (const name of ["birth", "drift", "shape"]) arrays[name] = g.getAttribute(name).array;
  return {
    mesh: trail.mesh, material: trail.material, geometry: g, source: trail,
    get count() { return trail.count; }, capacity: g.getAttribute("birth").count, arrays,
    get external() { return trail.external; }, set external(on) { trail.external = on; },
  };
}
// A pool of the Enhanced jetpack's wake (jetpack-wake.js): its particles as
// the pool keeps them; hidden from three by `external`.
function wakeOf(pool) {
  const arrays = {};
  for (const [name, attribute] of Object.entries(pool.attributes)) arrays[name] = attribute.array;
  return {
    mesh: pool.mesh, material: pool.mesh.material, geometry: pool.mesh.geometry, source: pool,
    get count() { return pool.count; }, capacity: pool.capacity, arrays,
    get external() { return pool.external; }, set external(on) { pool.external = on; },
  };
}
// A pool of the Enhanced destruction's fire or smoke (destruction-fx.js):
// its records as the pool keeps them; hidden from three by `external`.
function poolOf(pool) {
  const g = pool.mesh.geometry, arrays = {};
  for (const [name, attribute] of Object.entries(g.attributes)) if (attribute.isInstancedBufferAttribute) arrays[name] = attribute.array;
  return {
    mesh: pool.mesh, material: pool.material, geometry: g, source: pool,
    get count() { return pool.count; }, capacity: pool.capacity, arrays,
    get external() { return pool.external; }, set external(on) { pool.external = on; },
  };
}
function drops(mesh) {
  const seed = mesh.geometry.getAttribute("seed");
  return { mesh, material: mesh.material, geometry: mesh.geometry, get count() { return mesh.geometry.instanceCount; }, capacity: seed.count, arrays: { seed: seed.array }, external: false };
}

// A glow model's part (game.js glowModel: one material for its parts, kept
// on its group).
function glowOf(object) {
  const m = object.material;
  return object.isMesh && !object.isSkinnedMesh && !object.isInstancedMesh && m?.isMeshBasicMaterial && m.transparent && object.parent?.userData?.material === m;
}

// src[from..to) into data from `at`, without a view for a short run; where
// it ends.
function copy(data, at, src, from, to) {
  const n = to - from;
  if (n <= 16) for (let i = 0; i < n; i++) data[at + i] = src[from + i];
  else data.set(src.subarray(from, to), at);
  return at + n;
}
// The same for 32-bit indices (kept apart: one array type each, for the JIT).
function copyBits(bits, at, src, from, to) {
  const n = to - from;
  if (n <= 16) for (let i = 0; i < n; i++) bits[at + i] = src[from + i];
  else bits.set(src.subarray(from, to), at);
  return at + n;
}

// A droideka's shield (droideka-shield.js).
function shieldOf(object) {
  return object.isMesh && object.material?.isShaderMaterial && object.material.name === "droideka-energy-shield";
}

// A uniform's value as floats: [size, form] (effects.rs MaterialValue: 0 a
// float, vector or matrix; 1 a float array; 4 a vec4 array), or null for
// one the renderer doesn't take.
function formOf(v) {
  if (typeof v === "number" || typeof v === "boolean") return [1, 0];
  if (v?.isVector2) return [2, 0];
  if (v?.isVector3 || v?.isColor) return [3, 0];
  if (v?.isVector4) return [4, 0];
  if (v?.isMatrix3) return [9, 0];
  if (v?.isMatrix4) return [16, 0];
  if (ArrayBuffer.isView(v) || (Array.isArray(v) && v.every((x) => typeof x === "number"))) return [v.length, 1];
  if (Array.isArray(v) && v.length && v.every((x) => x?.isVector4)) return [4 * v.length, 4];
  return null;
}

// A uniform's value into data from `at`; where it ends.
function put(data, at, v) {
  if (typeof v === "number") data[at++] = v;
  else if (typeof v === "boolean") data[at++] = v ? 1 : 0;
  else if (v.isColor) { data[at++] = v.r; data[at++] = v.g; data[at++] = v.b; }
  else if (v.isVector2) { data[at++] = v.x; data[at++] = v.y; }
  else if (v.isVector3) { data[at++] = v.x; data[at++] = v.y; data[at++] = v.z; }
  else if (v.isVector4) { data[at++] = v.x; data[at++] = v.y; data[at++] = v.z; data[at++] = v.w; }
  else if (v.isMatrix3 || v.isMatrix4) { data.set(v.elements, at); at += v.elements.length; }
  else if (Array.isArray(v) && v[0]?.isVector4) for (const x of v) at = put(data, at, x);
  else at = copy(data, at, v, 0, v.length);
  return at;
}

// The order of the nearest Group above an object, as three's projectObject
// passes it down (0 without one).
function groupOrderOf(object) {
  for (let o = object.parent; o; o = o.parent) if (o.isGroup) return o.renderOrder;
  return 0;
}

class EffectsPass {
  constructor(core) {
    this.core = core;
    this.name = "effects";
    this.phase = "transparent";
    this.records = []; // registered batches
    this.byBatch = new Map(); // batch -> its record, or null (three draws it)
    this.owners = new Map(); // GpuEffects -> { id }
    this.meshes = new Map(); // the lightning's material -> its record, or null
    this.shaderMeshes = new Map(); // the death camera's trace -> its record, or null
    this.glows = new Map(); // a glow model's part -> its record, or null (three draws it)
    this.glowList = []; // the records, in registration order
    // This frame's batches and glow models drawn (kept, not made each frame).
    this.drawn = [];
    this.glowing = [];
    this.traced = [];
    this.models = new Map(); // what a model is made of -> its id
    // The renderer's programs for this pass's draws, made for it alone:
    // effects.rs keeps the values it last set in each program's uniforms
    // (`held`) and skips setting them again, so no other pass may set them
    // (a command post's beam can draw with a glow model's three program).
    this.programs = new Map();
    this.batchCount = -1;
    this.waiting = new Map(); // batch -> its registration, until three has drawn it
    this.frame = 0;
    this.active = true;
    // What the measurement tools read: last frame's draws and instances.
    this.draws = 0;
    this.instances = 0;
    core.addView("fxInput", Float64Array, () => core.rust.fx_input_ptr(), () => core.rust.fx_input_len());
    core.addView("fxData", Float32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
    core.addView("fxBits", Uint32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
    core.addView("fxMatrices", Float64Array, () => core.rust.fx_matrices_ptr(), () => core.rust.fx_matrices_len());
  }

  // three's render list of the near scene being drawn: the one whose opaque
  // list holds the far scene's image (lists are kept per render depth).
  threeList(renderer, scene) {
    const background = this.core.graphics.background;
    for (let depth = 0; depth < 4; depth++) {
      const list = renderer.renderLists.get(scene, depth);
      if (!list.opaque.length) return null;
      if (list.opaque.some((item) => item.object === background)) return list;
    }
    return null;
  }

  // What three draws this frame that this pass has not seen, each
  // registered once three has drawn it, then hidden from three: glow models
  // (with their group's other parts: they share its material), shields and
  // the other shader models, the snow, the rain, the environment's batches,
  // the Enhanced arcs', the droideka's dust, the jetpack's wake, the
  // destruction's fire and smoke and the death camera's trace.
  listed(ctx) {
    const list = this.threeList(ctx.renderer, ctx.scene);
    for (const item of list?.transparent || []) {
      const object = item.object;
      if (this.glows.has(object) || this.byBatch.has(object)) continue;
      if (glowOf(object)) this.registerGlow(object.parent);
      else if (shieldOf(object) || (SHADER_MODELS.has(object.name) && object.material?.isShaderMaterial)) this.registerShader(object);
      else if (object.name === "map-snow" && object.material?.isShaderMaterial) {
        const record = this.registerBatch(flakes(object), "snow", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (SEEDED[object.name] && object.material?.isShaderMaterial && object.geometry.getAttribute("seed")?.itemSize === 4) {
        const record = this.registerBatch(drops(object), SEEDED[object.name], null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (object.name === "rain-surface-impacts" && rainImpacts.has(object)) {
        const record = this.registerBatch(splashes(rainImpacts.get(object)), "impacts", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (JETS.has(object.name) && fountainJets.has(object) && object.material?.isShaderMaterial) {
        const record = this.registerBatch(instancedOf(object), "jets", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (object.name === "arc-lightning" && arcBatches.has(object) && object.material?.isShaderMaterial) {
        const record = this.registerBatch(interleavedBatch(arcBatches.get(object)), "arc", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (object.name === "droideka-rolling-dust" && droidekaTrails.has(object) && object.material?.isShaderMaterial) {
        const record = this.registerBatch(trailOf(droidekaTrails.get(object)), "trail", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (object.name.startsWith("jetpack-wake-") && jetpackWakes.has(object) && object.material?.isShaderMaterial) {
        const record = this.registerBatch(wakeOf(jetpackWakes.get(object)), "wake", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (destructionPools.has(object) && object.material?.isShaderMaterial) {
        const record = this.registerBatch(poolOf(destructionPools.get(object)), "pool", null);
        if (record !== undefined) this.byBatch.set(object, record);
      } else if (object.name === "death-trace" && object.material?.isShaderMaterial && !this.shaderMeshes.has(object)) {
        const record = this.registerShaderMesh(object);
        if (record !== undefined) this.shaderMeshes.set(object, record);
      }
    }
    // Now and then, the records of parts no longer in the scene let go.
    if (this.frame % 600 === 0) {
      this.glowList = this.glowList.filter((record) => {
        let o = record.mesh;
        while (o.parent) o = o.parent;
        if (o === ctx.scene) return true;
        this.glows.delete(record.mesh);
        return false;
      });
    }
  }

  registerGlow(group) {
    const core = this.core, renderer = core.renderer, material = group.userData.material;
    const parts = group.children.filter((o) => o.isMesh && o.material === material);
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return; // three draws it first
    const fail = (why) => { core.errors.push(`effects glow: ${why}`); for (const mesh of parts) this.glows.set(mesh, null); };
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return fail("state");
    const sides = sidesOf(material);
    const programs = sides.map((side) => { const p = programForSide(renderer, material, side); return p && core.program(p, WORLD, material, MODEL_POLICY, this.programs); });
    if (programs.some((p) => !p)) return fail("program");
    const map = material.map ?? null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return; // its texture first
    const values = BASIC.filter(([name]) => programs.some((p) => p.names.has(name)));
    const records = [];
    for (const mesh of parts) {
      const geometry = mesh.geometry;
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return fail("geometry");
      const geometries = programs.map((p) => core.geometry(geometry, p));
      if (geometries.some((g) => g === null || g === undefined)) return fail("geometry");
      const key = `${geometry.uuid}|${texture}|${blend.join()}|${state.func}|${state.flags}|${programs.map((p) => p.id).join()}|${sides.join()}`;
      let model = this.models.get(key);
      if (model === undefined) {
        if (!geometry.boundingSphere) geometry.computeBoundingSphere();
        const s = geometry.boundingSphere;
        model = core.rust.fx_add_model(texture, new Uint32Array(blend), state.flags & 3, state.func, values.map(([name, size]) => `${name}:${size}`).join(","), new Float64Array([s.center.x, s.center.y, s.center.z, s.radius]));
        sides.forEach((side, i) => core.rust.fx_model_pass(model, programs[i].id, side, geometries[i]));
        this.models.set(key, model);
      }
      records.push({ mesh, model, material, map, values, from: this.frame });
    }
    for (const record of records) { this.glows.set(record.mesh, record); this.glowList.push(record); }
    material.visible = false;
  }

  // The near scene's particle renderers' batches, looked over again when
  // one was added: new ones registered (or left to three).
  batches() {
    const scene = this.core.graphics.scene;
    const renderers = particleRenderers.get(scene) || [], haze = squareBatches.get(scene) || [];
    let n = haze.length;
    for (const r of renderers) n += r.batches.size;
    if (n !== this.batchCount) {
      this.batchCount = n;
      for (const r of renderers) {
        for (const batch of r.batches.values()) {
          if (this.byBatch.has(batch) || this.waiting.has(batch)) continue;
          // The Enhanced sniper's trace: two batches of its own.
          if (batch.streaks?.data && batch.flares?.data) {
            this.byBatch.set(batch, null);
            for (const [part, kind] of [[batch.streaks, "sniperStreaks"], [batch.flares, "sniperFlares"]]) this.waiting.set(part, () => this.registerBatch(interleavedBatch(part), kind, r));
            continue;
          }
          const kind = kindOf(batch);
          if (!kind) this.byBatch.set(batch, null);
          else this.waiting.set(batch, () => this.registerBatch(batch, kind, r));
        }
      }
      for (const batch of haze) if (!this.byBatch.has(batch) && !this.waiting.has(batch)) this.waiting.set(batch, () => this.registerBatch(squares(batch), "square", null));
    }
    // Those waiting for three to draw them first (the Enhanced bolts' batch
    // never is in Classic): a look at their program each frame.
    for (const [batch, register] of this.waiting) {
      const record = register();
      if (record === undefined) continue;
      this.byBatch.set(batch, record);
      this.waiting.delete(batch);
    }
  }

  // A batch's record, or null when the renderer can't draw it; undefined
  // while three has not drawn it yet.
  registerBatch(batch, kind, renderer) {
    const core = this.core, material = batch.material;
    let spec = KINDS[kind];
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (!threeProgram) return undefined;
    const fail = (why) => { core.errors.push(`effects ${kind}: ${why}`); return null; };
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    // A particle material: depth tested less-or-equal, not written, drawn in
    // one pass (double-sided, or the side three draws).
    if (!blend || !state || state.func !== 0x0203 || state.flags !== 5 || sides.length !== 1) return fail("state");
    const program = core.program(threeProgram, EFFECT, material, policyOf(spec), this.programs);
    if (!program) return fail("program");
    // Attributes as its geometry has them: instance ones, then its base.
    if (spec.attributes === "instanced") {
      const g = batch.geometry, read = program.attributes.map(([name]) => name).filter((name) => g.getAttribute(name));
      spec = { ...spec, attributes: read.filter((name) => g.getAttribute(name).isInstancedBufferAttribute).map((name) => [name, g.getAttribute(name).itemSize]),
        base: read.filter((name) => !g.getAttribute(name).isInstancedBufferAttribute).map((name) => [name, g.getAttribute(name).itemSize]) };
      if (!spec.base.length) return fail("base");
    }
    const at = new Map(program.attributes);
    const attributes = spec.attributes;
    for (const [name, size] of attributes) if (!at.has(name) || batch.geometry.getAttribute(name)?.itemSize !== size) return fail(`attribute ${name}`);
    if (batch.geometry.index?.array.constructor !== Uint16Array) return fail("index");
    for (const [name, size] of spec.base || []) if (!at.has(name) || batch.geometry.getAttribute(name)?.itemSize !== size || batch.geometry.getAttribute(name).isInstancedBufferAttribute) return fail(`base ${name}`);
    if (!spec.base && !at.has("corner")) return fail("quad");
    const map = material.uniforms.map?.value ?? null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return undefined; // not uploaded yet: next time
    let owner = -1;
    if (kind === "analytic") {
      const gpu = renderer.gpu;
      if (!gpu || material.uniforms.tracks !== gpu.uniforms.tracks) return fail("owner");
      owner = this.owner(gpu);
      if (owner < 0) return fail("tracks");
    }
    // Its own values: what its program reads of them (ints as ints).
    const values = [];
    for (const name of spec.values) {
      if (!program.names.has(name)) continue;
      const form = formOf(material.uniforms[name]?.value);
      if (!form) return fail(`value ${name}`);
      values.push([name, form[0], spec.ints?.includes(name) ? 2 : form[1]]);
    }
    const samplers = [];
    for (const name of spec.samplers) {
      const tex = material.uniforms[name]?.value ?? null;
      const id = tex ? core.texture(tex) : -1;
      if (tex && id < 0) return undefined; // not uploaded yet: next time
      samplers.push({ name, unit: SAMPLER_UNITS[name], map: tex, handle: this.handle(tex), id });
    }
    const id = core.rust.fx_add_batch(program.id, texture, new Uint32Array(blend), spec.base ? NO_CORNER : at.get("corner"), new Uint32Array(attributes.flatMap(([name, size]) => [at.get(name), size])), owner);
    if (spec.base) {
      // Its own vertices and indices, as its geometry has them.
      const g = batch.geometry, vertices = g.getAttribute(spec.base[0][0]).count;
      const data = new Float32Array(spec.base.reduce((sum, [, size]) => sum + size * vertices, 0));
      let o = 0;
      for (const [name, size] of spec.base) { data.set(g.getAttribute(name).array.subarray(0, vertices * size), o); o += vertices * size; }
      core.rust.fx_batch_base(id, new Uint32Array(spec.base.flatMap(([name, size]) => [at.get(name), size])), vertices, data, new Uint16Array(g.index.array.subarray(0, g.drawRange.count === Infinity ? g.index.count : Math.min(g.drawRange.count, g.index.count))));
    }
    if (spec.interleaved) core.rust.fx_batch_interleaved(id);
    if (sides[0] !== 2) core.rust.fx_batch_side(id, sides[0]);
    if (values.length) core.rust.fx_batch_values(id, values.map(([name, size, form]) => `${name}:${size}:${form}`).join(","));
    for (const sampler of samplers) core.rust.fx_batch_sampler(id, sampler.name, sampler.unit, sampler.id);
    // Its key but z, which never changes: no group above it but the map's
    // environment effects', the origin for a centre (no positions), the
    // identity for a world matrix.
    const key = new Float64Array(6);
    this.keyOf(batch.mesh, key);
    const record = { batch, kind, id, attributes, values, samplers, arrays: null, mesh: batch.mesh, map, handle: this.handle(map), version: map?.version, key,
      stride: attributes.reduce((sum, [, size]) => sum + size, 0), sizes: attributes.map(([, size]) => size), beforeRender: Boolean(spec.beforeRender), static: Boolean(spec.static), uploaded: 0,
      interleaved: Boolean(spec.interleaved), versioned: Boolean(spec.versioned), dataVersion: -1, depth: Boolean(spec.depth),
      jets: kind === "jets" ? fountainJets.get(batch.mesh) : null,
      hiddenBy: spec.hiddenBy ?? "external", synced: false, capacity: -1, from: this.frame };
    this.hide(record, true);
    this.records.push(record);
    return record;
  }

  // Hidden from three (the renderer draws it), or shown again.
  hide(record, hidden) {
    const b = record.batch;
    if (record.hiddenBy === "material") { b.material.visible = !hidden; return; }
    b.external = hidden;
    b.mesh.visible = hidden ? false : b.count > 0;
  }

  // An owner of analytic batches (effects-gpu.js GpuEffects), registered once.
  owner(gpu) {
    let o = this.owners.get(gpu);
    if (!o) {
      const tracks = this.core.texture(gpu.uniforms.tracks.value);
      if (tracks < 0) return -1;
      o = { id: this.core.rust.fx_add_owner(tracks, gpu.uniforms.trackRows.value) };
      this.owners.set(gpu, o);
    }
    return o.id;
  }

  // The Classic lightning of the near scene, while its material is one the
  // renderer has (registered once three drew it); null otherwise. While it
  // wears a material three has not drawn yet, three draws it.
  lightning() {
    const lightning = classicLightning.get(this.core.graphics.scene);
    if (!lightning || lightning.isEnhanced()) return null;
    const material = lightning.mesh.material;
    if (!material) return null;
    let record = this.meshes.get(material);
    if (record === undefined) {
      record = this.registerMesh(lightning.mesh, material, LIGHTNING);
      if (record !== undefined) this.meshes.set(material, record);
    }
    lightning.external = Boolean(record);
    return record ? { lightning, record } : null;
  }

  // A mesh drawn as three draws it (each of its sides in turn); undefined
  // while three has not drawn it as it is yet, null when the renderer can't.
  registerMesh(mesh, material, attributes) {
    const core = this.core, renderer = core.renderer;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return undefined;
    const fail = (why) => { core.errors.push(`effects mesh: ${why}`); return null; };
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return fail("state");
    const sides = sidesOf(material);
    const programs = sides.map((side) => { const p = programForSide(renderer, material, side); return p && core.program(p, EFFECT, material, MESH_POLICY, this.programs); });
    if (programs.some((p) => !p)) return fail("program");
    const locations = programs.map((p) => { const at = new Map(p.attributes); return attributes.map(([name]) => at.get(name)); });
    if (locations.some((l) => l.some((v) => v === undefined))) return fail("attributes");
    for (const [name, size] of attributes) if (mesh.geometry.getAttribute(name)?.itemSize !== size) return fail(`attribute ${name}`);
    if (!(mesh.geometry.index?.array instanceof Uint32Array)) return fail("index");
    const map = material.map ?? null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return undefined;
    const values = BASIC.filter(([name]) => programs.some((p) => p.names.has(name)));
    const id = core.rust.fx_add_mesh(texture, new Uint32Array(blend), state.flags & 3, state.func, new Uint32Array(attributes.map(([, size]) => size)), values.map(([name, size]) => `${name}:${size}`).join(","));
    sides.forEach((side, i) => core.rust.fx_mesh_pass(id, programs[i].id, side, new Uint32Array(locations[i])));
    return { id, mesh, material, map, handle: this.handle(map), values, attributes, from: this.frame };
  }

  // A mesh rebuilt each frame with a shader material of its own (the death
  // camera's trace): drawn as the lightning is (its vertices and indices
  // written each frame, each of three's passes in turn), with the values its
  // programs read of its uniforms. Undefined while three has not drawn it as
  // it is yet, null when the renderer can't.
  registerShaderMesh(mesh) {
    const core = this.core, renderer = core.renderer, material = mesh.material, geometry = mesh.geometry;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return undefined;
    const fail = (why) => { core.errors.push(`effects ${mesh.name}: ${why}`); return null; };
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return fail("state");
    const index = geometry.index?.array;
    if (!(index instanceof Uint16Array || index instanceof Uint32Array) || geometry.groups.length || Object.keys(geometry.morphAttributes).length) return fail("geometry");
    const uniforms = material.uniforms, names = Object.keys(uniforms);
    if (names.some((name) => uniforms[name].value?.isTexture)) return fail("samplers");
    const sides = sidesOf(material);
    const policy = { uniforms: new Set([...names, "modelViewMatrix"]), material: new Set() };
    const programs = sides.map((side) => { const p = side === 2 ? props.currentProgram : programForSide(renderer, material, side); return p && core.program(p, EFFECT, material, policy, this.programs); });
    if (programs.some((p) => !p)) return fail("program");
    // Its attributes as its programs read them, each of its geometry's.
    const read = new Set(programs.flatMap((p) => p.attributes.map(([name]) => name)));
    const attributes = Object.entries(geometry.attributes).filter(([name, a]) => read.has(name) && !a.isInterleavedBufferAttribute && a.array instanceof Float32Array).map(([name, a]) => [name, a.itemSize]);
    if (attributes.length !== read.size) return fail("attributes");
    const locations = programs.map((p) => { const at = new Map(p.attributes); return attributes.map(([name]) => at.get(name)); });
    // Its values: what its programs read of its uniforms but the frame's.
    const values = [];
    for (const name of names) {
      if (!programs.some((p) => p.names.has(name)) || uniforms[name] === lighting[name]) continue;
      const form = formOf(uniforms[name].value);
      if (!form || form[1] !== 0) return fail(`value ${name}`);
      values.push([name, form[0]]);
    }
    const id = core.rust.fx_add_mesh(-1, new Uint32Array(blend), state.flags & 3, state.func, new Uint32Array(attributes.map(([, size]) => size)), values.map(([name, size]) => `${name}:${size}`).join(","));
    sides.forEach((side, i) => core.rust.fx_mesh_pass(id, programs[i].id, side, new Uint32Array(locations[i])));
    material.visible = false;
    return { id, mesh, material, attributes, values, from: this.frame };
  }

  // A mesh with a shader material of its own, its values read from its
  // uniforms each frame (a droideka's shield, the rain's sheets, the
  // steam's volumes). Registered once three has drawn it as it is. What it
  // samples: its first texture on the map's unit, the scene's depth on its
  // own (the pass binds it), any other on a unit of its own (MODEL_UNITS).
  // One three does not cull is never culled.
  registerShader(mesh) {
    const core = this.core, renderer = core.renderer, material = mesh.material, geometry = mesh.geometry;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return; // three draws it first
    const fail = (why) => { core.errors.push(`effects ${mesh.name}: ${why}`); this.glows.set(mesh, null); };
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return fail("state");
    const sides = sidesOf(material);
    // The uniforms it may read: its own, the matrices set per draw.
    const uniforms = material.uniforms, names = Object.keys(uniforms);
    const samplers = names.filter((name) => name === "sceneDepth" || uniforms[name].value?.isTexture || (uniforms[name].value === null && /map$/i.test(name)));
    const textures = samplers.filter((name) => name !== "sceneDepth");
    if (textures.length > 1 + MODEL_UNITS.length) return fail("samplers");
    const policy = { uniforms: new Set([...names, "modelViewMatrix", "normalMatrix"]), material: new Set() };
    const programs = sides.map((side) => { const p = side === 2 ? props.currentProgram : programForSide(renderer, material, side); return p && core.program(p, WORLD, material, policy, this.programs); });
    if (programs.some((p) => !p)) return fail("program");
    if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return fail("geometry");
    const geometries = programs.map((p) => core.geometry(geometry, p));
    if (geometries.some((g) => g === null || g === undefined)) return fail("geometry");
    // Its values: what its programs read of its uniforms but the frame's
    // (the frame block holds those) and its sampler.
    const values = [];
    for (const name of names) {
      // (A value the frame block holds is materials.js lighting's own: the program check made sure.)
      if (samplers.includes(name) || !programs.some((p) => p.names.has(name)) || uniforms[name] === lighting[name]) continue;
      const form = formOf(uniforms[name].value);
      if (!form) return fail(`value ${name}`);
      values.push([name, ...form]);
    }
    const map = textures.length ? uniforms[textures[0]].value : null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return; // its texture first
    const extra = textures.slice(1).map((name, k) => ({ name, unit: MODEL_UNITS[k], map: uniforms[name].value, handle: null, id: -1 }));
    for (const x of extra) {
      x.id = x.map ? core.texture(x.map) : -1;
      if (x.map && x.id < 0) return; // its textures first
      x.handle = this.handle(x.map);
    }
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    const s = geometry.boundingSphere, sphere = mesh.frustumCulled ? [s.center.x, s.center.y, s.center.z, s.radius] : [0, 0, 0, -1];
    const model = core.rust.fx_add_model(texture, new Uint32Array(blend), state.flags & 3, state.func, values.map(([name, size, form]) => `${name}:${size}:${form}`).join(","), new Float64Array(sphere));
    sides.forEach((side, i) => core.rust.fx_model_pass(model, programs[i].id, side, geometries[i]));
    for (const x of extra) core.rust.fx_model_sampler(model, x.name, x.unit, x.id);
    const record = { mesh, model, material, map, handle: this.handle(map), sampler: textures[0] ?? null, extra, depth: samplers.includes("sceneDepth"), values, shader: true, from: this.frame };
    this.glows.set(mesh, record);
    this.glowList.push(record);
    material.visible = false;
  }

  // A texture's GL handle, uploaded first if three has a newer version of
  // it than it last uploaded (three uploads one only as it draws with it).
  handle(map) {
    if (!map) return null;
    const renderer = this.core.renderer, props = renderer.properties.get(map);
    if (props.__webglTexture && props.__version !== map.version) renderer.initTexture(map);
    return props.__webglTexture ?? null;
  }

  // Shown as three would show it: in the scene, it and its parents visible,
  // in the camera's layers.
  shown(mesh, camera) {
    const scene = this.core.graphics.scene;
    if (!mesh.layers.test(camera.layers)) return false;
    let o = mesh.parent;
    for (; o && o !== scene; o = o.parent) if (!o.visible) return false;
    return o === scene;
  }

  // three's sort key for an object (but z, which the renderer works out):
  // group order, render order, its bounding sphere's centre through its
  // world matrix, its id; into out from `at`.
  keyOf(object, out, at = 0) {
    const geometry = object.geometry;
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    center.copy(geometry.boundingSphere.center).applyMatrix4(object.matrixWorld);
    out[at] = groupOrderOf(object); out[at + 1] = object.renderOrder;
    out[at + 2] = center.x; out[at + 3] = center.y; out[at + 4] = center.z; out[at + 5] = object.id;
  }

  // The transparent phase, before the renderer's call: the frame's draws.
  // An error here gives the effects back to three for good (this frame's
  // are not drawn) rather than breaking three's render around it.
  draw(ctx) {
    try {
      this.drawFrame(ctx);
    } catch (error) {
      this.core.errors.push(`effects failed: ${String(error?.stack || error).slice(0, 400)}`);
      console.warn("Rust effects pass failed, three draws the effects:", error);
      this.failed = true;
      try { this.core.views(); this.core.fxInput[IN_DRAWS] = 0; } catch {}
      try { this.setActive(false); } catch (e) { this.core.errors.push(`effects restore: ${String(e).slice(0, 200)}`); }
    }
  }

  drawFrame(ctx) {
    const core = this.core;
    this.frame++;
    if (!this.active) {
      // Off (three draws them): none of last frame's draws again.
      core.views();
      core.fxInput[IN_DRAWS] = 0;
      return;
    }
    this.batches();
    this.listed(ctx);
    const bolt = this.lightning();
    // Textures three uploaded anew, or made anew (a new handle: registered
    // again); looked at when their version moved. Registering may move the
    // renderer's memory: views after.
    for (const record of this.records) {
      const map = record.map;
      if (!map || map.version === record.version) continue;
      record.version = map.version;
      const handle = this.handle(map);
      if (handle && handle !== record.handle) { record.handle = handle; core.rust.fx_batch_texture(record.id, core.texture(map)); }
    }
    if (bolt) {
      const record = bolt.record, handle = this.handle(record.map);
      if (handle && handle !== record.handle) { record.handle = handle; core.rust.fx_mesh_texture(record.id, core.texture(record.map)); }
    }
    // Soft particles: the scene's depth, taken before the first of them as
    // three's batches take it (particle-depth.js); an effect that reads it
    // whatever the setting takes it in its onBeforeRender. Bound for the
    // frame's draws when anything drawn reads it.
    const soft = particleDepth.enabled.value > 0.5;
    if (soft) particleDepth.capture(ctx.renderer, ctx.scene, ctx.camera);
    let reads = soft;
    // What three would have drawn this frame, registered before it (three
    // drew what was registered this frame itself), and the floats it needs.
    const camera = ctx.camera, frame = this.frame, drawn = this.drawn, glowing = this.glowing;
    drawn.length = glowing.length = 0;
    let need = 0, jets = false;
    for (const record of this.records) {
      const b = record.batch;
      if (record.from === frame || b.count <= 0 || (record.hiddenBy !== "material" && b.material.visible === false) || !b.mesh.visible && record.hiddenBy === "material" || !this.shown(record.mesh, camera)) continue;
      drawn.push(record);
      if (record.depth) reads = true;
      // The fountains' jets: their values set, and the target the frame so
      // far goes into sized, once a frame, before their samplers.
      if (record.jets && !jets) { jets = true; record.jets.prepare(ctx.renderer, camera); }
      need += b.count * record.stride;
      for (const [, size] of record.values) need += size;
      // Values its onBeforeRender sets, as three would call it.
      if (record.beforeRender) record.mesh.onBeforeRender(ctx.renderer, ctx.scene, camera, b.geometry, b.material, null);
      // Its data textures, uploaded by three as they change.
      for (const sampler of record.samplers) {
        const tex = b.material.uniforms[sampler.name].value ?? null, handle = this.handle(tex);
        if (tex !== sampler.map || handle !== sampler.handle) {
          sampler.map = tex; sampler.handle = handle;
          core.rust.fx_batch_sampler(record.id, sampler.name, sampler.unit, tex ? core.texture(tex) : -1);
        }
      }
    }
    const lit = bolt && bolt.record.from !== frame && bolt.lightning.index > 0 && bolt.record.material.visible !== false && this.shown(bolt.lightning.mesh, camera) ? bolt : null;
    if (lit) need += lit.lightning.vertex * 9 + lit.lightning.index + 13;
    // The shader meshes shown (the death camera's trace: never culled).
    const traced = this.traced;
    traced.length = 0;
    for (const record of this.shaderMeshes.values()) {
      if (!record || record.from === frame || !record.mesh.visible || !this.shown(record.mesh, camera)) continue;
      const g = record.mesh.geometry;
      traced.push(record);
      need += g.getAttribute("position").count * record.attributes.reduce((sum, [, size]) => sum + size, 0) + g.index.count + record.values.reduce((sum, [, size]) => sum + size, 0);
    }
    // Glow models and shields shown (three culls them by their spheres: the
    // renderer does). A shield's onBeforeRender runs first, as three's would
    // (it sets whether the camera is inside it); its texture follows three's.
    for (const record of this.glowList) {
      if (record.from === frame || !record.mesh.visible || !this.shown(record.mesh, camera)) continue;
      glowing.push(record);
      if (!record.shader) { need += 13; continue; }
      if (record.depth) reads = true;
      record.mesh.onBeforeRender(ctx.renderer, ctx.scene, camera, record.mesh.geometry, record.material, null);
      for (const [, size] of record.values) need += size;
      const map = record.sampler ? record.material.uniforms[record.sampler].value : null;
      const handle = this.handle(map);
      if (map !== record.map || handle !== record.handle) { record.map = map; record.handle = handle; core.rust.fx_model_texture(record.model, map ? core.texture(map) : -1); }
      // Its other textures, made anew too (a target three sized again).
      for (const x of record.extra) {
        const tex = record.material.uniforms[x.name].value ?? null, h = this.handle(tex);
        if (tex !== x.map || h !== x.handle) { x.map = tex; x.handle = h; core.rust.fx_model_sampler(record.model, x.name, x.unit, tex ? core.texture(tex) : -1); }
      }
    }
    const depth = reads && particleDepth.map.value ? core.texture(particleDepth.map.value) : -1;
    for (const gpu of this.owners.keys()) need += Math.min(gpu.count, gpu.uniforms.emitterRows.value) * EMITTER_FLOATS;
    core.views();
    if (need > core.fxData.length) {
      core.rust.fx_reserve(Math.max(need * 2, core.fxData.length * 2));
      core.buffer = null;
      core.views();
    }
    const input = core.fxInput, data = core.fxData, bits = core.fxBits, matrices = core.fxMatrices;
    let at = 0, owners = 0, draws = 0, instances = 0;
    for (const [gpu, o] of this.owners) {
      if (owners === MAX_OWNERS) break;
      const rows = gpu.uniforms.emitterRows.value, written = Math.min(gpu.count, rows);
      const e = OWNER_AT + owners++ * OWNER_STRIDE;
      input[e] = o.id; input[e + 1] = rows; input[e + 2] = written; input[e + 3] = at;
      at = copy(data, at, gpu.data, 0, written * EMITTER_FLOATS);
    }
    for (const record of drawn) {
      if (draws === MAX_DRAWS) break;
      const b = record.batch, count = b.count, attributes = record.attributes;
      // The batch's arrays (made anew as it grows).
      if (record.capacity !== b.capacity && !record.interleaved) record.arrays = attributes.map(([name]) => b.arrays[name]);
      // An analytic batch's instances from the first that changed since it
      // was last read (effects-gpu.js unread), when the renderer holds the
      // rest; all of them otherwise.
      let from = 0;
      if (record.kind === "analytic") {
        if (record.synced && record.capacity === b.capacity) from = Math.min(Math.max(b.unread ?? 0, 0), count);
        b.unread = Infinity;
      } else if (record.static) {
        // Fixed instances: those it did not have yet.
        if (record.synced && record.capacity === b.capacity) from = Math.min(record.uploaded, count);
        record.uploaded = Math.max(from === 0 ? 0 : record.uploaded, count);
      } else if (record.versioned) {
        // Instances rewritten as they change: none when none did.
        let version = 0;
        for (const [name] of attributes) version += b.geometry.getAttribute(name).version;
        if (record.synced && record.capacity === b.capacity && version === record.dataVersion) from = count;
        record.dataVersion = version;
      }
      const start = at, arrays = record.arrays, sizes = record.sizes;
      if (record.interleaved) at = copy(data, at, b.arrays.interleaved, from * record.stride, count * record.stride);
      else for (let k = 0; k < arrays.length; k++) at = copy(data, at, arrays[k], from * sizes[k], count * sizes[k]);
      if (record.values.length) for (const [name] of record.values) at = put(data, at, b.material.uniforms[name].value);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_BATCH; input[e + 1] = record.id; input[e + 2] = count; input[e + 3] = from; input[e + 4] = start; input[e + 5] = b.capacity;
      input.set(record.key, e + DRAW_KEY);
      record.synced = true;
      record.capacity = b.capacity;
      instances += count;
    }
    if (lit && draws < MAX_DRAWS) {
      const { lightning, record } = lit, geometry = lightning.geometry, material = record.material;
      const vertices = lightning.vertex, indices = lightning.index, start = at;
      for (const [name, size] of record.attributes) at = copy(data, at, geometry.attributes[name].array, 0, vertices * size);
      at = copyBits(bits, at, geometry.index.array, 0, indices);
      at = this.basicValues(data, at, material, record.values);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MESH; input[e + 1] = record.id; input[e + 2] = vertices; input[e + 3] = indices; input[e + 4] = start; input[e + 5] = 0;
      this.keyOf(lightning.mesh, input, e + DRAW_KEY);
    }
    for (const record of traced) {
      if (draws === MAX_DRAWS) break;
      const { mesh, material } = record, geometry = mesh.geometry, start = at;
      const vertices = geometry.getAttribute("position").count, indices = geometry.index.count;
      for (const [name, size] of record.attributes) at = copy(data, at, geometry.getAttribute(name).array, 0, vertices * size);
      at = copyBits(bits, at, geometry.index.array, 0, indices);
      for (const [name] of record.values) at = put(data, at, material.uniforms[name].value);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MESH; input[e + 1] = record.id; input[e + 2] = vertices; input[e + 3] = indices; input[e + 4] = start; input[e + 5] = 0;
      this.keyOf(mesh, input, e + DRAW_KEY);
    }
    let models = 0;
    for (const record of glowing) {
      if (draws === MAX_DRAWS) break;
      const { mesh, material } = record, m = models++ * 16;
      matrices.set(mesh.matrixWorld.elements, m);
      const start = at;
      if (record.shader) {
        for (const [name] of record.values) at = put(data, at, material.uniforms[name].value);
      } else at = this.basicValues(data, at, material, record.values);
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MODEL; input[e + 1] = record.model; input[e + 2] = m; input[e + 3] = start; input[e + 4] = 0; input[e + 5] = 0;
      this.keyOf(mesh, input, e + DRAW_KEY);
    }
    input[IN_DRAWS] = draws;
    input[IN_OWNERS] = draws ? owners : 0;
    input[IN_SOFT] = particleDepth.enabled.value;
    input[IN_SOFT + 1] = particleDepth.resolution.value.x; input[IN_SOFT + 2] = particleDepth.resolution.value.y;
    input[IN_SOFT + 3] = particleDepth.cameraRange.value.x; input[IN_SOFT + 4] = particleDepth.cameraRange.value.y;
    input[IN_SOFT + 5] = depth;
    this.draws = draws;
    this.instances = instances;
  }

  // A basic material's values, as three's refreshUniformsCommon sets them,
  // into `data` from `at`; where they end.
  basicValues(data, at, material, values) {
    for (const [name] of values) {
      if (name === "diffuse") { data[at++] = material.color.r; data[at++] = material.color.g; data[at++] = material.color.b; }
      else if (name === "opacity") data[at++] = material.opacity;
      else if (name === "mapTransform") {
        const map = material.map;
        if (map?.matrixAutoUpdate) map.updateMatrix();
        data.set(map ? map.matrix.elements : [1, 0, 0, 0, 1, 0, 0, 0, 1], at);
        at += 9;
      }
    }
    return at;
  }

  // Off: three draws everything again, everything uploaded anew; on: it
  // comes back here, written in full the first frame.
  setActive(on) {
    on = on && !this.failed;
    this.active = on;
    for (const record of this.records) {
      const b = record.batch;
      this.hide(record, on);
      record.synced = false;
      if (on) continue;
      // Each attribute, or the interleaved buffer they share (the sniper's
      // trace), marked for three to upload whole.
      let shared = null;
      for (const [name] of record.attributes) {
        const a = b.geometry.getAttribute(name);
        if (a.isInterleavedBufferAttribute) {
          if (shared === a.data) continue;
          shared = a.data;
          shared.clearUpdateRanges();
          shared.addUpdateRange(0, b.count * shared.stride);
          shared.needsUpdate = true;
          continue;
        }
        a.clearUpdateRanges();
        a.addUpdateRange(0, b.count * a.itemSize);
        a.needsUpdate = true;
      }
    }
    const lightning = classicLightning.get(this.core.graphics.scene);
    if (lightning && !on && lightning.external) {
      lightning.external = false;
      lightning.mesh.visible = lightning.index > 0 && !lightning.isEnhanced();
      for (const name of ["position", "uv", "color"]) lightning.geometry.attributes[name].needsUpdate = true;
      lightning.geometry.index.needsUpdate = true;
    } else if (lightning && on && this.meshes.get(lightning.mesh.material)) {
      lightning.external = true;
      lightning.mesh.visible = false;
    }
    for (const record of this.glowList) record.material.visible = !on;
    for (const record of this.shaderMeshes.values()) if (record) record.material.visible = !on;
    if (!on) this.draws = this.instances = 0;
  }

  restore() {
    this.setActive(false);
  }
}

export function effectsPass(core) {
  return new EffectsPass(core);
}
