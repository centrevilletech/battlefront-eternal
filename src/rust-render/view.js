// The view pass (view.rs): the first-person view scene (graphics.viewScene,
// drawn after a depth clear with graphics.viewCamera): the arms' rig and the
// weapon in their hand (firstperson.js), each part a rigid mesh written as a
// record the renderer draws instanced, read with the view camera's frame
// block (VIEW_FIRST_PERSON). The scene is walked as three's render walks it
// (visible objects, the camera's layers); a part is registered the first
// time three has a program for its material, then hidden from three.
//
// The phase is drawn in place of three's render of the scene (core.cover):
// when every object three would draw is a part drawn here (complete), three
// does not render the scene; otherwise (a muzzle flash, which blends, or
// anything else not registered) three renders it after, drawing only what
// is left.
import * as THREE from "three";
import { plainProgram, writeUniform } from "./post.js";
import { blendOf, sidesOf, stateOf } from "./transparent.js";

const RIGID = 2;
// view.rs's blended draws.
const D = { PROGRAM: 0, GEOMETRY: 1, BLEND: 2, DEPTH_FUNC: 9, FLAGS: 10, SIDES: 11, RANGE: 14, UNIFORMS: 16, FIRST: 17 };
const UNIFORM = 18, MAX_DRAW_UNIFORMS = 32, DRAW = D.FIRST + MAX_DRAW_UNIFORMS * UNIFORM, MAX_DRAWS = 16;
const viewProjection = new THREE.Matrix4(), point = new THREE.Vector4(), frustum = new THREE.Frustum();
// three's painterSortStable (opaque) and reversePainterSortStable (what blends).
const opaqueOrder = (a, b) => a.groupOrder - b.groupOrder || a.renderOrder - b.renderOrder || a.material - b.material || a.z - b.z || a.id - b.id;
const transparentOrder = (a, b) => a.groupOrder - b.groupOrder || a.renderOrder - b.renderOrder || b.z - a.z || a.id - b.id;
// Record layout (instances.rs REC_*).
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, GROUPS: 3, TEXTURES: 4, BONES: 8, SPHERE: 10, BOX: 14, WORLD: 20, BIND_INVERSE: 36, LOOK: 52, FLAGS: 56 };
const MAX_RECORDS = 256;
const flagsOf = (m) => m.side | (m.alphaToCoverage ? 4 : 0) | (m.depthWrite ? 8 : 0);
// What a part's program may read besides the frame block (as instances.js
// gives the guns'): its textures, its row's matrices, look and point
// lights, and lamp lighting's, read only while lamps are off.
const LOOK = new Set(["sunScale", "ambientPull", "brightness"]);
const INSTANCE = ["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix", ...LOOK, "zeroLights", "zeroLightCount"];
const LAMPS = ["localLightPositions", "localLightColors", "localLightCount"];
const POLICY = { uniforms: new Set(["map", "farMap", ...INSTANCE, ...LAMPS]), material: LOOK };

class ViewPass {
  constructor(core) {
    this.core = core;
    this.name = "view";
    this.phase = "view";
    this.parts = new WeakMap(); // mesh -> its record, or null (three draws it)
    this.records = [];
    // Whether the last phase drew everything three would have of the scene.
    this.complete = false;
    // This frame's blended draws, in three's transparent order.
    this.blends = [];
    this.opaques = [];
    this.input = false;
  }

  draw(ctx) {
    // Off (this pass alone): three draws the scene.
    if (this.on === false) return;
    this.complete = true;
    this.blends.length = 0;
    this.opaques.length = 0;
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
      else if (object.isSprite || object.isPoints || object.isLine || object.isLOD) this.complete = false;
    }
    const children = object.children;
    for (let i = 0; i < children.length; i++) this.visit(children[i], camera, ctx, groupOrder);
  }

  part(mesh, ctx, groupOrder) {
    let record = this.parts.get(mesh);
    if (record === undefined) record = this.registerPart(mesh);
    if (record && mesh.material === record.material) {
      if (record.blended) this.blend(record, groupOrder);
      else this.write(record, ctx);
    } else if (record) { mesh.material.visible = true; this.complete = false; }
    else if ([].concat(mesh.material).some((m) => m?.visible)) this.complete = false;
  }

  // A rigid part with a zero material that does not blend (instances.js
  // registerMesh's rule for a gun), or a basic material that blends (the
  // muzzle flash).
  registerPart(mesh) {
    const core = this.core;
    let record = null;
    const material = mesh.material, threeProgram = !Array.isArray(material) && core.renderer.properties.get(material).currentProgram;
    if (threeProgram && material.isMeshBasicMaterial) return this.registerBlended(mesh);
    // A zero material the instanced parts can't take (unlit, its texture
    // scrolling: the rifle's glowing sights): drawn as three compiled it.
    if (threeProgram && material.isShaderMaterial && !material.transparent && !("DYNAMIC" in (material.defines || {})) && "UNLIT" in (material.defines || {})) return this.registerBlended(mesh);
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && "DYNAMIC" in (material.defines || {}) && !mesh.isSkinnedMesh && !mesh.isInstancedMesh) {
      const program = core.program(threeProgram, RIGID, material, POLICY);
      const geometry = program && core.geometry(mesh.geometry, program);
      const texture = core.texture(material.uniforms.map?.value);
      if (geometry !== null && geometry !== undefined && texture >= 0) {
        if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
        if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
        record = { mesh, material, program: program.id, geometry, texture };
        material.visible = !core.active;
        this.records.push(record);
      }
    }
    // Three has not drawn it yet: asked again next frame.
    if (threeProgram || record) this.parts.set(mesh, record);
    return record;
  }

  // A mesh drawn with three's program as it is (no shader hooks), in one
  // geometry range: a basic material that blends (the muzzle flash), or an
  // unlit zero material (opaque).
  registerBlended(mesh) {
    const core = this.core, material = mesh.material, geometry = mesh.geometry;
    let record = null;
    const program = plainProgram(core, material, null);
    if (program === undefined) return undefined; // until three has made it
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    const why = !program ? "program" : program.uniforms.length > MAX_DRAW_UNIFORMS ? `${program.uniforms.length} uniforms` : !blend || !state ? "state"
      : material.onBeforeCompile !== THREE.Material.prototype.onBeforeCompile ? "shader hook" : !geometry.index || geometry.groups.length ? "geometry"
      : mesh.isSkinnedMesh || mesh.isInstancedMesh ? "kind" : null;
    if (why) core.errors.push(`view ${material.name || material.type}: ${why}`);
    else {
      const g = core.geometry(geometry, { id: program.id, attributes: program.attributes });
      if (g !== null && g !== undefined) {
        if (!geometry.boundingSphere) geometry.computeBoundingSphere();
        record = { blended: true, mesh, material, program, geometry: g, blend, state, sides };
        material.visible = !core.active;
        this.records.push(record);
      }
    }
    this.parts.set(mesh, record);
    return record;
  }

  // A blended mesh three would list this frame, with three's sort key: z of
  // its bounding sphere's centre through its world matrix and the view.
  blend(record, groupOrder) {
    const mesh = record.mesh;
    if (mesh.frustumCulled && !frustum.intersectsObject(mesh)) return;
    if (this.blends.length + this.opaques.length >= MAX_DRAWS) { this.complete = false; record.material.visible = true; return; }
    point.copy(mesh.geometry.boundingSphere.center).applyMatrix4(mesh.matrixWorld).applyMatrix4(viewProjection);
    (record.material.transparent ? this.blends : this.opaques).push({ record, groupOrder, renderOrder: mesh.renderOrder, material: record.material.id, z: point.z, id: mesh.id });
  }

  // The frame's draws for view.rs: the opaque ones in three's opaque order,
  // then what blends in its transparent order.
  writeBlends(camera) {
    const core = this.core;
    if (!this.input) {
      if (!this.blends.length && !this.opaques.length) return;
      core.addView("viewInput", Float64Array, () => core.rust.view_input_ptr(), () => core.rust.view_input_len());
      this.input = true;
    }
    core.views();
    const input = core.viewInput;
    input[0] = 0;
    this.opaques.sort(opaqueOrder);
    this.blends.sort(transparentOrder);
    let n = 0;
    for (const { record } of [...this.opaques, ...this.blends]) {
      const { mesh, material, program } = record, geometry = mesh.geometry, o = 1 + n * DRAW;
      const range = geometry.drawRange, count = geometry.index.count;
      const first = Math.max(0, range.start), last = Math.min(count, range.start + range.count);
      if (last <= first) continue;
      input[o + D.PROGRAM] = program.id; input[o + D.GEOMETRY] = record.geometry;
      for (let k = 0; k < 7; k++) input[o + D.BLEND + k] = record.blend[k];
      input[o + D.DEPTH_FUNC] = record.state.func; input[o + D.FLAGS] = record.state.flags;
      input[o + D.SIDES] = record.sides.length; input[o + D.SIDES + 1] = record.sides[0]; input[o + D.SIDES + 2] = record.sides[1] ?? 0;
      input[o + D.RANGE] = first; input[o + D.RANGE + 1] = last - first;
      input[o + D.UNIFORMS] = program.uniforms.length;
      let failed = null;
      for (let k = 0; k < program.uniforms.length && !failed; k++) if (!writeUniform(core, input, o + D.FIRST + k * UNIFORM, k, program.uniforms[k], material, mesh, camera)) failed = program.uniforms[k].name;
      if (failed) {
        if (!record.failed) { record.failed = true; core.errors.push(`view ${material.name || material.type}: no value for ${failed}`); }
        this.complete = false; material.visible = true; continue;
      }
      n++;
    }
    input[0] = n;
  }

  write(record, at) {
    if (at.records >= MAX_RECORDS) { this.complete = false; record.material.visible = true; return; }
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
    input[o + R.FLAGS] = flagsOf(material);
    at.records++;
  }

  setActive(on) {
    for (const record of this.records) record.material.visible = !on;
    this.on = on;
  }

  // Shown to three while one of three's own renders needs them (index.js
  // reveal): the temporal upscaler's motion pass draws the view scene again,
  // with an override material, for the mask that keeps it fixed on screen.
  reveal(on) {
    for (const record of this.records) record.material.visible = on;
  }

  restore() {
    this.setActive(false);
  }
}

export function viewPass(core) {
  return new ViewPass(core);
}
