// Install in both frozen and candidate applications. Only the reference records
// live phase inputs. Both replay those inputs through their default Rust API.
export function installReplay() {
  const b=__bf,g=b.graphics,core=g.rust,R=g.renderer,gl=R.getContext(),T=window.__corpus;
  const postFrame=core.rust.post_frame.bind(core.rust),hudFrame=core.rust.hud_frame.bind(core.rust);
  const fb=gl.createFramebuffer();
  const STEP=221,UNIFORM=18,HUD_GROUP=45;
  let depthTarget;
  const replayTargets=new Map();
  T.records=[];T.blobs={};
  const store=(buf)=>{const key=String(Object.keys(T.blobs).length);T.blobs[key]=buf;return key;};
  const read=(target)=>{
    const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight;
    const bytes=target?.texture.type===1016?2:target?.texture.type===1015?4:1;
    const buf=bytes===2?new Uint16Array(w*h*4):bytes===4?new Float32Array(w*h*4):new Uint8Array(w*h*4);
    if(target){const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,R.properties.get(target).__webglFramebuffer);gl.readPixels(0,0,w,h,gl.RGBA,bytes===2?gl.HALF_FLOAT:bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);}
    else {R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);}
    const error=gl.getError();if(error)throw Error('Replay read GL '+error);
    return {w,h,bytes,blob:store(buf)};
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
    const params=[gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER,gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T].map(p=>[p,gl.getTexParameter(gl.TEXTURE_2D,p)]);
    gl.bindTexture(gl.TEXTURE_2D,bound);gl.activeTexture(active);
    if(texture.isDepthTexture){
      if(!depthTarget || depthTarget.width!==w || depthTarget.height!==h){depthTarget?.dispose();depthTarget=new b.THREE.WebGLRenderTarget(w,h,{type:b.THREE.FloatType,depthBuffer:false,minFilter:b.THREE.NearestFilter,magFilter:b.THREE.NearestFilter});}
      R.initRenderTarget(depthTarget);
      const target=core.byName.post.target(depthTarget),program=core.rust.add_owned_program('post-hud/copy');
      core.views();const saved=core.postInput.slice();core.postInput.fill(0);core.postInput.set([program,target,w,h,1,0,32,id]);postFrame(1);core.views();core.postInput.set(saved);
      return {id,w,h,bytes:4,params,levels:[read(depthTarget).blob]};
    }
    const bytes=texture.type===1016?2:texture.type===1015?4:1,levels=[];
    const count=params[0][1]>=9984 && params[0][1]<=9987?Math.floor(Math.log2(Math.max(w,h)))+1:1;
    const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);
    for(let level=0;level<count;level++){
      const width=Math.max(1,w>>level),height=Math.max(1,h>>level);
      gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,handle,level);
      if(gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE)throw Error('Unrepresentable replay texture '+id+' level '+level);
      const buf=bytes===2?new Uint16Array(width*height*4):bytes===4?new Float32Array(width*height*4):new Uint8Array(width*height*4);
      gl.readPixels(0,0,width,height,gl.RGBA,bytes===2?gl.HALF_FLOAT:bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,buf);levels.push(store(buf));
    }
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
    const error=gl.getError();if(error)throw Error('Texture replay read GL '+error);
    return {id,w,h,bytes,params,levels};
  };
  const resources=(ids,objects)=>[...new Set(ids.filter(id=>id>=0))].map(id=>snapshot(id,objects));
  const post=core.post;
  core.post=function(renderer,steps){
    if(!T.want || !T.captureReplay)return post.call(this,renderer,steps);
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
    const objects=textures(b.hud.batch.mesh.material.flatMap(m=>[m.uniforms.map.value,m.uniforms.mask.value]));
    const data=resources(ids,objects),background=read(null);
    const arrays=Object.fromEntries(['hudPosition','hudUv','hudMaskUv','hudTint'].map(name=>[name,store(core[name].slice())]));
    const result=hudFrame(),output=read(null);
    T.records.push({kind:'hud',key:'post-hud/hud',input,arrays,resources:data,background,output});
    return result;
  };
  const upload=(s,blobs)=>{
    const handle=gl.createTexture();gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,handle);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);
    for(const [p,v] of s.params)gl.texParameteri(gl.TEXTURE_2D,p,v);
    for(let level=0;level<s.levels.length;level++)gl.texImage2D(gl.TEXTURE_2D,level,s.bytes===2?gl.RGBA16F:s.bytes===4?gl.RGBA32F:gl.RGBA8,Math.max(1,s.w>>level),Math.max(1,s.h>>level),0,gl.RGBA,s.bytes===2?gl.HALF_FLOAT:s.bytes===4?gl.FLOAT:gl.UNSIGNED_BYTE,blobs[s.levels[level]]);
    return {handle,id:core.rust.add_texture(handle)};
  };
  T.replay=(records,blobs)=>{
    const metrics=[];T.want=false;
    core.views();const savedPost=core.postInput.slice(),savedHud=core.hudInput.slice();
    const savedArrays=Object.fromEntries(['hudPosition','hudUv','hudMaskUv','hudTint'].map(n=>[n,core[n].slice()]));
    const before=R.getRenderTarget();
    for(const row of records){
      const created=row.resources.map(s=>[s.id,upload(s,blobs)]),ids=new Map(created.map(([old,s])=>[old,s.id]));
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
        core.views();core.postInput.set(input);postFrame(1);
      }else{
        // Restore the exact pre-HUD canvas with a blit, then draw the recorded
        // vertex mirrors, camera uniforms and texture/mask groups.
        const bg=upload({w:row.background.w,h:row.background.h,bytes:1,params:[[gl.TEXTURE_MIN_FILTER,gl.NEAREST],[gl.TEXTURE_MAG_FILTER,gl.NEAREST]],levels:[row.background.blob]},blobs);
        R.resetState();R.setRenderTarget(null);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,bg.handle,0);gl.blitFramebuffer(0,0,row.background.w,row.background.h,0,0,row.background.w,row.background.h,gl.COLOR_BUFFER_BIT,gl.NEAREST);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,null);
        input[1]=program;input[41]=0;input[42]=8192;
        for(let i=0;i<input[0];i++)for(const j of [2,3]){const at=HUD_GROUP+i*5+j;if(input[at]>=0)input[at]=ids.get(input[at]);}
        core.views();core.hudInput.set(input);for(const [name,key] of Object.entries(row.arrays))core[name].set(blobs[key]);hudFrame();gl.deleteTexture(bg.handle);
      }
      const actual=read(target),a=T.blobs[actual.blob],e=blobs[row.output.blob];let differing=0,maxDelta=0;
      for(let i=0;i<a.length;i+=4){let d=0;for(let j=0;j<4;j++)d=Math.max(d,Math.abs(a[i+j]-e[i+j]));if(d){differing++;maxDelta=Math.max(maxDelta,d);}}
      delete T.blobs[actual.blob];metrics.push({key:row.key,differing,maxDelta,pixels:a.length/4});
      for(const [,s] of created)gl.deleteTexture(s.handle);
    }
    core.views();core.postInput.set(savedPost);core.hudInput.set(savedHud);for(const [n,a] of Object.entries(savedArrays))core[n].set(a);core.byName.hud.full=true;
    R.resetState();R.setRenderTarget(before);const error=gl.getError();if(error)throw Error('Phase replay GL '+error);
    return metrics;
  };
  return 1;
}
