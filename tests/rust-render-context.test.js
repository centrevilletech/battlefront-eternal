import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { standinModule, standinThree } from "./support/rust-standin.mjs";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");

test("a lost context gives everything back to three for good", () => {
  const { BfeRenderer, memory, calls } = standinModule();
  const { renderer, graphics, scene } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
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
  assert.deepEqual(calls.slice(-2), [["resetState"], ["setRenderTarget", graphics.sceneTarget]]);
});
