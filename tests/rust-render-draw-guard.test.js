// Battle and interface scenes cannot reach a native render function. Named
// surfaces submit only through their required Rust owner; the guard counts
// accidental browser draws even when the call throws.
// These assertions cover the boundary independently of any particular battle.
import test from 'node:test';
import assert from 'node:assert/strict';
import { requireRustDraws, renderInterface, bindInterfaceRenderer } from '../src/rust-render/draw-guard.js';

test('three draws fail and named interface scenes require a Rust owner', () => {
  const calls = [], renderer = { getRenderTarget:()=>null,setRenderTarget(){},render: (...args) => calls.push(args) }, scene = {}, camera = {};
  requireRustDraws(renderer);
  requireRustDraws(renderer);
  assert.throws(() => renderer.render(scene, camera), /Rust pass is required/);
  assert.equal(renderer.battleThreeCalls, 1);
  assert.deepEqual(calls, []);
  assert.throws(() => renderInterface(renderer, scene, camera, 'classic-spawn'), /not initialized/);
  const rustCalls = [];
  bindInterfaceRenderer(renderer, { render: (...args) => rustCalls.push(args) });
  renderInterface(renderer, scene, camera, 'classic-spawn');
  assert.deepEqual(rustCalls, [[scene, camera]]);
  assert.deepEqual(calls, []);
  assert.throws(() => renderInterface(renderer, scene, camera, 'battle'), /Unknown interface screen/);
});

// Both compiler entry points are guarded before any source or reflection can
// be borrowed. Counter increments make an attempted first-use compile visible.
test('owned battle programs reject synchronous and asynchronous Three compilation',()=>{
 const renderer={};requireRustDraws(renderer);
 assert.throws(()=>renderer.compile(),/owned program is required/);
 assert.throws(()=>renderer.compileAsync(),/owned program is required/);
 assert.equal(renderer.battleCompileCalls,2);
});
