// Isolated first-frame views from the browser's prepared static resources.
// The native side loads content itself; the only recorded admission facts
// are the view/environment bank and the browser's cold random draw stream.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import crypto from 'node:crypto';
import * as frameLayout from '../src/rust-render/frame-layout.js';
import {FACTS as postFacts,PROGRAM as postPrograms} from '../src/rust-render/post-layout.js';
import {connect,loadBattle} from './renderer-corpus/battle.mjs';
import {readManifest,expand} from './renderer-corpus/manifest.mjs';
const dir=process.argv[2];if(!dir)throw Error('usage: native-static-frame.mjs OUTPUT_DIRECTORY');fs.mkdirSync(dir,{recursive:true});
const map=process.env.MAP||'nab2',preset=expand(readManifest()).cases.find(c=>c.id==='classic-'+map);if(!preset)throw Error('unknown map');
const extraSrc=`globalThis.__staticRandom=[];const staticRandom=Math.random;Math.random=()=>{const n=__gateRandom.valueCalls,v=staticRandom();if(__gateRandom.valueCalls!==n)__staticRandom.push(v);return v;};`;
const c=await connect();
try {
 const loaded=await loadBattle(c,{settings:preset.settings,query:preset.query,warm:1,extraSrc});if(loaded.error)throw Error(loaded.error);
 const random=await c.ev('({draws:__staticRandom,kinds:[...(__bf.graphics.scene.userData.foliage?.kinds.entries()||[])].map(([name,k])=>({name,grass:k.grass,definition:k.def,cards:k.grass?k.cards:[],tints:k.instances.map(i=>i.tint)}))})');
 let draws=null;
 if(random.kinds.length){
  const source=fs.readFileSync(new URL('../src/foliage.js',import.meta.url),'utf8');
  const build=new Function(source.slice(source.indexOf('function buildGrass'),source.indexOf('// A leafpatch'))+';return buildGrass;')();
  const saved=Math.random,chunks=[];let after=0;
  try {for(const first of random.kinds.filter(k=>k.grass&&k.cards.length)){
   let found=false;
   for(let i=after;i<random.draws.length;i++){
    let at=i;Math.random=()=>random.draws[at++]??NaN;
    const candidate=build(first.definition),a=candidate[0],b=first.cards[0];
    if(a&&['x','y','z','height','width','grey','phase','rate','part'].every(k=>Math.abs(a[k]-b[k])<1e-14)){chunks.push(random.draws.slice(i,at));after=at;found=true;break;}
   }if(!found)throw Error('grass random admission start not found');
  }}finally{Math.random=saved;}
  // Texture construction can consume unrelated UUID values between classes.
  // Keep only constructor-local draws, then locate the contiguous tint walk.
  const data=JSON.parse(fs.readFileSync(process.env.BFE_CONTENT_DIR+'/maps/'+map+'/map.json')).foliage;
  const kinds=new Map(random.kinds.map(k=>[k.name,k])),counts=new Map();
  const instances=data.instances.map(i=>{const k=kinds.get(i.class),at=counts.get(i.class)||0;counts.set(i.class,at+1);return {...i,kind:k,tintValue:k?.tints[at]};}).filter(i=>i.kind);
  let tint=null;
  for(let start=after;start<random.draws.length;start++){
   let at=start,valid=true;
   for(const i of instances){const shade=random.draws[at++];if(shade===undefined){valid=false;break;}if(i.tint||i.kind.grass)continue;
    for(let k=0;k<3;k++){const value=.7+.2*shade+.1*random.draws[at++];if(!Number.isFinite(value)||Math.abs(value-i.tintValue[k])>1e-14){valid=false;break;}}if(!valid)break;
   }if(valid){tint=random.draws.slice(start,at);break;}
  }
  if(!tint)throw Error('leaf tint admission stream not found');draws=chunks.flat().concat(tint);
 }
 const results=[];
 for(const phase of ['sky','props','foliage','water']){
  const result=await c.ev(`(()=>{
   const layout=${JSON.stringify(frameLayout)},postLayout=${JSON.stringify(postFacts)},postProgramCount=${JSON.stringify(postPrograms.COUNT)},phase=${JSON.stringify(phase)},b=__bf,g=b.graphics,T=b.THREE,core=g.rust,r=core.rust,renderer=g.renderer,gl=renderer.getContext(),size=[1280,720];
   const foliage=g.scene.userData.foliage,water=g.scene.userData.water;
   if(phase==='foliage'&&!foliage)return {skip:'no authored foliage'};
   if(phase==='water'&&!water?.lowPass)return {skip:'no authored low water'};
   if(phase==='props'&&!b.live?.buildings?.length)return {skip:'no initial placed props'};
   r.frame_cancel_frame();core.frame.active=false;
   const camera=new T.PerspectiveCamera(60,size[0]/size[1],.3,20000),at=new T.Vector3(),eye=new T.Vector3();
   if(phase==='props'){
    const trap=${JSON.stringify(process.env.BFE_STATIC_VIEW==='trap')};const first=trap?b.live.traps?.[0]:b.live.buildings.find(o=>o.looks?.[o.state]?.root?.visible);
    if(!first)return {skip:'no visible initial placed prop'};
    at.setFromMatrixPosition((trap?first.pad:first.looks[first.state]).root.matrixWorld).add(new T.Vector3(0,1,0));eye.copy(at).add(new T.Vector3(4,3,6));
   }else if(phase==='foliage'){
    const instance=[...foliage.kinds.values()].find(k=>k.instances.length)?.instances[0];if(!instance)return {skip:'no foliage placements'};
    at.copy(instance.position).add(new T.Vector3(0,1,0));eye.copy(at).add(new T.Vector3(0,5,12));
   }else if(phase==='water'){
    const cell=[...water.cellBounds][0];if(!cell)return {skip:'no water cells'};
    at.set((cell.min.x+cell.max.x)/2,water.height,(cell.min.z+cell.max.z)/2);eye.copy(at).add(new T.Vector3(10,8,15));
   }else{at.copy(g.camera.position);eye.copy(at).add(new T.Vector3(0,3,0));at.add(new T.Vector3(0,2,-20));camera.near=2;camera.far=g.farCamera.far;camera.updateProjectionMatrix();}
   camera.position.copy(eye);camera.lookAt(at);camera.updateMatrixWorld(true);
   const target=new T.WebGLRenderTarget(...size,{samples:0,depthBuffer:true,stencilBuffer:true});renderer.setRenderTarget(target);renderer.setViewport(0,0,...size);
   const farTarget=new T.WebGLRenderTarget(...size,{samples:0,depthBuffer:true});
   const originalFar=g.farTarget,originalMap=g.background.material.uniforms.map.value;
   g.farTarget=farTarget;g.background.material.uniforms.map.value=farTarget.texture;
   renderer.setClearColor(new T.Color(.04,.06,.09),1);
   core.frame.begin(true);core.views();core.postFacts.fill(0);core.postFacts.fill(0xffffffff,postLayout.PROGRAMS,postLayout.PROGRAMS+postProgramCount);
   for(const e of core.byName.world.meshes)r.world_shown(e.id,false);for(const e of core.byName.terrain.grounds)r.world_shown(e.id,false);
   core.views();for(const name of ['canopiesInput','fxInput','waterInput','transparentObjects','transparentKeys'])core[name]?.fill(0);
   for(let i=0;i<3;i++)r.frame_lines_publish_frame(i,0);
   if(core.farInput)core.farInput[7]=0;
   if(foliage){
    if(phase==='foliage'){foliage.time=0;foliage.nearEnd=()=>20000;core.byName.foliage.selectClasses(foliage,camera);}
    else r.foliage_clear_frame();
   }
   core.writeFrame(phase==='sky'?g.farScene:g.scene,camera,phase==='sky'?1:0);core.views();core.input[35]=0;core.input[64]=1e8;core.input[65]=2e8;
   // No live point-light facts enter an isolated cold admission view.
   for(let i=70;i<core.recordsAt;i++)core.input[i]=0;
   const values=Array.from(core.input.subarray(0,core.recordsAt));let records=0,bones=0;const rigs=[];
   const keep=new Set(phase==='sky'?g.skyLayers.map(s=>s.mesh):[]);for(const child of g.farScene.children)child.visible=keep.has(child);
   for(const {mesh,follow} of g.skyLayers){mesh.position.copy(eye);if(follow)mesh.position.y=follow[0]*eye.y+(1-follow[0])*follow[1];mesh.updateMatrixWorld(true);}
   renderer.setRenderTarget(farTarget);core.byName.far.frame({renderer,scene:g.farScene,camera});
   core.frame.seal(layout.ROLE.FAR,layout.KIND.FAR,null);
   renderer.setRenderTarget(target);core.views();core.input.set(values);
    if(phase==='props'){
     const p=core.byName.instances,ctx={records:0,bones:0,skeletons:new Map()};p.listed=Infinity;p.projScreen.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);p.frustum.setFromProjectionMatrix(p.projScreen);
     for(const object of b.live.buildings){
      const models=[object.looks?.[object.looks.unbuilt?'unbuilt':'built'],object.state==='unbuilt'?object.pad:null].filter(Boolean);
      for(const rig of models){for(const [bone,bind] of rig.bind){bone.quaternion.copy(bind.q);bone.position.copy(bind.p);}
      rig.root.updateMatrixWorld(true);
      for(const mesh of rig.meshes){let record=p.bodies.get(mesh);if(!record)record=p.registerRig(mesh);if(record&&p.inView(mesh)){p.writeBody(record,ctx);rigs.push({world:mesh.matrixWorld.elements,bind:Array.from(Float32Array.from(mesh.bindMatrixInverse.elements)),bones:Array.from(mesh.skeleton.boneMatrices)});}}}
     }records=ctx.records;bones=ctx.bones;
    }

   if(phase==='water'&&water.ocean){water.ocean.update(0,camera);water.ocean.material.uniforms.time.value=0;water.ocean.near.updateMatrixWorld(true);core.byName.sea.draw({camera,scene:g.scene,renderer});}
    if(phase==='water'&&!water.ocean){
     water.group.visible=true;water.group.position.y=0;water.group.updateMatrixWorld(true);water.lowPass.visible=true;water.tintPass.visible=false;water.mirrorPass.visible=false;
     for(const mesh of water.shore?.meshes||[])mesh.visible=false;if(water.ocean?.waves?.spray)water.ocean.waves.spray.visible=false;
     if(water.rings)water.rings.geometry.setDrawRange(0,0);
     const u=water.lowPass.material.uniforms;u.time.value=0;u.maskMix.value=0;u.maskA.value=water.masks?.textures[0]||u.maskA.value;u.maskB.value=u.maskA.value;u.cameraRange.value.set(camera.near,camera.far);
     core.byName.water.draw({camera,scene:g.scene,renderer});
    }
   core.frame.seal(layout.ROLE.NEAR,layout.KIND.SCENE,{records,bones});
   core.byName.near.writeImage(camera);core.frame.seal(layout.ROLE.IMAGE,layout.KIND.SCREEN,{steps:1});
   core.frame.seal(layout.ROLE.TRANSPARENT,layout.KIND.TRANSPARENT,{records,bones});
   core.frame.execute();core.views();
   const schedule=Array.from(new Float64Array(core.buffer,r.frame_trace_ptr(),r.frame_trace_len()).subarray(0,core.frameState[layout.STATE.OPERATION_COUNT]*layout.COMMAND.LEN));
   g.farTarget=originalFar;g.background.material.uniforms.map.value=originalMap;
   const uniforms={};if(phase==='water'){const program=gl.getParameter(gl.CURRENT_PROGRAM);if(program)for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_UNIFORMS);i++){const u=gl.getActiveUniform(program,i);if(u.type===gl.SAMPLER_2D)continue;const value=gl.getUniform(program,gl.getUniformLocation(program,u.name));uniforms[u.name]=ArrayBuffer.isView(value)?Array.from(value):[value];}}
   core.stale=true;const pixels=new Uint8Array(size[0]*size[1]*4);renderer.readRenderTargetPixels(target,0,0,...size,pixels);const error=gl.getError();
   let encoded='';for(let k=0;k<pixels.length;k+=32768)encoded+=String.fromCharCode.apply(null,pixels.subarray(k,k+32768));target.dispose();farTarget.dispose();renderer.setRenderTarget(null);
   return {size,values,pixels:btoa(encoded),records,error,uniforms,rigs,schedule};
  })()`);
  if(result.skip){results.push({phase,skip:result.skip});continue;}
  if(result.error)throw Error('browser GL error '+result.error);
  const frame={size:result.size,values:result.values,foliageRandom:draws};
  const input=dir+'/'+phase+'.json',output=dir+'/'+phase+'.rgba';fs.writeFileSync(input,JSON.stringify(frame));
  const receipt=JSON.parse(execFileSync(process.env.BFE_STATIC_FRAME||'rust/target/debug/static-frame',[process.env.BFE_CONTENT_DIR,map,input,output,phase],{encoding:'utf8',maxBuffer:16<<20}));
  if(JSON.stringify(receipt.schedule)!==JSON.stringify(result.schedule))throw Error('battle_frame schedule differs for '+phase);
  if(!receipt.schedule.length)throw Error('component bypassed battle_frame');
  const bindingDifferences=[];
  if(phase==='water')for(const [name,native] of Object.entries(receipt.bindings.uniforms)){const browser=result.uniforms[name];if(browser&&native.some((v,i)=>v!==browser[i]))bindingDifferences.push({name,native,browser});}
  if(phase==='props')for(const native of receipt.bindings.rigs){const browser=result.rigs.find(r=>JSON.stringify(r.world)===JSON.stringify(native.world));if(!browser)continue;for(const name of ['bones','bind']){const at=native[name].findIndex((v,i)=>v!==browser[name][i]);if(at>=0){bindingDifferences.push({name,index:at,native:native[name][at],browser:browser[name][at]});break;}}if(bindingDifferences.length)break;}
  delete receipt.bindings;
  const actual=fs.readFileSync(output),expected=Buffer.from(result.pixels,'base64');let changed=0,channels=0,max=0,sum=0,first=null;
  for(let i=0;i<expected.length;i+=4){let differs=false;for(let k=0;k<4;k++){const d=Math.abs(actual[i+k]-expected[i+k]);if(d){channels++;differs=true;sum+=d*d;max=Math.max(max,d);if(!first)first={x:(i/4)%result.size[0],y:Math.floor(i/4/result.size[0]),native:[...actual.subarray(i,i+4)],browser:[...expected.subarray(i,i+4)]};}}if(differs)changed++;}
  const browser=crypto.createHash('sha256').update(expected).digest('hex');results.push({phase,...receipt,browser,changed,channels,max,bindingDifferences,rms:Math.sqrt(sum/expected.length),first});
  fs.unlinkSync(output);fs.unlinkSync(input);
 }
 console.log(JSON.stringify({map,results}));
}finally{c.close();}
