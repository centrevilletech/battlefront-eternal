import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { lighting } = await import("../src/materials.js");

const GL = {
  ACTIVE_UNIFORMS: 0x8B86, ACTIVE_ATTRIBUTES: 0x8B89, SHADER_TYPE: 0x8B4F, VERTEX_SHADER: 0x8B31, FRAGMENT_SHADER: 0x8B30,
  FUNC_ADD: 0x8006, FUNC_SUBTRACT: 0x800A, FUNC_REVERSE_SUBTRACT: 0x800B, MIN: 0x8007, MAX: 0x8008, ZERO: 0, ONE: 1,
  SRC_COLOR: 0x0300, ONE_MINUS_SRC_COLOR: 0x0301, SRC_ALPHA: 0x0302, ONE_MINUS_SRC_ALPHA: 0x0303, DST_ALPHA: 0x0304,
  ONE_MINUS_DST_ALPHA: 0x0305, DST_COLOR: 0x0306, ONE_MINUS_DST_COLOR: 0x0307, SRC_ALPHA_SATURATE: 0x0308,
  READ_FRAMEBUFFER: 0x8CA8, DRAW_FRAMEBUFFER: 0x8CA9, COLOR_BUFFER_BIT: 0x4000, NEAREST: 0x2600,
};
const MAT4 = 0x8B5C, VEC3 = 0x8B51, FLOAT = 0x1406, SAMPLER_2D = 0x8B5E;

// A far scene's mesh with a material three has compiled (one program a
// material name): `uniforms` the program reads ([name, type]), the
// material's values for its own.
const programs = new Map();
function mesh(name, uniforms, { renderOrder = 0, transparent = false, values = {}, matrixAutoUpdate = false, points = false } = {}) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setIndex([0, 1, 2]);
  const material = new THREE.ShaderMaterial({ name, transparent, uniforms: { ...lighting, ...values } });
  const object = points ? new THREE.Points(geometry, material) : new THREE.Mesh(geometry, material);
  object.renderOrder = renderOrder;
  object.matrixAutoUpdate = matrixAutoUpdate;
  object.updateMatrixWorld(true);
  if (!programs.has(name)) programs.set(name, { id: name, program: { uniforms: uniforms.map(([n, type]) => ({ name: n, type, size: 1 })), attributes: [{ name: "position", location: 0 }] } });
  object.program = programs.get(name);
  return object;
}

function bridge() {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 17) };
  let next = 0;
  const id = (name) => function (...args) { calls.push([name, ...args]); return next++; };
  class BfeRenderer {
    input_ptr() { return 0; }
    input_len() { return 200 + 76 * 4; }
    bones_ptr() { return 8192; }
    bones_len() { return 64; }
    far_input_ptr() { return 16384; }
    far_input_len() { return 1024; }
    records_at() { return 200; }
    record_stride() { return 76; }
    set_far_map() {}
    sort() {}
    far_frame() { calls.push(["far_frame"]); return [0, 0, 0, 1, 10, 0, 0, 3, 0, 0, 0, 0, 0, 0]; }
  }
  for (const name of ["add_texture", "add_program", "add_geometry", "geometry_buffer", "far_table", "far_item"]) BfeRenderer.prototype[name] = id(name);
  for (const name of ["geometry_pointer", "geometry_index", "geometry_group", "far_source", "far_range", "far_cells", "far_terrain", "far_instanced"]) {
    BfeRenderer.prototype[name] = function (...args) { calls.push([name, ...args]); };
  }
  const gl = {
    ...GL,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => p.uniforms[i],
    getActiveAttrib: (p, i) => p.attributes[i],
    getAttribLocation: (p, name) => p.attributes.find((a) => a.name === name).location,
    getAttachedShaders: () => [{ type: GL.VERTEX_SHADER }, { type: GL.FRAGMENT_SHADER }],
    getShaderParameter: (s) => s.type,
    getShaderSource: () => "void main() {}",
    blitFramebuffer: (...args) => calls.push(["blitFramebuffer", ...args]),
  };
  const scene = new THREE.Scene(), farScene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(60, 1, 2, 1000);
  camera.position.set(0, 0, 10);
  camera.updateMatrixWorld(true);
  const farTarget = { texture: new THREE.Texture(), samples: 2, width: 4, height: 4 };
  const sceneTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  const get = (o) => {
    if (!properties.has(o)) properties.set(o, o.program ? { currentProgram: o.program } : o === farTarget ? { __webglMultisampledFramebuffer: "ms", __webglFramebuffer: "fb" } : { __webglTexture: {} });
    return properties.get(o);
  };
  let target = farTarget;
  const renderer = {
    getContext: () => gl,
    properties: { get: (o) => (o.isShaderMaterial ? get(farScene.getObjectByProperty("material", o)) : get(o)) },
    initTexture() {},
    shadowMap: { enabled: false },
    autoClear: true,
    state: { bindFramebuffer: (t, f) => calls.push(["bindFramebuffer", t, f]) },
    getRenderTarget: () => target,
    setRenderTarget(t) { calls.push(["setRenderTarget", t === farTarget ? "far" : t]); target = t; },
    resetState() { calls.push(["resetState"]); },
    getClearColor: (c) => c.setRGB(0.25, 0.5, 0.75),
    getClearAlpha: () => 1,
    getContextAttributes: () => ({ premultipliedAlpha: true }),
    render(s, c) {
      const shown = [];
      s.traverseVisible((o) => { if (o.material?.visible) shown.push(o.material.name); });
      calls.push(["render", s === farScene ? "far" : s, c === camera ? "camera" : c, this.autoClear, shown]);
    },
  };
  const graphics = { renderer, scene, farScene, camera, farCamera: camera, sceneTarget, farTarget, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  return { core, calls, renderer, farScene, camera, far: core.byName.far };
}

const SKY = [["modelMatrix", MAT4], ["modelViewMatrix", MAT4], ["normalMatrix", 0x8B5B], ["projectionMatrix", MAT4], ["sunColor", VEC3], ["sunScale", FLOAT], ["map", SAMPLER_2D]];
const BUCKET = [["projectionMatrix", MAT4], ["viewMatrix", MAT4], ["fogColor", VEC3], ["brightness", FLOAT], ["map", SAMPLER_2D]];
const values = () => ({ sunScale: { value: 1 }, brightness: { value: 0.5 }, map: { value: new THREE.Texture() } });

test("a far scene it covers whole: the renderer clears, draws it in three's order and resolves the target; three draws none of it", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const sky = mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true });
  sky.position.set(5, 6, 7);
  const a = mesh("zero:lit", BUCKET, { values: values() }), b = mesh("zero:lit", BUCKET, { values: values() });
  farScene.add(b, sky, a);
  assert.equal(far.render(renderer, farScene, camera), true);
  assert.equal(far.mode, "full");
  // One item a mesh, each program's uniform table once.
  assert.equal(calls.filter((c) => c[0] === "far_item").length, 3);
  assert.equal(calls.filter((c) => c[0] === "far_table").length, 2);
  // The renderer's call, three's state reset, the target bound again, then
  // the multisampled image resolved (colour only), and no render by three.
  const after = calls.slice(calls.findIndex((c) => c[0] === "far_frame"));
  assert.deepEqual(after.map((c) => c[0]), ["far_frame", "resetState", "setRenderTarget", "bindFramebuffer", "bindFramebuffer", "blitFramebuffer", "bindFramebuffer", "bindFramebuffer", "bindFramebuffer"]);
  assert.deepEqual(after[5].slice(1), [0, 0, 4, 4, 0, 0, 4, 4, GL.COLOR_BUFFER_BIT, GL.NEAREST]);
  // The input: the clear colour premultiplied, the pass drawing, each item
  // drawn, the sky's matrix where it stands this frame.
  core.views();
  const input = core.farInput;
  assert.deepEqual([...input.subarray(0, 5)], [0.25, 0.5, 0.75, 1, 1]);
  for (const item of far.all) assert.equal(input[item.slot], 1);
  const skyItem = far.all.find((i) => i.object === sky);
  assert.deepEqual([...input.subarray(skyItem.slot + 1, skyItem.slot + 17)], sky.matrixWorld.elements);
  // Its fixed values (a zero material's) go once, at registration.
  const sources = calls.filter((c) => c[0] === "far_source" && c[1] === far.all.find((i) => i.object === a).id);
  assert.deepEqual(sources.map((c) => c[3]), [0, 2]); // brightness (floats), map (texture)
  calls.length = 0;
  far.render(renderer, farScene, camera);
  assert.equal(calls.filter((c) => c[0].startsWith("far_") && c[0] !== "far_frame").length, 0);
});

test("a mesh it can't draw in view: the renderer draws nothing and three the frame; the same with the pass off", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const dust = mesh("dust", BUCKET, { renderOrder: 5, points: true, values: values() });
  farScene.add(mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true }), mesh("zero:lit", BUCKET, { values: values() }), dust);
  assert.equal(far.render(renderer, farScene, camera), false);
  assert.deepEqual([far.mode, far.threeObjects], ["three", 1]);
  core.views();
  assert.equal(core.farInput[4], 0);
  // The phase's call (which draws nothing), three's state reset after the
  // registering, and no resolve.
  assert.deepEqual(calls.slice(calls.findIndex((c) => c[0] === "far_frame")).map((c) => c[0]), ["far_frame", "resetState", "setRenderTarget"]);
  // Out of view, three would draw it no more than the renderer.
  dust.position.set(0, 0, 100);
  dust.updateMatrix();
  dust.updateMatrixWorld();
  assert.equal(far.render(renderer, farScene, camera), true);
  far.on = false;
  calls.length = 0;
  assert.equal(far.render(renderer, farScene, camera), false);
  assert.deepEqual(calls, []);
});
