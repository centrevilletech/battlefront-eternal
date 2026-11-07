// Observe compiled actor variants in a real, seeded battle. Missing states and
// programs remain explicit in the census; an observation is not replay proof.
import fs from 'node:fs';
import { connect, loadBattle } from './battle.mjs';
import { expand } from './manifest.mjs';
const manifest=expand(JSON.parse(fs.readFileSync(new URL('./manifest.json',import.meta.url))));
const test=manifest.cases.find(c=>c.id===process.env.BFE_CORPUS_CASE);
if(!test)throw Error('Unknown census case');
// Environment narrows one run to a shadow setting, a hero or droideka class, or the dissolve
// presentation; each combination is its own census id so results never overwrite.
const settings={...test.settings,...(process.env.BFE_ACTOR_SHADOWS?{shadows:process.env.BFE_ACTOR_SHADOWS}:{})};
const censusId=test.id+(process.env.BFE_ACTOR_SHADOWS?'-'+process.env.BFE_ACTOR_SHADOWS:'')+(process.env.BFE_ACTOR_CLASS?'-'+process.env.BFE_ACTOR_CLASS:'')+(process.env.BFE_ACTOR_DISSOLVE?'-dissolve':'')+(process.env.BFE_ACTOR_WRECKS?'-wrecks':'');
let query=test.query.replace('heroes=0','heroes=1');
const heroClasses=new Set(['rep_inf_macewindu','cis_inf_countdooku','imp_inf_darthvader','all_inf_lukeskywalker']);
if(process.env.BFE_ACTOR_CLASS){const cls=process.env.BFE_ACTOR_CLASS,side=cls.split('_')[0],unit=heroClasses.has(cls)?({rep:'rep_inf_clone_trooper',cis:'cis_inf_droideka',imp:'imp_inf_storm_trooper',all:'all_inf_soldierjungle'})[side]:cls;query=query.replace(/unit=[^&]+/,'unit='+unit).replace(/side=[^&]+/,'side='+side).replace(/era=[^&]+/,'era='+(['rep','cis'].includes(side)?'cw':'gcw'));}
// Injected before the game loads: wrap shader compilation and program reflection so every
 // program the battle links is recorded with its source, attributes and uniforms, including
 // warm-up programs that are disposed afterwards.
const censusSource=`(() => {
 const P=WebGL2RenderingContext.prototype,source=P.shaderSource,attach=P.attachShader,parameter=P.getProgramParameter;
 const shaders=new Map(),programs=new Map();window.__actorCompileCensus=[];
 P.shaderSource=function(shader,text){shaders.set(shader,text);return source.call(this,shader,text);};
 P.attachShader=function(program,shader){let row=programs.get(program);if(!row){row={shaders:[]};programs.set(program,row);}row.shaders.push({stage:this.getShaderParameter(shader,this.SHADER_TYPE),source:shaders.get(shader)});return attach.call(this,program,shader);};
 P.getProgramParameter=function(program,name){const value=parameter.call(this,program,name),row=programs.get(program);
  if(row && !row.recorded && ((name===this.LINK_STATUS && value) || name===this.ACTIVE_UNIFORMS || name===this.ACTIVE_ATTRIBUTES)){
   row.recorded=true;const attributes=[],uniforms=[];
   for(let i=0;i<parameter.call(this,program,this.ACTIVE_ATTRIBUTES);i++){const a=this.getActiveAttrib(program,i);attributes.push({name:a.name,type:a.type,size:a.size,location:this.getAttribLocation(program,a.name)});}
   for(let i=0;i<parameter.call(this,program,this.ACTIVE_UNIFORMS);i++){const u=this.getActiveUniform(program,i);uniforms.push({name:u.name,type:u.type,size:u.size});}
   __actorCompileCensus.push({shaders:row.shaders,attributes,uniforms});
  }return value;
 };
})()`;
// Load the seeded battle (heroes on), render a few lockstep frames, then record the states
// each actor presentation puts the materials in.
const c=await connect();
try {
 const loaded=await loadBattle(c,{settings,extraSrc:censusSource,query});
 if(loaded.error || !loaded.spawned)throw Error(loaded.error || 'Player not spawned');
 const observations=[];
 for(const shadows of [settings.shadows]){
  const result=await c.ev(`(async()=>{
   const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext();
   g.warmup();
   for(let i=0;i<3;i++)await b.lockstep(1/60,true);
   const heroes=[...b.units.all()].filter(u=>u.hero || /macewindu|darthvader|countdooku|lukeskywalker/.test(u.className)).map(u=>({className:u.className,alive:u.alive,visible:u.actor?.root.visible}));
   if(${heroClasses.has(process.env.BFE_ACTOR_CLASS)}){const target=${JSON.stringify(process.env.BFE_ACTOR_CLASS || '')};if(!heroes.some(h=>h.className===target && h.alive))throw Error('Requested AI hero absent: '+target);}
   const states=[];
   if(${Boolean(process.env.BFE_ACTOR_CLASS)}) {
    const key=(code,down)=>window.dispatchEvent(new KeyboardEvent(down?'keydown':'keyup',{code,key:code,bubbles:true}));
    for(const action of ['fire','walk','crouch']) {
     if(action==='fire')window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
     if(action==='walk')key('KeyW',true);
     if(action==='crouch')key('KeyC',true);
     for(let i=0;i<120;i++)await b.lockstep(1/60,i%10===0);
     key('KeyC',false);states.push({action,alive:b.playerUnit?.alive,className:b.playerUnit?.className,mode:b.playerUnit?.soldier?.state});
    }
   }
   if(${Boolean(process.env.BFE_ACTOR_DISSOLVE)}) {
    const unit=b.playerUnit;if(!unit?.walker || typeof unit.dissolve!=='function')throw Error('No live droideka dissolve presentation');
    unit.dissolve(.3,g.settings.get('destructionEffects')==='enhanced');g.warmup();
    states.push({action:'dissolve',className:unit.className,defines:unit.actor.body.material.defines});
   }
   const wrecks=[];
   for(const station of b.destruction?.stations || [])if(station.looks?.destroyed?.root){
    const site=b.destruction.wreck(station);
    wrecks.push(...site.meshes.map(mesh=>({skinned:Boolean(mesh.isSkinnedMesh),defines:[].concat(mesh.material).map(m=>m.defines)})));
   }
   if(wrecks.length){g.warmup();states.push({action:'prop-wreck',meshes:wrecks});}
   const stages=[],programs=[],materials=[];
   const hash=async source=>[...new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(source)))].map(x=>x.toString(16).padStart(2,'0')).join('');
   for(const row of window.__actorCompileCensus || []) {
    const sources=[];for(const shader of row.shaders){const h=await hash(shader.source);stages.push({hash:h,...shader});sources.push(h);}
    programs.push({stages:sources,attributes:row.attributes,uniforms:row.uniforms,lifetime:true});
   }
   for(const p of R.info.programs){
    const sources=[];for(const shader of gl.getAttachedShaders(p.program)||[]){const source=gl.getShaderSource(shader),h=await hash(source);stages.push({hash:h,stage:gl.getShaderParameter(shader,gl.SHADER_TYPE),source});sources.push(h);}
    const attributes=[],uniforms=[];for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_ATTRIBUTES);i++){const a=gl.getActiveAttrib(p.program,i);attributes.push({...a,name:a.name,type:a.type,size:a.size,location:gl.getAttribLocation(p.program,a.name)});}
    for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_UNIFORMS);i++){const u=gl.getActiveUniform(p.program,i);uniforms.push({name:u.name,type:u.type,size:u.size});}
    programs.push({id:p.id,stages:sources,attributes,uniforms});
   }
   for(const [role,scene] of [['scene',g.scene],['view',g.viewScene]])scene.traverse(o=>{
    for(const [depth,mm] of [[false,o.material],[true,o.customDepthMaterial]])for(const m of (Array.isArray(mm)?mm:[mm])){
     if(!m)continue;const props=R.properties.get(m),ps=props.programs?[...props.programs.values()]:props.currentProgram?[props.currentProgram]:[];
     materials.push({role,depth,mesh:o.name,skinned:!!o.isSkinnedMesh,instanced:!!o.isInstancedMesh,pose:!!o.userData.poseSphere,fabric:!!o.userData.fabricWind,name:m.name,type:m.type,defines:m.defines || {},side:m.side,programs:ps.map(p=>p.id),owned:m.userData.bfeProgram || null});
    }
   });
   return {heroes,states,shadows:${JSON.stringify(shadows)},cascades:g.csm?.lights.length || 0,stages,programs,materials,errors:[...g.rust.errors],gl:gl.getError()};
  })()`);
  observations.push(result);console.log(test.id,shadows,result.programs.length,result.errors.length,result.gl);
 }
 const out=process.env.OUT;fs.mkdirSync(out,{recursive:true});fs.writeFileSync(`${out}/${censusId}.json`,JSON.stringify({version:2,id:censusId,loaded,observations}));
}finally{c.close();}
