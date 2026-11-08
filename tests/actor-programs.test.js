import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {actorMaterial} from '../src/materials.js';
import {warmActorPrograms} from '../src/rust-render/actors.js';
import {RustRender} from '../src/rust-render/index.js';
globalThis.location ??= {search:''};
import {standinModule,standinThree} from './support/rust-standin.mjs';
import {programLayouts} from '../src/render-programs.js';

test('actor warmup settles each native catalog receipt and reuses it',async()=>{
 const {BfeRenderer,memory,calls}=standinModule(),{graphics}=standinThree(calls);
 const core=new RustRender(graphics,{memory},BfeRenderer),settled=[];
 const count=()=>calls.filter(row=>row[0]==='add_owned_program').length;
 await warmActorPrograms(core,async()=>settled.push(count()));
 assert.ok(settled.length>0);assert.deepEqual(settled,settled.map((_,i)=>i+1));
 const first=count();await warmActorPrograms(core,async()=>assert.fail('Cached preload must not settle'));
 assert.equal(count(),first);
});


test('fabric depth shells retain built-in texture transform values',async()=>{
 const {addFabricWind}=await import('../src/fabric.js');
 const {shadowPass}=await import('../src/rust-render/shadow.js');
 const map=new THREE.Texture();map.offset.set(.2,.3);map.repeat.set(2,3);map.updateMatrix();
 const mesh=new THREE.Mesh(new THREE.PlaneGeometry(1,1),new THREE.MeshBasicMaterial({map}));addFabricWind(mesh);
 const depth=mesh.customDepthMaterial,out=new Float64Array(20);
 assert.equal(depth.vertexShader,undefined);assert.equal(depth.uniforms,undefined);
 const pass=shadowPass({addView(){},cameras:{uv(map,out,offset){out.set(map.matrix.elements,offset);}}});
 assert.equal(pass.uniform(out,0,0,{name:'mapTransform',type:9},{object:mesh,depth},new THREE.PerspectiveCamera()),true);
 assert.deepEqual(Array.from(out.subarray(2,11)),Array.from(map.matrix.elements));
});


test('alpha-tested fabric selects its own depth descriptor and threshold',async()=>{
 const {addFabricWind}=await import('../src/fabric.js');
 const {shadowPass}=await import('../src/rust-render/shadow.js');
 const mesh=new THREE.Mesh(new THREE.PlaneGeometry(1,1),new THREE.MeshBasicMaterial({map:new THREE.Texture(),alphaTest:.5}));addFabricWind(mesh);
 assert.equal(mesh.customDepthMaterial.userData.bfeProgram,'actors/depth-fabric-cutout');
 assert.equal(mesh.customDepthMaterial.fragmentShader,undefined);
 const input=new Float64Array(20),pass=shadowPass({addView(){}});
 assert.equal(pass.uniform(input,0,0,{name:'alphaTest',type:1},{object:mesh,depth:mesh.customDepthMaterial},new THREE.PerspectiveCamera()),true);
 assert.equal(input[2],.5);
});


// The material space an actor can have is closed (describe() and partMaterial in
// materials.js and models.js): lit or unlit, cutout or blended or neither, scroll,
// heat, skinned or rigid, and a shadow setting's 0, 2, 3 or 4 cascades, with a
// lamp-table variant of every lit one. A cell with no program is a hard "Unowned
// actor variant" failure the first time a battle meets that material at that
// setting (a rigid unlit part without scroll under High shadows once did), so
// every cell is required here. Skin-bind bodies (RIGID_SKIN) are the one open
// family and are listed apart.
test('every cell of the actor material space has an owned program', () => {
  const have = new Set();
  for (const [key, layout] of Object.entries(programLayouts)) {
    const s = key.startsWith('actors/') && layout.selector;
    if (s) have.add(JSON.stringify([s.defines.map(([n]) => n).filter((n) => n !== 'ZERO_LIGHTS').sort(), s.mode, s.cascades, s.lamps]));
  }
  const holes = [];
  for (const kind of ['DYNAMIC', 'UNLIT'])
    for (const shape of [[], ['ALPHA_TEST'], ['BLENDED']])
      for (const scroll of [[], ['SCROLL']])
        for (const heat of [[], ['HEATED']])
          for (const mode of ['rigid', 'skinned'])
            for (const cascades of [0, 2, 3, 4])
              for (const lamps of kind === 'DYNAMIC' ? [false, true] : [false]) {
                const cell = JSON.stringify([[kind, ...shape, ...scroll, ...heat].sort(), mode, cascades, lamps]);
                if (!have.has(cell)) holes.push(cell);
              }
  assert.deepEqual(holes, []);
});


test('authored actor looks have no material or descriptor shell',()=>{const look=actorMaterial('dynamic',{scroll:[.2,.3],alphaTest:true});assert.equal(look.actorAppearance,true);assert.deepEqual(look.authored.scroll,[.2,.3]);for(const key of ['defines','uniforms','version','isMaterial','onBeforeCompile','onBeforeRender'])assert.equal(look[key],undefined);});
