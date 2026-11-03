import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule } = await import("./support/rust-standin.mjs");
const { ParticleRenderer, particleRenderers, STREAK } = await import("../src/particles.js");
const { GpuEffects } = await import("../src/effects-gpu.js");
const { Lightning, classicLightning } = await import("../src/lightning.js");

const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0];
const GL = { ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89, SHADER_TYPE: 0x8b4f, VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30 };
// The frame's draws (effects.rs DRAW_*).
const DRAW_AT = 8 + 8 * 4, DRAW_STRIDE = 12;

// Programs as three's report their active uniforms and attributes (and,
// for a side of a double-sided material, its vertex shader's defines).
const program = (uniforms, attributes, vertex = "vertex") => ({ uniforms, attributes, shaders: [{ type: GL.VERTEX_SHADER, source: vertex }, { type: GL.FRAGMENT_SHADER, source: "fragment" }] });
const CPU_PROGRAM = program(["projectionMatrix", "modelViewMatrix", "map", "fade", "sceneDepth", "softParticles", "depthResolution", "cameraRange"],
  ["corner", "iPosition", "iColor", "iShape", "iAxis", "iAxis2", "iFrame"]);
const ANALYTIC_PROGRAM = program([...CPU_PROGRAM.uniforms, "tracks", "emitters", "trackRows", "emitterRows"],
  ["corner", "spawnPosition", "spawnVelocity", "spawnColor", "spawnShape", "spawnMeta"]);
const BASIC = ["projectionMatrix", "modelViewMatrix", "mapTransform", "diffuse", "opacity", "map"];

// The bridge over stand-ins for the WASM renderer (its memory, and a log of
// the calls made to it), three's renderer (its render lists, empty) and its
// context (enough of it for a program's uniforms, attributes and shaders).
function bridge() {
  const next = { program: 0, texture: 0, batch: 0, owner: 0, mesh: 0, model: 0, geometry: 0 };
  // The shared stand-in for the WASM renderer (every pass's views), with the
  // effects' registration calls as this test reads them.
  const { BfeRenderer, memory, calls } = standinModule({
    methods: ({ calls }) => ({
      transparent_bucket_shown() {},
      add_texture() { calls.push(["add_texture"]); return next.texture++; },
      set_far_map(texture) { calls.push(["set_far_map", texture]); },
      add_program(kind, vs, fs, attributes, uniforms) { calls.push(["add_program", kind, vs, attributes, uniforms]); return next.program++; },
      fx_add_owner(tracks, rows) { calls.push(["fx_add_owner", tracks, rows]); return next.owner++; },
      fx_add_batch(program, texture, blend, corner, attributes, owner) { calls.push(["fx_add_batch", program, texture, [...blend], corner, [...attributes], owner]); return next.batch++; },
      fx_batch_texture(batch, texture) { calls.push(["fx_batch_texture", batch, texture]); },
      fx_batch_values(batch, values) { calls.push(["fx_batch_values", batch, values]); },
      fx_batch_interleaved(batch) { calls.push(["fx_batch_interleaved", batch]); },
      fx_batch_side(batch, side) { calls.push(["fx_batch_side", batch, side]); },
      fx_batch_sampler(batch, name, unit, texture) { calls.push(["fx_batch_sampler", batch, name, unit, texture]); },
      fx_batch_base(batch, attributes, vertices, data, index) { calls.push(["fx_batch_base", batch, [...attributes], vertices, data.length, [...index]]); },
      fx_add_mesh(texture, blend, flags, func, sizes, values) { calls.push(["fx_add_mesh", texture, [...blend], flags, func, [...sizes], values]); return next.mesh++; },
      fx_mesh_pass(mesh, program, side, locations) { calls.push(["fx_mesh_pass", mesh, program, side, [...locations]]); },
      fx_mesh_texture(mesh, texture) { calls.push(["fx_mesh_texture", mesh, texture]); },
      fx_add_model(texture, blend, flags, func, values, sphere) { calls.push(["fx_add_model", texture, [...blend], flags, func, values, [...sphere]]); return next.model++; },
      fx_model_pass(model, program, side, geometry) { calls.push(["fx_model_pass", model, program, side, geometry]); },
      fx_model_sampler(model, name, unit, texture) { calls.push(["fx_model_sampler", model, name, unit, texture]); },
      add_geometry() { calls.push(["add_geometry"]); return next.geometry++; },
      geometry_buffer() { return 0; },
      geometry_pointer() {},
      geometry_index() {},
      geometry_group() {},
      sort() {},
      frame(records) { calls.push(["frame", records]); return STATS; },
      transparent_frame(three) { calls.push(["transparent_frame", three]); return STATS; },
    }),
  });
  const gl = {
    ...GL,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i] }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: (p) => p.shaders,
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => s.source,
  };
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene", width: 800, height: 600 }, farTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  const list = { opaque: [], transmissive: [], transparent: [] };
  const renderer = {
    renderLists: { get: () => list },
    getContext: () => gl,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initTexture() {},
    initRenderTarget() {},
    shadowMap: { enabled: false },
    getRenderTarget: () => sceneTarget,
    setRenderTarget() {},
    resetState() { calls.push(["resetState"]); },
  };
  const background = new THREE.Object3D();
  list.opaque.push({ object: background });
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, background, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  // A frame's transparent phase, as the transparent pass runs it.
  const frame = () => core.phase("transparent", renderer, scene, camera);
  // three has drawn a material once: its program (and its programs by side).
  const drawn = (material, current, programs = [current]) => {
    const props = renderer.properties.get(material);
    props.currentProgram = { program: current };
    props.programs = new Map(programs.map((p, i) => [i, { program: p }]));
    props.__version = material.version;
  };
  const entry = (d) => [...core.fxInput.subarray(DRAW_AT + d * DRAW_STRIDE, DRAW_AT + (d + 1) * DRAW_STRIDE)];
  return { core, calls, scene, camera, renderer, list, frame, drawn, entry, fx: core.byName.effects };
}

test("the effects are the transparent phase's second pass, after the map's blended buckets", () => {
  const { core } = bridge();
  assert.deepEqual(core.phases.transparent.map((p) => p.name), ["transparent", "effects", "water"]);
});

test("a CPU batch: registered once three drew it, then hidden from three and written each frame with its key", () => {
  const { core, calls, scene, frame, drawn, entry, fx } = bridge();
  const particles = new ParticleRenderer(scene);
  assert.ok(particleRenderers.get(scene).includes(particles));
  const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
  const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 2 * i, 3 * i, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
  fill(2);
  assert.equal(batch.mesh.visible, true);
  // Not drawn by three yet: nothing to register, nothing drawn.
  frame();
  assert.equal(calls.some((c) => c[0] === "fx_add_batch"), false);
  assert.equal(core.fxInput[0], 0);
  drawn(batch.material, CPU_PROGRAM);
  // Registered as three draws it this frame: drawn by three alone.
  frame();
  const added = calls.find((c) => c[0] === "fx_add_batch");
  assert.ok(added, "registered");
  assert.equal(calls.find((c) => c[0] === "add_program")[1], 4, "an effect program");
  assert.deepEqual(added.slice(3), [[1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], 0, [1, 3, 2, 4, 3, 4, 4, 3, 5, 3, 6, 4], -1]);
  assert.equal(core.fxInput[0], 0, "not drawn by the renderer as it is registered");
  assert.equal(batch.external, true);
  assert.equal(batch.mesh.visible, false);
  // The next frame: three is shown nothing and uploads nothing; the
  // renderer draws it, its instances written attribute by attribute.
  const version = batch.geometry.getAttribute("iPosition").version;
  fill(3);
  assert.equal(batch.mesh.visible, false);
  assert.equal(batch.geometry.getAttribute("iPosition").version, version);
  frame();
  const data = core.fxData;
  assert.equal(core.fxInput[0], 1, "one draw");
  // Kind, batch, instances, from, where, capacity; then three's key: group
  // order, render order, centre (the origin: no positions), id.
  assert.deepEqual(entry(0), [0, 0, 3, 0, 0, batch.capacity, 0, 10, 0, 0, 0, batch.mesh.id]);
  assert.deepEqual([...data.subarray(0, 9)], [0, 0, 0, 1, 2, 3, 2, 4, 6], "iPosition, its three instances");
  assert.deepEqual([...data.subarray(9, 13)], [1, 1, 1, 0.5], "then iColor");
  assert.equal(fx.draws, 1);
  assert.equal(fx.instances, 3);
  // Off: three draws it again, all of it uploaded anew.
  core.setActive(false);
  assert.equal(batch.external, false);
  assert.equal(batch.mesh.visible, true);
  const position = batch.geometry.getAttribute("iPosition");
  assert.equal(position.version, version + 1);
  assert.deepEqual(position.updateRanges, [{ start: 0, count: 9 }]);
  core.setActive(true);
  assert.equal(batch.external, true);
  assert.equal(batch.mesh.visible, false);
});

test("a mirror's render draws what blends too: the transparent phase for the mirror's camera, after its near phase", () => {
  const { core, calls, scene, renderer, list, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
  const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 2 * i, -3 * i - 5, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
  fill(2);
  drawn(batch.material, CPU_PROGRAM);
  core.phase("transparent", renderer, scene, core.graphics.camera);
  assert.equal(batch.external, true, "registered by the frame's own render");
  // water.js's mirror: a camera of its own drawing into a target of its own.
  const mirror = new THREE.PerspectiveCamera();
  mirror.userData.mirror = true;
  mirror.position.set(0, -2, 0);
  mirror.updateMatrixWorld(true);
  const reflection = { name: "reflection", width: 400, height: 300 };
  const target = renderer.getRenderTarget;
  renderer.getRenderTarget = () => reflection;
  try {
    fill(3);
    calls.length = 0;
    // The far scene's image is the last opaque item: the mirror's
    // transparent phase follows its near phase at once.
    core.pass(renderer, scene, mirror);
  } finally {
    renderer.getRenderTarget = target;
  }
  const names = calls.map((c) => c[0]);
  assert.ok(names.includes("mirror_frame"), "the near phase's passes for the mirror");
  assert.ok(names.indexOf("mirror_transparent_frame") > names.indexOf("mirror_frame"), "then what blends");
  assert.equal(names.includes("transparent_frame"), false, "not the frame's own");
  assert.equal(core.fxInput[0], 1, "the effects drawn in the reflection");
  assert.deepEqual(entry(0).slice(0, 3), [0, 0, 3]);
});

test("an analytic batch: its owner's emitters, and its instances from the first changed", () => {
  const { core, calls, scene, frame, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const def = { geometry: { type: "particle", texture: "spark", blend: "additive" }, transformer: { lifetime: 1, position: [], size: [], color: [] }, registerStep: true };
  const gpu = new GpuEffects({ spark: [def] }, particles);
  assert.equal(particles.gpu, gpu);
  const texture = new THREE.Texture();
  const effect = { position: new THREE.Vector3(1, 2, 3), quaternion: new THREE.Quaternion(), tint: 1, time: 5, scale: 1 };
  const made = Array.from({ length: 4 }, (_, i) => ({ position: new THREE.Vector3(i, 0, 0), velocity: new THREE.Vector3(), color: [1, 1, 1, 1], size: 1, rotation: 0, spin: 0, birth: i, life: 1 }));
  let batch = null;
  const fill = (list) => {
    particles.begin(); gpu.begin();
    const slot = gpu.register(effect);
    batch = gpu.batch(def, texture);
    for (const p of list) batch.pushParticle(p, slot, gpu.definitions.get(def), gpu.epochs[slot]);
    particles.end(); gpu.end();
  };
  fill(made);
  drawn(batch.material, ANALYTIC_PROGRAM);
  frame();
  assert.ok(calls.find((c) => c[0] === "fx_add_owner"), "its owner registered");
  assert.equal(calls.find((c) => c[0] === "fx_add_batch")[6], 0, "with its owner");
  // Drawn: every instance, and the emitter row of the effect playing.
  fill(made);
  frame();
  const input = core.fxInput, data = core.fxData;
  assert.deepEqual([...input.subarray(8, 12)], [0, gpu.uniforms.emitterRows.value, 1, 0], "the owner's rows, one written at 0");
  assert.deepEqual([...data.subarray(0, 3)], [1, 2, 3], "the effect's place");
  assert.deepEqual(entry(0).slice(0, 5), [0, 0, 4, 0, 12], "all four instances, after the emitter row");
  // Nothing changed: none written.
  fill(made);
  frame();
  assert.deepEqual(entry(0).slice(2, 4), [4, 4]);
  // The third changed (over two frames, the first not drawn): from it on.
  fill([made[0], made[1], { ...made[2], birth: 9 }, made[3]]);
  fill([made[0], made[1], { ...made[2], birth: 9 }, made[3]]);
  frame();
  assert.deepEqual(entry(0).slice(2, 4), [4, 2]);
  assert.deepEqual([...data.subarray(12, 15)], [2, 0, 0], "spawnPosition of the third, first");
});

test("the Classic lightning: both of three's sides, its buffers as far as used, its material's values and key", () => {
  const { core, calls, scene, camera, frame, drawn, entry } = bridge();
  const settings = { get: (name) => (name === "arcCasterEffects" ? "classic" : undefined) };
  const lightning = new Lightning(scene, () => new THREE.Texture(), { settings });
  assert.equal(classicLightning.get(scene), lightning);
  const def = { Texture: "arc", FadeTime: 1, InterpolationSpeed: 0, Width: 0.1, SpreadFactor: 0.2, BreakDistance: 1, TextureSize: 1, Color: [255, 128, 64, 255],
    NumChildren: 0, ChildColor: [255, 255, 255, 255], ChildWidth: 0.05, ChildSpreadFactor: 0.1, ChildBreakDistance: 1, ChildTextureSize: 1,
    BranchFactor: 0, MaxBranches: 0, BranchLength: 0, BranchSpreadFactor: 0 };
  lightning.spawn(def, new THREE.Vector3(0, 0, -5), new THREE.Vector3(4, 0, -5), 1);
  lightning.update(1 / 60, camera);
  const material = lightning.mesh.material;
  assert.ok(lightning.index > 0 && lightning.mesh.visible);
  // three draws it first, back faces then front, each with its program.
  const back = program(BASIC, ["position", "uv", "color"], "#define FLIP_SIDED\nvertex"), front = program(BASIC, ["uv", "color", "position"]);
  drawn(material, front, [back, front]);
  lightning.mesh.geometry.computeBoundingSphere();
  frame();
  const mesh = calls.find((c) => c[0] === "fx_add_mesh");
  assert.deepEqual(mesh.slice(2), [[1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], 1, 0x0203, [3, 2, 4], "diffuse:3,opacity:1,mapTransform:9"]);
  assert.deepEqual(calls.filter((c) => c[0] === "fx_mesh_pass").map((c) => c.slice(2)), [[0, 1, [0, 1, 2]], [1, 0, [2, 0, 1]]], "back then front, at each program's locations");
  assert.equal(core.fxInput[0], 0, "three drew it this frame");
  assert.equal(lightning.external, true);
  // The next frame it is the renderer's.
  lightning.update(1 / 60, camera);
  assert.equal(lightning.mesh.visible, false);
  frame();
  const e = entry(0), sphere = lightning.mesh.geometry.boundingSphere;
  assert.deepEqual(e.slice(0, 5), [1, 0, lightning.vertex, lightning.index, 0]);
  assert.deepEqual(e.slice(6), [0, 5, sphere.center.x, sphere.center.y, sphere.center.z, lightning.mesh.id], "group order, render order 5, its bounding sphere's centre, id");
  const data = core.fxData, bits = core.fxBits, v = lightning.vertex, n = lightning.index;
  assert.deepEqual([...data.subarray(0, 3)], [...lightning.positions.subarray(0, 3)].map(Math.fround), "positions first");
  const indexAt = v * 9;
  assert.deepEqual([...bits.subarray(indexAt, indexAt + n)], [...lightning.indices.subarray(0, n)], "then the indices' bits");
  assert.deepEqual([...data.subarray(indexAt + n, indexAt + n + 4)], [1, 1, 1, 1], "then the diffuse colour and opacity");
  assert.deepEqual([...data.subarray(indexAt + n + 4, indexAt + n + 13)], lightning.mesh.material.map.matrix.elements, "and the map's transform (three's, its -0 too)");
  // Off: three draws it again.
  core.setActive(false);
  assert.equal(lightning.external, false);
  assert.equal(lightning.mesh.visible, true);
});

test("glow models: found in three's list, registered with their group once three drew them, then each part drawn with its world matrix", () => {
  const { core, calls, scene, list, frame, drawn, entry } = bridge();
  // As game.js glowModel makes one: a material its parts share, on the group.
  const material = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
  material.color.setRGB(1, 0.5, 0.25);
  material.opacity = 0.75;
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(6), 2));
  geometry.setIndex([0, 1, 2]);
  const group = new THREE.Group(), part = new THREE.Mesh(geometry, material), other = new THREE.Mesh(geometry, material);
  group.add(part, other);
  group.userData.material = material;
  group.position.set(1, 2, -6);
  scene.add(group);
  scene.updateMatrixWorld(true);
  list.transparent.push({ object: part, material, group: null });
  // In three's list, not drawn by it yet: nothing.
  frame();
  assert.equal(calls.some((c) => c[0] === "fx_add_model"), false);
  const back = program(BASIC, ["position", "uv"], "#define FLIP_SIDED\nvertex"), front = program(BASIC, ["position", "uv"]);
  drawn(material, front, [back, front]);
  frame();
  const models = calls.filter((c) => c[0] === "fx_add_model");
  assert.equal(models.length, 1, "one model serves both parts (one geometry, programs, texture, state)");
  assert.deepEqual(models[0].slice(2, 6), [[1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], 1, 0x0203, "diffuse:3,opacity:1,mapTransform:9"]);
  assert.deepEqual(calls.filter((c) => c[0] === "fx_model_pass").map((c) => c.slice(3)), [[1, 0], [0, 1]], "back then front, a geometry for each program");
  assert.equal(calls.find((c) => c[0] === "add_program")[1], 0, "world programs: the model-view matrix is set per draw");
  assert.equal(material.visible, false, "hidden from three");
  assert.equal(core.fxInput[0], 0, "three drew them this frame");
  list.transparent.length = 0;
  frame();
  assert.equal(core.fxInput[0], 2, "both parts");
  const e = entry(0);
  assert.deepEqual(e.slice(0, 4), [2, 0, 0, 0]);
  assert.deepEqual([...core.fxMatrices.subarray(0, 16)], part.matrixWorld.elements);
  assert.deepEqual([...core.fxData.subarray(0, 4)], [1, 0.5, 0.25, 0.75].map(Math.fround), "its colour and opacity");
  assert.deepEqual(e.slice(6), [0, 0, 1.5, 2.5, -6, part.id], "three's key: the group's order, its sphere's centre in the world");
  assert.equal(entry(1)[2], 16, "the second part's matrix after the first's");
  // Hidden part: not drawn.
  other.visible = false;
  frame();
  assert.equal(core.fxInput[0], 1);
  core.setActive(false);
  assert.equal(material.visible, true);
});

test("the effects' programs are their own, apart from another pass's for the same three program, whose values can't leak in", () => {
  const { core, calls, scene, list, frame, drawn, entry } = bridge();
  const material = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(6), 2));
  geometry.setIndex([0, 1, 2]);
  const group = new THREE.Group(), part = new THREE.Mesh(geometry, material);
  group.add(part);
  group.userData.material = material;
  group.position.set(0, 0, -6);
  scene.add(group);
  scene.updateMatrixWorld(true);
  const shared = program(BASIC, ["position", "uv"]);
  drawn(material, shared);
  // Another pass (the transparent pass's command post beams) made one for
  // the same three program first, and sets its colour and opacity itself.
  const theirs = core.program(core.renderer.properties.get(material).currentProgram, 0, material, { uniforms: new Set(BASIC), material: new Set() });
  list.transparent.push({ object: part, material, group: null });
  frame();
  assert.equal(calls.filter((c) => c[0] === "add_program").length, 2, "made again for the effects");
  const pass = calls.find((c) => c[0] === "fx_model_pass");
  assert.notEqual(pass[2], theirs.id, "the glow drawn with its own");
  list.transparent.length = 0;
  frame();
  assert.equal(entry(0)[0], 2, "drawn");
});

test("a droideka's shield: its uniforms as the model's values, the frame's own left to the frame block, its onBeforeRender run", async () => {
  const { ShieldBubble } = await import("../src/droideka-shield.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const bubble = new ShieldBubble(scene, { Radius: 2 }, () => null, { get: () => "classic" });
  const droideka = { position: new THREE.Vector3(0, 0, -8), aimYaw: 0, shield: { on: true, def: {}, value: 1, max: 1 }, dead: false, rolled: false };
  bubble.update(0.1, droideka);
  scene.updateMatrixWorld(true);
  const mesh = bubble.mesh, material = mesh.material;
  assert.ok(mesh.visible);
  list.transparent.push({ object: mesh, material, group: null });
  const shader = program(["projectionMatrix", "modelViewMatrix", "normalMatrix", "viewMatrix", "fieldMap", "hasMap", "shieldTime", "shieldFade", "charge", "enhanced", "birth", "inside", "flash",
    "fieldColor", "specularColor", "sunDirection", "sunColor", "impacts[0]", "powers[0]", "blasts[0]"], ["position", "normal", "uv"]);
  drawn(material, shader);
  frame();
  const model = calls.find((c) => c[0] === "fx_add_model");
  assert.ok(model, "registered");
  assert.equal(model[1], -1, "no texture: the field has no map");
  assert.deepEqual(model[2], [1, 0x8006, 0x8006, 0x0302, 0x0303, 1, 0x0303], "normal blending");
  assert.equal(model[5], "hasMap:1:0,shieldTime:1:0,shieldFade:1:0,charge:1:0,enhanced:1:0,birth:1:0,inside:1:0,flash:1:0,fieldColor:3:0,specularColor:3:0,impacts:24:4,powers:6:1,blasts:6:1",
    "its own values; the sun's from the frame block");
  assert.deepEqual(calls.filter((c) => c[0] === "fx_model_pass").map((c) => c[3]), [2], "one pass, both sides");
  assert.equal(material.visible, false);
  // Next frame: drawn, its values written from its uniforms after its
  // onBeforeRender (the camera far outside: inside 0).
  list.transparent.length = 0;
  material.uniforms.inside.value = 1;
  frame();
  assert.equal(core.fxInput[0], 1);
  assert.deepEqual(entry(0).slice(0, 2), [2, 0]);
  const data = core.fxData;
  assert.equal(data[6], 0, "inside, as its onBeforeRender set it");
  assert.equal(data[2], Math.fround(material.uniforms.shieldFade.value), "its fade");
  assert.deepEqual(entry(0).slice(6, 8), [0, 4], "three's key: render order 4");
});

test("the atmosphere's squares: a batch with values of its own after its instances", async () => {
  const { Atmosphere, squareBatches } = await import("../src/atmosphere.js");
  const { core, calls, scene, camera, frame, drawn, entry } = bridge();
  const atmosphere = new Atmosphere({ data: { fogCloud: { texture: "fog", color: [255, 128, 64], size: 20 } }, scene, collision: { cast: () => null }, texture: () => new THREE.Texture() });
  const [batch] = squareBatches.get(scene);
  assert.equal(batch, atmosphere.fog.batch);
  batch.begin(); batch.push(10, 1, 2, -10, 4, 0.5, 0.75); batch.push(5, 3, 2, -5, 4, 0.25, 0.5); batch.end();
  assert.equal(batch.mesh.visible, true);
  drawn(batch.material, program(["projectionMatrix", "viewMatrix", "map", "tint", "axisU", "axisV", "fade", "sceneDepth", "softParticles", "depthResolution", "cameraRange"], ["corner", "center", "spin"]));
  frame();
  assert.deepEqual(calls.find((c) => c[0] === "fx_batch_values").slice(2), ["tint:4:0,axisU:3:0,axisV:3:0"]);
  assert.equal(batch.external, true);
  batch.begin(); batch.push(10, 1, 2, -10, 4, 0.5, 0.75); batch.push(5, 3, 2, -5, 4, 0.25, 0.5); batch.end();
  assert.equal(batch.mesh.visible, false, "hidden from three");
  frame();
  const e = entry(0), data = core.fxData;
  assert.deepEqual(e.slice(0, 5), [0, 0, 2, 0, 0]);
  assert.deepEqual(e.slice(6, 8), [0, 10], "render order 10");
  assert.deepEqual([...data.subarray(0, 8)], [1, 2, -10, 4, 3, 2, -5, 4], "centres, farthest first as atmosphere.js sorts them");
  assert.deepEqual([...data.subarray(8, 12)], [0.5, 0.75, 0.25, 0.5].map(Math.fround), "spins");
  assert.deepEqual([...data.subarray(12, 16)], [1, 128 / 255, 64 / 255, 1].map(Math.fround), "then the tint");
  core.setActive(false);
  assert.equal(batch.external, false);
  assert.equal(batch.mesh.visible, true);
});

test("the snow: no instances of its own, its flakes and boxes in data textures on units of their own, an int value", async () => {
  const { Snow } = await import("../src/snow.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const snow = new Snow({ data: { precipitation: { ParticleDensity: [4], Range: [10] } }, scene, collision: { cast: () => null }, texture: () => new THREE.Texture() });
  camera.position.set(0, 2, 0); camera.updateMatrixWorld(true);
  snow.update(1 / 60, camera);
  const mesh = snow.mesh;
  assert.ok(mesh.visible && mesh.geometry.instanceCount > 0);
  list.transparent.push({ object: mesh, material: mesh.material, group: null });
  drawn(mesh.material, program(["projectionMatrix", "viewMatrix", "flakes", "boxes", "flakeCount", "size", "cameraRight", "cameraUp", "map", "cover", "sceneDepth", "tint", "coverCell", "coverSize", "softParticles", "depthResolution", "cameraRange"], ["corner"]));
  frame();
  const added = calls.find((c) => c[0] === "fx_add_batch");
  assert.deepEqual(added[5], [], "no instance attributes");
  assert.deepEqual(calls.find((c) => c[0] === "fx_batch_values").slice(2), ["flakeCount:1:2,size:1:0,cameraRight:3:0,cameraUp:3:0,tint:4:0,coverCell:1:0,coverSize:1:0"]);
  assert.deepEqual(calls.filter((c) => c[0] === "fx_batch_sampler").map((c) => c.slice(2, 4)), [["flakes", 7], ["boxes", 8], ["cover", 9]]);
  assert.equal(mesh.material.visible, false, "hidden by its material");
  list.transparent.length = 0;
  snow.update(1 / 60, camera);
  frame();
  const e = entry(0);
  assert.deepEqual(e.slice(0, 5), [0, 0, mesh.geometry.instanceCount, 0, 0]);
  assert.deepEqual(e.slice(6, 8), [0, 20], "render order 20");
  assert.equal(core.fxData[0], 4, "its flake count first");
  core.setActive(false);
  assert.equal(mesh.material.visible, true);
});

test("the Enhanced bolts: a batch whose pixel angle its onBeforeRender sets, as three would call it", async () => {
  const { BlasterBatch } = await import("../src/blaster-bolts.js");
  const { core, calls, scene, camera, frame, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const batch = particles.custom("blaster", () => new BlasterBatch());
  const fill = () => { particles.begin(); batch.push(new THREE.Vector3(0, 0, -5), new THREE.Vector3(0, 0, -8), [1, 0.2, 0.1], 1, 0.02, 0.1, 0.5, 0.6); particles.end(); };
  fill();
  drawn(batch.material, program(["projectionMatrix", "modelViewMatrix", "pixelAngle", "fade", "time", "sceneDepth", "softParticles", "depthResolution", "cameraRange"], ["corner", "iHead", "iTail", "iColor", "iSize"]));
  frame();
  assert.deepEqual(calls.find((c) => c[0] === "fx_add_batch")[3], [1, 0x8006, 0x8006, 1, 0x0303, 1, 0x0303], "three's custom blending: one, one minus source alpha");
  assert.deepEqual(calls.find((c) => c[0] === "fx_batch_values").slice(2), ["pixelAngle:1:0"]);
  fill();
  assert.equal(batch.mesh.visible, false);
  batch.material.uniforms.pixelAngle.value = 0;
  frame();
  const e = entry(0);
  assert.deepEqual(e.slice(0, 5), [0, 0, 1, 0, 0]);
  assert.deepEqual(e.slice(6, 8), [0, 11], "render order 11");
  const angle = (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / 600;
  assert.equal(core.fxData[14], Math.fround(angle), "its pixel angle after its 14 floats of instance, as onBeforeRender set it for the target's height");
});

test("the rain: fixed seeds uploaded once on a streak plane of its own, its clock its own", async () => {
  const { Weather } = await import("../src/weather.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const settings = { get: () => "classic", onChange: () => () => {} };
  const weather = new Weather({ data: { precipitation: { Type: ["Streaks"], ParticleDensity: [64] } }, scene, collision: { cast: () => null }, water: null, settings });
  weather.update(1 / 60, camera);
  const mesh = weather.mesh;
  assert.ok(mesh.visible);
  assert.equal(mesh.material.uniforms.rainTime.value, 1 / 60, "its own clock");
  list.transparent.push({ object: mesh, material: mesh.material, group: null });
  drawn(mesh.material, program(["projectionMatrix", "viewMatrix", "eye", "cameraMotion", "wind", "rainTime", "range", "velocity", "velocityRange", "width", "streak", "enhanced", "effect",
    "cover", "coverCell", "coverSize", "color", "ambient", "alpha", "sceneDepth", "softParticles", "depthResolution", "cameraRange"], ["position", "uv", "seed"]));
  frame();
  const added = calls.find((c) => c[0] === "fx_add_batch");
  assert.equal(added[4], 0xffffffff, "no corner quad");
  assert.deepEqual(added[5], [2, 4], "its seed, per instance");
  const base = calls.find((c) => c[0] === "fx_batch_base");
  assert.deepEqual(base.slice(2, 4), [[0, 3, 1, 2], 4], "position and uv of its four vertices");
  assert.equal(base[5].length, 6, "its six indices");
  assert.equal(mesh.material.visible, false);
  list.transparent.length = 0;
  frame();
  assert.deepEqual(entry(0).slice(0, 5), [0, 0, 64, 0, 0], "every seed the first time");
  frame();
  assert.deepEqual(entry(0).slice(2, 4), [64, 64], "none again");
});

test("the Enhanced rain: its mist the rain's own batch, its splashes uploaded as they land, its sheets a model never culled", async () => {
  const { Weather } = await import("../src/weather.js");
  const { particleDepth } = await import("../src/particle-depth.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const settings = { get: () => "enhanced", onChange: () => () => {} };
  const weather = new Weather({ data: { precipitation: { Type: ["Streaks"], ParticleDensity: [64] } }, scene, collision: { cast: () => null }, water: null, settings });
  weather.update(1 / 60, camera);
  const mist = weather.mist, impacts = weather.impacts, sheets = weather.sheets.mesh;
  assert.ok(mist.visible && impacts.mesh.visible && sheets.visible);
  for (const object of [mist, impacts.mesh, sheets]) list.transparent.push({ object, material: object.material, group: null });
  const soft = ["sceneDepth", "softParticles", "depthResolution", "cameraRange"];
  drawn(mist.material, program(["projectionMatrix", "viewMatrix", "eye", "cameraMotion", "wind", "rainTime", "range", "velocity", "velocityRange", "width", "streak", "enhanced", "effect",
    "cover", "coverCell", "coverSize", "color", "ambient", "alpha", ...soft], ["position", "uv", "seed"]));
  drawn(impacts.mesh.material, program(["projectionMatrix", "viewMatrix", "impactTime", ...soft], ["position", "uv", "impact", "surfaceNormal", "kind"]));
  drawn(sheets.material, program(["projectionMatrix", "viewMatrix", "eye", "radii", "streaks", "cover", "rainTime", "speed", "coverCell", "coverSize", "strength", "wind", "color", "ambient", ...soft], ["sheet"]));
  frame();
  const batches = calls.filter((c) => c[0] === "fx_add_batch");
  assert.equal(batches.length, 2, "the mist and the splashes");
  assert.deepEqual(batches[1][5], [2, 4, 3, 3, 4, 1], "the splashes' impact, surface normal and kind, per instance");
  assert.equal(mist.material.visible, false, "the mist hidden by its material");
  assert.equal(impacts.external, true, "the splashes by their own flag");
  weather.update(1 / 60, camera);
  assert.equal(impacts.mesh.visible, false, "and so kept from three");
  const model = calls.find((c) => c[0] === "fx_add_model");
  assert.ok(model, `the sheets registered (${core.errors.join("; ")})`);
  assert.deepEqual(model[6], [0, 0, 0, -1], "the sheets: never culled");
  assert.ok(model[5].includes("rainTime:1:0") && !model[5].includes("ambient"), "its clock the rain's, the ambient light the frame's");
  assert.deepEqual(calls.filter((c) => c[0] === "fx_model_sampler").map((c) => c.slice(2, 4)), [["cover", 9]], "the streaks on the map's unit, the cover on its own");
  list.transparent.length = 0;
  particleDepth.map.value = new THREE.Texture();
  try {
    frame();
    assert.equal(core.fxInput[0], 3);
    assert.ok(core.fxInput[7] >= 0, "the scene's depth bound: the sheets read it");
    const splashes = [0, 1, 2].map(entry).find((e) => e[0] === 0 && e[1] === 1);
    assert.deepEqual(splashes.slice(2, 4), [192, 0], "every splash the first time");
    frame();
    assert.deepEqual([0, 1, 2].map(entry).find((e) => e[0] === 0 && e[1] === 1).slice(2, 4), [192, 192], "none while none landed");
    impacts.mesh.geometry.attributes.impact.needsUpdate = true;
    frame();
    assert.deepEqual([0, 1, 2].map(entry).find((e) => e[0] === 0 && e[1] === 1).slice(2, 4), [192, 0], "all again once one did");
  } finally {
    particleDepth.map.value = null;
  }
  core.setActive(false);
  assert.equal(impacts.external, false);
  assert.equal(sheets.material.visible, true);
});

test("the steam's wisps and the wake splashes: single-sided batches of fixed seeds; the steam's volumes a model over the screen", async () => {
  const { SteamWisps } = await import("../src/steam-wisps.js");
  const { FountainSpray } = await import("../src/fountain-spray.js");
  const { EnvironmentVolume, EnvironmentWakes } = await import("../src/environment-volume.js");
  const { particleDepth } = await import("../src/particle-depth.js");
  const { core, calls, scene, list, frame, drawn, entry } = bridge();
  const wakes = new EnvironmentWakes();
  const wisps = new SteamWisps(scene, wakes, new THREE.Texture()), spray = new FountainSpray(scene, wakes), volume = new EnvironmentVolume(scene, 2, wakes);
  const vent = { position: new THREE.Vector3(0, 0, -6), direction: new THREE.Vector3(0, 1, 0), extent: new THREE.Vector3(6, 5.5, 6), kind: 0 };
  wisps.update(1, [vent]); spray.update(1, [{ slot: 0, position: new THREE.Vector3(0, 0, -6) }]); volume.update(1, [vent]);
  for (const object of [wisps.mesh, spray.mesh, volume.mesh]) list.transparent.push({ object, material: object.material, group: null });
  const depth = ["sceneDepth", "depthResolution", "cameraRange", "hasDepth"];
  drawn(wisps.mesh.material, program(["projectionMatrix", "viewMatrix", "map", "wispTime", "sources[0]", "directions[0]", "wakes[0]", "motion[0]", ...depth], ["position", "uv", "seed"]));
  drawn(spray.mesh.material, program(["projectionMatrix", "viewMatrix", "sprayTime", "sources[0]", "wakes[0]", "motion[0]", ...depth], ["position", "uv", "seed"]));
  drawn(volume.mesh.material, program(["volumeMap", "sceneDepth", "volumeResolution", "depthResolution", "cameraRange", "enabled"], ["position"]));
  frame();
  const values = calls.filter((c) => c[0] === "fx_batch_values").map((c) => c[2]);
  assert.deepEqual(values, ["wispTime:1:0,sources:36:4,directions:36:4,wakes:32:4,motion:32:4,hasDepth:1:0", "sprayTime:1:0,sources:20:4,wakes:32:4,motion:32:4,hasDepth:1:0"]);
  assert.deepEqual(calls.filter((c) => c[0] === "fx_batch_side").map((c) => c[2]), [0, 0], "front faces only, as three draws them");
  const model = calls.find((c) => c[0] === "fx_add_model");
  assert.ok(model, `the volumes registered (${core.errors.join("; ")})`);
  assert.ok(model[1] >= 0, "the volumes' image on the map's unit");
  assert.equal(model[5], "volumeResolution:2:0,depthResolution:2:0,cameraRange:2:0,enabled:1:0");
  assert.deepEqual(model[6], [0, 0, 0, -1]);
  assert.equal(volume.mesh.material.visible, false);
  list.transparent.length = 0;
  particleDepth.map.value = new THREE.Texture();
  try {
    frame();
    assert.equal(core.fxInput[0], 3);
    assert.equal(core.fxInput[2], 0, "soft particles off");
    assert.ok(core.fxInput[7] >= 0, "yet the scene's depth bound: they read it whatever the setting");
    assert.deepEqual([0, 1, 2].map((d) => entry(d)[0]).sort(), [0, 0, 2]);
  } finally {
    particleDepth.map.value = null;
  }
});

test("the fountains' jets: batches on bases of their own after three's copy of the frame so far; their spray halves composited", async () => {
  const { FountainJets } = await import("../src/fountain-jets.js");
  const { FountainSpray } = await import("../src/fountain-spray.js");
  const { EnvironmentWakes } = await import("../src/environment-volume.js");
  const { core, calls, scene, list, frame, drawn, renderer } = bridge();
  const spray = new FountainSpray(new THREE.Group(), new EnvironmentWakes());
  const jets = new FountainJets(scene, spray, core.graphics, { foam: new THREE.Texture(), tier: true });
  jets.update(1, [{ slot: 0, position: new THREE.Vector3(0, 0, -8), level: 0.8, low: 0 }]);
  const sky = ["sunDirection", "sunColor", "ambient", "ambientTop", "ambientBottom", "fade"];
  const shared = ["projectionMatrix", "viewMatrix", "jetTime", "side", "perFountain", "sources[0]", "levels[0]", "lows[0]", "visible[0]", "wind",
    "atlas", "foamMap", "hasFoam", "sceneDepth", "depthResolution", "cameraRange", "hasDepth", "sceneColor", "sceneResolution", "hasScene", "fragScale", ...sky];
  const base = { "fountain-pool": ["corner", "seed"], "fountain-jets": ["corner", "seed"], "fountain-jets-near": ["corner", "seed"], "fountain-ropes": ["strip", "rope"],
    "fountain-ropes-near": ["strip", "rope"], "fountain-jets-core": ["sweep"], "fountain-overflow": ["film"], "fountain-overflow-near": ["film"] };
  for (const mesh of jets.meshes) {
    list.transparent.push({ object: mesh, material: mesh.material, group: null });
    if (mesh.userData.spray) drawn(mesh.material, program(["sprayMap", "sceneDepth", "sprayResolution", "depthResolution", "cameraRange", "enabled"], ["position"]));
    // The column's material is the shared values' own: no side, no blocks.
    else drawn(mesh.material, program(mesh === jets.core ? shared.filter((n) => n !== "side" && n !== "perFountain") : shared, base[mesh.name]));
  }
  frame();
  assert.equal(calls.filter((c) => c[0] === "fx_add_batch").length, 8, `eight batches (${core.errors.join("; ")})`);
  assert.deepEqual(calls.filter((c) => c[0] === "fx_batch_side").map((c) => c[2]), [0], "the column single-sided");
  const bases = calls.filter((c) => c[0] === "fx_batch_base");
  assert.equal(bases.length, 8);
  assert.deepEqual(bases[0].slice(2, 4), [[0, 2], 4], "the pool's corner quad, its own base");
  assert.equal(calls.filter((c) => c[0] === "fx_add_model").length, 2, "the two halves of soft spray");
  const values = calls.find((c) => c[0] === "fx_batch_values")[2];
  assert.ok(values.includes("jetTime:1:0") && values.includes("visible:5:1") && values.includes("sources:20:4") && !values.includes("sunDirection"));
  assert.ok(jets.meshes.every((m) => m.material.visible === false));
  assert.ok(jets.copier.visible && jets.copier.material.visible, "the copier left to three, just before the jets");
  assert.ok(jets.copier.renderOrder < jets.meshes[0].renderOrder);
  list.transparent.length = 0;
  frame();
  assert.equal(core.fxInput[0], 10, "eight batches and two composites");
  assert.deepEqual([jets.behind.width, jets.behind.height], [800, 600], "the copy's target at the frame's size");
  assert.equal(jets.core.material.uniforms.hasScene.value, 1);
  assert.notEqual(jets.meshes.find((m) => m.userData.spray).material.uniforms.sprayMap.value, jets.meshes.findLast((m) => m.userData.spray).material.uniforms.sprayMap.value, "a target each half");
  assert.equal(renderer.getRenderTarget().name, "scene");
});

test("a dissolving body's heated record, an untextured gun and a scrolling rig: what was left to three, drawn with their own values", async () => {
  const { heatMaterial } = await import("../src/destruction-debris.js");
  const { lighting } = await import("../src/materials.js");
  const { core, calls, scene, camera, renderer, drawn } = bridge();
  renderer.info = { render: { frame: 0 } };
  const inst = core.byName.instances;
  inst.rigsOn = true;
  const R = { TEXTURES: 4, LOOK: 52, EXTRA: 76 }, at = (r) => core.recordsAt + r * core.stride;
  const look = () => ({ map: { value: new THREE.Texture() }, sunScale: { value: 1 }, ambientPull: { value: new THREE.Vector2() }, brightness: { value: 1 }, scroll: { value: new THREE.Vector2() } });
  const shaders = { vertexShader: "#include <common>\nvoid main() {\n#include <begin_vertex>\n}", fragmentShader: "void main() { vec4 color = vec4(1.0); gl_FragColor = color; }" };
  const skinnedGeometry = () => {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
    g.setAttribute("skinIndex", new THREE.BufferAttribute(new Uint16Array(12), 4));
    g.setAttribute("skinWeight", new THREE.BufferAttribute(new Float32Array(12).fill(0.25), 4));
    g.setIndex([0, 1, 2]);
    return g;
  };
  const skinned = (material) => {
    const mesh = new THREE.SkinnedMesh(skinnedGeometry(), material), bone = new THREE.Bone();
    mesh.add(bone);
    mesh.bind(new THREE.Skeleton([bone]));
    mesh.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1);
    return mesh;
  };
  const SKINNED = ["projectionMatrix", "modelViewMatrix", "normalMatrix", "map", "boneTexture", "bindMatrix", "bindMatrixInverse", "sunScale", "ambientPull", "brightness"];
  // A droideka's body, a gun without a map in its hand, and a rig with a scrolling texture.
  const own = new THREE.ShaderMaterial({ defines: { RIGID_SKIN: "" }, uniforms: look(), ...shaders });
  drawn(own, program(SKINNED, ["position", "skinIndex", "skinWeight"]));
  const body = skinned(own), root = new THREE.Group(), hand = new THREE.Group(), gun = new THREE.Group();
  body.userData.poseSphere = true; // a soldier's (actor.js): not a rig
  const gunMaterial = new THREE.ShaderMaterial({ defines: { DYNAMIC: "" }, uniforms: { ...look(), map: { value: null } } });
  drawn(gunMaterial, program(["projectionMatrix", "modelViewMatrix", "normalMatrix", "map", "sunScale", "ambientPull", "brightness"], ["position"]));
  const gunGeometry = new THREE.BufferGeometry();
  gunGeometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  gunGeometry.setIndex([0, 1, 2]);
  gun.add(new THREE.Mesh(gunGeometry, gunMaterial));
  hand.add(gun);
  root.add(body, hand);
  root.position.z = -5;
  root.userData.parts = { body, hand };
  scene.add(root);
  core.graphics.cullRoots.push(root);
  const scrolling = new THREE.ShaderMaterial({ defines: { UNLIT: "", SCROLL: "" }, uniforms: { ...look(), time: lighting.time } });
  scrolling.uniforms.scroll.value.set(0.25, -0.5);
  drawn(scrolling, program([...SKINNED, "scroll", "time"], ["position", "skinIndex", "skinWeight"]));
  const rig = skinned(scrolling);
  rig.position.set(2, 0, -6);
  scene.add(rig);
  const near = () => { scene.updateMatrixWorld(true); renderer.info.render.frame++; core.pass(renderer, scene, camera); };
  const records = () => calls.filter((c) => c[0] === "frame").pop()[1];
  near();
  assert.equal(records(), 3, "the body, the gun and the rig");
  assert.equal(core.input[at(1) + R.TEXTURES], -1, "the gun: no texture (the renderer binds an empty one)");
  assert.deepEqual([...core.input.subarray(at(2) + R.EXTRA, at(2) + R.EXTRA + 2)], [0.25, -0.5], "the rig's scroll, its row's");
  // The droideka goes up: heated copies of its materials, its look and heat its own.
  const hot = heatMaterial(own);
  body.material = hot;
  body.userData.heated = { heat: 0.5, tear: new THREE.Vector4(1, 2, 3, 0.6), base: 0.3, scale: 0.6, soot: 1, dissolve: 0.25, sun: 0.9, brightness: 1.1, darken: 0.1, lighten: 0.2 };
  near();
  assert.equal(records(), 2, "three draws it while it has no program of the heated copy");
  assert.equal(hot.visible, true);
  drawn(hot, program([...SKINNED, "heat", "heatTear", "heatBase", "heatScale", "soot", "dissolve"], ["position", "skinIndex", "skinWeight"]));
  near();
  assert.equal(records(), 3, "then the renderer, as its own record");
  assert.equal(hot.visible, false);
  assert.deepEqual([...core.input.subarray(at(0) + R.LOOK, at(0) + R.LOOK + 4)].map((v) => +v.toFixed(3)), [0.9, 0.1, 0.2, 1.1], "its look as heatMaterial sets it");
  assert.deepEqual([...core.input.subarray(at(0) + R.EXTRA + 4, at(0) + R.EXTRA + 13)].map((v) => +v.toFixed(3)), [0.5, 0.3, 0.6, 1, 1, 2, 3, 0.6, 0.25], "its heat, base, scale, soot, tear and dissolve");
  // Its own materials back: its own record again.
  body.material = own;
  delete body.userData.heated;
  near();
  assert.equal(records(), 3);
  assert.deepEqual([...core.input.subarray(at(0) + R.EXTRA + 4, at(0) + R.EXTRA + 13)], [0, 0, 0, 0, 0, 0, 0, 0, 0]);
});

test("vehicles' and props' rigs: drawn by the renderer with ?rust, left to three with ?rustRigs=0", () => {
  const location = globalThis.location;
  try {
    assert.equal(bridge().core.byName.instances.rigsOn, true);
    globalThis.location = { search: "?rust&rustRigs=0" };
    assert.equal(bridge().core.byName.instances.rigsOn, false);
    globalThis.location = { search: "?rust&rustRigs" };
    assert.equal(bridge().core.byName.instances.rigsOn, true);
  } finally {
    globalThis.location = location;
  }
});

test("a rig that blends: a record of its own kind, its blending and passes registered, three's key for the transparent list", () => {
  const { core, calls, scene, camera, renderer, drawn } = bridge();
  renderer.info = { render: { frame: 0 } };
  core.byName.instances.rigsOn = true;
  const material = new THREE.ShaderMaterial({ transparent: true, depthWrite: false, side: THREE.DoubleSide,
    uniforms: { map: { value: new THREE.Texture() }, sunScale: { value: 1 }, ambientPull: { value: new THREE.Vector2() }, brightness: { value: 1 } } });
  const uniforms = ["projectionMatrix", "modelViewMatrix", "normalMatrix", "map", "boneTexture", "bindMatrix", "bindMatrixInverse", "sunScale", "ambientPull", "brightness"];
  // A double-sided ShaderMaterial: one pass, both sides (three forces one).
  drawn(material, program(uniforms, ["position", "skinIndex", "skinWeight"], "#define DOUBLE_SIDED\nvertex"));
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setAttribute("skinIndex", new THREE.BufferAttribute(new Uint16Array(12), 4));
  geometry.setAttribute("skinWeight", new THREE.BufferAttribute(new Float32Array(12).fill(0.25), 4));
  geometry.setIndex([0, 1, 2]);
  const rig = new THREE.SkinnedMesh(geometry, material), bone = new THREE.Bone(), fighter = new THREE.Group();
  rig.add(bone);
  rig.bind(new THREE.Skeleton([bone]));
  rig.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 2);
  rig.renderOrder = 3;
  fighter.renderOrder = 7;
  fighter.add(rig);
  fighter.position.z = -10;
  scene.add(fighter);
  scene.updateMatrixWorld(true);
  renderer.info.render.frame++;
  core.pass(renderer, scene, camera);
  const blend = calls.find((c) => c[0] === "inst_add_blend");
  assert.ok(blend, "its material's state registered");
  assert.deepEqual([...blend[1]], [1, 0x8006, 0x8006, 0x0302, 0x0303, 1, 0x0303], "normal blending");
  assert.deepEqual(blend.slice(2, 4), [1, 0x0203], "depth tested, not written");
  assert.deepEqual([...blend[4]], [0, 2], "one pass, both sides, with three's program for it");
  assert.equal(material.visible, false, "hidden from three");
  const at = core.recordsAt;
  assert.equal(core.input[at], 3, "a blending rig's record");
  assert.deepEqual([...core.input.subarray(at + 97, at + 100)], [7, 3, rig.id], "three's key: its group's order, its own, its id");
});

test("flying models: parts of tagged groups as rigid records, lit and unlit, hidden by their layers not their shared material", () => {
  const { core, calls, scene, camera, renderer, drawn } = bridge();
  const zero = (kind) => new THREE.ShaderMaterial({ name: `zero:${kind}`, defines: { [kind === "dynamic" ? "DYNAMIC" : "UNLIT"]: "" },
    uniforms: { map: { value: new THREE.Texture() }, sunScale: { value: 1 }, ambientPull: { value: new THREE.Vector2() }, brightness: { value: 1 } } });
  const lit = zero("dynamic"), unlit = zero("unlit");
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  // In front of the camera (it looks down -z), or behind it.
  const piece = (material, z = -5) => { const group = new THREE.Group(); group.add(new THREE.Mesh(geometry, material)); group.userData.flying = true; group.position.z = z; return group; };
  const a = piece(lit), b = piece(unlit), held = piece(lit);
  scene.add(a);
  // Not drawn by three yet: three's.
  const near = () => { scene.updateMatrixWorld(true); core.pass(renderer, scene, camera); };
  near();
  assert.equal(a.children[0].layers.mask, 1);
  const rigid = program(["projectionMatrix", "modelViewMatrix", "normalMatrix", "map", "sunScale", "ambientPull", "brightness"], ["position"]);
  drawn(lit, rigid); drawn(unlit, rigid);
  scene.add(b);
  near();
  assert.equal(a.children[0].layers.mask, 1 << 30, "hidden from three by its layers");
  assert.equal(b.children[0].layers.mask, 1 << 30, "an unlit part too");
  assert.equal(lit.visible, true, "its material, shared with other copies, untouched");
  assert.equal(calls.find((c) => c[0] === "add_program")[1], 2, "a rigid program");
  near();
  assert.equal(calls.filter((c) => c[0] === "frame").pop()[1], 2, "two records");
  // Behind the camera: the renderer's, but not written (three's render would cull it).
  const behind = piece(lit, 5);
  scene.add(behind);
  near();
  assert.equal(behind.children[0].layers.mask, 1 << 30);
  assert.equal(calls.filter((c) => c[0] === "frame").pop()[1], 2, "culled as three culls it");
  scene.remove(behind);
  // A copy inside another object is not the renderer's: three draws it.
  const holder = new THREE.Group(); holder.add(held); scene.add(holder);
  near();
  assert.equal(held.children[0].layers.mask, 1);
  // Hidden: not drawn; taken out of the scene: three's again.
  b.visible = false;
  near();
  assert.equal(calls.filter((c) => c[0] === "frame").pop()[1], 1);
  scene.remove(a);
  assert.equal(a.children[0].layers.mask, 1);
  core.setActive(false);
  assert.equal(b.children[0].layers.mask, 1, "off: back to its layers");
  core.setActive(true);
  assert.equal(b.children[0].layers.mask, 1 << 30);
});

test("a flying part that blends: a rigid record of its own kind, one blend for every copy, three's key for the transparent list", () => {
  const { core, calls, scene, camera, renderer, drawn } = bridge();
  const glow = new THREE.ShaderMaterial({ name: "zero:unlit", defines: { UNLIT: "", SCROLL: "", BLENDED: "" }, transparent: true, depthWrite: false, side: THREE.DoubleSide,
    blending: THREE.AdditiveBlending, forceSinglePass: false,
    uniforms: { map: { value: new THREE.Texture() }, scroll: { value: new THREE.Vector2(0.5, 0) }, sunScale: { value: 1 }, ambientPull: { value: new THREE.Vector2() }, brightness: { value: 1 } } });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  const piece = (z) => { const group = new THREE.Group(); group.add(new THREE.Mesh(geometry, glow)); group.userData.flying = true; group.position.z = z; group.renderOrder = 2; return group; };
  // three draws a double-sided material that blends back faces, then front: a program for each.
  const uniforms = ["projectionMatrix", "modelViewMatrix", "map", "scroll"];
  const back = program(uniforms, ["position"], "#define FLIP_SIDED\nvertex"), front = program(uniforms, ["position"], "vertex");
  drawn(glow, front, [back, front]);
  const a = piece(-5), b = piece(-8);
  scene.add(a, b);
  scene.updateMatrixWorld(true);
  core.pass(renderer, scene, camera);
  assert.equal(a.children[0].layers.mask, 1 << 30, "hidden from three by its layers");
  assert.equal(glow.visible, true, "its material, shared with other copies, untouched");
  const blends = calls.filter((c) => c[0] === "inst_add_blend");
  assert.equal(blends.length, 1, "one blend for the copies of a part");
  assert.deepEqual([...blends[0][1]], [1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], "additive");
  assert.deepEqual(blends[0].slice(2, 4), [1, 0x0203], "depth tested, not written");
  const programs = calls.filter((c) => c[0] === "add_program");
  assert.deepEqual(programs.map((c) => c[1]), [2, 2], "rigid programs, one a side");
  assert.deepEqual([...blends[0][4]], [0, 1, 1, 0], "back faces, then front, each with three's program for it");
  scene.updateMatrixWorld(true);
  core.pass(renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "frame").pop()[1], 2, "two records");
  const at = (r) => core.recordsAt + r * core.stride;
  assert.equal(core.input[at(0)], 4, "a blending part's record");
  assert.deepEqual([...core.input.subarray(at(0) + 97, at(0) + 100)], [2, 0, a.children[0].id], "three's key: its group's order, its own, its id");
  assert.deepEqual([...core.input.subarray(at(0) + 76, at(0) + 78)], [0.5, 0], "its scroll in its row");
});

test("soldiers: only those three's render would list are written, with the bones three worked out in that render", () => {
  const { core, calls, scene, camera, renderer, drawn } = bridge();
  renderer.info = { render: { frame: 0 } };
  const material = new THREE.ShaderMaterial({ defines: { RIGID_SKIN: "" },
    uniforms: { map: { value: new THREE.Texture() }, sunScale: { value: 1 }, ambientPull: { value: new THREE.Vector2() }, brightness: { value: 1 } } });
  const skinned = program(["projectionMatrix", "modelViewMatrix", "normalMatrix", "map", "boneTexture", "bindMatrix", "bindMatrixInverse", "sunScale", "ambientPull", "brightness"],
    ["position", "skinIndex", "skinWeight"]);
  drawn(material, skinned);
  const soldier = (z) => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
    geometry.setAttribute("skinIndex", new THREE.BufferAttribute(new Uint16Array(12), 4));
    geometry.setAttribute("skinWeight", new THREE.BufferAttribute(new Float32Array(12).fill(0.25), 4));
    geometry.setIndex([0, 1, 2]);
    const body = new THREE.SkinnedMesh(geometry, material), bone = new THREE.Bone(), root = new THREE.Group();
    body.add(bone);
    body.bind(new THREE.Skeleton([bone]));
    body.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1);
    body.userData.poseSphere = true; // a soldier's (actor.js): not a rig
    root.add(body);
    root.position.z = z;
    root.userData.parts = { body, hand: null };
    scene.add(root);
    core.graphics.cullRoots.push(root);
    return { body, bone, skeleton: body.skeleton };
  };
  // Every update of a skeleton's bones, whoever asks.
  const update = THREE.Skeleton.prototype.update;
  let updates = 0;
  THREE.Skeleton.prototype.update = function () { updates++; return update.call(this); };
  try {
    const front = soldier(-5), back = soldier(5);
    const records = () => calls.filter((c) => c[0] === "frame").pop()[1];
    // three's render: it lists the scene at a count (and works out the bones
    // of the skinned meshes it lists, at most once a count), then counts on.
    const render = (listed = []) => {
      scene.updateMatrixWorld(true);
      for (const s of listed) s.skeleton.update();
      renderer.info.render.frame++;
      core.pass(renderer, scene, camera);
    };
    render();
    assert.equal(records(), 1, "the soldier behind the camera: not written");
    assert.equal(updates, 1, "his bones not worked out; the other's worked out here, three not having done it");
    // three listed the one in view: his bones as three worked them out.
    front.bone.position.x = 1;
    renderer.info.render.frame = 10;
    render([front]);
    assert.equal(updates, 2, "three's update only");
    assert.deepEqual([...core.bones.subarray(0, 16)], [...front.skeleton.boneMatrices], "three's bones written");
    assert.equal(core.bones[12], 1);
    // Worked out as three drew his shadow, after listing the scene (at the
    // count it counted on to): the same.
    scene.updateMatrixWorld(true);
    renderer.info.render.frame++;
    front.skeleton.update();
    const shadowed = updates;
    core.pass(renderer, scene, camera);
    assert.equal(updates, shadowed);
    // Not listed by three in this render: worked out here.
    renderer.info.render.frame = 20;
    front.bone.position.x = 2;
    render([]);
    assert.equal(updates, shadowed + 1);
    assert.equal(core.bones[12], 2);
    assert.equal(back.skeleton.boneMatrices[12], 0, "never worked out for the renderer");
  } finally {
    THREE.Skeleton.prototype.update = update;
  }
});

test("the Enhanced sniper's trace: two batches, their instances written interleaved as the trace keeps them", async () => {
  const { SniperTrace } = await import("../src/sniper-trace.js");
  const { core, calls, scene, camera, frame, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const trace = particles.custom("sniper", () => new SniperTrace({ add() {}, flash() {} }, () => camera, () => null));
  const streaks = trace.streaks, flares = trace.flares;
  drawn(streaks.mesh.material, program(["projectionMatrix", "modelViewMatrix", "pixelAngle", "fade", "time", "sceneDepth", "softParticles", "depthResolution", "cameraRange"], ["corner", "iHead", "iTail", "iColor", "iSize", "iMisc"]));
  drawn(flares.mesh.material, program(["projectionMatrix", "modelViewMatrix"], ["corner", "iCenter", "iAxis", "iColor", "iParam"]));
  frame();
  assert.equal(calls.filter((c) => c[0] === "fx_batch_interleaved").length, 2, "both kept interleaved");
  assert.equal(streaks.external, true);
  assert.equal(flares.external, true);
  // Two streaks this frame, as the trace leaves them.
  streaks.data.set(Array.from({ length: 36 }, (_, i) => i));
  streaks.count = 2;
  frame();
  const e = entry(0);
  assert.deepEqual(e.slice(0, 5), [0, 0, 2, 0, 0]);
  assert.deepEqual(e.slice(6, 8), [0, 11], "render order 11");
  assert.deepEqual([...core.fxData.subarray(0, 36)], Array.from({ length: 36 }, (_, i) => i), "interleaved, as kept");
  // Off: three draws them again, their shared buffer marked for upload whole.
  core.setActive(false);
  assert.equal(streaks.external, false);
  assert.deepEqual(streaks.buffer.updateRanges, [{ start: 0, count: 36 }]);
  core.setActive(true);
  assert.equal(streaks.external, true);
});

test("the Enhanced arc caster's lightning: its capsules a batch kept interleaved, found in three's list, its values after them", async () => {
  const { ArcBatch } = await import("../src/arc-lightning.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const arcs = new ArcBatch(8);
  scene.add(arcs.mesh);
  // Shown empty while the battle loads: one dark capsule, drawn by three.
  arcs.begin(); arcs.end(true);
  assert.equal(arcs.mesh.visible, true);
  list.transparent.push({ object: arcs.mesh, material: arcs.material, group: null });
  drawn(arcs.material, program(["projectionMatrix", "viewMatrix", "pixelAngle", "nearDepth", "closeRange", "eyeFade", "fade", "sceneDepth", "softParticles", "depthResolution", "cameraRange"],
    ["corner", "iA", "iB", "iNA", "iNB", "iColor", "iSize"]));
  frame();
  assert.equal(calls.filter((c) => c[0] === "fx_batch_interleaved").length, 1, "kept interleaved");
  assert.equal(arcs.external, true);
  assert.equal(calls.find((c) => c[0] === "fx_batch_values")[2], "eyeFade:2:0,closeRange:1:0,pixelAngle:1:0,nearDepth:1:0");
  // External: shown to three never, even kept warm, and nothing uploaded.
  arcs.begin(); arcs.end(true);
  assert.equal(arcs.mesh.visible, false);
  list.transparent.length = 0;
  // Two segments this frame.
  const v = (x) => new THREE.Vector3(x, 0, -5), version = arcs.buffer.version;
  arcs.begin();
  arcs.push(v(0), v(1), null, null, [1, 0.5, 0.25], 1, 0.02, 0.1);
  arcs.push(v(1), v(2), null, null, [1, 0.5, 0.25], 0.5, 0.02, 0.1);
  arcs.end();
  assert.equal(arcs.buffer.version, version, "three uploads it nothing");
  frame();
  const e = entry(0);
  assert.deepEqual(e.slice(0, 5), [0, 0, 2, 0, 0]);
  assert.deepEqual(e.slice(6, 8), [0, 12], "render order 12");
  assert.deepEqual([...core.fxData.subarray(0, 40)], [...arcs.data.subarray(0, 40)], "interleaved, as kept");
  assert.equal(core.fxData[40 + 2], arcs.material.uniforms.closeRange.value, "its values after them");
  // Off: three draws it again, its buffer marked for upload whole.
  core.setActive(false);
  assert.equal(arcs.external, false);
  assert.deepEqual(arcs.buffer.updateRanges, [{ start: 0, count: 40 }]);
});

test("the droideka's rolling dust: its wisps a batch on a quad of its own, written each frame with its clock and tint", async () => {
  const { DroidekaTrail } = await import("../src/droideka-trail.js");
  const { core, calls, scene, list, frame, drawn, entry } = bridge();
  const collision = { cast(origin, direction, far, out) { if (direction.y > 0) return null; out.point.set(origin.x, 0, origin.z); out.normal.set(0, 1, 0); out.terrain = true; return out; } };
  const trail = new DroidekaTrail({ scene, collision, settings: { get: () => "enhanced" }, random: () => 0.5 });
  const soldier = { mode: 12, dead: false, air: null, grounded: true, position: new THREE.Vector3(0, 0, -5), velocity: new THREE.Vector3(10, 0, 0) };
  const roll = (frames) => { for (let i = 0; i < frames; i++) { soldier.position.addScaledVector(soldier.velocity, 1 / 60); trail.update(1 / 60, soldier); } };
  roll(30);
  assert.ok(trail.count > 0 && trail.mesh.visible);
  list.transparent.push({ object: trail.mesh, material: trail.material, group: null });
  drawn(trail.material, program(["projectionMatrix", "viewMatrix", "trailTime", "tint", "fogColor", "fogRange", "sceneDepth", "softParticles", "depthResolution", "cameraRange"],
    ["position", "uv", "birth", "drift", "shape"]));
  frame();
  const added = calls.find((c) => c[0] === "fx_add_batch");
  assert.equal(added[4], 0xffffffff, "no corner: a base of its own");
  const base = calls.find((c) => c[0] === "fx_batch_base");
  assert.deepEqual([base[2], base[3], base[5]], [[0, 3, 1, 2], 4, [0, 2, 1, 2, 3, 1]], "the quad's position and uv, as its geometry has them");
  assert.equal(calls.find((c) => c[0] === "fx_batch_values")[2], "trailTime:1:0,tint:3:0");
  assert.equal(trail.external, true);
  list.transparent.length = 0;
  const version = trail.geometry.getAttribute("birth").version;
  roll(1);
  assert.equal(trail.mesh.visible, false, "shown to three never");
  assert.equal(trail.geometry.getAttribute("birth").version, version, "uploaded nothing by three");
  frame();
  const e = entry(0), n = trail.count;
  assert.deepEqual(e.slice(0, 4), [0, 0, n, 0], "every wisp, each frame");
  const birth = trail.geometry.getAttribute("birth").array, shape = trail.geometry.getAttribute("shape").array;
  assert.deepEqual([...core.fxData.subarray(0, 4 * n)], [...birth.subarray(0, 4 * n)]);
  assert.deepEqual([...core.fxData.subarray(8 * n, 12 * n)], [...shape.subarray(0, 4 * n)]);
  assert.equal(core.fxData[12 * n], Math.fround(trail.time), "its own clock after them");
  core.setActive(false);
  assert.equal(trail.external, false);
});

test("the Enhanced jetpack's wake: its three pools batches on a quad of their own, written each frame with the wake's clock", async () => {
  const { JetpackWake } = await import("../src/jetpack-wake.js");
  const { core, calls, scene, list, frame, drawn, entry } = bridge();
  const wake = new JetpackWake({ scene, random: () => 0.5 });
  const nozzle = { position: new THREE.Vector3(0, 0, -5), direction: new THREE.Vector3(0, -1, 0), radius: 0.034 };
  const input = { nozzles: [nozzle], velocity: new THREE.Vector3(), active: true, throttle: 1 };
  for (let i = 0; i < 30; i++) wake.update(1 / 60, input);
  const pools = wake.pools.filter((pool) => pool.count > 0);
  assert.ok(pools.length >= 2);
  const uniforms = ["projectionMatrix", "viewMatrix", "wakeTime", "kind", "drag", "fogColor", "fogRange", "sceneDepth", "softParticles", "depthResolution", "cameraRange"];
  for (const pool of pools) {
    list.transparent.push({ object: pool.mesh, material: pool.mesh.material, group: null });
    drawn(pool.mesh.material, program(uniforms, ["position", "uv", "birth", "flow", "axis", "style"]));
  }
  frame();
  assert.equal(calls.filter((c) => c[0] === "fx_add_batch").length, pools.length, "a batch a pool");
  assert.ok(calls.filter((c) => c[0] === "fx_batch_values").every((c) => c[2] === "wakeTime:1:0,kind:1:0,drag:1:0"));
  assert.ok(pools.every((pool) => pool.external));
  list.transparent.length = 0;
  const version = pools[0].attributes.birth.version;
  wake.update(1 / 60, input);
  assert.ok(pools.every((pool) => !pool.mesh.visible), "shown to three never");
  assert.equal(pools[0].attributes.birth.version, version, "uploaded nothing by three");
  frame();
  const pool = pools[0], n = pool.count, e = entry(0);
  assert.equal(e[2], n, "every particle, each frame");
  assert.deepEqual([...core.fxData.subarray(0, 4 * n)], [...pool.attributes.birth.array.subarray(0, 4 * n)]);
  assert.equal(core.fxData[14 * n], Math.fround(wake.time), "its clock after them");
  core.setActive(false);
  assert.ok(pools.every((pool) => !pool.external));
});

test("the death camera's trace: a mesh rebuilt each frame, its vertices, indices and own values written as three draws it", async () => {
  const { DeathTrace } = await import("../src/death-trace.js");
  const { core, calls, scene, camera, list, frame, drawn, entry } = bridge();
  const trace = new DeathTrace(scene);
  const from = new THREE.Vector3(-3, 0, -10), to = new THREE.Vector3(3, 0, -10);
  trace.show(from, to, false, 0.5, 1, 0.25, camera);
  list.transparent.push({ object: trace.mesh, material: trace.material, group: null });
  // A double-sided ShaderMaterial: one pass, both sides.
  drawn(trace.material, program(["projectionMatrix", "modelViewMatrix", "color", "head", "opacity", "span", "traceTime"], ["position", "line"], "#define DOUBLE_SIDED\nvertex"));
  frame();
  const added = calls.find((c) => c[0] === "fx_add_mesh");
  assert.ok(added, "registered");
  assert.deepEqual(added.slice(2, 7), [[1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], 0, 0x0203, [3, 2], "color:3,head:1,opacity:1,span:1,traceTime:1"], "additive, no depth test, its own values");
  assert.deepEqual(calls.find((c) => c[0] === "fx_mesh_pass").slice(3), [2, [0, 1]], "one pass, both sides");
  assert.equal(trace.material.visible, false, "hidden from three");
  assert.equal(core.fxInput[0], 0, "drawn by three as it is registered");
  list.transparent.length = 0;
  trace.show(from, to, true, 0.75, 0.5, 0.5, camera);
  frame();
  const e = entry(0);
  assert.deepEqual(e.slice(0, 5), [1, 0, 4, 6, 0], "a mesh: four vertices, six indices");
  assert.deepEqual(e.slice(6, 8), [0, 990], "render order 990");
  assert.deepEqual([...core.fxData.subarray(0, 12)], [...trace.positions].map(Math.fround), "its corners as this frame placed them");
  assert.deepEqual([...core.fxBits.subarray(20, 26)], [0, 2, 1, 1, 2, 3]);
  assert.deepEqual([...core.fxData.subarray(26, 33)].map((v) => +v.toFixed(3)), [0.3, 1, 0.45, 0.75, 0.5, 6, 0.5].map((v) => +Math.fround(v).toFixed(3)), "a teammate's green, head, opacity, span, its clock");
  // Hidden by the death shot: not drawn.
  trace.hide();
  frame();
  assert.equal(core.fxInput[0], 0);
  core.setActive(false);
  assert.equal(trace.material.visible, true, "off: three's again");
});

test("an error in the effects pass gives the effects back to three for good, three's render going on", () => {
  const { core, scene, frame, drawn } = bridge();
  const particles = new ParticleRenderer(scene);
  const batch = particles.batch(new THREE.Texture(), "additive");
  const fill = () => { particles.begin(); batch.push(0, 0, 0, 1, 1, 1, 1, 0.2, 0, STREAK, 1, [0, 0, 1], [1, 0, 0]); particles.end(); };
  fill();
  drawn(batch.material, CPU_PROGRAM);
  frame();
  fill();
  assert.equal(batch.external, true);
  // A batch the pass can't read.
  batch.arrays.iPosition = null;
  assert.doesNotThrow(() => frame());
  assert.equal(core.fxInput[0], 0, "nothing drawn of it this frame");
  assert.equal(batch.external, false, "three's again");
  assert.ok(core.errors.some((e) => e.startsWith("effects failed")));
  core.setActive(false);
  core.setActive(true);
  assert.equal(batch.external, false, "for good");
});
