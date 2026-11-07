import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { standinModule, standinThree, fakeProgram, GL } from "./support/rust-standin.mjs";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { lighting } = await import("../src/materials.js");
globalThis.__pointLights = await import("../src/point-lights.js");

const WORLD = 0, SKINNED = 1, EFFECT = 4;
// A shadow receiver's active uniforms as three's GLSL for a zero material
// lists them with shadows on (SwiftShader's list, two cascades).
const RECEIVER = [["projectionMatrix", GL.FLOAT_MAT4], ["viewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["receiveShadow", GL.BOOL],
  ["directionalLightShadows[0].shadowMapSize", GL.FLOAT_VEC2], ["directionalLightShadows[1].shadowMapSize", GL.FLOAT_VEC2],
  ["cascades[0]", GL.FLOAT_VEC2], ["directionalShadowMatrix[0]", GL.FLOAT_MAT4], ["cascadePlanes[0]", GL.FLOAT_MAT3], ["cascadeMaps[0]", 0x8b62]];
const LAMPS = [["localLightPositions[0]", GL.FLOAT_VEC4], ["localLightColors[0]", GL.FLOAT_VEC3], ["localLightCount", GL.FLOAT]];

// The bridge over stand-ins, with three's shadow maps on (two cascades) or
// off, and lamps on or off; the near scene's material a program is for.
function bridge({ shadows = false, lamps = false } = {}) {
  const { BfeRenderer, memory, calls, region } = standinModule();
  const settings = new Map([["lampLighting", lamps]]);
  const csm = shadows ? { lights: [0, 1].map((i) => ({ shadow: {
    intensity: 1, bias: -0.015 / 2999, normalBias: 0, radius: 1, mapSize: new THREE.Vector2(1024, 1024),
    matrix: new THREE.Matrix4().makeTranslation(i + 1, 2, 3) } })) } : null;
  const three = standinThree(calls, { graphics: { csm, settings: { get: (k) => settings.get(k) ?? false } } });
  three.renderer.shadowMap.enabled = shadows;
  const core = new RustRender(three.graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const material = new THREE.ShaderMaterial();
  for (const [name, value] of Object.entries(lighting)) material.uniforms[name] = value;
  return { core, calls, region, memory, settings, material, ...three };
}

// The shared cascade inputs three's materials read, set for a test.
function withCascades(fn) {
  const saved = [lighting.cascadeMaps.value, lighting.cascadePlanes.value, lighting.cascades.value];
  lighting.cascadeMaps.value = [new THREE.Texture(), new THREE.Texture()];
  lighting.cascadeMaps.value.forEach((t, i) => { t.name = `cascade${i}`; });
  lighting.cascadePlanes.value = [new THREE.Matrix3().set(1, 2, 3, 4, 5, 6, 7, 8, 9), new THREE.Matrix3()];
  lighting.cascades.value = [new THREE.Vector2(0, 0.25), new THREE.Vector2(0.25, 1), new THREE.Vector2(), new THREE.Vector2()];
  try { return fn(); } finally { [lighting.cascadeMaps.value, lighting.cascadePlanes.value, lighting.cascades.value] = saved; }
}



test("a shadow receiver's program is the renderer's only from a pass that feeds shadows, with shadows on", () => withCascades(() => {
  const { core, material } = bridge({ shadows: true });
  // A world draw's receiveShadow is the pass's own value (world.js MATERIAL); an instance's is its row's.
  const feeds = { uniforms: new Set(["map", "receiveShadow"]), material: new Set(["receiveShadow"]), shadows: true };
  const instanced = { uniforms: new Set(["map"]), material: new Set(), shadows: true };
  const not = { uniforms: new Set(["map", "receiveShadow"]), material: new Set(["receiveShadow"]) };
  assert.ok(core.program(fakeProgram(RECEIVER), WORLD, material, feeds), "world: the block and the maps are the core's; receiveShadow the pass's");
  assert.ok(core.program(fakeProgram(RECEIVER), SKINNED, material, instanced), "an instance's receiveShadow is its row's");
  assert.throws(() => core.program(fakeProgram(RECEIVER), WORLD, material, instanced), /Program/);
  const p = fakeProgram(RECEIVER);
  assert.throws(() => core.program(p, WORLD, material, not), /Program/);
  assert.ok(core.errors.some(e => e.includes(`Program ${p.id}`) && e.endsWith("(shadows)")));
  assert.throws(() => core.program(fakeProgram(RECEIVER), EFFECT, material, feeds), /Program/);
}));

test("with shadows off nothing of the shadow block is the renderer's to give", () => {
  const { core, material } = bridge({ shadows: false });
  const feeds = { uniforms: new Set(["map"]), material: new Set(), shadows: true };
  assert.throws(() => core.program(fakeProgram(RECEIVER), WORLD, material, feeds), /Program/);
});

test("with lamps on a DYNAMIC program is the renderer's only from a pass that feeds lamps", () => {
  const { core, material } = bridge({ lamps: true });
  const uniforms = new Set(["map", "localLightPositions", "localLightColors", "localLightCount"]);
  const p = fakeProgram([["map", GL.SAMPLER_2D], ...LAMPS]);
  assert.throws(() => core.program(p, SKINNED, material, { uniforms, material: new Set() }), /Program/);
  assert.ok(core.errors.some(e => e.includes("localLightPositions (lamps)")));
  assert.ok(core.program(fakeProgram([["map", GL.SAMPLER_2D], ...LAMPS]), SKINNED, material, { uniforms, material: new Set(), lamps: true }));
  // Lamps off (Classic): unread placeholders, as before.
  const classic = bridge({ lamps: false });
  assert.ok(classic.core.program(fakeProgram([["map", GL.SAMPLER_2D], ...LAMPS]), SKINNED, classic.material, { uniforms, material: new Set() }));
});

test("reveal: three's own render runs with what each pass hid shown, hidden again after", () => {
  const { core } = bridge();
  const log = [];
  core.addPass({ name: "a", phase: "near", reveal: (on) => log.push(["a", on]) });
  core.addPass({ name: "b", phase: "far", reveal: (on) => log.push(["b", on]) });
  const result = core.reveal(() => { log.push(["render"]); return 7; });
  assert.equal(result, 7);
  assert.deepEqual(log, [["a", true], ["b", true], ["render"], ["a", false], ["b", false]]);
  // Hidden again even when the render throws.
  log.length = 0;
  assert.throws(() => core.reveal(() => { throw new Error("boom"); }));
  assert.deepEqual(log, [["a", true], ["b", true], ["a", false], ["b", false]]);
  });

test("the world pass shows its taken buckets to three while three's render needs them", () => {
  const { core } = bridge();
  const world = core.byName.world;
  const taken = new THREE.ShaderMaterial(), stale = new THREE.ShaderMaterial();
  taken.visible = false;
  world.meshes.push({ mesh: { material: taken }, id: 0, version: taken.version, stale: false }, { mesh: { material: stale }, id: 1, version: 0, stale: true });
  core.reveal(() => assert.equal(taken.visible, true, "three's shadow pass draws it"));
  assert.equal(taken.visible, false);
  assert.equal(stale.visible, true, "one three draws anyway is left alone");
});

test("the shadow block: each cascade's values, matrix, plane matrix and slice in BfeShadow's layout, and its maps", () => withCascades(() => {
  const { core, calls, region, memory } = bridge({ shadows: true });
  core.writeShadow();
  const maps = calls.filter((c) => c[0] === "set_shadow_maps");
  assert.equal(maps.length, 1);
  assert.deepEqual([...maps[0][1]], [0, 1], "the cascades' depth maps, registered, in cascade order");
  const out = new Float64Array(memory.buffer, region("shadow_input"), 160);
  assert.deepEqual([...out.subarray(0, 8)], [1, -0.015 / 2999, 0, 1, 1024, 1024, 0, 0], "cascade 0's DirectionalLightShadow (std140: map size at float 4)");
  assert.deepEqual([...out.subarray(32 + 12, 32 + 15)], [1, 2, 3], "cascade 0's matrix, column-major (its translation)");
  assert.deepEqual([...out.subarray(48 + 12, 48 + 15)], [2, 2, 3], "cascade 1's");
  assert.deepEqual([...out.subarray(96, 108)], [1, 4, 7, 0, 2, 5, 8, 0, 3, 6, 9, 0], "plane matrix: three columns of four");
  assert.deepEqual([...out.subarray(144, 152)], [0, 0.25, 0, 0, 0.25, 1, 0, 0], "slices, a vec4 each");
  assert.deepEqual([...out.subarray(64, 96)], new Array(32).fill(0), "no third and fourth cascade");
  core.writeShadow();
  assert.equal(calls.filter((c) => c[0] === "set_shadow_maps").length, 1, "the maps given again only when they change");
}));

test("Classic: no shadow block written and no maps given", () => {
  const { core, calls } = bridge({ shadows: false });
  core.writeShadow();
  assert.equal(calls.filter((c) => c[0] === "set_shadow_maps" || c[0] === "shadow_input_ptr").length, 0);
});

test("with lamps on, a pass that gives lamps gets a program reading them from the lamp table, kept apart from three's", () => {
  const { core, calls, material } = bridge({ lamps: true });
  const uniforms = new Set(["map", "localLightPositions", "localLightColors", "localLightCount"]);
  const three = fakeProgram([["map", GL.SAMPLER_2D], ...LAMPS]);
  const fed = core.program(three, SKINNED, material, { uniforms, material: new Set(), lamps: true });
  assert.ok(fed && fed.lamps);
  assert.equal(calls.filter((c) => c[0] === "add_program_lamps").length, 1);
  assert.throws(() => core.program(three, SKINNED, material, { uniforms, material: new Set() }), /Program/);
  assert.equal(core.program(three, SKINNED, material, { uniforms, material: new Set(), lamps: true }), fed, "made once");
  // A program reading no lamps is an ordinary one even from such a pass.
  const plain = core.program(fakeProgram([["map", GL.SAMPLER_2D]]), SKINNED, material, { uniforms, material: new Set(), lamps: true });
  assert.ok(plain && !plain.lamps);
});

test("the lamp table: every lamp source, given again only when there are more", () => {
  const { core, calls, graphics } = bridge({ lamps: true });
  const source = (x, r) => ({ position: new THREE.Vector3(x, 2, 3), radius: r, color: new THREE.Vector3(0.5, 0.25, 0.125) });
  graphics.localLights = { sources: [source(1, 6), source(4, 9)], select: () => [] };
  core.writeLamps();
  const sent = calls.filter((c) => c[0] === "set_lamps");
  assert.equal(sent.length, 1);
  assert.deepEqual([...sent[0][1]], [1, 2, 3, 6, 0.5, 0.25, 0.125, 0, 4, 2, 3, 9, 0.5, 0.25, 0.125, 0]);
  core.writeLamps();
  assert.equal(calls.filter((c) => c[0] === "set_lamps").length, 1, "the same sources: nothing sent");
  graphics.localLights.sources.push(source(7, 6));
  core.writeLamps();
  assert.equal(calls.filter((c) => c[0] === "set_lamps").length, 2);
});

test("a draw's lamps: the sources select() gives, as lamp table indices, in its order, and their count", () => {
  const { core, graphics } = bridge({ lamps: true });
  const sources = [0, 1, 2, 3, 4].map((i) => ({ position: new THREE.Vector3(i, 0, 0), radius: 5, color: new THREE.Vector3(1, 1, 1) }));
  const asked = [];
  graphics.localLights = { sources, select: (object, viewModel) => { asked.push([object, viewModel]); return [sources[3], sources[1]]; } };
  core.writeLamps();
  const object = {};
  const lamps = core.lamps(object, true);
  assert.deepEqual([...lamps.indices, lamps.count], [3, 1, -1, -1, 2]);
  assert.deepEqual(asked, [[object, true]], "selected as three's onBeforeRender would (a view model's)");
  // Classic (lamps off): select() gives none.
  graphics.localLights.select = () => [];
  const none = core.lamps(object);
  assert.deepEqual([...none.indices, none.count], [-1, -1, -1, -1, 0]);
});

test("the frame block survives the renderer's memory growing while the Enhanced inputs are written", () => withCascades(() => {
  const { installPointLights } = globalThis.__pointLights;
  installPointLights();
  const { core, memory, region, scene, camera, renderer, graphics } = bridge({ shadows: true, lamps: true });
  graphics.localLights = { sources: [{ position: new THREE.Vector3(1, 2, 3), radius: 6, color: new THREE.Vector3(1, 1, 1) }], select: () => [] };
  const light = new THREE.PointLight(0xffffff, 1, 10);
  light.position.set(4, 5, 6);
  scene.add(light);
  scene.updateMatrixWorld(true);
  scene.onBeforeRender(renderer, scene, camera);
  // Sending the lamp table grows the memory: every view of the old one detaches.
  let grown = 0;
  core.rust.set_lamps = () => { const old = new Uint8Array(memory.buffer); memory.buffer = new ArrayBuffer(old.length * 2); new Uint8Array(memory.buffer).set(old); grown++; };
  core.writeFrame(scene, camera, 0);
  assert.equal(grown, 1);
  const input = new Float64Array(memory.buffer, region("input"), 200);
  assert.equal(input[70], 1, "the point light counted in the memory as it is now");
  assert.deepEqual([...input.subarray(72, 75)], [4, 5, 6], "and its position written there");
  assert.deepEqual([...input.subarray(0, 16)], camera.projectionMatrix.elements, "the camera too");
}));

// A cascade light as graphics.js's CSM makes one: its shadow's camera,
// matrices and map (a depth texture), updateMatrices as three's.
function cascade(size = 1024) {
  const light = new THREE.DirectionalLight();
  light.position.set(0, 10, 0);
  light.target.position.set(0, 0, 0);
  light.updateMatrixWorld(true);
  light.target.updateMatrixWorld(true);
  const depthTexture = new THREE.DepthTexture(size, size);
  light.shadow.mapSize.set(size, size);
  light.shadow.map = { depthTexture, texture: new THREE.Texture() };
  return light;
}

// A caster with a source-owned depth material.
function caster() {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  const mesh = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.FrontSide }));
  mesh.castShadow = true;
  mesh.frustumCulled = false;
  mesh.customDepthMaterial = new THREE.Material();
  mesh.customDepthMaterial.userData.bfeProgram="actors/depth-solid";
  return mesh;
}

test("a cascade's pass: the casters three's shadow pass would draw, drawn by the shadow pass", () => {
  const { core, calls, region, memory, scene, camera, renderer, properties } = bridge({ shadows: true });
  const light = cascade(), mesh = caster();
  scene.add(mesh);
  const hidden = caster();
  hidden.visible = false;
  scene.add(hidden);
  const other = caster();
  other.layers.set(5);
  scene.add(other);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverShadow(renderer, light, scene, camera, null), true);
  assert.ok(calls.some((c) => c[0] === "shadow_frame"));
  const input = new Float64Array(memory.buffer, region("caster_input"), 4 + 2 * 151);
  assert.equal(input[1], -1, "no cache: the map is cleared");
  assert.equal(input[2], 1024);
  assert.equal(input[3], 1, "the one caster shown and on the camera's layers");
  const d = 4;
  assert.deepEqual([input[d + 2], input[d + 3]], [0, 3], "its range");
  assert.equal(input[d + 4], THREE.BackSide, "a front-sided material casts its back faces");
  assert.equal(input[d + 6], 2, "two uniforms");
  assert.deepEqual([...input.subarray(d + 7 + 18 + 2, d + 7 + 18 + 18)], light.shadow.camera.projectionMatrix.elements, "the shadow camera's projection");
  assert.deepEqual(calls.slice(-2), [["resetState"], ["setRenderTarget", graphics_target(renderer)]], "three's state reset after");
});

function graphics_target(renderer) { return renderer.getRenderTarget(); }

test("a caster without an owned depth material is skipped and reported, the rest of the pass drawn", () => {
  const { core, region, memory, scene, camera, renderer, properties } = bridge({ shadows: true });
  const light = cascade(), mesh = caster();
  mesh.customDepthMaterial = undefined;
  scene.add(mesh);
  scene.updateMatrixWorld(true);
  const owned = caster();
  scene.add(owned);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverShadow(renderer, light, scene, camera, null), true);
  assert.equal(new Float64Array(memory.buffer, region("caster_input"), 4)[3], 1, "only the owned caster is drawn");
  assert.match(core.errors.join("\n"), /Not drawn: .* Mesh material MeshBasicMaterial: no custom depth material/);
  assert.equal(core.failure, undefined);
});

test("a caster's geometry rewritten since: sent as three sends it, its update ranges merged, into the buffer as made", () => {
  const { core, calls, scene, camera, renderer, properties } = bridge({ shadows: true });
  const light = cascade(), mesh = caster();
  mesh.geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(12), 1));
  scene.add(mesh);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverShadow(renderer, light, scene, camera, null), true);
  const sent = (from) => calls.slice(from).filter((c) => /^geometry_(index|buffer)_/.test(c[0])).map((c) => [c[0], ...c.slice(2).map((v) => (ArrayBuffer.isView(v) ? v.byteLength : v))]);
  const pass = () => { const at = calls.length; core.coverShadow(renderer, light, scene, camera, null); return sent(at); };
  // Two ranges three merges (adjacent ones), as the crowd's mesh marks its
  // drawn index when it packs it again.
  const index = mesh.geometry.index;
  index.addUpdateRange(6, 3);
  index.addUpdateRange(3, 3);
  index.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_index_sub_data", 12, 24]], "elements 3 to 8 at their bytes, into the index as made");
  assert.equal(index.updateRanges.length, 2, "three's ranges left for three");
  assert.deepEqual(pass(), [], "nothing again while its version stands");
  // Marked twice since (or sent by three between, which clears its ranges):
  // the ranges no longer say all that changed, so all of it, as far as the
  // draw reads it.
  mesh.geometry.setDrawRange(0, 6);
  index.needsUpdate = true;
  index.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_index_sub_data", 0, 24]], "the six drawn");
  mesh.geometry.setDrawRange(0, 12);
  assert.deepEqual(pass(), [["geometry_index_sub_data", 24, 24]], "the rest once a draw reads it");
  index.clearUpdateRanges();
  index.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_index_sub_data", 0, 48]], "no ranges: all of it the draw reads");
  // A vertex source's range, in elements (a vertex's three floats each).
  const position = mesh.geometry.attributes.position;
  position.addUpdateRange(3, 6);
  position.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_buffer_sub_data", 0, 12, 24]], "slot 0, its bytes from the fourth float");
  position.needsUpdate = true;
  position.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_buffer_sub_data", 0, 0, 36]], "marked twice: all of it");
  // An array of another size: all of it, into an index made again.
  index.array = new Uint32Array(24);
  index.needsUpdate = true;
  assert.deepEqual(pass(), [["geometry_index_data", 96]]);
  // Another index altogether: three makes buffers for it, so three draws the pass.
  mesh.geometry.setIndex([0, 1, 2]);
  assert.equal(core.coverShadow(renderer, light, scene, camera, null), true);
  assert.match(core.errors.join("\n"), /geometry the renderer cannot hold/);
});
