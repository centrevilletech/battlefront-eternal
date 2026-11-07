import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { ownedFullScreenPass } from './render-passes.js';
import { SKY_ORDER } from './world.js';
import { SurfaceResult as RESULT } from './environment/layout.js';

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
 constructor({graphics,uniforms,surfaceHost}) {
   this.graphics=graphics;this.uniforms=uniforms;this.enabled=false;this.cloudGeneration=(this.cloudGeneration||0)+1;
   this.strikes=0;
   this.previousFlash=0;
   let randomState=8217;
   const random=()=>((randomState=(Math.imul(randomState,1664525)+1013904223)>>>0)/4294967296);
   const bytes=new Uint8Array(64**3);
   for(let i=0;i<bytes.length;i++)bytes[i]=random()*255;
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
   this.positions=surfaceHost.stormPositions;this.tangents=surfaceHost.stormTangents;
   this.edges=surfaceHost.stormEdges;this.widths=surfaceHost.stormWidths;this.energies=surfaceHost.stormEnergies;
   for(const [name,array,itemSize] of [['position',this.positions,3],['tangent',this.tangents,3],['edge',this.edges,1],['width',this.widths,1],['energy',this.energies,1]])
     this.boltGeometry.setAttribute(name,new THREE.BufferAttribute(array,itemSize).setUsage(THREE.DynamicDrawUsage));
   this.boltGeometry.setDrawRange(0,0);
   this.bolt=new THREE.Mesh(this.boltGeometry,ownedEffectMaterial('effects/storm-lightning', {uniforms,transparent:true,depthWrite:false,blending:THREE.AdditiveBlending,side:THREE.DoubleSide,forceSinglePass:true}));
   this.bolt.name='storm-lightning';this.bolt.frustumCulled=false;this.bolt.renderOrder=8;
   this.sky.visible=this.bolt.visible=false;graphics.farScene.add(this.sky,this.bolt);
 }
 setEnabled(enabled) {
   if(this.enabled===enabled)return;
   this.enabled=enabled;this.sky.visible=enabled;this.bolt.visible=false;
   if(enabled){
     // Every part of the dome gives way to the panorama; the models on its
     // horizon stay, in the storm's light.
     const graphics=this.graphics,layers=graphics.skyLayers?.length?graphics.skyLayers:graphics.sky?[{mesh:graphics.sky,layer:0}]:[];
     this.replaced=layers.map(({mesh,layer})=>({mesh,layer,visible:mesh.visible,material:mesh.material}));
     for(const {mesh,layer,material} of this.replaced){if(layer)mesh.material=this.horizonMaterial(material);else mesh.visible=false;}
     this.cloudGeneration=(this.cloudGeneration||0)+1;
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
   this.graphics.rust.cameras.restoreLight();
   this.previousFlash=0;
 }
 // StormSky.strike and pulse animation are native surface records. The
 // resource host publishes the bounded vertex prefix only after a strike.
 update(time,camera,result) {
   if(!this.enabled)return;
   this.restoreLight();
   this.strikes=result[RESULT.STORM_STRIKES];
   this.uniforms.lightningPosition.value.fromArray(result,RESULT.STORM_POSITION);
   if(result[RESULT.STORM_DIRTY]) {
     this.boltGeometry.setDrawRange(0,result[RESULT.STORM_COUNT]);
     const attributes=this.boltGeometry.attributes;
     attributes.position.needsUpdate=attributes.tangent.needsUpdate=attributes.edge.needsUpdate=attributes.width.needsUpdate=attributes.energy.needsUpdate=true;
   }
   const flash=result[RESULT.STORM_FLASH];
   this.uniforms.lightningFlash.value=flash;this.bolt.visible=flash>.005;
   if(flash>0){
     this.graphics.rust.cameras.flash(flash);
     this.previousFlash=flash;
   }
   this.uniforms.eyePosition.value.copy(camera.position);
   this.graphics.rust.frame.cloud(this,time,camera);
 }
 dispose(){this.setEnabled(false);this.sky.removeFromParent();this.bolt.removeFromParent();this.sky.material.dispose();this.cloudPass.material.dispose();this.boltGeometry.dispose();this.bolt.material.dispose();this.target.dispose();this.noise.dispose();
   for(const material of this.horizonMaterials.values())material.dispose();this.horizonMaterials.clear();}
}
