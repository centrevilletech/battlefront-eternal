// Capture shadow phase resources before source ownership changes. The frozen
// reference replays its compiled source; the candidate registers the owned key.
// The shadow-caster replay. `reference` is true for the frozen reference build,
// which replays the three.js-compiled source it recorded; the candidate
// registers the owned program by key. Both then draw the same recorded casters
// into an isolated depth target and the depths are compared as RGBA.
export function installActorReplay(reference) {
  const b=__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=__corpus;
  const {store,release,snapshot,textures,upload,depthRGBA}=T.replayTools;
  const shadowFrame=core.rust.shadow_frame.bind(core.rust),replay=T.replay;
  const DRAW=151,UNIFORM=18;
  const source=(d)=>{
    const p=R.properties.get(d.depth).currentProgram;
    if(!p)throw Error('Missing frozen shadow program');
    const shaders=gl.getAttachedShaders(p.program).map(s=>({stage:gl.getShaderParameter(s,gl.SHADER_TYPE),source:gl.getShaderSource(s)}));
    return {vertex:shaders.find(s=>s.stage===gl.VERTEX_SHADER).source,fragment:shaders.find(s=>s.stage===gl.FRAGMENT_SHADER).source};
  };
  // Which actors/ depth program a caster uses, as the candidate names it, so a reference draw can be matched to a variant.
  const key=(d)=>{
    if(d.depth.userData.bfeProgram)return d.depth.userData.bfeProgram;
    if(d.object.userData.fabricWind)return d.depth.alphaTest>0?'actors/depth-fabric-cutout':'actors/depth-fabric';
    const cut=Boolean(d.depth.uniforms?.cut);
    return 'actors/depth-'+(d.object.isSkinnedMesh?'skinned'+(cut?'-cutout':''):cut?'cutout':'solid');
  };
  // The caster's attributes and index, copied (the live buffers change next frame).
  const geometry=(d)=>{
    const geo=d.object.geometry,attributes={};
    for(const [name] of d.program.attributes){
      const a=geo.attributes[name],interleaved=a.isInterleavedBufferAttribute,buffer=interleaved?a.data:a;
      attributes[name]={array:store(buffer.array.slice()),type:buffer.array.constructor.name,size:a.itemSize,normalized:a.normalized,stride:interleaved?buffer.stride:0,offset:interleaved?a.offset:0};
    }
    return {attributes,index:store(geo.index.array.slice()),indexType:geo.index.array.constructor.name};
  };
  // Wrap the renderer's shadow entry point: when a capture is wanted, record the
  // pass's input, resources and draws, run it, then snapshot the depth it wrote.
  core.rust.shadow_frame=function(){
    if(!T.want || !T.captureReplay)return shadowFrame();
    const pass=core.byName.shadow,req=pass.request;
    if(!req || !pass.done)return shadowFrame();
    core.views();const input=Array.from(core.casterInput.subarray(0,4+core.casterInput[3]*DRAW));
    const objects=textures([req.light.shadow.map.depthTexture,req.from?.depthTexture,...pass.draws.flatMap(d=>[d.object.skeleton?.boneTexture,d.depth.map,d.depth.alphaMap,...Object.values(d.depth.uniforms || {}).map(u=>u.value)])]);
    const ids=[];if(input[1]>=0)ids.push(input[1]);
    for(let i=0;i<input[3];i++)for(let j=0;j<input[4+i*DRAW+6];j++){const at=4+i*DRAW+7+j*UNIFORM;if(input[at+1]===32 && input[at+2]>=0)ids.push(input[at+2]);}
    const resources=[...new Set(ids)].map(id=>({...snapshot(id,objects),depth:id===input[1]}));
    const draws=pass.draws.map(d=>({key:key(d),program:d.program.id,uniforms:d.program.uniforms,attributes:d.program.attributes,geometry:geometry(d),...source(d)}));
    core.views();core.casterInput.set(input);const result=shadowFrame();
    const output=snapshot(input[0],objects),depth=T.blobs[output.levels[0]];let covered=0;for(let i=0;i<depth.length;i+=output.packedDepth?1:4)if(depth[i]<1)covered++;
    const row={kind:'shadow',key:'actors/shadow',input,draws,resources,covered,output:{w:output.w,h:output.h,bytes:4,depthType:output.depthType,blob:output.levels[0],packedDepth:output.packedDepth}};
    T.records.push(row);
    // (The whole pass is one record; each variant is also recorded alone below.)
    // Isolate each caster program as well: another caster must not mask a
    // changed fragment behind its depth. Every isolated target starts clear.
    const groups=new Map();
    for(let i=0;i<draws.length;i++){
      const d=draws[i];let group=groups.get(d.key);
      if(!group){group={...row,input:input.slice(0,4),draws:[]};group.input[1]=-1;groups.set(d.key,group);}
      group.input.push(...input.slice(4+i*DRAW,4+(i+1)*DRAW));group.draws.push(d);
    }
    const wanted=T.want,target=R.getRenderTarget();
    try{for(const group of groups.values()){
      group.input[3]=group.draws.length;
      const isolated=run(group,T.blobs,true),pixels=T.blobs[isolated.levels[0]];let count=0;
      for(let i=0;i<pixels.length;i+=isolated.packedDepth?1:4)if(pixels[i]<1)count++;
      group.covered=count;group.output={w:isolated.w,h:isolated.h,bytes:4,depthType:isolated.depthType,blob:isolated.levels[0],packedDepth:isolated.packedDepth};T.records.push(group);
    }}finally{T.want=wanted;core.views();core.casterInput.set(input);R.resetState();R.setRenderTarget(target);}

    return result;
  };
  const targets=new Map(),programs=new Map();
  // Redraw one record into a cleared depth target of the recorded size and, unless
  // capturing, compare it with the recorded output.
  const run=(row,blobs,capture=false)=>{
      T.want=false;core.views();const saved=core.casterInput.slice();
      const created=row.resources.map(s=>[s.id,upload(s,blobs)]),ids=new Map(created.map(([id,s])=>[id,s.id]));
      const targetKey=row.output.w+':'+row.output.depthType;let target=targets.get(targetKey);
      if(!target){target=new b.THREE.WebGLRenderTarget(row.output.w,row.output.h,{depthTexture:new b.THREE.DepthTexture(row.output.w,row.output.h,row.output.depthType ?? b.THREE.FloatType)});R.initRenderTarget(target);targets.set(targetKey,target);}
      const targetId=core.texture(target.depthTexture),input=Float64Array.from(row.input);input[0]=targetId;input[1]=input[1]<0?-1:ids.get(input[1]);
      for(let i=0;i<row.draws.length;i++){
        const d=row.draws[i],at=4+i*DRAW;
        const cacheKey=reference?d.vertex+'\n'+d.fragment:d.key;
        let program=programs.get(cacheKey);
        if(program===undefined){program=reference?core.rust.add_program(3,d.vertex,d.fragment,d.attributes.map(([n,l])=>n+':'+l).join(','),d.uniforms.map(u=>u.name).join(',')):core.rust.add_owned_program(d.key);programs.set(cacheKey,program);}
        // (A recorded draw's uniform and attribute order is the frozen program's.)
        // Source-owned layouts may choose a different uniform/attribute order.
        const layout=reference?d.attributes:T.actorLayouts[d.key].attributes;
        const uniforms=reference?d.uniforms:T.actorLayouts[d.key].uniforms;
        const geo=new b.THREE.BufferGeometry();
        for(const [name,a] of Object.entries(d.geometry.attributes)){
          const data=blobs[a.array];geo.setAttribute(name,a.stride?new b.THREE.InterleavedBufferAttribute(new b.THREE.InterleavedBuffer(data,a.stride),a.size,a.offset,a.normalized):new b.THREE.BufferAttribute(data,a.size,a.normalized));
        }
        geo.setIndex(new b.THREE.BufferAttribute(blobs[d.geometry.index],1));
        const geometryId=core.geometry(geo,{id:program,attributes:layout});if(geometryId==null)throw Error('Shadow replay geometry');
        input[at]=program;input[at+1]=geometryId;
        const old=input.slice(at+7,at+DRAW);input[at+6]=uniforms.length;
        for(let j=0;j<uniforms.length;j++){
          const k=d.uniforms.findIndex(u=>u.name===uniforms[j].name);if(k<0)throw Error('Missing shadow uniform '+uniforms[j].name);
          input.set(old.subarray(k*UNIFORM,(k+1)*UNIFORM),at+7+j*UNIFORM);input[at+7+j*UNIFORM]=j;
          if(input[at+8+j*UNIFORM]===32)input[at+9+j*UNIFORM]=ids.get(input[at+9+j*UNIFORM]);
        }
      }
      core.views();core.casterInput.set(input);shadowFrame();
      const actual=snapshot(targetId,textures([target.depthTexture]));let metric;
      if(!capture){
        const a=depthRGBA(actual,T.blobs),e=depthRGBA(row.output,blobs);
        metric={key:row.key,...T.replayTools.rgbaMetrics(a,e),draws:row.draws.length,covered:row.covered,variants:[...new Set(row.draws.map(d=>d.key))]};
        for(const key of actual.levels)release(key,blobs);
      }
      core.views();core.casterInput.set(saved);for(const [,s] of created)s.dispose();
      return capture?actual:metric;
  };
  T.replay=async function(records,blobs){
    const metrics=[];
    for(const row of records)if(row.kind==='shadow')metrics.push(run(row,blobs));else metrics.push(...await replay([row],blobs));
    return metrics;
  };
}
