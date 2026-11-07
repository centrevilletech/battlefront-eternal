import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
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
import { EnvironmentFrame as FRAME, EnvironmentInstance as INSTANCE, EnvironmentResult as RESULT, View as VIEW } from './environment/layout.js';

// Renderer resources consume a bounded indexed list. Keeping its numeric
// slots avoids rebuilding Array backing stores when visibility changes.
class ResourceList {
  constructor(capacity) { this.length=0;for(let i=0;i<capacity;i++)this[i]=null; }
  push(value) { this[this.length++]=value; }
}
const EMPTY_ACTORS=[];
const UP=new THREE.Vector3(0,1,0),FORWARD=new THREE.Vector3(0,0,1);
// The effects placed objects carry (tools/zero_environment.py). Fountains and
// steam vents have Enhanced forms; the rest (torches, campfires, smoke, mist,
// waterspouts) play their stock effect in either mode while in view, out to
// the near scene's end.
const ENHANCED=new Set(['waterfountain','prop_steamgrate','prop_steampipe']);
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
    const enhanced=data.instances.filter(i=>ENHANCED.has(i.effect)),steam=enhanced.some(i=>i.effect!=='waterfountain');
    this.instances=data.instances.map(item=>{
      const matrix=new THREE.Matrix4().fromArray(item.matrix),position=new THREE.Vector3().setFromMatrixPosition(matrix),quaternion=new THREE.Quaternion().setFromRotationMatrix(matrix);
      const fountain=item.effect==='waterfountain',pipe=item.effect==='prop_steampipe',classic=!ENHANCED.has(item.effect);
      const direction=sceneTransform((pipe?FORWARD:UP).clone().applyQuaternion(quaternion));
      const volume={position:position.clone(),direction,extent:new THREE.Vector3(...(fountain?[10,1.5,10]:pipe?[2.5,4,2.5]:[6,5.5,6])),kind:fountain?2:pipe?1:0};
      if(!fountain&&!pipe&&["hp_steam_1","hp_steam_5"].includes(item.hardpoint))volume.extent.set(7,6,7);
      const jet=fountain?{position:position.clone(),direction:UP,extent:new THREE.Vector3(5,5.3,5),kind:3}:null;
      // Smoke and mist rise well above where they start.
      const sphere=classic?new THREE.Sphere(position.clone().addScaledVector(UP,8),16):new THREE.Sphere(position.clone().addScaledVector(direction,2),fountain?9:7);
      return {...item,position,quaternion,volume,jet,classic,handle:null,sphere};
    });
    // Each fountain's fixed slot in the jets' uniforms, and the levels of its
    // pools: Enhanced fills the inner tier to its lip (TIER, fountain-jets.js).
    for(const [slot,f] of this.instances.filter(i=>i.jet).entries()){
      const basin=data.basins.reduce((a,b)=>Math.hypot(b.center[0]-f.position.x,b.center[2]-f.position.z)<Math.hypot(a.center[0]-f.position.x,a.center[2]-f.position.z)?b:a,data.basins[0]);
      const water=basin?basin.center[1]:f.position.y-.2;
      f.slot=slot;f.level=water+TIER.rise;f.low=water+TIER.outer;
    }
    this.visibleVolumes=new ResourceList(this.instances.length);this.visibleFountains=new ResourceList(this.instances.length);
    // Which placed effects are in view is the Rust phase's: each instance's sphere
    // (where it plays), its position and what kind it is.
    this.host=new VisibilityHost(surfaceOwner,this.instances.length);
    this.instances.forEach((i,k)=>{
      const c=this.host.config,at=k*INSTANCE.STRIDE;
      c[at+INSTANCE.SPHERE]=i.sphere.center.x;c[at+INSTANCE.SPHERE+1]=i.sphere.center.y;c[at+INSTANCE.SPHERE+2]=i.sphere.center.z;c[at+INSTANCE.SPHERE+3]=i.sphere.radius;
      c[at+INSTANCE.POSITION]=i.position.x;c[at+INSTANCE.POSITION+1]=i.position.y;c[at+INSTANCE.POSITION+2]=i.position.z;
      c[at+INSTANCE.CLASSIC]=i.classic?1:0;c[at+INSTANCE.JET]=i.jet?1:0;
    });
    this.host.start();
    this.wakes=new EnvironmentWakes(this.host);
    this.water=data.basins.length?new FountainWater(graphics,data.basins,this.wakes,texture('waterfoam1'),TIER):null;
    this.volume=enhanced.length?new EnvironmentVolume(this.group,enhanced.length*2,this.wakes,this.water?.reflectionUniforms):null;
    this.spray=data.basins.length?new FountainSpray(this.group,this.wakes):null;
    this.jets=this.water?new FountainJets(this.group,this.spray,graphics,{foam:texture('waterfoam1'),tier:true}):null;
    this.steam=!data.basins.length&&steam?new SteamWisps(this.group,this.wakes,texture("thicksmoke2")):null;

    this.stats={activeSources:0,volumes:0,classicParticles:0};
    this.warmup=new Map(Object.entries(data.effects).map(([name,defs])=>[name,Math.min(4,Math.max(...defs.map(d=>d.transformer?.lifetime||0)))]));
  }
  prepareFrame(dt,camera,actors) {
    this.host.refresh();
    const mode=this.settings.get('environmentEffects')==='classic'?'classic':'enhanced';
    this.nextMode=mode;
    const view=this.host.view;
    view.set(camera.matrixWorld.elements,VIEW.CAMERA);view.set(camera.matrixWorldInverse.elements,VIEW.INVERSE);view.set(camera.projectionMatrix.elements,VIEW.PROJECTION);
    view[VIEW.POSITION]=camera.position.x;view[VIEW.POSITION+1]=camera.position.y;view[VIEW.POSITION+2]=camera.position.z;
    this.host.frame[FRAME.REACH]=Math.max(90,lighting.fade.value.y);this.host.frame[FRAME.ENHANCED]=mode==='enhanced'?1:0;
    this.host.frame[FRAME.DT]=dt;
    this.wakes.prepare(actors);
  }
  publishFrame() {
    this.host.refresh();
    const r=this.host.result,n=this.instances.length,mode=this.nextMode,dt=r[RESULT.DT];
    this.time=r[RESULT.TIME];
    if(r[RESULT.MODE_CHANGED]) {
      // Native actions below restart only sources with Enhanced forms. Kill
      // them before flushing the particle owner, retaining the original order.
      for(let k=0;k<n;k++)if(!this.instances[k].classic){this.instances[k].handle?.kill();this.instances[k].handle=null;this.host.handles[k]=0;}
      this.classic.update(0);this.mode=mode;this.water?.setEnabled(mode==='enhanced');
    }
    this.wakes.synced();
    this.visibleVolumes.length=0;this.visibleFountains.length=0;
    this.stats.activeSources=r[RESULT.SOURCES];
    for(let k=0;k<n;k++) {
      const i=this.instances[k],action=r[RESULT.STRIDE+3*n+k];
      if(action>0) {
        if(action===2&&i.handle){i.handle.kill();i.handle=null;}
        i.handle=this.classic.play(i.effect,i.position,UP,null,i.quaternion);
        if(i.handle)this.classic.step(i.handle,this.warmup.get(i.effect)||0);
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
    this.stats.volumes=this.visibleVolumes.length;
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
