import test from 'node:test';
import assert from 'node:assert/strict';
import { generate } from '../tools/render-shaders.mjs';
import { programLayouts } from '../src/render-programs.js';
import { ownedProgram } from '../src/rust-render/programs.js';
import { ownedFullScreenPass } from '../src/render-passes.js';

test('shader artifacts match the source manifests', () => generate(true));
test('post and HUD register without a three program or shader reflection', () => {
  const calls = [];
  const core = { rust: { add_owned_program(key) { calls.push(key); return calls.length; } }, errors: [] };
  for (const [key, s] of Object.entries(programLayouts)) {
    if (!key.endsWith('/hud')) assert.equal(ownedFullScreenPass(key, {}).material.isShaderMaterial, undefined);
    const layout = s.attributes.map(([n,l]) => `${n}:${l}`).join(',');
    const entry = ownedProgram(core, key, layout);
    assert.ok(entry);
    assert.equal(ownedProgram(core, key, layout), entry);
  }
  assert.equal(calls.length, 12);
  assert.deepEqual(core.errors, []);
});
test('unknown variants and link failures are reported without a source bridge', () => {
  const core = { rust: { add_owned_program() { throw Error('link failed'); } }, errors: [] };
  assert.throws(() => ownedProgram(core, 'post-hud/copy', 'position:0,uv:1'), /post-hud\/copy:.*link failed/);
  assert.throws(() => ownedProgram(core, 'missing', ''), /unknown shader variant/);
  assert.equal(core.ownedPrograms.size, 0);
});
