// One default-path run. Reference, candidate and repeat reference are separate
// applications; comparison never selects an alternate runtime implementation.
import fs from 'node:fs';
import { expand } from './manifest.mjs';
import { installReplay } from './replay.mjs';
import { installWorldTrace, installWorldReplay } from './world-replay.mjs';
import { programLayouts } from '../../src/render-programs.js';
import { connect, loadBattle, GL_STATE, HELPERS_SRC } from './battle.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./world-manifest.json', import.meta.url))));
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
const gpuDeaths = () => log && fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(l => /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|exit_code=512/i.test(l)) : ['missing GPU log'];
const c = await connect();
const result = { role, id, contract: manifest.version, points: [] };
try {
  result.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query, extraSrc: `(${installWorldTrace.toString()})()` });
  if (result.loaded.error || !result.loaded.spawned) throw Error(result.loaded.error || 'player not spawned');
  await c.ev(HELPERS_SRC);
  await c.ev(`window.__worldTrace.ready=true;1`);
  result.setup = await c.ev(`(() => {
    const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext();
    const T=window.__corpus={want:false,stages:{}};
    const read=(name,target)=>{
      const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight;
      const half=target?.texture.type===1016,buf=half?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
      if(target){const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,R.properties.get(target).__webglFramebuffer);gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);}
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
  await c.ev(`(${installReplay.toString()})()`);
  await c.ev(`(${installWorldReplay.toString()})(${JSON.stringify(programLayouts)})`);
  await c.ev(`window.__corpus.captureReplay=${role === 'reference'};window.__corpus.role=${JSON.stringify(role)}`);
  if (testCase.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__corpus,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
  })()`);
  if (testCase.camera === 'canopy') await c.ev(`(() => {
    const b=__bf,g=b.graphics,T=window.__corpus,V=b.THREE.Vector3,trees=g.treeCanopies;
    if(!trees?.groups.length)throw Error('No tree canopy groups');
    const group=trees.groups[0],at=group.sphere.center.clone(),eye=at.clone().add(new V(0,1,Math.min(35,group.sphere.radius+12)));
    const render=g.render;g.render=function(...args){g.camera.position.copy(eye);g.camera.lookAt(at);g.camera.updateMatrixWorld(true);return render.apply(this,args);};
    return 1;
  })()`);
  if(testCase.camera==='world-transparent')await c.ev(`(() => {
    const b=__bf,g=b.graphics,T=window.__corpus,V=b.THREE.Vector3,groups=new Map();
    T.transparentOnly=true;
    g.scene.traverse(o=>{const m=o.material;if(!m?.transparent||!m.defines||!('WORLD_SPACE' in m.defines)||!o.geometry?.index)return;
      const key=JSON.stringify([Object.entries(m.defines).sort(([a],[z])=>a<z?-1:a>z?1:0),m.side,Boolean(o.geometry.attributes.fabricAnchor),Boolean(m.uniforms?.treeStates)]);
      if(!groups.has(key))groups.set(key,o);
    });
    const targets=[...groups].sort(([a],[z])=>a<z?-1:a>z?1:0).map(([,o])=>o).slice(${testCase.targetOffset||0},${(testCase.targetOffset||0)+4});
    if(!targets.length)throw Error('No world transparent targets');
    T.place=k=>{const o=targets[k%targets.length],p=o.geometry.attributes.position,index=o.geometry.index.array,at=new V(),normal=new V();
      for(let j=0;j<3;j++)at.add(new V().fromBufferAttribute(p,index[j]));at.multiplyScalar(1/3);
      const a=new V().fromBufferAttribute(p,index[0]),c=new V().fromBufferAttribute(p,index[1]),d=new V().fromBufferAttribute(p,index[2]);normal.crossVectors(c.sub(a),d.sub(a)).normalize();if(!normal.lengthSq())normal.set(0,0,1);
      T.pose={at,eye:at.clone().addScaledVector(normal,6)};
    };
    const render=g.render;g.render=function(...args){if(T.pose){g.camera.position.copy(T.pose.eye);g.camera.lookAt(T.pose.at);g.camera.updateMatrixWorld(true);}return render.apply(this,args);};return targets.length;
  })()`);
  const captures=Number(process.env.BFE_CORPUS_POINTS || manifest.captures);
  if(!Number.isInteger(captures) || captures<1 || captures>manifest.captures)throw Error('Invalid capture count');
  for (let k=0;k<captures;k++) {
    const point = await c.ev(`(async () => {
      const b=__bf,g=b.graphics,G=window.__gate,T=window.__corpus,gl=g.renderer.getContext();
      G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,${testCase.advanceDraws!==false});}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);T.place?.(${k});
      for(const w of g.cascadeWindows || [])w.invalidate();
      for(let i=0;i<${manifest.settleFrames || 2};i++)await b.lockstep(0,true);
      g.upscaler?.invalidate();if(g.upscaling==='temporal')g.upscaler.frame=${k};
      for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
      G.resetFrame();T.stages={};T.records=[];T.blobs={};T.want=true;const tick=await b.lockstep(0,true);T.want=false;
      const s=T.stages.presented;if(!s)throw Error('HUD did not draw');
      let lit=0;for(let i=0;i<s.buf.length;i+=64)if(s.buf[i]>8 || s.buf[i+1]>8 || s.buf[i+2]>8)lit++;
      // Old references can reject an unported program before another pass
      // takes it. Preserve those diagnostics with the actual linked source;
      // a world diagnostic or an unidentified program still invalidates replay.
      const externalDiagnostics=[],worldErrors=[];
      for(const message of g.rust.errors){
        const match=/^p(\\d+) [A-Za-z_][A-Za-z_0-9]*(?: \([^)]*\))?$/.exec(message),program=match&&(g.renderer.info.programs||[]).find(p=>p.id===Number(match[1]));
        const source=program&&(gl.getAttachedShaders(program.program)||[]).map(s=>gl.getShaderSource(s)).join('\\n');
        if(source&&!/^#define (WORLD_SPACE|TERRAIN)[ \t]*$/m.test(source)&&!source.includes('foliageCorner')&&!source.includes('leafCenter'))externalDiagnostics.push({message,program:program.id,key:program.cacheKey});
        else worldErrors.push(message);
      }
      return {k:${k},tick,externalDiagnostics,worldErrors,lit:lit/(s.buf.length/64),rustDraws:G.rustDraws(),rustPhases:Object.keys(G.frameStats),errors:[...g.rust.errors],gl:{...${GL_STATE},rustEnabled:Boolean(g.rust && g.rust.enabled!==false && !g.rust.failure),error:gl.getError()},stages:Object.fromEntries(Object.entries(T.stages).map(([n,s])=>[n,{w:s.w,h:s.h,bytes:s.bytes}]))};
    })()`);
    for (const [name, stage] of Object.entries(point.stages)) {
      const data=await c.ev(`(() => {const s=window.__corpus.stages[${JSON.stringify(name)}],buf=new Uint8Array(s.buf.buffer);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
      stage.file=`${rawDir}/${k}-${name}.bin`;
      fs.writeFileSync(stage.file,Buffer.from(data,'base64'));
    }
    if (role==='reference') {
      const payload=await c.ev(`({records:window.__corpus.records,blobs:Object.fromEntries(Object.entries(window.__corpus.blobs).map(([key,buf])=>[key,{type:buf.constructor.name,bytes:buf.byteLength}]))})`);
      for (const [key,blob] of Object.entries(payload.blobs)) {
        blob.file=`${rawDir}/${k}-replay-${key}.bin`;
        const fd=fs.openSync(blob.file,'w');
        try { for(let offset=0;offset<blob.bytes;offset+=1048576) {
          const data=await c.ev(`(() => {const b=window.__corpus.blobs[${JSON.stringify(key)}],buf=new Uint8Array(b.buffer,b.byteOffset+${offset},Math.min(1048576,b.byteLength-${offset}));let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
          fs.writeSync(fd,Buffer.from(data,'base64'));
        }} finally {fs.closeSync(fd);}
      }
      point.replayFile=`${rawDir}/${k}-replay.json`;fs.writeFileSync(point.replayFile,JSON.stringify(payload));
      point.selfReplay=await c.ev(`window.__corpus.replay(window.__corpus.records,window.__corpus.blobs)`);
    } else if(role==='candidate'||role==='control') {
      const payload=JSON.parse(fs.readFileSync(`${process.env.BFE_CORPUS_RAW || out}/${id}-reference/${k}-replay.json`));
      await c.ev(`window.__corpus.payload={records:${JSON.stringify(payload.records)},blobs:{}};1`);
      for(const [key,blob] of Object.entries(payload.blobs)) {
        const data=fs.readFileSync(blob.file);
        await c.ev(`window.__corpus.payload.blobs[${JSON.stringify(key)}]=new window[${JSON.stringify(blob.type)}](new ArrayBuffer(${data.length}));1`);
        for(let offset=0;offset<data.length;offset+=1048576) {
          const chunk=data.subarray(offset,offset+1048576).toString('base64');
          await c.ev(`(() => {const raw=atob(${JSON.stringify(chunk)}),b=window.__corpus.payload.blobs[${JSON.stringify(key)}],bytes=new Uint8Array(b.buffer);for(let i=0;i<raw.length;i++)bytes[${offset}+i]=raw.charCodeAt(i);return 1;})()`);
        }
      }
      point.replay=await c.ev(`window.__corpus.replay(window.__corpus.payload.records,window.__corpus.payload.blobs)`);
      await c.ev(`window.__corpus.payload=null;1`);
    }
    result.points.push(point);
    console.log(JSON.stringify({role,k,tick:point.tick,stages:Object.keys(point.stages),errors:point.errors,gl:point.gl}));
  }
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
     programs.push({id:p.id,key:p.cacheKey,stages,attributes,uniforms});
   }
   for(const [key,source] of window.__corpus.worldProgramSources)for(const [stage,body] of Object.entries(source)){
     const h=await hash(text(body));shaders.push({key,stage,hash:h,source:body});
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

  result.lateWorldSources=await c.ev(`window.__worldTrace.lateWorldSources`);
  result.shaderPrograms=await c.ev(`__bf.graphics.rust.shaderPrograms || []`);
  result.gpuDeaths=gpuDeaths();
  const size=result.setup.buffer[0]===1600 && result.setup.buffer[1]===900 && result.setup.target[0]===Math.round(1600*testCase.settings.renderScale) && result.setup.target[1]===Math.round(900*testCase.settings.renderScale);
  result.transparentOnly=testCase.camera==='world-transparent';
  result.valid=(role!=='candidate'||!result.lateWorldSources) && size && !result.gpuDeaths.length && result.points.length===manifest.captures && result.points.every(p=>p.lit>=.2 && !(role==='candidate'?p.errors:p.worldErrors).length && p.rustDraws>0 && !p.gl.lostEvents && p.gl.lostEvents!==null && !p.gl.contextLost && !p.gl.rustLost && p.gl.rustEnabled && !p.gl.error && (testCase.camera!=='water' || (p.rustPhases.includes('mirror') && p.stages.reflection)));
} catch(error) { result.error=String(error.stack || error);result.valid=false; }
finally {
  result.console=c.consoleErrors.slice(0,30);
  fs.writeFileSync(`${dir}/capture.json`,JSON.stringify(result,null,2));c.close();
}
process.exitCode=result.valid?0:2;
