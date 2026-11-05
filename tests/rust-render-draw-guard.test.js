// A loaded battle cannot reach the retained native render function directly.
// Only a named interface scene can use it; a mistyped label must not silently
// become an exemption. The counter records attempts even when the guard throws.
// These assertions cover the boundary independently of any particular battle.
import test from 'node:test';
import assert from 'node:assert/strict';
import { requireRustDraws, renderInterface } from '../src/rust-render/draw-guard.js';

test('three draws fail during a battle while named interface scenes remain drawable', () => {
  const calls = [], renderer = { getRenderTarget:()=>null,setRenderTarget(){},render: (...args) => calls.push(args) }, scene = {}, camera = {};
  requireRustDraws(renderer);
  requireRustDraws(renderer);
  assert.throws(() => renderer.render(scene, camera), /Rust pass is required/);
  assert.equal(renderer.battleThreeCalls, 1);
  assert.deepEqual(calls, []);
  renderInterface(renderer, scene, camera, 'classic-spawn');
  assert.deepEqual(calls, [[scene, camera]]);
  assert.throws(() => renderInterface(renderer, scene, camera, 'battle'), /Unknown interface screen/);
});
