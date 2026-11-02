// The Rust renderer as committed in src/rust-pkg/ (docs/development.md,
// "The Rust parts"), checked without Rust: wasm-bindgen's glue and the
// module are one build. The glue's imports instantiate the module, and every
// export the glue calls is the module's. A merge that kept one file from
// each side fails here. CI's rust job checks both against a rebuild from
// rust/bfe-render.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const pkg = new URL('../src/rust-pkg/', import.meta.url);

test('the Rust renderer module instantiates with its committed glue', async () => {
  const { initSync, BfeRenderer } = await import(new URL('bfe_render.js', pkg));
  const bytes = readFileSync(new URL('bfe_render_bg.wasm', pkg));
  const exported = new Set(WebAssembly.Module.exports(new WebAssembly.Module(bytes)).map((e) => e.name));
  const called = new Set(readFileSync(new URL('bfe_render.js', pkg), 'utf8').match(/\bwasm\.\w+/g).map((name) => name.slice(5)));
  assert.deepEqual([...called].filter((name) => !exported.has(name)), []);
  const exports = initSync({ module: bytes });
  assert.ok(exports.memory instanceof WebAssembly.Memory);
  assert.equal(typeof BfeRenderer, 'function');
});
