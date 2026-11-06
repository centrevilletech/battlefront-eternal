import { standinResources } from "./support/rust-standin.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule } = await import("./support/rust-standin.mjs");
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
  // The shared stand-in for the WASM renderer (every pass's views), with
  // the far pass's calls as this test reads them: ids from one counter, the
  // far phase's stats.
  let next = 0;
  const { BfeRenderer, memory, calls } = standinModule({
    quiet: ["set_far_map", "sort", "transparent_bucket_shown", "foliage_on"],
    methods: ({ calls }) => {
      const methods = {
        far_frame() { calls.push(["far_frame"]); return [0, 0, 0, 1, 10, 0, 0, 3, 0, 0, 0, 0, 0, 0]; },
        geometry_buffer(...args) { calls.push(["geometry_buffer", ...args]); return 0; },
      };
      for (const name of ["add_texture", "add_program", "add_geometry", "far_table", "far_item"]) methods[name] = (...args) => { calls.push([name, ...args]); return next++; };
      for (const name of ["geometry_pointer", "geometry_index", "geometry_group", "far_source", "far_range", "far_base", "far_cells", "far_terrain", "far_instanced"]) methods[name] = (...args) => { calls.push([name, ...args]); };
      return methods;
    },
  });
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
    bindFramebuffer: (t, f) => calls.push(["bindFramebuffer", t, f]),
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
  standinResources(renderer,calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
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
  assert.equal(core.cover("far", renderer, farScene, camera), true);
  assert.equal(far.mode, "full");
  // One item a mesh, each program's uniform table once.
  assert.equal(calls.filter((c) => c[0] === "far_item").length, 3);
  assert.equal(calls.filter((c) => c[0] === "far_table").length, 2);
  // The renderer's call, then the multisampled image resolved (colour only),
  // and no render by three. Three's state is reset when three is next used
  // (the core's settle), once, whatever the renderer drew in between.
  const after = calls.slice(calls.findIndex((c) => c[0] === "far_frame"));
  assert.deepEqual(after.map((c) => c[0]), ["far_frame", "resolveTarget"]);
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
  core.settle();
  assert.deepEqual(calls.map((c) => c[0]), ["resetState", "setRenderTarget"]);
  assert.equal(calls[1][1], "far", "the target three believed bound, bound again");
  calls.length = 0;
  core.cover("far", renderer, farScene, camera);
  assert.equal(calls.filter((c) => c[0].startsWith("far_") && c[0] !== "far_frame").length, 0);
});

test("a mesh it can't draw in view: the renderer draws nothing and three the frame; the same with the pass off", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const dust = mesh("dust", BUCKET, { renderOrder: 5, points: true, values: values() });
  farScene.add(mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true }), mesh("zero:lit", BUCKET, { values: values() }), dust);
  assert.throws(() => core.cover("far", renderer, farScene, camera), /Incomplete Rust/);
  assert.deepEqual([far.mode, far.unclaimed], ["incomplete", 1]);
  core.views();
  assert.equal(core.farInput[4], 0);
  // The phase's call (which draws nothing) and no resolve; three's state is
  // reset as three renders the frame.
  assert.deepEqual(calls.slice(calls.findIndex((c) => c[0] === "far_frame")).map((c) => c[0]), ["far_frame"]);
});

test("a far bucket with no texture: the renderer samples an empty one there, as three does, and draws the frame", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const bare = mesh("zero:lit", BUCKET, { values: { ...values(), map: { value: null } } });
  farScene.add(mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true }), bare);
  assert.equal(core.cover("far", renderer, farScene, camera), true);
  assert.equal(far.mode, "full");
  const item = far.all.find((i) => i.object === bare);
  assert.ok(item, "an item");
  const map = calls.filter((c) => c[0] === "far_source" && c[1] === item.id).find((c) => c[3] === 2);
  assert.ok(map, "its map from a texture of the renderer's");
  assert.equal(far.empty.image.data.every((v) => v === 0), true, "one texel of zeros");
  assert.equal(map[5], core.textures.get(renderer.properties.get(far.empty).__webglTexture));
});

test("an error in the far pass leaves the far scene to three for good", () => {
  const { core, renderer, farScene, camera, far } = bridge();
  farScene.add(mesh("zero:lit", BUCKET, { values: values() }));
  core.rust.far_item = () => { throw new Error("no"); };
  assert.throws(() => core.cover("far", renderer, farScene, camera), /no/);
  assert.equal(core.failure.rendererFailure, true);
});

test("the map's far buckets laid out alike share one vertex array, each drawn from where its index starts", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const bucket = (vertices, index) => {
    const object = mesh("zero:lit", BUCKET, { values: values() });
    const data = new THREE.InterleavedBuffer(new Float32Array(vertices), 3);
    object.geometry = new THREE.BufferGeometry();
    object.geometry.setAttribute("position", new THREE.InterleavedBufferAttribute(data, 3, 0));
    object.geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(index), 1));
    object.material.defines = { FAR: "", WORLD_SPACE: "" };
    return object;
  };
  const a = bucket([0, 0, 0, 1, 0, 0, 0, 1, 0], [0, 1, 2]), b = bucket([0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1], [0, 1, 2, 1, 3, 2]);
  farScene.add(a, b);
  assert.equal(core.cover("far", renderer, farScene, camera), true);
  // One geometry for both: their vertices one after the other, b's indices
  // offset by a's three vertices.
  assert.equal(calls.filter((c) => c[0] === "add_geometry").length, 1);
  const vertex = calls.find((c) => c[0] === "geometry_buffer")[2], index = calls.find((c) => c[0] === "geometry_index")[2];
  assert.deepEqual([...new Float32Array(vertex.buffer, vertex.byteOffset, vertex.byteLength / 4)], [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1]);
  assert.deepEqual([...new Uint32Array(index.buffer, index.byteOffset, index.byteLength / 4)], [0, 1, 2, 3, 4, 5, 4, 6, 5]);
  // Each drawn from its own start, over its own indices.
  const item = (o) => far.all.find((i) => i.object === o).id;
  assert.deepEqual(calls.filter((c) => c[0] === "far_base").map((c) => c.slice(1)), [[item(a), 0], [item(b), 3]]);
  assert.deepEqual(calls.filter((c) => c[0] === "far_range").map((c) => c.slice(1)), [[item(a), 0, 3], [item(b), 0, 6]]);
});

test("a water mirror's far scene: its sky pass cleared, the rest drawn over it uncleared, each with the mirror's frame block", () => {
  const { core, calls, renderer, farScene, far } = bridge();
  const sky = mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true });
  const bucket = mesh("zero:lit", BUCKET, { values: values() });
  farScene.add(sky, bucket);
  const mirror = new THREE.PerspectiveCamera(60, 1, 2, 1000);
  mirror.userData.mirror = true;
  mirror.position.set(0, -4, 10);
  mirror.updateMatrixWorld(true);
  // The sky pass: the mirror on the sky's layer alone (water.js
  // MIRROR_SKY_LAYER), three's clear on.
  sky.layers.enable(5);
  mirror.layers.set(5);
  assert.equal(core.cover("farMirror", renderer, farScene, mirror), true);
  core.views();
  assert.deepEqual([core.farInput[4], core.farInput[5]], [1, 0], "drawn, cleared");
  const skyItem = () => far.all.find((i) => i.object === sky), bucketItem = () => far.all.find((i) => i.object === bucket);
  assert.equal(core.farInput[skyItem().slot], 1);
  assert.equal(bucketItem(), undefined, "the bucket isn't on the sky's layer");
  // The rest over it: autoClear off, the sky hidden.
  sky.visible = false;
  mirror.layers.set(0);
  renderer.autoClear = false;
  assert.equal(core.cover("farMirror", renderer, farScene, mirror), true);
  core.views();
  assert.deepEqual([core.farInput[4], core.farInput[5]], [1, 1], "drawn over what is there");
  assert.deepEqual([core.farInput[skyItem().slot], core.farInput[bucketItem().slot]], [0, 1]);
  assert.deepEqual(calls.filter((c) => /^far(_mirror)?_frame$/.test(c[0])).map((c) => c[0]), ["far_mirror_frame", "far_mirror_frame"]);
  // The frame block's camera is the mirror's.
  assert.deepEqual([...core.input.subarray(32, 35)], [0, -4, 10]);
  // The eye's own far scene afterwards: cleared again.
  renderer.autoClear = true;
  sky.visible = true;
  core.cover("far", renderer, farScene, far.camera ?? mirror);
  core.views();
  assert.equal(core.farInput[5], 0);
});

const SAMPLER_CUBE = 0x8B60;
const FLYER = [["modelMatrix", MAT4], ["projectionMatrix", MAT4], ["reach", FLOAT], ["map", SAMPLER_2D], ["skyProbe", SAMPLER_CUBE]];

test("a cube map a material reads (the Enhanced sky probe) is named each frame and drawn by the renderer; none is never bound", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const probe = new THREE.CubeTexture();
  const flyer = mesh("sky-flyer", FLYER, { values: { reach: { value: 900 }, map: { value: new THREE.Texture() }, skyProbe: { value: probe } } });
  const classic = mesh("sky-flyer-classic", FLYER, { values: { reach: { value: 900 }, map: { value: new THREE.Texture() }, skyProbe: { value: null } } });
  farScene.add(flyer, classic);
  assert.equal(core.cover("far", renderer, farScene, camera), true);
  assert.equal(far.mode, "full");
  const item = far.all.find((i) => i.object === flyer), bare = far.all.find((i) => i.object === classic);
  const cube = (it) => calls.filter((c) => c[0] === "far_source" && c[1] === it.id).find((c) => c[3] === 10);
  assert.ok(cube(item) && cube(bare), "each its cube sampler's source");
  core.views();
  const at = cube(item)[5];
  assert.equal(core.farInput[at], core.textures.get(renderer.properties.get(probe).__webglTexture), "the probe's texture, registered");
  assert.equal(core.farInput[cube(bare)[5]], -1, "none: left unbound");
  assert.equal(core.farInput[6], 0, "no mipmaps: not a probe's face");
});

test("a sky probe's faces: each its own phase call, the mipmaps after the last face when the texture asks for them", () => {
  const { core, calls, renderer, farScene, far } = bridge();
  const sky = mesh("zero:sky", SKY, { renderOrder: -1e9, values: values(), matrixAutoUpdate: true });
  sky.layers.enable(3);
  farScene.add(sky);
  const target = { texture: new THREE.CubeTexture(), isWebGLCubeRenderTarget: true };
  target.texture.generateMipmaps = false;
  const cubeCamera = new THREE.CubeCamera(0.05, 50, target);
  for (const face of cubeCamera.children) face.layers.set(3);
  cubeCamera.updateMatrixWorld(true);
  cubeCamera.children.forEach((face, i) => {
    if (i === 5) target.texture.generateMipmaps = true;
    assert.equal(core.cover("probe", renderer, farScene, face), true);
    core.views();
    const texture = core.textures.get(renderer.properties.get(target.texture).__webglTexture);
    assert.equal(core.farInput[6], i === 5 ? 1 + texture : 0, `face ${i}`);
  });
  assert.deepEqual(calls.filter((c) => c[0] === "far_probe_frame").map((c) => c[1]), [0, 1, 2, 3, 4, 5]);
  assert.ok(far.all.some((i) => i.object === sky));
});

test("an unindexed mesh (the storm's bolt): an index of the pass's own, its range each frame, its rewritten vertices sent when three would upload them", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const geometry = new THREE.BufferGeometry();
  const position = new THREE.BufferAttribute(new Float32Array(6 * 3), 3).setUsage(THREE.DynamicDrawUsage);
  geometry.setAttribute("position", position);
  geometry.setDrawRange(0, 3);
  const material = new THREE.ShaderMaterial({ name: "storm-lightning", uniforms: { lightningFlash: { value: 0.5 } }, transparent: true, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, forceSinglePass: true, depthWrite: false });
  const bolt = new THREE.Mesh(geometry, material);
  bolt.frustumCulled = false;
  bolt.program = { id: "bolt", program: { uniforms: [{ name: "projectionMatrix", type: MAT4, size: 1 }, { name: "lightningFlash", type: FLOAT, size: 1 }], attributes: [{ name: "position", location: 0 }] } };
  farScene.add(bolt);
  assert.equal(core.cover("far", renderer, farScene, camera), true);
  const item = far.all.find((i) => i.object === bolt);
  assert.ok(item, "an item");
  const index = calls.find((c) => c[0] === "geometry_index");
  assert.deepEqual([...new Uint32Array(index[2].buffer, index[2].byteOffset, 6)], [0, 1, 2, 3, 4, 5], "0, 1, 2... over every vertex");
  core.views();
  assert.deepEqual([core.farInput[item.rangeAt], core.farInput[item.rangeAt + 1]], [0, 3]);
  assert.equal(calls.filter((c) => c[0] === "far_refresh").length, 1);
  assert.equal(core.farInput[item.refreshAt], 1, "sent the first time");
  // Unchanged: not sent; rewritten (a strike): sent again with its new range.
  core.cover("far", renderer, farScene, camera);
  core.views();
  assert.equal(core.farInput[item.refreshAt], 0);
  position.array.set([1, 2, 3], 0);
  position.needsUpdate = true;
  geometry.setDrawRange(0, 6);
  core.cover("far", renderer, farScene, camera);
  core.views();
  assert.equal(core.farInput[item.refreshAt], 1);
  assert.deepEqual([...core.farVertices.subarray(item.refresh[0].at, item.refresh[0].at + 3)], [1, 2, 3]);
  assert.deepEqual([core.farInput[item.rangeAt], core.farInput[item.rangeAt + 1]], [0, 6]);
});

test("the near ocean surface draws in Rust on its first frame", () => {
  const { core, calls, renderer, farScene, camera, far } = bridge();
  const near = new THREE.Scene();
  const sea = mesh("native-ocean", [["modelMatrix", MAT4], ["projectionMatrix", MAT4], ["time", FLOAT]], { values: { time: { value: 2 } }, matrixAutoUpdate: true });
  near.add(sea);
  // Its far twin shares its material (ocean-surface.js), as the stand-in's
  // program lookup needs.
  const twin = new THREE.Mesh(sea.geometry, sea.material);
  twin.program = sea.program;
  farScene.add(twin);
  core.graphics.scene = near;
  near.userData.water = { oceanSurface: { near: sea } };
  const sea_ = core.byName.sea;
  assert.ok(sea_, "the sea pass, in the near phase");
  sea_.draw({ renderer, scene: near, camera });
  core.views();
  assert.equal(core.farInput[4], 1, "registered and drawn immediately");
  assert.equal(sea.layers.mask, 1 << 30, "owned by the sea pass");
  sea_.draw({ renderer, scene: near, camera });
  core.views();
  assert.deepEqual([core.farInput[4], core.farInput[5], core.farInput[7]], [1, 1, 1], "drawn by the near phase, over what is there");
  const item = far.all.find((i) => i.object === sea);
  assert.equal(core.farInput[item.slot], 1);
  // The far phase's input is never the near phase's.
  core.cover("far", renderer, farScene, camera);
  core.views();
  assert.equal(core.farInput[7], 0);
});
