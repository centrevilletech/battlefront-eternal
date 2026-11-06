import test from 'node:test';
import assert from 'node:assert/strict';
import * as T from 'three';
import {Effects} from './helpers/presentation.js';
import {Header as H} from '../src/presentation/layout.js';
import {emitter,library} from './fixtures/effects.js';
const texture=new T.DataTexture(new Uint8Array([255,255,255,255]),1,1);
function make(defs=library(),gpu=true){return new Effects(new T.Scene(),defs,{texture:()=>texture,gpu});}
test('animation, moving emitters and tint do not rewrite live spawn buffers',()=>{
  const fx=make();const effect=fx.play('particle',new T.Vector3());fx.update(0);
  const batch=[...fx.renderer.batches.values()][0];
  const versions=Object.values(batch.geometry.attributes).map(a=>a.version);
  for(let i=0;i<120;i++){
    effect.move(new T.Vector3(i*.01,0,0));effect.tint=.7;fx.update(1/120);
  }
  assert.deepEqual(Object.values(batch.geometry.attributes).map(a=>a.version),versions);
  assert.equal(batch.geometry.instanceCount,4);
  assert.equal(fx.gpu.data[0],Math.fround(1.19));assert.equal(fx.gpu.data[3],Math.fround(.7));
});

test('different tracks share texture batches; expired references and uploads remain bounded',()=>{
  const fx=make({a:[emitter({maxParticles:[-1,-1],burstDelay:[.02,.02]})],b:[emitter()]});
  fx.play('a',new T.Vector3());fx.play('b',new T.Vector3());
  for(let i=0;i<1000;i++)fx.update(1/60);
  assert.equal(fx.renderer.batches.size,1);
  const batch=[...fx.renderer.batches.values()][0];
  assert.ok(batch.count<=604&&batch.capacity<=1024);
  assert.equal(batch.records.length,batch.count);
  for(const a of Object.values(batch.geometry.attributes))assert.ok(a.updateRanges.length<=1);
  for(const e of fx.effects)e.kill();fx.update(0);
  assert.equal(batch.records.length,0);assert.equal(batch.mesh.visible,false);
});

test('compacting emitter slots refreshes affected particles; state texture grows and disposes',()=>{
  const fx=make();const handles=[];
  for(let i=0;i<40;i++)handles.push(fx.play('particle',new T.Vector3(i,0,0)));
  fx.update(0);assert.equal(fx.gpu.uniforms.emitterRows.value,64);
  handles[0].kill();fx.update(.1);
  const batch=[...fx.renderer.batches.values()][0];
  assert.equal(batch.arrays.spawnMeta[2],0);assert.equal(fx.gpu.data[0],1);
  let disposed=0;
  for(const u of [fx.gpu.uniforms.tracks,fx.gpu.uniforms.emitters])u.value.addEventListener('dispose',()=>disposed++);
  fx.gpu.dispose();assert.equal(disposed,2);
});

test('long tracks and unsupported geometry fall back without truncating authored effects',()=>{
  const def=emitter();def.transformer.position=Array.from({length:9},()=>({lifetime:.1,accelerate:[0,1,0]}));
  const fx=make({long:[def],blur:[emitter({geometry:{type:'particle',texture:'fixture',blend:'blur'}})]});
  assert.equal(fx.gpu.definitions.size,0);
  fx.play('long',new T.Vector3());fx.update(.1);assert.equal(fx.header[H.DRAWS],4);
  assert.equal([...fx.renderer.batches.values()][0].count,4);
});

test('warmup outside update is replaced by the next complete presentation',()=>{
  const fx=make({a:[emitter({maxParticles:[-1,-1]})]});
  const e=fx.play('a',new T.Vector3());fx.step(e,2);fx.update(0);
  const batch=[...fx.renderer.batches.values()][0];
  assert.equal(batch.count,fx.particleCount);assert.equal(fx.gpu.data[8],2);
  e.kill();fx.update(0);assert.equal(batch.mesh.visible,false);
});

test('prewarming a later source retains its pending GPU upload range',()=>{
  const fx=make({a:[emitter({maxParticles:[-1,-1]})]});
  fx.play('a',new T.Vector3());fx.update(0);
  const batch=[...fx.renderer.batches.values()][0],before=batch.count;
  // The renderer has consumed the existing prefix. A source entering view
  // must publish its warm particles even when they share that same batch.
  batch.external=true;batch.unread=Infinity;
  const later=fx.play('a',new T.Vector3());fx.step(later,2);fx.update(0);
  assert.equal(batch.count,fx.particleCount);
  assert.equal(batch.unread,before);
});

test('GPU emitter tables stay bounded; excess sources keep the CPU fallback',()=>{
  const fx=make();fx.play('particle',new T.Vector3());fx.update(0);
  fx.gpu.begin();
  for(let i=0;i<1024;i++)assert.equal(fx.gpu.registerRecord(fx.effectOutput,0),i);
  assert.equal(fx.gpu.registerRecord(fx.effectOutput,0),-1);
  assert.equal(fx.gpu.uniforms.emitterRows.value,1024);
  assert.equal(fx.gpu.data.byteLength,49152);
});
