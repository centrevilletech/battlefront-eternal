// Capture shader inputs at the GL boundary of the default Rust phases. Programs
// still come from Rust registration; only the test runner replays submissions.
// Fresh multisampled targets isolate source arithmetic from unrelated draws and
// preserve alpha-to-coverage sample tests and the actual multi-draw ranges.
export function installWorldTrace() {
  const P=WebGL2RenderingContext.prototype;
  const T=window.__worldTrace={programs:new Map(),shaders:new WeakMap(),textures:new WeakMap()};
  const shaderSource=P.shaderSource,attach=P.attachShader;
  P.shaderSource=function(shader,source){T.shaders.set(shader,source);return shaderSource.call(this,shader,source);};
  P.attachShader=function(program,shader){let row=T.programs.get(program);if(!row)T.programs.set(program,row={});row[this.getShaderParameter(shader,this.SHADER_TYPE)===this.VERTEX_SHADER?'vertex':'fragment']=T.shaders.get(shader);return attach.call(this,program,shader);};
  for(const name of ['texImage2D','compressedTexImage2D','texStorage2D']){
    const fn=P[name];P[name]=function(...a){
      if(a[0]===this.TEXTURE_2D){
        const texture=this.getParameter(this.TEXTURE_BINDING_2D);let row=T.textures.get(texture);if(!row)T.textures.set(texture,row={levels:[]});
        if(name==='texStorage2D'){row.format=a[2];row.w=a[3];row.h=a[4];row.count=a[1];}
        else{
          const source=a[5],explicit=a.length>=9 || name==='compressedTexImage2D';
          const w=explicit?a[3]:(source.videoWidth||source.naturalWidth||source.width),h=explicit?a[4]:(source.videoHeight||source.naturalHeight||source.height);
          row.levels[a[1]]={w,h,format:a[2],type:name==='texImage2D'&&explicit?a[7]:this.UNSIGNED_BYTE,compressed:name==='compressedTexImage2D',data:name==='compressedTexImage2D'?a[6]:null};
          if(a[1]===0){row.w=w;row.h=h;row.format=a[2];}
        }
      }
      return fn.apply(this,a);
    };
  }
}

export function installWorldReplay(layouts) {
  const b=window.__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=window.__corpus,trace=window.__worldTrace;
  if(!trace)throw Error('World replay trace was not installed before loading');
  T.worldProgramSources=new Map();
  const features=new Map(),defineNames=new Set();
  for(const [key,p] of Object.entries(layouts))if(key.startsWith('world/')&&p.selector){features.set(JSON.stringify(p.selector),key);for(const [name] of p.selector.defines)defineNames.add(name);}
  const programKeys=new WeakMap(),referencePrograms=new Map();
  const keyOf=program=>{
    if(programKeys.has(program))return programKeys.get(program);
    const source=trace.programs.get(program);if(!source || !source.vertex?.includes('uniform BfeFrame'))return null;
    let key;
    if(source.vertex.includes('attribute vec3 leafCenter;'))key=/^#define USE_SHADOWMAP\s*$/m.test(source.vertex)?'world/canopies':'world/canopies-no-shadows';
    else if(source.vertex.includes('attribute vec2 foliageCorner;'))key='world/foliage-'+(source.vertex.includes('float sink =')?'grass':'leaves')+(/^#define USE_SHADOWMAP\s*$/m.test(source.vertex)?'-shadows':'');
    else if(/^#define (WORLD_SPACE|TERRAIN|FAR)[ \t]*$/m.test(source.vertex)&&!/^#define SKY[ \t]*$/m.test(source.vertex)){
      const values=new Map([...source.vertex.matchAll(/^#define[ \t]+(\w+)[ \t]*([^\n]*)/gm)].filter(m=>defineNames.has(m[1])).map(m=>[m[1],m[2].trim()]));
      const count=name=>Number(source.fragment.match(new RegExp('\\b'+name+'\\[[ \\t]*(\\d+)[ \\t]*\\]'))?.[1]||0);
      const selector={defines:[...values].sort(([a],[b])=>a<b?-1:a>b?1:0),side:/^#define DOUBLE_SIDED\s*$/m.test(source.vertex)?2:/^#define FLIP_SIDED\s*$/m.test(source.vertex)?1:0,shadows:count('cascadeMaps'),points:count('pointLights'),csm:source.fragment.includes('uniform vec2 CSM_cascades'),fabric:source.vertex.includes('fabricAnchor'),tree:source.fragment.includes('treeStates'),map:/^#define USE_MAP\s*$/m.test(source.vertex),lamps:source.fragment.includes('uniform highp sampler2D bfeLamps')};
      key=features.get(JSON.stringify(selector));if(!key)throw Error('Unowned replay world variant '+JSON.stringify(selector));
    }
    programKeys.set(program,key||null);if(key){referencePrograms.set(key,program);T.worldProgramSources.set(key,source);}return key||null;
  };
  const store=T.store;
  const buffer=(handle,cache)=>{
    if(cache.has(handle))return cache.get(handle);
    const old=gl.getParameter(gl.COPY_READ_BUFFER_BINDING);gl.bindBuffer(gl.COPY_READ_BUFFER,handle);
    const data=new Uint8Array(gl.getBufferParameter(gl.COPY_READ_BUFFER,gl.BUFFER_SIZE));gl.getBufferSubData(gl.COPY_READ_BUFFER,0,data);gl.bindBuffer(gl.COPY_READ_BUFFER,old);
    const blob=store(data);cache.set(handle,blob);return blob;
  };
  const fb=gl.createFramebuffer();
  const depthCopies=new Map();
  const postFrame=core.rust.post_frame.bind(core.rust);
  const targets=new Map();
  const targetFor=row=>{
    const key=[row.w,row.h,row.type,row.samples].join(':');let target=targets.get(key);
    if(!target){target=new b.THREE.WebGLRenderTarget(row.w,row.h,{type:row.type,samples:row.samples,depthBuffer:true});targets.set(key,target);R.initRenderTarget(target);}
    return target;
  };
  const read=target=>{
    const half=target.texture.type===1016,float=target.texture.type===1015,buf=half?new Uint16Array(target.width*target.height*4):float?new Float32Array(target.width*target.height*4):new Uint8Array(target.width*target.height*4),old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER,R.properties.get(target).__webglFramebuffer);gl.readPixels(0,0,target.width,target.height,gl.RGBA,half?gl.HALF_FLOAT:float?gl.FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
    const error=gl.getError();if(error)throw Error('World replay read GL '+error);return {blob:store(buf),w:target.width,h:target.height,bytes:half?2:float?4:1};
  };
  const textureSnapshot=handle=>{
    const meta=trace.textures.get(handle);if(!meta?.w||!meta?.h)throw Error('Missing world replay texture dimensions');
    const active=gl.getParameter(gl.ACTIVE_TEXTURE);gl.activeTexture(gl.TEXTURE0);const bound=gl.getParameter(gl.TEXTURE_BINDING_2D);gl.bindTexture(gl.TEXTURE_2D,handle);
    const params=[gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER,gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T,gl.TEXTURE_COMPARE_MODE,gl.TEXTURE_COMPARE_FUNC,gl.TEXTURE_BASE_LEVEL,gl.TEXTURE_MAX_LEVEL].map(p=>[p,gl.getTexParameter(gl.TEXTURE_2D,p)]);
    const aniso=gl.getExtension('EXT_texture_filter_anisotropic');if(aniso)params.push([aniso.TEXTURE_MAX_ANISOTROPY_EXT,gl.getTexParameter(gl.TEXTURE_2D,aniso.TEXTURE_MAX_ANISOTROPY_EXT)]);
    const count=params[0][1]>=9984&&params[0][1]<=9987?Math.floor(Math.log2(Math.max(meta.w,meta.h)))+1:1;
    const depth=[gl.DEPTH_COMPONENT,gl.DEPTH_COMPONENT16,gl.DEPTH_COMPONENT24,gl.DEPTH_COMPONENT32F].includes(meta.format),levels=[];
    if(depth){
      const before=R.getRenderTarget(),face=R.getActiveCubeFace(),mip=R.getActiveMipmapLevel(),key=meta.w+':'+meta.h;
      let target=depthCopies.get(key);
      if(!target){target=new b.THREE.WebGLRenderTarget(meta.w,meta.h,{type:b.THREE.FloatType,depthBuffer:false,minFilter:b.THREE.NearestFilter,magFilter:b.THREE.NearestFilter});R.initRenderTarget(target);depthCopies.set(key,target);}
      gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_COMPARE_MODE,gl.NONE);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);
      const id=core.rust.add_texture(handle),program=core.rust.add_owned_program('post-hud/copy'),output=core.byName.post.target(target);
      core.views();const saved=core.postInput.slice();core.postInput.fill(0);core.postInput.set([program,output,meta.w,meta.h,1,0,32,id]);
      try{postFrame(1);const captured=read(target),data=T.blobs[captured.blob],texels=new Float32Array(meta.w*meta.h);for(let i=0;i<texels.length;i++)texels[i]=data[4*i];delete T.blobs[captured.blob];return {w:meta.w,h:meta.h,params,levels:[store(texels)],depth:meta.format,type:gl.FLOAT};}
      finally{core.views();core.postInput.set(saved);gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);for(const [p,v] of params)gl.texParameteri(gl.TEXTURE_2D,p,v);gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);R.resetState();R.setRenderTarget(before,face,mip);}
    }
    const previous=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);
    let compressed=null,type=gl.UNSIGNED_BYTE;
    try{
      for(let level=0;level<count;level++){
        const m=meta.levels[level];if(m?.compressed){compressed=m.format;levels.push(store(m.data.slice()));continue;}
        const w=Math.max(1,meta.w>>level),h=Math.max(1,meta.h>>level);
        type=m?.type||([gl.RGBA32F].includes(meta.format)?gl.FLOAT:meta.format===gl.RGBA16F?gl.HALF_FLOAT:gl.UNSIGNED_BYTE);
        gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,handle,level);
        if(gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE)throw Error('World replay texture is not readable');
        const data=type===gl.FLOAT?new Float32Array(w*h*4):type===gl.HALF_FLOAT?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
        gl.readPixels(0,0,w,h,gl.RGBA,type,data);levels.push(store(data));
      }
    }finally{gl.bindFramebuffer(gl.READ_FRAMEBUFFER,previous);gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);}
    const error=gl.getError();if(error)throw Error('World replay texture GL '+error);
    return {w:meta.w,h:meta.h,params,levels,type,compressed,srgb:meta.format===gl.SRGB8_ALPHA8||meta.format===gl.SRGB8};
  };
  const uniformValues=(program,u)=>{
    const out=[];
    for(let i=0;i<(u.size||1);i++){
      const name=(u.size||1)>1?u.name.replace('[0]','['+i+']'):u.name,location=gl.getUniformLocation(program,name);
      if(location===null)continue;
      const value=gl.getUniform(program,location);if(ArrayBuffer.isView(value))out.push(...value);else out.push(value);
    }
    return out;
  };
  let capture=null;
  const captureDraw=(name,args)=>{
    if(!capture)return;
    const program=gl.getParameter(gl.CURRENT_PROGRAM),key=keyOf(program);if(!key)return;
    const attrs=[],uniforms=[],blocks=[],samplers=[];
    for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_ATTRIBUTES);i++){
      const a=gl.getActiveAttrib(program,i),location=gl.getAttribLocation(program,a.name),handle=gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING);
      if(!handle)throw Error('World replay requires explicit vertex data '+a.name);
      attrs.push({location,blob:buffer(handle,capture.buffers),size:gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_SIZE),type:gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_TYPE),normalized:gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_NORMALIZED),stride:gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_STRIDE),offset:gl.getVertexAttribOffset(location,gl.VERTEX_ATTRIB_ARRAY_POINTER),divisor:gl.getVertexAttrib(location,gl.VERTEX_ATTRIB_ARRAY_DIVISOR)});
    }
    for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_UNIFORM_BLOCKS);i++){
      const binding=gl.getActiveUniformBlockParameter(program,i,gl.UNIFORM_BLOCK_BINDING),handle=gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING,binding);
      blocks.push({binding,blob:buffer(handle,capture.buffers)});
    }
    const active=gl.getParameter(gl.ACTIVE_TEXTURE);
    for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_UNIFORMS);i++){
      const u=gl.getActiveUniform(program,i),location=gl.getUniformLocation(program,u.name);if(location===null)continue;
      const v=uniformValues(program,u);uniforms.push({name:u.name,type:u.type,size:u.size,value:store([gl.INT,gl.BOOL,gl.SAMPLER_2D,gl.SAMPLER_2D_SHADOW].includes(u.type)?Int32Array.from(v):Float32Array.from(v))});
      if(u.type===gl.SAMPLER_2D||u.type===gl.SAMPLER_2D_SHADOW)for(const unit of v){gl.activeTexture(gl.TEXTURE0+unit);const handle=gl.getParameter(gl.TEXTURE_BINDING_2D);if(!handle)throw Error('World replay sampler is unbound '+u.name);let id=capture.textureIds.get(handle);if(id===undefined){id=capture.textures.length;capture.textures.push(handle);capture.textureIds.set(handle,id);}samplers.push([unit,id]);}
    }
    gl.activeTexture(active);
    const element=gl.getParameter(gl.ELEMENT_ARRAY_BUFFER_BINDING);
    capture.draws.push({key,attrs,uniforms,blocks,samplers,index:element?buffer(element,capture.buffers):null,name,args:args.map(a=>ArrayBuffer.isView(a)?{blob:store(a.slice()),type:a.constructor.name}:a),
      caps:[gl.DEPTH_TEST,gl.CULL_FACE,gl.BLEND,gl.SAMPLE_ALPHA_TO_COVERAGE,gl.SCISSOR_TEST,gl.POLYGON_OFFSET_FILL,gl.STENCIL_TEST,gl.SAMPLE_COVERAGE].map(cap=>[cap,gl.isEnabled(cap)]),depthRange:Array.from(gl.getParameter(gl.DEPTH_RANGE)),polygon:[gl.getParameter(gl.POLYGON_OFFSET_FACTOR),gl.getParameter(gl.POLYGON_OFFSET_UNITS)],coverage:[gl.getParameter(gl.SAMPLE_COVERAGE_VALUE),gl.getParameter(gl.SAMPLE_COVERAGE_INVERT)],depthFunc:gl.getParameter(gl.DEPTH_FUNC),depthMask:gl.getParameter(gl.DEPTH_WRITEMASK),colorMask:gl.getParameter(gl.COLOR_WRITEMASK),frontFace:gl.getParameter(gl.FRONT_FACE),cullFace:gl.getParameter(gl.CULL_FACE_MODE),blend:[gl.getParameter(gl.BLEND_EQUATION_RGB),gl.getParameter(gl.BLEND_EQUATION_ALPHA),gl.getParameter(gl.BLEND_SRC_RGB),gl.getParameter(gl.BLEND_DST_RGB),gl.getParameter(gl.BLEND_SRC_ALPHA),gl.getParameter(gl.BLEND_DST_ALPHA)],viewport:Array.from(gl.getParameter(gl.VIEWPORT)),scissor:Array.from(gl.getParameter(gl.SCISSOR_BOX))});
  };
  const methods=['drawElements','drawArrays','drawElementsInstanced','drawArraysInstanced','drawRangeElements'];
  for(const name of methods){const fn=gl[name];gl[name]=function(...args){captureDraw(name,args);return fn.apply(this,args);};}
  const multi=gl.getExtension('WEBGL_multi_draw');
  if(multi)for(const name of ['multiDrawElementsWEBGL','multiDrawArraysWEBGL','multiDrawElementsInstancedWEBGL','multiDrawArraysInstancedWEBGL'])if(multi[name]){const fn=multi[name];multi[name]=function(...args){captureDraw(name,args);return fn.apply(this,args);};}
  const uploadTexture=(s,blobs)=>{
    const texture=gl.createTexture();gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,texture);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);
    for(const [p,v] of s.params)gl.texParameteri(gl.TEXTURE_2D,p,v);
    if(s.depth){
      const source=blobs[s.levels[0]];
      if(s.depth===gl.DEPTH_COMPONENT32F)gl.texImage2D(gl.TEXTURE_2D,0,s.depth,s.w,s.h,0,gl.DEPTH_COMPONENT,gl.FLOAT,source);
      else if(s.depth===gl.DEPTH_COMPONENT24){const data=Uint32Array.from(source,v=>Math.round(Math.round(v*16777215)/16777215*4294967295));gl.texImage2D(gl.TEXTURE_2D,0,s.depth,s.w,s.h,0,gl.DEPTH_COMPONENT,gl.UNSIGNED_INT,data);}
      else if(s.depth===gl.DEPTH_COMPONENT16)gl.texImage2D(gl.TEXTURE_2D,0,s.depth,s.w,s.h,0,gl.DEPTH_COMPONENT,gl.UNSIGNED_SHORT,Uint16Array.from(source,v=>Math.round(v*65535)));
      else throw Error('Unsupported depth replay format '+s.depth);
      return texture;
    }
    for(let level=0;level<s.levels.length;level++)if(s.compressed)gl.compressedTexImage2D(gl.TEXTURE_2D,level,s.compressed,Math.max(1,s.w>>level),Math.max(1,s.h>>level),0,blobs[s.levels[level]]);else gl.texImage2D(gl.TEXTURE_2D,level,s.type===gl.FLOAT?gl.RGBA32F:s.type===gl.HALF_FLOAT?gl.RGBA16F:s.srgb?gl.SRGB8_ALPHA8:gl.RGBA8,Math.max(1,s.w>>level),Math.max(1,s.h>>level),0,gl.RGBA,s.type,blobs[s.levels[level]]);
    return texture;
  };
  const setUniform=(program,u,blobs)=>{const location=gl.getUniformLocation(program,u.name),v=Array.isArray(u.value)?u.value:blobs[u.value];if(location===null)return;switch(u.type){case gl.FLOAT:gl.uniform1fv(location,v);break;case gl.FLOAT_VEC2:gl.uniform2fv(location,v);break;case gl.FLOAT_VEC3:gl.uniform3fv(location,v);break;case gl.FLOAT_VEC4:gl.uniform4fv(location,v);break;case gl.FLOAT_MAT3:gl.uniformMatrix3fv(location,false,v);break;case gl.FLOAT_MAT4:gl.uniformMatrix4fv(location,false,v);break;case gl.INT:case gl.BOOL:case gl.SAMPLER_2D:case gl.SAMPLER_2D_SHADOW:gl.uniform1iv(location,v);break;default:throw Error('Unsupported world replay uniform '+u.name);}};
  const drawRows=(row,blobs,owned)=>{
    const before=R.getRenderTarget(),face=R.getActiveCubeFace(),mip=R.getActiveMipmapLevel(),target=targetFor(row);R.resetState();R.setRenderTarget(target);gl.disable(gl.SCISSOR_TEST);gl.colorMask(true,true,true,true);gl.depthMask(true);gl.clearColor(0,0,0,0);gl.clearDepth(1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
    const buffers=new Map(),textures=row.textures.map(s=>uploadTexture(s,blobs)),vao=gl.createVertexArray();gl.bindVertexArray(vao);
    const uploaded=(key,target=gl.ARRAY_BUFFER)=>{let buffer=buffers.get(key);if(!buffer){buffer=gl.createBuffer();gl.bindBuffer(target,buffer);gl.bufferData(target,blobs[key],gl.STATIC_DRAW);buffers.set(key,buffer);}return buffer;};
    const enabled=new Set(),uniformState=new Map();
    try{
      for(const d of row.draws){
        let program=referencePrograms.get(d.key);
        if(owned){core.rust.add_owned_program(d.key);program=[...trace.programs.keys()].find(p=>keyOf(p)===d.key);}
        if(!program)program=[...trace.programs.keys()].find(p=>keyOf(p)===d.key);
        if(!program)throw Error('Missing Rust replay program '+d.key);
        gl.useProgram(program);
        const locations=new Set(d.attrs.map(a=>a.location));for(const location of enabled)if(!locations.has(location)){gl.disableVertexAttribArray(location);enabled.delete(location);}
        for(const a of d.attrs){gl.bindBuffer(gl.ARRAY_BUFFER,uploaded(a.blob));gl.enableVertexAttribArray(a.location);enabled.add(a.location);gl.vertexAttribPointer(a.location,a.size,a.type,a.normalized,a.stride,a.offset);gl.vertexAttribDivisor(a.location,a.divisor);}
        if(d.index)gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,uploaded(d.index,gl.ELEMENT_ARRAY_BUFFER));
        for(const block of d.blocks)gl.bindBufferBase(gl.UNIFORM_BUFFER,block.binding,uploaded(block.blob));
        if(!uniformState.has(program))uniformState.set(program,d.uniforms.map(u=>({...u,value:uniformValues(program,u)})));
        for(const u of d.uniforms)setUniform(program,u,blobs);
        for(const [unit,id] of d.samplers){gl.activeTexture(gl.TEXTURE0+unit);gl.bindTexture(gl.TEXTURE_2D,textures[id]);}
        for(const [cap,on] of d.caps)on?gl.enable(cap):gl.disable(cap);gl.depthRange(...d.depthRange);gl.polygonOffset(...d.polygon);gl.sampleCoverage(...d.coverage);gl.depthFunc(d.depthFunc);gl.depthMask(d.depthMask);gl.colorMask(...d.colorMask);gl.frontFace(d.frontFace);gl.cullFace(d.cullFace);gl.blendEquationSeparate(d.blend[0],d.blend[1]);gl.blendFuncSeparate(...d.blend.slice(2));gl.viewport(...d.viewport);gl.scissor(...d.scissor);
        const args=d.args.map(a=>a&&a.blob!==undefined?blobs[a.blob]:a);
        if(d.name.startsWith('multiDraw')){if(!multi)throw Error('World replay requires multi-draw');multi[d.name](...args);}else gl[d.name](...args);
        const error=gl.getError();if(error)throw Error(`World replay ${d.name} ${d.key}: GL ${error}`);
      }
      core.resolve(target);return read(target);
    }finally{for(const [program,uniforms] of uniformState){gl.useProgram(program);for(const u of uniforms)if(u.value.length)setUniform(program,u);}gl.bindVertexArray(null);gl.deleteVertexArray(vao);for(const buffer of buffers.values())gl.deleteBuffer(buffer);for(const texture of textures)gl.deleteTexture(texture);R.resetState();R.setRenderTarget(before,face,mip);}
  };
  const replay=(row,blobs,owned)=>{
    const actual=drawRows(row,blobs,owned),raw=T.blobs[actual.blob],expected=blobs[row.output.blob],a=raw instanceof Float32Array?new Uint32Array(raw.buffer,raw.byteOffset,raw.length):raw,e=expected instanceof Float32Array?new Uint32Array(expected.buffer,expected.byteOffset,expected.length):expected;let differing=0,maxDelta=0;
    for(let i=0;i<a.length;i+=4){let d=0;for(let j=0;j<4;j++)d=Math.max(d,Math.abs(a[i+j]-e[i+j]));if(d){differing++;maxDelta=Math.max(maxDelta,d);}}
    delete T.blobs[actual.blob];return {key:row.key,differing,maxDelta,pixels:a.length/4,samples:row.samples,programs:[...new Set(row.draws.map(d=>d.key))],draws:row.draws.length,multiDraws:row.draws.filter(d=>d.name.startsWith('multiDraw')).length,alphaToCoverage:row.draws.filter(d=>d.caps.some(([cap,on])=>cap===gl.SAMPLE_ALPHA_TO_COVERAGE&&on)).length};
  };
  for(const [method,key,target] of [['frame','world/near',()=>R.getRenderTarget()||g.sceneTarget],['far_frame','world/far',()=>R.getRenderTarget()||g.farTarget],['mirror_frame','world/mirror',()=>R.getRenderTarget()],['far_mirror_frame','world/far-mirror',()=>R.getRenderTarget()],['far_probe_frame','world/probe',()=>R.getRenderTarget()]]){
    const original=core.rust[method].bind(core.rust);core.rust[method]=function(...args){
      if(!T.want||!T.captureReplay)return original(...args);
      capture={draws:[],textures:[],textureIds:new Map(),buffers:new Map()};let result,data;
      try{result=original(...args);data=capture;}finally{capture=null;}
      if(!data.draws.length)return result;
      const bound=R.getRenderTarget(),face=R.getActiveCubeFace(),mip=R.getActiveMipmapLevel(),source=target(),row={kind:'world',key,w:source.width,h:source.height,type:source.texture.type,samples:source.samples,draws:data.draws,textures:data.textures.map(textureSnapshot),resources:[]};
      row.output=drawRows(row,T.blobs,false);T.records.push(row);R.resetState();R.setRenderTarget(bound,face,mip);return result;
    };
  }
  const previousReplay=T.replay;
  T.replay=(records,blobs)=>[...previousReplay(records.filter(r=>r.kind!=='world'),blobs),...records.filter(r=>r.kind==='world').map(row=>replay(row,blobs,T.role==='candidate'))];
}
