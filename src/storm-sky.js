import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { lighting } from './materials.js';
import { ownedFullScreenPass } from './render-passes.js';
import { SKY_ORDER } from './world.js';

// A world-oriented panorama means looking around never invalidates the cloud
// cache. Only the low-resolution volume integration runs at 10 Hz; lightning,
// camera rotation and final composition run at the display's frame rate.




// The models on the dome's horizon (Kamino's city) in the storm, in place of
// their sky material: the texture as the sky draws it, in the storm's dim,
// cool, grey light (the light painted into the texture loses some of its
// colour; lit windows keep their glow), seen through its sea haze, and lit by
// a flash as the clouds are, most on the strike's bearing. Below the horizon
// the ocean covers them, and the horizon veil (ocean-storm.js) thickens the
// haze toward their feet.



export class StormSky {
 constructor({graphics,uniforms}) {
   this.graphics=graphics;this.uniforms=uniforms;this.enabled=false;this.lastCloud=-Infinity;
   this.nextStrike=6;this.strikeStart=-100;this.strikes=0;this.randomState=8217;
   this.lastEye=new THREE.Vector3(Infinity,Infinity,Infinity);
   this.lightKeys=['ambient','ambientTop','ambientBottom','characterAmbient'];
   this.lightBase=this.lightKeys.map(key=>lighting[key].value.clone());this.previousFlash=0;
   const bytes=new Uint8Array(64**3);
   for(let i=0;i<bytes.length;i++)bytes[i]=this.random()*255;
   this.noise=new THREE.Data3DTexture(bytes,64,64,64);
   this.noise.format=THREE.RedFormat;this.noise.minFilter=this.noise.magFilter=THREE.LinearFilter;
   this.noise.wrapS=this.noise.wrapT=this.noise.wrapR=THREE.RepeatWrapping;this.noise.unpackAlignment=1;this.noise.needsUpdate=true;
   this.target=new THREE.WebGLRenderTarget(768,384,{depthBuffer:false});
   this.target.texture.wrapS=THREE.RepeatWrapping;
   Object.assign(uniforms,{cloudMap:{value:this.target.texture},cloudReady:{value:0},volumeNoise:{value:this.noise},eyePosition:{value:new THREE.Vector3()},
     lightningFlash:{value:0},lightningPosition:{value:new THREE.Vector3()},viewRotation:{value:new THREE.Matrix4()},inverseProjection:{value:new THREE.Matrix4()}});
   this.cloudPass=ownedFullScreenPass('effects/storm-clouds',uniforms);this.cloudPass.material.userData.bfeNoShadows=true;this.passCamera=new THREE.Camera();
   // Drawn in the dome's place, just after it (world.js), so the models on
   // its horizon draw over the panorama as they draw over the dome.
   this.sky=ownedFullScreenPass('effects/storm-sky',uniforms);this.sky.name='storm-volume-clouds';this.sky.renderOrder=SKY_ORDER+.5;
   this.horizonMaterials=new Map();this.replaced=[];
   this.sky.onBeforeRender=(renderer,scene,camera)=>{uniforms.viewRotation.value.copy(camera.matrixWorld);uniforms.inverseProjection.value.copy(camera.projectionMatrixInverse);};
   this.boltGeometry=new THREE.BufferGeometry();
   this.positions=new Float32Array(1536*3);this.tangents=new Float32Array(1536*3);
   this.edges=new Float32Array(1536);this.widths=new Float32Array(1536);this.energies=new Float32Array(1536);
   for(const [name,array,itemSize] of [['position',this.positions,3],['tangent',this.tangents,3],['edge',this.edges,1],['width',this.widths,1],['energy',this.energies,1]])
     this.boltGeometry.setAttribute(name,new THREE.BufferAttribute(array,itemSize).setUsage(THREE.DynamicDrawUsage));
   this.boltGeometry.setDrawRange(0,0);
   this.bolt=new THREE.Mesh(this.boltGeometry,ownedEffectMaterial('effects/storm-lightning', {uniforms,transparent:true,depthWrite:false,blending:THREE.AdditiveBlending,side:THREE.DoubleSide,forceSinglePass:true}));
   this.bolt.name='storm-lightning';this.bolt.frustumCulled=false;this.bolt.renderOrder=8;
   this.sky.visible=this.bolt.visible=false;graphics.farScene.add(this.sky,this.bolt);
 }
 random(){this.randomState=(Math.imul(this.randomState,1664525)+1013904223)>>>0;return this.randomState/4294967296;}
 setEnabled(enabled) {
   if(this.enabled===enabled)return;
   this.enabled=enabled;this.sky.visible=enabled;this.bolt.visible=false;
   if(enabled){
     // Every part of the dome gives way to the panorama; the models on its
     // horizon stay, in the storm's light.
     const graphics=this.graphics,layers=graphics.skyLayers?.length?graphics.skyLayers:graphics.sky?[{mesh:graphics.sky,layer:0}]:[];
     this.replaced=layers.map(({mesh,layer})=>({mesh,layer,visible:mesh.visible,material:mesh.material}));
     for(const {mesh,layer,material} of this.replaced){if(layer)mesh.material=this.horizonMaterial(material);else mesh.visible=false;}
     this.lastCloud=-Infinity;
   } else {
     for(const {mesh,layer,visible,material} of this.replaced){if(layer)mesh.material=material;else mesh.visible=visible;}
     this.replaced=[];this.restoreLight();this.uniforms.lightningFlash.value=0;
   }
 }
 // One storm material for each sky material a horizon model draws with, made
 // once and kept through mode switches. It blends as the sky material does.
 horizonMaterial(original) {
   let material=this.horizonMaterials.get(original);
   if(material)return material;
   const defines={};
   for(const key of ['ALPHA_TEST','BLENDED','ADDITIVE'])if(key in (original.defines||{}))defines[key]='';
   material=ownedEffectMaterial('effects/storm-horizon', {name:'storm-horizon',defines,uniforms:{map:original.uniforms?.map||{value:original.map||null},hazeColor:this.uniforms.hazeColor,
       lightningFlash:this.uniforms.lightningFlash,lightningPosition:this.uniforms.lightningPosition},
     transparent:false,depthTest:false,depthWrite:false,side:original.side,alphaToCoverage:original.alphaToCoverage,
     blending:original.blending,blendSrc:original.blendSrc,blendDst:original.blendDst,blendEquation:original.blendEquation});
   this.horizonMaterials.set(original,material);
   return material;
 }
 restoreLight(){
   if(!this.previousFlash)return;
   for(let i=0;i<this.lightKeys.length;i++)lighting[this.lightKeys[i]].value.copy(this.lightBase[i]);
   this.previousFlash=0;
 }
 strike(time,camera) {
   this.strikeStart=time;this.nextStrike=time+7+this.random()*11;this.strikes++;
   // Distributed around the storm, never locked to the player's reticle.
   const angle=this.random()*Math.PI*2,distance=430+this.random()*220;
   const x=camera.position.x+Math.cos(angle)*distance,z=camera.position.z+Math.sin(angle)*distance;
   const top=new THREE.Vector3(x,300+this.random()*100,z);this.uniforms.lightningPosition.value.copy(top);
   let count=0;
   const segment=(a,b,width,energy)=>{
     for(const layer of [1,4])for(const [end,edge] of [[0,-1],[1,-1],[0,1],[0,1],[1,-1],[1,1]]){
       const p=end?b:a;this.positions.set([p.x,p.y,p.z],count*3);this.tangents.set([b.x-a.x,b.y-a.y,b.z-a.z],count*3);
       this.edges[count]=edge;this.widths[count]=width*layer;this.energies[count]=energy*(layer===1?1:.16);count++;
     }
   };
   let a=top.clone();
   const steps=28,bottom=-8;
   for(let i=1;i<=steps;i++){
     const b=new THREE.Vector3(a.x+(this.random()-.5)*22,top.y+(bottom-top.y)*i/steps,a.z+(this.random()-.5)*22);
     segment(a,b,.7,1);
     if(i>3&&i<22&&i%5===0){
       let branch=a.clone();const dx=(this.random()-.5)*30,dz=(this.random()-.5)*30;
       for(let j=0;j<6;j++){const tip=new THREE.Vector3(branch.x+dx+(this.random()-.5)*10,branch.y-10-this.random()*12,branch.z+dz);segment(branch,tip,.45,(1-j/7)*.65);branch=tip;}
     }
     a=b;
   }
   this.boltGeometry.setDrawRange(0,count);for(const attr of Object.values(this.boltGeometry.attributes))attr.needsUpdate=true;
 }
 update(time,camera) {
   if(!this.enabled)return;
   this.restoreLight();
   if(time>=this.nextStrike)this.strike(time,camera);
   const age=time-this.strikeStart;
   const pulse=(start,span,power)=>age<start||age>start+span?0:power*Math.sin(Math.PI*(age-start)/span);
   const flash=pulse(0,.12,1)+pulse(.19,.08,.6)+pulse(.34,.19,.8);
   this.uniforms.lightningFlash.value=flash;this.bolt.visible=flash>.005;
   if(flash>0){
     for(let i=0;i<this.lightKeys.length;i++){
       const value=lighting[this.lightKeys[i]].value;this.lightBase[i].copy(value);
       value.x+=flash*.10;value.y+=flash*.13;value.z+=flash*.18;
     }
     this.previousFlash=flash;
   }
   this.uniforms.eyePosition.value.copy(camera.position);
   const renderer=this.graphics.renderer;
   if(!renderer||time-this.lastCloud<.1&&this.lastEye.distanceToSquared(camera.position)<256)return;
   this.lastCloud=time;this.lastEye.copy(camera.position);this.cloudRenders=(this.cloudRenders||0)+1;
   const target=renderer.getRenderTarget(),autoClear=renderer.autoClear,shadowAuto=renderer.shadowMap.autoUpdate;
   try{
     renderer.shadowMap.autoUpdate=false;renderer.autoClear=false;
     // The Rust skyClouds phase draws the cloud layers.
     renderer.setRenderTarget(this.target);this.graphics.rust.cover('skyClouds',renderer,this.cloudPass,this.passCamera);
     this.uniforms.cloudReady.value=1;
   } finally {renderer.setRenderTarget(target);renderer.autoClear=autoClear;renderer.shadowMap.autoUpdate=shadowAuto;}
 }
 dispose(){this.setEnabled(false);this.sky.removeFromParent();this.bolt.removeFromParent();this.sky.material.dispose();this.cloudPass.material.dispose();this.boltGeometry.dispose();this.bolt.material.dispose();this.target.dispose();this.noise.dispose();
   for(const material of this.horizonMaterials.values())material.dispose();this.horizonMaterials.clear();}
}
