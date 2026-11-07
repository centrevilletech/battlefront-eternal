import * as THREE from 'three';
import { EFFECT } from './rust-render/effect-layout.js';
import { lighting } from './materials.js';
import { EnhancedOcean } from './ocean-storm.js';
import { SurfaceResult as RESULT } from './environment/layout.js';

// Weather type is an authored fact. Native descriptors select the corresponding
// ocean programs, topology, mask sampling, raster state and material constants.
export function storms(weather) {
  const type=weather?.precipitation?.Type?.[0];
  return Boolean(weather?.precipitation)&&String(type??'').toLowerCase()!=='quads';
}
const copy3=(v,at,p)=>{if(at!==undefined){v[at]=p.x;v[at+1]=p.y;v[at+2]=p.z;}};
export class OceanSurface {
  constructor({data,texture,graphics,weather=null,surfaceHost,waterOwner=null}) {
    this.host=surfaceHost;this.water=waterOwner;this.data=data;this.graphics=graphics;this.height=data.height;
    this.time=0;this.look=data.look||{};this.stormy=storms(weather);this.core=null;this.sources=[];
    this.waterMap=texture(this.look.MainTexture?.[0]);this.foamMap=texture(this.look.FoamTexture?.[0])||this.waterMap;
    this.nearClassic=this.anchor(EFFECT.OCEAN_CLASSIC,false);this.farClassic=this.anchor(EFFECT.OCEAN_CLASSIC,true);
    this.nearEnhanced=this.anchor(this.stormy?EFFECT.OCEAN_STORM:EFFECT.OCEAN,false);
    this.farEnhanced=this.anchor(this.stormy?EFFECT.OCEAN_STORM:EFFECT.OCEAN,true);
    graphics.scene.add(this.nearClassic,this.nearEnhanced);graphics.farScene.add(this.farClassic,this.farEnhanced);
    this.applyMode();this.unsubscribe=graphics.settings?.onChange(key=>{if(key==='oceanEffects')this.applyMode();});
  }
  // The page retains scene/camera facts. Ocean meshes carry no geometry,
  // material, texture descriptor or uniform table for the renderer to inspect.
  anchor(row,sceneFar,order=0) {
    const mesh=new THREE.Object3D();mesh.name='native-ocean';mesh.frustumCulled=false;mesh.position.y=this.height;mesh.renderOrder=order;mesh.visible=false;
    const source={family:'ocean',owner:this,mesh,row,nativeKind:row.ID,values:new Float64Array(row.LENGTH),far:true,sceneFar,center:null};
    source.prepare=camera=>this.prepareSource(source,camera);mesh.userData.effectModel=source;this.sources.push(source);return mesh;
  }
  prepareResources(core) {
    if(this.core){this.prepareAuxiliary(core);return;}this.core=core;this.water.prepareResources(core);this.sourceId=this.water.sourceId;
    const resources=core.resources;this.maskSample=resources.gpu.water_mask(JSON.stringify(this.data));this.maskId=core.texture(this.maskSample);
    this.waterId=this.waterMap?core.texture(resources.staticSample(this.waterMap)):-1;
    this.foamId=this.foamMap?core.texture(resources.staticSample(this.foamMap)):-1;
    this.prepareAuxiliary(core);
  }
  prepareAuxiliary(core) {
    const resources=core.resources;
    if(this.waves?.probe&&this.probeId===undefined){this.waves.probe.prepareResources(resources);this.probeId=core.texture(this.waves.probe.texture);}
    const target=this.waves?.storm?.target;
    if(target&&this.cloudId===undefined){resources.targetId(target);this.cloudId=core.texture(target.texture);}
  }
  // Generated numeric rows replace OceanSurface's material uniform reads.
  // The native look defaults stay untouched; only published frame facts move.
  prepareSource(source,camera) {
    const v=source.values,row=source.row,cameraWorld=camera.matrixWorld.elements;
    if(row.MODEL_MATRIX!==undefined)v.set(source.mesh.matrixWorld.elements,row.MODEL_MATRIX);
    if(row.PROJECTION_MATRIX!==undefined)v.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);
    if(row.VIEW_MATRIX!==undefined)v.set(camera.matrixWorldInverse.elements,row.VIEW_MATRIX);
    if(row.CAMERA_POSITION!==undefined){v[row.CAMERA_POSITION]=cameraWorld[12];v[row.CAMERA_POSITION+1]=cameraWorld[13];v[row.CAMERA_POSITION+2]=cameraWorld[14];}
    if(row.TIME!==undefined)v[row.TIME]=this.time;
    copy3(v,row.SUN_DIRECTION,lighting.sunDirection.value);copy3(v,row.SUN_COLOR,lighting.sunColor.value);copy3(v,row.FOG_COLOR,lighting.fogColor.value);copy3(v,row.AMBIENT,lighting.ambient.value);
    if(row.FOG_RANGE!==undefined)lighting.fogRange.value.toArray(v,row.FOG_RANGE);
    if(row.MASK!==undefined)v[row.MASK]=this.maskId;
    if(row.WATER_MAP!==undefined)v[row.WATER_MAP]=this.waterId;
    if(row.FOAM_MAP!==undefined)v[row.FOAM_MAP]=this.foamId;
    if(row.SKY_PROBE!==undefined)v[row.SKY_PROBE]=this.probeId??-1;
    if(row.CLOUD_MAP!==undefined)v[row.CLOUD_MAP]=this.cloudId??-1;
    if(row.CLOUD_READY!==undefined)v[row.CLOUD_READY]=this.waves.storm.uniforms.cloudReady.value;
    if(row.LIGHTNING_FLASH!==undefined)v[row.LIGHTNING_FLASH]=this.host.result[RESULT.STORM_FLASH];
    if(row.LIGHTNING_POSITION!==undefined)for(let i=0;i<3;i++)v[row.LIGHTNING_POSITION+i]=this.host.result[RESULT.STORM_POSITION+i];
    if(row.EYE!==undefined&&this.camera){v[row.EYE]=this.camera.position.x;v[row.EYE+1]=this.camera.position.z;}
    if(row.SEA_LEVEL!==undefined&&this.camera)v[row.SEA_LEVEL]=this.height;
  }
  applyMode() {
    this.enhanced=this.graphics.settings?.get('oceanEffects')==='enhanced';
    if(this.enhanced&&!this.waves){this.waves=new EnhancedOcean(this,{storm:this.stormy,look:this.look,surfaceHost:this.host});this.nearEnhanced.add(this.waves.spray);if(this.waves.haze)this.farEnhanced.add(this.waves.haze);}
    this.nearClassic.visible=this.farClassic.visible=!this.enhanced;this.nearEnhanced.visible=this.farEnhanced.visible=this.enhanced;
    this.near=this.enhanced?this.nearEnhanced:this.nearClassic;this.far=this.enhanced?this.farEnhanced:this.farClassic;
    this.waves?.setEnabled(this.enhanced);if(this.core)this.prepareAuxiliary(this.core);
  }
  // OceanSurface.update's clock and grid snap are native environment outputs.
  // Probe and cloud captures keep their existing presentation boundaries.
  publishFrame(result,camera) {
    this.time=result[RESULT.OCEAN_TIME];this.camera=camera;
    this.near.position.set(result[RESULT.OCEAN_X],this.height,result[RESULT.OCEAN_Z]);this.far.position.copy(this.near.position);
    if(this.enhanced)this.waves.update(this.time,camera,this.height,result);
  }
  dispose(){for(const mesh of [this.nearClassic,this.nearEnhanced,this.farClassic,this.farEnhanced])mesh.removeFromParent();this.unsubscribe?.();this.waves?.dispose();if(this.core)this.core.resources.gpu.dispose_sample(this.maskSample);}
}
