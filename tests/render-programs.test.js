import test from 'node:test';
import assert from 'node:assert/strict';
import { generate } from '../tools/render-shaders.mjs';
import { shaderSources } from '../src/render-shaders.js';
import { ownedProgram } from '../src/rust-render/programs.js';
import { ownedFullScreenPass } from '../src/render-passes.js';
import { Batch } from '../src/interface/batch.js';
import * as THREE from 'three';

test('shader artifacts match the source manifests', () => generate(true));
test('post and HUD register without a three program or shader reflection', () => {
  const calls = [];
  const core = { rust: { add_owned_program(key) { calls.push(key); return calls.length; } }, errors: [] };
  for (const [key, s] of Object.entries(shaderSources)) {
    const material = key.endsWith('/hud') ? new Batch().material(new THREE.Texture(), null) : ownedFullScreenPass(key, {}).material;
    const layout = s.attributes.map(([n,l]) => `${n}:${l}`).join(',');
    const entry = ownedProgram(core, material, layout);
    assert.ok(entry);
    assert.equal(ownedProgram(core, material, layout), entry);
  }
  assert.equal(calls.length, 12);
  assert.deepEqual(core.errors, []);
});
test('unknown or failed source variants are cached as refused and leave the pass to three', () => {
  const core = { rust: { add_owned_program() { throw Error('link failed'); } }, errors: [] };
  const material = { userData: { bfeProgram: 'post-hud/copy' } };
  assert.equal(ownedProgram(core, material, 'position:0,uv:1'), null);
  assert.equal(ownedProgram(core, material, 'position:0,uv:1'), null);
  assert.equal(core.errors.length, 1);
  assert.equal(ownedProgram(core, { userData: { bfeProgram: 'missing' } }, ''), null);
  assert.equal(core.errors.length, 2);
});
