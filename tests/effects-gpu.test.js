import test from 'node:test';
import assert from 'node:assert/strict';
import * as T from 'three';
import {Effects} from '../src/effects.js';
import {emitter,library,seeded} from './fixtures/effects.js';
const texture=new T.DataTexture(new Uint8Array([255,255,255,255]),1,1);
function make(defs=library(),gpu=true){return new Effects(new T.Scene(),defs,{texture:()=>texture,gpu});}
function state(fx){return fx.effects.map(e=>({time:e.time,stopped:e.stopped,done:e.done,position:e.position.toArray(),emitters:e.emitters.map(em=>({emitted:em.emitted,next:em.nextBurst,particles:em.particles.map(p=>({birth:p.birth,life:p.life,position:p.position.toArray(),velocity:p.velocity.toArray(),size:p.size,color:p.color,rotation:p.rotation,spin:p.spin,child:!!p.child}))}))}));}

test('GPU path preserves randomized emission, paths, child emitters and stop/kill timing',()=>{
  const defs=library();defs.child=[emitter({geometry:{type:'emitter',emitters:[emitter()]}})];
  const systems=[false,true].map(gpu=>seeded(()=>{
    const fx=make(defs,gpu);for(const name of Object.keys(defs))fx.play(name,new T.Vector3());return fx;
  }));
  for(let frame=0;frame<260;frame++){
    for(const fx of systems)seeded(()=>{
      for(const e of fx.effects)e.move(new T.Vector3(frame*.01,0,0));
      if(frame===20)fx.effects[0]?.stop();if(frame===30)fx.effects[1]?.kill();
      fx.update([1/30,1/144,0,.03][frame%4]);
    },frame+7);
    assert.deepEqual(state(systems[1]),state(systems[0]));
  }
  assert.equal(systems[1].effects.length,0);
  assert.ok([...systems[1].renderer.batches.values()].every(b=>!b.mesh.visible&&b.records.length===0));
});

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
  let draws=0;const original=fx.draw;fx.draw=function(...args){draws++;return original.apply(this,args);};
  fx.play('long',new T.Vector3());fx.update(.1);assert.equal(draws,4);
  assert.equal([...fx.renderer.batches.values()][0].count,4);
});

test('warmup outside update is replaced by the next complete presentation',()=>{
  const fx=make({a:[emitter({maxParticles:[-1,-1]})]});
  const e=fx.play('a',new T.Vector3());fx.step(e,2);fx.update(0);
  const batch=[...fx.renderer.batches.values()][0];
  assert.equal(batch.count,e.emitters[0].particles.length);assert.equal(fx.gpu.data[8],2);
  e.kill();fx.update(0);assert.equal(batch.mesh.visible,false);
});

test('long-running emitters rebase both clock and live births without losing sub-frame motion',()=>{
  const fx=make();const e=fx.play('particle',new T.Vector3());fx.update(0);
  e.time=36_000;
  for(const p of e.emitters[0].particles)p.birth+=36_000;
  fx.update(1/240);
  const batch=[...fx.renderer.batches.values()][0];
  const age=fx.gpu.data[8]-batch.arrays.spawnMeta[0];
  assert.ok(Math.abs(age-1/240)<.00001,`age ${age}`);
  assert.ok(fx.gpu.data[8]<256);
  assert.equal(batch.geometry.instanceCount,4);
});

test('GPU emitter tables stay bounded; excess sources keep the CPU fallback',()=>{
  const fx=make();const e=fx.play('particle',new T.Vector3());
  fx.gpu.begin();
  for(let i=0;i<1024;i++)assert.equal(fx.gpu.register(e),i);
  assert.equal(fx.gpu.register(e),-1);
  assert.equal(fx.gpu.uniforms.emitterRows.value,1024);
  assert.equal(fx.gpu.data.byteLength,49152);
});
