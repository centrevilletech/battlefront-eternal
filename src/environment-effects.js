import * as THREE from 'three';
import { Effects } from './effects.js';
import { lighting } from './materials.js';
import { modelPack, modelGroup } from './models.js';
import { EnvironmentVolume, EnvironmentWakes } from './environment-volume.js';
import { FountainWater } from './fountain-water.js';
import { FountainSpray } from './fountain-spray.js';
import { FountainJets, TIER } from './fountain-jets.js';
import { SteamWisps } from './steam-wisps.js';

const UP=new THREE.Vector3(0,1,0),FORWARD=new THREE.Vector3(0,0,1);
// The effects placed objects carry (tools/zero_environment.py). Fountains and
// steam vents have Enhanced forms; the rest (torches, campfires, smoke, mist,
// waterspouts) play their stock effect in either mode while in view, out to
// the near scene's end.
const ENHANCED=new Set(['waterfountain','prop_steamgrate','prop_steampipe']);
export class EnvironmentEffects {
  static async load({map,graphics,settings,loadTexture}) {
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
      const material=new THREE.MeshBasicMaterial({map:texture(parts[0].texture),transparent:true,blending:THREE.AdditiveBlending,depthWrite:false,side:THREE.DoubleSide});
      const group=modelGroup(parts,()=>material);group.userData.material=material;return group;};};
    const environment=new EnvironmentEffects({graphics,settings,data,texture,model});
    // Compile these shaders during map loading, before entering a fountain or
    // chamber can stall a gameplay frame. No world/model materials are rebuilt.
    const waterMesh=environment.water?.original[0]?.mesh;
    const warmWater=waterMesh?new THREE.Mesh(waterMesh.geometry,environment.water.material):null;
    if(warmWater)environment.group.add(warmWater);
    try{
      await environment.classic.warmup(graphics.renderer,graphics.camera);
      await graphics.renderer.compileAsync(environment.group,graphics.camera,graphics.scene);
      if(environment.volume){
        await graphics.renderer.compileAsync(environment.volume.volumeScene,graphics.camera);
        graphics.renderer.initTexture(environment.volume.noise);
      }
    }finally{warmWater?.removeFromParent();}
    return environment;
  }
  constructor({graphics,settings,data,texture,model}) {
    this.graphics=graphics;this.settings=settings;this.data=data;this.time=0;this.mode=null;
    this.group=new THREE.Group();this.group.name='map-environment-effects';graphics.scene.add(this.group);
    this.classic=new Effects(this.group,data.effects,{texture,model});this.classic.preload();
    this.wakes=new EnvironmentWakes();
    const enhanced=data.instances.filter(i=>ENHANCED.has(i.effect)),steam=enhanced.some(i=>i.effect!=='waterfountain');
    this.water=data.basins.length?new FountainWater(graphics,data.basins,this.wakes,texture('waterfoam1'),TIER):null;
    this.volume=enhanced.length?new EnvironmentVolume(this.group,enhanced.length*2,this.wakes,this.water?.reflectionUniforms):null;
    this.spray=data.basins.length?new FountainSpray(this.group,this.wakes):null;
    this.jets=this.water?new FountainJets(this.group,this.spray,graphics,{foam:texture('waterfoam1'),tier:true}):null;
    this.steam=!data.basins.length&&steam?new SteamWisps(this.group,this.wakes,texture("thicksmoke2")):null;
    this.instances=data.instances.map(item=>{
      const matrix=new THREE.Matrix4().fromArray(item.matrix),position=new THREE.Vector3().setFromMatrixPosition(matrix),quaternion=new THREE.Quaternion().setFromRotationMatrix(matrix);
      const fountain=item.effect==='waterfountain',pipe=item.effect==='prop_steampipe',classic=!ENHANCED.has(item.effect);
      const direction=(pipe?FORWARD:UP).clone().applyQuaternion(quaternion);
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
    this.frustum=new THREE.Frustum();this.matrix=new THREE.Matrix4();this.visibleVolumes=[];this.visibleFountains=[];
    this.relevant=position=>this.instances.some(i=>i.position.distanceToSquared(position)<144);
    this.stats={activeSources:0,volumes:0,classicParticles:0};
    this.warmup=new Map(Object.entries(data.effects).map(([name,defs])=>[name,Math.min(4,Math.max(...defs.map(d=>d.transformer?.lifetime||0)))]));
  }
  update(dt,camera,actors=[]) {
    dt=Math.min(Math.max(dt,0),.1);this.time+=dt;
    const mode=this.settings.get('environmentEffects')==='classic'?'classic':'enhanced';
    if(mode!==this.mode){
      // The stock effects that have no Enhanced form play on through the switch.
      for(const i of this.instances){if(i.classic)continue;i.handle?.kill();i.handle=null;}
      this.classic.update(0);this.wakes.clear();this.mode=mode;this.water?.setEnabled(mode==='enhanced');
    }
    camera.updateMatrixWorld();this.matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);this.frustum.setFromProjectionMatrix(this.matrix);
    this.wakes.update(dt,mode==='enhanced'?actors:[],this.relevant);
    this.visibleVolumes.length=0;this.visibleFountains.length=0;this.stats.activeSources=0;
    const reach=Math.max(90,lighting.fade.value.y);
    for(const i of this.instances){
      const visible=i.position.distanceToSquared(camera.position)<(i.classic?reach*reach:90*90)&&this.frustum.intersectsSphere(i.sphere);
      if(visible)this.stats.activeSources++;
      if(mode==='classic'||i.classic){
        if(visible&&!i.handle){
          i.handle=this.classic.play(i.effect,i.position,UP,null,i.quaternion);
          // Emit the last lifetime analytically, so a fountain is already
          // flowing when it enters view, without stepping an offscreen sim.
          if(i.handle)this.classic.step(i.handle,this.warmup.get(i.effect)||0);
        }
        else if(!visible&&i.handle){i.handle.kill();i.handle=null;}
      }else if(visible){
        // Fountains are drawn as water (fountain-jets.js); only steam is a volume.
        if(!i.jet)this.visibleVolumes.push(i.volume);
        if(i.jet)this.visibleFountains.push(i);
      }
    }
    this.classic.update(dt);
    this.volume?.update(this.time,this.visibleVolumes);this.spray?.update(this.time,this.visibleFountains);this.water?.update(this.time);
    this.jets?.update(this.time,this.visibleFountains);
    this.steam?.update(this.time,this.visibleVolumes);
    this.stats.volumes=this.visibleVolumes.length;
    this.stats.classicParticles=this.classic.effects.reduce((sum,e)=>sum+e.emitters.reduce((n,em)=>n+em.particles.length,0),0);
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
