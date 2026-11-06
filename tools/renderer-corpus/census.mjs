// Inventory every linked program after the corpus's battle sequence. Sources
// remain external test artifacts; no captured game resources enter the tree.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { expand, readManifest } from './manifest.mjs';
import { installGLTracking } from './gl-replay.mjs';
import { connect, loadBattle, GL_STATE, HELPERS_SRC } from './battle.mjs';
const manifest=expand(readManifest());
const id=process.env.BFE_CORPUS_CASE,out=process.env.OUT;
const test=manifest.cases.find(c=>c.id===id);
if(!test || !out)throw Error('Supply OUT and a corpus BFE_CORPUS_CASE');
fs.mkdirSync(out+'/shaders',{recursive:true});
const c=await connect();
const result={id,programs:[]};
try {
  result.loaded=await loadBattle(c,{settings:test.settings,query:test.query,extraSrc:`(${installGLTracking.toString()})([])`});
  if(result.loaded.error || !result.loaded.spawned)throw Error(result.loaded.error || 'Player not spawned');
  await c.ev(HELPERS_SRC);
  // Collect material provenance even for auxiliary scenes rendered by a hook.
  await c.ev(`(() => {const R=__bf.graphics.renderer,T=window.__shaderCensus={materials:new Map()};
    T.collect=(scene)=>scene?.traverse(o=>{for(const m of [].concat(o.material || []))T.materials.set(m,{object:o.name,material:m.name,type:m.type,skinned:!!o.isSkinnedMesh,instanced:!!o.isInstancedMesh});});
    const render=R.render;R.render=function(s,c){T.collect(s);return render.call(this,s,c);};
    for(const s of [__bf.graphics.scene,__bf.graphics.farScene,__bf.graphics.viewScene,__bf.hud.scene])T.collect(s);
    return 1;})()`);
  if (test.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__shaderCensus,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
  })()`);
  for(let k=0;k<manifest.captures;k++){
    await c.ev(`(async()=>{const b=__bf,G=window.__gate;window.__shaderCensus.place?.(${k});G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);
      for(let i=0;i<${manifest.historyFrames+manifest.settleFrames};i++)await b.lockstep(0,true);return 1;})()`);
  }
  result.programs=await c.ev(`(()=>{const g=__bf.graphics,R=g.renderer,gl=R.getContext(),T=window.__shaderCensus;
    return R.info.programs.map(p=>{const attributes=[],uniforms=[],materials=[];
      for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_ATTRIBUTES);i++){const a=gl.getActiveAttrib(p.program,i);attributes.push({name:a.name,type:a.type,size:a.size,location:gl.getAttribLocation(p.program,a.name)});}
      for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_UNIFORMS);i++){const u=gl.getActiveUniform(p.program,i);uniforms.push({name:u.name,type:u.type,size:u.size});}
      for(const [m,info] of T.materials){const props=R.properties.get(m);if(props.currentProgram===p || [...(props.programs?.values() || [])].includes(p))materials.push(info);}
      return {cacheKey:p.cacheKey,attributes,uniforms,materials,sources:gl.getAttachedShaders(p.program).map(s=>({stage:gl.getShaderParameter(s,gl.SHADER_TYPE),source:gl.getShaderSource(s)}))};});})()`);
  const linked=await c.ev(`Array.from(window.__glTracking.state(__bf.graphics.renderer.getContext()).seenPrograms.values()).map(p=>({attributes:p.layout,uniforms:p.uniforms,materials:[],cacheKey:'linked-source',sources:[{stage:35633,source:p.vertex},{stage:35632,source:p.fragment}]}))`);
  const raw=new Set(result.programs.map(p=>p.sources.map(x=>x.source).join('')));
  for(const p of linked)if(!raw.has(p.sources.map(x=>x.source).join('')) && !p.sources.some(x=>/uniform BfeFrame\b|uniform BfeShadow\b|bfeLoad\(/.test(x.source)))result.programs.push(p);
  for(const p of result.programs){p.stages=[];for(const s of p.sources){const hash=createHash('sha256').update(s.source).digest('hex');fs.writeFileSync(`${out}/shaders/${hash}.glsl`,s.source);p.stages.push({stage:s.stage,hash});}delete p.sources;}
  result.gl=await c.ev(GL_STATE);result.errors=await c.ev('[...__bf.graphics.rust.errors]');result.console=c.consoleErrors;
  result.valid=!result.gl.contextLost && !result.gl.lostEvents;
} catch(e){result.error=String(e.stack || e);process.exitCode=1;}
finally{fs.writeFileSync(`${out}/${id}.json`,JSON.stringify(result,null,2));c.close();}
