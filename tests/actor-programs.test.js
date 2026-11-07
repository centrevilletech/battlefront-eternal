import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {actorMaterial} from '../src/materials.js';
import {actorProgram,warmActorPrograms} from '../src/rust-render/actors.js';
import {compileUnowned} from '../src/rust-render/world-programs.js';
import {programLayouts} from '../src/render-programs.js';

test('actor materials carry values without browser GLSL or shader hooks',()=>{
 const material=actorMaterial('dynamic',{scroll:[.2,.3],alphaTest:true});
 assert.equal(material.isShaderMaterial,undefined);
 assert.equal(material.vertexShader,undefined);
 assert.equal(material.fragmentShader,undefined);
 assert.equal(material.onBeforeCompile,THREE.Material.prototype.onBeforeCompile);
 assert.equal(material.visible,false);
 assert.equal(material.forceSinglePass,true);
 assert.equal(material.fog,false);
 assert.equal(material.lights,undefined);
 assert.equal(material.uniforms.directionalLights,undefined);
 assert.deepEqual(material.uniforms.scroll.value.toArray(),[.2,.3]);
});

test('actor selection separates skin and lamp contracts and refuses gaps',()=>{
 const calls=[],core={graphics:{settings:{get:()=>false},csm:null},rust:{add_owned_program(key){calls.push(key);return calls.length;}}};
 const rigid=actorMaterial('dynamic'),skinned=actorMaterial('dynamic');skinned.defines.RIGID_SKIN='';
 const a=actorProgram(core,rigid,2),b=actorProgram(core,skinned,1);
 assert.notEqual(a.id,b.id);
 assert.equal(actorProgram(core,rigid,2),a);
 core.graphics.settings.get=()=>true;
 assert.notEqual(actorProgram(core,rigid,2).id,a.id);
 rigid.defines.UNKNOWN_FLAG='';
 assert.throws(()=>actorProgram(core,rigid,2),/Unowned actor variant/);
 assert.equal(calls.length,3);
});

test('browser preparation excludes owned actor shells and keeps scene lighting',()=>{
 const scene=new THREE.Scene(),owned=new THREE.Mesh(new THREE.BoxGeometry(),actorMaterial('dynamic'));
 const browser=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.MeshBasicMaterial()),light=new THREE.PointLight();
 const shadow=new THREE.Mesh(new THREE.BoxGeometry(),new THREE.Material());shadow.userData.shadowOnly=true;
 scene.add(owned,browser,light,shadow);
 const visited=[],renderer={compile(root,camera,target){
  assert.equal(target,scene);root.traverse(o=>{if(o.material || o.isLight)visited.push(o);});
  const lights=[];const collect=o=>{if(o.isLight)lights.push(o);};
  target.traverseVisible(collect);if(root!==target)root.traverseVisible(collect);
  assert.deepEqual(lights,[light],'a filtered compile must count each scene light once');
 }};
 compileUnowned(renderer,scene,new THREE.PerspectiveCamera());
 assert.ok(visited.includes(browser));assert.ok(visited.includes(light));assert.ok(!visited.includes(owned));assert.ok(!visited.includes(shadow));
});


test('actor warmup settles each new program and reuses the cache',async()=>{
 const calls=[],settled=[],target={},core={graphics:{settings:{get:()=>false},csm:null},renderer:{getRenderTarget:()=>target,resetState(){},setRenderTarget(t){assert.equal(t,target);}},rust:{add_owned_program(key){calls.push(key);return calls.length;}}};
 await warmActorPrograms(core,async()=>{settled.push(calls.length);});
 assert.ok(calls.length>0);
 assert.deepEqual(settled,calls.map((_,i)=>i+1));
 const first=calls.length;
 await warmActorPrograms(core,async()=>assert.fail('Cached warmup must not settle'));
 assert.equal(calls.length,first);
});


test('fabric depth shells retain built-in texture transform values',async()=>{
 const {addFabricWind}=await import('../src/fabric.js');
 const {shadowPass}=await import('../src/rust-render/shadow.js');
 const map=new THREE.Texture();map.offset.set(.2,.3);map.repeat.set(2,3);map.updateMatrix();
 const mesh=new THREE.Mesh(new THREE.PlaneGeometry(1,1),new THREE.MeshBasicMaterial({map}));addFabricWind(mesh);
 const depth=mesh.customDepthMaterial,out=new Float64Array(20);
 assert.equal(depth.vertexShader,undefined);assert.equal(depth.uniforms,undefined);
 const pass=shadowPass({});
 assert.equal(pass.uniform(out,0,0,{name:'mapTransform',type:9},{object:mesh,depth},new THREE.PerspectiveCamera()),true);
 assert.deepEqual(Array.from(out.subarray(2,11)),Array.from(map.matrix.elements));
});


test('heated actor materials have no browser draw hook',async()=>{
 const {heatMaterial}=await import('../src/destruction-debris.js');
 const material=heatMaterial(actorMaterial('dynamic'));
 assert.equal(material.fragmentShader,undefined);
 assert.equal(material.onBeforeRender,THREE.Material.prototype.onBeforeRender);
 assert.equal(material.onBeforeCompile,THREE.Material.prototype.onBeforeCompile);
 assert.equal(material.defines.HEATED,'');
 assert.equal(material.uniforms.heat,undefined,'owned heat comes from the instance row');
});


test('model parts and their heated derivatives have no browser sources',async()=>{
 const {partMaterial}=await import('../src/models.js');
 const {heatMaterial}=await import('../src/destruction-debris.js');
 const texture=new THREE.Texture(),material=partMaterial({flags:1,texture:'part'},()=>texture);
 assert.equal(material.userData.actorShader,'zero');
 assert.equal(material.vertexShader,undefined);
 assert.equal(material.fragmentShader,undefined);
 assert.equal(heatMaterial(material).userData.actorShader,'zero');
 assert.throws(()=>heatMaterial(new THREE.ShaderMaterial()),/owned model material/);
});

test('pending actor maps adopt loaded pixels once without a browser draw',async()=>{
 const {actorMap,trackActorMaps,refreshActorMaps}=await import('../src/rust-render/actors.js');
 const map=new THREE.Texture(),material=actorMaterial('dynamic',{map}),properties=new WeakMap();let uploads=0,adoptions=0;
 const core={renderer:{properties:{get(texture){if(!properties.has(texture))properties.set(texture,{});return properties.get(texture);}},initTexture(texture){uploads++;Object.assign(this.properties.get(texture),{__version:texture.version,__webglTexture:{}});}},texture(texture){adoptions++;return properties.get(texture)?.__webglTexture?7:-1;}};
 const record={textures:[actorMap(core,material)]};trackActorMaps(record,[material]);
 assert.deepEqual(record.textures,[-1]);const before=adoptions;
 refreshActorMaps(core,record);assert.equal(adoptions,before);
 map.image={width:1,height:1};map.needsUpdate=true;
 refreshActorMaps(core,record);assert.deepEqual(record.textures,[7]);assert.equal(uploads,1);
 const adopted=adoptions;refreshActorMaps(core,record);assert.equal(adoptions,adopted);
 assert.equal(material.visible,false);assert.equal(material.vertexShader,undefined);
});


test('alpha-tested fabric selects its own depth descriptor and threshold',async()=>{
 const {addFabricWind}=await import('../src/fabric.js');
 const {shadowPass}=await import('../src/rust-render/shadow.js');
 const mesh=new THREE.Mesh(new THREE.PlaneGeometry(1,1),new THREE.MeshBasicMaterial({map:new THREE.Texture(),alphaTest:.5}));addFabricWind(mesh);
 assert.equal(mesh.customDepthMaterial.userData.bfeProgram,'actors/depth-fabric-cutout');
 assert.equal(mesh.customDepthMaterial.fragmentShader,undefined);
 const input=new Float64Array(20),pass=shadowPass({});
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

test('an additive part uses the program of the same part blended normally', () => {
  const core = { graphics: { settings: { get: () => false }, csm: null }, ownedPrograms: new Map(), rust: { add_owned_program: (key) => key.length } };
  const plain = actorMaterial('unlit', { map: new THREE.Texture(), blending: 'normal' });
  const additive = actorMaterial('unlit', { map: new THREE.Texture(), blending: 'additive' });
  assert.equal(actorProgram(core, additive, 2).id, actorProgram(core, plain, 2).id);
});
