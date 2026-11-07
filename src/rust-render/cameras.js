// Graphics' camera shells expose native publications to audio, controls and
// the scene consumers awaiting registry admission. They never solve a view.
import * as THREE from "three";
import { lighting, bindLighting } from "../materials.js";
import { INPUT as I, CAMERA as C, CASCADE as W, LENS as N, ROLE as R, SKY as Y, LIMIT as L, LIGHT_INPUT as LI, LIGHT_OUTPUT as LO, LIGHT_COMMAND as LC, LIGHT_FLAG as LF, SOURCE as U } from "./camera-layout.js";
const noop=()=>{};
const lensNames=["fov","aspect","near","far","zoom","filmGauge","filmOffset"];
const lightNames=["sunColor","ambient","ambientTop","ambientBottom","characterAmbient","fogColor"];
const lightDefaults=[128,128,128],sunDefault=[0,-1,0];
const viewNames=["fullWidth","fullHeight","width","height","offsetX","offsetY"];
export class CameraOwner {
  // Stable roles and fixed native views survive every displayed callback.
  // Weak keys follow the lifetime of the existing graph readback shells.
  constructor(core){
    this.core=core;core.renderer.nativeCameraOwner=this;this.roles=new WeakMap();this.bindings=new WeakMap();this.shells=new Array(R.COUNT);this.nextPack=0;this.shifted=new Uint8Array(R.COUNT);
    for(const name of ["input","output","windows","sky","lights","packs","mirror"]){
      const key="camera"+name[0].toUpperCase()+name.slice(1);
      core.addView(key,Float64Array,()=>core.rust[`camera_${name}_ptr`](),()=>core.rust[`camera_${name}_len`]());
    }
    core.addView("cameraLightInput",Float64Array,()=>core.rust.camera_light_input_ptr(),()=>core.rust.camera_light_input_len());
    core.addView("cameraLightOutput",Float64Array,()=>core.rust.camera_light_output_ptr(),()=>core.rust.camera_light_output_len());
    core.views();const g=core.graphics;
    this.bind(g.camera,R.MAIN,false);this.bind(g.farCamera,R.FAR);this.bind(g.viewCamera,R.VIEW);
    for(let i=0;i<(g.csm?.lights.length||0);i++)this.bind(g.csm.lights[i].shadow.camera,R.SHADOW0+i);
    for(let i=0;i<(g.cascadeWindows?.length||0);i++)this.bindWindow(g.cascadeWindows[i],i);
    this.configureShadows();this.seedLights();bindLighting(this);if(g.environmentFacts)this.environment(g.environmentFacts);
    g.camera.updateProjectionMatrix=()=>this.project(g.camera);
    this.bind(g.cascadeCamera,R.CASCADE);this.bind(g.shadowCamera,R.SHADOW_DYNAMIC);this.bind(g.staticShadowCamera,R.SHADOW_STATIC);
  }
  // Derived shells carry layers and resource references for scene consumers.
  // Automatic matrix updates would overwrite the native publication.
  bind(camera,role,derived=true){
    if(!camera)return;
    this.bindings.set(camera,role);this.shells[role]=camera;
    if(derived){camera.matrixAutoUpdate=false;camera.matrixWorldAutoUpdate=false;camera.updateMatrixWorld=noop;camera.updateWorldMatrix=noop;camera.updateProjectionMatrix=noop;}
  }
  // Light uniforms remain readback facades for authored materials. Their
  // arithmetic and shader publication use the same native settings owner.
  // Preserve the pre-map defaults when a scene client starts before content.
  // Map admission subsequently replaces only its authored light fields.
  seedLights(){
    const core=this.core;core.views();const a=core.cameraLightInput;
    lighting.sunDirection.value.toArray(a,LI.DIRECTION);lighting.fogRange.value.toArray(a,LI.RANGE);a[LI.STRENGTH]=lighting.shadowStrength.value;
    for(let i=0;i<lightNames.length;i++)lighting[lightNames[i]].value.toArray(a,LI.COLORS+i*3);
    core.rust.camera_light_command_frame(LC.SEED);
  }
  // Keep the uniform vector identities: imported materials share them.
  // Replacing a vector would strand those cold material bindings.
  readLights(){
    const core=this.core;core.views();const a=core.cameraLightOutput,g=core.graphics;
    lighting.sunDirection.value.fromArray(a,LO.DIRECTION);g.sunDirection?.fromArray(a,LO.DIRECTION);g.csm?.lightDirection?.fromArray(a,LO.DIRECTION);
    for(let i=0;i<lightNames.length;i++)lighting[lightNames[i]].value.fromArray(a,LO.COLORS+i*3);
    lighting.fogRange.value.fromArray(a,LO.RANGE);lighting.shadowStrength.value=a[LO.STRENGTH];
  }
  // setLighting uses engine defaults for missing map fields. Defaults are
  // raw RGB facts here; division and direction normalization stay native.
  mapLighting(map){
    const core=this.core,l=map.lighting||{};core.views();const a=core.cameraLightInput;
    a.set(l.sunDirection||sunDefault,LI.DIRECTION);
    a.set(l.sunColor||lightDefaults,LI.COLORS);a.set(l.ambient||lightDefaults,LI.COLORS+3);a.set(l.top||lightDefaults,LI.COLORS+6);a.set(l.bottom||lightDefaults,LI.COLORS+9);a.set(l.character||l.ambient||lightDefaults,LI.COLORS+12);
    core.rust.camera_light_command_frame(LC.MAP);this.readLights();
  }
  // setFog keeps the last colour when a map omits LowResTerrain.
  // Rust decides whether its authored distance pair enables any fog.
  mapFog(map){
    const core=this.core,low=map.distance?.lowRes;core.views();const a=core.cameraLightInput;
    a[LI.FLAGS]=+!!low?.fog;if(low?.fog)a.set(low.fog,LI.COLORS+15);if(low?.fogRange)a.set(low.fogRange,LI.RANGE);else{a[LI.RANGE]=NaN;a[LI.RANGE+1]=NaN;}
    core.rust.camera_light_command_frame(LC.FOG);this.readLights();
  }
  // Graphics.setEnvironment is sparse: an omitted field keeps its value.
  // Flags carry that distinction without manufacturing replacement colours.
  environment(facts){
    const core=this.core;core.views();const a=core.cameraLightInput;let flags=0;
    if(facts.sunDirection){a.set(facts.sunDirection,LI.DIRECTION);flags|=LF.DIRECTION;}
    for(let i=0;i<lightNames.length;i++){const key=i===2?"top":i===3?"bottom":i===4?"character":lightNames[i],rgb=facts[key];if(rgb){a.set(rgb,LI.COLORS+i*3);flags|=1<<i;}}
    if(facts.fogRange){a.set(facts.fogRange,LI.RANGE);flags|=LF.RANGE;}
    if(facts.shadowStrength!==undefined){a[LI.STRENGTH]=facts.shadowStrength;flags|=LF.STRENGTH;}a[LI.FLAGS]=flags;
    core.rust.camera_light_command_frame(LC.ENVIRONMENT);this.readLights();
    if(facts.sunDirection)for(const window of core.graphics.cascadeWindows)window.invalidate();
  }
  // StormSky publishes only the four ambient vectors touched by a flash.
  // Sun/fog and CSM readbacks do not change with lightning intensity.
  readFlash(){this.core.views();for(let i=1;i<5;i++)lighting[lightNames[i]].value.fromArray(this.core.cameraLightOutput,LO.COLORS+i*3);}
  flash(amount){this.core.views();this.core.cameraLightInput[LI.FLASH]=amount;this.core.rust.camera_light_command_frame(LC.FLASH);this.readFlash();}
  restoreLight(){this.core.rust.camera_light_command_frame(LC.RESTORE);this.readFlash();}
  // Raw pose/lens facts preserve externally forced camera poses. Native
  // PageCameras matrices bypass the Three facade on their way to this owner.
  main(camera){
    const core=this.core;core.views();const a=core.cameraInput;
    camera.position.toArray(a,I.POSITION);camera.quaternion.toArray(a,I.QUATERNION);camera.scale.toArray(a,I.SCALE);camera.up.toArray(a,I.UP);
    for(let i=0;i<lensNames.length;i++)a[I.LENS+i]=camera[lensNames[i]];
    const view=camera.view;a[I.LENS+N.VIEW_ENABLED]=+(!!view?.enabled);
    if(view)for(let i=0;i<viewNames.length;i++)a[I.LENS+N.FULL_WIDTH+i]=view[viewNames[i]];
    const source=camera.userData.nativeView;a[I.PUBLISHED]=+!!source;
    if(source)a.set(source,I.MATRICES);
    const changed=core.rust.camera_main_frame();
    // PageCameras has already published its readback. Import that record once
    // without copying the same matrices back into the same Three facade.
    if(source)this.roles.set(camera,R.MAIN);else if(changed)this.read(camera,R.MAIN);
    const g=core.graphics;a[I.FAR_RANGE]=g.farCamera.near;a[I.FAR_RANGE+1]=g.farCamera.far;
    a[I.VIEW_RANGE]=g.viewCamera.near;a[I.VIEW_RANGE+1]=g.viewCamera.far;
  }
  // Resize changes lens facts without advancing gameplay camera history.
  project(camera){
    this.main(camera);this.core.rust.camera_project_frame();this.read(camera,R.MAIN);
    const source=camera.userData.nativeView;if(source){const a=this.core.cameraOutput;for(let i=0;i<16;i++){source[U.PROJECTION+i]=a[C.PROJECTION+i];source[U.PROJECTION_INVERSE+i]=a[C.PROJECTION_INVERSE+i];}}
  }
  // Quaternion readback bypasses Euler callbacks and their trigonometry.
  // Gameplay Euler values already come from PageCameras; derived views have
  // no gameplay turn solver consuming a second rotation representation.
  read(camera,role=this.bindings.get(camera)){
    if(role===undefined||!camera)return;
    this.roles.set(camera,role);
    const core=this.core;core.views();const a=core.cameraOutput,at=role*C.LEN;
    camera.position.fromArray(a,at+C.POSITION);camera.up.fromArray(a,at+C.UP);
    const q=camera.quaternion;q._x=a[at+C.QUATERNION];q._y=a[at+C.QUATERNION+1];q._z=a[at+C.QUATERNION+2];q._w=a[at+C.QUATERNION+3];
    camera.matrix.fromArray(a,at+C.WORLD);camera.matrixWorld.fromArray(a,at+C.WORLD);
    // CubeCamera's parent is an Object3D. Only its six children carry lens,
    // inverse and projection fields; the parent publishes pose alone.
    if(camera.isCamera){
      for(let i=0;i<lensNames.length;i++)camera[lensNames[i]]=a[at+C.LENS+i];
      camera.matrixWorldInverse.fromArray(a,at+C.INVERSE);
      camera.projectionMatrix.fromArray(a,at+C.PROJECTION);camera.projectionMatrixInverse.fromArray(a,at+C.PROJECTION_INVERSE);
    }
    camera.matrixWorldNeedsUpdate=false;
  }
  // Graphics.prepareFrame publishes these views at their original boundaries.
  // Shader blocks borrow their native rows directly, before another view.
  projection(camera,role){
    this.core.views();const a=this.core.cameraOutput,at=role*C.LEN;
    camera.projectionMatrix.fromArray(a,at+C.PROJECTION);camera.projectionMatrixInverse.fromArray(a,at+C.PROJECTION_INVERSE);
  }
  derived(camera,role){this.core.rust.camera_derived_frame(role);this.read(camera,role);}
  copy(camera,from,to){this.core.rust.camera_copy_frame(from,to);this.read(camera,to);}
  // Upscaler.restore visits only cameras actually shifted in this frame.
  // Reading unused rows could replace an idle view camera's near/far range.
  shift(camera){const core=this.core,role=this.bindings.get(camera);if(role===undefined)return;core.views();core.cameraInput[I.SHIFT]=core.postState[core.graphics.upscaler.shiftOffset];core.cameraInput[I.SHIFT+1]=core.postState[core.graphics.upscaler.shiftOffset+1];this.shifted[role]=1;core.rust.camera_shift_frame(role);this.projection(camera,role);}
  restore(){this.core.rust.camera_restore_frame();for(let i=0;i<R.COUNT;i++)if(this.shifted[i]){this.projection(this.shells[i],i);this.shifted[i]=0;}}
  // Sky movement is computed in the camera owner. Existing mesh identities
  // and visibility remain graph facts until the scene registry consumes them.
  sky(layers,role,follow=true){
    const core=this.core;core.views();const a=core.cameraInput,n=Math.min(layers.length,L.SKY);a[I.SKY_COUNT]=n;
    for(let i=0;i<n;i++){const f=layers[i].follow,at=I.SKY_FOLLOW+i*Y.LEN;a[at+Y.FOLLOW]=+!!f;a[at+Y.SCALE]=f?.[0]??0;a[at+Y.OFFSET]=f?.[1]??0;}
    core.rust.camera_sky_frame(role,follow);for(let i=0;i<n;i++)layers[i].mesh.position.fromArray(core.cameraSky,i*3);
  }
  // Completion and invalidation change these flags natively. Readback
  // accessors observe that publication even before another view is prepared.
  bindWindow(window,index){
    const core=this.core,at=index*W.LEN;
    Object.defineProperties(window,{
      held:{configurable:true,get(){core.views();return !!core.cameraWindows[at+W.HELD];}},
      dirty:{configurable:true,get(){core.views();return !!core.cameraWindows[at+W.DIRTY];}},
      moved:{configurable:true,get(){core.views();return !!core.cameraWindows[at+W.MOVED];}},
    });
  }
  // Cold light/map shells retain resource lifetime only. CSM split, fitting,
  // window placement and shadow camera matrices have one native owner.
  // Resource recreation replaces light matrices before any possible mirror;
  // receiver slices still belong to the previous main-view publication.
  configureShadows(){
    const core=this.core,g=core.graphics,csm=g.csm;core.views();const a=core.cameraInput;
    a[I.CASCADE_COUNT]=csm?.lights.length||0;if(!csm){core.rust.camera_shadow_configure_frame();return;}a[I.MAP_SIZE]=csm.shadowMapSize;a[I.MAX_FAR]=csm.maxFar;a[I.LIGHT_RANGE]=csm.lightNear;a[I.LIGHT_RANGE+1]=csm.lightFar;a[I.MARGIN]=csm.lightMargin;a[I.RESET]=g.shadowReset;csm.lightDirection.toArray(a,I.SUN);
    core.rust.camera_shadow_configure_frame();
  }
  cascades(){
    this.configureShadows();const core=this.core,g=core.graphics,csm=g.csm;
    core.rust.camera_cascades_frame();this.read(g.cascadeCamera,R.CASCADE);
    const out=core.cameraWindows;
    for(let i=0;i<csm.lights.length;i++){
      const at=i*W.LEN,light=csm.lights[i],window=g.cascadeWindows[i],camera=light.shadow.camera;
      window.center.fromArray(out,at+W.CENTER);window.half=out[at+W.HALF];
      csm.breaks[i]=out[at+W.BREAKS+1];lighting.cascades.value[i]?.fromArray(out,at+W.BREAKS);
      light.position.fromArray(out,at+W.POSITION);light.target.position.fromArray(out,at+W.TARGET);
      // Translation-only light/target facades are retained for scene readers.
      light.matrixWorld.elements[12]=light.position.x;light.matrixWorld.elements[13]=light.position.y;light.matrixWorld.elements[14]=light.position.z;
      light.target.matrixWorld.elements[12]=light.target.position.x;light.target.matrixWorld.elements[13]=light.target.position.y;light.target.matrixWorld.elements[14]=light.target.position.z;
      camera.left=out[at+W.EXTENT];camera.right=out[at+W.EXTENT+1];camera.top=out[at+W.EXTENT+2];camera.bottom=out[at+W.EXTENT+3];
      // Fitting publishes the projection now; the shadow pass publishes pose.
      const row=(R.SHADOW0+i)*C.LEN;camera.projectionMatrix.fromArray(core.cameraOutput,row+C.PROJECTION);camera.projectionMatrixInverse.fromArray(core.cameraOutput,row+C.PROJECTION_INVERSE);
    }
    lighting.cascadeNear.value=out[W.RANGE];lighting.cascadeFar.value=out[W.RANGE+1];
  }
  // LightShadow.updateMatrices used this late boundary. Only selected passes
  // advance the receiver matrix; a held reflection reads the preceding one.
  shadow(light){
    const core=this.core,index=light.userData.nativeCascade,role=R.SHADOW0+index;
    core.rust.camera_shadow_frame(index);this.read(light.shadow.camera,role);core.views();const at=index*W.LEN;
    light.shadow.matrix.fromArray(core.cameraWindows,at+W.MATRIX);lighting.cascadePlanes.value[index].fromArray(core.cameraWindows,at+W.PLANE);
    const planes=light.shadow.getFrustum().planes,start=role*C.LEN+C.FRUSTUM;
    for(let i=0;i<6;i++){planes[i].normal.fromArray(core.cameraOutput,start+i*4);planes[i].constant=core.cameraOutput[start+i*4+3];}
  }
}
export { R as CAMERA_ROLE };

// DirectionalLight and target shells retain cold resource lifetime only.
// Split, window, camera and receiver-plane operations belong to Rust.
export function shadowShells(g,preset){
    g.shadowReset=(g.shadowReset||0)+1;
    const csm={lights:[],breaks:[],lightDirection:g.sunDirection.clone(),shadowMapSize:preset.size,maxFar:preset.far,lightNear:1,lightFar:3000,lightMargin:400,
      remove(){for(const light of this.lights){g.scene.remove(light);g.scene.remove(light.target);}},dispose:noop};
    for(let i=0;i<preset.cascades;i++){
      const light=new THREE.DirectionalLight(0xffffff,3);light.castShadow=true;light.userData.nativeCascade=i;
      light.shadow.mapSize.set(preset.size,preset.size);light.shadow.bias=-.015/2999;light.shadow.normalBias=0;light.shadow.camera.near=1;light.shadow.camera.far=3000;
      light.matrixAutoUpdate=light.matrixWorldAutoUpdate=false;light.target.matrixAutoUpdate=light.target.matrixWorldAutoUpdate=false;
      g.rust?.cameras.bind(light.shadow.camera,R.SHADOW0+i);g.scene.add(light);g.scene.add(light.target);csm.lights.push(light);
      const window={center:new THREE.Vector3(),half:0,held:false,dirty:true,moved:false,invalidate(){g.rust?.rust.camera_invalidate_frame(i);}};g.cascadeWindows.push(window);g.rust?.cameras.bindWindow(window,i);
    }
    return csm;
  }
