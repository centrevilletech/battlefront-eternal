// Graphics.render and BattleHud.render still calculate view inputs during the
// scene transition. This host seals named packets; Rust alone orders passes.
import * as THREE from "three";
import { FACTS as F, STATE as S, BINDING as B, META as M, KIND as K, ROLE as R, LIMIT as L, STATS as T, UI, UI_CAPACITY } from "./frame-layout.js";
import { CAMERA_ROLE as CR } from "./cameras.js";
import { CAMERA as CA, MIRROR as MI, INPUT as CI } from "./camera-layout.js";
import { lighting } from "../materials.js";
import { particleDepth } from "../particle-depth.js";
import { motionVectors } from "../motion-vectors.js";

// Skeleton slots survive view preparation. Clearing a frame increments a
// stamp rather than allocating Map entries again for every visible rig.
class SkeletonSlots {
  constructor(){this.rows=new WeakMap();this.stamp=0;}
  clear(){this.stamp++;}
  get(key){const row=this.rows.get(key);return row?.stamp===this.stamp?row.value:undefined;}
  set(key,value){let row=this.rows.get(key);if(!row){row={stamp:0,value:0};this.rows.set(key,row);}row.stamp=this.stamp;row.value=value;}
}
export class BattleFrame {
  constructor(core) {
    this.core=core;this.active=false;this.role=R.FAR;this.ui=UI.NATIVE;this.uiEnd=UI.LAYER;this.volumeCount=0;
    this.screenRow={steps:1};this.volumeRow={fxWords:0};
    // The reveal helpers prepare facts only. Retaining their arguments and
    // callbacks avoids rebuilding render-tail scaffolding every frame.
    const g=core.graphics;
    this.motionInput={root:g.scene,camera:g.camera,viewScene:null,view:g.viewCamera,depth:null};
    this.shadowProducer=()=>g.renderCascades(g.scene,g.camera);
    this.motionProducer=()=>g.upscaler.drawMotion(core.renderer,this.motionInput);
    // A full UI store still prepares into a retained sink. Publication clips
    // that sink before it can address any native packet or fact row.
    this.contexts=Array.from({length:R.COUNT+1},()=>({renderer:core.renderer,scene:null,camera:null,records:0,bones:0,skeletons:new SkeletonSlots(),walked:null,steps:0,fxWords:0,priming:false}));
    this.uiLimits=new Uint16Array(UI.LEN);
    for(const name of Object.keys(UI_CAPACITY))this.uiLimits[UI[name]]=UI_CAPACITY[name];
    this.metrics=Array.from({length:R.COUNT},()=>new Float64Array(T.LEN));
    this.color=new THREE.Color();this.viewport=new THREE.Vector4();this.scissor=new THREE.Vector4();
    this.current=null;this.face=0;this.level=0;this.pendingClear=0;this.preparingFog=false;
    this.skyPositions=new Float64Array(256*3);this.skyMasks=new Int32Array(256);this.skyVisible=new Uint8Array(256);
    this.resolution=new THREE.Vector2();this.normal=new THREE.Vector3(0,1,0);
    this.reserved=false;
    for(const name of ["facts","state","bindings","meta","stats","lines"]){
      const title="frame"+name[0].toUpperCase()+name.slice(1);
      core.addView(title,Float64Array,()=>core.rust[`frame_${name}_ptr`](),()=>core.rust[`frame_${name}_len`]());
    }
    this.installPreparationHost();
    if(core.graphics.sceneTarget)this.reserve();
    core.phaseStats.post=this.metrics[R.POST];core.phaseStats.lines=this.metrics[R.LINES2];
    // Graphics.updateScene's stamp now reads the native completion counter.
    // Capture clients can reset it explicitly without a second JS counter.
    Object.defineProperty(core.graphics,"frameCount",{configurable:true,
      get:()=>{core.views();return core.frameState[S.FRAME];},
      set:value=>{core.views();core.frameState[S.FRAME]=value;}});
  }
  // Resource descriptors can still arrive during scene preparation. They are
  // admitted cold, but a preparation target never binds a framebuffer.
  installPreparationHost() {
    const core=this.core,r=core.renderer,resources=core.resources;
    const bind=r.setRenderTarget.bind(r),get=r.getRenderTarget.bind(r),face=r.getActiveCubeFace.bind(r),level=r.getActiveMipmapLevel.bind(r);
    r.setRenderTarget=(target,face=0,level=0)=>{if(!this.active)return bind(target,face,level);this.target(target,face,level);};
    r.getRenderTarget=()=>this.active?this.current:get();
    r.getActiveCubeFace=()=>this.active?this.face:face();r.getActiveMipmapLevel=()=>this.active?this.level:level();
    const vp=r.getCurrentViewport.bind(r);r.getCurrentViewport=out=>this.active?out.copy(this.viewport):vp(out);
    const reset=r.resetState.bind(r);r.resetState=()=>{if(!this.active)reset();};
    for(const name of ["clear","clearColor","clearDepth","clearStencil"]){
      const original=r[name].bind(r);
      r[name]=(color=true,depth=true,stencil=true)=>{if(!this.active)return original(color,depth,stencil);
        this.pendingClear|=name==="clearDepth"?0x100:name==="clearStencil"?0x400:name==="clearColor"?0x4000:((color!==false?0x4000:0)|(depth!==false?0x100:0)|(stencil!==false?0x400:0));};
    }
    const copy=resources.copy.bind(resources);
    resources.copy=(source,target,mask,mipmaps=false)=>{if(!this.active)return copy(source,target,mask,mipmaps);this.copyFact(source,target,mask);};
  }
  // NearPass and sky producers consult this preparation view. It carries
  // coordinates only; GpuResources receives the actual bind from Rust.
  target(target,face=0,level=0) {
    const r=this.core.renderer;this.current=target;this.face=face;this.level=level;
    if(target){this.core.resources.targetId(target);this.viewport.copy(target.viewport);this.scissor.copy(target.scissor);}
    else{r.getViewport(this.viewport).multiplyScalar(r.getPixelRatio()).floor();r.getScissor(this.scissor).multiplyScalar(r.getPixelRatio()).floor();}
  }
  // game.frame starts collection before water/sky presentation updates. Even
  // a callback without a main draw can therefore refresh a due sky capture.
  reserve() {
    if(this.reserved)return;
    this.core.rust.frame_reserve_frame();this.reserved=true;
    // The line store moved even if the module already had enough heap pages.
    this.core.buffer=null;this.core.views();
  }
  begin(main) {
    this.reserve();
    const core=this.core;this.active=true;this.uiStage(UI.NATIVE);this.volumeCount=0;this.pendingClear=0;
    core.rust.frame_begin_frame();core.views();core.frameFacts.fill(0);core.frameFacts[F.MAIN]=main?1:0;
    this.target(null);
  }
  // RenderResources.targetId is the sole target owner. These stable handles
  // borrow its attachments, including cube faces, mip levels and scissoring.
  // Register first: a resize or sampler admission may detach shared views.
  binding(role,target=this.current,face=this.face,level=this.level) {
    const core=this.core,r=core.renderer,id=target?core.resources.targetId(target):-1;
    const sample=target?core.resources.textureId(target.texture):-1;
    core.views();core.frameMeta[role*M.LEN+M.BOUND]=1;const b=core.frameBindings,at=role*B.LEN;
    b[at+B.TARGET]=id;b[at+B.FACE]=face;b[at+B.LEVEL]=level;b[at+B.SAMPLE]=sample;
    if(target){this.viewport.copy(target.viewport);this.scissor.copy(target.scissor);}
    else{r.getViewport(this.viewport).multiplyScalar(r.getPixelRatio()).floor();r.getScissor(this.scissor).multiplyScalar(r.getPixelRatio()).floor();}
    b[at+B.X]=this.viewport.x;b[at+B.Y]=this.viewport.y;b[at+B.WIDTH]=this.viewport.z;b[at+B.HEIGHT]=this.viewport.w;
    b[at+B.SCISSOR]=this.scissor.x;b[at+B.SCISSOR+1]=this.scissor.y;b[at+B.SCISSOR+2]=this.scissor.z;b[at+B.SCISSOR+3]=this.scissor.w;
    b[at+B.CLIPPED]=target?+target.scissorTest:+r.getScissorTest();
    r.getClearColor(this.color);const alpha=r.getClearAlpha(),pre=r.getContextAttributes()?.premultipliedAlpha!==false?alpha:1;
    b[at+B.CLEAR]=this.color.r*pre;b[at+B.CLEAR+1]=this.color.g*pre;b[at+B.CLEAR+2]=this.color.b*pre;b[at+B.CLEAR+3]=alpha;
  }
  // RustRender.phase formerly allocated a context and skeleton Map per call.
  // Named packets retain both, while each producer resets its used prefix.
  context(name,scene,camera,walked,values) {
    const role=this.namedRole(name),ctx=this.contexts[Math.min(role,R.COUNT)];
    ctx.scene=scene;ctx.camera=camera;ctx.walked=walked;ctx.records=ctx.bones=ctx.steps=ctx.fxWords=0;ctx.priming=false;ctx.reset=false;ctx.skeletons.clear();
    if(values)Object.assign(ctx,values);return ctx;
  }
  // Names identify input shapes, never execution positions. A shadow or
  // cube producer sets its fixed semantic role before writing camera facts.
  namedRole(name) {
    switch(name){
      case "far":return R.FAR;case "near":return R.NEAR;case "transparent":return R.TRANSPARENT;
      case "view":return R.VIEW;case "motion":return R.MOTION;case "image":return this.role===R.MIRROR0_NEAR?R.MIRROR0_IMAGE:R.IMAGE;
      case "mirror":return R.MIRROR0_NEAR;case "mirrorTransparent":return R.MIRROR0_TRANSPARENT;
      case "shadow":case "probe":case "farMirror":return this.role;
      case "skyClouds":return R.CLOUDS;case "underwater":return R.UNDERWATER;
      case "hud":return this.uiRole();default:return R.NEAR;
    }
  }
  // Preserve bit patterns before another camera reuses the working memory.
  // Standalone interface clients submit one native auxiliary packet; loaded
  // battles keep every packet sealed until their single native execution.
  seal(role,kind,ctx) {
    this.reserve();
    const core=this.core;this.binding(role);core.views();const m=core.frameMeta,at=role*M.LEN;
    m[at+M.KIND]=kind;m[at+M.RECORDS]=ctx?.records||0;m[at+M.BONES]=ctx?.bones||0;m[at+M.STEPS]=ctx?.steps||0;m[at+M.FX_WORDS]=ctx?.fxWords||0;
    core.rust.frame_publish_frame(role);
    if(!this.active){if(!core.rust.frame_aux_frame(core.resources.gpu,role))core.fail(Error(core.rust.frame_error_frame()));core.stale=true;core.views();for(let i=0;i<T.LEN;i++)this.metrics[role][i]=core.frameStats[role*T.LEN+i];}
  }
  // Full UI stores drop later packets, retaining the earlier visible layers.
  publish(name,ctx) {
    if(name==="hud"&&this.active&&this.ui>=this.uiEnd){this.core.rust.frame_overflow_frame();return;}
    const role=this.namedRole(name),kind=name==="hud"?K.HUD:name==="shadow"?K.SHADOW:name==="motion"?K.MOTION:name==="view"?K.VIEW:name==="near"||name==="mirror"?K.SCENE:name==="transparent"||name==="mirrorTransparent"?K.TRANSPARENT:name==="image"||name==="underwater"||name==="skyClouds"?K.SCREEN:K.FAR;
    if(name==="hud")this.uiFacts(kind);
    this.seal(role,kind,ctx);
    this.core.phaseStats[name]=this.metrics[role];
    if(name==="hud"&&this.active)this.ui++;
  }
  // BattleHud.render supplies layer inputs in retained semantic slots. The
  // native owner chooses their binds, clears and draws after the post chain.
  uiRole(){return this.ui<8?R.HUD0+this.ui:R.HUD8+this.ui-8;}
  uiStage(stage){this.ui=stage;this.uiEnd=stage+this.uiLimits[stage];}
  uiFacts(kind) {
    const core=this.core;core.views();const i=this.ui;
    core.frameFacts[F.HUD_KINDS+i]=kind;core.frameFacts[F.HUD_CLEARS+i]=this.pendingClear;
    this.pendingClear=0;
  }
  // Dirty UI streams must be captured before a later preview rewrites them.
  // Admission is cold and reserves storage only for an actual interface view.
  interfaceBegin() {
    if(!this.active)return;
    if(this.ui>=this.uiEnd){this.core.rust.frame_ui_slot_frame(R.COUNT);this.core.rust.frame_overflow_frame();return;}
    const role=this.uiRole();
    this.core.rust.frame_admit_interface_frame(role);this.core.rust.frame_ui_slot_frame(role);
  }
  // InterfacePass has finished geometry/uniform publication. No browser
  // draw follows; the packet retains its own ordered records and dirty bytes.
  interfaceEnd(count) {
    if(this.ui>=this.uiEnd){this.core.rust.frame_overflow_frame();return;}
    const core=this.core,role=this.uiRole();
    this.uiFacts(K.INTERFACE);this.binding(role);core.views();const at=role*M.LEN;
    core.frameMeta[at+M.KIND]=K.INTERFACE;core.frameMeta[at+M.COUNT]=count;
    core.rust.frame_publish_frame(role);this.ui++;
  }
  // NearPass.cover prepares inputs formerly submitted by afterPhase. The
  // far image and fog each need their own screen row before it is reused.
  sceneTail(ctx,near) {
    const core=this.core,mirror=ctx.camera.userData?.mirror===true;
    core.byName.lines.prepare(ctx,mirror?0:2);
    near.writeImage(ctx.camera);this.seal(mirror?R.MIRROR0_IMAGE:R.IMAGE,K.SCREEN,this.screenRow);
    core.byName.transparent.prepare(ctx);
    core.publishView(mirror?"mirrorTransparent":"transparent",ctx.renderer,ctx.scene,ctx.camera,ctx.walked);
  }
  screenFog(){this.seal(R.FOG,K.SCREEN,this.screenRow);}
  // ParticleDepth.copy and Water.captureSurface publish destinations here.
  // Their semantic dependency selects the native copy position and mask;
  // this host never accumulates an ordered list of resource operations.
  copyFact(source,target,mask) {
    const core=this.core,g=core.graphics;
    if(target===g.particleDepth.target){
      this.binding(R.DEPTH,target);core.views();
      if(!this.finalDepth)core.frameFacts[g.particleDepth.mode===2?F.DEPTH_EARLY:F.DEPTH_LATE]=1;
    }else if(target===g.scene.userData.water?.surfaceBuffer){
      this.binding(R.SURFACE,target);core.views();if(!this.preparingFog)core.frameFacts[F.SURFACE_COPY]=1;
    }
    else {this.binding(R.BASIN,target);core.views();core.frameFacts[F.FOUNTAIN_COPY]=1;}
  }
  // EffectsPass.drawVolumes used to draw while collecting transparency.
  // Its offscreen batch is now a retained input of the native near-view tail.
  volume(record,texture,target,count,capacity,words) {
    if(this.volumeCount>=L.VOLUMES){this.core.rust.frame_overflow_frame();return;}
    const core=this.core,role=R.VOLUME0+this.volumeCount++,saved=this.current;
    this.target(target);core.views();const at=role*M.LEN;
    core.frameMeta[at+M.BATCH]=record;core.frameMeta[at+M.TEXTURE]=texture;core.frameMeta[at+M.COUNT]=count;core.frameMeta[at+M.OFFSET]=0;core.frameMeta[at+M.CAPACITY]=capacity;
    core.frameFacts[F.VOLUMES]=this.volumeCount;this.volumeRow.fxWords=words;this.seal(role,K.VOLUME,this.volumeRow);this.target(saved);
  }
  // SkyProbe.update publishes time/eye facts. Native cadence decides whether
  // the six camera packets are needed; there is no JS draw/mipmap loop.
  probe(probe,time,camera) {
    const core=this.core,g=core.graphics,slot=probe.slot;
    if(slot>=L.PROBES || !this.active)return;
    core.views();const f=core.frameFacts;f[F.PROBE_ENABLED+slot]=+(probe.enabled&&probe.layers.length>0);f[F.PROBE_TIME+slot]=time;f[F.PROBE_GENERATION+slot]=probe.generation;camera.position.toArray(f,F.PROBE_EYE+slot*3);
    core.rust.frame_select_frame();core.views();if(!core.frameState[S.PROBE_CAPTURE+slot])return;
    core.cameras.main(camera);core.cameras.sky(probe.layers,CR.MAIN);
    core.rust.camera_probe_frame(slot);
    const cube=probe.camera;core.cameras.bind(cube,CR.PROBE_PARENT0+slot);core.cameras.read(cube,CR.PROBE_PARENT0+slot);
    for(let i=0;i<6;i++){
      const child=cube.children[i],role=CR.PROBE0+slot*6+i;
      core.cameras.bind(child,role);core.cameras.read(child,role);
      this.role=R.PROBE0+slot*6+i;this.target(probe.target,i,cube.activeMipmapLevel);core.cover("probe",core.renderer,g.farScene,child);
    }
    this.target(null);probe.captures++;
  }
  cloud(storm,time,camera) {
    if(!this.active)return;
    const core=this.core;core.views();const f=core.frameFacts;f[F.CLOUDS]=1;f[F.CLOUD_TIME]=time;f[F.CLOUD_GENERATION]=storm.cloudGeneration;camera.position.toArray(f,F.CLOUD_EYE);
    core.rust.frame_select_frame();core.views();if(!core.frameState[S.CLOUD_CAPTURE])return;
    this.target(storm.target);
    core.cover("skyClouds",core.renderer,storm.cloudPass,storm.passCamera);
    storm.uniforms.cloudReady.value=1;storm.cloudRenders=(storm.cloudRenders||0)+1;this.target(null);
  }
  // Water.reflect's camera/clip producer. Eligibility, hold cadence and draw
  // dependencies are native; retained scratch survives both mirrored views.
  mirror(water,camera) {
    if(!water)return;
    const core=this.core,g=core.graphics,r=core.renderer,w=water;
    w.farGroup.visible=false;
    if(w.ocean){w.group.visible=false;w.underwaterFog.visible=false;return;}
    core.cameras.main(camera);w.matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);w.frustum.setFromProjectionMatrix(w.matrix);
    let visible=false;if(w.frustum.intersectsBox(w.bounds))for(const box of w.cellBounds)if(w.frustum.intersectsBox(box)){visible=true;break;}
    const below=camera.position.y<w.level,farVisible=!below&&w.farInView(camera);
    w.group.visible=visible;w.farGroup.visible=farVisible;w.mirrorPass.visible=w.farMirror.visible=false;
    w.surfaceUniforms.cameraRange.value.set(camera.near,camera.far);
    w.resizeReflection();w.forward.fromArray(core.cameraOutput,CR.MAIN*CA.LEN+CA.DIRECTION);
    core.views();const f=core.frameFacts;
    f[F.MIRROR_ENABLED]=1;f[F.MIRROR_VISIBLE]=+visible;f[F.MIRROR_FAR_VISIBLE]=+farVisible;f[F.MIRROR_LOW]=+!!w.lowPass?.visible;
    f[F.MIRROR_NEAR]=+(w.reflectionFar&&w.quality.planarNear);f[F.MIRROR_LEVEL]=w.level;f[F.MIRROR_SIZE]=w.target.width;f[F.MIRROR_SIZE+1]=w.target.height;
    camera.position.toArray(f,F.MIRROR_EYE);w.forward.toArray(f,F.MIRROR_FORWARD);
    this.waterFacts(w,camera);core.rust.frame_select_frame();core.views();
    w.underwaterFog.visible=!!core.frameState[S.FOG];
    if(!core.frameState[S.MIRROR_ELIGIBLE])return;
    if(core.frameState[S.MIRROR_HELD]){w.mirrorPass.visible=w.farMirror.visible=true;return;}
    const mirror=w.mirror;
    core.views();core.cameraInput[CI.WATER_LEVEL]=w.level;core.cameraInput[CI.NEAR_REFLECTION]=+!!w.reflectionFar;
    core.rust.camera_reflect_frame();core.views();
    core.cameras.bind(mirror,CR.MIRROR);core.cameras.read(mirror,CR.MIRROR);
    w.plainProjection.fromArray(core.cameraOutput,CR.MIRROR_PLAIN*CA.LEN+CA.PROJECTION);
    if(!core.cameraMirror[MI.VALID])return;
    w.reflectionMatrix.fromArray(core.cameraMirror,MI.REFLECTION);
    core.views();core.frameFacts[F.MIRROR_CLIP_VALID]=1;
    const layers=g.skyLayers,n=Math.min(layers.length,256),sky=g.sky;
    const sx=sky?.position.x,sy=sky?.position.y,sz=sky?.position.z;
    for(let i=0;i<n;i++){const mesh=layers[i].mesh;mesh.position.toArray(this.skyPositions,i*3);this.skyMasks[i]=mesh.layers.mask;this.skyVisible[i]=+mesh.visible;mesh.position.copy(mirror.position);}
    // A standalone sky is represented in skyLayers by the loader. Preserve
    // its legacy follow position as well when a synthetic scene omits it.
    if(sky)sky.position.copy(mirror.position);

    const bg=g.background.material.uniforms.map.value,farMap=lighting.farMap.value,soft=particleDepth.enabled.value,rings=w.rings.visible;
    this.resolution.copy(lighting.resolution.value);
    const near=!!(w.reflectionFar&&w.quality.planarNear);this.target(near?w.reflectionFar:w.target);
    w.obliqueProjection.copy(mirror.projectionMatrix);
    let skyCount=0;for(let i=0;i<n;i++)if(this.skyVisible[i])skyCount++;
    try {
      if(skyCount){
        core.cameras.read(mirror,CR.MIRROR_PLAIN);core.cameras.roles.set(mirror,CR.MIRROR_PLAIN);mirror.layers.set(5);
        for(let i=0;i<n;i++)if(this.skyVisible[i])layers[i].mesh.layers.enable(5);
        this.role=R.MIRROR0_SKY;core.cover("farMirror",r,g.farScene,mirror);
        for(let i=0;i<n;i++)if(this.skyVisible[i])layers[i].mesh.visible=false;
        mirror.layers.set(0);core.cameras.read(mirror,CR.MIRROR);core.cameras.roles.set(mirror,CR.MIRROR);
      }
      core.views();core.frameFacts[F.MIRROR_SKY]=+(skyCount>0);
      this.role=R.MIRROR0_FAR;core.cover("farMirror",r,g.farScene,mirror);
      for(let i=0;i<n;i++){layers[i].mesh.layers.mask=this.skyMasks[i];layers[i].mesh.visible=!!this.skyVisible[i];}
      if(near){
        w.group.visible=false;w.rings.visible=false;particleDepth.enabled.value=0;
        lighting.resolution.value.set(w.target.width,w.target.height);lighting.farMap.value=w.reflectionFar.texture;g.background.material.uniforms.map.value=w.reflectionFar.texture;
        g.updateScene();this.target(w.target);this.role=R.MIRROR0_NEAR;core.coverMirror(r,g.scene,mirror);
      }
    }finally{
      w.group.visible=visible;w.mirrorPass.visible=w.farMirror.visible=true;w.rings.visible=rings;
      lighting.farMap.value=farMap;lighting.resolution.value.copy(this.resolution);particleDepth.enabled.value=soft;g.background.material.uniforms.map.value=bg;
      this.target(null);
      for(let i=0;i<n;i++){const mesh=layers[i].mesh;mesh.position.fromArray(this.skyPositions,i*3);mesh.layers.mask=this.skyMasks[i];mesh.visible=!!this.skyVisible[i];}
      if(sky)sky.position.set(sx,sy,sz);mirror.layers.set(0);core.cameras.read(mirror,CR.MIRROR);core.cameras.roles.set(mirror,CR.MIRROR);
    }
  }
  // Surface containment is still a camera/math fact. Rust selects the low
  // tint versus the enhanced fog, including the ocean exclusion.
  waterFacts(w,camera) {
    const core=this.core;core.views();const f=core.frameFacts;
    f[F.WATER_SCALE]=w.quality.scale;f[F.WATER_OVER]=+w.over(camera.position.x,camera.position.z);
    f[F.WATER_LEVEL]=w.level;f[F.WATER_EYE_Y]=camera.position.y;f[F.WATER_OCEAN]=+!!w.ocean;
  }
  // Water.drawUnderwater's material is prepared only when native selection
  // requires it. The native operation follows the completed post chain.
  underwater(w,camera) {
    if(!w)return;this.waterFacts(w,camera);this.core.rust.frame_select_frame();this.core.views();
    if(!this.core.frameState[S.UNDERWATER])return;
    const r=this.core.renderer;this.target(null);
    this.core.cover("underwater",r,w.underwaterScene,w.passCamera);
  }
  // A failed producer must not leave the resource host in preparation mode.
  // Context recovery rebuilds resources before another packet can execute.
  cancel() {
    this.active=false;this.core.graphics.sceneFrame=-1;this.core.rust.frame_cancel_frame();
  }
  // The only loaded-battle draw entry point has no scene/draw callbacks.
  // Native completion publishes the motion-history and cascade dirty effects
  // which their current JS numeric producers still consume.
  execute() {
    if(this.core.contextLost){this.cancel();return true;}
    const core=this.core;core.views();
    const started=performance.now(),ok=core.rust.battle_frame(core.resources.gpu);
    core.phaseMs.frame=performance.now()-started;core.views();
    this.active=false;core.stale=true;
    if(!ok && core.gl.isContextLost()){core.rust.frame_error_frame();this.cancel();return true;}
    for(let role=0;role<R.COUNT;role++)for(let i=0;i<T.LEN;i++)this.metrics[role][i]=core.frameStats[role*T.LEN+i];
    if(core.frameState[S.MOTION_COMPLETE])motionVectors.commit();
    if(!ok)core.fail(Error(core.rust.frame_error_frame()));
    const g=core.graphics;
    // A clipped frame retains its native completion stamp. Graphics.updateScene
    // must still revisit changed transforms on the next displayed callback.
    if(core.frameState[S.CLIPPED])g.sceneFrame=-1;
    return true;
  }
}
