// Install in both frozen and candidate applications. Only the reference records
// live phase inputs. Both replay those inputs through their default Rust API.
export function installReplay(createStore) {
  const b=__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=window.__corpus;
  const postFrame=core.rust.post_frame.bind(core.rust),hudFrame=core.rust.hud_frame.bind(core.rust);
  // This raw readback framebuffer is outside the renderer's resource bank.
  // Recreate it after deliberate loss; managed texture targets restore there.
  let fb=gl.createFramebuffer();
  R.domElement.addEventListener('webglcontextrestored',()=>{fb=gl.createFramebuffer();});
  const STEP=221,UNIFORM=18,HUD_GROUP=45;
  const textureCopies=new Map();
  const anisotropy=gl.getExtension('EXT_texture_filter_anisotropic');
  const floatParameters=new Set([gl.TEXTURE_MIN_LOD,gl.TEXTURE_MAX_LOD,...(anisotropy?[anisotropy.TEXTURE_MAX_ANISOTROPY_EXT]:[])]);
  let depthTarget;
  const replayTargets=new Map();
  T.records=[];T.blobs={};
  let blobOwner,blobSequence=0;
  const store=T.store=createStore?createStore(T):(buf)=>{if(blobOwner!==T.blobs){blobOwner=T.blobs;blobSequence=0;}const key=String(blobSequence++);T.blobs[key]=buf;return key;};
  // Self replay reads the capture table itself. Deduplicated actual output
  // can share an expected buffer, so retain that immutable input table.
  const release=(key,inputs)=>{if(!createStore || inputs!==T.blobs)delete T.blobs[key];};
  const rgbaMetrics=(a,e)=>{
    if(a.constructor!==e.constructor || a.length!==e.length || a.length%4)throw Error('Replay output layout differs');
    const words=data=>data instanceof Float32Array?new Uint32Array(data.buffer,data.byteOffset,data.length):data;
    const aw=words(a),ew=words(e);let differing=0,maxDelta=0;
    for(let i=0;i<a.length;i+=4){let different=false,delta=0;for(let c=0;c<4;c++){if(!Number.isFinite(a[i+c]) || !Number.isFinite(e[i+c]))throw Error('Nonfinite replay output');different ||= aw[i+c]!==ew[i+c];delta=Math.max(delta,Math.abs(a[i+c]-e[i+c]));}if(different){differing++;maxDelta=Math.max(maxDelta,delta);}}
    return {differing,maxDelta,pixels:a.length/4};
  };
  const depthRGBA=(s,blobs)=>{const data=blobs[s.levels?.[0] ?? s.blob];if(!s.packedDepth)return data;const rgba=new Float32Array(s.w*s.h*4);for(let i=0;i<data.length;i++)for(let c=0;c<4;c++)rgba[i*4+c]=s.packedDepth[c]==='r'?data[i]:s.packedDepth[c];return rgba;};
  const read=(target,retain=true)=>{
    const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight;
    const bytes=target?.texture.type===1016?2:target?.texture.type===1015?4:1;
    const buf=bytes===2?new Uint16Array(w*h*4):bytes===4?new Float32Array(w*h*4):new Uint8Array(w*h*4);
    if(target){const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,(R.resources?.gpu?R.resources.gpu.framebuffer(R.resources.targetId(target),false,0):R.properties.get(target).__webglFramebuffer));gl.readPixels(0,0,w,h,gl.RGBA,bytes===2?gl.HALF_FLOAT:bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);}
    else {R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);}
    const error=gl.getError();if(error)throw Error('Replay read GL '+error);
    return retain?{w,h,bytes,blob:store(buf)}:{w,h,bytes,buf};
  };
  const textures=(objects)=>{
    const entries=new Map();
    for(const texture of objects){if(!texture?.isTexture)continue;const handle=R.properties.get(texture).__webglTexture,id=core.textures.get(handle);if(id!==undefined)entries.set(id,{texture,handle});}
    return entries;
  };
  const snapshot=(id,objects)=>{
    if(id<0)return null;
    const source=objects.get(id);if(!source)throw Error('No source texture for replay '+id);
    const {texture,handle}=source,im=texture.image || texture.mipmaps?.[0];
    const w=im?.width || texture.width,h=im?.height || texture.height;
    if(!w || !h)throw Error('No replay texture dimensions');
    const active=gl.getParameter(gl.ACTIVE_TEXTURE);gl.activeTexture(gl.TEXTURE0);
    const bound=gl.getParameter(gl.TEXTURE_BINDING_2D);gl.bindTexture(gl.TEXTURE_2D,handle);
    const params=[gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER,gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T,gl.TEXTURE_COMPARE_MODE,gl.TEXTURE_COMPARE_FUNC,gl.TEXTURE_BASE_LEVEL,gl.TEXTURE_MAX_LEVEL,gl.TEXTURE_MIN_LOD,gl.TEXTURE_MAX_LOD,...(anisotropy?[anisotropy.TEXTURE_MAX_ANISOTROPY_EXT]:[])].map(p=>[p,gl.getTexParameter(gl.TEXTURE_2D,p)]);
    gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);
    if(texture.isDepthTexture){
      if(!depthTarget || depthTarget.width!==w || depthTarget.height!==h){depthTarget?.dispose();depthTarget=new b.THREE.WebGLRenderTarget(w,h,{type:b.THREE.FloatType,depthBuffer:false,minFilter:b.THREE.NearestFilter,magFilter:b.THREE.NearestFilter});}
      R.initRenderTarget(depthTarget);
      gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);
      const compare=gl.getTexParameter(gl.TEXTURE_2D,gl.TEXTURE_COMPARE_MODE);
      gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_COMPARE_MODE,gl.NONE);
      const target=core.byName.post.target(depthTarget),program=core.rust.add_owned_program('post-hud/copy');
      core.views();const saved=core.postInput.slice();core.postInput.fill(0);core.postInput.set([program,target,w,h,1,0,32,id]);postFrame(1);core.views();core.postInput.set(saved);
      const rgba=read(depthTarget,false).buf;
      gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_COMPARE_MODE,compare);
      gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);
      const pattern=['r'];let packable=true;
      for(let c=1;c<4;c++){let same=true,constant=true;const value=rgba[c];for(let i=0;i<rgba.length;i+=4){same &&= rgba[i+c]===rgba[i];constant &&= rgba[i+c]===value;}if(same)pattern.push('r');else if(constant)pattern.push(value);else packable=false;}
      if(packable){const depth=new Float32Array(w*h);for(let i=0;i<depth.length;i++){depth[i]=rgba[i*4];if(!Number.isFinite(depth[i]))throw Error('Nonfinite replay depth');}return {id,w,h,bytes:4,depthType:texture.type,params,levels:[store(depth)],packedDepth:pattern};}
      return {id,w,h,bytes:4,depthType:texture.type,params,levels:[store(rgba)]};
    }
    const bytes=texture.type===1016?2:texture.type===1015?4:1,levels=[];
    const count=params[0][1]>=9984 && params[0][1]<=9987?Math.floor(Math.log2(Math.max(w,h)))+1:1;
    const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);
    for(let level=0;level<count;level++){
      const width=Math.max(1,w>>level),height=Math.max(1,h>>level);
      if(!texture.isDepthTexture && !texture.isCompressedTexture)gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,handle,level);
      if(texture.isDepthTexture || texture.isCompressedTexture || gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE){
        // Sample formats that cannot be attached directly, preserving decoded
        // texels in their native precision. Self replay verifies the conversion.
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
        const copyBytes=texture.isDepthTexture?4:bytes;
        const cacheKey=width+':'+height+':'+copyBytes;
        let target=textureCopies.get(cacheKey);
        if(!target){target=new b.THREE.WebGLRenderTarget(width,height,{type:copyBytes===4?b.THREE.FloatType:copyBytes===2?b.THREE.HalfFloatType:b.THREE.UnsignedByteType,depthBuffer:false,minFilter:b.THREE.NearestFilter,magFilter:b.THREE.NearestFilter});textureCopies.set(cacheKey,target);}
        R.initRenderTarget(target);
        const active=gl.getParameter(gl.ACTIVE_TEXTURE);gl.activeTexture(gl.TEXTURE0);const bound=gl.getParameter(gl.TEXTURE_BINDING_2D);gl.bindTexture(gl.TEXTURE_2D,handle);
        const base=gl.getTexParameter(gl.TEXTURE_2D,gl.TEXTURE_BASE_LEVEL),max=gl.getTexParameter(gl.TEXTURE_2D,gl.TEXTURE_MAX_LEVEL);
        gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_BASE_LEVEL,level);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAX_LEVEL,level);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);
        const program=core.rust.add_owned_program('post-hud/copy');core.views();const saved=core.postInput.slice();core.postInput.fill(0);core.postInput.set([program,core.byName.post.target(target),width,height,1,0,32,id]);postFrame(1);core.views();core.postInput.set(saved);levels.push(read(target).blob);
        gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);for(const [p,v] of params)gl.texParameteri(gl.TEXTURE_2D,p,v);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_BASE_LEVEL,base);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAX_LEVEL,max);gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);
        continue;
      }
      const buf=bytes===2?new Uint16Array(width*height*4):bytes===4?new Float32Array(width*height*4):new Uint8Array(width*height*4);
      gl.readPixels(0,0,width,height,gl.RGBA,bytes===2?gl.HALF_FLOAT:bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,buf);levels.push(store(buf));
    }
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
    const error=gl.getError();if(error)throw Error('Texture replay read GL '+error);
    return {id,w,h,bytes:texture.isDepthTexture?4:bytes,params,levels};
  };
  // Post samplers read depth as a float, without comparison. Re-uploading
  // that float through normalized depth storage can round it a second time.
  const resources=(ids,objects)=>[...new Set(ids.filter(id=>id>=0))].map(id=>({...snapshot(id,objects),sampledDepth:!!objects.get(id)?.texture.isDepthTexture}));
  const post=core.post;
  core.post=function(renderer,steps){
    if(!T.want || !T.captureReplay || !steps)return post.call(this,renderer,steps);
    for(const step of steps){T.step=step;if(!post.call(this,renderer,[step]))throw Error('Post capture refused');}T.step=null;return true;
  };
  core.rust.post_frame=function(steps){
    if(!T.want || !T.captureReplay || !T.step)return postFrame(steps);
    if(steps!==1)throw Error('Expected one captured post step');
    core.views();const input=Array.from(core.postInput.subarray(0,STEP)),ids=[];
    for(let i=0;i<input[4];i++)if(input[5+i*UNIFORM+1]===32)ids.push(input[5+i*UNIFORM+2]);
    const objects=textures(Object.values(T.step.pass.material.uniforms).map(u=>u.value));
    const data=resources(ids,objects);core.views();core.postInput.set(input);
    const result=postFrame(steps),output=read(T.step.target);
    T.records.push({kind:'post',canvas:!T.step.target,key:T.step.pass.material.userData.bfeProgram,input,resources:data,output});
    return result;
  };
  core.rust.hud_frame=function(){
    if(!T.want || !T.captureReplay)return hudFrame();
    core.views();const input=Array.from(core.hudInput.subarray(0,HUD_GROUP+core.hudInput[0]*5)),ids=[];
    for(let i=0;i<input[0];i++)ids.push(input[HUD_GROUP+i*5+2],input[HUD_GROUP+i*5+3]);
    // Native HUD assets are retained renderer registrations, not batch materials.
    const objects=textures(core.textureSources);
    const data=resources(ids,objects),background=read(null);
    const arrays=Object.fromEntries(['hudPosition','hudUv','hudMaskUv','hudTint'].map(name=>[name,store(core[name].slice())]));
    const result=hudFrame(),output=read(null);
    T.records.push({kind:'hud',key:'post-hud/hud',input,arrays,resources:data,background,output});
    return result;
  };
  // A cascade's depth as a comparison texture, at its recorded storage precision. The frozen reference
  // owns raw textures; on a build with the Rust resource owner the texture is a depth target of that owner
  // (so the renderer can sample it) and its texels are written into the owner's storage.
  const uploadDepth=(s,blobs)=>{
    const rgba=blobs[s.levels[0]],depth=s.packedDepth?rgba:new Float32Array(s.w*s.h);if(!s.packedDepth)for(let i=0;i<depth.length;i++)depth[i]=rgba[i*4];
    const Three=b.THREE,type=s.depthType===Three.UnsignedIntType?Three.UnsignedIntType:s.depthType===Three.UnsignedShortType?Three.UnsignedShortType:Three.FloatType;
    const values=type===Three.UnsignedIntType?Uint32Array.from(depth,v=>Math.round(v*4294967295)):type===Three.UnsignedShortType?Uint16Array.from(depth,v=>Math.round(v*65535)):depth;
    const glType=type===Three.UnsignedIntType?gl.UNSIGNED_INT:type===Three.UnsignedShortType?gl.UNSIGNED_SHORT:gl.FLOAT;
    const param=p=>s.params.find(([k])=>k===p)?.[1];
    if(!R.resources?.gpu){
      const handle=gl.createTexture();gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);
      // WebGL refuses a depth upload with either unpack conversion on (three leaves them set per image).
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);
      for(const [p,v] of s.params){if(floatParameters.has(p))gl.texParameterf(gl.TEXTURE_2D,p,v);else gl.texParameteri(gl.TEXTURE_2D,p,v);}
      gl.texImage2D(gl.TEXTURE_2D,0,type===Three.UnsignedIntType?gl.DEPTH_COMPONENT24:type===Three.UnsignedShortType?gl.DEPTH_COMPONENT16:gl.DEPTH_COMPONENT32F,s.w,s.h,0,gl.DEPTH_COMPONENT,glType,values);
      return {handle,id:core.rust.add_texture(handle),dispose:()=>gl.deleteTexture(handle)};
    }
    const texture=new Three.DepthTexture(s.w,s.h,type);
    const filter=v=>v===gl.LINEAR?Three.LinearFilter:Three.NearestFilter;
    texture.minFilter=filter(param(gl.TEXTURE_MIN_FILTER));texture.magFilter=filter(param(gl.TEXTURE_MAG_FILTER));
    if(param(gl.TEXTURE_COMPARE_MODE)===gl.COMPARE_REF_TO_TEXTURE)texture.compareFunction=param(gl.TEXTURE_COMPARE_FUNC);
    const target=new Three.WebGLRenderTarget(s.w,s.h,{depthBuffer:true,depthTexture:texture});R.initRenderTarget(target);
    const id=core.texture(texture),handle=R.properties.get(texture).__webglTexture;
    const active=gl.getParameter(gl.ACTIVE_TEXTURE),bound=gl.getParameter(gl.TEXTURE_BINDING_2D);
    gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);
    gl.texSubImage2D(gl.TEXTURE_2D,0,0,0,s.w,s.h,gl.DEPTH_COMPONENT,glType,values);
    gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);
    const error=gl.getError();if(error)throw Error('Replay depth upload GL '+error);
    return {handle,id,resource:R.resources.textureId(texture),dispose:()=>{texture.dispose();target.dispose();}};
  };
  const upload=(s,blobs)=>{
    if(s.sampledDepth){
      // Preserve the exact shader-visible texels. Comparison samplers in
      // world/actor fixtures still take uploadDepth and their storage type.
      return upload({...s,depth:undefined,depthType:undefined,packedDepth:undefined,sampledDepth:false,levels:['sample']},{sample:depthRGBA(s,blobs)});
    }
    // The frozen reference still lets three own its textures; the candidate's are the
    // Rust resource owner's, so replayed bytes go through that owner there.
    if(s.depth || s.depthType!==undefined)return uploadDepth(s,blobs);
    if(!R.resources?.gpu){
    const handle=gl.createTexture();gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);
    for(const [p,v] of s.params){if(floatParameters.has(p))gl.texParameterf(gl.TEXTURE_2D,p,v);else gl.texParameteri(gl.TEXTURE_2D,p,v);}
    for(let level=0;level<s.levels.length;level++)gl.texImage2D(gl.TEXTURE_2D,level,s.bytes===2?gl.RGBA16F:s.bytes===4?gl.RGBA32F:gl.RGBA8,Math.max(1,s.w>>level),Math.max(1,s.h>>level),0,gl.RGBA,s.bytes===2?gl.HALF_FLOAT:s.bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,blobs[s.levels[level]]);
    return {handle,id:core.rust.add_texture(handle),dispose:()=>gl.deleteTexture(handle)};
    }
    // Replayed source bytes use the same Rust resource owner as live images.
      const Three=b.THREE,type=s.bytes===2?Three.HalfFloatType:s.bytes===4?Three.FloatType:Three.UnsignedByteType;
      const size=s.levels.reduce((n,key)=>n+blobs[key].byteLength,0),bytes=new Uint8Array(size);let offset=0;
      const mips=s.levels.map((key,level)=>{const input=blobs[key],length=input.byteLength;bytes.set(new Uint8Array(input.buffer,input.byteOffset,length),offset);const data=new Uint8Array(bytes.buffer,offset,length);offset+=length;return {width:Math.max(1,s.w>>level),height:Math.max(1,s.h>>level),data};});
      const texture=new Three.DataTexture(null,s.w,s.h,Three.RGBAFormat,type);texture.mipmaps=mips;texture.generateMipmaps=false;
      const filters={9728:Three.NearestFilter,9729:Three.LinearFilter,9984:Three.NearestMipmapNearestFilter,9985:Three.LinearMipmapNearestFilter,9986:Three.NearestMipmapLinearFilter,9987:Three.LinearMipmapLinearFilter};
      const wraps={33071:Three.ClampToEdgeWrapping,10497:Three.RepeatWrapping,33648:Three.MirroredRepeatWrapping};
      for(const [p,v] of s.params){if(p===gl.TEXTURE_MIN_FILTER)texture.minFilter=filters[v];else if(p===gl.TEXTURE_MAG_FILTER)texture.magFilter=filters[v];else if(p===gl.TEXTURE_WRAP_S)texture.wrapS=wraps[v];else if(p===gl.TEXTURE_WRAP_T)texture.wrapT=wraps[v];else if(anisotropy && p===anisotropy.TEXTURE_MAX_ANISOTROPY_EXT)texture.anisotropy=v;}
      texture.needsUpdate=true;const id=core.texture(texture),handle=R.properties.get(texture).__webglTexture;
      return {handle,id,resource:R.resources.textureId(texture),dispose:()=>texture.dispose()};
  };
  // A second renderer (the actor replays' fresh one) registers an uploaded texture by its owner's id
  // where the build has the Rust resource owner, and by its handle on the frozen reference.
  const register=(renderer,s)=>R.resources?.gpu?renderer.add_texture(R.resources.gpu,s.resource):renderer.add_texture(s.handle);
  T.replayTools={store,release,snapshot,textures,upload,read,depthRGBA,rgbaMetrics,register};
  T.replay=async(records,blobs)=>{
    const metrics=[];T.want=false;
    core.views();const savedPost=core.postInput.slice(),savedHud=core.hudInput.slice();
    const savedArrays=Object.fromEntries(['hudPosition','hudUv','hudMaskUv','hudTint'].map(n=>[n,core[n].slice()]));
    const before=R.getRenderTarget();
    for(const row of records){
      if(row.kind==='post-policy'){metrics.push(await T.replayPostPolicy(row,blobs));continue;}
      if(row.kind==='gl-effect'){metrics.push(await T.replayGL(row,blobs));continue;}
      // A GL error is named by the row and step that raised it, not left for the readback to report.
      const checked=step=>{const error=gl.getError();if(error)throw Error(`Phase replay GL ${error} at ${step} of ${row.kind} ${row.key}`);};
      checked('start');
      const created=row.resources.map(s=>[s.id,upload(s,blobs)]),ids=new Map(created.map(([old,s])=>[old,s.id]));
      checked('upload');
      const input=Float64Array.from(row.input),program=core.rust.add_owned_program(row.key);
      let target=null;
      if(row.kind==='post'){
        input[0]=program;
        if(!row.canvas){
          const key=row.output.w+':'+row.output.h+':'+row.output.bytes;target=replayTargets.get(key);
          if(!target){target=new b.THREE.WebGLRenderTarget(row.output.w,row.output.h,{type:row.output.bytes===2?b.THREE.HalfFloatType:row.output.bytes===4?b.THREE.FloatType:b.THREE.UnsignedByteType,depthBuffer:false});replayTargets.set(key,target);}
        }
        if(target)R.initRenderTarget(target);
        input[1]=target?core.byName.post.target(target):-1;
        for(let i=0;i<input[4];i++){const at=5+i*UNIFORM;if(input[at+1]===32 && input[at+2]>=0)input[at+2]=ids.get(input[at+2]);}
        core.views();core.postInput.set(input);postFrame(1);checked('draw');
      }else{
        // Restore the exact pre-HUD canvas with a blit, then draw the recorded
        // vertex mirrors, camera uniforms and texture/mask groups.
        const bg=upload({w:row.background.w,h:row.background.h,bytes:1,params:[[gl.TEXTURE_MIN_FILTER,gl.NEAREST],[gl.TEXTURE_MAG_FILTER,gl.NEAREST]],levels:[row.background.blob]},blobs);
        R.resetState();R.setRenderTarget(null);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,bg.handle,0);gl.blitFramebuffer(0,0,row.background.w,row.background.h,0,0,row.background.w,row.background.h,gl.COLOR_BUFFER_BIT,gl.NEAREST);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,null);
        input[1]=program;input[41]=0;input[42]=8192;
        for(let i=0;i<input[0];i++)for(const j of [2,3]){const at=HUD_GROUP+i*5+j;if(input[at]>=0)input[at]=ids.get(input[at]);}
        core.views();core.hudInput.set(input);for(const [name,key] of Object.entries(row.arrays))core[name].set(blobs[key]);hudFrame();bg.dispose();
      }
      const actual=read(target),a=T.blobs[actual.blob],e=blobs[row.output.blob];const comparison=rgbaMetrics(a,e);
      release(actual.blob,blobs);metrics.push({key:row.key,...comparison});
      for(const [,s] of created)s.dispose();
    }
    core.views();core.postInput.set(savedPost);core.hudInput.set(savedHud);for(const [n,a] of Object.entries(savedArrays))core[n].set(a);core.byName.hud.full=true;
    R.resetState();R.setRenderTarget(before);const error=gl.getError();if(error)throw Error('Phase replay GL '+error);
    return metrics;
  };
  return 1;
}
