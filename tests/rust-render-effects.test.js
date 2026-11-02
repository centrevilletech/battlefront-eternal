import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { blendOf } = await import("../src/rust-render/effects.js");
const { ParticleRenderer, particleRenderers, STREAK } = await import("../src/particles.js");
const { GpuEffects } = await import("../src/effects-gpu.js");

const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0];
const GL = { ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89, SHADER_TYPE: 0x8b4f, VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30 };

// The uniforms and attributes three's programs for the two kinds of batch
// report as active.
const CPU_PROGRAM = {
  uniforms: ["projectionMatrix", "modelViewMatrix", "map", "fade", "sceneDepth", "softParticles", "depthResolution", "cameraRange"],
  attributes: ["corner", "iPosition", "iColor", "iShape", "iAxis", "iAxis2", "iFrame"],
};
const ANALYTIC_PROGRAM = {
  uniforms: [...CPU_PROGRAM.uniforms, "tracks", "emitters", "trackRows", "emitterRows"],
  attributes: ["corner", "spawnPosition", "spawnVelocity", "spawnColor", "spawnShape", "spawnMeta"],
};

// The bridge over stand-ins for the WASM renderer (its memory, and a log of
// the calls made to it), three's renderer and its context (enough of it for
// a program's uniforms and attributes).
function bridge() {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 17) };
  let next = { program: 0, texture: 0, batch: 0, owner: 0 };
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
    fx_data_len() { return 8192; }
    add_texture() { calls.push(["add_texture"]); return next.texture++; }
    set_far_map(texture) { calls.push(["set_far_map", texture]); }
    add_program(kind, vs, fs, attributes, uniforms) { calls.push(["add_program", kind, attributes, uniforms]); return next.program++; }
    fx_add_owner(tracks, rows) { calls.push(["fx_add_owner", tracks, rows]); return next.owner++; }
    fx_add_batch(program, texture, blend, corner, attributes, owner) { calls.push(["fx_add_batch", program, texture, [...blend], corner, [...attributes], owner]); return next.batch++; }
    fx_batch_texture(batch, texture) { calls.push(["fx_batch_texture", batch, texture]); }
    sort() {}
    frame() { calls.push(["frame"]); return STATS; }
    effects_frame() { calls.push(["effects_frame"]); return STATS; }
  }
  const gl = {
    ...GL,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i] }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: () => [{ type: GL.VERTEX_SHADER, source: "vertex" }, { type: GL.FRAGMENT_SHADER, source: "fragment" }],
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => s.source,
  };
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene" }, farTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  const renderer = {
    getContext: () => gl,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initTexture() {},
    shadowMap: { enabled: false },
    getRenderTarget: () => sceneTarget,
    setRenderTarget() {},
    resetState() { calls.push(["resetState"]); },
  };
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  const anchor = scene.children.find((o) => o.name === "rust-render anchor");
  // A frame of three's: the anchor drawn (it calls back after).
  const frame = () => anchor.onAfterRender(renderer, scene, camera, anchor.geometry, anchor.material, null);
  // three has drawn a material once: its program.
  const drawn = (material, program) => { renderer.properties.get(material).currentProgram = { program }; };
  return { core, calls, scene, renderer, anchor, frame, drawn, fx: core.byName.effects };
}

test("three's blending, as setBlending sets it, in GL enums", () => {
  const additive = new THREE.ShaderMaterial({ transparent: true, blending: THREE.AdditiveBlending });
  assert.deepEqual(blendOf(additive), [1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1]);
  const normal = new THREE.ShaderMaterial({ transparent: true, blending: THREE.NormalBlending });
  assert.deepEqual(blendOf(normal), [1, 0x8006, 0x8006, 0x0302, 0x0303, 1, 0x0303]);
  normal.premultipliedAlpha = true;
  assert.deepEqual(blendOf(normal), [1, 0x8006, 0x8006, 1, 0x0303, 1, 0x0303]);
  assert.deepEqual(blendOf(new THREE.ShaderMaterial())[0], 0, "normal blending of an opaque material: none");
  const custom = new THREE.ShaderMaterial({ transparent: true, blending: THREE.CustomBlending, blendEquation: THREE.AddEquation, blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor });
  assert.deepEqual(blendOf(custom), [1, 0x8006, 0x8006, 1, 0x0303, 1, 0x0303]);
});

test("the effects' anchor sits after everything else at the particles' renderOrder, in the transparent list", () => {
  const { anchor } = bridge();
  assert.ok(anchor.renderOrder > 10 && anchor.renderOrder < 10.001);
  assert.equal(anchor.material.transparent, true);
});

test("a CPU batch: registered once three drew it, then hidden from three and written each frame", () => {
  const { core, calls, scene, frame, drawn, fx } = bridge();
  const particles = new ParticleRenderer(scene);
  assert.ok(particleRenderers.get(scene).includes(particles));
  const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
  const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 2 * i, 3 * i, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
  fill(2);
  assert.equal(batch.mesh.visible, true);
  // Not drawn by three yet: nothing to register, nothing drawn.
  frame();
  assert.deepEqual(calls.filter((c) => c[0] !== "add_texture" && c[0] !== "set_far_map"), []);
  drawn(batch.material, CPU_PROGRAM);
  // Registered as three draws it this frame: drawn by three alone.
  frame();
  const added = calls.find((c) => c[0] === "fx_add_batch");
  assert.ok(added, "registered");
  assert.equal(calls.find((c) => c[0] === "add_program")[1], 3, "an effect program");
  assert.deepEqual(added.slice(3), [[1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], 0, [1, 3, 2, 4, 3, 4, 4, 3, 5, 3, 6, 4], -1]);
  assert.equal(calls.some((c) => c[0] === "effects_frame"), false);
  assert.equal(batch.external, true);
  assert.equal(batch.mesh.visible, false);
  // The next frame: three is shown nothing and uploads nothing; the
  // renderer draws it, its instances written attribute by attribute.
  const version = batch.geometry.getAttribute("iPosition").version;
  fill(3);
  assert.equal(batch.mesh.visible, false);
  assert.equal(batch.geometry.getAttribute("iPosition").version, version);
  calls.length = 0;
  frame();
  // (The far scene's image registered as the first view over it is drawn.)
  assert.deepEqual(calls.map((c) => c[0]).filter((c) => c !== "add_texture" && c !== "set_far_map"), ["effects_frame", "resetState"]);
  const input = core.fxInput, data = core.fxData;
  assert.equal(input[0], 1, "one draw");
  const at = 8 + 8 * 4;
  assert.deepEqual([...input.subarray(at, at + 5)], [0, 3, 0, 0, batch.capacity]);
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
  calls.length = 0;
  frame();
  assert.deepEqual(calls, [], "nothing drawn while off");
  core.setActive(true);
  assert.equal(batch.external, true);
  assert.equal(batch.mesh.visible, false);
});

test("an analytic batch: its owner's emitters, and its instances from the first changed", () => {
  const { core, calls, scene, frame, drawn } = bridge();
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
  const owner = calls.find((c) => c[0] === "fx_add_owner");
  assert.ok(owner, "its owner registered");
  assert.equal(calls.find((c) => c[0] === "fx_add_batch")[6], 0, "with its owner");
  // Drawn: every instance, and the emitter row of the effect playing.
  fill(made);
  frame();
  const input = core.fxInput, data = core.fxData, at = 8 + 8 * 4;
  assert.deepEqual([...input.subarray(8, 12)], [0, gpu.uniforms.emitterRows.value, 1, 0], "the owner's rows, one written at 0");
  assert.deepEqual([...data.subarray(0, 3)], [1, 2, 3], "the effect's place");
  assert.deepEqual([...input.subarray(at, at + 4)], [0, 4, 0, 12], "all four instances after the emitter row");
  // Nothing changed: none written.
  fill(made);
  frame();
  assert.deepEqual([...input.subarray(at, at + 3)], [0, 4, 4]);
  // The third changed (over two frames, the first not drawn): from it on.
  fill([made[0], made[1], { ...made[2], birth: 9 }, made[3]]);
  fill([made[0], made[1], { ...made[2], birth: 9 }, made[3]]);
  frame();
  assert.deepEqual([...input.subarray(at, at + 3)], [0, 4, 2]);
  assert.deepEqual([...data.subarray(12, 15)], [2, 0, 0], "spawnPosition of the third, first");
});
