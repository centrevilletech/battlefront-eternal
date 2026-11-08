// One default-path run. Reference, candidate and repeat reference are separate
// applications; comparison never selects an alternate runtime implementation.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { expand, readManifest } from './manifest.mjs';
import { installReplay } from './replay.mjs';
import { replayBlobStore } from './replay-blobs.mjs';
import { uploadReplayBlobs } from './blob-transfer.mjs';
import { installPostPolicy } from './post-policy.mjs';
import * as postLayout from '../../src/rust-render/post-layout.js';
import { STATE } from '../../src/rust-render/frame-layout.js';
import { installScenario } from './scenarios.mjs';
import { installGLTracking, installGLReplay } from './gl-replay.mjs';
import { connect, loadBattle, GL_STATE, HELPERS_SRC } from './battle.mjs';
const manifest = expand(readManifest());
const id = process.env.BFE_CORPUS_CASE, out = process.env.OUT;
const role = process.env.BFE_CORPUS_ROLE;
// With a shader index the capture also records every GPU draw for native stage replay (gl-replay.mjs); without one it records frames only.
const shaderIndex=process.env.BFE_SHADER_INDEX?JSON.parse(fs.readFileSync(process.env.BFE_SHADER_INDEX)):null;
// Whole-frame runs preserve the game's context without inline fixture draws.
// Stage runs independently serialize and replay every sampled shader input.
const drawReplay=!!shaderIndex&&!process.env.BFE_CORPUS_FRAME_ONLY;
if (!['reference', 'candidate', 'control'].includes(role) || !out) throw Error('Supply OUT and BFE_CORPUS_ROLE');
const testCase = manifest.cases.find(c => c.id === id);
if (!testCase) throw Error('Unknown corpus case ' + id);
const dir = `${out}/${id}-${role}`;
const rawDir = `${process.env.BFE_CORPUS_RAW || out}/${id}-${role}`;
fs.mkdirSync(dir, { recursive: true });
fs.mkdirSync(rawDir, { recursive: true });
const log = process.env.BFE_GPU_LOG;
// GPU process exits and context losses logged by the application: any of them invalidates the session.
const gpuDeaths = () => log && fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(l => /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|exit_code=512/i.test(l)) : ['missing GPU log'];
// One capture is one application session in one role (reference, control or candidate); compare.mjs reads the three directories together.
const c = await connect();
const result = { role, id, build:process.env.BFE_CORPUS_BUILD || null, contract: manifest.version, gpuInputVersion: drawReplay ? 3 : null, drawReplay, points: [] };
try {
  // Load the immutable case: same query, settings and seeded world in every role, advanced to a fixed authority tick before sampling.
  result.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query, extraSrc: shaderIndex?`(${installGLTracking.toString()})(${JSON.stringify(shaderIndex)})`:"" });
  if (result.loaded.error || !result.loaded.spawned) throw Error(result.loaded.error || 'player not spawned');
  await c.ev(HELPERS_SRC);
  // The renderer's configuration for the session, recorded so a role that drew with another setting is rejected, not compared.
  result.setup = await c.ev(`(() => {
    const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext();
    const T=window.__corpus={want:false,stages:{},captureFamilies:["post-hud/"]};
    const read=(name,target)=>{
      const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight;
      const half=target?.texture.type===1016,buf=half?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
      if(target){const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,(R.resources?.gpu?R.resources.gpu.framebuffer(R.resources.targetId(target),false,0):R.properties.get(target).__webglFramebuffer));gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);}
      else {R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);}
      const error=gl.getError();if(error)throw Error(name+' read GL '+error);
      T.stages[name]={w,h,bytes:half?2:1,buf};
    };
    if(g.rust.frame?.execute){
      const frame=g.rust.frame,execute=frame.execute;T.captureFamilies=['post-hud/'];
      const cached=[];T.nativeCallbacks=0;
      const nativeFrame=g.rust.rust.battle_frame;
      g.rust.rust.battle_frame=function(...args){T.executing=true;try{return nativeFrame.apply(this,args);}finally{T.executing=false;}};
      const guard=(object,name)=>{const original=object[name];if(!original)return;object[name]=function(...args){if(T.executing){T.nativeCallbacks++;throw Error('Browser callback during native frame: '+name);}return original.apply(this,args);};};
      guard(g.rust,'publishView');guard(R,'setRenderTarget');guard(R.resources,'copy');guard(R.resources,'resolve');
      for(const pass of g.rust.passes)guard(pass,'draw');
      const raw=(name,target,handle)=>{
        const w=target?.width||gl.drawingBufferWidth,h=target?.height||gl.drawingBufferHeight,half=target?.texture.type===1016,buf=half?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
        const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,handle);
        gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
        const error=gl.getError();if(error)throw Error(name+' raw GL '+error);T.stages[name]={w,h,bytes:half?2:1,buf};
      };
      frame.execute=function(...args){
        if(T.want){
          cached.length=0;const add=(name,target)=>cached.push([name,target,R.resources.gpu.framebuffer(R.resources.targetId(target),false,0)]);
          add('far',g.farTarget);add('scene',g.sceneTarget);
          if(g.upscaling==='temporal'){add('velocity',g.upscaler.velocity);add('motion',g.upscaler.motion);add('history',g.upscaler.history[g.upscaler.read]);}
          const water=g.scene.userData.water;if(water?.reflectionFar){add('reflection',water.target);add('reflectionFar',water.reflectionFar);}
        }
        const result=execute.apply(this,args);if(T.want)raw('presented',null,null);return result;
      };
      T.beforeNativeDraw=source=>{
        if(source.key?.startsWith('post-hud/') && source.key!=='post-hud/hud' && !T.stages.far)for(const [name,target,handle] of cached)raw(name,target,handle);
      };
      T.afterNativeDraw=source=>{if((source.key?.startsWith('post-hud/') && source.key!=='post-hud/hud' || source.key?.startsWith('effects/water-underwater-tint-')) && gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING)===null)raw('post',null,null);};
    } else {
    const postName=typeof g.postSteps === "function" ? "postSteps" : "finishPost";const post=g[postName];g[postName]=function(...a){if(T.want){
      read('far',g.farTarget);read('scene',g.sceneTarget);
      if(g.upscaling==='temporal'){read('velocity',g.upscaler.velocity);read('motion',g.upscaler.motion);read('history',g.upscaler.history[g.upscaler.read]);}
      const water=g.scene.userData.water;
      if(water?.reflectionFar){read('reflection',water.target);read('reflectionFar',water.reflectionFar);}
    }return post.apply(this,a);};
    const hud=b.hud.render;b.hud.render=function(...a){if(T.want)read('post',null);const r=hud.apply(this,a);if(T.want)read('presented',null);return r;};
    }
    return {buffer:[gl.drawingBufferWidth,gl.drawingBufferHeight],target:[g.sceneTarget.width,g.sceneTarget.height],samples:g.sceneTarget.samples,supportedSamples:g.supportedSamples,upscaling:g.upscaling};
  })()`);
  result.setup.settings=await c.ev(`Object.fromEntries(${JSON.stringify(Object.keys(testCase.settings))}.map(key=>[key,__bf.graphics.settings.get(key)]))`);
  const wrongSettings=Object.keys(testCase.settings).filter(key=>JSON.stringify(result.setup.settings[key])!==JSON.stringify(testCase.settings[key]));
  if(wrongSettings.length)throw Error('Fixture settings changed: '+wrongSettings.join(', '));
  if(drawReplay)await c.ev(`(${installReplay.toString()})(${replayBlobStore.toString()})`);
  await c.ev(`(${installPostPolicy.toString()})(${JSON.stringify(postLayout)},${JSON.stringify(testCase.post) || "null"})`);
  if(drawReplay)await c.ev(`(${installGLReplay.toString()})()`);
  await c.ev(`window.__corpus.captureReplay=${role === 'reference'&&drawReplay};window.__corpus.referenceBuild=${role !== 'candidate'}`);
  if (testCase.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__corpus,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const name=g.prepareFrame?"prepareFrame":"render",render=g[name];g[name]=function(...a){aim(g.camera);return render.apply(this,a);};
    if(g.rust.frame){const mirror=g.rust.frame.mirror;g.rust.frame.mirror=function(w,c){aim(c);g.rust.frameState[${STATE.MIRROR_FRAMES}]=0;return mirror.call(this,w,c);};}return 1;
  })()`);
  await c.ev(`(${installScenario.toString()})(${JSON.stringify(testCase.scenario)})`);
  const captures=Number(process.env.BFE_CORPUS_POINTS || manifest.captures);
  if(!Number.isInteger(captures) || captures<1 || captures>manifest.captures)throw Error('Invalid capture count');
  // Each point advances to a fixed tick, draws, and saves the stages (far, scene, velocity, motion, history, post, presented) and every recorded GPU draw.
  for (let k=0;k<captures;k++) {
    // Replay uploads can leave large dead typed arrays and deleted GL wrappers
    // until a later collection. Bound fixture memory independently of the game.
    await c.psend('HeapProfiler.collectGarbage');
    const point = await c.ev(`(async () => {
      const b=__bf,g=b.graphics,G=window.__gate,T=window.__corpus,gl=g.renderer.getContext();
      T.scenarioInput?.();G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);T.place?.(${k});
      for(const w of g.cascadeWindows || [])w.invalidate();
      for(let i=0;i<${manifest.settleFrames || 2};i++)await b.lockstep(0,true);
      if(!${Boolean(testCase.retainHistory)}){if(g.upscaler?.reset)g.upscaler.reset(${k},-1);else{g.upscaler?.invalidate();if(g.upscaling==='temporal')g.upscaler.frame=${k};}}
      for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
      await T.scenarioCapture?.();G.resetFrame();T.stages={};T.records=[];T.blobs={};T.resetGLStages?.();T.want=true;const tick=await b.lockstep(0,true);T.want=false;T.completePostPolicy?.();
      const s=T.stages.presented;if(!s)throw Error('HUD did not draw');
      let lit=0;for(let i=0;i<s.buf.length;i+=64)if(s.buf[i]>8 || s.buf[i+1]>8 || s.buf[i+2]>8)lit++;
      return {k:${k},tick,lit:lit/(s.buf.length/64),rustDraws:G.rustDraws(),rustPhases:Object.keys(G.frameStats),programGaps:T.programGaps || [],nativeCallbacks:T.nativeCallbacks||0,errors:[...g.rust.errors],gl:{...${GL_STATE},error:gl.getError()},stages:Object.fromEntries(Object.entries(T.stages).map(([n,s])=>[n,{w:s.w,h:s.h,bytes:s.bytes}]))};
    })()`);
    for (const [name, stage] of Object.entries(point.stages)) {
      const data=await c.ev(`(() => {const s=window.__corpus.stages[${JSON.stringify(name)}],buf=new Uint8Array(s.buf.buffer);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
      stage.file=`${rawDir}/${k}-${name}.bin.gz`;stage.compression="gzip";
      fs.writeFileSync(stage.file,gzipSync(Buffer.from(data,'base64'),{level:1}));
    }
    if (role==='reference'&&drawReplay) {
      const payload=await c.ev(`({records:window.__corpus.records,blobs:Object.fromEntries(Object.entries(window.__corpus.blobs).map(([key,buf])=>[key,{type:buf.constructor.name}]))})`);
      for(const row of payload.records.filter(row=>row.kind==='gl-effect')){
        const expected=shaderIndex.find(p=>p.key===row.key&&(row.source.transport&&row.source.transport!=='Attributes'?p.kind==='Plain':p.declared));
        if(!expected || createHash('sha256').update(row.source.vertex).digest('hex')!==expected.vertex || createHash('sha256').update(row.source.fragment).digest('hex')!==expected.fragment)throw Error('Effects source hash mismatch: '+row.key);
      }
      for (const [key,blob] of Object.entries(payload.blobs)) {
        const data=await c.ev(`(() => {const b=window.__corpus.blobs[${JSON.stringify(key)}],buf=new Uint8Array(b.buffer,b.byteOffset,b.byteLength);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
        blob.file=`${rawDir}/${k}-replay-${key}.bin`;fs.writeFileSync(blob.file,Buffer.from(data,'base64'));
      }
      point.replayFile=`${rawDir}/${k}-replay.json`;fs.writeFileSync(point.replayFile,JSON.stringify(payload));
      point.selfReplay=await c.ev(`window.__corpus.replay(window.__corpus.records,window.__corpus.blobs)`);
    } else if(drawReplay&&(role==='candidate' || role==='control')) {
      const payload=JSON.parse(fs.readFileSync(`${process.env.BFE_CORPUS_RAW || out}/${id}-reference/${k}-replay.json`));
      await c.ev(`window.__corpus.payload={records:${JSON.stringify(payload.records)},blobs:{}};1`);
      await uploadReplayBlobs(c,payload.blobs);
      point.replay=await c.ev(`window.__corpus.replay(window.__corpus.payload.records,window.__corpus.payload.blobs)`);
      await c.ev(`window.__corpus.payload=null;1`);
    }
    await c.ev('window.__corpus.payload=null;window.__corpus.records=[];window.__corpus.blobs={};window.__corpus.stages={};1');
    await c.psend('HeapProfiler.collectGarbage');
    result.points.push(point);
    console.log(JSON.stringify({role,k,tick:point.tick,stages:Object.keys(point.stages),errors:point.errors,gl:point.gl}));
  }
  // Hash every shader source the session linked; sources go to files by hash so identical stages across cases are stored once.
  result.inputHashes = await c.ev(`(async () => {
   const g=__bf.graphics,R=g.renderer,gl=R.getContext();
   const hash=async data=> {const digest=await crypto.subtle.digest('SHA-256',data);return [...new Uint8Array(digest)].map(v=>v.toString(16).padStart(2,'0')).join('');};
   const text=s=>new TextEncoder().encode(s);
   const shaders=[],programs=[];
   // Keep disposed warm programs in the inventory as well as live materials.
   // The tracker starts before application loading, so warm-up cannot hide a variant.
   const linkedPrograms=[];
   for(const p of window.__glTracking?.state(gl).seenPrograms.values() || []) {
     if(/uniform BfeFrame\\b|uniform BfeShadow\\b|bfeLoad\\(/.test(p.vertex+p.fragment))continue;
     const stages=[];
     for(const [stage,source] of [[35633,p.vertex],[35632,p.fragment]]) {
       const h=await hash(text(source));shaders.push({stage,hash:h,source});stages.push({stage,hash:h});
     }
     linkedPrograms.push({stages,attributes:p.layout,uniforms:p.uniforms,materials:[]});
   }
   const textures=new Set();
   const material=m=> {if(!m)return;for(const u of Object.values(m.uniforms || {}))if(u.value?.isTexture)textures.add(u.value);if(m.map)textures.add(m.map);};
   for(const scene of [g.scene,g.farScene,g.viewScene,__bf.hud.scene]) scene?.traverse(o=>{for(const m of (Array.isArray(o.material)?o.material:[o.material]))material(m);});
   const payloads=[];
   for(const t of textures){const parts=[];const images=[t.image,...(t.mipmaps || [])];for(const im of images){if(!im)continue;if(im.data && ArrayBuffer.isView(im.data))parts.push(await hash(im.data));else if(im.width && im.height && !t.isRenderTargetTexture){try{const c=document.createElement('canvas');c.width=im.width;c.height=im.height;const x=c.getContext('2d');x.drawImage(im,0,0);parts.push(await hash(x.getImageData(0,0,c.width,c.height).data));}catch{parts.push('unreadable');}}}payloads.push({format:t.format,type:t.type,wrap:[t.wrapS,t.wrapT],filter:[t.minFilter,t.magFilter],flip:t.flipY,mips:parts});}
   return {shaders,programs,linkedPrograms,textures:payloads,textureHash:await hash(text(JSON.stringify(payloads)))};
  })()`);
  const shaderDir = `${out}/shaders`;
  fs.mkdirSync(shaderDir, { recursive: true });
  for (const shader of result.inputHashes.shaders) { fs.writeFileSync(`${shaderDir}/${shader.hash}.glsl`, shader.source); delete shader.source; }

  // What the candidate registered from the catalog and how long each link took (the loading-time measurement reads this).
  result.shaderPrograms=await c.ev(`__bf.graphics.rust.shaderPrograms || []`);
  result.gpuDeaths=gpuDeaths();
  const canvasScale=result.setup.upscaling==='off'?testCase.settings.renderScale:1;
  const size=result.setup.buffer[0]===Math.floor(1600*canvasScale) && result.setup.buffer[1]===Math.floor(900*canvasScale) && result.setup.target[0]===Math.round(1600*testCase.settings.renderScale) && result.setup.target[1]===Math.round(900*testCase.settings.renderScale);
  // The candidate must have no error naming an owned family: a refused variant is a failure, not a skipped draw.
  result.familyErrors=role==='candidate'?result.points.flatMap(p=>p.errors).filter(e=>/^effects |^water |^motion |effects\/|Owned (?:water|underwater|transparent)|First-person arc/.test(e)):[];
  // A session is valid only with a full set of points, retained draws, no GPU death or context loss, no program gaps and no GL error.
  result.valid=size && !result.familyErrors.length && !result.gpuDeaths.length && result.points.length===manifest.captures && result.points.every(p=>p.lit>=.2 && p.rustDraws>0 && (!testCase.post?.restore || (/NVIDIA/.test(p.gl.renderer) && !/SwiftShader|llvmpipe|Vulkan/i.test(p.gl.renderer))) && p.gl.lostEvents===(testCase.post?.restore&&p.k>0?1:0) && p.gl.restoredEvents===(testCase.post?.restore&&p.k>0?1:0) && !p.gl.contextLost && !p.gl.rustLost && !(p.programGaps?.length) && p.gl.rustEnabled && !p.gl.error && p.nativeCallbacks===0 && (testCase.camera!=='water' || (p.rustPhases.includes('mirror') && p.stages.reflection)));
} catch(error) { result.error=String(error.stack || error);result.valid=false; }
finally {
  result.console=c.consoleErrors.slice(0,30);
  fs.writeFileSync(`${dir}/capture.json`,JSON.stringify(result,null,2));c.close();
}
process.exitCode=result.valid?0:2;
