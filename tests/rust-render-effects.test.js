import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
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
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 17) };
  const next = { program: 0, texture: 0, batch: 0, owner: 0, mesh: 0, model: 0, geometry: 0 };
  class BfeRenderer {
    input_ptr() { return 0; }
    input_len() { return 200 + 76 * 4; }
    bones_ptr() { return 8192; }
    bones_len() { return 64; }
    records_at() { return 200; }
    record_stride() { return 76; }
    fx_input_ptr() { return 16384; }
    fx_input_len() { return 2600; }
    fx_data_ptr() { return 40960; }
    fx_data_len() { return 16384; }
    transparent_keys_ptr() { return 12288; }
    transparent_keys_len() { return 64; }
    transparent_segments_ptr() { return 106496; }
    transparent_segments_len() { return 2; }
    transparent_bucket_shown() {}
    add_texture() { calls.push(["add_texture"]); return next.texture++; }
    set_far_map(texture) { calls.push(["set_far_map", texture]); }
    add_program(kind, vs, fs, attributes, uniforms) { calls.push(["add_program", kind, vs, attributes, uniforms]); return next.program++; }
    fx_add_owner(tracks, rows) { calls.push(["fx_add_owner", tracks, rows]); return next.owner++; }
    fx_add_batch(program, texture, blend, corner, attributes, owner) { calls.push(["fx_add_batch", program, texture, [...blend], corner, [...attributes], owner]); return next.batch++; }
    fx_batch_texture(batch, texture) { calls.push(["fx_batch_texture", batch, texture]); }
    fx_batch_values(batch, values) { calls.push(["fx_batch_values", batch, values]); }
    fx_batch_sampler(batch, name, unit, texture) { calls.push(["fx_batch_sampler", batch, name, unit, texture]); }
    fx_add_mesh(texture, blend, flags, func, sizes, values) { calls.push(["fx_add_mesh", texture, [...blend], flags, func, [...sizes], values]); return next.mesh++; }
    fx_mesh_pass(mesh, program, side, locations) { calls.push(["fx_mesh_pass", mesh, program, side, [...locations]]); }
    fx_mesh_texture(mesh, texture) { calls.push(["fx_mesh_texture", mesh, texture]); }
    fx_matrices_ptr() { return 90112; }
    fx_matrices_len() { return 1024; }
    fx_add_model(texture, blend, flags, func, values, sphere) { calls.push(["fx_add_model", texture, [...blend], flags, func, values, [...sphere]]); return next.model++; }
    fx_model_pass(model, program, side, geometry) { calls.push(["fx_model_pass", model, program, side, geometry]); }
    add_geometry() { calls.push(["add_geometry"]); return next.geometry++; }
    geometry_buffer() { return 0; }
    geometry_pointer() {}
    geometry_index() {}
    geometry_group() {}
    sort() {}
    frame() { calls.push(["frame"]); return STATS; }
    transparent_frame(three) { calls.push(["transparent_frame", three]); return STATS; }
  }
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
  assert.deepEqual(core.phases.transparent.map((p) => p.name), ["transparent", "effects"]);
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
