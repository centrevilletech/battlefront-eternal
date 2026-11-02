// The Rust battle renderer (rust/bfe-render, built to src/rust-pkg) behind
// the `rust` query parameter: an experiment in drawing the near scene's
// static world and its soldiers with far fewer WebGL calls than three makes
// for them. Three.js still draws everything else and stays the default.
//
// It draws inside the near scene's pass, right after the far scene's image
// (graphics.background): the map's opaque buckets, then every visible
// soldier's body and held gun, instanced. Three's state is reset after it
// (renderer.resetState) and three draws the rest of the scene. What it draws
// three does not (each such material is set invisible, which three's render
// lists respect), but three's scene update still composes their matrices
// and bones, which the page's other code reads. It draws them with programs
// made from the GLSL three compiled for them (shader.rs), so they light as
// three's do.
//
// Only what it supports moves; everything else stays with three, as does
// everything in a configuration it does not cover (shadows, lamp lighting).
import * as THREE from "three";
import { lighting } from "./materials.js";
import { packed } from "./point-lights.js";

const WORLD = 0, SKINNED = 1, RIGID = 2;
// The frame block's members (shader.rs FRAME_MEMBERS), in its float layout.
const FRAME = [
  ["projectionMatrix", 0, 16], ["viewMatrix", 16, 16], ["cameraPosition", 32, 3], ["time", 35, 1],
  ["sunDirection", 36, 3], ["bump", 39, 1], ["sunColor", 40, 3], ["shadowStrength", 43, 1],
  ["ambient", 44, 3], ["aaSamples", 47, 1], ["ambientTop", 48, 3], ["terrainShade", 51, 1],
  ["ambientBottom", 52, 3], ["farAaSamples", 55, 1], ["characterAmbient", 56, 3], ["cascadeNear", 59, 1],
  ["fogColor", 60, 3], ["cascadeFar", 63, 1], ["fade", 64, 2], ["resolution", 66, 2], ["fogRange", 68, 2],
];
const FRAME_NAMES = new Set(FRAME.map(([name]) => name));
const LIGHT_COUNT = 70, LIGHTS = 72, MAX_LIGHTS = 16;
// Record layout (renderer.rs REC_*).
const R = { KIND: 0, PROGRAM: 1, GEOMETRY: 2, GROUPS: 3, TEXTURES: 4, BONES: 8, BONE_AT: 9, SPHERE: 10, BOX: 14, WORLD: 20, BIND_INVERSE: 36, LOOK: 52, FLAGS: 56, BIND: 60 };
// A record's state flags (renderer.rs REC_FLAGS): its material's side, then
// alpha to coverage and depth write.
const flagsOf = (m) => m.side | (m.alphaToCoverage ? 4 : 0) | (m.depthWrite ? 8 : 0);
// Uniforms each kind of program may read besides the frame block.
const SAMPLERS = { [WORLD]: new Set(["map", "farMap", "bumpMap"]), [SKINNED]: new Set(["map", "farMap", "boneTexture"]), [RIGID]: new Set(["map", "farMap"]) };
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular"]);
const LIGHTING = new Set(["zeroLights", "zeroLightCount"]);
// Lamp lighting's uniforms: read only while lamps are off (each soldier's
// four nearest lamps would be per instance otherwise).
const LAMPS = new Set(["localLightPositions", "localLightColors", "localLightCount"]);
const INSTANCE = new Set(["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix", "sunScale", "ambientPull", "brightness"]);
const GL_TYPES = { Float32Array: 0x1406, Uint16Array: 0x1403, Uint8Array: 0x1401, Int16Array: 0x1402, Int8Array: 0x1400, Uint32Array: 0x1405 };
const IDENTITY = new THREE.Matrix4();

function o_inScene(object, scene) {
  let o = object;
  while (o.parent) o = o.parent;
  return o === scene;
}

// The renderer with each registration call it is given kept (typed arrays
// copied), so another build of it can be given the same scene.
function recorder(rust, recording) {
  return new Proxy(rust, {
    get(target, name) {
      const value = target[name];
      if (typeof value !== "function") return value;
      if (/^(frame|input_|bones_|records_at|record_stride)/.test(String(name))) return value.bind(target);
      return (...args) => {
        const result = value.apply(target, args);
        recording.calls.push([name, args.map((a) => (ArrayBuffer.isView(a) ? a.slice() : a?.constructor?.name === "WebGLTexture" ? null : a)), result]);
        return result;
      };
    },
  });
}

// A geometry's groups with consecutive ranges of one material merged: a
// soldier's two ranges with the same material draw as one.
const mergedGroups = new WeakMap();
function groupsOf(geometry) {
  let groups = mergedGroups.get(geometry);
  if (groups) return groups;
  groups = [];
  for (const g of geometry.groups) {
    const last = groups[groups.length - 1];
    if (last && last.materialIndex === g.materialIndex && last.start + last.count === g.start) last.count += g.count;
    else groups.push({ start: g.start, count: g.count, materialIndex: g.materialIndex });
  }
  mergedGroups.set(geometry, groups);
  return groups;
}

export async function createRustRender(graphics) {
  const { default: init, BfeRenderer } = await import("./rust-pkg/bfe_render.js");
  const wasm = await init();
  return new RustRender(graphics, wasm, BfeRenderer);
}

class RustRender {
  constructor(graphics, wasm, BfeRenderer) {
    this.graphics = graphics;
    this.renderer = graphics.renderer;
    this.gl = this.renderer.getContext();
    this.wasm = wasm;
    this.rust = new BfeRenderer(this.gl);
    // ?rustRecord: every call made to the renderer, and each frame's inputs,
    // kept for a replay outside the browser (rust/bfe-native).
    this.recording = new URLSearchParams(location.search).has("rustRecord") ? { calls: [], textures: [], frames: [], keepFrames: 0 } : null;
    if (this.recording) this.rust = recorder(this.rust, this.recording);
    this.buffer = null;
    this.programs = new Map(); // three's program -> Rust's (or null: unsupported)
    this.geometries = new Map(); // "geometry uuid:program" -> Rust geometry
    this.textures = new Map(); // WebGLTexture -> Rust texture
    this.bodies = new WeakMap(); // root -> its body's record, or null
    this.meshes = new WeakMap(); // a gun's mesh -> its record, or null
    this.worldPending = null;
    this.worldCount = 0;
    this.farTexture = null;
    this.stats = null;
    // three's own opaque order (by material) unless ?rustSort: sorting by
    // program saves a few switches but moves which of two surfaces at the
    // same depth wins at a few multisampled edges.
    this.byProgram = new URLSearchParams(location.search).has("rustSort");
    this.enabled = true;
    this.active = !new URLSearchParams(location.search).has("rustOff");
    this.errors = [];
    this.worldMeshes = [];
    // ?rustRigs: vehicles' and props' skinned meshes too.
    this.rigsOn = new URLSearchParams(location.search).has("rustRigs");
    this.bodyRecords = [];
    this.meshRecords = [];
  }

  // Three's config the renderer covers.
  supported() {
    const settings = this.graphics.settings;
    return !this.renderer.shadowMap.enabled && !settings.get("lampLighting") && this.graphics.upscaling !== "temporal";
  }

  // Views of the renderer's input in WASM memory, made again whenever the
  // memory has grown (which detaches the old ones): registering a mesh can.
  views() {
    if (this.buffer === this.wasm.memory.buffer) return;
    this.buffer = this.wasm.memory.buffer;
    this.input = new Float64Array(this.buffer, this.rust.input_ptr(), this.rust.input_len());
    this.bones = new Float32Array(this.buffer, this.rust.bones_ptr(), this.rust.bones_len());
    this.recordsAt = this.rust.records_at();
    this.stride = this.rust.record_stride();
  }

  texture(texture) {
    if (!texture) return -1;
    if (!this.renderer.properties.get(texture).__webglTexture) this.renderer.initTexture(texture);
    const handle = this.renderer.properties.get(texture).__webglTexture;
    if (!handle) return -1;
    let id = this.textures.get(handle);
    if (id === undefined) {
      id = this.rust.add_texture(handle);
      this.textures.set(handle, id);
      if (this.recording) this.recording.textures[id] = { handle, width: texture.image?.width ?? texture.mipmaps?.[0]?.width ?? texture.width ?? 0, height: texture.image?.height ?? texture.mipmaps?.[0]?.height ?? texture.height ?? 0 };
    }
    return id;
  }

  // Rust's program for three's (made once), or null when the renderer can't
  // give it what it reads. material: one that draws with it, whose shared
  // frame values must be the frame's.
  program(threeProgram, kind, material) {
    let entry = this.programs.get(threeProgram);
    if (entry !== undefined) return entry;
    entry = null;
    const gl = this.gl, p = threeProgram.program;
    const names = [];
    let ok = true;
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
      const u = gl.getActiveUniform(p, i), name = u.name.replace(/\[0\]$/, "");
      names.push(name);
      if (FRAME_NAMES.has(name)) {
        // A frame value this material keeps of its own can't come from the block.
        if (!["projectionMatrix", "viewMatrix", "cameraPosition"].includes(name) && material.uniforms?.[name] !== lighting[name]) ok = false;
      } else if (SAMPLERS[kind].has(name) || LIGHTING.has(name)) {
        // given per draw
      } else if (LAMPS.has(name)) {
        // off (supported())
      } else if (kind === WORLD ? MATERIAL.has(name) : INSTANCE.has(name)) {
        // given per draw or per instance
      } else ok = false;
      if (!ok) { this.errors.push(`p${threeProgram.id} ${name}`); break; }
    }
    if (ok) {
      const attributes = [];
      for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) {
        const a = gl.getActiveAttrib(p, i);
        attributes.push(`${a.name}:${gl.getAttribLocation(p, a.name)}`);
      }
      const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
      try {
        entry = { id: this.rust.add_program(kind, gl.getShaderSource(vs), gl.getShaderSource(fs), attributes.join(","), names.filter((name) => MATERIAL.has(name)).join(",")),
          attributes: attributes.map((a) => { const [name, location] = a.split(":"); return [name, Number(location)]; }), names: new Set(names) };
      } catch (error) {
        this.errors.push(`p${threeProgram.id}: ${String(error).slice(0, 300)}`);
      }
    }
    this.programs.set(threeProgram, entry);
    return entry;
  }

  // Rust's copy of a geometry's attributes (those the program reads, at its
  // locations) and index.
  geometry(geometry, program) {
    const key = `${geometry.uuid}:${program.id}`;
    let id = this.geometries.get(key);
    if (id !== undefined) return id;
    id = null;
    const attributes = [];
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      const array = a?.isInterleavedBufferAttribute ? a.data.array : a?.array;
      if (!array || !(array.constructor.name in GL_TYPES)) { this.errors.push(`attribute ${name}`); this.geometries.set(key, null); return null; }
      attributes.push([a, location, array]);
    }
    if (!geometry.index) { this.geometries.set(key, null); return null; }
    id = this.rust.add_geometry();
    // Each buffer once (interleaved attributes share theirs).
    const buffers = new Map();
    for (const [a, location, array] of attributes) {
      const source = a.isInterleavedBufferAttribute ? a.data : a;
      let slot = buffers.get(source);
      if (slot === undefined) { slot = this.rust.geometry_buffer(id, new Uint8Array(array.buffer, array.byteOffset, array.byteLength)); buffers.set(source, slot); }
      const bytes = array.BYTES_PER_ELEMENT;
      const stride = a.isInterleavedBufferAttribute ? a.data.stride * bytes : 0, offset = a.isInterleavedBufferAttribute ? a.offset * bytes : 0;
      this.rust.geometry_pointer(id, slot, location, a.itemSize, GL_TYPES[array.constructor.name], a.normalized, stride, offset);
    }
    const index = geometry.index.array;
    this.rust.geometry_index(id, new Uint8Array(index.buffer, index.byteOffset, index.byteLength), index.BYTES_PER_ELEMENT === 2 ? 0x1403 : 0x1405);
    for (const group of groupsOf(geometry)) this.rust.geometry_group(id, group.start, group.count);
    this.geometries.set(key, id);
    return id;
  }

  // The map's opaque meshes in the near scene that the renderer can draw.
  registerWorld() {
    const scene = this.graphics.scene, renderer = this.renderer;
    if (!this.worldPending) {
      this.worldPending = scene.children.filter((o) => o.isMesh && !o.matrixAutoUpdate && o.matrix.equals(IDENTITY) && !Array.isArray(o.material)
        && o.material.defines && "WORLD_SPACE" in o.material.defines && !("TERRAIN" in o.material.defines) && !o.material.transparent);
    }
    this.worldPending = this.worldPending.filter((mesh) => {
      const material = mesh.material, geometry = mesh.geometry;
      const threeProgram = renderer.properties.get(material).currentProgram;
      if (!threeProgram) return true;
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return false;
      const program = this.program(threeProgram, WORLD, material);
      if (!program) return false;
      const g = this.geometry(geometry, program);
      if (g === null) return false;
      if (!geometry.boundingSphere) geometry.computeBoundingSphere();
      if (!geometry.boundingBox) geometry.computeBoundingBox();
      const s = geometry.boundingSphere, b = geometry.boundingBox;
      const pointLit = Boolean(material.userData.pointLit) && program.names.has("zeroLightCount");
      const flags = material.side | (material.alphaToCoverage ? 4 : 0) | (material.depthWrite ? 8 : 0) | (pointLit ? 16 : 0);
      const w = this.rust.add_world(g, program.id, material.id, flags, new Float64Array([s.center.x, s.center.y, s.center.z, s.radius, b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]));
      const map = this.texture(material.uniforms.map?.value);
      if (map >= 0) this.rust.world_texture(w, 0, map);
      if (program.names.has("bumpMap")) { const bump = this.texture(material.uniforms.bumpMap?.value); if (bump >= 0) this.rust.world_texture(w, 2, bump); }
      for (const name of program.names) {
        if (!MATERIAL.has(name)) continue;
        const v = material.uniforms[name]?.value;
        const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
        if (values) this.rust.world_uniform(w, name, new Float32Array(values), false);
      }
      if (mesh.userData.cellLights) {
        // The positions alone (the map's vertices are interleaved).
        const position = geometry.attributes.position, positions = new Float32Array(position.count * 3);
        for (let i = 0; i < position.count; i++) { positions[3 * i] = position.getX(i); positions[3 * i + 1] = position.getY(i); positions[3 * i + 2] = position.getZ(i); }
        const index = geometry.index.array;
        this.rust.world_cells(w, positions, index instanceof Uint32Array ? index : Uint32Array.from(index), 48);
      }
      material.visible = false;
      this.worldMeshes.push(mesh);
      this.worldCount++;
      return false;
    });
    this.rust.sort(this.byProgram);
  }

  // A soldier's body and the guns in his hand as records; registers them
  // (and hides them from three) the first time each is seen drawn by three.
  soldier(root, camera, at) {
    const parts = root.userData.parts;
    if (!parts || !root.visible) return at;
    const { body, hand } = parts;
    let record = this.bodies.get(body);
    if (record === undefined) record = this.registerBody(body);
    if (record && body.visible && camera.layers.test(body.layers) && body.material === record.materials) {
      at = this.writeBody(record, at);
    } else if (record && body.material !== record.materials) {
      // Materials of its own for now (a dissolve, units.js): three draws them.
      for (const m of [].concat(body.material)) m.visible = true;
    }
    for (const group of hand?.children || []) {
      if (!group.visible || !group.isGroup) continue;
      for (const mesh of group.children) {
        if (!mesh.isMesh || !mesh.visible || !camera.layers.test(mesh.layers)) continue;
        let rec = this.meshes.get(mesh);
        if (rec === undefined) rec = this.registerMesh(mesh);
        if (rec && mesh.material === rec.material) at = this.writeMesh(rec, at);
        else if (rec) mesh.material.visible = true;
      }
    }
    return at;
  }

  registerBody(body) {
    let record = null;
    // The materials its groups draw with: the array is the actor's own
    // (actor.js), to which combat.js adds the guns' materials too.
    const all = [].concat(body.material);
    const merged = groupsOf(body.geometry);
    const used = [...new Set(merged.length ? merged.map((g) => all[g.materialIndex]) : [all[0]])];
    const programs = used.map((m) => m && this.renderer.properties.get(m).currentProgram);
    const why = !programs[0] ? "" : !programs.every((p) => p === programs[0]) ? "programs differ" : merged.length > 4 ? "groups" : !("RIGID_SKIN" in (used[0].defines || {})) ? "not rigid" : null;
    if (why) this.errors.push(`body: ${why}`);
    if (programs[0] && why === null) {
      const program = this.program(programs[0], SKINNED, used[0]);
      const geometry = program && this.geometry(body.geometry, program);
      if (geometry !== null && geometry !== undefined) {
        const groups = merged.length ? merged : [{ materialIndex: 0 }];
        const textures = groups.map((g) => this.texture(all[g.materialIndex]?.uniforms.map?.value));
        if (textures.every((t) => t >= 0)) {
          record = { body, materials: body.material, used, program: program.id, geometry, textures, whole: !merged.length, look: used[0].uniforms };
          for (const m of used) m.visible = false;
          this.bodyRecords.push(record);
        }
      }
    }
    if (programs[0] || record) this.bodies.set(body, record);
    return record;
  }

  // The scene's other skinned meshes (vehicle-rig.js: hulls, walkers,
  // animated props), looked for again now and then; soldiers' bodies are
  // found through their roots.
  findRigs() {
    const found = [];
    this.graphics.scene.traverse((o) => { if (o.isSkinnedMesh && !o.userData.poseSphere && o.name !== "body-shadow" && !Array.isArray(o.material)) found.push(o); });
    this.rigList = found;
  }

  rigs(camera, at) {
    if (!this.rigList || this.frames % 120 === 0) this.findRigs();
    for (const mesh of this.rigList) {
      let rec = this.bodies.get(mesh);
      if (rec === undefined) rec = this.registerRig(mesh);
      if (!rec) continue;
      if (mesh.material !== rec.materials) { mesh.material.visible = true; continue; }
      if (!camera.layers.test(mesh.layers)) continue;
      let shown = true;
      for (let o = mesh; o && o !== this.graphics.scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown && o_inScene(mesh, this.graphics.scene)) at = this.writeBody(rec, at);
    }
    return at;
  }

  registerRig(mesh) {
    let record = null;
    const material = mesh.material, threeProgram = this.renderer.properties.get(material).currentProgram;
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && mesh.skeleton) {
      const program = this.program(threeProgram, SKINNED, material);
      const geometry = program && this.geometry(mesh.geometry, program);
      const texture = this.texture(material.uniforms?.map?.value);
      if (geometry !== null && geometry !== undefined && texture >= 0) {
        record = { body: mesh, materials: material, used: [material], program: program.id, geometry, textures: [texture], whole: true, look: material.uniforms };
        material.visible = false;
        this.bodyRecords.push(record);
      }
    }
    if (threeProgram || record) this.bodies.set(mesh, record);
    return record;
  }

  registerMesh(mesh) {
    let record = null;
    const material = mesh.material, threeProgram = !Array.isArray(material) && this.renderer.properties.get(material).currentProgram;
    if (threeProgram && !material.transparent && !mesh.geometry.groups.length && "DYNAMIC" in (material.defines || {})) {
      const program = this.program(threeProgram, RIGID, material);
      const geometry = program && this.geometry(mesh.geometry, program);
      const texture = this.texture(material.uniforms.map?.value);
      if (geometry !== null && geometry !== undefined && texture >= 0) {
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
    if (at.records >= 256) return at;
    this.views();
    const { body } = record, input = this.input, skeleton = body.skeleton;
    const bones = skeleton.bones.length;
    let boneAt = at.skeletons.get(skeleton);
    if (boneAt === undefined) {
      if (at.bones + bones > this.bones.length / 16) return at;
      skeleton.update();
      this.bones.set(skeleton.boneMatrices, at.bones * 16);
      boneAt = at.bones; at.skeletons.set(skeleton, boneAt); at.bones += bones;
    }
    const o = this.recordsAt + at.records * this.stride;
    input[o + R.KIND] = SKINNED; input[o + R.PROGRAM] = record.program; input[o + R.GEOMETRY] = record.geometry;
    input[o + R.GROUPS] = record.whole ? 0 : record.textures.length;
    for (let g = 0; g < record.textures.length; g++) input[o + R.TEXTURES + g] = record.textures[g];
    input[o + R.BONES] = bones; input[o + R.BONE_AT] = boneAt;
    const s = body.boundingSphere;
    input[o + R.SPHERE] = s.center.x; input[o + R.SPHERE + 1] = s.center.y; input[o + R.SPHERE + 2] = s.center.z; input[o + R.SPHERE + 3] = s.radius;
    const w = body.matrixWorld.elements, b = body.bindMatrixInverse.elements, bind = body.bindMatrix.elements;
    for (let k = 0; k < 16; k++) { input[o + R.WORLD + k] = w[k]; input[o + R.BIND_INVERSE + k] = b[k]; input[o + R.BIND + k] = bind[k]; }
    const look = record.look;
    input[o + R.LOOK] = look.sunScale.value; input[o + R.LOOK + 1] = look.ambientPull.value.x; input[o + R.LOOK + 2] = look.ambientPull.value.y; input[o + R.LOOK + 3] = look.brightness.value;
    input[o + R.FLAGS] = flagsOf(record.used[0]);
    at.records++;
    return at;
  }

  writeMesh(record, at) {
    if (at.records >= 256) return at;
    this.views();
    const { mesh, material } = record, input = this.input, geometry = mesh.geometry;
    const o = this.recordsAt + at.records * this.stride;
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
    return at;
  }

  // The frame's shared values, as three would upload them.
  writeFrame(scene, camera) {
    this.views();
    const input = this.input;
    input.set(camera.projectionMatrix.elements, 0);
    input.set(camera.matrixWorldInverse.elements, 16);
    const m = camera.matrixWorld.elements;
    input[32] = m[12]; input[33] = m[13]; input[34] = m[14];
    for (const [name, at, size] of FRAME) {
      if (at < 35) continue;
      const v = lighting[name].value;
      if (size === 1) input[at] = v;
      else { input[at] = v.x; input[at + 1] = v.y; if (size === 3) input[at + 2] = v.z; }
    }
    const pack = packed(scene, camera);
    const count = Math.min(pack.count, MAX_LIGHTS);
    input[LIGHT_COUNT] = count;
    for (let i = 0; i < 2 * count; i++) {
      const v = pack.lights[i], o = LIGHTS + 4 * i;
      input[o] = v.x; input[o + 1] = v.y; input[o + 2] = v.z; input[o + 3] = v.w;
    }
  }

  // Off: three draws everything again (for comparisons in one battle); on:
  // what is registered moves back.
  setActive(on) {
    on = Boolean(on);
    if (on === this.active) return;
    this.active = on;
    const set = (m) => { if (m) m.visible = !on; };
    for (const mesh of this.worldMeshes) set(mesh.material);
    for (const record of this.bodyRecords) for (const m of record.used) set(m);
    for (const record of this.meshRecords) set(record.material);
  }

  // graphics.background's onAfterRender: the near scene's pass, the far
  // scene's image just drawn behind it.
  pass(renderer, scene, camera) {
    if (!this.enabled || !this.active || camera !== this.graphics.camera || renderer.getRenderTarget() !== this.graphics.sceneTarget) return;
    if (!this.supported()) { this.restore(); return; }
    const far = this.renderer.properties.get(this.graphics.farTarget.texture).__webglTexture;
    if (far !== this.farTexture) { this.farTexture = far; this.rust.set_far_map(this.texture(this.graphics.farTarget.texture)); }
    if (!this.worldPending || this.worldPending.length) this.registerWorld();
    this.writeFrame(scene, camera);
    const at = { records: 0, bones: 0, skeletons: new Map() };
    for (const root of this.graphics.cullRoots) this.soldier(root, camera, at);
    if (this.graphics.playerRoot) this.soldier(this.graphics.playerRoot, camera, at);
    if (this.rigsOn) this.rigs(camera, at);
    this.frames = (this.frames || 0) + 1;
    if (this.recording?.keepFrames > 0) {
      this.recording.keepFrames--;
      this.recording.frames.push({ records: at.records, input: this.input.slice(0, this.recordsAt + at.records * this.stride), bones: this.bones.slice(0, at.bones * 16) });
    }
    const t0 = performance.now();
    this.stats = this.rust.frame(at.records);
    this.frameMs = performance.now() - t0;
    renderer.resetState();
    renderer.setRenderTarget(this.graphics.sceneTarget);
  }

  // Everything back to three (the configuration left what it covers).
  restore() {
    this.enabled = false;
    this.errors.push("unsupported configuration: three draws everything");
    this.graphics.scene.traverse((o) => { for (const m of [].concat(o.material || [])) if (m && m.visible === false && m.isShaderMaterial) m.visible = true; });
  }
}
