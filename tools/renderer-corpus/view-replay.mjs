// Frozen view draws that formerly used individual uniforms: unlit sights and
// muzzle flashes. Candidate unlit draws use rigid rows; flash draws stay Plain.
export function installViewReplay(reference) {
 const b=__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=__corpus;
 const {store,release,snapshot,textures,upload,read}=T.replayTools;
 const {select,captureGeometry,run:instanceRun}=T.instanceReplayTools;
 const viewFrame=core.rust.view_frame.bind(core.rust),previous=T.replay;
 // View parts are drawn in their own scene with the view camera's frame block, so they replay
 // apart from the instance rows (the draw record is view.rs's, much larger than a caster's).
 const DRAW=593,UNIFORM=18;
 const run=(row,blobs)=>{
  if(!reference && row.actor){
   const r=row.actor,input=new Array(core.recordsAt+core.stride).fill(0);input.splice(0,core.recordsAt,...row.frame);
   const at=core.recordsAt;input[at]=2;input[at+3]=0;input[at+4]=r.map;
   input.splice(at+10,4,...r.sphere);input.splice(at+14,6,...r.box);input.splice(at+20,16,...r.world);input.splice(at+52,4,...r.look);input[at+56]=r.flags;input[at+76]=r.scroll[0];input[at+77]=r.scroll[1];
   return instanceRun({view:true,width:row.width,height:row.height,bytes:row.bytes,samples:row.samples,input,records:[{key:row.key,kind:2,geometry:row.geometry,attributes:row.attributes}],resources:row.resources,cascades:[],far:r.far,lamps:[],shadow:new Array(core.rust.shadow_input_len()).fill(0),bones:r.bones},blobs);
  }
  const before=R.getRenderTarget(),clear=R.getClearColor(new b.THREE.Color()),alpha=R.getClearAlpha(),fresh=new core.rust.constructor(gl),memory=()=>core.wasm.memory.buffer;
  const created=row.resources.map(s=>[s.id,upload(s,blobs)]),ids=new Map(created.map(([id,s])=>[id,T.replayTools.register(fresh,s)]));
  const target=new b.THREE.WebGLRenderTarget(row.width,row.height,{type:row.bytes===2?b.THREE.HalfFloatType:row.bytes===4?b.THREE.FloatType:b.THREE.UnsignedByteType,samples:row.samples});R.initRenderTarget(target);
  try {
   const layout=reference?{attributes:row.attributes,uniforms:row.uniforms}:T.actorLayouts[row.key];
   const program=reference?fresh.add_program(3,row.vertex,row.fragment,row.attributes.map(([n,l])=>n+':'+l).join(','),row.uniforms.map(u=>u.name).join(',')):fresh.add_owned_program(row.key);
   const geo=new b.THREE.BufferGeometry();for(const [name,a] of Object.entries(row.geometry.attributes)){const data=blobs[a.blob];geo.setAttribute(name,a.stride?new b.THREE.InterleavedBufferAttribute(new b.THREE.InterleavedBuffer(data,a.stride),a.size,a.offset,a.normalized):new b.THREE.BufferAttribute(data,a.size,a.normalized));}geo.setIndex(new b.THREE.BufferAttribute(blobs[row.geometry.index],1));
   const uploader=Object.create(core);uploader.rust=fresh;uploader.geometries=new Map();uploader.errors=[];
   const geometry=uploader.geometry(geo,{id:program,attributes:layout.attributes});if(geometry==null)throw Error('View replay geometry');
   const input=new Float64Array(1+DRAW);input[0]=1;input.set(row.draw,1);input[1]=program;input[2]=geometry;
   for(let j=0;j<layout.uniforms.length;j++){const k=row.uniforms.findIndex(u=>u.name===layout.uniforms[j].name);if(k<0)throw Error('Missing view uniform');input.set(row.draw.slice(17+k*UNIFORM,17+(k+1)*UNIFORM),18+j*UNIFORM);input[18+j*UNIFORM]=j;if(input[19+j*UNIFORM]===32 && input[20+j*UNIFORM]>=0)input[20+j*UNIFORM]=ids.get(input[20+j*UNIFORM]);}
   input[17]=layout.uniforms.length;
   new Float64Array(memory(),fresh.input_ptr(),fresh.input_len()).set(row.frame);
   new Float64Array(memory(),fresh.view_input_ptr(),fresh.view_input_len()).set(input);
   R.resetState();R.setRenderTarget(target);R.setClearColor(0x204060,.625);R.clear(true,true,true);core.resolve(target);
   const clearPixel=row.bytes===2?new Uint16Array(4):row.bytes===4?new Float32Array(4):new Uint8Array(4),oldRead=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);
   gl.bindFramebuffer(gl.READ_FRAMEBUFFER,(R.resources?.gpu?R.resources.gpu.framebuffer(R.resources.targetId(target),false,0):R.properties.get(target).__webglFramebuffer));gl.readPixels(0,0,1,1,gl.RGBA,row.bytes===2?gl.HALF_FLOAT:row.bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,clearPixel);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,oldRead);
   const stats=fresh.view_frame(0);core.resolve(target);const output=read(target),pixels=T.blobs[output.blob];let covered=0;
   for(let i=0;i<pixels.length;i+=4)if(pixels[i]!==clearPixel[0] || pixels[i+1]!==clearPixel[1] || pixels[i+2]!==clearPixel[2] || pixels[i+3]!==clearPixel[3])covered++;
   return {output,stats,covered};
  }finally{fresh.free();for(const [,s] of created)s.dispose();target.dispose();core.views();R.setClearColor(clear,alpha);R.resetState();R.setRenderTarget(before);}
 };
 core.rust.view_frame=function(count){
  if(T.want && T.captureReplay && core.viewInput){
   core.views();const saved=core.viewInput.slice(),frame=Array.from(core.input.subarray(0,core.recordsAt));
   for(let i=0;i<saved[0];i++){
    const draw=Array.from(saved.subarray(1+i*DRAW,1+(i+1)*DRAW));if(draw[0]<0)continue;
    const record=core.byName.view.records.find(r=>r.program?.id===draw[0] && r.geometry===draw[1]);if(!record)throw Error('Missing frozen view draw');
    const material=record.material,object=record.mesh,program=R.properties.get(material).currentProgram,shaders=gl.getAttachedShaders(program.program),source=stage=>gl.getShaderSource(shaders.find(s=>gl.getShaderParameter(s,gl.SHADER_TYPE)===stage));
    const ids=[];for(let j=0;j<draw[16];j++){const at=17+j*UNIFORM;if(draw[at+1]===32 && draw[at+2]>=0)ids.push(draw[at+2]);}
    const objects=textures([material.map,...Object.values(material.uniforms || {}).map(u=>u.value)]),resources=[...new Set(ids)].map(id=>snapshot(id,objects));
    const actor=material.isShaderMaterial,key=actor?select(material,2,false,0):'effects/view-glow-basic-s0';
    const row={kind:'view-uniforms',key,width:g.sceneTarget.width,height:g.sceneTarget.height,bytes:g.sceneTarget.texture.type===1016?2:g.sceneTarget.texture.type===1015?4:1,samples:g.sceneTarget.samples,frame,draw,resources,uniforms:record.program.uniforms,attributes:record.program.attributes,geometry:captureGeometry(object.geometry,record.program.attributes),vertex:source(gl.VERTEX_SHADER),fragment:source(gl.FRAGMENT_SHADER)};
    if(actor){if(!object.geometry.boundingSphere)object.geometry.computeBoundingSphere();if(!object.geometry.boundingBox)object.geometry.computeBoundingBox();const s=object.geometry.boundingSphere,box=object.geometry.boundingBox,u=material.uniforms;
     const textureId=name=>{const k=row.uniforms.findIndex(v=>v.name===name);return k<0?-1:draw[17+k*UNIFORM+2];};
     row.actor={world:Array.from(object.matrixWorld.elements),sphere:[...s.center.toArray(),s.radius],box:[...box.min.toArray(),...box.max.toArray()],map:textureId('map'),far:textureId('farMap'),look:[u.sunScale.value,u.ambientPull.value.x,u.ambientPull.value.y,u.brightness.value],flags:material.side|(material.alphaToCoverage?4:0)|(material.depthWrite?8:0),scroll:u.scroll.value.toArray(),bones:store(new Float32Array())};
    }
    const expected=run(row,T.blobs);row.output=expected.output;row.covered=expected.covered;row.draws=expected.stats.slice(5).reduce((a,b)=>a+b,0);T.records.push(row);
   }
   core.views();core.viewInput.set(saved);core.input.set(frame);
  }
  return viewFrame(count);
 };
 T.replay=async function(records,blobs){const metrics=[];for(const row of records){if(row.kind!=='view-uniforms'){metrics.push(...await previous([row],blobs));continue;}
  const actual=run(row,blobs),a=T.blobs[actual.output.blob],e=blobs[row.output.blob];metrics.push({key:row.key,...T.replayTools.rgbaMetrics(a,e),covered:row.covered,draws:row.draws});release(actual.output.blob,blobs);
 }return metrics;};
}
