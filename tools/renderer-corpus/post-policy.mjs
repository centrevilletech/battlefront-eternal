// Capture before post selection. New builds replay native policy from facts;
// frozen builds replay their recorded operations, never a live backend switch.
export function installPostPolicy(layout, options) {
  const {FACTS:F,STATE:S,RESOURCE:R,MODE:M,AA:A,POST_KEYS:keys}=layout;
  const b=__bf,g=b.graphics,c=g.rust,renderer=g.renderer,T=window.__corpus,gl=renderer.getContext();
  const tools=T.replayTools,native=typeof c.rust.post_chain_frame==='function';
  const mode={off:M.OFF,bilinear:M.BILINEAR,spatial:M.SPATIAL,temporal:M.TEMPORAL};
  const stepSize=221,uniformSize=18;
  let before=null,amount=0,turn=0;
  const motion=g.setVehicleMotion;
  g.setVehicleMotion=function(a,t){amount=a;turn=t;return motion.call(this,a,t);};
  // The old temporal owner is captured before begin mutates valid/previous.
  // This supplies inputs to native camera-cut and jitter decisions as well.
  if(!native){
    const u=g.upscaler,begin=u.begin;
    u.begin=function(camera){
      const s=new Float64Array(S.LEN);
      s[S.READ]=this.read;s[S.FRAME]=this.frame;s[S.VALID]=this.valid?1:0;
      s[S.OUTPUT_WIDTH]=this.output.x;s[S.OUTPUT_HEIGHT]=this.output.y;
      s.set(this.lastEye.toArray(),S.LAST_EYE);s.set(this.lastLook.toArray(),S.LAST_LOOK);
      s.set(this.current.elements,S.CURRENT);s.set(this.previous.elements,S.PREVIOUS);
      before={state:s,projection:camera.projectionMatrix.elements.slice(),view:camera.matrixWorldInverse.elements.slice(),world:camera.matrixWorld.elements.slice()};
      return begin.call(this,camera);
    };
  }
  if(options){
    let point=0;
    if(options.mapBlur!==undefined)g.blur={blend:options.mapBlur,downsize:.25};
    if(options.vehicleAmount!==undefined){
      const render=g.render;g.render=function(...args){g.setVehicleMotion(options.vehicleAmount,options.vehicleTurn);return render.apply(this,args);};
    }
    T.scenarioCapture=async()=>{
      const k=point++;
      if(options.resizeOutput){
        const sizes=[[1600,900],[1280,720],[1920,1080],[1600,900]];
        renderer.setSize(sizes[k][0],sizes[k][1]);g.resizeTargets();
      }
      if(options.resize){
        const modes=['temporal','spatial','bilinear','temporal'],scales=[.5,.75,1.25,1],aa=['off','fxaa','fxaa-fast','off'];
        g.settings.set('upscaler',modes[k]);g.settings.set('renderScale',scales[k]);g.settings.set('antialiasing',aa[k]);
      }
      if(options.restore&&k===1){
        const loss=gl.getExtension('WEBGL_lose_context'),canvas=renderer.domElement;
        if(!loss)throw Error('No context-loss extension');
        const lost=new Promise(resolve=>canvas.addEventListener('webglcontextlost',resolve,{once:true}));loss.loseContext();await lost;
        await new Promise(resolve=>setTimeout(resolve,100));
        const restored=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Post context restore timeout')),30000);canvas.addEventListener('webglcontextrestored',()=>{clearTimeout(timer);resolve();},{once:true});});
        loss.restoreContext();await restored;g.resetHistory();
        for(let i=0;i<4;i++)await b.lockstep(0,true);
      }
    };
  }
  const post=c.post;
  c.post=function(...args){
    if(native||!T.want||!T.captureReplay)return post.apply(this,args);
    const u=g.upscaler,aa=g.settings.get('antialiasing'),f=new Float64Array(F.LEN),temporal=g.upscaling==='temporal',s=temporal?before.state:new Float64Array(S.LEN);
    if(!temporal){s[S.READ]=u.read;s[S.FRAME]=u.frame;s[S.VALID]=u.valid?1:0;}
    f[F.MODE]=mode[g.upscaling];f[F.AA]=aa==='off'?A.OFF:aa==='fxaa-fast'?A.FAST:A.FXAA;
    f[F.SOFT_FOCUS]=g.settings.get('softFocus')?1:0;f[F.BLUR_ENABLED]=g.blur?1:0;f[F.BLUR_BLEND]=g.blur?.blend||0;
    f[F.BLOOM]=g.settings.get('bloom')?1:0;f[F.SPEED_ENABLED]=g.settings.get('vehicleMotionBlur')?1:0;
    f[F.SPEED_AMOUNT]=amount;f[F.SPEED_TURN]=turn;f[F.SHARPNESS]=u.sharpness;f[F.RESET_FRAME]=f[F.RESET_READ]=-1;
    f[F.INPUT_WIDTH]=g.renderSize.x;f[F.INPUT_HEIGHT]=g.renderSize.y;f[F.OUTPUT_WIDTH]=g.outputSize.x;f[F.OUTPUT_HEIGHT]=g.outputSize.y;
    const viewport=renderer.getViewport(new b.THREE.Vector4()).multiplyScalar(renderer.getPixelRatio()).floor();
    f[F.CANVAS_WIDTH]=viewport.z;f[F.CANVAS_HEIGHT]=viewport.w;
    f.set(before?.projection||g.camera.projectionMatrix.elements,F.PROJECTION);f.set(before?.view||g.camera.matrixWorldInverse.elements,F.VIEW);f.set(before?.world||g.camera.matrixWorld.elements,F.WORLD);
    const targets=[g.sceneTarget,g.quarterTarget,g.bloom.target,g.bloom.scratch,g.postTarget,g.aaTarget,u.history[0],u.history[1],u.velocity,u.motion,null];
    // A non-temporal resize can retire the last motion depth descriptor.
    // It is not an input to that chain and must not be snapshotted.
    const textures=targets.map(t=>t?.texture||(temporal?u.motionPass.material.uniforms.depthMap.value:null));
    const bindings=[],objects=new Map();
    for(let i=0;i<targets.length;i++){
      // Depth is a sampler, never a colour framebuffer attachment.
      const target=i===R.DEPTH?-1:c.byName.post.target(targets[i]);
      const texture=textures[i],id=texture?c.texture(texture):-1,image=texture?.image;
      const width=targets[i]?.width||image?.width||1,height=targets[i]?.height||image?.height||1;
      const at=F.RESOURCES+i*R.STRIDE;f[at+R.TEXTURE]=id;f[at+R.TARGET]=target;f[at+R.WIDTH]=width;f[at+R.HEIGHT]=height;
      bindings.push({id,target,width,height});
      if(texture)objects.set(id,{texture,handle:renderer.properties.get(texture).__webglTexture});
    }
    const resources=bindings.map((r,i)=>r.id<0?null:({...tools.snapshot(r.id,objects),sampledDepth:i===R.DEPTH}));
    c.stale=true;c.settle();
    const record={kind:'post-policy',key:'post-policy',facts:T.store(f),state:T.store(s),bindings,resources,steps:[],output:null};
    const at=T.records.length;T.records.push(record);
    const result=post.apply(this,args);
    for(const row of T.records.slice(at+1))if(row.kind==='post')record.steps.push({key:row.key,input:T.store(Float64Array.from(row.input))});
    const last=T.records.at(-1);if(last.kind!=='post'||!last.canvas)throw Error('Post policy did not finish on the canvas');
    record.output=last.output;return result;
  };
  // Graphics invoked the old after/commit closure only after post returned.
  // Capture that completed state after the whole battle frame, not mid-chain.
  T.completePostPolicy=()=>{
    if(native||!T.captureReplay)return;
    const row=T.records.find(r=>r.kind==='post-policy');if(!row)return;
    const u=g.upscaler,s=new Float64Array(S.LEN),indices=[S.READ,S.FRAME,S.VALID];
    s[S.READ]=u.read;s[S.FRAME]=u.frame;s[S.VALID]=u.valid?1:0;
    if(g.upscaling==='temporal'){
      s.set(u.lastEye.toArray(),S.LAST_EYE);s.set(u.lastLook.toArray(),S.LAST_LOOK);
      s.set(u.current.elements,S.CURRENT);s.set(u.previous.elements,S.PREVIOUS);
      s.set(u.inverseJittered.elements,S.INVERSE_JITTERED);s[S.JITTER]=u.jitter.x;s[S.JITTER+1]=u.jitter.y;
      s[S.PHASES]=u.phases;
      for(let i=S.LAST_EYE;i<S.JITTER+2;i++)indices.push(i);indices.push(S.PHASES);
    }
    row.committed=T.store(s);row.stateIndices=indices;
  };
  const activeWords=input=>{
    const out=[];for(let i=0;i<5;i++)out.push(input[i]);
    for(let k=0;k<input[4];k++){const at=5+k*uniformSize,ty=input[at+1],n=ty>=32||ty===5?1:ty;for(let i=0;i<n+2;i++)out.push(input[at+i]);}
    return out;
  };
  T.replayPostPolicy=async(row,blobs)=>{
    const savedPost=c.postInput.slice(),savedFacts=native?c.postFacts.slice():null,savedState=native?c.postState.slice():null;
    const previousTarget=renderer.getRenderTarget(),created=[];
    try{
      const facts=blobs[row.facts].slice(),state=blobs[row.state].slice(),textures=new Map(),targets=new Map();
      for(let i=0;i<row.bindings.length;i++){
        const old=row.bindings[i],image=row.resources[i];
        if(!image)continue;
        const upload=tools.upload(image,blobs);created.push(upload);textures.set(old.id,upload.id);
        const target=i===R.DEPTH?-1:c.rust.post_target(upload.id);
        if(i!==R.DEPTH)targets.set(old.target,target);
        const at=F.RESOURCES+i*R.STRIDE;facts[at+R.TEXTURE]=upload.id;facts[at+R.TARGET]=target;
      }
      const programs=new Map();for(let i=0;i<keys.length;i++){const id=c.rust.add_owned_program(keys[i]);facts[F.PROGRAMS+i]=id;programs.set(keys[i],id);}
      const expected=row.steps.map(step=>{
        const input=blobs[step.input].slice();input[0]=programs.get(step.key);if(input[1]>=0)input[1]=targets.get(input[1]);
        for(let k=0;k<input[4];k++){const at=5+k*uniformSize;if(input[at+1]===32&&input[at+2]>=0)input[at+2]=textures.get(input[at+2]);}
        return input;
      });
      c.views();
      if(native){
        c.postFacts.set(facts);c.postState.set(state);c.rust.post_chain_begin_frame();
        const count=c.rust.post_chain_plan_frame();c.views();
        if(count!==expected.length)throw Error(`Post policy step count ${count}/${expected.length}`);
        for(let i=0;i<count;i++){
          const actual=activeWords(c.postInput.subarray(i*stepSize,(i+1)*stepSize)),wanted=activeWords(expected[i]);
          if(actual.length!==wanted.length)throw Error('Post policy uniform layout');
          for(let k=0;k<actual.length;k++)if(!Object.is(actual[k],wanted[k]))throw Error(`Post policy input ${i}/${k}: ${actual[k]}/${wanted[k]}`);
        }
        c.rust.post_chain_frame();c.views();
        const committed=blobs[row.committed];
        for(const i of row.stateIndices)if(!Object.is(c.postState[i],committed[i]))throw Error(`Post commit state ${i}: ${c.postState[i]}/${committed[i]}`);
      }else{
        // Frozen self/control proves the same initial sampler snapshots can
        // reproduce the entire chain, including all intermediate attachments.
        for(const input of expected){c.views();c.postInput.set(input);c.rust.post_frame(1);}
      }
      const output=tools.read(null,false),comparison=tools.rgbaMetrics(output.buf,blobs[row.output.blob]);
      const error=gl.getError();if(error)throw Error('Post policy GL '+error);
      return {key:'post-policy',planExact:true,...comparison};
    }finally{
      c.views();c.postInput.set(savedPost);if(native){c.postFacts.set(savedFacts);c.postState.set(savedState);}
      for(const source of created)source.dispose();renderer.resetState();renderer.setRenderTarget(previousTarget);c.stale=true;
    }
  };
  return 1;
}
