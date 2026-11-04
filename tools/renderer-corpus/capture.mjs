// One default-path run. Reference, candidate and repeat reference are separate
// applications; comparison never selects an alternate runtime implementation.
import fs from 'node:fs';
import { expand } from './manifest.mjs';
import { connect, loadBattle, GL_STATE, HELPERS_SRC } from './battle.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./manifest.json', import.meta.url))));
const id = process.env.BFE_CORPUS_CASE, out = process.env.OUT;
const role = process.env.BFE_CORPUS_ROLE;
if (!['reference', 'candidate', 'control'].includes(role) || !out) throw Error('Supply OUT and BFE_CORPUS_ROLE');
const testCase = manifest.cases.find(c => c.id === id);
if (!testCase) throw Error('Unknown corpus case ' + id);
const dir = `${out}/${id}-${role}`;
fs.mkdirSync(dir, { recursive: true });
const log = process.env.BFE_GPU_LOG;
const gpuDeaths = () => log && fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(l => /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|exit_code=512/i.test(l)) : ['missing GPU log'];
const c = await connect();
const result = { role, id, contract: manifest.version, points: [] };
try {
  result.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query });
  if (result.loaded.error || !result.loaded.spawned) throw Error(result.loaded.error || 'player not spawned');
  await c.ev(HELPERS_SRC);
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
  if (testCase.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__corpus,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
  })()`);
  for (let k=0;k<manifest.captures;k++) {
    const point = await c.ev(`(async () => {
      const b=__bf,g=b.graphics,G=window.__gate,T=window.__corpus,gl=g.renderer.getContext();
      G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);T.place?.(${k});
      for(const w of g.cascadeWindows || [])w.invalidate();
      for(let i=0;i<${manifest.settleFrames || 2};i++)await b.lockstep(0,true);
      g.upscaler?.invalidate();if(g.upscaling==='temporal')g.upscaler.frame=${k};
      for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
      G.resetFrame();T.stages={};T.want=true;const tick=await b.lockstep(0,true);T.want=false;
      const s=T.stages.presented;if(!s)throw Error('HUD did not draw');
      let lit=0;for(let i=0;i<s.buf.length;i+=64)if(s.buf[i]>8 || s.buf[i+1]>8 || s.buf[i+2]>8)lit++;
      return {k:${k},tick,lit:lit/(s.buf.length/64),rustDraws:G.rustDraws(),rustPhases:Object.keys(G.frameStats),errors:[...g.rust.errors],gl:{...${GL_STATE},error:gl.getError()},stages:Object.fromEntries(Object.entries(T.stages).map(([n,s])=>[n,{w:s.w,h:s.h,bytes:s.bytes}]))};
    })()`);
    for (const [name, stage] of Object.entries(point.stages)) {
      const data=await c.ev(`(() => {const s=window.__corpus.stages[${JSON.stringify(name)}],buf=new Uint8Array(s.buf.buffer);let str='';for(let i=0;i<buf.length;i+=0x8000)str+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(str);})()`);
      stage.file=`${k}-${name}.bin`;
      fs.writeFileSync(`${dir}/${stage.file}`,Buffer.from(data,'base64'));
    }
    result.points.push(point);
    console.log(JSON.stringify({role,k,tick:point.tick,stages:Object.keys(point.stages),errors:point.errors,gl:point.gl}));
  }
  result.shaderPrograms=await c.ev(`__bf.graphics.rust.shaderPrograms || []`);
  result.gpuDeaths=gpuDeaths();
  const size=result.setup.buffer[0]===1600 && result.setup.buffer[1]===900 && result.setup.target[0]===Math.round(1600*testCase.settings.renderScale) && result.setup.target[1]===Math.round(900*testCase.settings.renderScale);
  result.valid=size && !result.gpuDeaths.length && result.points.length===manifest.captures && result.points.every(p=>p.lit>=.2 && p.rustDraws>0 && !p.gl.lostEvents && p.gl.lostEvents!==null && !p.gl.contextLost && !p.gl.rustLost && p.gl.rustEnabled && !p.gl.error && !p.errors.length && (testCase.camera!=='water' || (p.rustPhases.includes('mirror') && p.stages.reflection)));
} catch(error) { result.error=String(error.stack || error);result.valid=false; }
finally {
  result.console=c.consoleErrors.slice(0,30);
  fs.writeFileSync(`${dir}/capture.json`,JSON.stringify(result,null,2));c.close();
}
process.exitCode=result.valid?0:2;
