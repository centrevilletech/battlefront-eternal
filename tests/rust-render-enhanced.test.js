import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { standinModule, standinThree, fakeProgram, GL } from "./support/rust-standin.mjs";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { lighting } = await import("../src/materials.js");

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

test("the configuration the battle started in is the one the renderer covers, Enhanced ones included", () => {
  const { core, settings, graphics, renderer } = bridge({ shadows: true, lamps: true });
  graphics.upscaling = "temporal";
  assert.equal(core.supported(), true, "shadows, lamps and the temporal upscaler: covered, item by item");
  assert.equal(core.configKey(), "2:1:1");
  settings.set("lampLighting", false);
  assert.equal(core.supported(), false, "a change mid-battle: three's for the rest of it");
  settings.set("lampLighting", true);
  renderer.shadowMap.enabled = false;
  assert.equal(core.supported(), false);
});

test("a shadow receiver's program is the renderer's only from a pass that feeds shadows, with shadows on", () => withCascades(() => {
  const { core, material } = bridge({ shadows: true });
  // A world draw's receiveShadow is the pass's own value (world.js MATERIAL); an instance's is its row's.
  const feeds = { uniforms: new Set(["map", "receiveShadow"]), material: new Set(["receiveShadow"]), shadows: true };
  const instanced = { uniforms: new Set(["map"]), material: new Set(), shadows: true };
  const not = { uniforms: new Set(["map", "receiveShadow"]), material: new Set(["receiveShadow"]) };
  assert.ok(core.program(fakeProgram(RECEIVER), WORLD, material, feeds), "world: the block and the maps are the core's; receiveShadow the pass's");
  assert.ok(core.program(fakeProgram(RECEIVER), SKINNED, material, instanced), "an instance's receiveShadow is its row's");
  assert.equal(core.program(fakeProgram(RECEIVER), WORLD, material, instanced), null, "a world pass that gives no receiveShadow");
  const p = fakeProgram(RECEIVER);
  assert.equal(core.program(p, WORLD, material, not), null, "a pass that doesn't feed them");
  assert.ok(core.errors.includes(`p${p.id} receiveShadow (shadows)`) || core.errors.some((e) => e.startsWith(`p${p.id} `) && e.endsWith("(shadows)")));
  assert.equal(core.program(fakeProgram(RECEIVER), EFFECT, material, feeds), null, "the effect kind is never a receiver");
}));

test("with shadows off nothing of the shadow block is the renderer's to give", () => {
  const { core, material } = bridge({ shadows: false });
  const feeds = { uniforms: new Set(["map"]), material: new Set(), shadows: true };
  assert.equal(core.program(fakeProgram(RECEIVER), WORLD, material, feeds), null);
});

test("with lamps on a DYNAMIC program is the renderer's only from a pass that feeds lamps", () => {
  const { core, material } = bridge({ lamps: true });
  const uniforms = new Set(["map", "localLightPositions", "localLightColors", "localLightCount"]);
  const p = fakeProgram([["map", GL.SAMPLER_2D], ...LAMPS]);
  assert.equal(core.program(p, SKINNED, material, { uniforms, material: new Set() }), null);
  assert.ok(core.errors.includes(`p${p.id} localLightPositions (lamps)`));
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
  // Off (or disabled), nothing is the renderer's: the render runs as it is.
  core.setActive(false);
  log.length = 0;
  core.reveal(() => log.push(["render"]));
  assert.deepEqual(log, [["render"]]);
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
