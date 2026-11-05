// Rust draws soldiers, held guns, vehicle and prop rigs, and flying pieces.
// New programs are compiled before registration. Dissolves, shock overlays and
// blended rigid parts publish their values and ordered transparent records.
import * as THREE from "three";
import { blendOf, stateOf, sidesOf, programForSide } from "./transparent.js";

const SKINNED = 1, RIGID = 2, BLENDED = 3, BLENDED_RIGID = 4;
// The layer a flying model's part goes on while the renderer draws it: no
// camera renders it (graphics.js uses 1, 2 and 4, sky-probe.js 3, water.js 5).
const FLYING_LAYER = 30;
// Record layout (instances.rs REC_*).
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, GROUPS: 3, TEXTURES: 4, BONES: 8, BONE_AT: 9, SPHERE: 10, BOX: 14, WORLD: 20, BIND_INVERSE: 36, LOOK: 52, FLAGS: 56, BIND: 60, EXTRA: 76, BLEND: 96 };
const MAX_RECORDS = 256;
// A record's state flags (instances.rs REC_FLAGS): its material's side, then
// alpha to coverage and depth write.
const flagsOf = (m) => m.side | (m.alphaToCoverage ? 4 : 0) | (m.depthWrite ? 8 : 0);
// The material values these programs read, each instance's from its row
// (writeBody, writeMesh).
const LOOK = new Set(["sunScale", "ambientPull", "brightness"]);
// Uniforms each kind of program may read besides the frame block: its
// textures, what the renderer gives each instance from its row (matrices,
// look, point lights), and lamp lighting's, read only while lamps are off
// (each soldier's four nearest lamps would be per instance otherwise).
// A material's own values an instance gives from its row (shader.rs
// EXTRAS): a scrolling texture's speed, a heated copy's (destruction-
// debris.js heatMaterial), and the arc caster's shock glow's (arc-
// lightning.js shockMaterial: in the heated values' place, an overlay being
// never heated).
const SHOCK = ["shockTime", "intensity", "seed", "inflate", "color"];
const EXTRAS = ["scroll", "heat", "heatTear", "heatBase", "heatScale", "soot", "dissolve", ...SHOCK];
const INSTANCE = ["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix", ...LOOK, "zeroLights", "zeroLightCount", ...EXTRAS];
const LAMPS = ["localLightPositions", "localLightColors", "localLightCount"];
// shadows, lamps: this pass gives its draws their receiveShadow and lamps
// from their rows (writeExtras), and reveals what it hides to three's
// shadow pass (reveal), so programs reading them are its (core.program).
const policy = (samplers) => ({ uniforms: new Set([...samplers, ...INSTANCE, ...LAMPS]), material: LOOK, shadows: true, lamps: true });
const POLICIES = { [SKINNED]: policy(["map", "farMap", "boneTexture"]), [RIGID]: policy(["map", "farMap"]) };
// A material's map as the renderer's texture: -1 for none (the renderer
// binds what three's empty texture gives), undefined while it isn't
// uploaded yet.
function mapOf(core, material) {
  const map = material?.uniforms?.map?.value ?? null;
  if (!map) return -1;
  const id = core.texture(map);
  return id >= 0 ? id : undefined;
}

// The render order of the nearest Group above an object, as three's
// projectObject passes it down (0 without one).
function groupOrderOf(object) {
  for (let o = object.parent; o; o = o.parent) if (o.isGroup) return o.renderOrder;
  return 0;
}

// Whether two of a body's materials (one, or an array of them) are the same.
function sameMaterials(a, b) {
  if (a === b) return true;
  return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((m, i) => m === b[i]);
}

// A heated material's values for an object without a heat of its own, as
// destruction-debris.js heatMaterial's onBeforeRender gives them.
const NO_HEAT = { heat: 0, tear: new THREE.Vector4(0, 0, 0, 0.3), soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };

// A record's material values from its row's ROW_EXTRA (shader.rs EXTRAS):
// its scroll, then a heated body's heat, base, scale and soot, its tear,
// its dissolve (heatMaterial's onBeforeRender's values from the object).
function writeExtras(input, o, uniforms, heated, object, lamps) {
  const e = o + R.EXTRA, scroll = uniforms?.scroll?.value;
  input[e] = scroll ? scroll.x : 0; input[e + 1] = scroll ? scroll.y : 0; input[e + 2] = 0; input[e + 3] = 0;
  if (heated) {
    input[e + 4] = heated.heat; input[e + 5] = heated.base || 0; input[e + 6] = heated.scale || 1; input[e + 7] = heated.soot;
    const t = heated.tear;
    input[e + 8] = t.x; input[e + 9] = t.y; input[e + 10] = t.z; input[e + 11] = t.w;
    input[e + 12] = heated.dissolve;
  } else if (uniforms?.shockTime) {
    // A shock glow's clock, intensity, seed and inflation, then its colour.
    input[e + 4] = uniforms.shockTime.value; input[e + 5] = uniforms.intensity.value; input[e + 6] = uniforms.seed.value; input[e + 7] = uniforms.inflate.value;
    const c = uniforms.color.value;
    input[e + 8] = c.r; input[e + 9] = c.g; input[e + 10] = c.b; input[e + 11] = 0; input[e + 12] = 0;
  } else for (let k = 4; k < 13; k++) input[e + k] = 0;
  // Its lamps (their count, then four lamp table indices), when its program
  // reads them, as three's onBeforeRender selects them; whether it receives
  // shadows.
  input[e + 13] = lamps ? lamps.count : 0;
  input[e + 14] = object?.receiveShadow ? 1 : 0;
  input[e + 15] = 0;
  for (let k = 0; k < 4; k++) input[e + 16 + k] = lamps ? lamps.indices[k] : -1;
}

// three works out a skeleton's bones as its render lists a skinned mesh of
// it or draws one's shadow (WebGLObjects.update: once for each count of its
// renders, which it counts on as it has listed the scene): the count each
// skeleton's bones were last worked out at, kept by its update.
const updatedAt = new WeakMap();
function stamp(skeleton, info) {
  if (updatedAt.has(skeleton)) return;
  // Its own update if it has one, else its class's as it is when called (a
  // wrapper of the class's sees three's calls too).
  const own = Object.prototype.hasOwnProperty.call(skeleton, "update") ? skeleton.update : null;
  skeleton.update = function () {
    if (own) own.call(this);
    else Object.getPrototypeOf(this).update.call(this);
    updatedAt.set(this, info.render.frame);
  };
  updatedAt.set(skeleton, -1);
}

// three's programs for meshes' materials, built or found without a draw
// (renderer.compile), for the scene and camera three is about to draw them
// for: what a first draw would have given them. entries: [mesh, materials]
// (those of its materials it draws with: a body's array also holds its
// guns', never drawn on it), the mesh holding them alone while compiled.
export function compileFor(renderer, entries, camera, scene) {
  const each = (fn) => { for (const [mesh] of entries) fn(mesh); };
  const held = entries.map(([mesh, materials]) => { const own = mesh.material; mesh.material = materials; return own; });
  try {
    renderer.compile({ traverse: each, traverseVisible: each }, camera, scene);
  } finally {
    entries.forEach(([mesh], i) => { mesh.material = held[i]; });
  }
}

// Whether three has no program for a material (or for one of a mesh's) yet.
export function unbuilt(renderer, material) {
  return [].concat(material).some((m) => { const p=m && renderer.properties.get(m); return p && (!p.currentProgram || p.__version !== undefined && p.__version !== m.version); });
}

export function materialVersion(materials) {
  let version=0; for(const m of materials)version+=m.version; return version;
}

class InstancePass {
  constructor(core) {
    this.core = core;
    this.name = "instances";
    this.phase = "near";
    this.bodies = new WeakMap(); // root -> its body's record, or null
    this.heated = new WeakMap(); // a dissolving body -> its heated record, or null
    this.meshes = new WeakMap(); // a gun's mesh -> its record, or null
    this.rigList = null;
    this.bodyRecords = [];
    this.meshRecords = [];
    this.frames = 0;
    // The phase camera's frustum, as three's render makes it to cull by.
    this.frustum = new THREE.Frustum();
    this.projScreen = new THREE.Matrix4();
    // The count three's render listed the scene at (writeBody).
    this.listed = -1;
    // What beforeSort found new this frame.
    this.fresh = [];
  }

  // Before three lists the frame's own render of the near scene (the core's
  // watchScene): the soldiers' bodies and guns, the rigs and the flying parts
  // not seen yet, registered now and so hidden from three before it lists
  // them (the renderer draws them from their first frame), their materials'
  // programs built by three first where it has none yet (compileFor).
  beforeSort(renderer, scene, camera, target, added) {
    const graphics = this.core.graphics;
    if (scene !== graphics.scene || camera !== graphics.camera || target !== graphics.sceneTarget) return;
    try {
      this.standIn(renderer, scene, camera, added);
      const fresh = this.fresh, roots = graphics.playerRoot ? [...graphics.cullRoots, graphics.playerRoot] : graphics.cullRoots;
      fresh.length = 0;
      const add = (mesh, materials) => { if (unbuilt(renderer, materials)) fresh.push([mesh, materials]); };
      for (const root of roots) this.newParts(root, (o) => add(o, o.isSkinnedMesh ? this.usedOf(o) : o.material));
      if (this.rigList) for (const mesh of this.rigList) if (!this.bodies.has(mesh)) add(mesh, mesh.material);
      if (this.flyingPending?.size) for (const group of this.flyingPending) group.traverse((o) => { if (o.isMesh && !this.flyingByMesh.has(o)) add(o, o.material); });
      if (fresh.length) compileFor(renderer, fresh, camera, scene);
      fresh.length = 0;
      for (const root of roots) this.newParts(root, (o) => { if (o.isSkinnedMesh) this.registerBody(o); else this.registerMesh(o); });
      if (this.rigList) for (const mesh of this.rigList) if (!this.bodies.has(mesh)) this.registerRig(mesh);
      if (this.flyingRecords) this.registerPending();
    } catch (error) {
      this.core.fail(error);
    }
  }

  // The arc caster's stand-in for a shock's glow (arc-lightning.js
  // warmShock: one bone, nothing drawn), which three draws for its first 90
  // frames so that the glow's program is built while the battle loads: that
  // program built by three without a draw and the renderer's made from it
  // now (as a struck body's glow will want them), and the stand-in kept from
  // three, its count of frames run here as three's draws would run it (it
  // takes itself away at the end of it).
  standIn(renderer, scene, camera, added) {
    for (const root of added || []) root.traverse((o) => { if (o.isSkinnedMesh && o.name === "arc-shock-warm") this.warm = o; });
    const warm = this.warm;
    if (!warm) return;
    if (!warm.parent || !warm.visible) { if (warm.material) warm.material.visible = true; this.warm = null; return; }
    const material = warm.material;
    if (!this.warmProgram) {
      if (unbuilt(renderer, material)) compileFor(renderer, [[warm, material]], camera, scene);
      const programs = sidesOf(material).map((side) => { const p = programForSide(renderer, material, side); return p && this.core.program(p, SKINNED, material, POLICIES[SKINNED]); });
      this.warmProgram = programs.length > 0 && programs.every(Boolean);
      if (!this.warmProgram) this.core.fail(new Error("Cannot prepare arc shock program"));
    }
    material.visible = false;
    warm.onBeforeRender(renderer, scene, camera, warm.geometry, material, null);
  }

  // The materials a body's groups draw with (its groups merged as the
  // renderer draws them).
  usedOf(body) {
    const all = [].concat(body.material), merged = this.core.groups(body.geometry);
    return [...new Set(merged.length ? merged.map((g) => all[g.materialIndex]) : [all[0]])];
  }

  // A shown soldier's body and guns not registered yet (as soldier() would
  // meet them), each to fn.
  newParts(root, fn) {
    const parts = root.userData.parts;
    if (!parts || !root.visible) return;
    const { body, hand } = parts;
    if (body && !this.bodies.has(body)) fn(body);
    for (const group of hand?.children || []) {
      if (!group.visible || !group.isGroup) continue;
      for (const mesh of group.children) if (mesh.isMesh && mesh.visible && !this.meshes.has(mesh)) fn(mesh);
    }
  }

  // ctx: the phase's camera, and what has been written to the renderer's
  // input so far (records, bones, skeletons).
  draw(ctx) {

    const { camera } = ctx, graphics = this.core.graphics;
    this.projScreen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.projScreen);
    // The phase runs in three's render of the scene, which counted on once
    // it had listed it (a renderer that counts none: every body's bones
    // worked out here).
    const count = ctx.renderer.info?.render.frame;
    this.listed = count === undefined ? Infinity : count - 1;
    for (const root of graphics.cullRoots) this.soldier(root, camera, ctx);
    if (graphics.playerRoot) this.soldier(graphics.playerRoot, camera, ctx);
    this.rigs(camera, ctx);
    this.flying(camera, ctx);
    this.frames++;
  }

  // Whether three's render lists an object by the camera's frustum, as its
  // projectObject culls (one it does not cull always).
  inView(object) {
    return !object.frustumCulled || this.frustum.intersectsObject(object);
  }

  // A soldier's body and the guns in his hand as records; registers them
  // (and hides them from three) the first time each is seen drawn by three.
  soldier(root, camera, at) {
    const parts = root.userData.parts;
    if (!parts || !root.visible) return at;
    const { body, hand } = parts;
    let record = this.bodies.get(body);
    if (record === undefined || record && record.version !== materialVersion(record.used)) record = this.registerBody(body);
    if (record && body.material !== record.materials) {
      // Dissolving (units.js dissolve): register the heated material before
      // emitting the changed body's Rust record.
      let heated = body.userData.heated ? this.heated.get(body) : this.registerBody(body);
      if (heated !== null && (heated === undefined || !sameMaterials(heated.materials, body.material))) {
        heated = this.registerBody(body, true);
        if (heated !== undefined) this.heated.set(body, heated);
      }
      if (heated) record = heated;
      else this.core.fail(new Error("Cannot register changed body material"));
    }
    if (record && body.visible && camera.layers.test(body.layers) && this.inView(body)) at = this.writeBody(record, at);
    for (const group of hand?.children || []) {
      if (!group.visible || !group.isGroup) continue;
      for (const mesh of group.children) {
        if (!mesh.isMesh || !mesh.visible || !camera.layers.test(mesh.layers)) continue;
        let rec = this.meshes.get(mesh);
        if (rec === undefined || rec && (mesh.material !== rec.material || rec.version !== mesh.material.version)) rec = this.registerMesh(mesh);
        if (rec && mesh.material === rec.material) {
          if (this.inView(mesh)) at = this.writeMesh(rec, at);
        } else if (rec) this.core.fail(new Error("Cannot register changed rigid material"));
      }
    }
    return at;
  }

  // A body's record (or, `heated`, the record of the heated copies of its
  // materials it dissolves in), or null when the renderer can't draw it;
  // undefined while three has not drawn it as it is yet.
  registerBody(body, heated = false) {
    const core = this.core;
    let record = null;
    // The materials its groups draw with: the array is the actor's own
    // (actor.js), to which combat.js adds the guns' materials too.
    const all = [].concat(body.material);
    const merged = core.groups(body.geometry);
    const used = [...new Set(merged.length ? merged.map((g) => all[g.materialIndex]) : [all[0]])];
    if (unbuilt(core.renderer, used)) compileFor(core.renderer, [[body, used]], core.graphics.camera, core.graphics.scene);
    const programs = used.map((m) => m && core.renderer.properties.get(m).currentProgram);
    const why = !programs[0] ? "" : !programs.every((p) => p === programs[0]) ? "programs differ" : merged.length > 4 ? "groups" : !("RIGID_SKIN" in (used[0].defines || {})) ? "not rigid" : null;
    if (why) core.errors.push(`body: ${why}`);
    if (programs[0] && why === null) {
      const program = core.program(programs[0], SKINNED, used[0], POLICIES[SKINNED]);
      const geometry = program && core.geometry(body.geometry, program);
      if (geometry !== null && geometry !== undefined) {
        const groups = merged.length ? merged : [{ materialIndex: 0 }];
        const textures = groups.map((g) => mapOf(core, all[g.materialIndex]));
        if (textures.some((t) => t === undefined)) return undefined; // its textures first
        record = { version: materialVersion(used), body, materials: body.material, used, program: program.id, geometry, textures, whole: !merged.length, look: used[0].uniforms, heated, lamps: Boolean(program.lamps) };
        for (const m of used) m.visible = false;
        this.bodyRecords.push(record);
      }
    }
    if (heated) return programs[0] || record ? record : undefined;
    if (programs[0] || record) this.bodies.set(body, record);
    return record;
  }

  // The scene's other skinned meshes (vehicle-rig.js: hulls, walkers,
  // animated props), looked for again now and then; soldiers' bodies are
  // found through their roots.
  findRigs() {
    const found = [];
    // Shadow-only meshes belong to the shadow pass, not colour rigs. The
    // arc caster's warm-up stand-in has its own program-registration path.
    this.core.graphics.scene.traverse((o) => { if (o.isSkinnedMesh && !o.userData.poseSphere && o.name !== "body-shadow" && o.name !== "crowd-shadows" && o.name !== "arc-shock-warm" && !Array.isArray(o.material)) found.push(o); });
    this.rigList = found;
  }

  rigs(camera, at) {
    const scene = this.core.graphics.scene;
    if (!this.rigList || this.frames % 120 === 0) this.findRigs();
    for (const mesh of this.rigList) {
      let rec = this.bodies.get(mesh);
      if (rec === undefined || rec && (mesh.material !== rec.materials || rec.version !== materialVersion(rec.used))) rec = this.registerRig(mesh);
      if (!rec) continue;
      if (mesh.material !== rec.materials) this.core.fail(new Error("Cannot register changed rig material"));
      if (!camera.layers.test(mesh.layers)) continue;
      // It and its parents shown, up to the scene (the walk ends there).
      let shown = true, o = mesh;
      for (; o && o !== scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o === scene && this.inView(mesh)) at = this.writeBody(rec, at);
    }
    return at;
  }

  registerRig(mesh) {
    const core = this.core;
    let record = null;
    const material = mesh.material;
    if (unbuilt(core.renderer, material)) compileFor(core.renderer, [[mesh, material]], core.graphics.camera, core.graphics.scene);
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (threeProgram && material.transparent && !mesh.geometry.groups.length && mesh.skeleton) {
      record = this.registerBlended(mesh);
      if (record === undefined) return undefined;
    } else if (threeProgram && !material.transparent && !mesh.geometry.groups.length && mesh.skeleton) {
      const program = core.program(threeProgram, SKINNED, material, POLICIES[SKINNED]);
      const geometry = program && core.geometry(mesh.geometry, program);
      const texture = mapOf(core, material);
      if (texture === undefined) return undefined; // its texture first
      if (geometry !== null && geometry !== undefined) {
        record = { version: material.version, body: mesh, materials: material, used: [material], program: program.id, geometry, textures: [texture], whole: true, look: material.uniforms, lamps: Boolean(program.lamps) };
        material.visible = false;
        this.bodyRecords.push(record);
      }
    }
    if (threeProgram || record) this.bodies.set(mesh, record);
    return record;
  }

  // A rig whose material blends: an item of the transparent list
  // (instances.rs KIND_BLENDED), drawn with three's program for each side
  // three draws it with (back faces, then front, for a double-sided one),
  // its blending and depth as three sets them. Null when the renderer
  // can't draw it; undefined while its texture isn't uploaded yet.
  registerBlended(mesh) {
    const core = this.core, renderer = core.renderer, material = mesh.material;
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    if (!blend || !state) return null;
    const programs = sides.map((side) => { const p = programForSide(renderer, material, side); return p && core.program(p, SKINNED, material, POLICIES[SKINNED]); });
    if (programs.some((p) => !p) || programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) return null;
    const texture = mapOf(core, material);
    if (texture === undefined) return undefined;
    const geometry = core.geometry(mesh.geometry, programs[0]);
    if (geometry === null || geometry === undefined) return null;
    const blendId = core.rust.inst_add_blend(new Uint32Array(blend), state.flags & ~4, state.func, new Uint32Array(programs.flatMap((p, i) => [p.id, sides[i]])));
    const record = { version: material.version, body: mesh, materials: material, used: [material], program: programs[programs.length - 1].id, geometry, textures: [texture], whole: true, look: material.uniforms, blend: blendId,
      lamps: programs.some((p) => p.lamps) };
    material.visible = false;
    this.bodyRecords.push(record);
    return record;
  }

  registerMesh(mesh) {
    const core = this.core;
    let record = null;
    const material = mesh.material;
    if (unbuilt(core.renderer, material)) compileFor(core.renderer, [[mesh, material]], core.graphics.camera, core.graphics.scene);
    const threeProgram = !Array.isArray(material) && core.renderer.properties.get(material).currentProgram;
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && "DYNAMIC" in (material.defines || {})) {
      const program = core.program(threeProgram, RIGID, material, POLICIES[RIGID]);
      const geometry = program && core.geometry(mesh.geometry, program);
      const texture = mapOf(core, material);
      if (texture === undefined) return undefined; // its texture first
      if (geometry !== null && geometry !== undefined) {
        if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
        if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
        record = { version: material.version, mesh, material, program: program.id, geometry, texture, lamps: Boolean(program.lamps) };
        material.visible = false;
        this.meshRecords.push(record);
      }
    }
    if (threeProgram || record) this.meshes.set(mesh, record);
    return record;
  }

  writeBody(record, at) {
    if (at.records >= MAX_RECORDS) this.core.fail(new Error("Too many instance records"));
    const core = this.core;
    core.views();
    const { body } = record, input = core.input, skeleton = body.skeleton;
    const bones = skeleton.bones.length;
    let boneAt = at.skeletons.get(skeleton);
    if (boneAt === undefined) {
      if (at.bones + bones > core.bones.length / 16) core.fail(new Error("Too many instance bones"));
      // Its bones as three worked them out in this render: as it listed the
      // body (at the count before this one) or drew a shadow (at this one).
      stamp(skeleton, core.renderer.info);
      if (!(updatedAt.get(skeleton) >= this.listed)) skeleton.update();
      core.bones.set(skeleton.boneMatrices, at.bones * 16);
      boneAt = at.bones; at.skeletons.set(skeleton, boneAt); at.bones += bones;
    }
    const o = core.recordsAt + at.records * core.stride;
    input[o + R.KIND] = record.blend === undefined ? SKINNED : BLENDED; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    input[o + R.GROUPS] = record.whole ? 0 : record.textures.length;
    // A blending rig's blend and three's key for it (but z, the renderer's).
    if (record.blend !== undefined) { input[o + R.BLEND] = record.blend; input[o + R.BLEND + 1] = groupOrderOf(body); input[o + R.BLEND + 2] = body.renderOrder; input[o + R.BLEND + 3] = body.id; }
    for (let g = 0; g < record.textures.length; g++) input[o + R.TEXTURES + g] = record.textures[g];
    input[o + R.BONES] = bones; input[o + R.BONE_AT] = boneAt;
    if (!body.boundingSphere) body.computeBoundingSphere();
    const s = body.boundingSphere;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    const w = body.matrixWorld.elements, b = body.bindMatrixInverse.elements, bind = body.bindMatrix.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = b[k]; input[o + R.BIND + k] = bind[k]; }
    // A heated body's look is its own (heatMaterial's onBeforeRender sets it
    // from the object), as are its heat and dissolve.
    const heated = record.heated ? body.userData.heated : null;
    if (heated) { input[o + R.LOOK] = heated.sun; input[o + R.LOOK + 1] = heated.darken; input[o + R.LOOK + 2] = heated.lighten; input[o + R.LOOK + 3] = heated.brightness; }
    else if (record.look.sunScale) { const look = record.look; input[o + R.LOOK] = look.sunScale.value; input[o + R.LOOK + 1] = look.ambientPull.value.x; input[o + R.LOOK + 2] = look.ambientPull.value.y; input[o + R.LOOK + 3] = look.brightness.value; }
    // A material with no look of its own (a shock glow's): what three's
    // defaults would be, unread by its program.
    else { input[o + R.LOOK] = 1; input[o + R.LOOK + 1] = 0; input[o + R.LOOK + 2] = 0; input[o + R.LOOK + 3] = 1; }
    writeExtras(input, o, record.used[0].uniforms, heated, body, record.lamps ? core.lamps(body, false) : null);
    input[o + R.FLAGS] = flagsOf(record.used[0]);
    at.records++;
    return at;
  }

  writeMesh(record, at) {
    if (at.records >= MAX_RECORDS) this.core.fail(new Error("Too many instance records"));
    const core = this.core;
    core.views();
    const { mesh, material } = record, input = core.input, geometry = mesh.geometry;
    const o = core.recordsAt + at.records * core.stride;
    input[o + R.KIND] = record.blend === undefined ? RIGID : BLENDED_RIGID; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    input[o + R.GROUPS] = 0; input[o + R.TEXTURES] = record.texture; input[o + R.BONES] = 0;
    // A blending part's blend and three's key for it (but z, the renderer's).
    if (record.blend !== undefined) { input[o + R.BLEND] = record.blend; input[o + R.BLEND + 1] = groupOrderOf(mesh); input[o + R.BLEND + 2] = mesh.renderOrder; input[o + R.BLEND + 3] = mesh.id; }
    const s = geometry.boundingSphere, b = geometry.boundingBox;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    input[o + R.BOX] = b.min.x; input[o + R.BOX + 1] = b.min.y; input[o + R.BOX + 2] = b.min.z; input[o + R.BOX + 3] = b.max.x; input[o + R.BOX + 4] = b.max.y; input[o + R.BOX + 5] = b.max.z;
    const w = mesh.matrixWorld.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = 0; }
    const u = material.uniforms, heated = record.heated ? mesh.userData.heated || NO_HEAT : null;
    if (heated) { input[o + R.LOOK] = heated.sun; input[o + R.LOOK + 1] = heated.darken; input[o + R.LOOK + 2] = heated.lighten; input[o + R.LOOK + 3] = heated.brightness; }
    else { input[o + R.LOOK] = u.sunScale.value; input[o + R.LOOK + 1] = u.ambientPull.value.x; input[o + R.LOOK + 2] = u.ambientPull.value.y; input[o + R.LOOK + 3] = u.brightness.value; }
    writeExtras(input, o, u, heated, mesh, record.lamps ? core.lamps(mesh, false) : null);
    input[o + R.FLAGS] = flagsOf(material);
    at.records++;
    return at;
  }


  // Reveal resource objects for the Rust shadow/motion ownership walks,
  // then hide them from the colour walk again.
  reveal(on) {

    for (const record of this.bodyRecords) for (const m of record.used) if (m) m.visible = on;
    for (const record of this.meshRecords) if (record.material) record.material.visible = on;
    this.revealFlying(!on);
  }

  flying(camera, at) {
    if (!this.flyingRecords) this.watchFlying();
    this.registerPending();
    const scene = this.core.graphics.scene, mask = camera.layers.mask;
    for (let i = 0; i < this.flyingRecords.length; i++) {
      const record = this.flyingRecords[i], mesh = record.mesh;
      if (mesh.material !== record.material || record.version !== mesh.material.version) {
        mesh.layers.mask = record.layers;
        this.flyingRecords.splice(i--, 1);
        const replacement=this.registerFlying(mesh);
        if (!replacement) this.core.fail(new Error(`Unregistered flying material: ${mesh.name}`));
        this.flyingByMesh.set(mesh,replacement);
        continue;
      }
      if (!(record.layers & mask)) continue;
      let shown = true, o = mesh;
      for (; o && o !== scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o === scene && this.inView(mesh)) at = this.writeMesh(record, at);
    }
    return at;
  }

  registerPending() {
    for (const group of this.flyingPending) {
      let waiting = false;
      group.traverse((o) => {
        if (!o.isMesh || this.flyingByMesh.has(o)) return;
        const record = this.registerFlying(o);
        if (record === undefined) waiting = true;
        else this.flyingByMesh.set(o, record);
      });
      if (!waiting) this.flyingPending.delete(group);
    }
  }

  watchFlying() {
    const scene = this.core.graphics.scene;
    this.flyingRecords = [];
    this.flyingByMesh = new WeakMap(); // part -> its record, or null (unregistered)
    this.flyingBlends = new WeakMap(); // a blending part's material -> its blend (inst_add_blend) and programs
    this.flyingPending = new Set(scene.children.filter((o) => o.userData?.flying));
    scene.addEventListener("childadded", ({ child }) => { if (child.userData?.flying) this.flyingPending.add(child); });
    scene.addEventListener("childremoved", ({ child }) => {
      if (!child.userData?.flying) return;
      this.flyingPending.delete(child);
      child.traverse((o) => {
        const record = this.flyingByMesh.get(o);
        if (!record) return;
        o.layers.mask = record.layers;
        this.flyingByMesh.delete(o);
        const i = this.flyingRecords.indexOf(record);
        if (i >= 0) this.flyingRecords.splice(i, 1);
      });
    });
  }

  // A flying part's rigid record, or null when the renderer can't draw it;
  // undefined while three has drawn no copy of its model yet.
  registerFlying(mesh) {
    const core = this.core, material = mesh.material;
    if (mesh.isSkinnedMesh || mesh.isInstancedMesh || Array.isArray(material) || !material?.isShaderMaterial) return null;
    if (unbuilt(core.renderer, material)) compileFor(core.renderer, [[mesh, material]], core.graphics.camera, core.graphics.scene);
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (!threeProgram) return undefined;
    const defines = material.defines || {};
    if (mesh.geometry.groups.length || !("DYNAMIC" in defines || "UNLIT" in defines)) return null;
    // One that blends: drawn with three's program for each side three draws
    // it with, its blending and depth as three sets them (one blend for all
    // the copies of a part, which share its material).
    let blend, programs;
    if (material.transparent) {
      const sides = sidesOf(material), state = stateOf(material), how = blendOf(material);
      if (!how || !state) return null;
      programs = sides.map((side) => { const p = programForSide(core.renderer, material, side); return p && core.program(p, RIGID, material, POLICIES[RIGID]); });
      if (programs.some((p) => !p) || programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) return null;
      const key = programs.map((p, i) => `${p.id}:${sides[i]}`).join();
      let held = this.flyingBlends.get(material);
      if (!held || held.key !== key) {
        held = { key, blend: core.rust.inst_add_blend(new Uint32Array(how), state.flags & ~4, state.func, new Uint32Array(programs.flatMap((p, i) => [p.id, sides[i]]))) };
        this.flyingBlends.set(material, held);
      }
      blend = held.blend;
    } else programs = [core.program(threeProgram, RIGID, material, POLICIES[RIGID])];
    const program = programs[programs.length - 1];
    const geometry = program && core.geometry(mesh.geometry, programs[0]);
    const texture = mapOf(core, material);
    if (texture === undefined) return undefined; // its texture first
    if (geometry === null || geometry === undefined) return null;
    if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
    if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
    // A heated copy of a part's material (destruction-debris.js
    // heatMaterial: the destruction's burning pieces) takes its heat and
    // look from the object, as its onBeforeRender sets them.
    const record = { version: material.version, mesh, material, program: program.id, geometry, texture, layers: mesh.layers.mask, blend, heated: "HEATED" in defines, lamps: programs.some((p) => p?.lamps) };
    mesh.layers.set(FLYING_LAYER);
    this.flyingRecords.push(record);
    return record;
  }

  // On: the flying parts hidden from three again; off: back to their layers.
  revealFlying(on) {
    for (const record of this.flyingRecords || []) {
      if (on) record.mesh.layers.set(FLYING_LAYER);
      else record.mesh.layers.mask = record.layers;
    }
  }

  // Everything three may draw again, for good: the flying parts' layers.

}

export function instancesPass(core) {
  return new InstancePass(core);
}
