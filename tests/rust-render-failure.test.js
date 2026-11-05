import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { standinModule, standinThree, fakeProgram, GL } from './support/rust-standin.mjs';
import { requireLinkedPrograms } from '../src/rust-render/failure.js';
import { ownedProgram } from '../src/rust-render/programs.js';
globalThis.location ??= { search: '' };
const { RustRender } = await import('../src/rust-render/index.js');

function fixture(method) {
  const cause = Error('fixture link rejected');
  const { BfeRenderer, memory, calls } = standinModule({ methods: { [method]() { throw cause; } } });
  const { renderer, graphics } = standinThree(calls);
  const gl = renderer.getContext();
  gl.getExtension = () => ({ UNMASKED_RENDERER_WEBGL: 1 });
  gl.getParameter = () => 'Fixture GPU';
  return { core: new RustRender(graphics, { memory }, BfeRenderer), cause };
}

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

test('an owned program link error uses the same required-renderer failure path', () => {
  const { core } = fixture('add_owned_program');
  const material = new THREE.ShaderMaterial();material.userData.bfeProgram = 'post-hud/copy';
  assert.throws(() => ownedProgram(core, 'post-hud/copy', 'position:0,uv:1'), /Fixture GPU: post-hud\/copy:.*fixture link rejected/);
  assert.equal(core.ownedPrograms.has('post-hud/copy'), false);
});

test('the retained native compiler reports link rejection with its GPU', () => {
 const gl={getExtension:()=>null,getParameter:()=>"Fixture GPU",getProgramInfoLog:()=>"invalid interface"};
 const renderer={debug:{},getContext:()=>gl};
 requireLinkedPrograms(renderer);
 assert.equal(renderer.debug.checkShaderErrors,true);
 assert.throws(()=>renderer.debug.onShaderError(gl,{}),/Fixture GPU: Shader program failed to link: invalid interface/);
});
