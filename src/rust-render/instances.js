// Rust draws bodies, held parts, skinned rigs and flying models through
// source-owned descriptors. Material changes publish new instance records.
import { actorProgram, actorFailure, claimActor, isActorMaterial, actorMap, trackActorMaps, refreshActorMaps, refreshActorMap } from "./actors.js";
import * as THREE from "three";
import { ownedEffectProgram } from "./effect-material.js";
import { ownsEffectSource } from "./programs.js";
import { blendOf, stateOf, sidesOf } from "./transparent.js";

function hasEffectSource(materials) {
  if (!Array.isArray(materials)) return ownsEffectSource(materials);
  for (const material of materials) if (ownsEffectSource(material)) return true;
  return false;
}

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
// the source-owned heat material uses them.
const NO_HEAT = { heat: 0, tear: new THREE.Vector4(0, 0, 0, 0.3), soot: 0, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };

// A record's material values from its row's ROW_EXTRA (shader.rs EXTRAS):
// its scroll, then a heated body's heat, base, scale and soot, its tear,
// its dissolve, from the object's presentation values.
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
// WebGLRenderer.compile supplies programs without issuing a first draw.
// Temporarily expose the requested shells, restore their authored materials,
// and let Rust registration import the linked sources and explicit bindings.
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
  return [].concat(material).some(m => {
    if(!m || isActorMaterial(m) || m.userData.bfeFamily)return false;
    const p=renderer.properties.get(m);return !p.currentProgram || p.__version!==undefined && p.__version!==m.version;
  });
}

export function materialVersion(materials) { let version=0;for(const material of materials)version+=material.version;return version; }

class InstancePass {
  constructor(core) {
    this.core = core;
    this.name = "instances";
    this.phase = "near";
    this.bodies = new WeakMap(); // root -> its body's record, or null
    this.heated = new WeakMap(); // a dissolving body -> its heated record, or null
    this.meshes = new WeakMap(); // a gun's mesh -> its record, or null
    this.rigList = null;
    // Scene additions must reach mirrors and the main view immediately.
    // Source-owned rigs cannot wait for a browser draw or the periodic scan.
    this.collectRig = object => {
      if (this.rigList && object.isSkinnedMesh && !object.userData.shadowOnly && !object.userData.poseSphere && object.name !== "body-shadow" && object.name !== "arc-shock-warm" && !Array.isArray(object.material) && !this.rigList.includes(object)) this.rigList.push(object);
      if(object.userData.flying)this.flyingPending?.add(object);
    };
    this.forgetFlying = object => {
      this.flyingPending?.delete(object);
      const record=this.flyingByMesh?.get(object);
      if(!record)return;
      object.layers.mask=record.layers;this.flyingByMesh.delete(object);
      const index=this.flyingRecords.indexOf(record);if(index>=0)this.flyingRecords.splice(index,1);
    };
    this.bodyRecords = [];
    this.meshRecords = [];
    this.frames = 0;
    // The phase camera's frustum, as three's render makes it to cull by.
    this.frustum = new THREE.Frustum();
    this.projScreen = new THREE.Matrix4();
    // The count three's render listed the scene at (writeBody).
    this.listed = -1;
    this.registerNewPart=object=>{claimActor(this.core, object, () => object.isSkinnedMesh ? this.registerBody(object) : this.registerMesh(object));};
  }

  // Before three lists the frame's own render of the near scene (the core's
  // watchScene): the soldiers' bodies and guns, the rigs and the flying parts
  // not seen yet, registered now and so hidden from three before it lists
  // them from their first frame. Registration links only owned descriptors.
  beforeSort(renderer, scene, camera, target, added) {
    const graphics=this.core.graphics;
    const mainFrame=camera===graphics.camera && target===graphics.sceneTarget;
    if(scene!==graphics.scene || !mainFrame && camera.userData?.mirror!==true)return;
    // The arc caster's shock stand-in (effects-owned) is prepared here too.
    this.standIn(renderer, scene, camera, added, mainFrame);
    for(const root of graphics.cullRoots)this.newParts(root,this.registerNewPart);
    if(graphics.playerRoot)this.newParts(graphics.playerRoot,this.registerNewPart);
    if(this.rigList)for(const mesh of this.rigList)if(!this.bodies.has(mesh))claimActor(this.core, mesh, () => this.registerRig(mesh));
    if(this.flyingRecords)this.registerPending();
  }

  // The arc caster's stand-in for a shock's glow (arc-lightning.js
  // warmShock: one bone, nothing drawn), which three draws for its first 90
  // frames so that the glow's program is built while the battle loads: that
  // program built by three without a draw and the renderer's made from it
  // now (as a struck body's glow will want them), and the stand-in kept from
  // three, its count of frames run here as three's draws would run it (it
  // takes itself away at the end of it).
  // New effects can first be visible in a mirror. Discover and compile their
  // stand-ins there too, but advance warm-up lifetime only on the main frame.
  // Otherwise a reflection can consume the stand-in before its first main view.
  standIn(renderer, scene, camera, added, advance = true) {
    for (const root of added || []) root.traverse((o) => { if (o.isSkinnedMesh && o.name === "arc-shock-warm") this.warm = o; });
    const warm = this.warm;
    if (!warm) return;
    if (!warm.parent || !warm.visible) { this.warm = null; return; }
    const material = warm.material;
    if (!this.warmProgram) {
      // The glow's program is the effects family's own: linked now, never by three.
      const programs = ownsEffectSource(material) ? sidesOf(material).map((side) => ownedEffectProgram(this.core, material, SKINNED, side)) : [];
      this.warmProgram = programs.length > 0 && programs.every(Boolean);
      if (!this.warmProgram) this.core.fail(new Error("Cannot prepare arc shock program"));
    }
    material.visible = false;
    // Register for reflected views too; retire the stand-in on main frames.
    if (advance) warm.onBeforeRender(renderer, scene, camera, warm.geometry, material, null);
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

  // Register a soldier's body and held guns before writing their first
  // Rust records; refresh records when their materials change.
  soldier(root, camera, at) {
    const parts = root.userData.parts;
    if (!parts || !root.visible) return at;
    const { body, hand } = parts;
    let record = this.bodies.get(body);
    if (record === undefined || record && record.version !== materialVersion(record.used)) record = claimActor(this.core, body, () => this.registerBody(body));
    if (record && body.material !== record.materials) {
      if(!body.userData.heated)record=claimActor(this.core, body, () => this.registerBody(body));
      else {
        let heated=this.heated.get(body);
        if(!heated || !sameMaterials(heated.materials,body.material)){heated=claimActor(this.core, body, () => this.registerBody(body,true));this.heated.set(body,heated);}
        record=heated;
      }
    }
    if (record && body.visible && camera.layers.test(body.layers) && this.inView(body)) at = this.writeBody(record, at);
    for (const group of hand?.children || []) {
      if (!group.visible || !group.isGroup) continue;
      for (const mesh of group.children) {
        if (!mesh.isMesh || !mesh.visible || !camera.layers.test(mesh.layers)) continue;
        let rec = this.meshes.get(mesh);
        if (rec === undefined || (rec && mesh.material!==rec.material && isActorMaterial(mesh.material))) rec = claimActor(this.core, mesh, () => this.registerMesh(mesh));
        if (rec && mesh.material === rec.material) {
          if (this.inView(mesh)) at = this.writeMesh(rec, at);
        } else if (rec) this.core.skip(mesh, mesh.material, "held part whose material no descriptor covers");
      }
    }
    return at;
  }

  // A body's record (or, `heated`, the record of the heated copies of its
  // materials it dissolves in), or null when the renderer can't draw it;
  // undefined while three has not drawn it as it is yet.
  registerBody(body, heated = false) {
    const core=this.core,all=[].concat(body.material),merged=core.groups(body.geometry);
    const used=[...new Set(merged.length?merged.map(g=>all[g.materialIndex]):[all[0]])];
    if(merged.length>4 || !used.every(isActorMaterial) || !('RIGID_SKIN' in used[0].defines))throw actorFailure(`Unsupported actor body ${body.name}`);
    const programs=used.map(m=>actorProgram(core,m,SKINNED));
    if(!programs.every(p=>p.id===programs[0].id))throw actorFailure(`Actor body programs differ: ${body.name}`);
    const program=programs[0],geometry=core.geometry(body.geometry,program);
    if(geometry===null || geometry===undefined)throw actorFailure(`Actor body geometry: ${body.name}`);
    const groups=merged.length?merged:[{materialIndex:0}],textures=groups.map(g=>actorMap(core,all[g.materialIndex]));
    const record={version:materialVersion(used),body,materials:body.material,used,program:program.id,geometry,textures,whole:!merged.length,look:used[0].uniforms,heated:heated || 'HEATED' in used[0].defines,lamps:Boolean(program.lamps)};
    trackActorMaps(record,groups.map(g=>all[g.materialIndex]));
    for(const material of used)material.visible=false;
    this.bodyRecords.push(record);
    if(!heated){
      const previous=this.bodies.get(body),index=this.bodyRecords.indexOf(previous);
      if(index>=0)this.bodyRecords.splice(index,1);
      this.bodies.set(body,record);
    }
    return record;
  }

  noteAdded(root) {
    if(this.rigList || this.flyingPending)root.traverse(this.collectRig);
  }

  noteRemoved(root) {
    if(this.flyingRecords)root.traverse(this.forgetFlying);
  }

  findRigs() {
    const found = [];
    this.core.graphics.scene.traverse((o) => { if (o.isSkinnedMesh && !o.userData.shadowOnly && !o.userData.poseSphere && o.name !== "body-shadow" && o.name !== "arc-shock-warm" && !Array.isArray(o.material)) found.push(o); });
    this.rigList = found;
  }

  rigs(camera, at) {
    const scene = this.core.graphics.scene;
    if (!this.rigList || this.frames % 120 === 0) this.findRigs();
    for (const mesh of this.rigList) {
      let rec = this.bodies.get(mesh);
      if (rec === undefined || rec && (mesh.material !== rec.materials || rec.version !== materialVersion(rec.used))) rec = claimActor(this.core, mesh, () => this.registerRig(mesh));
      if (!rec) continue;
      if (mesh.material !== rec.materials) {
        if(isActorMaterial(mesh.material) || ownsEffectSource(mesh.material))rec=claimActor(this.core, mesh, () => this.registerRig(mesh));
        else this.core.skip(mesh, mesh.material, "rig whose material no descriptor covers");
        if(!rec)continue;
      }
      if (!camera.layers.test(mesh.layers)) continue;
      // It and its parents shown, up to the scene (the walk ends there).
      let shown = true, o = mesh;
      for (; o && o !== scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o === scene && this.inView(mesh)) at = this.writeBody(rec, at);
    }
    return at;
  }

  registerRig(mesh) {
    const core=this.core,material=mesh.material;
    // A shock glow over a struck body is the effects family's (arc-lightning.js):
    // a blended rig drawn with its owned program.
    const effect=ownsEffectSource(material);
    if(!effect && (!isActorMaterial(material) || mesh.geometry.groups.length || !mesh.skeleton))throw actorFailure(`Unsupported actor rig ${mesh.name}`);
    let record;
    if(effect || material.transparent)record=this.registerBlended(mesh);
    else {
      const program=actorProgram(core,material,SKINNED),geometry=core.geometry(mesh.geometry,program),texture=actorMap(core,material);
      if(geometry===null || geometry===undefined || texture===undefined)throw actorFailure(`Actor rig resources: ${mesh.name}`);
      record={version:material.version,body:mesh,materials:material,used:[material],program:program.id,geometry,textures:[texture],whole:true,look:material.uniforms,heated:'HEATED' in material.defines,lamps:Boolean(program.lamps)};
      trackActorMaps(record,[material]);
      material.visible=false;this.bodyRecords.push(record);
    }
    if(!record)throw actorFailure(`Cannot register actor rig ${mesh.name}`);
    const previous=this.bodies.get(mesh),index=this.bodyRecords.indexOf(previous);
    if(index>=0)this.bodyRecords.splice(index,1);
    this.bodies.set(mesh,record);return record;
  }

  // A rig whose material blends: an item of the transparent list
  // (instances.rs KIND_BLENDED), drawn with its owned program for each side
  // three draws it with (back faces, then front, for a double-sided one),
  // its blending and depth as three sets them. Null when the renderer
  // can't draw it; undefined while its texture isn't uploaded yet.
  registerBlended(mesh) {
    const core = this.core, material = mesh.material;
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    if (!blend || !state) return null;
    const programs = sides.map((side) => ownsEffectSource(material) ? ownedEffectProgram(core, material, SKINNED, side) : actorProgram(core,material,SKINNED));
    if (programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) return null;
    const texture = actorMap(core, material);
    const geometry = core.geometry(mesh.geometry, programs[0]);
    if (geometry === null || geometry === undefined) return null;
    const blendId = core.rust.inst_add_blend(new Uint32Array(blend), state.flags & ~4, state.func, new Uint32Array(programs.flatMap((p, i) => [p.id, sides[i]])));
    const record = { version: material.version, body: mesh, materials: material, used: [material], program: programs[programs.length - 1].id, geometry, textures: [texture], whole: true, look: material.uniforms, heated: "HEATED" in material.defines, blend: blendId,
      lamps: programs.some((p) => p.lamps) };
    trackActorMaps(record,[material]);
    material.visible = false;
    this.bodyRecords.push(record);
    return record;
  }

  registerMesh(mesh) {
    if(isActorMaterial(mesh.material)) {
      const record=this.rigidRecord(mesh);
      if(!record)throw actorFailure(`Cannot register actor part ${mesh.name || mesh.material.name}`);
      mesh.material.visible=false;
      const previous=this.meshes.get(mesh),index=this.meshRecords.indexOf(previous);
      if(index>=0)this.meshRecords.splice(index,1);
      this.meshes.set(mesh,record);this.meshRecords.push(record);return record;
    }
    throw actorFailure(`Unowned held actor material: ${mesh.name}`);
  }


  // A body past the pass's record capacity is skipped and reported once, never a stop of the frame (a battle
  // with more actors in view than the pass holds draws the first ones).
  full(object, material, at, why) {
    this.core.skip(object, material, why);
    return at;
  }

  writeBody(record, at) {
    if (at.records >= MAX_RECORDS)return this.full(record.body, record.used[0], at, "actor instance capacity exceeded");
    const core = this.core;
    core.views();
    refreshActorMaps(core,record);core.views();
    const { body } = record, input = core.input, skeleton = body.skeleton;
    const bones = skeleton.bones.length;
    let boneAt = at.skeletons.get(skeleton);
    if (boneAt === undefined) {
      if (at.bones + bones > core.bones.length / 16)return this.full(body, record.used[0], at, "actor bone capacity exceeded");
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
    if (at.records >= MAX_RECORDS)return this.full(record.mesh, record.material, at, "actor instance capacity exceeded");
    const core = this.core;
    core.views();
    refreshActorMap(core,record);core.views();
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
    for (const record of this.flyingRecords || []) record.material.visible=on;
    this.revealFlying(!on);
  }

  // Flying models (game.js flyingModel: rounds, grenades, droids' pieces,
  // items), groups the game adds to the scene tagged userData.flying, their
  // parts drawn as rigid records, dynamic (lit) and unlit zero materials
  // alike: an unlit part's program reads no look values, so its record's go
  // unread. A part shares its material with every copy of its model, which
  // the renderer may not draw (one not yet registered, one inside another
  // object), so a part it draws is hidden from three by its layers (on
  // FLYING_LAYER), not by its material; its own layers stay in its record,
  // tested against the camera's as three would test them. A part whose
  // material blends is an item of the transparent list (registerFlying). A
  // part made with a heated material (the destruction's burning pieces)
  // takes its heat and look from the object. A part three gives another
  // material (destruction.js heats a wreck's) registers that material anew.
  flying(camera, at) {
    if (!this.flyingRecords) this.watchFlying();
    this.registerPending();
    const scene = this.core.graphics.scene, mask = camera.layers.mask;
    for (let i = 0; i < this.flyingRecords.length; i++) {
      const record = this.flyingRecords[i], mesh = record.mesh;
      if (mesh.material !== record.material || record.version !== mesh.material.version) {
        mesh.layers.mask = record.layers;
        this.flyingRecords.splice(i--, 1);
        if(isActorMaterial(mesh.material)) {
          this.flyingByMesh.set(mesh,claimActor(this.core, mesh, () => this.registerFlying(mesh)));
        }else this.core.skip(mesh, mesh.material, "flying part whose material no descriptor covers");
        continue;
      }
      if (!(record.layers & mask)) continue;
      let shown = true, o = mesh;
      for (; o && o !== scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o === scene && this.inView(mesh)) at = this.writeMesh(record, at);
    }
    return at;
  }

  // The flying groups added and not all registered yet: their parts
  // registered before their first visible frame.
  registerPending() {
    for (const group of this.flyingPending) {
      group.traverse((o) => {
        if (!o.isMesh || this.flyingByMesh.has(o)) return;
        this.flyingByMesh.set(o, claimActor(this.core, o, () => this.registerFlying(o)));
      });
      this.flyingPending.delete(group);
    }
  }

  // The scene's flying groups, now and as the game adds and removes them: a
  // group taken out of the scene releases its records and original layers.
  watchFlying() {
    const scene = this.core.graphics.scene;
    this.flyingRecords = [];
    this.flyingByMesh = new WeakMap(); // part -> its owned record
    this.flyingBlends ||= new WeakMap(); // a blending part's material -> its blend (inst_add_blend) and programs
    this.flyingPending = new Set();
    scene.traverse(this.collectRig);
  }

  // A flying part uses a source-owned rigid record from its first frame.
  registerFlying(mesh) {
    const record=this.rigidRecord(mesh);
    if(!record)throw actorFailure(`Cannot register actor debris ${mesh.name}`);
    mesh.layers.set(FLYING_LAYER);this.flyingRecords.push(record);
    return record;
  }

  // Held parts and flying pieces share the same rigid input contract.
  rigidRecord(mesh) {
    const core=this.core,material=mesh.material;
    const defines=material?.defines || {};
    if(mesh.isSkinnedMesh || mesh.isInstancedMesh || Array.isArray(material) || !isActorMaterial(material) || mesh.geometry.groups.length || !("DYNAMIC" in defines || "UNLIT" in defines))throw actorFailure(`Unsupported lit model part ${mesh.name}`);
    // One that blends: drawn with three's program for each side three draws
    // it with, its blending and depth as three sets them (one blend for all
    // the copies of a part, which share its material).
    let blend, programs;
    if (material.transparent) {
      const sides = sidesOf(material), state = stateOf(material), how = blendOf(material);
      if (!how || !state) return null;
      programs = sides.map(() => actorProgram(core,material,RIGID));
      if (programs.some((p) => p.attributes.join() !== programs[0].attributes.join())) return null;
      const key = programs.map((p, i) => `${p.id}:${sides[i]}`).join();
      this.flyingBlends ||= new WeakMap();
      let held = this.flyingBlends.get(material);
      if (!held || held.key !== key) {
        held = { key, blend: core.rust.inst_add_blend(new Uint32Array(how), state.flags & ~4, state.func, new Uint32Array(programs.flatMap((p, i) => [p.id, sides[i]]))) };
        this.flyingBlends.set(material, held);
      }
      blend = held.blend;
    } else programs = [actorProgram(core,material,RIGID)];
    const program = programs[programs.length - 1];
    const geometry = program && core.geometry(mesh.geometry, programs[0]);
    const texture = actorMap(core, material);
    if (geometry === null || geometry === undefined) return null;
    if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
    if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
    // A heated copy of a part's material (destruction-debris.js
    // heatMaterial: the destruction's burning pieces) takes its heat and
    // look from the object, as its onBeforeRender sets them.
    const record = { version: material.version, mesh, material, program: program.id, geometry, texture, layers: mesh.layers.mask, blend, heated: "HEATED" in defines, lamps: programs.some((p) => p.lamps) };
    return record;
  }

  // On: the flying parts hidden from three again; off: back to their layers.
  revealFlying(on) {
    for (const record of this.flyingRecords || []) {
      if (on) record.mesh.layers.set(FLYING_LAYER);
      else record.mesh.layers.mask = record.layers;
    }
  }


}

export function instancesPass(core) {
  return new InstancePass(core);
}
