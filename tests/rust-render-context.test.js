import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");

// The bridge over a stand-in for the WASM renderer and the page's graphics,
// its canvas an event target.
function bridge() {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 16) };
  class BfeRenderer {
    input_ptr() { return 0; }
    input_len() { return 200 + 76 * 4; }
    bones_ptr() { return 8192; }
    bones_len() { return 64; }
    // The transparent pass's views (src/rust-render/transparent.js): an empty list.
    transparent_keys_ptr() { return 16384; }
    transparent_keys_len() { return 64; }
    transparent_objects_ptr() { return 24576; }
    transparent_objects_len() { return 1 + 2 * 40; }
    transparent_object_stride() { return 40; }
    transparent_segments_ptr() { return 32768; }
    transparent_segments_len() { return 0; }
    transparent_segment() { return []; }
    records_at() { return 200; }
    record_stride() { return 76; }
  }
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  const target = { name: "drawing" };
  const renderer = {
    domElement: new EventTarget(),
    getContext: () => ({}),
    properties: { get: () => ({}) },
    shadowMap: { enabled: false },
    getRenderTarget: () => target,
    setRenderTarget(t) { calls.push(["setRenderTarget", t]); },
    resetState() { calls.push(["resetState"]); },
  };
  const graphics = { renderer, scene, camera, sceneTarget: target, farTarget: { texture: new THREE.Texture() }, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  return { core: new RustRender(graphics, { memory }, BfeRenderer), calls, renderer, scene, target };
}

test("a lost context gives everything back to three for good", () => {
  const { core, calls, renderer, scene, target } = bridge();
  const hidden = new THREE.ShaderMaterial();
  hidden.visible = false;
  scene.add(new THREE.Mesh(new THREE.BufferGeometry(), hidden));
  const warn = console.warn;
  console.warn = () => {};
  try { renderer.domElement.dispatchEvent(new Event("webglcontextlost")); } finally { console.warn = warn; }
  assert.equal(core.lost, true);
  assert.equal(core.enabled, false);
  assert.equal(hidden.visible, true, "what the renderer drew is three's again");
  assert.ok(core.errors.some((e) => e.includes("WebGL context lost")));
  assert.deepEqual(calls.slice(-2), [["resetState"], ["setRenderTarget", target]]);
});
