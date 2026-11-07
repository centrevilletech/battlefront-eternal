import test from 'node:test';
import { programLayouts } from '../src/render-programs.js';
import { expand } from '../tools/renderer-corpus/manifest.mjs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const manifest = expand(JSON.parse(readFileSync(new URL('../tools/renderer-corpus/manifest.json', import.meta.url))));

test('the parity corpus includes every map in every preset with valid spawn queries', () => {
  const maps = 'bes1 bes2 end1 geo1 hot1 kam1 kas1 kas2 nab1 nab2 rhn1 rhn2 tat1 tat2 tat3 yav1 yav2'.split(' ');
  for (const preset of ['classic','balanced','high']) for (const map of maps) {
    const c = manifest.cases.find(c => c.id === `${preset}-${map}`);
    assert.ok(c);
    const q = new URLSearchParams(c.query);
    assert.equal(q.get('map'), map);
    assert.equal(q.get('view'), '1st');
    assert.equal(c.settings.antialiasing, '2x');
    assert.equal(c.settings.dynamicResolution, 'off');
    assert.equal(q.get('side'), ['end1','tat2','tat3','hot1'].includes(map) ? 'imp' : 'rep');
    assert.equal(q.get('unit'), map === 'hot1' ? 'imp_inf_storm_troopersnow' : q.get('side') === 'imp' ? 'imp_inf_storm_trooper' : 'rep_inf_clone_trooper');
  }
  assert.equal(new Set(manifest.cases.map(c => c.id)).size, manifest.cases.length);
  for (const key of Object.keys(programLayouts).filter(key => key.startsWith("post-hud/"))) assert.ok(manifest.programs.includes(key));
  for (const key of manifest.programs) assert.ok(programLayouts[key]);
  assert.equal(manifest.contextLossOwner, 'renderer lifecycle gate');
  for (const c of manifest.cases) if (c.settings.renderScale < 1) assert.notEqual(c.settings.upscaler, "off", "scaled cases retain the native canvas for HUD capture");
  for (const map of ['kas1','kas2','kam1']) assert.equal(manifest.cases.find(c=>c.id===`mirror-${map}`).settings.waterQuality,'ultra');
});

import { compare, compareCase } from '../tools/renderer-corpus/actor-compare.mjs';
test('frozen comparison requires exact stable channels and uses only local control noise', () => {
  const a=Uint8Array.of(10,20,30,255,10,20,30,255);
  assert.deepEqual(compare(a,a,a),{pixels:2,differing:0,control:0,residual:0,stableResidual:0,maxDelta:0,maxExcess:0});
  const noisy=Uint8Array.of(12,20,30,255,10,20,30,255);
  assert.equal(compare(a,noisy,noisy).residual,0);
  assert.equal(compare(a,Uint8Array.of(10,22,30,255,10,20,30,255),noisy).residual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,255,12,20,30,255),noisy).stableResidual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,254,10,20,30,255),a).residual,1);
});

import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
test('exact shader replay cannot hide a failed frozen whole-frame baseline', () => {
  const out=mkdtempSync(join(tmpdir(),'renderer-golden-'));
  try {
    for(const role of ['reference','candidate','control']) {
      const dir=join(out,`case-${role}`);mkdirSync(dir);
      const points=Array.from({length:4},(_,k)=>{
        const file=join(dir,`${k}.bin`);writeFileSync(file,Uint8Array.of(role==='candidate'?1:0,0,0,255));
        return {tick:k,stages:{presented:{w:1,h:1,bytes:1,file}},...(role==='reference'?{selfReplay:[{key:'post-hud/copy',differing:0}]}:{replay:[{key:'post-hud/copy',differing:0}]})};
      });
      writeFileSync(join(dir,'capture.json'),JSON.stringify({valid:true,points}));
    }
    const r=compareCase(out,'case');
    assert.equal(r.pass,true);assert.equal(r.frameExact,false);assert.equal(r.baselinePass,false);
    assert.equal(r.points[0].stages.presented.stableResidual,1);
  } finally {rmSync(out,{recursive:true});}
});

import { actorManifest, observedPrograms } from '../tools/renderer-corpus/actors-manifest.mjs';
test('actor corpus crosses every map, quality and cascade setting',()=>{
 const base=JSON.parse(readFileSync(new URL('../tools/renderer-corpus/manifest.json',import.meta.url)));
 const actors=actorManifest(base,observedPrograms(programLayouts)),expanded=expand(actors);
 const generated=JSON.parse(readFileSync(new URL('../tools/renderer-corpus/actors-manifest.json',import.meta.url)));
 assert.deepEqual(generated,actors);
 assert.equal(new Set(actors.cases.map(c=>c.id)).size,312);
 for(const preset of ['classic','balanced','high'])for(const map of new Set(manifest.cases.filter(c=>c.preset===preset).map(c=>new URLSearchParams(c.query).get('map'))))for(const shadows of ['off','low','medium','high','ultra']){
  assert.ok(expanded.cases.some(c=>c.preset===preset && new URLSearchParams(c.query).get('map')===map && c.settings.shadows===shadows));
 }
 for(const c of actors.cases.filter(c=>c.actorFocus)){
  assert.notEqual(new URLSearchParams(c.query).get('unit'),c.actorFocus);
  assert.equal(new URLSearchParams(c.query).get('heroes'),'1');
 }
 assert.equal(actors.cases.filter(c=>c.actorPieces).length,15);
 assert.equal(actors.cases.filter(c=>c.actorSkinnedCutout).length,3);
 assert.ok(actors.programs.every(key=>key.startsWith('actors/') || key.startsWith('post-hud/')));
});

import * as THREE from 'three';
import { installActorWreck } from '../tools/renderer-corpus/actor-wreck.mjs';
test('wreck replay retains its heated presentation when the live station cools',()=>{
 const previous=globalThis.__bf,heated={},look={heat:.85},mesh={isSkinnedMesh:true,material:heated,userData:{heated:look}};
 const root={visible:false},site={meshes:[mesh],look,centre:new THREE.Vector3()},station={looks:{destroyed:{root}}};
 let rendered=0,prepared=false;
 const graphics={camera:new THREE.PerspectiveCamera(),scene:{},renderer:{compile(proxy,camera,scene){
  assert.equal(camera,graphics.camera);assert.equal(scene,graphics.scene);
  const objects=[];proxy.traverse(o=>objects.push(o));assert.deepEqual(objects,[mesh]);
  proxy.traverseVisible(()=>assert.fail('The target scene already supplies the lights'));prepared=true;
 }},rust:{byName:{instances:{registerRig(object){assert.equal(object,mesh);assert.equal(prepared,true);return {};}}}},warmup(){},render(){rendered++;}};
 globalThis.__bf={THREE,graphics,destruction:{stations:[station],wreck(){return site;}}};
 try{
  assert.equal(installActorWreck(true).skinned,1);
  site.meshes.length=0;mesh.material={};delete mesh.userData.heated;
  graphics.render();
  assert.equal(mesh.material,heated);assert.equal(mesh.userData.heated,look);
  assert.equal(root.visible,true);assert.equal(rendered,1);
 }finally{globalThis.__bf=previous;}
});
