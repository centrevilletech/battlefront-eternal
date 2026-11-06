// GPU-input fixtures for individual effects draws. Install tracking before the
// application creates its context; no game sources or resources are bundled.
// Installed before the page loads. Wraps the WebGL2 entry points so every linked program, texture and draw of a battle is recorded with the exact
  // sources the browser linked; replay later rebuilds each draw from those records alone. Nothing here changes what the page draws.
export function installGLTracking(index) {
  const P=WebGL2RenderingContext.prototype,contexts=new WeakMap(),original={};
  const state=gl=>{let s=contexts.get(gl);if(!s){s={textures:new WeakMap(),renderbuffers:new WeakMap(),programs:new WeakMap(),seenPrograms:new Map(),multiDraw:new WeakSet(),busy:false};contexts.set(gl,s);}return s;};
  const fp=source=>{let h=2166136261;for(let i=0;i<source.length;i++)h=Math.imul(h^source.charCodeAt(i),16777619);return (h>>>0).toString(16)+':'+source.length;};
  const binding=(gl,target)=>target===gl.TEXTURE_3D?gl.TEXTURE_BINDING_3D:target===gl.TEXTURE_2D_ARRAY?gl.TEXTURE_BINDING_2D_ARRAY:target===gl.TEXTURE_CUBE_MAP || target>=34069&&target<=34074?gl.TEXTURE_BINDING_CUBE_MAP:gl.TEXTURE_BINDING_2D;
  const texture=(gl,target)=>{const s=state(gl),t=gl.getParameter(binding(gl,target));let r=s.textures.get(t);if(!r){r={levels:[],generated:false};s.textures.set(t,r);}return r;};
  const wrap=(name,fn)=>{original[name]=P[name];P[name]=function(...args){return fn.call(this,original[name],args);};};
  for(const name of ['texImage2D','texImage3D','compressedTexImage2D','compressedTexImage3D'])wrap(name,function(call,a){
    const result=call.apply(this,a),r=texture(this,a[0]),compressed=name.startsWith('compressed'),three=name.endsWith('3D');let width,height,depth=1,format,type,data;
    if(compressed){width=a[3];height=a[4];if(three)depth=a[5];data=a[three?7:6];}
    else if(three){width=a[3];height=a[4];depth=a[5];format=a[7];type=a[8];data=a[9];}
    else if(a.length>=9){width=a[3];height=a[4];format=a[6];type=a[7];data=a[8];}
    else{width=a[5].width;height=a[5].height;format=a[3];type=a[4];data=a[5];}
    const row={target:a[0],level:a[1],internal:a[2],width,height,depth,format,type,compressed};
    if(compressed&&ArrayBuffer.isView(data))row.data=new Uint8Array(data.buffer,data.byteOffset,data.byteLength).slice();
    r.levels=r.levels.filter(x=>x.target!==row.target||x.level!==row.level);r.levels.push(row);return result;
  });
  for(const name of ['texStorage2D','texStorage3D'])wrap(name,function(call,a){const result=call.apply(this,a),r=texture(this,a[0]);for(let i=0;i<a[1];i++)for(const target of a[0]===this.TEXTURE_CUBE_MAP?[34069,34070,34071,34072,34073,34074]:[a[0]])r.levels.push({target,level:i,internal:a[2],width:Math.max(1,a[3]>>i),height:Math.max(1,a[4]>>i),depth:name==='texStorage3D'?Math.max(1,a[5]>>i):1});return result;});
  wrap('generateMipmap',function(call,a){const r=texture(this,a[0]);r.generated=true;r.levels=r.levels.filter(x=>x.level===0);for(const base of r.levels.filter(x=>x.level===0)){for(let i=1;i<=Math.floor(Math.log2(Math.max(base.width,base.height,base.depth)));i++)if(!r.levels.some(x=>x.target===base.target&&x.level===i))r.levels.push({...base,level:i,width:Math.max(1,base.width>>i),height:Math.max(1,base.height>>i),depth:Math.max(1,base.depth>>i),data:null});}return call.apply(this,a);});
  for(const name of ['renderbufferStorage','renderbufferStorageMultisample'])wrap(name,function(call,a){const result=call.apply(this,a);state(this).renderbuffers.set(this.getParameter(this.RENDERBUFFER_BINDING),{samples:name.endsWith('Multisample')?a[1]:0,internal:a[name.endsWith('Multisample')?2:1],width:a[a.length-2],height:a[a.length-1]});return result;});
  // A program's uniform value as a flat array, one element per array slot, so replay can set it back without knowing the type.
  const uniformValue=(gl,program,info)=>{
    const first=gl.getUniform(program,gl.getUniformLocation(program,info.name));
    if(info.size===1)return first;
    const width=ArrayBuffer.isView(first)?first.length:1,Type=ArrayBuffer.isView(first)?first.constructor:info.type===gl.FLOAT?Float32Array:Int32Array;
    const values=new Type(width*info.size);
    for(let element=0;element<info.size;element++){
      const name=info.name.replace('[0]','['+element+']');
      const value=element?gl.getUniform(program,gl.getUniformLocation(program,name)):first;
      if(ArrayBuffer.isView(value))values.set(value,element*width);else values[element*width]=value;
    }
    return values;
  };
  // Links a program to a catalog entry by source fingerprint and attribute names. A browser-compiled stage and its owned counterpart are matched here;
  // a program with alternatives but no declared match is recorded as a gap and makes the capture invalid.
  const program=(gl,p)=>{const s=state(gl);if(s.programs.has(p))return s.programs.get(p);const shaders=gl.getAttachedShaders(p),v=gl.getShaderSource(shaders.find(x=>gl.getShaderParameter(x,gl.SHADER_TYPE)===gl.VERTEX_SHADER)),f=gl.getShaderSource(shaders.find(x=>gl.getShaderParameter(x,gl.SHADER_TYPE)===gl.FRAGMENT_SHADER)),hash=fp(v)+'/'+fp(f);const attrs=[];for(let i=0;i<gl.getProgramParameter(p,gl.ACTIVE_ATTRIBUTES);i++){const a=gl.getActiveAttrib(p,i);const at=gl.getAttribLocation(p,a.name);if(at>=0)attrs.push([a.name,at]);}const choices=index.filter(x=>x.fingerprint===hash&&x.declared&&attrs.every(([name,location])=>x.attributes.some(([n,l])=>n===name&&l===location)));let choice=choices[0],transport,owned;
    // Plain stages can be identical while their link-time locations differ.
    // Keep the recorded pointers and rebind them by name during native replay.
    if(!choice){choice=index.find(x=>x.fingerprint===hash&&x.declared&&attrs.every(([name])=>x.attributes.some(([n])=>n===name)));if(choice){transport='Attributes';owned=choice;}}
    // The frozen browser's first draw can precede its bridge adoption. World
    // keeps the same object matrices and moves shared camera values to a block.
    // Effect likewise reads the camera from the frame block. Motion and Skinned move object matrices to their row. Match exact expanded
    // sources and copy recorded values; neither transport recomputes matrices.
    if(!choice)for(const raw of index.filter(x=>x.fingerprint===hash&&x.kind==='Plain')){
      const world=index.find(x=>x.key===raw.key&&x.declared&&(['World','Effect'].includes(x.kind)||['Motion','Skinned'].includes(x.kind)&&x.instanceLayout)&&attrs.every(([name])=>x.attributes.some(([n])=>n===name)));
      if(world){choice=world;transport=world.kind;owned=world;break;}
    }
    const row={key:choice?.key,transport,owned,vertex:v,fragment:f,attributes:attrs,alternatives:index.filter(x=>x.fingerprint===hash).map(x=>x.key+':'+x.kind)};s.programs.set(p,row);s.seenPrograms.set(hash+JSON.stringify(attrs),row);return row;};
  wrap('linkProgram',function(call,a){const result=call.apply(this,a);if(this.getProgramParameter(a[0],this.LINK_STATUS)){
    const row=program(this,a[0]);row.uniforms=[];row.layout=[];
    for(let i=0;i<this.getProgramParameter(a[0],this.ACTIVE_UNIFORMS);i++){const u=this.getActiveUniform(a[0],i);row.uniforms.push({name:u.name,type:u.type,size:u.size});}
    for(let i=0;i<this.getProgramParameter(a[0],this.ACTIVE_ATTRIBUTES);i++){const x=this.getActiveAttrib(a[0],i);row.layout.push({name:x.name,type:x.type,size:x.size,location:this.getAttribLocation(a[0],x.name)});}
  }return result;});
  wrap('getExtension',function(call,a){
    const ext=call.apply(this,a),gl=this,s=state(gl);
    if(a[0]==='WEBGL_multi_draw'&&ext&&!s.multiDraw.has(ext)){
      s.multiDraw.add(ext);
      for(const name of ['multiDrawArraysWEBGL','multiDrawElementsWEBGL','multiDrawArraysInstancedWEBGL','multiDrawElementsInstancedWEBGL']){
        if(typeof ext[name]!=='function')continue;
        const draw=ext[name];ext[name]=function(...args){
          if(!s.busy&&window.__corpus?.want&&s.capture){
            s.busy=true;
            try{
              const source=program(gl,gl.getParameter(gl.CURRENT_PROGRAM));
              if(source.key&&/\bgl_DrawID(?:ARB)?\b/.test(source.vertex+source.fragment))throw Error('Effects multi-draw uses draw IDs: '+source.key);
              const elements=name.includes('Elements'),instanced=name.includes('Instanced'),length=args[args.length-1];
              for(let i=0;i<length;i++){
                let command,values;
                if(elements){command=instanced?'drawElementsInstanced':'drawElements';values=[args[0],args[1][args[2]+i],args[3],args[4][args[5]+i]];if(instanced)values.push(args[6][args[7]+i]);}
                else{command=instanced?'drawArraysInstanced':'drawArrays';values=[args[0],args[1][args[2]+i],args[3][args[4]+i]];if(instanced)values.push(args[5][args[6]+i]);}
                s.capture(command,values,source);
              }
            }finally{s.busy=false;}
          }
          return draw.apply(this,args);
        };
      }
    }
    return ext;
  });
  for(const name of ['drawArrays','drawElements','drawArraysInstanced','drawElementsInstanced','drawRangeElements'])wrap(name,function(call,a){const s=state(this);if(!s.busy&&window.__corpus?.want&&s.capture){s.busy=true;try{s.capture(name,a,program(this,this.getParameter(this.CURRENT_PROGRAM)));}finally{s.busy=false;}}return call.apply(this,a);});
  // Reads the per-material values a rewritten source loads from its instance row, so replay can supply them from the recorded draw.
  const instanceExtras=source=>{
    const fields={};
    for(const match of source.matchAll(/^\s*(\w+) = bfeTexel\((\d+)\)(?:\.([xyzw]+))?(?: > 0\.5)?;$/gm)){
      const parts=match[3] || 'xyzw',component='xyzw'.indexOf(parts[0]);
      if(parts!=='xyzw'.slice(component,component+parts.length))throw Error('Noncontiguous native instance field '+match[1]);
      fields[match[1]]={at:Number(match[2])*4+component,columns:1,rows:parts.length};
    }
    return fields;
  };
  window.__glTracking={state,original,binding,uniformValue,instanceExtras};
}

// Each fixture uses captured GPU inputs and material state over a fixed color
// and depth. Live-reference output and serialized-input self replay must agree
// before candidate or repeat-reference results can count as a gate.
// The replay half: captures each draw (state, buffers, textures, uniforms) and later reissues it through the owned program in a fresh context.
export function installGLReplay() {
  const b=__bf,core=b.graphics.rust,gl=b.graphics.renderer.getContext(),T=window.__corpus;
  const track=window.__glTracking,s=track.state(gl),native=track.original;
  const get=p=>gl.getParameter(p),store=data=>{const key='gl'+Object.keys(T.blobs).length;T.blobs[key]=data;return key;};
  const buffers=new Map(),programs=new Map(),seen=new Set(),framebuffers=new WeakMap();let nextFramebuffer=1;const scratch=gl.createFramebuffer();
  const caps=[gl.BLEND,gl.CULL_FACE,gl.DEPTH_TEST,gl.STENCIL_TEST,gl.SCISSOR_TEST,gl.POLYGON_OFFSET_FILL,gl.SAMPLE_ALPHA_TO_COVERAGE,gl.SAMPLE_COVERAGE,gl.DITHER,gl.RASTERIZER_DISCARD];
  const params=[gl.VIEWPORT,gl.SCISSOR_BOX,gl.BLEND_EQUATION_RGB,gl.BLEND_EQUATION_ALPHA,gl.BLEND_SRC_RGB,gl.BLEND_DST_RGB,gl.BLEND_SRC_ALPHA,gl.BLEND_DST_ALPHA,gl.BLEND_COLOR,gl.CULL_FACE_MODE,gl.FRONT_FACE,gl.DEPTH_FUNC,gl.DEPTH_WRITEMASK,gl.DEPTH_RANGE,gl.COLOR_WRITEMASK,gl.COLOR_CLEAR_VALUE,gl.DEPTH_CLEAR_VALUE,gl.POLYGON_OFFSET_FACTOR,gl.POLYGON_OFFSET_UNITS,gl.SAMPLE_COVERAGE_VALUE,gl.SAMPLE_COVERAGE_INVERT,gl.STENCIL_CLEAR_VALUE,gl.STENCIL_WRITEMASK,gl.STENCIL_BACK_WRITEMASK,gl.STENCIL_FUNC,gl.STENCIL_REF,gl.STENCIL_VALUE_MASK,gl.STENCIL_FAIL,gl.STENCIL_PASS_DEPTH_FAIL,gl.STENCIL_PASS_DEPTH_PASS,gl.STENCIL_BACK_FUNC,gl.STENCIL_BACK_REF,gl.STENCIL_BACK_VALUE_MASK,gl.STENCIL_BACK_FAIL,gl.STENCIL_BACK_PASS_DEPTH_FAIL,gl.STENCIL_BACK_PASS_DEPTH_PASS];
  const raster=()=>({caps:caps.map(p=>[p,gl.isEnabled(p)]),params:params.map(p=>[p,ArrayBuffer.isView(get(p))?Array.from(get(p)):get(p)])});
  const applyRaster=r=>{for(const[p,on]of r.caps)(on?gl.enable:gl.disable).call(gl,p);const p=new Map(r.params);gl.viewport(...p.get(gl.VIEWPORT));gl.scissor(...p.get(gl.SCISSOR_BOX));gl.blendEquationSeparate(p.get(gl.BLEND_EQUATION_RGB),p.get(gl.BLEND_EQUATION_ALPHA));gl.blendFuncSeparate(p.get(gl.BLEND_SRC_RGB),p.get(gl.BLEND_DST_RGB),p.get(gl.BLEND_SRC_ALPHA),p.get(gl.BLEND_DST_ALPHA));gl.blendColor(...p.get(gl.BLEND_COLOR));gl.cullFace(p.get(gl.CULL_FACE_MODE));gl.frontFace(p.get(gl.FRONT_FACE));gl.depthFunc(p.get(gl.DEPTH_FUNC));gl.depthMask(p.get(gl.DEPTH_WRITEMASK));gl.depthRange(...p.get(gl.DEPTH_RANGE));gl.colorMask(...p.get(gl.COLOR_WRITEMASK));gl.clearColor(...p.get(gl.COLOR_CLEAR_VALUE));gl.clearDepth(p.get(gl.DEPTH_CLEAR_VALUE));gl.polygonOffset(p.get(gl.POLYGON_OFFSET_FACTOR),p.get(gl.POLYGON_OFFSET_UNITS));gl.sampleCoverage(p.get(gl.SAMPLE_COVERAGE_VALUE),p.get(gl.SAMPLE_COVERAGE_INVERT));gl.clearStencil(p.get(gl.STENCIL_CLEAR_VALUE));for(const face of [gl.FRONT,gl.BACK]){const back=face===gl.BACK;gl.stencilMaskSeparate(face,p.get(back?gl.STENCIL_BACK_WRITEMASK:gl.STENCIL_WRITEMASK));gl.stencilFuncSeparate(face,p.get(back?gl.STENCIL_BACK_FUNC:gl.STENCIL_FUNC),p.get(back?gl.STENCIL_BACK_REF:gl.STENCIL_REF),p.get(back?gl.STENCIL_BACK_VALUE_MASK:gl.STENCIL_VALUE_MASK));gl.stencilOpSeparate(face,p.get(back?gl.STENCIL_BACK_FAIL:gl.STENCIL_FAIL),p.get(back?gl.STENCIL_BACK_PASS_DEPTH_FAIL:gl.STENCIL_PASS_DEPTH_FAIL),p.get(back?gl.STENCIL_BACK_PASS_DEPTH_PASS:gl.STENCIL_PASS_DEPTH_PASS));}};
  const savedState=()=>{const active=get(gl.ACTIVE_TEXTURE),units=[];for(let i=0;i<get(gl.MAX_COMBINED_TEXTURE_IMAGE_UNITS);i++){gl.activeTexture(gl.TEXTURE0+i);units.push([get(gl.TEXTURE_BINDING_2D),get(gl.TEXTURE_BINDING_CUBE_MAP),get(gl.TEXTURE_BINDING_3D),get(gl.TEXTURE_BINDING_2D_ARRAY),get(gl.SAMPLER_BINDING)]);}gl.activeTexture(active);return {r:raster(),active,units,program:get(gl.CURRENT_PROGRAM),vao:get(gl.VERTEX_ARRAY_BINDING),read:get(gl.READ_FRAMEBUFFER_BINDING),draw:get(gl.DRAW_FRAMEBUFFER_BINDING),renderbuffer:get(gl.RENDERBUFFER_BINDING),ubo:Array.from({length:get(gl.MAX_UNIFORM_BUFFER_BINDINGS)},(_,i)=>[gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING,i),gl.getIndexedParameter(gl.UNIFORM_BUFFER_START,i),gl.getIndexedParameter(gl.UNIFORM_BUFFER_SIZE,i)]),constants:Array.from({length:get(gl.MAX_VERTEX_ATTRIBS)},(_,i)=>Array.from(gl.getVertexAttrib(i,gl.CURRENT_VERTEX_ATTRIB))),buffers:[gl.ARRAY_BUFFER,gl.COPY_READ_BUFFER,gl.UNIFORM_BUFFER,gl.PIXEL_PACK_BUFFER,gl.PIXEL_UNPACK_BUFFER].map(p=>[p,get(({[gl.ARRAY_BUFFER]:gl.ARRAY_BUFFER_BINDING,[gl.COPY_READ_BUFFER]:gl.COPY_READ_BUFFER_BINDING,[gl.UNIFORM_BUFFER]:gl.UNIFORM_BUFFER_BINDING,[gl.PIXEL_PACK_BUFFER]:gl.PIXEL_PACK_BUFFER_BINDING,[gl.PIXEL_UNPACK_BUFFER]:gl.PIXEL_UNPACK_BUFFER_BINDING})[p])]),pixel:[gl.PACK_ALIGNMENT,gl.PACK_ROW_LENGTH,gl.PACK_SKIP_PIXELS,gl.PACK_SKIP_ROWS,gl.UNPACK_ALIGNMENT,gl.UNPACK_FLIP_Y_WEBGL,gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,gl.UNPACK_COLORSPACE_CONVERSION_WEBGL,gl.UNPACK_ROW_LENGTH,gl.UNPACK_IMAGE_HEIGHT,gl.UNPACK_SKIP_PIXELS,gl.UNPACK_SKIP_ROWS,gl.UNPACK_SKIP_IMAGES].map(p=>[p,get(p)])};};
  const restore=a=>{applyRaster(a.r);gl.useProgram(a.program);gl.bindVertexArray(a.vao);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,a.read);gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER,a.draw);gl.bindRenderbuffer(gl.RENDERBUFFER,a.renderbuffer);for(let i=0;i<a.ubo.length;i++){const[h,o,n]=a.ubo[i];if(h&&n)gl.bindBufferRange(gl.UNIFORM_BUFFER,i,h,o,n);else gl.bindBufferBase(gl.UNIFORM_BUFFER,i,h);}for(let i=0;i<a.constants.length;i++)gl.vertexAttrib4fv(i,a.constants[i]);for(const[p,v]of a.buffers)gl.bindBuffer(p,v);for(const[p,v]of a.pixel)gl.pixelStorei(p,v);for(let i=0;i<a.units.length;i++){gl.activeTexture(gl.TEXTURE0+i);const t=a.units[i];for(const[target,j]of [[gl.TEXTURE_2D,0],[gl.TEXTURE_CUBE_MAP,1],[gl.TEXTURE_3D,2],[gl.TEXTURE_2D_ARRAY,3]])gl.bindTexture(target,t[j]);gl.bindSampler(i,t[4]);}gl.activeTexture(a.active);};
  const buffer=h=>{if(!h)return null;if(buffers.has(h))return buffers.get(h);gl.bindBuffer(gl.COPY_READ_BUFFER,h);const data=new Uint8Array(gl.getBufferParameter(gl.COPY_READ_BUFFER,gl.BUFFER_SIZE));gl.getBufferSubData(gl.COPY_READ_BUFFER,0,data);const id=store(data);buffers.set(h,id);return id;};
  const check=label=>{const e=gl.getError();if(e)throw Error(label+' GL '+e);};
  const targetFormat=()=>{const draw=get(gl.DRAW_FRAMEBUFFER_BINDING);if(!draw)return {width:gl.drawingBufferWidth,height:gl.drawingBufferHeight,internal:gl.RGBA8,samples:0};const kind=gl.getFramebufferAttachmentParameter(gl.DRAW_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE),handle=gl.getFramebufferAttachmentParameter(gl.DRAW_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME);if(kind===gl.RENDERBUFFER){const meta=s.renderbuffers.get(handle);if(!meta)throw Error('Untracked stage renderbuffer');return meta;}const level=gl.getFramebufferAttachmentParameter(gl.DRAW_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL),meta=s.textures.get(handle)?.levels.find(x=>x.level===level);if(!meta)throw Error('Untracked stage color target');return {...meta,samples:0};};
  const simpleProgram=(v,f,attrs=[])=>{const p=gl.createProgram();for(const[type,src]of [[gl.VERTEX_SHADER,v],[gl.FRAGMENT_SHADER,f]]){const shader=gl.createShader(type);gl.shaderSource(shader,src);gl.compileShader(shader);if(!gl.getShaderParameter(shader,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(shader));gl.attachShader(p,shader);gl.deleteShader(shader);}for(const[n,l]of attrs)gl.bindAttribLocation(p,l,n);gl.linkProgram(p);if(!gl.getProgramParameter(p,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(p));return p;};
  let depthCopy;
  const texture=(handle,target)=>{
    if(!handle)return null;const meta=s.textures.get(handle);if(!meta||!meta.levels.length)throw Error('Untracked sampled texture');gl.bindTexture(target,handle);
    const params=[gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER,gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T,gl.TEXTURE_BASE_LEVEL,gl.TEXTURE_MAX_LEVEL,gl.TEXTURE_COMPARE_MODE,gl.TEXTURE_COMPARE_FUNC];if(target!==gl.TEXTURE_2D)params.push(gl.TEXTURE_WRAP_R);
    const anisotropy=gl.getExtension('EXT_texture_filter_anisotropic');
    if(anisotropy)params.push(anisotropy.TEXTURE_MAX_ANISOTROPY_EXT);
    params.push(gl.TEXTURE_MIN_LOD,gl.TEXTURE_MAX_LOD);
    const row={target,params:params.map(p=>[p,gl.getTexParameter(target,p)]),levels:[]};
    for(const level of meta.levels){const {width:w,height:h,depth:d=1,internal}=level;if(level.compressed){if(!level.data)throw Error('Compressed texture mip has no exact block snapshot');row.levels.push({...level,data:store(level.data)});continue;}
      const isDepth=[gl.DEPTH_COMPONENT16,gl.DEPTH_COMPONENT24,gl.DEPTH_COMPONENT32F,gl.DEPTH24_STENCIL8,gl.DEPTH32F_STENCIL8,gl.DEPTH_COMPONENT,gl.DEPTH_STENCIL].includes(internal);
      const half=[gl.RGBA16F,gl.RGB16F,gl.RG16F,gl.R16F].includes(internal),float=isDepth||[gl.RGBA32F,gl.RGB32F,gl.RG32F,gl.R32F,gl.R11F_G11F_B10F,gl.RGB9_E5].includes(internal),type=float?gl.FLOAT:half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE;
      const data=float?new Float32Array(w*h*4*d):half?new Uint16Array(w*h*4*d):new Uint8Array(w*h*4*d);
      for(let z=0;z<d;z++){
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER,scratch);
        if(isDepth){
          if(target!==gl.TEXTURE_2D)throw Error('Unsupported depth texture target');
          depthCopy ||= simpleProgram('#version 300 es\nvoid main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(p*2.-1.,0.,1.);}', '#version 300 es\nprecision highp float;uniform sampler2D source;uniform int level;out vec4 color;void main(){color=vec4(texelFetch(source,ivec2(gl_FragCoord.xy),level).r,0.,0.,1.);}');
          const out=gl.createTexture();gl.activeTexture(gl.TEXTURE0+31);gl.bindTexture(gl.TEXTURE_2D,out);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA32F,w,h,0,gl.RGBA,gl.FLOAT,null);gl.bindFramebuffer(gl.FRAMEBUFFER,scratch);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,out,0);gl.useProgram(depthCopy);gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);gl.bindSampler(0,null);gl.uniform1i(gl.getUniformLocation(depthCopy,'source'),0);gl.uniform1i(gl.getUniformLocation(depthCopy,'level'),level.level);gl.viewport(0,0,w,h);for(const cap of caps)gl.disable(cap);gl.colorMask(true,true,true,true);native.drawArrays.call(gl,gl.TRIANGLES,0,3);gl.readPixels(0,0,w,h,gl.RGBA,gl.FLOAT,data);gl.deleteTexture(out);
        }else{
          if(target===gl.TEXTURE_3D||target===gl.TEXTURE_2D_ARRAY)gl.framebufferTextureLayer(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,handle,level.level,z);else gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,level.target,handle,level.level);
          if(gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE)throw Error('Unrenderable sampled texture '+internal);
          gl.readPixels(0,0,w,h,gl.RGBA,type,data.subarray(z*w*h*4,(z+1)*w*h*4));
        }
      }
      row.levels.push({target:level.target,level:level.level,width:w,height:h,depth:d,internal:isDepth?gl.DEPTH_COMPONENT32F:float?gl.RGBA32F:half?gl.RGBA16F:internal===gl.SRGB8_ALPHA8?gl.SRGB8_ALPHA8:gl.RGBA8,type,depthTexture:isDepth,data:store(isDepth?Float32Array.from({length:w*h*d},(_,i)=>data[i*4]):data)});
    }
    return row;
  };
  // Records one draw: the program, bindings, raster state and every resource it reads, deduplicated by content in T.blobs.
  const capture=(name,args,source)=>{
    if(!T.captureReplay)return;
    const indexed=name.includes('Elements'),range=name==='drawRangeElements',count=args[range?3:indexed?1:2],instances=name.endsWith('Instanced')?args[indexed?4:3]:1;
    if(count===0||instances===0)return;
    if(!source.key){if(source.alternatives.length)(T.programGaps ||= []).push({source,reason:'No declared kind/layout match'});return;}
    if(!source.key.startsWith('effects/'))return;
    const fb=get(gl.DRAW_FRAMEBUFFER_BINDING);if(fb&&!framebuffers.has(fb))framebuffers.set(fb,nextFramebuffer++);const signature=source.key+':'+(fb?framebuffers.get(fb):0)+':'+(source.transport || '');
    // Preserve distinct programs and render targets; repeated instanced draws
    // with different resources are separate fixtures within the captured frame.
    if(seen.has(signature))return;seen.add(signature);
    buffers.clear();
    const saved=savedState(),program=saved.program,uniforms=[],blocks=[],attributes=[],sampled=[],format=targetFormat();
    try{
      if(gl.isEnabled(gl.STENCIL_TEST))throw Error("Stencil effects require a stencil fixture");
      gl.bindBuffer(gl.PIXEL_PACK_BUFFER,null);gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER,null);gl.pixelStorei(gl.PACK_ALIGNMENT,1);for(const p of [gl.PACK_ROW_LENGTH,gl.PACK_SKIP_PIXELS,gl.PACK_SKIP_ROWS])gl.pixelStorei(p,0);
      for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_UNIFORMS);i++){
        const u=gl.getActiveUniform(program,i),location=gl.getUniformLocation(program,u.name);if(location===null)continue;
        const value=window.__glTracking.uniformValue(gl,program,u),sampler=[gl.SAMPLER_2D,gl.SAMPLER_3D,gl.SAMPLER_CUBE,gl.SAMPLER_2D_ARRAY,gl.SAMPLER_2D_SHADOW].includes(u.type);
        uniforms.push({name:u.name,type:u.type,size:u.size,value:store(ArrayBuffer.isView(value)?value.slice():([gl.FLOAT,gl.FLOAT_VEC2,gl.FLOAT_VEC3,gl.FLOAT_VEC4,gl.FLOAT_MAT3,gl.FLOAT_MAT4].includes(u.type)?Float32Array:Int32Array).of(value))});
        if(sampler)for(const unit of ArrayBuffer.isView(value)?value:[value]){
          const target=u.type===gl.SAMPLER_3D?gl.TEXTURE_3D:u.type===gl.SAMPLER_CUBE?gl.TEXTURE_CUBE_MAP:u.type===gl.SAMPLER_2D_ARRAY?gl.TEXTURE_2D_ARRAY:gl.TEXTURE_2D;
          const handle=saved.units[unit][target===gl.TEXTURE_2D?0:target===gl.TEXTURE_CUBE_MAP?1:target===gl.TEXTURE_3D?2:3];
          const resource=texture(handle,target),sampler=saved.units[unit][4];if(resource&&sampler)for(const p of [gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER,gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T,gl.TEXTURE_COMPARE_MODE,gl.TEXTURE_COMPARE_FUNC]){const pair=resource.params.find(x=>x[0]===p);pair[1]=gl.getSamplerParameter(sampler,p);}sampled.push({unit,resource});
        }
      }
      // Texture readback uses an auxiliary program, so inspect vertex arrays
      // and block bindings from the saved live program/VAO explicitly.
      gl.bindVertexArray(saved.vao);
      // A matrix occupies one vertex-array slot per column. Reflection lists
      // only its base location, while the VAO retains each column separately.
      for(const[n,base]of source.attributes){
        const declaration=source.layout.find(a=>a.name===n);
        const columns=({[gl.FLOAT_MAT2]:2,[gl.FLOAT_MAT3]:3,[gl.FLOAT_MAT4]:4,[gl.FLOAT_MAT2x3]:2,[gl.FLOAT_MAT2x4]:2,[gl.FLOAT_MAT3x2]:3,[gl.FLOAT_MAT3x4]:3,[gl.FLOAT_MAT4x2]:4,[gl.FLOAT_MAT4x3]:4})[declaration?.type] || 1;
        for(let column=0;column<columns*(declaration?.size || 1);column++){
          const at=base+column;
          attributes.push({name:n,at,enabled:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_ENABLED),buffer:buffer(gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING)),size:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_SIZE),type:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_TYPE),normalized:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_NORMALIZED),integer:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_INTEGER),stride:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_STRIDE),offset:gl.getVertexAttribOffset(at,gl.VERTEX_ATTRIB_ARRAY_POINTER),divisor:gl.getVertexAttrib(at,gl.VERTEX_ATTRIB_ARRAY_DIVISOR),value:Array.from(gl.getVertexAttrib(at,gl.CURRENT_VERTEX_ATTRIB))});
        }
      }
      for(let i=0;i<gl.getProgramParameter(program,gl.ACTIVE_UNIFORM_BLOCKS);i++){const binding=gl.getActiveUniformBlockParameter(program,i,gl.UNIFORM_BLOCK_BINDING),handle=gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING,binding);blocks.push({name:gl.getActiveUniformBlockName(program,i),binding,buffer:buffer(handle),offset:gl.getIndexedParameter(gl.UNIFORM_BUFFER_START,binding),size:gl.getIndexedParameter(gl.UNIFORM_BUFFER_SIZE,binding)});}
      const row={kind:'gl-effect',key:source.key,source,command:{name,args},format,raster:saved.r,uniforms,blocks,attributes,index:buffer(get(gl.ELEMENT_ARRAY_BUFFER_BINDING)),sampled};
      // The original live GPU resources establish the expected fixture. A
      // separate serialized-input draw checks that capture lost no inputs.
      restore(saved);row.output=draw(row,T.blobs,{handle:program,live:true});row.depthOutput=draw(row,T.blobs,{handle:program,live:true},true);T.records.push(row);check('Effects draw capture');
    }finally{restore(saved);}
  };
  // Recreates a recorded texture, mips included, with the sampler state the draw had.
  const uploadTexture=(resource,blobs)=>{
    if(!resource)return null;const h=gl.createTexture(),target=resource.target;gl.bindTexture(target,h);
    gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER,null);for(const p of [gl.UNPACK_FLIP_Y_WEBGL,gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL])gl.pixelStorei(p,false);for(const p of [gl.UNPACK_ROW_LENGTH,gl.UNPACK_IMAGE_HEIGHT,gl.UNPACK_SKIP_PIXELS,gl.UNPACK_SKIP_ROWS,gl.UNPACK_SKIP_IMAGES])gl.pixelStorei(p,0);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);
    for(const [p,v]of resource.params)gl.texParameterf(target,p,v);
    for(const l of resource.levels){const data=blobs[l.data];if(l.compressed){if(target===gl.TEXTURE_3D||target===gl.TEXTURE_2D_ARRAY)gl.compressedTexImage3D(l.target,l.level,l.internal,l.width,l.height,l.depth,0,data);else gl.compressedTexImage2D(l.target,l.level,l.internal,l.width,l.height,0,data);}else if(target===gl.TEXTURE_3D||target===gl.TEXTURE_2D_ARRAY)gl.texImage3D(l.target,l.level,l.internal,l.width,l.height,l.depth,0,l.depthTexture?gl.DEPTH_COMPONENT:gl.RGBA,l.type,data);else gl.texImage2D(l.target,l.level,l.internal,l.width,l.height,0,l.depthTexture?gl.DEPTH_COMPONENT:gl.RGBA,l.type,data);}
    return h;
  };
  const uniform=(p,u,blobs)=>{
    const loc=gl.getUniformLocation(p,u.name),v=blobs[u.value];if(loc===null)throw Error('Missing replay uniform '+u.name);
    if(u.type===gl.FLOAT)gl.uniform1fv(loc,v);else if(u.type===gl.FLOAT_VEC2)gl.uniform2fv(loc,v);else if(u.type===gl.FLOAT_VEC3)gl.uniform3fv(loc,v);else if(u.type===gl.FLOAT_VEC4)gl.uniform4fv(loc,v);else if(u.type===gl.FLOAT_MAT3)gl.uniformMatrix3fv(loc,false,v);else if(u.type===gl.FLOAT_MAT4)gl.uniformMatrix4fv(loc,false,v);else if([gl.INT,gl.BOOL,gl.SAMPLER_2D,gl.SAMPLER_3D,gl.SAMPLER_CUBE,gl.SAMPLER_2D_ARRAY,gl.SAMPLER_2D_SHADOW].includes(u.type))gl.uniform1iv(loc,v);else if(u.type===gl.INT_VEC2||u.type===gl.BOOL_VEC2)gl.uniform2iv(loc,v);else if(u.type===gl.INT_VEC3||u.type===gl.BOOL_VEC3)gl.uniform3iv(loc,v);else if(u.type===gl.INT_VEC4||u.type===gl.BOOL_VEC4)gl.uniform4iv(loc,v);else throw Error('Unsupported uniform type '+u.type);
  };
  let renderer;
  // Links the owned program for a recorded draw and verifies its stages are byte for byte the catalog's and its attribute locations the catalog's.
  const selectProgram=async(row)=>{
    const cacheKey=row.key+(T.referenceBuild?':'+(row.source.transport || ''):'');
    if(programs.has(cacheKey))return programs.get(cacheKey);let entry;
    if(T.referenceBuild)entry={handle:simpleProgram(row.source.vertex,row.source.fragment,row.source.attributes)};
    else{
      renderer ||= new (Object.getPrototypeOf(core.rust).constructor)(gl);
      const create=gl.createProgram;let handle;gl.createProgram=function(){handle=create.call(this);return handle;};let id;
      try{id=renderer.add_owned_program(row.key);}finally{gl.createProgram=create;}
      if(!handle)throw Error('Owned replay program was not linked');
      const instanceExtras={};
      // Native linking detaches its shaders. The link hook preserves the
      // exact linked stages before detach, including their row field loads.
      const linked=window.__glTracking.state(gl).programs.get(handle);
      if(!linked?.vertex || !linked?.fragment)throw Error('Missing linked replay sources');
      for(const stage of ['vertex','fragment']){
        const source=linked[stage],expected=row.source[stage];
        if(row.source.transport){
          const digest=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(source));
          const hash=Array.from(new Uint8Array(digest),x=>x.toString(16).padStart(2,'0')).join('');
          if(hash!==row.source.owned[stage])throw Error('Owned replay source differs: '+row.key);
        }else if(source!==expected)throw Error('Owned replay source differs: '+row.key);
        // Material-row fields come from the verified Rust-emitted loads.
        if(source.includes('void bfeLoadExtras()'))Object.assign(instanceExtras,window.__glTracking.instanceExtras(source));
      }
      for(const[name,location]of row.source.attributes){const expected=row.source.transport?row.source.owned.attributes.find(([n])=>n===name)?.[1]:location;if(expected===undefined||gl.getAttribLocation(handle,name)!==expected)throw Error('Owned replay attribute differs: '+row.key+' '+name);}
      entry={handle,id,instanceExtras};
    }
    programs.set(cacheKey,entry);return entry;
  };
  // World, Effect and Skinned programs read the camera from the BfeFrame block; put the recorded camera values into one buffer bound at its slot.
  const bindWorldFrame=(row,entry,blobs,created)=>{
    if(!['World','Effect','Skinned'].includes(row.source.transport)||entry.id===undefined)return;
    const block=gl.getUniformBlockIndex(entry.handle,'BfeFrame');
    if(block===gl.INVALID_INDEX)throw Error('Owned World replay has no camera block');
    const bytes=new ArrayBuffer(gl.getActiveUniformBlockParameter(entry.handle,block,gl.UNIFORM_BLOCK_DATA_SIZE)),floats=new Float32Array(bytes),moved=new Set();
    for(const u of row.uniforms){
      // An Effect program draws in world space: the rewrite reads its model-view
      // as the frame's view matrix, which is exactly the value the plain program had.
      const member=row.source.transport==='Effect'&&u.name==='modelViewMatrix'?'viewMatrix':u.name,index=gl.getUniformIndices(entry.handle,[member])[0];
      if(index===gl.INVALID_INDEX){if(row.source.owned.instanceLayout?.fields[u.name]||entry.instanceExtras?.[u.name])continue;if(gl.getUniformLocation(entry.handle,u.name)===null)throw Error('Unsupported camera uniform transport: '+u.name);continue;}
      if(gl.getActiveUniforms(entry.handle,[index],gl.UNIFORM_BLOCK_INDEX)[0]!==block)continue;
      moved.add(u.name);
      const offset=gl.getActiveUniforms(entry.handle,[index],gl.UNIFORM_OFFSET)[0]/4,value=blobs[u.value];
      const stride=gl.getActiveUniforms(entry.handle,[index],gl.UNIFORM_MATRIX_STRIDE)[0]/4;
      if(stride){const columns=u.type===gl.FLOAT_MAT4?4:u.type===gl.FLOAT_MAT3?3:2;for(let c=0;c<columns;c++)floats.set(value.subarray(c*columns,(c+1)*columns),offset+c*stride);}
      else floats.set(value,offset);
    }
    const buffer=gl.createBuffer();created.push(buffer);gl.bindBuffer(gl.UNIFORM_BUFFER,buffer);gl.bufferData(gl.UNIFORM_BUFFER,new Uint8Array(bytes),gl.STATIC_DRAW);gl.uniformBlockBinding(entry.handle,block,0);gl.bindBufferBase(gl.UNIFORM_BUFFER,0,buffer);return moved;
  };
  // Motion and Skinned programs read per-object matrices from an instance row texture; fill it from the recorded values and bind it.
  const bindInstanceRow=(row,entry,blobs,created)=>{
    if(!['Motion','Skinned'].includes(row.source.transport)||entry.id===undefined)return;
    const layout=row.source.owned.instanceLayout;
    if(!layout)throw Error('Missing native instance row layout');
    const data=new Float32Array(layout.texels*4),moved=new Set();
    for(const u of row.uniforms){
      const field=layout.fields[u.name]||(row.source.transport==='Skinned'?entry.instanceExtras?.[u.name]:undefined);if(!field)continue;
      const value=blobs[u.value];
      if(value.length!==field.columns*field.rows)throw Error('Instance matrix size changed: '+u.name);
      for(let c=0;c<field.columns;c++)data.set(value.subarray(c*field.rows,(c+1)*field.rows),field.at+c*4);
      moved.add(u.name);
    }
    let unit=0;while(unit===31||row.sampled.some(s=>s.unit===unit))unit++;
    const texture=gl.createTexture();created.push(texture);gl.activeTexture(gl.TEXTURE0+unit);gl.bindTexture(gl.TEXTURE_2D,texture);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA32F,layout.texels,1,0,gl.RGBA,gl.FLOAT,data);
    gl.useProgram(entry.handle);gl.uniform1i(gl.getUniformLocation(entry.handle,'bfeInstances'),unit);gl.uniform1i(gl.getUniformLocation(entry.handle,'bfeInstanceBase'),0);
    return moved;
  };
  // Issues the recorded draw call itself, through the module's replay entry for an owned program or directly for a browser one.
  const issue=(row,entry)=>{const {name,args}=row.command;if(entry.id!==undefined){const indexed=name.includes('Elements'),instanced=name.endsWith('Instanced'),range=name==='drawRangeElements',mode=args[0],count=args[range?3:indexed?1:2],type=indexed?args[range?4:2]:0,offset=args[range?5:indexed?3:1],instances=instanced?args[indexed?4:3]:1;renderer.replay_program_draw(entry.id,mode,count,type,offset,instances,indexed);}else native[name].apply(gl,args);};
  // One replayed draw over fixed colour and depth, optionally with occluders in front (depthPattern) to prove depth state is reproduced.
  const draw=(row,blobs,entry,depthPattern=false)=>{
    const {width:w,height:h,internal,samples=0}=row.format,createdBuffers=[],createdTextures=[],createdRenderbuffers=[];
    const outputTexture=gl.createTexture(),output=gl.createFramebuffer(),drawFB=samples?gl.createFramebuffer():output,vao=entry.live?null:gl.createVertexArray();
    gl.activeTexture(gl.TEXTURE0+31);const oldUnit31=get(gl.TEXTURE_BINDING_2D);gl.bindTexture(gl.TEXTURE_2D,outputTexture);gl.texImage2D(gl.TEXTURE_2D,0,internal,w,h,0,gl.RGBA,[gl.RGBA16F,gl.RGBA32F].includes(internal)?gl.FLOAT:gl.UNSIGNED_BYTE,null);gl.bindFramebuffer(gl.FRAMEBUFFER,output);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,outputTexture,0);
    gl.bindFramebuffer(gl.FRAMEBUFFER,drawFB);
    const renderbuffer=(attachment,format)=>{const hnd=gl.createRenderbuffer();createdRenderbuffers.push(hnd);gl.bindRenderbuffer(gl.RENDERBUFFER,hnd);if(samples)gl.renderbufferStorageMultisample(gl.RENDERBUFFER,samples,format,w,h);else gl.renderbufferStorage(gl.RENDERBUFFER,format,w,h);gl.framebufferRenderbuffer(gl.FRAMEBUFFER,attachment,gl.RENDERBUFFER,hnd);};
    if(samples)renderbuffer(gl.COLOR_ATTACHMENT0,internal);renderbuffer(gl.DEPTH_STENCIL_ATTACHMENT,gl.DEPTH24_STENCIL8);
    if(gl.checkFramebufferStatus(gl.FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE)throw Error('Incomplete stage framebuffer');
    try{
      if(!entry.live){
        gl.bindVertexArray(vao);const handles=new Map(),upload=(id,target=gl.COPY_READ_BUFFER)=>{if(id===null)return null;if(handles.has(id))return handles.get(id);const hnd=gl.createBuffer();createdBuffers.push(hnd);gl.bindBuffer(target,hnd);gl.bufferData(target,blobs[id],gl.STATIC_DRAW);handles.set(id,hnd);return hnd;};
        for(const a of row.attributes){
          const original=row.source.attributes.find(([name])=>name===a.name)?.[1],owned=row.source.owned?.attributes.find(([name])=>name===a.name)?.[1];
          const at=entry.id!==undefined&&row.source.transport?owned+(a.at-original):a.at;
          if(!Number.isInteger(at))throw Error('Missing owned attribute binding: '+a.name);
          gl.bindBuffer(gl.ARRAY_BUFFER,upload(a.buffer));if(a.enabled){gl.enableVertexAttribArray(at);if(a.integer)gl.vertexAttribIPointer(at,a.size,a.type,a.stride,a.offset);else gl.vertexAttribPointer(at,a.size,a.type,a.normalized,a.stride,a.offset);gl.vertexAttribDivisor(at,a.divisor);}else{gl.disableVertexAttribArray(at);gl.vertexAttrib4fv(at,a.value);}
        }
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,upload(row.index,gl.ELEMENT_ARRAY_BUFFER));check(row.key+" index upload");
        for(const block of row.blocks){const index=gl.getUniformBlockIndex(entry.handle,block.name);if(index===gl.INVALID_INDEX)throw Error('Missing replay block '+block.name);gl.uniformBlockBinding(entry.handle,index,block.binding);if(block.size)gl.bindBufferRange(gl.UNIFORM_BUFFER,block.binding,upload(block.buffer),block.offset,block.size);else gl.bindBufferBase(gl.UNIFORM_BUFFER,block.binding,upload(block.buffer));}
        for(const sample of row.sampled){gl.activeTexture(gl.TEXTURE0+sample.unit);const hnd=uploadTexture(sample.resource,blobs);if(hnd)createdTextures.push(hnd);gl.bindSampler(sample.unit,null);}
        const moved=new Set([...(bindWorldFrame(row,entry,blobs,createdBuffers)||[]),...(bindInstanceRow(row,entry,blobs,createdTextures)||[])]);
        gl.useProgram(entry.handle);for(const u of row.uniforms)if(!moved?.has(u.name))uniform(entry.handle,u,blobs);check(row.key+" input upload");
      }
      // Clear all samples to a fixed, nonblack blend destination. Restore the
      // exact captured raster state before drawing with the live geometry.
      gl.disable(gl.SCISSOR_TEST);gl.colorMask(true,true,true,true);gl.depthMask(true);gl.stencilMask(0xff);gl.clearColor(.125,.25,.375,1);gl.clearDepth(1);gl.clearStencil(0);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT|gl.STENCIL_BUFFER_BIT);
      // A second fixture uses fixed depth occluders. This exercises fragment
      // depth and depth rejection with identical input depth in all roles.
      if(depthPattern){gl.enable(gl.SCISSOR_TEST);for(const[x,y,depth]of [[0,0,.2],[Math.floor(w/2),0,.8],[0,Math.floor(h/2),.5]]){gl.scissor(x,y,Math.ceil(w/2),Math.ceil(h/2));gl.clearDepth(depth);gl.clear(gl.DEPTH_BUFFER_BIT);}}
      applyRaster(row.raster);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER,drawFB);if(entry.live){gl.activeTexture(gl.TEXTURE0+31);gl.bindTexture(gl.TEXTURE_2D,oldUnit31);}issue(row,entry);check(row.key+" draw");
      if(samples){gl.bindFramebuffer(gl.READ_FRAMEBUFFER,drawFB);gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER,output);gl.disable(gl.SCISSOR_TEST);gl.blitFramebuffer(0,0,w,h,0,0,w,h,gl.COLOR_BUFFER_BIT,gl.NEAREST);}
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER,output);gl.bindBuffer(gl.PIXEL_PACK_BUFFER,null);gl.pixelStorei(gl.PACK_ALIGNMENT,1);for(const p of [gl.PACK_ROW_LENGTH,gl.PACK_SKIP_PIXELS,gl.PACK_SKIP_ROWS])gl.pixelStorei(p,0);
      const half=internal===gl.RGBA16F,float=internal===gl.RGBA32F,data=half?new Uint16Array(w*h*4):float?new Float32Array(w*h*4):new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:float?gl.FLOAT:gl.UNSIGNED_BYTE,data);check(row.key+' fixture');return {w,h,bytes:half?2:float?4:1,blob:store(data)};
    }finally{
      for(const hnd of createdTextures)gl.deleteTexture(hnd);for(const hnd of createdBuffers)gl.deleteBuffer(hnd);for(const hnd of createdRenderbuffers)gl.deleteRenderbuffer(hnd);gl.deleteTexture(outputTexture);gl.deleteFramebuffer(output);if(samples)gl.deleteFramebuffer(drawFB);if(vao)gl.deleteVertexArray(vao);
    }
  };
  s.capture=capture;
  T.resetGLStages=()=>{seen.clear();buffers.clear();};
  T.replayGL=async(row,blobs)=>{
    const saved=savedState();s.busy=true;
    try{
      const entry=await selectProgram(row);let differing=0,maxDelta=0;
      const modes=row.depthOutput?[false,true]:[false];
      const stages=[];
      for(const depthPattern of modes){
        const expected=depthPattern?row.depthOutput:row.output,actual=draw(row,blobs,entry,depthPattern),a=T.blobs[actual.blob],e=blobs[expected.blob];
        if(a.byteLength!==e.byteLength)throw Error('Effects fixture size changed');
        const x=new Uint8Array(a.buffer,a.byteOffset,a.byteLength),y=new Uint8Array(e.buffer,e.byteOffset,e.byteLength);let changedPixels=0;
        for(let i=0;i<x.length;i+=expected.bytes*4){let changed=false;for(let j=0;j<expected.bytes*4;j++)if(x[i+j]!==y[i+j]){changed=true;maxDelta=Math.max(maxDelta,Math.abs(x[i+j]-y[i+j]));}if(changed)changedPixels++;}
        delete T.blobs[actual.blob];differing+=changedPixels;stages.push({depthPattern,differing:changedPixels});
      }
      return {key:row.key,differing,maxDelta,pixels:row.output.w*row.output.h*modes.length,stages};
    }finally{s.busy=false;restore(saved);}
  };
  return 1;
}
