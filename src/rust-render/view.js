// The view pass (view.rs): the first-person view scene (graphics.viewScene,
// drawn after a depth clear with graphics.viewCamera): the arms' rig and the
// weapon in their hand (firstperson.js), each part a rigid mesh written as a
// record the renderer draws instanced, read with the view camera's frame
// block (VIEW_FIRST_PERSON). Material shells carry values and owned program
// keys. The browser never compiles an actor material.
import { actorProgram, actorFailure, claimActor, isActorMaterial, actorMap, refreshActorMap } from "./actors.js";
import * as THREE from "three";
import { compileFor, unbuilt } from "./instances.js";
import { ownedEffectProgram } from "./effect-material.js";
import { ownsEffectSource } from "./programs.js";
import { arcBatches } from "../arc-lightning.js";
import { plainProgram, writeUniform } from "./post.js";
import { blendOf, sidesOf, stateOf } from "./transparent.js";

// instances.rs's kinds: a rigid part, and one that blends (its draw its own:
// view.rs, from its row).
const RIGID = 2, BLENDED_RIGID = 4;
// view.rs's blended draws.
const D = { PROGRAM: 0, GEOMETRY: 1, BLEND: 2, DEPTH_FUNC: 9, FLAGS: 10, SIDES: 11, RANGE: 14, UNIFORMS: 16, FIRST: 17 };
// A draw with this program is an effects batch drawn in view order: it reuses the
// geometry, range and uniform-count slots as batch id, first instance, count and
// capacity (view.rs VIEW_BATCH).
const VIEW_BATCH = -2, B = { ID: D.GEOMETRY, AT: D.RANGE, COUNT: D.RANGE + 1, CAPACITY: D.UNIFORMS };
const UNIFORM = 18, MAX_DRAW_UNIFORMS = 32, DRAW = D.FIRST + MAX_DRAW_UNIFORMS * UNIFORM, MAX_DRAWS = 16;
const viewProjection = new THREE.Matrix4(), point = new THREE.Vector4(), frustum = new THREE.Frustum();
// three's painterSortStable (opaque) and reversePainterSortStable (what blends).
const opaqueOrder = (a, b) => a.groupOrder - b.groupOrder || a.renderOrder - b.renderOrder || a.material - b.material || a.z - b.z || a.id - b.id;
const transparentOrder = (a, b) => a.groupOrder - b.groupOrder || a.renderOrder - b.renderOrder || b.z - a.z || a.id - b.id;
// Record layout (instances.rs REC_*).
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, GROUPS: 3, TEXTURES: 4, BONES: 8, SPHERE: 10, BOX: 14, WORLD: 20, BIND_INVERSE: 36, LOOK: 52, FLAGS: 56, EXTRA: 76, BLEND: 96 };
const MAX_RECORDS = 256;
const flagsOf = (m) => m.side | (m.alphaToCoverage ? 4 : 0) | (m.depthWrite ? 8 : 0);

class ViewPass {
  constructor(core) {
    this.core = core;
    this.name = "view";
    this.phase = "view";
    this.parts = new WeakMap(); // mesh -> registered record
    this.records = [];
    // Whether the last phase drew everything three would have of the scene.
    this.complete = false;
    // This frame's blended draws, in three's transparent order.
    this.blends = [];
    this.opaques = [];
    this.input = false;
    // This frame's parts written that blend (their order among the records
    // that blend: view.rs draws the k-th).
    this.blending = 0;
  }

  // First-person geometry has its own camera and depth ordering. Preserve that
  // view block instead of treating the held gun as another world-space actor.
  // All visits must be claimed by Rust before the completed view is composited.
  draw(ctx) {

    this.complete = true;
    this.blends.length = 0;
    this.opaques.length = 0;
    this.blending = 0;
    const camera = ctx.camera;
    viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    frustum.setFromProjectionMatrix(viewProjection);
    this.visit(ctx.scene, camera, ctx, 0);
    this.writeBlends(camera);
  }

  // projectObject's walk: a hidden object hides its subtree; one on other
  // layers is not drawn, but its children may be; a group's render order
  // orders what blends below it.
  visit(object, camera, ctx, groupOrder) {
    if (!object.visible) return;
    if (object.isGroup) groupOrder = object.renderOrder;
    if (object.layers.test(camera.layers)) {
      if (object.isMesh) this.part(object, ctx, groupOrder);
      else if (object.isSprite || object.isPoints || object.isLine || object.isLOD) this.core.skip(object, object.material, "view object no pass owns");
    }
    const children = object.children;
    for (let i = 0; i < children.length; i++) this.visit(children[i], camera, ctx, groupOrder);
  }

  part(mesh, ctx, groupOrder) {
    let record = this.parts.get(mesh);
    if(record && mesh.material!==record.material && !isActorMaterial(mesh.material) && !ownsEffectSource(mesh.material))return this.core.skip(mesh, mesh.material, "view part whose material no descriptor covers");
    if(record===undefined || record===null && isActorMaterial(mesh.material) || record && (mesh.material!==record.material || record.version!==mesh.material.version)) {
      if(!isActorMaterial(mesh.material) && ![].concat(mesh.material).every(ownsEffectSource) && unbuilt(this.core.renderer,mesh.material))compileFor(this.core.renderer,[[mesh,mesh.material]],ctx.camera,ctx.scene);
      const previous=record;record=claimActor(this.core, mesh, () => this.registerPart(mesh));
      if(previous){const at=this.records.indexOf(previous);if(at>=0)this.records.splice(at,1);}
    }
    if (record && mesh.material === record.material) {
      if (record.rigid) {
        record.k = -1;
        if (!mesh.frustumCulled || frustum.intersectsObject(mesh)) { this.write(record, ctx); if (record.k >= 0) this.blend(record, groupOrder); }
      }
      else if (record.blended) this.blend(record, groupOrder);
      else this.write(record, ctx);
    } else if (record) this.core.skip(mesh, mesh.material, "view part whose material no descriptor covers");
    else if ([].concat(mesh.material).some((m) => m?.visible)) this.core.skip(mesh, mesh.material, "view mesh no pass owns");
  }

  // A rigid part with a zero material that does not blend (instances.js
  // registerMesh's rule for a gun), or a source-owned blended muzzle flash.
  registerPart(mesh) {
    const core = this.core;
    let record = null;
    const material = mesh.material;
    if (material?.userData?.bfeProgram === "effects/glow-basic") return this.registerBlended(mesh);
    if (material?.userData?.bfeProgram === "effects/arc" && arcBatches.has(mesh)) {
      const batch = core.byName.effects.viewBatch(mesh);
      record = { mesh, material, blended: true, fx: batch };
      this.parts.set(mesh, record); this.records.push(record);
      return record;
    }
    // A zero material: an actor part with an owned rigid program.
    // View effects retain their own family/pass; they are not actor variants.
    if (!material.userData.actorShader) {
      this.parts.set(mesh,null);return null;
    }
    if (mesh.geometry.groups.length || mesh.isSkinnedMesh || mesh.isInstancedMesh) throw actorFailure(`Unsupported view geometry: ${mesh.name || mesh.type}`);
    if (material.transparent) {
      record = this.registerRigidBlend(mesh);
      this.parts.set(mesh, record);
      return record;
    }
    const program = actorProgram(core, material, RIGID, true);
    const geometry = core.geometry(mesh.geometry, program);
    const texture = actorMap(core,material);
    if (geometry === null || geometry === undefined)throw actorFailure(`View actor resources: ${mesh.name}`);
    {
      if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
      if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
      record = { version: material.version, mesh, material, program: program.id, geometry, texture, lamps: program.lamps };
      material.visible = false;
      this.records.push(record);
    }
    this.parts.set(mesh, record);
    return record;
  }

  // A source-owned per-object program for a blended muzzle flash.
  registerBlended(mesh) {
    const core = this.core, material = mesh.material, geometry = mesh.geometry;
    let record = null;
    const program = material.userData?.bfeProgram === "effects/glow-basic"
      ? ownedEffectProgram(core, material, 3, material.side === THREE.DoubleSide ? THREE.FrontSide : material.side, false)
      : plainProgram(core, material, null);
    if (program === undefined) return undefined; // until three has made it
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    const why = !program ? "program" : program.uniforms.length > MAX_DRAW_UNIFORMS ? `${program.uniforms.length} uniforms` : !blend || !state ? "state"
      : material.onBeforeCompile !== THREE.Material.prototype.onBeforeCompile ? "shader hook" : !geometry.index || geometry.groups.length ? "geometry"
      : mesh.isSkinnedMesh || mesh.isInstancedMesh ? "kind" : null;
    if (why) throw actorFailure(`view ${material.name || material.type}: ${why}`);
    else {
      const g = core.geometry(geometry, { id: program.id, attributes: program.attributes });
      if (g === null || g === undefined)throw actorFailure(`View flash geometry: ${mesh.name}`);
      {
        if (!geometry.boundingSphere) geometry.computeBoundingSphere();
        record = { version: material.version, blended: true, mesh, material, program, geometry: g, blend, state, sides };
        material.visible = false;
        this.records.push(record);
      }
    }
    this.parts.set(mesh, record);
    return record;
  }

  // A rigid part whose zero material blends: a record drawn from its row
  // (instances.rs KIND_BLENDED_RIGID) with an owned program for each side
  // three draws it with, its blending and depth as three sets them, at its
  // place in three's transparent order among what the view blends (view.rs).
  // Unsupported owned resources stop registration.
  registerRigidBlend(mesh) {
    const core = this.core, material = mesh.material;
    const how = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    if (!how || !state)throw actorFailure(`View actor blend state: ${mesh.name}`);
    const programs = sides.map(() => actorProgram(core, material, RIGID, true));
    if (programs.some((p) => p.attributes.join() !== programs[0].attributes.join()))throw actorFailure(`View actor attribute layout: ${mesh.name}`);
    const geometry = core.geometry(mesh.geometry, programs[0]);
    const texture = actorMap(core,material);
    if (geometry === null || geometry === undefined)throw actorFailure(`View actor blend resources: ${mesh.name}`);
    if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
    if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
    const blend = core.rust.inst_add_blend(new Uint32Array(how), state.flags & ~4, state.func, new Uint32Array(programs.flatMap((p, i) => [p.id, sides[i]])));
    const record = { version: material.version, mesh, material, program: programs[programs.length - 1].id, geometry, texture, lamps: programs.some((p) => p.lamps), rigid: true, blend: how, blendId: blend, k: -1 };
    material.visible = false;
    this.records.push(record);
    return record;
  }

  // A blended mesh three would list this frame, with three's sort key: z of
  // its bounding sphere's centre through its world matrix and the view.
  blend(record, groupOrder) {
    const mesh = record.mesh;
    if (mesh.frustumCulled && !frustum.intersectsObject(mesh)) return;
    if (this.blends.length + this.opaques.length >= MAX_DRAWS)return this.core.skip(mesh, record.material, "view draw capacity exceeded");
    point.copy(mesh.geometry.boundingSphere.center).applyMatrix4(mesh.matrixWorld).applyMatrix4(viewProjection);
    (record.material.transparent ? this.blends : this.opaques).push({ record, groupOrder, renderOrder: mesh.renderOrder, material: record.material.id, z: point.z, id: mesh.id });
  }

  // The frame's draws for view.rs: the opaque ones in three's opaque order,
  // then what blends in its transparent order.
  // Publish current weapon transforms and sort keys after their programs and
  // samplers exist. A first-use texture upload may grow WASM memory, so the
  // shared views must be current before writing any records.
  writeBlends(camera) {
    const core = this.core;
    if (!this.input) {
      if (!this.blends.length && !this.opaques.length) return;
      core.addView("viewInput", Float64Array, () => core.rust.view_input_ptr(), () => core.rust.view_input_len());
      this.input = true;
    }
    // Written into a buffer of the page's first: a uniform's texture registered
    // on the way (writeUniform: core.texture) can grow the renderer's memory,
    // which detaches every view of it; the view is taken once all is written.
    const input = this.scratch ||= new Float64Array(1 + MAX_DRAWS * DRAW);
    input[0] = 0;
    this.opaques.sort(opaqueOrder);
    this.blends.sort(transparentOrder);
    let n = 0, fxAt = 0;
    for (const { record } of [...this.opaques, ...this.blends]) {
      const o = 1 + n * DRAW;
      if (record.fx) {
        input[o + D.PROGRAM] = VIEW_BATCH; input[o + B.ID] = record.fx.id;
        input[o + B.AT] = fxAt; input[o + B.COUNT] = record.fx.batch.count;
        input[o + B.CAPACITY] = record.fx.batch.capacity;
        fxAt = core.byName.effects.writeViewBatch(record.fx, camera, fxAt);
        n++; continue;
      }
      // A part that blends drawn from its record's row: the k-th of those written.
      if (record.rigid) {
        if (record.k < 0) continue;
        input[o + D.PROGRAM] = -1; input[o + D.GEOMETRY] = record.k;
        n++;
        continue;
      }
      const { mesh, material, program } = record, geometry = mesh.geometry;
      const range = geometry.drawRange, count = geometry.index.count;
      const first = Math.max(0, range.start), last = Math.min(count, range.start + range.count);
      if (last <= first) continue;
      if (material.userData?.bfeShape !== undefined) mesh.onBeforeRender(core.renderer, core.graphics.viewScene, camera, geometry, material, null);
      input[o + D.PROGRAM] = program.id; input[o + D.GEOMETRY] = record.geometry;
      for (let k = 0; k < 7; k++) input[o + D.BLEND + k] = record.blend[k];
      input[o + D.DEPTH_FUNC] = record.state.func; input[o + D.FLAGS] = record.state.flags;
      input[o + D.SIDES] = record.sides.length; input[o + D.SIDES + 1] = record.sides[0]; input[o + D.SIDES + 2] = record.sides[1] ?? 0;
      input[o + D.RANGE] = first; input[o + D.RANGE + 1] = last - first;
      input[o + D.UNIFORMS] = program.uniforms.length;
      // Built-in view materials store maps as fields, shader shells in
      // uniforms. Publish both before writing their scalar draw record.
      for(const uniform of program.uniforms)if(uniform.type>=32){
        const texture=material.uniforms?.[uniform.name]?.value ?? material[uniform.name];
        if(texture?.isTexture)core.texture(texture);
      }
      let failed = null;
      for (let k = 0; k < program.uniforms.length && !failed; k++) if (!writeUniform(core, input, o + D.FIRST + k * UNIFORM, k, program.uniforms[k], material, mesh, camera)) failed = program.uniforms[k].name;
      if (failed) { core.skip(record.mesh, material, `no value for ${failed}`); continue; }
      n++;
    }
    input[0] = n;
    core.views();
    core.viewInput.set(input.subarray(0, 1 + n * DRAW));
  }

  write(record, at) {
    if (at.records >= MAX_RECORDS)return this.core.skip(record.mesh, record.material, "view instance capacity exceeded");
    const core = this.core;
    core.views();
    refreshActorMap(core,record);core.views();
    const { mesh, material } = record, input = core.input, geometry = mesh.geometry;
    const o = core.recordsAt + at.records * core.stride;
    input[o + R.KIND] = record.rigid ? BLENDED_RIGID : RIGID; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    if (record.rigid) {
      record.k = this.blending++;
      input[o + R.BLEND] = record.blendId; input[o + R.BLEND + 1] = 0; input[o + R.BLEND + 2] = mesh.renderOrder; input[o + R.BLEND + 3] = mesh.id;
    }
    input[o + R.GROUPS] = 0; input[o + R.TEXTURES] = record.texture; input[o + R.BONES] = 0;
    const s = geometry.boundingSphere, b = geometry.boundingBox;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    input[o + R.BOX] = b.min.x; input[o + R.BOX + 1] = b.min.y; input[o + R.BOX + 2] = b.min.z; input[o + R.BOX + 3] = b.max.x; input[o + R.BOX + 4] = b.max.y; input[o + R.BOX + 5] = b.max.z;
    const w = mesh.matrixWorld.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = 0; }
    const u = material.uniforms;
    input[o + R.LOOK] = u.sunScale.value; input[o + R.LOOK + 1] = u.ambientPull.value.x; input[o + R.LOOK + 2] = u.ambientPull.value.y; input[o + R.LOOK + 3] = u.brightness.value;
    input[o + R.FLAGS] = flagsOf(material);
    for (let k = 0; k < 20; k++) input[o + R.EXTRA + k] = 0;
    input[o + R.EXTRA] = u.scroll?.value.x || 0;
    input[o + R.EXTRA + 1] = u.scroll?.value.y || 0;
    if (record.lamps) {
      // The view model's lamps, selected for this part.
      const lamps = core.lamps(mesh, true);
      input[o + R.EXTRA + 13] = lamps.count;
      for (let k = 0; k < 4; k++) input[o + R.EXTRA + 16 + k] = lamps.indices[k];
    }
    at.records++;
  }


  // Shown to three while one of three's own renders needs them (index.js
  // reveal): the temporal upscaler's motion pass draws the view scene again,
  // with an override material, for the mask that keeps it fixed on screen.
  // Visibility here only coordinates compiler/resource inspection. It does not
  // select another drawer; the guarded battle renderer remains required.
  reveal(on) {
    for (const record of this.records) record.material.visible = on;
  }

}

export function viewPass(core) {
  return new ViewPass(core);
}
