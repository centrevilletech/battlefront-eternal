import test from 'node:test';
import assert from 'node:assert/strict';
import { RenderProfiler } from '../src/render-profiler.js';

test('restoration discards GPU queries and fences from the lost context', () => {
  const canvas=new EventTarget(), extension={TIME_ELAPSED_EXT:1,GPU_DISJOINT_EXT:2};
  let extensions=0;
  const gl={isContextLost:()=>false,getExtension(){extensions++;return extension;},fenceSync(){},getParameter(){return false;},createQuery(){return {};},beginQuery(){}};
  const profiler=new RenderProfiler({getContext:()=>gl,domElement:canvas});
  profiler.pending.push({});profiler.fences.push({sync:{}});profiler.active={};profiler.frame=7;
  canvas.dispatchEvent(new Event('webglcontextlost'));
  assert.equal(profiler.pending.length,0);assert.equal(profiler.fences.length,0);assert.equal(profiler.active,null);
  canvas.dispatchEvent(new Event('webglcontextrestored'));
  assert.equal(extensions,2);assert.equal(profiler.frame,0);
  profiler.begin();
  assert.ok(profiler.active,'a fresh query starts without polling dead handles');
});
