// Replay observed actor inputs in an isolated renderer and target. The scene's
// world draws are excluded; actor geometry, poses, lights and sampled texels are
// frozen. Both implementations execute their normal frame API with those inputs.
export function installInstanceReplay(reference) {
 const b=__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=__corpus;
 const {store,release,snapshot,textures,upload,read}=T.replayTools;
 const previous=T.replay,frame=core.rust.frame.bind(core.rust),view=core.rust.view_frame.bind(core.rust),mirror=core.rust.mirror_frame.bind(core.rust);
 // Both builds run their own normal frame API; only the inputs are frozen and shared.
 const layouts=T.actorLayouts,modeOf=k=>k===1 || k===3?1:2;
 const entryFor=id=>{
  for(const cache of [core.programs,core.lampPrograms])for(const [program,entry] of cache)if(entry?.id===id)return {program,entry};
  throw Error('No frozen actor program '+id);
 };
 const findRecord=(id,geometry,viewMode)=>{
  const pass=core.byName[viewMode?'view':'instances'];
  for(const record of [...(pass.records || []),...(pass.bodyRecords || []),...(pass.meshRecords || []),...(pass.flyingRecords || [])])if(record.geometry===geometry && record.program===id)return record;
  throw Error('No frozen actor geometry '+id+':'+geometry);
 };
 const captureGeometry=(geometry,attributes)=>{
  const rows={};for(const [name] of attributes){const a=geometry.attributes[name],buffer=a.isInterleavedBufferAttribute?a.data:a;rows[name]={blob:store(buffer.array.slice()),size:a.itemSize,normalized:a.normalized,stride:a.isInterleavedBufferAttribute?buffer.stride:0,offset:a.isInterleavedBufferAttribute?a.offset:0};}
  return {attributes:rows,index:store(geometry.index.array.slice()),groups:geometry.groups};
 };
 const select=(material,kind,lamps,cascades)=>{
  // The shock glow is the effects family's: its owned variant by the cascades on (shock-s0, or its shadowed twin).
  if(material.name==='arc-shock')return cascades>0?'effects/shock-s0-shadow':'effects/shock-s0';
  const defines=Object.entries(material.defines).filter(([n])=>n!=='ADDITIVE').map(([n,v])=>[n,String(v)]).sort(([a],[b])=>a.localeCompare(b));
  for(const [key,layout] of Object.entries(layouts)){const s=layout.selector;if(s && s.skin===(kind===1) && s.mode===(kind===1?'skinned':'rigid') && s.lamps===lamps && s.cascades===cascades && JSON.stringify(s.defines)===JSON.stringify(defines))return key;}
  throw Error('Unowned observed actor '+JSON.stringify({defines,kind,lamps,cascades}));
 };
 const blendState=material=>{
  const B=b.THREE,p=material.premultipliedAlpha;let blend;
  if(material.blending===B.NormalBlending)blend=p?[1,gl.FUNC_ADD,gl.FUNC_ADD,gl.ONE,gl.ONE_MINUS_SRC_ALPHA,gl.ONE,gl.ONE_MINUS_SRC_ALPHA]:[1,gl.FUNC_ADD,gl.FUNC_ADD,gl.SRC_ALPHA,gl.ONE_MINUS_SRC_ALPHA,gl.ONE,gl.ONE_MINUS_SRC_ALPHA];
  else if(material.blending===B.AdditiveBlending)blend=[1,gl.FUNC_ADD,gl.FUNC_ADD,p?gl.ONE:gl.SRC_ALPHA,gl.ONE,p?gl.ONE:gl.SRC_ALPHA,gl.ONE];
  else throw Error('Unrepresented actor blend '+material.blending);
  const funcs={[B.NeverDepth]:gl.NEVER,[B.LessDepth]:gl.LESS,[B.EqualDepth]:gl.EQUAL,[B.LessEqualDepth]:gl.LEQUAL,[B.GreaterDepth]:gl.GREATER,[B.NotEqualDepth]:gl.NOTEQUAL,[B.GreaterEqualDepth]:gl.GEQUAL,[B.AlwaysDepth]:gl.ALWAYS};
  return {blend,flags:(material.depthTest?1:0)|(material.depthWrite?2:0),func:funcs[material.depthFunc],sides:material.side===B.DoubleSide && !material.forceSinglePass?[B.BackSide,B.FrontSide]:[material.side]};
 };
 // asReference picks the program a record draws with: the recorded three-compiled source (true) or the
 // owned descriptor (false). A role replays with its own kind; the draw diagnostic below uses both.
 const run=(row,blobs,asReference=reference)=>{
  diagGl();
  const before=R.getRenderTarget(),clearColor=R.getClearColor(new b.THREE.Color()),clearAlpha=R.getClearAlpha(),fresh=new core.rust.constructor(gl),memory=()=>core.wasm.memory.buffer;
  const created=row.resources.map(s=>[s.id,upload(s,blobs)]),ids=new Map(created.map(([id,s])=>[id,T.replayTools.register(fresh,s)]));
  if(T.diagGl){const index=fresh.geometry_index.bind(fresh);T.indexUploads=[];fresh.geometry_index=(g,bytes,type)=>{T.indexUploads.push([g,bytes.byteLength,bytes.buffer.byteLength,bytes.buffer===core.wasm.memory.buffer]);return index(g,bytes,type);};}
  const input=Float64Array.from(row.input),stride=fresh.record_stride(),recordsAt=fresh.records_at();
  const target=new b.THREE.WebGLRenderTarget(row.width,row.height,{type:row.bytes===2?b.THREE.HalfFloatType:row.bytes===4?b.THREE.FloatType:b.THREE.UnsignedByteType,samples:row.samples,depthBuffer:true});R.initRenderTarget(target);
  const programs=new Map();
  try {
   for(let i=0;i<row.records.length;i++){
    const rec=row.records[i],at=recordsAt+i*stride;
    let program=programs.get(rec.key);
    if(program===undefined){program=asReference?(rec.lamps?fresh.add_program_lamps(rec.kind,rec.vertex,rec.fragment,rec.attributes.map(([n,l])=>n+':'+l).join(','),''):fresh.add_program(rec.kind,rec.vertex,rec.fragment,rec.attributes.map(([n,l])=>n+':'+l).join(','),'')):fresh.add_owned_program(rec.key);programs.set(rec.key,program);}
    const attributes=asReference?rec.attributes:layouts[rec.key].attributes;
    const geo=new b.THREE.BufferGeometry();for(const [name,a] of Object.entries(rec.geometry.attributes)){const data=blobs[a.blob];geo.setAttribute(name,a.stride?new b.THREE.InterleavedBufferAttribute(new b.THREE.InterleavedBuffer(data,a.stride),a.size,a.offset,a.normalized):new b.THREE.BufferAttribute(data,a.size,a.normalized));}
    {const index=blobs[rec.geometry.index];if(!index?.length)throw Error(`Replay index ${rec.geometry.index} is empty: ${index?.constructor?.name} of ${index?.length}, buffer ${index?.buffer?.byteLength} bytes`);}
    geo.setIndex(new b.THREE.BufferAttribute(blobs[rec.geometry.index],1));for(const group of rec.geometry.groups)geo.addGroup(group.start,group.count,group.materialIndex);
    // Reuse the bridge's byte-for-byte buffer upload contract with the new
    // renderer, so no context-local resource handle crosses the replay.
    const uploader=Object.create(core);uploader.rust=fresh;uploader.geometries=new Map();uploader.errors=[];
    const geometry=uploader.geometry(geo,{id:program,attributes});if(geometry==null)throw Error('Actor replay geometry');
    input[at+1]=program;input[at+2]=geometry;
    if(rec.blend)input[at+96]=fresh.inst_add_blend(new Uint32Array(rec.blend.blend),rec.blend.flags,rec.blend.func,new Uint32Array(rec.blend.sides.flatMap(side=>[program,side])));
    for(let j=0;j<Math.max(1,input[at+3]);j++)if(input[at+4+j]>=0)input[at+4+j]=ids.get(input[at+4+j]);
   }
   if(row.far>=0)fresh.set_far_map(ids.get(row.far));
   fresh.set_shadow_maps(new Uint32Array(row.cascades.map(id=>ids.get(id))));
   if(row.lamps.length)fresh.set_lamps(new Float32Array(row.lamps));
   new Float64Array(memory(),fresh.input_ptr(),fresh.input_len()).set(input);
   new Float64Array(memory(),fresh.shadow_input_ptr(),fresh.shadow_input_len()).set(row.shadow);
   new Float32Array(memory(),fresh.bones_ptr(),fresh.bones_len()).set(blobs[row.bones]);
   R.resetState();R.setRenderTarget(target);R.setClearColor(0x204060,.625);R.clear(true,true,true);
   if(row.viewInput)new Float64Array(memory(),fresh.view_input_ptr(),fresh.view_input_len()).set(row.viewInput);
   core.resolve(target);
   const clearPixel=row.bytes===2?new Uint16Array(4):row.bytes===4?new Float32Array(4):new Uint8Array(4),oldRead=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);
   gl.bindFramebuffer(gl.READ_FRAMEBUFFER,(R.resources?.gpu?R.resources.gpu.framebuffer(R.resources.targetId(target),false,0):R.properties.get(target).__webglFramebuffer));gl.readPixels(0,0,1,1,gl.RGBA,row.bytes===2?gl.HALF_FLOAT:row.bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,clearPixel);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,oldRead);
   const stats=row.view?fresh.view_frame(row.records.length):row.mirror?fresh.mirror_frame(row.records.length):fresh.frame(row.records.length);
   if(!row.view){const blended=row.mirror?fresh.mirror_transparent_frame(0,0):fresh.transparent_frame(0,0);for(let i=5;i<stats.length;i++)stats[i]+=blended[i];}
   // A GL error here came from this row's own draws: name its programs rather than failing later in the readback.
   {const error=gl.getError();if(error)throw Error('Replay draw GL '+error+' in '+[...new Set(row.records.map(r=>r.key))].join(','));}
   // Resolve MSAA before reading the isolated phase's RGBA attachment.
   core.resolve(target);const output=read(target);
   const pixels=T.blobs[output.blob];let covered=0;for(let i=0;i<pixels.length;i+=4)if(pixels[i]!==clearPixel[0] || pixels[i+1]!==clearPixel[1] || pixels[i+2]!==clearPixel[2] || pixels[i+3]!==clearPixel[3])covered++;
   return {output,stats,covered};
  }finally{fresh.free();for(const [,s] of created)s.dispose();target.dispose();core.views();R.setClearColor(clearColor,clearAlpha);R.resetState();R.setRenderTarget(before);}
 };
 const capture=(count,viewMode,draw,mirrorMode=false)=>{
  if(!T.want || !T.captureReplay || !count)return draw(count);
  core.views();const stride=core.stride,recordsAt=core.recordsAt,input=Array.from(core.input.subarray(0,recordsAt+count*stride)),records=[],textureIds=[];
  let bones=0;
  for(let i=0;i<count;i++){
   const at=recordsAt+i*stride,kind=modeOf(input[at]),record=findRecord(input[at+1],input[at+2],viewMode),object=record.body || record.mesh,material=record.used?.[0] || record.material;
   const {program,entry}=entryFor(input[at+1]),shaders=gl.getAttachedShaders(program.program),source=stage=>gl.getShaderSource(shaders.find(s=>gl.getShaderParameter(s,gl.SHADER_TYPE)===stage));
   const key=select(material,kind,Boolean(entry.lamps),viewMode?0:g.csm?.lights.length || 0);
   records.push({key,kind,lamps:Boolean(entry.lamps),vertex:source(gl.VERTEX_SHADER),fragment:source(gl.FRAGMENT_SHADER),attributes:entry.attributes,geometry:captureGeometry(object.geometry,entry.attributes),blend:input[at]>=3?blendState(material):null});
   bones=Math.max(bones,input[at+9]+input[at+8]);for(let j=0;j<Math.max(1,input[at+3]);j++)if(input[at+4+j]>=0)textureIds.push(input[at+4+j]);
  }
  const cascades=viewMode?[]:(g.shadowMaps || []).map(t=>core.texture(t.depthTexture)),far=core.textures.get(core.farTexture) ?? core.texture(g.farTarget.texture);
  textureIds.push(...cascades,far);
  const objects=[];for(const scene of [g.scene,g.viewScene,g.farScene])scene.traverse(o=>{for(const m of Array.isArray(o.material)?o.material:[o.material])for(const u of Object.values(m?.uniforms || {}))if(u.value?.isTexture)objects.push(u.value);});objects.push(g.farTarget.texture,...g.shadowMaps.map(t=>t.depthTexture));const water=g.scene.userData.water;if(water?.reflectionFar)objects.push(water.reflectionFar.texture);
  const available=textures(objects),resources=[...new Set(textureIds.filter(id=>id>=0))].map(id=>({...snapshot(id,available),depth:cascades.includes(id)}));
  const lamps=[];if(g.settings.get('lampLighting'))for(const s of g.localLights?.sources || [])lamps.push(s.position.x,s.position.y,s.position.z,s.radius,s.color.x,s.color.y,s.color.z,0);
  const target=R.getRenderTarget() || g.sceneTarget;core.views();const row={kind:'instances',key:viewMode?'actors/view':mirrorMode?'actors/mirror':'actors/instances',view:viewMode,mirror:mirrorMode,width:target.width,height:target.height,bytes:target.texture.type===1016?2:target.texture.type===1015?4:1,samples:target.samples,input,records,resources,cascades,far,lamps,shadow:core.shadowInput?Array.from(core.shadowInput):new Array(core.rust.shadow_input_len()).fill(0),bones:store(core.bones.slice(0,bones*16))};
  if(viewMode && core.viewInput) {
   row.viewInput=[0];const saved=core.viewInput;
   for(let i=0;i<saved[0];i++){const at=1+i*593;if(saved[at]<0){row.viewInput.push(...saved.subarray(at,at+593));row.viewInput[0]++;}}
  }
  const groups=new Map();let blendIndex=0;
  for(let i=0;i<row.records.length;i++){
   const record=row.records[i];let group=groups.get(record.key);if(!group){group={...row,input:row.input.slice(0,recordsAt),records:[],blendMap:new Map()};groups.set(record.key,group);}
   group.input.push(...row.input.slice(recordsAt+i*stride,recordsAt+(i+1)*stride));group.records.push(record);
   if(row.input[recordsAt+i*stride]>=3)group.blendMap.set(blendIndex++,group.blendMap.size);
  }
  for(const group of groups.values()){
   if(row.viewInput){group.viewInput=[0];for(let i=0;i<row.viewInput[0];i++){const at=1+i*593,old=row.viewInput[at+1];if(group.blendMap.has(old)){const draw=row.viewInput.slice(at,at+593);draw[1]=group.blendMap.get(old);group.viewInput.push(...draw);group.viewInput[0]++;}}}
   delete group.blendMap;
   const expected=run(group,T.blobs);group.output=expected.output;group.draws=expected.stats.slice(5).reduce((a,b)=>a+b,0);group.covered=expected.covered;T.records.push(group);
  }
  core.views();core.input.set(input);return draw(count);
 };
 // BFE_DIAG_GL: the first instanced draw that raises a GL error, with the bound VAO's attribute arrays and the
 // element buffer as the driver sees them (sizes in bytes), so an "insufficient buffer" error names its buffer.
 const diagGl=()=>{
  if(!T.diagGl || T.diagGlInstalled)return;
  T.diagGlInstalled=true;
  const P=WebGL2RenderingContext.prototype,draw=P.drawElementsInstanced;
  // Each vertex array's element-buffer history: which buffer was bound into it and how many bytes it was given.
  const ids=new WeakMap();let nextId=1;const id=o=>o?(ids.get(o) ?? (ids.set(o,nextId++),ids.get(o))):0;
  const history=T.elementHistory=new Map();const note=(gl,what,extra)=>{const vao=id(gl.getParameter(gl.VERTEX_ARRAY_BINDING));const h=history.get(vao) || [];if(h.length<12)h.push([what,extra]);history.set(vao,h);};
  const bindBuffer=P.bindBuffer;P.bindBuffer=function(target,buffer){if(target===this.ELEMENT_ARRAY_BUFFER)note(this,'bind',id(buffer));return bindBuffer.call(this,target,buffer);};
  const bufferData=P.bufferData;P.bufferData=function(target,data,...rest){if(target===this.ELEMENT_ARRAY_BUFFER)note(this,'data',typeof data==='number'?data:[data?.byteLength,data?.length,data?.byteOffset,data?.buffer?.byteLength,data?.buffer===core.wasm.memory.buffer,data?.constructor?.name,rest.length]);return bufferData.call(this,target,data,...rest);};
  const deleteBuffer=P.deleteBuffer;P.deleteBuffer=function(buffer){T.deletedBuffers=(T.deletedBuffers||[]);if(T.deletedBuffers.length<50)T.deletedBuffers.push(id(buffer));return deleteBuffer.call(this,buffer);};
  P.drawElementsInstanced=function(mode,count,type,offset,instances){
   draw.call(this,mode,count,type,offset,instances);
   if(T.firstGlError)return;
   const error=this.getError();if(!error)return;
   const sizeOf=b=>{if(!b)return null;const old=this.getParameter(this.COPY_READ_BUFFER_BINDING);this.bindBuffer(this.COPY_READ_BUFFER,b);const n=this.getBufferParameter(this.COPY_READ_BUFFER,this.BUFFER_SIZE);this.bindBuffer(this.COPY_READ_BUFFER,old);return n;};
   const attributes=[];
   for(let l=0;l<16;l++)if(this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_ENABLED))attributes.push({location:l,size:this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_SIZE),type:this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_TYPE),stride:this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_STRIDE),offset:this.getVertexAttribOffset(l,this.VERTEX_ATTRIB_ARRAY_POINTER),divisor:this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_DIVISOR),bytes:sizeOf(this.getVertexAttrib(l,this.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING))});
   const vao=id(this.getParameter(this.VERTEX_ARRAY_BINDING)),elementId=id(this.getParameter(this.ELEMENT_ARRAY_BUFFER_BINDING));
   T.firstGlError={indexUploads:T.indexUploads,vao,elementId,history:history.get(vao),deleted:(T.deletedBuffers||[]).includes(elementId),error,count,type,offset,instances,element:sizeOf(this.getParameter(this.ELEMENT_ARRAY_BUFFER_BINDING)),attributes,program:this.getParameter(this.CURRENT_PROGRAM)?true:false};
   this.getError();this.__pending=error;
  };
  const getError=P.getError;P.getError=function(){const e=this.__pending;if(e){this.__pending=0;return e;}return getError.call(this);};
 };
 T.instanceReplayTools={run,select,captureGeometry};
 // Diagnostic (BFE_DIAG): redraw each record of an instances row alone with the recorded program and with the
 // owned one in this application, and report the draws whose pixels differ, with their rows. It finds which draw
 // of a failing pair the owned program draws differently without a second application.
 T.diagDraws=function(records,blobs){
  const found=[];
  for(const row of records){
   if(row.kind!=='instances' || row.view)continue;
   core.views();const stride=core.stride,recordsAt=core.recordsAt;
   const without=(skip)=>({...row,input:row.input.slice(0,recordsAt).concat(...row.records.map((_,i)=>i===skip?[]:row.input.slice(recordsAt+i*stride,recordsAt+(i+1)*stride))),records:row.records.filter((_,i)=>i!==skip)});
   // Draws that interact (overlapping surfaces whose depths tie to the last bit) only differ together: the
   // whole row, then the row without each draw in turn; a draw whose removal changes the count is part of it.
   // A draw that raises a GL error (the readback names the row, not the draw) is found by drawing each alone.
   for(let i=0;i<row.records.length;i++){
    const alone={...row,input:row.input.slice(0,recordsAt).concat(row.input.slice(recordsAt+i*stride,recordsAt+(i+1)*stride)),records:[row.records[i]]};
    try{run(alone,blobs,false);}catch(error){found.push({key:row.key,draw:i,variant:row.records[i].key,error:String(error.message),row:alone.input.slice(recordsAt),geometry:Object.fromEntries(Object.entries(row.records[i].geometry.attributes).map(([n,a])=>[n,{size:a.size,stride:a.stride,offset:a.offset,length:blobs[a.blob].length}])),index:blobs[row.records[i].geometry.index].length,attributes:row.records[i].attributes});}
   }
   if(found.length)continue;
   const whole=T.replayTools.rgbaMetrics(T.blobs[run(row,blobs,false).output.blob],T.blobs[run(row,blobs,true).output.blob]);
   if(whole.differing){
    const involved=[];
    for(let i=0;i<row.records.length;i++){
     const partial=without(i),recorded=run(partial,blobs,true),owned=run(partial,blobs,false);
     const m=T.replayTools.rgbaMetrics(T.blobs[owned.output.blob],T.blobs[recorded.output.blob]);
     release(owned.output.blob,blobs);release(recorded.output.blob,blobs);
     if(m.differing!==whole.differing)involved.push({draw:i,differing:m.differing});
    }
    found.push({key:row.key,whole:whole.differing,involved,draws:row.records.length,spheres:Array.from(row.records,(_,i)=>row.input.slice(recordsAt+i*stride+10,recordsAt+i*stride+14))});
   }
   for(let i=0;i<row.records.length;i++){
    const alone={...row,input:row.input.slice(0,recordsAt).concat(row.input.slice(recordsAt+i*stride,recordsAt+(i+1)*stride)),records:[row.records[i]]};
    const recorded=run(alone,blobs,true),owned=run(alone,blobs,false);
    const metrics=T.replayTools.rgbaMetrics(T.blobs[owned.output.blob],T.blobs[recorded.output.blob]);
    release(owned.output.blob,blobs);release(recorded.output.blob,blobs);
    if(metrics.differing)found.push({key:row.key,draw:i,variant:row.records[i].key,metrics,row:alone.input.slice(recordsAt),lamps:row.lamps.length});
   }
  }
  return found;
 };
 core.rust.frame=count=>capture(count,false,frame);
 core.rust.view_frame=count=>capture(count,true,view);
 core.rust.mirror_frame=count=>capture(count,false,mirror,true);
 T.replay=async function(records,blobs){
  const metrics=[];for(const row of records){if(row.kind!=='instances'){metrics.push(...await previous([row],blobs));continue;}
   const actual=run(row,blobs),a=T.blobs[actual.output.blob],e=blobs[row.output.blob];metrics.push({key:row.key,...T.replayTools.rgbaMetrics(a,e),draws:row.draws,covered:row.covered,variants:[...new Set(row.records.map(r=>r.key))]});release(actual.output.blob,blobs);
  }return metrics;
 };
}
