// Non-battle interface scenes publish one packet of retained geometry and
// source-owned programs. Rust owns ordering, raster state and every draw.
import * as THREE from "three";
import { UI } from "./interface-layout.js";
import { ownedGeometryProgram } from "./programs.js";
import { writeUniform } from "./post.js";
import { blendOf, stateOf } from "./transparent.js";
import { GL_TYPES } from "./geometry-types.js";

const MODEL_KEYS = ["model", "model-color", "model-skin", "model-skin-color"];
// interface_scene.rs raster flags; the depth bits also match stateOf's input.
const DEPTH_TEST = 1, DEPTH_WRITE = 2, BLEND = 4, TRANSPARENT = 8, TWO_PASS = 16;
const COLOR_WRITE = 4;

export class InterfacePass {
  constructor(core) {
    this.core = core;
    this.geometry = new WeakMap();
    this.materials = new WeakMap();
    this.programs = new Map();
    this.skeletons = new WeakMap();
    this.rows = [];
    this.count = 0;
    this.frame = 0;
    this.screen = new THREE.Matrix4();
    this.frustum = new THREE.Frustum();
    this.point = new THREE.Vector3();
    this.size = new THREE.Vector2();
    core.addView("interfaceInput", Float64Array, () => core.rust.interface_input_ptr(), () => core.rust.interface_input_len());
  }

  // renderInterface's old callers retain layer clears and scene values. Finish
  // all registrations before taking WASM views: any new resource can grow it.
  render(scene, camera) {
    const core = this.core, renderer = core.renderer;
    if (core.contextLost) return;
    if (core.failure) throw core.failure;
    if (scene.matrixWorldAutoUpdate) scene.updateMatrixWorld();
    if (camera.parent === null && camera.matrixWorldAutoUpdate) camera.updateMatrixWorld();
    scene.onBeforeRender(renderer, scene, camera, renderer.getRenderTarget());
    this.screen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.screen);
    core.frame.interfaceBegin();
    this.frame++; this.count = 0;
    this.walk(scene, camera, 0);
    // Resource creation may touch VAOs, textures or framebuffer bindings. Restore
    // the host's target once before handing the whole ordered packet to Rust.
    core.stale = true;
    core.settle();
    core.views();
    for (let i = 0; i < this.count; i++) this.write(this.rows[i], i, camera);
    if(core.frame.active)core.frame.interfaceEnd(this.count);else core.rust.interface_frame(this.count);
    core.stale = true;
    scene.onAfterRender(renderer, scene, camera);
  }

  // WebGLRenderer.projectObject's visibility/layer and group-order semantics.
  // A hidden ancestor suppresses its subtree, while camera layers hide only
  // that node. Retained rows avoid render-list allocation on static menus.
  walk(object, camera, order) {
    if (!object.visible) return;
    if (object.layers.test(camera.layers)) {
      if (object.isGroup) order = object.renderOrder;
      if ((object.isMesh || object.isPoints) && (!object.frustumCulled || this.frustum.intersectsObject(object))) {
        const materials = object.material, geometry = object.geometry;
        if (object.isSkinnedMesh) this.skin(object);
        if (Array.isArray(materials)) {
          for (const group of geometry.groups) {
            const material = materials[group.materialIndex];
            if (material?.visible) this.add(object, material, group, camera, order);
          }
        } else if (materials?.visible) this.add(object, materials, null, camera, order);
      }
    }
    for (let i = 0; i < object.children.length; i++) this.walk(object.children[i], camera, order);
  }

  // WebGLObjects.update refreshes a skeleton once per render, including UI
  // previews whose class changes independently of the battle's actor phase.
  skin(object) {
    const skeleton = object.skeleton;
    if (this.skeletons.get(skeleton) === this.frame) return;
    skeleton.update();
    if (!skeleton.boneTexture) skeleton.computeBoneTexture();
    this.skeletons.set(skeleton, this.frame);
  }

  add(object, material, group, camera, order) {
    const core = this.core, geometry = object.geometry;
    const available = geometry.index ? geometry.index.count : geometry.attributes.position.count;
    const first = Math.max(geometry.drawRange.start, group ? group.start : 0);
    const last = Math.min(available, geometry.drawRange.start + geometry.drawRange.count, group ? group.start + group.count : Infinity);
    if (last <= first) return;
    if (this.count === UI.MAX_DRAWS) core.fail(Error("Too many interface draw records"));
    let key = material.userData?.interfaceProgram;
    if (key === "model") key = MODEL_KEYS[(object.isSkinnedMesh ? 2 : 0) | (material.vertexColors ? 1 : 0)];
    else if (key === "basic" && material.map) key = "basic-map";
    if (!key) core.fail(Error(`Unowned interface material: ${material.type}`));
    let program = this.programs.get(key);
    if (!program) {
      program = ownedGeometryProgram(core, `interface/${key}`);
      this.programs.set(key, program);
    }
    if (program.uniforms.length > UI.MAX_UNIFORMS) core.fail(Error("Interface uniform capacity exceeded"));
    const resource = this.upload(geometry, program);
    let values = this.materials.get(material);
    if (!values || values.version !== material.version) {
      const state = stateOf(material), blend = blendOf(material);
      if (!state || !blend || !(state.flags & COLOR_WRITE) || material.stencilWrite || material.polygonOffset || material.alphaToCoverage)
        core.fail(Error("Unsupported interface raster state"));
      values = { state, blend, version: material.version };
      this.materials.set(material, values);
    }
    this.values(material, key);
    for (const u of program.uniforms) if (u.type >= 32) {
      const texture = u.name === "boneTexture" ? object.skeleton.boneTexture : material.uniforms[u.name]?.value;
      if (texture) core.texture(texture);
    }
    let row = this.rows[this.count];
    if (!row) this.rows[this.count] = row = {};
    row.object = object; row.material = material; row.program = program;
    row.resource = resource; row.first = first; row.count = last - first;
    row.order = order; row.values = values;
    // Three sorts the geometry sphere centre's clip z without dividing by W.
    // Skinned previews carry an authored object-space sphere. Using their
    // bind-pose geometry sphere instead changes transparent part ordering.
    let sphere;
    if (object.boundingSphere !== undefined) {
      if (object.boundingSphere === null) object.computeBoundingSphere();
      sphere = object.boundingSphere;
    } else {
      if (!geometry.boundingSphere) geometry.computeBoundingSphere();
      sphere = geometry.boundingSphere;
    }
    this.point.copy(sphere.center);
    const e = object.matrixWorld.elements, p = this.point;
    const x = p.x, y = p.y, z = p.z;
    const wx = e[0]*x+e[4]*y+e[8]*z+e[12], wy = e[1]*x+e[5]*y+e[9]*z+e[13], wz = e[2]*x+e[6]*y+e[10]*z+e[14];
    const s = this.screen.elements;
    row.depth = s[2]*wx+s[6]*wy+s[10]*wz+s[14];
    this.count++;
  }

  // BasicMaterial and PointsMaterial's scalar refresh, now values for owned
  // sources. Custom room/model uniforms already carry their authored values.
  values(material, key) {
    if (key !== "basic" && key !== "basic-map" && key !== "points") return;
    let u = material.uniforms;
    if (!u.diffuse) {
      u.diffuse = { value: material.color }; u.opacity = { value: material.opacity };
      u.map = { value: null }; u.mapTransform = { value: null };
      u.uvTransform = { value: null }; u.size = { value: 0 }; u.scale = { value: 0 };
    }
    u.opacity.value = material.opacity;
    u.map.value = material.map;
    if (material.map) {
      if (material.map.matrixAutoUpdate) material.map.updateMatrix();
      u.mapTransform.value = u.uvTransform.value = material.map.matrix;
    }
    if (key === "points") {
      const renderer = this.core.renderer;
      renderer.getSize(this.size);
      u.size.value = material.size * renderer.getPixelRatio();
      u.scale.value = this.size.y * 0.5;
    }
  }

  // Retain VAOs and byte views by geometry/program. Changed host attributes
  // update their Rust buffers; no shader discovery or browser draw occurs.
  upload(geometry, program) {
    const core = this.core;
    let programs = this.geometry.get(geometry);
    if (!programs) this.geometry.set(geometry, programs = new Map());
    let record = programs.get(program.id);
    if (!record) {
      record = { id: core.rust.add_geometry(), buffers: [], index: null };
      programs.set(program.id, record);
      core.stale = true;
    }
    for (let i = 0; i < program.attributes.length; i++) {
      const pair = program.attributes[i], name = pair[0], location = pair[1], attribute = geometry.attributes[name];
      if (!attribute) core.fail(Error(`Missing interface attribute: ${name}`));
      const source = attribute.isInterleavedBufferAttribute ? attribute.data : attribute;
      const array = source.array;
      let buffer = record.buffers[i];
      if (!buffer || buffer.array !== array) {
        const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
        if (!buffer) buffer = record.buffers[i] = { slot: core.rust.geometry_buffer(record.id, bytes) };
        else core.rust.geometry_buffer_data(record.id, buffer.slot, bytes);
        buffer.array = array; buffer.bytes = bytes; buffer.version = source.version;
        const stream = buffer.stream = core.rust.interface_buffer_init(record.id, buffer.slot, bytes);
        buffer.view = `interfaceBytes${stream}`;
        core.addView(buffer.view, Uint8Array, () => core.rust.interface_buffer_ptr(stream), () => core.rust.interface_buffer_len(stream));
        core.rust.geometry_pointer(record.id, buffer.slot, location, attribute.itemSize, GL_TYPES[array.constructor.name], attribute.normalized,
          attribute.isInterleavedBufferAttribute ? source.stride * array.BYTES_PER_ELEMENT : 0,
          attribute.isInterleavedBufferAttribute ? attribute.offset * array.BYTES_PER_ELEMENT : 0);
        core.stale = true;
      } else if (buffer.version !== source.version) {
        core.views();
        core[buffer.view].set(buffer.bytes);
        let first = buffer.bytes.length, last = 0;
        for (const range of source.updateRanges) {
          first = Math.min(first, range.start * array.BYTES_PER_ELEMENT);
          last = Math.max(last, (range.start + range.count) * array.BYTES_PER_ELEMENT);
        }
        if (last <= first) { first = 0; last = buffer.bytes.length; }
        core.rust.interface_upload_frame(buffer.stream, first, last - first);
        buffer.version = source.version; core.stale = true;
      }
    }
    if (geometry.index && (!record.index || record.index.version !== geometry.index.version || record.index.array !== geometry.index.array)) {
      const index = geometry.index, bytes = new Uint8Array(index.array.buffer, index.array.byteOffset, index.array.byteLength);
      core.rust.geometry_index(record.id, bytes, GL_TYPES[index.array.constructor.name]);
      record.index = { array: index.array, version: index.version }; core.stale = true;
    }
    if (!geometry.index && !record.arrays) {
      core.rust.geometry_primitive(record.id, 4, 0, false, 1); record.arrays = true;
    }
    return record;
  }

  write(row, index, camera) {
    const core = this.core, out = core.interfaceInput, at = index * UI.STRIDE;
    const object = row.object, material = row.material, state = row.values.state, blend = row.values.blend;
    out[at + UI.PROGRAM] = row.program.id; out[at + UI.GEOMETRY] = row.resource.id;
    out[at + UI.FIRST] = row.first; out[at + UI.COUNT] = row.count;
    out[at + UI.PRIMITIVE] = object.isPoints ? 0 : 4;
    out[at + UI.SIDE] = material.side;
    out[at + UI.FLAGS] = (state.flags & (DEPTH_TEST | DEPTH_WRITE)) | (blend[0] ? BLEND : 0) | (material.transparent ? TRANSPARENT : 0)
      | (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass ? TWO_PASS : 0);
    out[at + UI.DEPTH_FUNC] = state.func;
    for (let i = 1; i < 7; i++) out[at + UI.EQUATION_RGB + i - 1] = blend[i];
    out[at + UI.FLIP] = object.matrixWorld.determinant() < 0 ? 1 : 0;
    out[at + UI.GROUP_ORDER] = row.order; out[at + UI.RENDER_ORDER] = object.renderOrder;
    out[at + UI.MATERIAL] = material.id; out[at + UI.DEPTH] = row.depth; out[at + UI.OBJECT] = object.id;
    out[at + UI.UNIFORMS] = row.program.uniforms.length;
    for (let i = 0; i < row.program.uniforms.length; i++) {
      const uniform = row.program.uniforms[i], offset = at + UI.VALUES + i * UI.UNIFORM;
      if (uniform.name === "bindMatrix" || uniform.name === "bindMatrixInverse") {
        out[offset] = i; out[offset + 1] = 16;
        const elements = object[uniform.name].elements;
        for (let k = 0; k < 16; k++) out[offset + 2 + k] = elements[k];
      } else if (uniform.name === "boneTexture") {
        out[offset] = i; out[offset + 1] = 32; out[offset + 2] = core.texture(object.skeleton.boneTexture);
      } else if (!writeUniform(core, out, offset, i, uniform, material, object, camera)) core.fail(Error(`Missing interface uniform: ${uniform.name}`));
    }
  }
}
