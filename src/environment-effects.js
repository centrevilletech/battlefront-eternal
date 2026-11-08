import {sceneProperty} from "./rust-render/scene-changes.js";
import {glowLook} from './actor-resources.js';
import * as THREE from 'three';
import {PageEffects} from './presentation/effects.js';
import { lighting } from './materials.js';
import { modelPack, modelGroup } from './models.js';
import { EnvironmentVolume, EnvironmentWakes } from './environment-volume.js';
import { FountainWater } from './fountain-water.js';
import { FountainSpray } from './fountain-spray.js';
import { FountainJets, TIER } from './fountain-jets.js';
import { SteamWisps } from './steam-wisps.js';
import { VisibilityHost } from './environment/module.js';
import { EnvironmentFrame as FRAME, EnvironmentResult as RESULT, View as VIEW,PageFrame as PAGE } from './environment/layout.js';

// Renderer resources consume a bounded indexed list. Keeping its numeric
// slots avoids rebuilding Array backing stores when visibility changes.
class ResourceList {
  constructor(capacity) { this.length=0;for(let i=0;i<capacity;i++)this[i]=null; }
  push(value) { this[this.length++]=value; }
}
const UP=new THREE.Vector3(0,1,0);
// The effects placed objects carry (tools/zero_environment.py). Fountains and
// steam vents have Enhanced forms; the rest (torches, campfires, smoke, mist,
// waterspouts) play their stock effect in either mode while in view, out to
// the near scene's end.
export class EnvironmentEffects {
  static async load({map,graphics,settings,loadTexture,presentationLoaded,surfaceOwner}) {
    // Maps whose objects carry no effects have no file (a dev server answers
    // a missing one with its page, which isn't JSON).
    const dir=`/maps/${map}/environment`,response=await fetch(`${dir}/effects.json`).catch(()=>null);
    if(!response?.ok)return null;
    const data=await response.json().catch(()=>null);
    if(!data?.instances?.length)return null;
    const [vertices,indices]=await Promise.all(['models.bin','models_index.bin'].map(async name=>{
      const result=await fetch(`${dir}/${name}`);if(!result.ok)throw Error(`Environment FX asset missing: ${name}`);return result.arrayBuffer();}));
    const packed=modelPack(data.models,vertices,indices),textures=new Map(Object.entries(data.textures).map(([name,path])=>[name.toLowerCase(),loadTexture(`${dir}/${path}`)]));
    const texture=name=>textures.get(name?.toLowerCase())||null;
    const model=name=>{const parts=packed.get(name);if(!parts)return null;return()=>{
      const material=glowLook(texture(parts[0].texture));
      const group=modelGroup(parts,()=>material);sceneProperty(group.userData,"material",material);return group;};};
    const environment=new EnvironmentEffects({graphics,settings,data,texture,model,presentationLoaded,surfaceOwner});
    await environment.classic.warmup(graphics.renderer,graphics.camera);
    return environment;
  }
  constructor({graphics,settings,data,texture,model,presentationLoaded,surfaceOwner}) {
    this.graphics=graphics;this.settings=settings;this.data=data;this.time=0;this.mode=null;
    this.group=new THREE.Group();this.group.name='map-environment-effects';graphics.scene.add(this.group);
    this.classic=new PageEffects(this.group,data.effects,{texture,model},presentationLoaded);this.classic.preload();
    this.host=new VisibilityHost(surfaceOwner,0,{...data,effects:Object.entries(data.effects)});
    const recipe=this.host.recipe;
    this.instances=data.instances.map((item,k)=>{
      const r=recipe.instances[k],position=new THREE.Vector3().fromArray(r.position),quaternion=new THREE.Quaternion().fromArray(r.quaternion);
      const volume={position,direction:new THREE.Vector3().fromArray(r.direction),extent:new THREE.Vector3().fromArray(r.extent),kind:r.kind};
      const jet=r.jet?{position,direction:new THREE.Vector3().fromArray(r.jet.direction),extent:new THREE.Vector3().fromArray(r.jet.extent),kind:r.jet.kind}:null;
      return {...item,position,quaternion,volume,jet,classic:r.classic,handle:null,slot:r.slot,level:r.level,low:r.low,warmup:r.warmup};
    });
    this.resetSources=recipe.resets;
    this.visibleVolumes=new ResourceList(this.instances.length);this.visibleFountains=new ResourceList(this.instances.length);
    this.wakes=new EnvironmentWakes(this.host);
    this.water=recipe.water?new FountainWater(graphics,data.basins,this.wakes,texture('waterfoam1'),TIER):null;
    this.volume=recipe.volumeCapacity?new EnvironmentVolume(this.group,recipe.volumeCapacity,this.wakes,this.water?.reflectionUniforms):null;
    this.spray=recipe.water?new FountainSpray(this.group,this.wakes):null;
    this.jets=this.water?new FountainJets(this.group,this.spray,graphics,{foam:texture('waterfoam1'),tier:true}):null;
    this.steam=recipe.steam?new SteamWisps(this.group,this.wakes,texture("thicksmoke2")):null;

    this.stats={activeSources:0,volumes:0,classicParticles:0};
  }
  prepareFrame(dt,camera,actors) {
    this.host.refresh();

    const view=this.host.view;
    view.set(camera.matrixWorld.elements,VIEW.CAMERA);view.set(camera.matrixWorldInverse.elements,VIEW.INVERSE);view.set(camera.projectionMatrix.elements,VIEW.PROJECTION);
    view[VIEW.POSITION]=camera.position.x;view[VIEW.POSITION+1]=camera.position.y;view[VIEW.POSITION+2]=camera.position.z;
    lighting.ambientTop.value.toArray(this.host.frame,FRAME.AMBIENT_TOP);
    this.host.frame[FRAME.REACH]=lighting.fade.value.y;this.host.frame[FRAME.ENHANCED]=this.settings.get('environmentEffects')==='classic'?0:1;
    lighting.sunColor.value.toArray(this.host.pageFrame,PAGE.SUN_COLOR);lighting.sunDirection.value.toArray(this.host.pageFrame,PAGE.SUN_DIRECTION);
    this.host.frame[FRAME.DT]=dt;
    this.wakes.prepare(actors);
  }
  publishFrame() {
    this.host.refresh();
    const r=this.host.result,n=this.instances.length,dt=r[RESULT.DT];
    this.time=r[RESULT.TIME];
    if(r[RESULT.MODE_CHANGED]) {
      // Native actions below restart only sources with Enhanced forms. Kill
      // them before flushing the particle owner, retaining the original order.
      for(const k of this.resetSources){this.instances[k].handle?.kill();this.instances[k].handle=null;this.host.handles[k]=0;}
      this.classic.update(0);this.mode=r[RESULT.ENHANCED]?'enhanced':'classic';this.water?.setEnabled(Boolean(r[RESULT.ENHANCED]));
    }
    this.wakes.synced();
    this.visibleVolumes.length=0;this.visibleFountains.length=0;
    this.stats.activeSources=r[RESULT.SOURCES];
    for(let k=0;k<n;k++) {
      const i=this.instances[k],action=r[RESULT.STRIDE+3*n+k];
      if(action>0) {
        if(action===2&&i.handle){i.handle.kill();i.handle=null;}
        i.handle=this.classic.play(i.effect,i.position,UP,null,i.quaternion);
        if(i.handle)this.classic.step(i.handle,i.warmup);
        this.host.handles[k]=i.handle?1:0;
      } else if(action<0&&i.handle){i.handle.kill();i.handle=null;this.host.handles[k]=0;}
    }
    // The Enhanced steam volumes and fountains in view, for their draws.
    for(let k=0;k<r[RESULT.VOLUMES];k++)this.visibleVolumes.push(this.instances[r[RESULT.STRIDE+n+k]].volume);
    for(let k=0;k<r[RESULT.FOUNTAINS];k++)this.visibleFountains.push(this.instances[r[RESULT.STRIDE+2*n+k]]);
    this.classic.update(dt);
    this.volume?.update(this.time,this.visibleVolumes);this.spray?.update(this.time,this.visibleFountains);this.water?.update(this.time);
    this.jets?.update(this.time,this.visibleFountains);
    this.steam?.update(this.time,this.visibleVolumes);
    this.stats.volumes=r[RESULT.VOLUMES];
    this.stats.classicParticles=this.classic.particleCount;
  }

  reflect(camera,terrainWater){this.water?.reflect(camera,[this.volume?.mesh,terrainWater?.group,terrainWater?.rings]);}
  dispose(){
    this.classic.gpu?.dispose();
    this.water?.dispose();this.volume?.dispose();this.spray?.dispose();this.jets?.dispose();this.steam?.dispose();
    const geometries=new Set(),materials=new Set();
    this.group.traverse(object=>{if(object.geometry)geometries.add(object.geometry);if(object.material)materials.add(object.material);});
    for(const geometry of geometries)geometry.dispose();for(const material of materials)material.dispose();
    this.group.removeFromParent();this.classic.effects.length=0;this.classic.meshes.clear();this.classic.renderer.batches.clear();this.classic.renderer.lookup?.clear();
    this.wakes.clear();
  }
}
