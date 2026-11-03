// The instance pass (instances.rs): every visible soldier's body and the
// guns in his hand, and behind ?rustRigs the scene's other skinned meshes
// (vehicles' and props' rigs), each a record the renderer draws instanced
// in the near phase, after the world; and the flying models (rounds,
// grenades, droids' pieces, items: flying()). Each is registered the first time
// three has drawn it (which gives its material a program), then hidden from
// three. A dying droideka's body dissolves in heated copies of its
// materials (units.js dissolve): a record of its own, its heat and dissolve
// its row's (writeExtras); a body in other materials of its own goes back
// to three.
//
// Only what three's render lists by the camera's frustum is written, culled
// here as three culls it (the renderer culls the records again by the same
// spheres): a battle's other soldiers and rigs cost the page nothing. A
// written body's bones are the ones three worked out as it listed the body
// in this render (or drew its shadow), as its own draw of it would read
// them; worked out here only when three has not (writeBody).
import * as THREE from "three";

const SKINNED = 1, RIGID = 2;
// The layer a flying model's part goes on while the renderer draws it: no
// camera renders it (graphics.js uses 1, 2 and 4, sky-probe.js 3, water.js 5).
const FLYING_LAYER = 30;
// Record layout (instances.rs REC_*).
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, GROUPS: 3, TEXTURES: 4, BONES: 8, BONE_AT: 9, SPHERE: 10, BOX: 14, WORLD: 20, BIND_INVERSE: 36, LOOK: 52, FLAGS: 56, BIND: 60, EXTRA: 76 };
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
// EXTRAS): a scrolling texture's speed, and a heated copy's (destruction-
// debris.js heatMaterial).
const EXTRAS = ["scroll", "heat", "heatTear", "heatBase", "heatScale", "soot", "dissolve"];
const INSTANCE = ["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix", ...LOOK, "zeroLights", "zeroLightCount", ...EXTRAS];
const LAMPS = ["localLightPositions", "localLightColors", "localLightCount"];
const policy = (samplers) => ({ uniforms: new Set([...samplers, ...INSTANCE, ...LAMPS]), material: LOOK });
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

// Whether two of a body's materials (one, or an array of them) are the same.
function sameMaterials(a, b) {
  if (a === b) return true;
  return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((m, i) => m === b[i]);
}

// A record's material values from its row's ROW_EXTRA (shader.rs EXTRAS):
// its scroll, then a heated body's heat, base, scale and soot, its tear,
// its dissolve (heatMaterial's onBeforeRender's values from the object).
function writeExtras(input, o, uniforms, heated) {
  const e = o + R.EXTRA, scroll = uniforms?.scroll?.value;
  input[e] = scroll ? scroll.x : 0; input[e + 1] = scroll ? scroll.y : 0; input[e + 2] = 0; input[e + 3] = 0;
  if (heated) {
    input[e + 4] = heated.heat; input[e + 5] = heated.base || 0; input[e + 6] = heated.scale || 1; input[e + 7] = heated.soot;
    const t = heated.tear;
    input[e + 8] = t.x; input[e + 9] = t.y; input[e + 10] = t.z; input[e + 11] = t.w;
    input[e + 12] = heated.dissolve;
  } else for (let k = 4; k < 13; k++) input[e + k] = 0;
  for (let k = 13; k < 20; k++) input[e + k] = 0;
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

class InstancePass {
  constructor(core) {
    this.core = core;
    this.name = "instances";
    this.phase = "near";
    this.bodies = new WeakMap(); // root -> its body's record, or null
    this.heated = new WeakMap(); // a dissolving body -> its heated record, or null
    this.meshes = new WeakMap(); // a gun's mesh -> its record, or null
    // ?rustRigs: vehicles' and props' skinned meshes too.
    this.rigsOn = new URLSearchParams(location.search).has("rustRigs");
    this.rigList = null;
    this.bodyRecords = [];
    this.meshRecords = [];
    this.frames = 0;
    // The phase camera's frustum, as three's render makes it to cull by.
    this.frustum = new THREE.Frustum();
    this.projScreen = new THREE.Matrix4();
    // The count three's render listed the scene at (writeBody).
    this.listed = -1;
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
    if (this.rigsOn) this.rigs(camera, ctx);
    // An error with the flying models gives them back to three for good.
    if (!this.flyingFailed) {
      try { this.flying(camera, ctx); } catch (error) {
        this.core.errors.push(`flying models failed: ${String(error?.stack || error).slice(0, 400)}`);
        this.flyingFailed = true;
        try { this.restoreFlying(); } catch {}
      }
    }
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
    if (record === undefined) record = this.registerBody(body);
    if (record && body.material !== record.materials) {
      // Dissolving (units.js dissolve): its heated record, registered once
      // three has drawn the heated materials; any other materials of its
      // own: three draws them.
      let heated = body.userData.heated ? this.heated.get(body) : null;
      if (heated !== null && (heated === undefined || !sameMaterials(heated.materials, body.material))) {
        heated = this.registerBody(body, true);
        if (heated !== undefined) this.heated.set(body, heated);
      }
      if (heated) record = heated;
      else { for (const m of [].concat(body.material)) m.visible = true; record = null; }
    }
    if (record && body.visible && camera.layers.test(body.layers) && this.inView(body)) at = this.writeBody(record, at);
    for (const group of hand?.children || []) {
      if (!group.visible || !group.isGroup) continue;
      for (const mesh of group.children) {
        if (!mesh.isMesh || !mesh.visible || !camera.layers.test(mesh.layers)) continue;
        let rec = this.meshes.get(mesh);
        if (rec === undefined) rec = this.registerMesh(mesh);
        if (rec && mesh.material === rec.material) {
          if (this.inView(mesh)) at = this.writeMesh(rec, at);
        } else if (rec) mesh.material.visible = true;
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
        record = { body, materials: body.material, used, program: program.id, geometry, textures, whole: !merged.length, look: used[0].uniforms, heated };
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
    this.core.graphics.scene.traverse((o) => { if (o.isSkinnedMesh && !o.userData.poseSphere && o.name !== "body-shadow" && !Array.isArray(o.material)) found.push(o); });
    this.rigList = found;
  }

  rigs(camera, at) {
    const scene = this.core.graphics.scene;
    if (!this.rigList || this.frames % 120 === 0) this.findRigs();
    for (const mesh of this.rigList) {
      let rec = this.bodies.get(mesh);
      if (rec === undefined) rec = this.registerRig(mesh);
      if (!rec) continue;
      if (mesh.material !== rec.materials) { mesh.material.visible = true; continue; }
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
    const material = mesh.material, threeProgram = core.renderer.properties.get(material).currentProgram;
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && mesh.skeleton) {
      const program = core.program(threeProgram, SKINNED, material, POLICIES[SKINNED]);
      const geometry = program && core.geometry(mesh.geometry, program);
      const texture = mapOf(core, material);
      if (texture === undefined) return undefined; // its texture first
      if (geometry !== null && geometry !== undefined) {
        record = { body: mesh, materials: material, used: [material], program: program.id, geometry, textures: [texture], whole: true, look: material.uniforms };
        material.visible = false;
        this.bodyRecords.push(record);
      }
    }
    if (threeProgram || record) this.bodies.set(mesh, record);
    return record;
  }

  registerMesh(mesh) {
    const core = this.core;
    let record = null;
    const material = mesh.material, threeProgram = !Array.isArray(material) && core.renderer.properties.get(material).currentProgram;
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && "DYNAMIC" in (material.defines || {})) {
      const program = core.program(threeProgram, RIGID, material, POLICIES[RIGID]);
      const geometry = program && core.geometry(mesh.geometry, program);
      const texture = mapOf(core, material);
      if (texture === undefined) return undefined; // its texture first
      if (geometry !== null && geometry !== undefined) {
        if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
        if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
        record = { mesh, material, program: program.id, geometry, texture };
        material.visible = false;
        this.meshRecords.push(record);
      }
    }
    if (threeProgram || record) this.meshes.set(mesh, record);
    return record;
  }

  writeBody(record, at) {
    if (at.records >= MAX_RECORDS) return at;
    const core = this.core;
    core.views();
    const { body } = record, input = core.input, skeleton = body.skeleton;
    const bones = skeleton.bones.length;
    let boneAt = at.skeletons.get(skeleton);
    if (boneAt === undefined) {
      if (at.bones + bones > core.bones.length / 16) return at;
      // Its bones as three worked them out in this render: as it listed the
      // body (at the count before this one) or drew a shadow (at this one).
      stamp(skeleton, core.renderer.info);
      if (!(updatedAt.get(skeleton) >= this.listed)) skeleton.update();
      core.bones.set(skeleton.boneMatrices, at.bones * 16);
      boneAt = at.bones; at.skeletons.set(skeleton, boneAt); at.bones += bones;
    }
    const o = core.recordsAt + at.records * core.stride;
    input[o + R.KIND] = SKINNED; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    input[o + R.GROUPS] = record.whole ? 0 : record.textures.length;
    for (let g = 0; g < record.textures.length; g++) input[o + R.TEXTURES + g] = record.textures[g];
    input[o + R.BONES] = bones; input[o + R.BONE_AT] = boneAt;
    const s = body.boundingSphere;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    const w = body.matrixWorld.elements, b = body.bindMatrixInverse.elements, bind = body.bindMatrix.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = b[k]; input[o + R.BIND + k] = bind[k]; }
    // A heated body's look is its own (heatMaterial's onBeforeRender sets it
    // from the object), as are its heat and dissolve.
    const heated = record.heated ? body.userData.heated : null;
    if (heated) { input[o + R.LOOK] = heated.sun; input[o + R.LOOK + 1] = heated.darken; input[o + R.LOOK + 2] = heated.lighten; input[o + R.LOOK + 3] = heated.brightness; }
    else { const look = record.look; input[o + R.LOOK] = look.sunScale.value; input[o + R.LOOK + 1] = look.ambientPull.value.x; input[o + R.LOOK + 2] = look.ambientPull.value.y; input[o + R.LOOK + 3] = look.brightness.value; }
    writeExtras(input, o, record.used[0].uniforms, heated);
    input[o + R.FLAGS] = flagsOf(record.used[0]);
    at.records++;
    return at;
  }

  writeMesh(record, at) {
    if (at.records >= MAX_RECORDS) return at;
    const core = this.core;
    core.views();
    const { mesh, material } = record, input = core.input, geometry = mesh.geometry;
    const o = core.recordsAt + at.records * core.stride;
    input[o + R.KIND] = RIGID; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    input[o + R.GROUPS] = 0; input[o + R.TEXTURES] = record.texture; input[o + R.BONES] = 0;
    const s = geometry.boundingSphere, b = geometry.boundingBox;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    input[o + R.BOX] = b.min.x; input[o + R.BOX + 1] = b.min.y; input[o + R.BOX + 2] = b.min.z; input[o + R.BOX + 3] = b.max.x; input[o + R.BOX + 4] = b.max.y; input[o + R.BOX + 5] = b.max.z;
    const w = mesh.matrixWorld.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = 0; }
    const u = material.uniforms;
    input[o + R.LOOK] = u.sunScale.value; input[o + R.LOOK + 1] = u.ambientPull.value.x; input[o + R.LOOK + 2] = u.ambientPull.value.y; input[o + R.LOOK + 3] = u.brightness.value;
    writeExtras(input, o, u, null);
    input[o + R.FLAGS] = flagsOf(material);
    at.records++;
    return at;
  }

  setActive(on) {
    const set = (m) => { if (m) m.visible = !on; };
    for (const record of this.bodyRecords) for (const m of record.used) set(m);
    for (const record of this.meshRecords) set(record.material);
    this.flyingActive(on);
  }

  // Flying models (game.js flyingModel: rounds, grenades, droids' pieces,
  // items), groups the game adds to the scene tagged userData.flying, their
  // parts drawn as rigid records, dynamic (lit) and unlit zero materials
  // alike: an unlit part's program reads no look values, so its record's go
  // unread. A part shares its material with every copy of its model, which
  // the renderer may not draw (one not yet registered, one inside another
  // object), so a part it draws is hidden from three by its layers (on
  // FLYING_LAYER), not by its material; its own layers stay in its record,
  // tested against the camera's as three would test them. A part three
  // gives another material (destruction.js heats a wreck's) goes back to
  // three.
  flying(camera, at) {
    if (!this.flyingRecords) this.watchFlying();
    if (this.flyingPending.size) {
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
    const scene = this.core.graphics.scene, mask = camera.layers.mask;
    for (let i = 0; i < this.flyingRecords.length; i++) {
      const record = this.flyingRecords[i], mesh = record.mesh;
      if (mesh.material !== record.material) {
        mesh.layers.mask = record.layers;
        this.flyingByMesh.set(mesh, null);
        this.flyingRecords.splice(i--, 1);
        continue;
      }
      if (!(record.layers & mask)) continue;
      let shown = true, o = mesh;
      for (; o && o !== scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o === scene && this.inView(mesh)) at = this.writeMesh(record, at);
    }
    return at;
  }

  // The scene's flying groups, now and as the game adds and removes them: a
  // group taken out of the scene has its parts given back to three.
  watchFlying() {
    const scene = this.core.graphics.scene;
    this.flyingRecords = [];
    this.flyingByMesh = new WeakMap(); // part -> its record, or null (three draws it)
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
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (!threeProgram) return undefined;
    const defines = material.defines || {};
    if (material.transparent || mesh.geometry.groups.length || !("DYNAMIC" in defines || "UNLIT" in defines)) return null;
    const program = core.program(threeProgram, RIGID, material, POLICIES[RIGID]);
    const geometry = program && core.geometry(mesh.geometry, program);
    const texture = mapOf(core, material);
    if (texture === undefined) return undefined; // its texture first
    if (geometry === null || geometry === undefined) return null;
    if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
    if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
    const record = { mesh, material, program: program.id, geometry, texture, layers: mesh.layers.mask };
    if (this.core.active) mesh.layers.set(FLYING_LAYER);
    this.flyingRecords.push(record);
    return record;
  }

  // On: the flying parts hidden from three again; off: back to their layers.
  flyingActive(on) {
    for (const record of this.flyingRecords || []) {
      if (on) record.mesh.layers.set(FLYING_LAYER);
      else record.mesh.layers.mask = record.layers;
    }
  }

  // Everything three may draw again, for good: the flying parts' layers.
  restore() {
    this.restoreFlying();
  }

  restoreFlying() {
    this.flyingActive(false);
    this.flyingRecords = [];
    this.flyingPending?.clear();
    this.flyingFailed = true;
  }
}

export function instancesPass(core) {
  return new InstancePass(core);
}
