// Rust links every owned program. Failure must stop loading, identify the GPU,
// and leave no cached entry that another frame could mistake for a valid draw.
import test from 'node:test';
import assert from 'node:assert/strict';
import { standinModule, standinThree } from './support/rust-standin.mjs';
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

// Catalog keys must be retained in diagnostics as well as the GPU identity.
// A source-free material has no native compilation path to recover through.
test('an owned program link error uses the same required-renderer failure path', () => {
  const { core } = fixture('add_owned_program', 'post-hud/copy');
  assert.throws(() => ownedProgram(core, 'post-hud/copy', 'position:0,uv:1'), /Fixture GPU: post-hud\/copy:.*fixture link rejected/);
  assert.equal(core.ownedPrograms.has('post-hud/copy'), false);
});

// Loading is an asynchronous browser boundary. An interrupted native admission
// resumes only after the resource bank and journal have handled restoration.
test('cold preparation resumes after context restoration without a fatal failure',async()=>{
 const {core}=fixture('add_owned_program','post-hud/copy');let lost=false,attempts=0;
 core.gl.isContextLost=()=>lost;
 const value=await core.prepare(()=>{
  attempts++;if(attempts===1){lost=true;core.renderer.domElement.dispatchEvent(new Event('webglcontextlost',{cancelable:true}));
   queueMicrotask(()=>{lost=false;core.renderer.domElement.dispatchEvent(new Event('webglcontextrestored'));});
   core.fail(Error('interrupted admission'));
  }return 37;
 });
 assert.equal(value,37);assert.equal(attempts,2);assert.equal(core.contextLost,false);
 assert.equal(core.failure,undefined);assert.deepEqual(core.errors,[]);
});
test('cold preparation keeps a genuine compiler rejection fatal',async()=>{
 const {core}=fixture('add_owned_program','post-hud/copy');
 await assert.rejects(core.prepare(()=>{throw Error('cold shader rejected');}),/Fixture GPU: cold shader rejected/);
 assert.equal(core.failure.rendererFailure,true);assert.equal(core.errors.length,1);
});
