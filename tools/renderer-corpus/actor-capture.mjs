// One role of one actor corpus case: load a seeded battle in the real
// application, capture the renderer's stages at several points, and for the
// reference role record every Rust draw (the replay) so the candidate and the
// repeat reference can redraw the identical inputs. Reference, candidate and
// repeat reference (control) are separate applications; comparison never
// selects an alternate runtime implementation (actor-compare.mjs does that).
import { installActorSkinnedCutout } from './actor-skinned-cutout.mjs';
import { installActorPieces } from './actor-pieces.mjs';
import { installActorFlash } from './actor-flash.mjs';
import { installActorShock } from './actor-shock.mjs';
import { installActorWreck } from './actor-wreck.mjs';
import { installActorDissolve } from './actor-dissolve.mjs';
import { installActorFocus } from './actor-focus.mjs';
import { installViewReplay } from './view-replay.mjs';
import { installInstanceReplay } from './instance-replay.mjs';
import fs from 'node:fs';
import { expand } from './manifest.mjs';
import { installActorReplay } from './actor-replay.mjs';
import { programLayouts } from '../../src/render-programs.js';
import { installReplay } from './replay.mjs';
import { replayBlobStore } from './replay-blobs.mjs';
import { uploadReplayBlobs } from './blob-transfer.mjs';
import { connect, loadBattle, GL_STATE, HELPERS_SRC } from './battle.mjs';
// The case list: the settings, query and special setups (shock, wreck, dissolve...) this run applies.
const manifest = expand(JSON.parse(fs.readFileSync(process.env.BFE_CORPUS_MANIFEST || new URL('./manifest.json', import.meta.url))));
const id = process.env.BFE_CORPUS_CASE, out = process.env.OUT;
const role = process.env.BFE_CORPUS_ROLE;
if (!['reference', 'candidate', 'control'].includes(role) || !out) throw Error('Supply OUT and BFE_CORPUS_ROLE');
const testCase = manifest.cases.find(c => c.id === id);
if (!testCase) throw Error('Unknown corpus case ' + id);
const dir = `${out}/${id}-${role}`;
const rawDir = `${process.env.BFE_CORPUS_RAW || out}/${id}-${role}`;
fs.mkdirSync(dir, { recursive: true });
fs.mkdirSync(rawDir, { recursive: true });
const log = process.env.BFE_GPU_LOG;
// A GPU process death during a run invalidates its pixels, so it is read from the application's log, never ignored.
const gpuDeaths = () => log && fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(l => /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|exit_code=512/i.test(l)) : ['missing GPU log'];
const c = await connect();
const result = { role, id, contract: manifest.version, points: [] };
try {
  result.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query });
  if (result.loaded.error || !result.loaded.spawned) throw Error(result.loaded.error || 'player not spawned');
  await c.ev(HELPERS_SRC);
  // Hook the stages the comparison reads: the far and scene targets and, where
  // the temporal upscaler is on, its velocity, motion and history; then the
  // final HUD-included frame. Reads happen only while T.want is set, in the
  // one frame captured per point.
  result.setup = await c.ev(`(() => {
    const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext();
    const T=window.__corpus={want:false,stages:{}};
    const read=(name,target)=>{
      const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight;
      const half=target?.texture.type===1016,buf=half?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
      if(target){const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,(R.resources?.gpu?R.resources.gpu.framebuffer(R.resources.targetId(target),false,0):R.properties.get(target).__webglFramebuffer));gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);}
      else {R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);}
      const error=gl.getError();if(error)throw Error(name+' read GL '+error);
      T.stages[name]={w,h,bytes:half?2:1,buf};
    };
    const post=g.postSteps;g.postSteps=function(...a){if(T.want){
      read('far',g.farTarget);read('scene',g.sceneTarget);
      if(g.upscaling==='temporal'){read('velocity',g.upscaler.velocity);read('motion',g.upscaler.motion);read('history',g.upscaler.history[g.upscaler.read]);}
      const water=g.scene.userData.water;
      if(water?.reflectionFar){read('reflection',water.target);read('reflectionFar',water.reflectionFar);}
    }return post.apply(this,a);};
    const hud=b.hud.render;b.hud.render=function(...a){if(T.want)read('post',null);const r=hud.apply(this,a);if(T.want)read('presented',null);return r;};
    return {buffer:[gl.drawingBufferWidth,gl.drawingBufferHeight],target:[g.sceneTarget.width,g.sceneTarget.height],samples:g.sceneTarget.samples,supportedSamples:g.supportedSamples,upscaling:g.upscaling};
  })()`);
  // The replay recorder: wraps the renderer's draw entry points to log each Rust
  // draw's inputs in the reference role, and redraws logged inputs elsewhere.
  await c.ev(`(${installReplay.toString()})(${replayBlobStore.toString()})`);
  await c.ev(`window.__corpus.captureReplay=${role === 'reference'}`);
  // The actor family adds its own recorders (bodies, rigs, view parts) and the
  // optional synthetic setups a case asks for, each installed once before the first point.
  if(manifest.family==='actors') {
    await c.ev(`window.__corpus.actorLayouts=${JSON.stringify(Object.fromEntries(Object.entries(programLayouts).filter(([key])=>key.startsWith('actors/') || key.startsWith('effects/'))))};(${installActorReplay.toString()})(${role !== 'candidate'})`);
    await c.ev(`(${installInstanceReplay.toString()})(${role !== 'candidate'})`);
    await c.ev(`(${installViewReplay.toString()})(${role !== 'candidate'})`);
    if(testCase.actorSkinnedCutout)result.actorSkinnedCutout=await c.ev(`(${installActorSkinnedCutout.toString()})()`);
    if(testCase.actorFlash)result.actorFlash=await c.ev(`(${installActorFlash.toString()})()`);
    if(testCase.actorShock)result.actorShock=await c.ev(`(${installActorShock.toString()})()`);
    if(testCase.actorWreck)result.actorWreck=await c.ev(`(${installActorWreck.toString()})(${role !== 'candidate'})`);
    if(testCase.actorDissolve)result.actorDissolve=await c.ev(`(${installActorDissolve.toString()})()`);
    if(testCase.actorPieces)result.actorPieces=await c.ev(`(${installActorPieces.toString()})()`);
    if(testCase.actorFocus)result.actorFocus=await c.ev(`(${installActorFocus.toString()})(${JSON.stringify(testCase.actorFocus)})`);
  }
  // Water cases pin the camera over a reflecting cell so every point sees the planar mirror.
  if (testCase.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__corpus,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
  })()`);
  const captures=Number(process.env.BFE_CORPUS_POINTS || manifest.captures);
  if(!Number.isInteger(captures) || captures<1 || captures>manifest.captures)throw Error('Invalid capture count');
  // Each point: walk the player a fixed number of lockstep frames (same inputs in every role), settle
  // the cascades and history, then capture exactly one frame.
  for (let k=0;k<captures;k++) {
    const point = await c.ev(`(async () => {
      const b=__bf,g=b.graphics,G=window.__gate,T=window.__corpus,gl=g.renderer.getContext();
      G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);T.place?.(${k});
      for(const w of g.cascadeWindows || [])w.invalidate();
      for(let i=0;i<${manifest.settleFrames || 2};i++)await b.lockstep(0,true);
      g.upscaler?.invalidate();if(g.upscaling==='temporal')g.upscaler.frame=${k};
      for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
      G.resetFrame();T.stages={};T.records=[];T.blobs={};T.want=true;const tick=await b.lockstep(0,true);T.want=false;
      const s=T.stages.presented;if(!s)throw Error('HUD did not draw');
      let lit=0;for(let i=0;i<s.buf.length;i+=64)if(s.buf[i]>8 || s.buf[i+1]>8 || s.buf[i+2]>8)lit++;
      return {k:${k},tick,lit:lit/(s.buf.length/64),rustDraws:G.rustDraws(),rustPhases:Object.keys(G.frameStats),errors:[...g.rust.errors],gl:{...${GL_STATE},error:gl.getError()},stages:Object.fromEntries(Object.entries(T.stages).map(([n,s])=>[n,{w:s.w,h:s.h,bytes:s.bytes}]))};
    })()`);
    for (const [name, stage] of Object.entries(point.stages)) {
      const data=await c.ev(`(() => {const s=window.__corpus.stages[${JSON.stringify(name)}],buf=new Uint8Array(s.buf.buffer);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
      stage.file=`${rawDir}/${k}-${name}.bin`;
      fs.writeFileSync(stage.file,Buffer.from(data,'base64'));
    }
    // The reference writes its replay and replays itself at once (self replay
    // proves the recorder is lossless); the others replay the reference's file.
    if (role==='reference') {
      const payload=await c.ev(`({records:window.__corpus.records,blobs:Object.fromEntries(Object.entries(window.__corpus.blobs).map(([key,buf])=>[key,{type:buf.constructor.name}]))})`);
      for (const [key,blob] of Object.entries(payload.blobs)) {
        const data=await c.ev(`(() => {const b=window.__corpus.blobs[${JSON.stringify(key)}],buf=new Uint8Array(b.buffer,b.byteOffset,b.byteLength);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
        blob.file=`${rawDir}/${k}-replay-${key}.bin`;fs.writeFileSync(blob.file,Buffer.from(data,'base64'));
      }
      point.replayFile=`${rawDir}/${k}-replay.json`;fs.writeFileSync(point.replayFile,JSON.stringify(payload));
      point.selfReplay=await c.ev(`window.__corpus.replay(window.__corpus.records,window.__corpus.blobs)`);
    } else if(role==='candidate' || role==='control') {
      const payload=JSON.parse(fs.readFileSync(`${process.env.BFE_CORPUS_RAW || out}/${id}-reference/${k}-replay.json`));
      await c.ev(`window.__corpus.payload={records:${JSON.stringify(payload.records)},blobs:{}};1`);
      await uploadReplayBlobs(c,payload.blobs);
      // BFE_DIAG=1: first say which single draws the owned program draws differently from the recorded one
      // (instance-replay.mjs), before the pair's own replay so a first-use difference shows too.
      if(process.env.BFE_DIAG)point.diagDraws=await c.ev(`window.__corpus.diagDraws(window.__corpus.payload.records,window.__corpus.payload.blobs)`);
      if(point.diagDraws?.some(d=>d.error))console.log(JSON.stringify({diagnosticErrors:point.diagDraws}));
      // BFE_DIAG_INFO=1: what the instance rows of this point hold, before their replay (its first run is the one that has failed).
      if(process.env.BFE_DIAG_GL)await c.ev(`window.__corpus.diagGl=true;1`);
      if(process.env.BFE_DIAG_INFO)console.log(JSON.stringify({info:await c.ev(`window.__corpus.payload.records.filter(r=>r.kind==='instances').map(r=>({key:r.key,lamps:r.lamps.length,records:r.records.length,cascades:r.cascades.length,variants:[...new Set(r.records.map(x=>x.key+(x.lamps?':lamps':'')))],bytes:r.bytes,samples:r.samples}))`)}));
      point.replay=await c.ev(`window.__corpus.replay(window.__corpus.payload.records,window.__corpus.payload.blobs)`);
      await c.ev(`window.__corpus.payload=null;1`);
    }
    if(process.env.BFE_DIAG_GL)point.firstGlError=await c.ev(`window.__corpus.firstGlError ?? null`);
    result.points.push(point);
    console.log(JSON.stringify({role,k,tick:point.tick,stages:Object.keys(point.stages),errors:point.errors,gl:point.gl}));
  }
  // Hashes of every linked program, shader source and texture payload, so a
  // difference in inputs between roles is visible even when pixels agree.
  result.inputHashes = await c.ev(`(async () => {
   const g=__bf.graphics,R=g.renderer,gl=R.getContext();
   const hash=async data=> {const digest=await crypto.subtle.digest('SHA-256',data);return [...new Uint8Array(digest)].map(v=>v.toString(16).padStart(2,'0')).join('');};
   const text=s=>new TextEncoder().encode(s);
   const shaders=[],programs=[];
   for(const p of R.info.programs || []) {
     const attributes=[],uniforms=[];
     for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_ATTRIBUTES);i++){const a=gl.getActiveAttrib(p.program,i);attributes.push({name:a.name,type:a.type,size:a.size,location:gl.getAttribLocation(p.program,a.name)});}
     for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_UNIFORMS);i++){const u=gl.getActiveUniform(p.program,i);uniforms.push({name:u.name,type:u.type,size:u.size});}
     const stages=[];
     for(const shader of gl.getAttachedShaders(p.program) || []){const source=gl.getShaderSource(shader),h=await hash(text(source));shaders.push({stage:gl.getShaderParameter(shader,gl.SHADER_TYPE),hash:h,source});stages.push(h);}
     programs.push({key:p.cacheKey,stages,attributes,uniforms});
   }
   const textures=new Set();
   const material=m=> {if(!m)return;for(const u of Object.values(m.uniforms || {}))if(u.value?.isTexture)textures.add(u.value);if(m.map)textures.add(m.map);};
   for(const scene of [g.scene,g.farScene,g.viewScene,__bf.hud.scene]) scene?.traverse(o=>{for(const m of (Array.isArray(o.material)?o.material:[o.material]))material(m);});
   const payloads=[];
   for(const t of textures){const parts=[];const images=[t.image,...(t.mipmaps || [])];for(const im of images){if(!im)continue;if(im.data && ArrayBuffer.isView(im.data))parts.push(await hash(im.data));else if(im.width && im.height && !t.isRenderTargetTexture){try{const c=document.createElement('canvas');c.width=im.width;c.height=im.height;const x=c.getContext('2d');x.drawImage(im,0,0);parts.push(await hash(x.getImageData(0,0,c.width,c.height).data));}catch{parts.push('unreadable');}}}payloads.push({format:t.format,type:t.type,wrap:[t.wrapS,t.wrapT],filter:[t.minFilter,t.magFilter],flip:t.flipY,mips:parts});}
   return {shaders,programs,textures:payloads,textureHash:await hash(text(JSON.stringify(payloads)))};
  })()`);
  const shaderDir = `${out}/shaders`;
  fs.mkdirSync(shaderDir, { recursive: true });
  for (const shader of result.inputHashes.shaders) { fs.writeFileSync(`${shaderDir}/${shader.hash}.glsl`, shader.source); delete shader.source; }

  result.shaderPrograms=await c.ev(`__bf.graphics.rust.shaderPrograms || []`);
  result.gpuDeaths=gpuDeaths();
  // A case is valid only at the exact size, with no program failure or GPU loss,
  // enough lit pixels and positive Rust draws at every point.
  const size=result.setup.buffer[0]===1600 && result.setup.buffer[1]===900 && result.setup.target[0]===Math.round(1600*testCase.settings.renderScale) && result.setup.target[1]===Math.round(900*testCase.settings.renderScale);
  result.programFailures=[...new Set(result.points.flatMap(p=>p.errors.filter(error=>!/^p\d+ diffuse$/.test(error))))];
  result.valid=!result.programFailures.length && size && !result.gpuDeaths.length && result.points.length===manifest.captures && result.points.every(p=>p.lit>=.2 && p.rustDraws>0 && !p.gl.lostEvents && p.gl.lostEvents!==null && !p.gl.contextLost && p.gl.rustLost===false && !p.gl.error && (testCase.camera!=='water' || (p.rustPhases.includes('mirror') && p.stages.reflection)));
} catch(error) {
  result.error=String(error.stack || error);result.valid=false;
  if(process.env.BFE_DIAG_GL)result.firstGlError=await c.ev(`window.__corpus?.firstGlError ?? null`).catch(e=>String(e));
}
finally {
  result.console=c.consoleErrors.slice(0,30);
  fs.writeFileSync(`${dir}/capture.json`,JSON.stringify(result,null,2));c.close();
}
process.exitCode=result.valid?0:2;
