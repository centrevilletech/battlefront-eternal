// Compilation has two owners: Rust links its programs, while Three still
// compiles unowned material shells. Both failures must stop the same required
// drawer and identify the GPU. Failed program ids must never enter caches.
// The injected failure avoids relying on a particular driver's error wording.
// Assertions also reject the old activation/material-restoration handback API.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { standinModule, standinThree, fakeProgram, GL } from './support/rust-standin.mjs';
import { requireLinkedPrograms } from '../src/rust-render/failure.js';
import { ownedProgram } from '../src/rust-render/programs.js';
globalThis.location ??= { search: '' };
const { RustRender } = await import('../src/rust-render/index.js');

// Only the named key is rejected, so the passes that register their own
// programs at construction still start.
function fixture(method, only) {
  const cause = Error('fixture link rejected');
  const { BfeRenderer, memory, calls } = standinModule({ methods: { [method](key) { if (only === undefined || key === only) throw cause; return 1; } } });
  const { renderer, graphics } = standinThree(calls);
  const gl = renderer.getContext();
  gl.getExtension = () => ({ UNMASKED_RENDERER_WEBGL: 1 });
  gl.getParameter = () => 'Fixture GPU';
  return { core: new RustRender(graphics, { memory }, BfeRenderer), cause };
}

// A failed link must leave no cached null/program entry that later frames
// could mistake for an intentionally unsupported item and silently omit.
test('a rewritten program link error names the GPU and never caches a skipped program', () => {
  const { core, cause } = fixture('add_program');
  const native = fakeProgram([['projectionMatrix', GL.FLOAT_MAT4]], ['position']);
  assert.throws(() => core.program(native, 0, new THREE.ShaderMaterial(), { uniforms: new Set(), material: new Set() }), error => {
    assert.equal(error.message, 'Battle renderer failed on Fixture GPU: fixture link rejected');
    assert.equal(error.cause, cause);return true;
  });
  assert.equal(core.programs.has(native), false);
  assert.equal(core.setActive, undefined);
  assert.equal(core.restore, undefined);
});

// Catalog keys must be retained in diagnostics as well as the GPU identity.
// A source-free material has no native compilation path to recover through.
test('an owned program link error uses the same required-renderer failure path', () => {
  const { core } = fixture('add_owned_program', 'post-hud/copy');
  const material = new THREE.ShaderMaterial();material.userData.bfeProgram = 'post-hud/copy';
  assert.throws(() => ownedProgram(core, 'post-hud/copy', 'position:0,uv:1'), /Fixture GPU: post-hud\/copy:.*fixture link rejected/);
  assert.equal(core.ownedPrograms.has('post-hud/copy'), false);
});

// The native compiler remains a resource dependency, so its link callback
// must fail just as loudly as a link performed directly by the Rust core.
// Merely enabling Three's default console diagnostics would not stop loading.
test('the retained native compiler reports link rejection with its GPU', () => {
 const gl={getExtension:()=>null,getParameter:()=>"Fixture GPU",getProgramInfoLog:()=>"invalid interface"};
 const renderer={debug:{},getContext:()=>gl};
 requireLinkedPrograms(renderer);
 assert.equal(renderer.debug.checkShaderErrors,true);
 assert.throws(()=>renderer.debug.onShaderError(gl,{}),/Fixture GPU: Shader program failed to link: invalid interface/);
});
