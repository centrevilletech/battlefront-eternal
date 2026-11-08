import {sceneProperty,sceneTransform} from "./scene-changes.js";
// Audio and prediction read the retained camera facades. PageAudio and the
// gameplay camera owner remove those queries; draws use native row identities.
import * as THREE from "three";
import { lighting, bindLighting } from "../materials.js";
import { INPUT as I, CAMERA as C, CASCADE as W, LENS as N, ROLE as R, SKY as Y, LIMIT as L, LIGHT_INPUT as LI, LIGHT_OUTPUT as LO, LIGHT_COMMAND as LC, LIGHT_FLAG as LF, SOURCE as U } from "./camera-layout.js";
import {DEPTH_MARKER as DM,DEPTH_KEY as DK,MIRROR as MI,UNIFORM_INPUT as UI,UNIFORM_OUTPUT as UO} from "./camera-layout.js";
const noop=()=>{};
const lensNames=["fov","aspect","near","far","zoom","filmGauge","filmOffset"];
const lensDefaults=[60,1,.5,200,1,35,0];
const lightNames=["sunColor","ambient","ambientTop","ambientBottom","characterAmbient","fogColor"];
const lightDefaults=[128,128,128],sunDefault=[0,-1,0];
const matrixFields=["matrix","matrixWorld","matrixWorldInverse","projectionMatrix","projectionMatrixInverse"];
const matrixOffsets=[C.WORLD,C.WORLD,C.INVERSE,C.PROJECTION,C.PROJECTION_INVERSE];
const viewNames=["fullWidth","fullHeight","width","height","offsetX","offsetY"];
// Graph keys are weak. A collected pair gives its lease back; generations
// make a delayed finalizer harmless after the bounded bank has reused it.
const lightPacks=new FinalizationRegistry(record=>{const owner=record.owner.deref();if(owner)owner.core.rust.camera_pack_release_frame(record.handle);});
const extraCameras=new FinalizationRegistry(record=>{const owner=record.owner.deref();if(owner)owner.core.rust.camera_release_frame(record.handle);});
export class CameraOwner {
  // Stable roles and fixed native views survive every displayed callback.
  // Weak keys follow the lifetime of the existing graph readback shells.
  constructor(core){
    this.core=core;core.renderer.nativeCameraOwner=this;this.roles=new WeakMap();this.bindings=new WeakMap();this.shells=new Array(R.COUNT);this.weak=new WeakRef(this);this.extra=new WeakMap();this.shifted=new Uint8Array(R.COUNT);
    for(const name of ["input","output","windows","sky","lights","packs","mirror","placement","uniformInput","uniformOutput","projectionView","depthKey","depthMarker","unitView"]){
      const key="camera"+name[0].toUpperCase()+name.slice(1),native=name.replace(/[A-Z]/g,c=>"_"+c.toLowerCase());
      core.addView(key,Float64Array,()=>core.rust[`camera_${native}_ptr`](),()=>core.rust[`camera_${native}_len`]());
    }
    core.addView("cameraLightInput",Float64Array,()=>core.rust.camera_light_input_ptr(),()=>core.rust.camera_light_input_len());
    core.addView("cameraLightOutput",Float64Array,()=>core.rust.camera_light_output_ptr(),()=>core.rust.camera_light_output_len());
    core.addView("cameraPackHandles",Float64Array,()=>core.rust.camera_pack_handles_ptr(),()=>core.rust.camera_pack_handles_len());
    core.addView("cameraExtraHandles",Float64Array,()=>core.rust.camera_extra_handles_ptr(),()=>core.rust.camera_extra_handles_len());
    core.views();const g=core.graphics;
    this.bind(g.camera,R.MAIN,false);this.bind(g.farCamera,R.FAR);this.bind(g.viewCamera,R.VIEW);
    for(let i=0;i<(g.csm?.lights.length||0);i++)this.bind(g.csm.lights[i].shadow.camera,R.SHADOW0+i);
    for(let i=0;i<(g.cascadeWindows?.length||0);i++)this.bindWindow(g.cascadeWindows[i],i);
    this.depthProjection(g.particleDepth);this.configureShadows();this.seedLights();bindLighting(this);if(g.environmentFacts)this.environment(g.environmentFacts);
    // The front end builds this renderer without a battle camera.
    if(g.camera)g.camera.updateProjectionMatrix=()=>this.project(g.camera);
    this.bind(g.cascadeCamera,R.CASCADE);this.bind(g.shadowCamera,R.SHADOW_DYNAMIC);this.bind(g.staticShadowCamera,R.SHADOW_STATIC);
  }
  // Body visibility samples the camera before its current pose update, as
  // game.frame did. The unit phase consumes these native planes directly.
  unitView(view,camera){
    if(this.bindings.get(camera)===R.MAIN)this.main(camera);
    this.core.rust.camera_unit_view_frame(this.query(camera));
    if(!view.nativeView){const core=this.core;Object.defineProperty(view,'planes',{get(){core.views();return core.cameraUnitView;}});view.nativeView=true;}
  }
  // ParticleDepth retains resource lifetime and request callbacks. Its host
  // delegates the view-dependent marker to the native camera owner; the depth
  // resource owner removes these call adapters when it removes that host.
  depthProjection(depth){
    if(!depth?.requesters)return;const owner=this;let buffer=null,address=-1,view=null;
    const matrix=()=>{const core=owner.core;core.views();const values=core.cameraDepthMarker,at=values.byteOffset+DM.MATRIX*8;if(buffer!==values.buffer||address!==at){buffer=values.buffer;address=at;view=new Float64Array(buffer,address,16);}return view;};
    depth.place=function(renderer,scene,camera,target){
      const copier=this.copier;if(target!==this.source||camera!==this.camera||!this.pending){sceneProperty(copier,'visible',false);return;}
      const core=owner.core;this.armed=false;core.rust.camera_depth_begin_frame(owner.query(camera));this.placing=true;
      try{for(const request of this.requesters)request(camera);}finally{this.placing=false;}
      core.rust.camera_depth_finish_frame();core.views();const output=core.cameraDepthMarker;this.requests=output[DM.REQUESTS];
      sceneProperty(copier,'visible',false);if(!output[DM.VISIBLE])return;
      if(this.nativeMarker!==copier){for(const target of [copier.matrixWorld,copier.matrix])Object.defineProperty(target,'elements',{configurable:true,get:matrix});this.nativeMarker=copier;}
      sceneProperty(this.group,'renderOrder',output[DM.GROUP]);sceneProperty(copier,'renderOrder',output[DM.ORDER]);sceneProperty(copier,'visible',true);this.armed=true;
    };
    depth.request=function(key,required=false){if(!this.placing||(!required&&!this.enabled))return false;const core=owner.core;core.views();for(let i=0;i<DK.LEN;i++)core.cameraDepthKey[i]=key[i];return core.rust.camera_depth_request_frame();};
  }
  // Derived shells carry layers and resource references for scene consumers.
  // Automatic matrix updates would overwrite the native publication.
  bind(camera,role,derived=true){
    if(!camera)return;
    this.bindings.set(camera,role);this.shells[role]=camera;
    if(derived){
      sceneProperty(camera,"matrixAutoUpdate",false);sceneProperty(camera,"matrixWorldAutoUpdate",false);
      camera.updateMatrixWorld=noop;camera.updateWorldMatrix=noop;camera.updateProjectionMatrix=noop;
      this.facades??=new WeakSet();
      if(!this.facades.has(camera)){
        const owner=this;
        for(let i=0;i<matrixFields.length;i++){
          const matrix=camera[matrixFields[i]];if(!matrix)continue;
          const offset=matrixOffsets[i];
          Object.defineProperty(matrix,"elements",{configurable:true,get(){return CameraOwner.prototype.matrix.call(owner,camera,offset);}});
        }
        this.facades.add(camera);
      }
    }
  }
  // Derived camera facades borrow the native f64 publication. Rebinding is
  // cold after heap growth; ordinary draws never copy matrices through Three.
  matrix(camera,offset){
    const core=this.core;core.views();const role=this.roles.get(camera)??this.bindings.get(camera);
    this.matrixRows??=new Array(R.COUNT);let rows=this.matrixRows[role];
    const values=core.cameraOutput;
    if(!rows||rows.buffer!==values.buffer||rows.address!==values.byteOffset){
      rows={buffer:values.buffer,address:values.byteOffset,values:[]};
      for(let i=0;i<matrixOffsets.length;i++){
        const field=matrixOffsets[i];rows.values[field]=new Float64Array(values.buffer,values.byteOffset+(role*C.LEN+field)*8,16);
      }
      this.matrixRows[role]=rows;
    }
    return rows.values[offset];
  }
  // Water visibility consumes authored boxes once; both view projections and
  // the far-span transform remain in the native camera owner thereafter.
  mirrorVisible(water){
    const core=this.core,g=core.graphics;
    if(this.boundWater!==water){
      const count=water.cellBounds.length,bounds=new Float64Array((count+1)*6);
      water.bounds.min.toArray(bounds,0);water.bounds.max.toArray(bounds,3);
      for(let i=0;i<count;i++){water.cellBounds[i].min.toArray(bounds,(i+1)*6);water.cellBounds[i].max.toArray(bounds,(i+1)*6+3);}
      core.rust.camera_mirror_bounds_frame(bounds);this.boundWater=water;
      const owner=this;let buffer=null,address=-1,view=null;
      Object.defineProperty(water.reflectionMatrix,'elements',{configurable:true,get(){owner.core.views();const a=owner.core.cameraMirror,at=a.byteOffset+MI.REFLECTION*8;if(buffer!==a.buffer||address!==at){buffer=a.buffer;address=at;view=new Float64Array(buffer,address,16);}return view;}});
    }
    core.views();const input=core.cameraInput;
    input[I.WATER_LEVEL]=water.level;lighting.fade.value.toArray(input,I.WATER_FADE);
    input[I.WATER_REACH]=water.farReach||0;input[I.FAR_REACH]=g.farReach?.[1]||0;
    core.rust.camera_mirror_visibility_frame();
  }
  // Texture.updateMatrix's f64 operation order belongs to the uniform owner.
  // An explicitly authored matrix remains a raw input, without Three math.
  uv(map,out,offset){
    if(!map){for(let k=0;k<9;k++)out[offset+k]=k%4===0?1:0;return;}
    if(!map.matrixAutoUpdate){for(let k=0;k<9;k++)out[offset+k]=map.matrix.elements[k];return;}
    const core=this.core;core.views();const a=core.cameraUniformInput;
    a[UI.UV]=map.offset.x;a[UI.UV+1]=map.offset.y;a[UI.UV+2]=map.repeat.x;a[UI.UV+3]=map.repeat.y;
    a[UI.UV+4]=map.rotation;a[UI.UV+5]=map.center.x;a[UI.UV+6]=map.center.y;
    core.rust.camera_uniform_frame(true);for(let k=0;k<9;k++)out[offset+k]=core.cameraUniformOutput[UO.UV+k];
  }
  packValid(pack){this.core.views();return pack.owner===this&&pack.native>=0&&this.core.cameraPackHandles[pack.native%L.PACKS]===pack.native;}
  acquirePack(pack){
    if(this.packValid(pack))return;
    if(pack.owner)pack.owner.releasePack(pack);
    pack.owner=this;pack.native=this.core.rust.camera_pack_acquire_frame();this.core.views();
    lightPacks.register(pack,{owner:this.weak,handle:pack.native},pack);
  }
  releasePack(pack){
    lightPacks.unregister(pack);
    if(pack.owner===this)this.core.rust.camera_pack_release_frame(pack.native);
    pack.owner=null;pack.native=-1;
  }
  // Every drawn scene camera has a native row, including menu previews.
  // Retained generations allow a long page session to reuse bounded slots.
  shaderRole(camera){
    const known=this.roles.get(camera);if(known!==undefined&&known<R.EXTRA0)return known;
    const core=this.core;core.views();let entry=this.extra.get(camera);
    if(!entry){entry={handle:-1};this.extra.set(camera,entry);}
    if(entry.handle<0||core.cameraExtraHandles[entry.handle%L.EXTRA_CAMERAS]!==entry.handle){
      extraCameras.unregister(camera);entry.handle=core.rust.camera_acquire_frame();
      extraCameras.register(camera,{owner:this.weak,handle:entry.handle},camera);core.views();
    }
    const a=core.cameraInput;this.pose(camera);
    a[I.ORTHOGRAPHIC]=+!!camera.isOrthographicCamera;
    if(camera.isOrthographicCamera){a[I.ORTHO_BOUNDS]=camera.left;a[I.ORTHO_BOUNDS+1]=camera.right;a[I.ORTHO_BOUNDS+2]=camera.top;a[I.ORTHO_BOUNDS+3]=camera.bottom;}
    a[I.HAS_PARENT]=+!!camera.parent;if(camera.parent)a.set(camera.parent.matrixWorld.elements,I.PARENT_WORLD);
    a[I.USE_LOCAL_MATRIX]=+!camera.matrixAutoUpdate;if(!camera.matrixAutoUpdate)a.set(camera.matrix.elements,I.LOCAL_MATRIX);
    const role=core.rust.camera_import_frame(entry.handle);this.roles.set(camera,role);return role;
  }
  // Publish raw inputs once; main and independent cameras use identical lens
  // defaults and the same optional native gameplay publication boundary.
  pose(camera){
    const a=this.core.cameraInput;
    camera.position.toArray(a,I.POSITION);camera.quaternion.toArray(a,I.QUATERNION);camera.scale.toArray(a,I.SCALE);camera.up.toArray(a,I.UP);
    for(let i=0;i<lensNames.length;i++)a[I.LENS+i]=camera[lensNames[i]]??lensDefaults[i];
    const view=camera.view;a[I.LENS+N.VIEW_ENABLED]=+(!!view?.enabled);
    if(view)for(let i=0;i<viewNames.length;i++)a[I.LENS+N.FULL_WIDTH+i]=view[viewNames[i]];
    const source=camera.userData.nativeView;a[I.PUBLISHED]=+!!source;if(source)a.set(source,I.MATRICES);
  }
  // Unnamed standalone clients publish authored camera facts once at admission.
  // Battle views already have stable native roles before any scene query.
  query(camera){const role=this.roles.get(camera)??this.bindings.get(camera);return role!==undefined&&role<R.EXTRA0?role:this.shaderRole(camera);}
  mirrored(object){const node=this.core.registry.admitIfNew(object);this.core.registry.worlds.flush();return !!node&&this.core.rust.scene_mirrored_frame(node.slot,node.generation);}
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
    const core=this.core;core.views();const a=core.cameraLightOutput;
    lighting.sunDirection.value.fromArray(a,LO.DIRECTION);
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
    this.pose(camera);const source=camera.userData.nativeView;
    const changed=core.rust.camera_main_frame();
    // PageCameras has already published its readback. Import that record once
    // without copying the same matrices back into the same Three facade.
    if(source){if(!this.facades?.has(camera))this.bind(camera,R.MAIN);this.roles.set(camera,R.MAIN);}else if(changed)this.read(camera,R.MAIN);
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
    sceneTransform(camera.position.fromArray(a,at+C.POSITION));camera.up.fromArray(a,at+C.UP);
    const q=camera.quaternion;q._x=a[at+C.QUATERNION];q._y=a[at+C.QUATERNION+1];q._z=a[at+C.QUATERNION+2];q._w=a[at+C.QUATERNION+3];
    if(!this.facades?.has(camera)){sceneTransform(camera.matrix.fromArray(a,at+C.WORLD));sceneTransform(camera.matrixWorld.fromArray(a,at+C.WORLD));}
    // CubeCamera's parent is an Object3D. Only its six children carry lens,
    // inverse and projection fields; the parent publishes pose alone.
    if(camera.isCamera){
      for(let i=0;i<lensNames.length;i++)camera[lensNames[i]]=a[at+C.LENS+i];
      if(!this.facades?.has(camera)){
        camera.matrixWorldInverse.fromArray(a,at+C.INVERSE);
        camera.projectionMatrix.fromArray(a,at+C.PROJECTION);camera.projectionMatrixInverse.fromArray(a,at+C.PROJECTION_INVERSE);
      }
    }
    camera.matrixWorldNeedsUpdate=false;
  }
  // Graphics.prepareFrame publishes these views at their original boundaries.
  // Shader blocks borrow their native rows directly, before another view.
  projection(camera,role){
    if(this.facades?.has(camera))return;
    this.core.views();const a=this.core.cameraOutput,at=role*C.LEN;
    camera.projectionMatrix.fromArray(a,at+C.PROJECTION);camera.projectionMatrixInverse.fromArray(a,at+C.PROJECTION_INVERSE);
  }
  derived(camera,role){this.core.rust.camera_derived_frame(role);this.read(camera,role);}
  copy(camera,from,to){this.core.rust.camera_copy_frame(from,to);this.read(camera,to);}
  // Upscaler.restore visits only cameras actually shifted in this frame.
  // Reading unused rows could replace an idle view camera's near/far range.
  shift(camera){const core=this.core,role=this.bindings.get(camera);if(role===undefined)return;core.views();core.cameraInput[I.SHIFT]=core.postState[core.graphics.upscaler.shiftOffset];core.cameraInput[I.SHIFT+1]=core.postState[core.graphics.upscaler.shiftOffset+1];this.shifted[role]=1;core.rust.camera_shift_frame(role);this.projection(camera,role);}
  restore(){this.core.rust.camera_restore_frame();for(let i=0;i<R.COUNT;i++)if(this.shifted[i]){this.projection(this.shells[i],i);this.shifted[i]=0;}}
  // Sky bindings change only at content admission. Follow metadata remains
  // authored input; no displayed view copies a resulting position to Three.
  bindSky(layers){
    const core=this.core,single=core.graphics.sky,count=Math.min(layers.length,L.SKY);
    let changed=this.skyObjects?.length!==count||this.boundSingle!==single;
    for(let i=0;!changed&&i<count;i++){
      const item=layers[i],f=item.follow,at=i*Y.LEN;
      changed=this.skyObjects[i]!==item.mesh||this.skyFacts[at+Y.FOLLOW]!==+!!f||!Object.is(this.skyFacts[at+Y.SCALE],f?.[0]??0)||!Object.is(this.skyFacts[at+Y.OFFSET],f?.[1]??0);
    }
    if(changed){
      this.skyObjects=new Array(count);this.skyFacts=new Float64Array(count*Y.LEN);this.boundSingle=single;const nodes=new Uint32Array(count*2);
      for(let i=0;i<count;i++){
        const item=layers[i],f=item.follow,at=i*Y.LEN,node=core.registry.admitIfNew(item.mesh);this.skyObjects[i]=item.mesh;
        if(node){node.transformPositionNative=true;core.registry.worlds.touch(node);}
        nodes[i*2]=node?.slot??0xffffffff;nodes[i*2+1]=node?.generation??0;
        this.skyFacts[at+Y.FOLLOW]=+!!f;this.skyFacts[at+Y.SCALE]=f?.[0]??0;this.skyFacts[at+Y.OFFSET]=f?.[1]??0;
      }
      const standalone=single&&this.skyObjects.indexOf(single)<0?core.registry.admitIfNew(single):null;
      if(standalone){standalone.transformPositionNative=true;core.registry.worlds.touch(standalone);}
      core.rust.camera_sky_nodes_frame(nodes,standalone?.slot??0xffffffff,standalone?.generation??0);core.views();core.cameraInput[I.SKY_COUNT]=count;core.cameraInput.set(this.skyFacts,I.SKY_FOLLOW);
    }
    core.registry.worlds.flush();
  }
  sky(layers,role,follow=true){this.bindSky(layers);this.core.rust.camera_scene_sky_frame(role,follow);}
  saveSky(layers){this.bindSky(layers);this.core.rust.camera_scene_sky_save_frame();}
  restoreSky(){this.core.rust.camera_scene_sky_restore_frame();}
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
    a[I.CASCADE_COUNT]=csm?.lights.length||0;if(!csm){core.rust.camera_shadow_configure_frame();return;}a[I.MAP_SIZE]=csm.shadowMapSize;a[I.MAX_FAR]=csm.maxFar;a[I.LIGHT_RANGE]=csm.lightNear;a[I.LIGHT_RANGE+1]=csm.lightFar;a[I.MARGIN]=csm.lightMargin;a[I.RESET]=g.shadowReset;
    core.rust.camera_shadow_configure_frame();
  }
  cascades(){
    const core=this.core,g=core.graphics,csm=g.csm;
    core.rust.camera_cascades_frame();this.read(g.cascadeCamera,R.CASCADE);

  }
  // LightShadow.updateMatrices used this late boundary. Only selected passes
  // advance the receiver matrix; a held reflection reads the preceding one.
  shadow(light){
    const core=this.core,index=light.userData.nativeCascade,role=R.SHADOW0+index;
    core.rust.camera_shadow_frame(index);this.read(light.shadow.camera,role);
  }
}
export { R as CAMERA_ROLE };

// DirectionalLight and target shells retain cold resource lifetime only.
// Split, window, camera and receiver-plane operations belong to Rust.
export function shadowShells(g,preset){
    g.shadowReset=(g.shadowReset||0)+1;
    const csm={lights:[],shadowMapSize:preset.size,maxFar:preset.far,lightNear:1,lightFar:3000,lightMargin:400,
      remove(){for(const light of this.lights){g.scene.remove(light);g.scene.remove(light.target);}},dispose:noop};
    for(let i=0;i<preset.cascades;i++){
      const light=new THREE.DirectionalLight(0xffffff,3);sceneProperty(light,"castShadow",true);light.userData.nativeCascade=i;
      light.shadow.mapSize.set(preset.size,preset.size);light.shadow.bias=-.015/2999;light.shadow.normalBias=0;light.shadow.camera.near=1;light.shadow.camera.far=3000;
      sceneProperty(light,"matrixAutoUpdate",sceneProperty(light,"matrixWorldAutoUpdate",false));sceneProperty(light.target,"matrixAutoUpdate",sceneProperty(light.target,"matrixWorldAutoUpdate",false));
      g.rust?.cameras.bind(light.shadow.camera,R.SHADOW0+i);g.scene.add(light);g.scene.add(light.target);csm.lights.push(light);
      const window={held:false,dirty:true,moved:false,invalidate(){g.rust?.rust.camera_invalidate_frame(i);}};g.cascadeWindows.push(window);g.rust?.cameras.bindWindow(window,i);
    }
    return csm;
  }
