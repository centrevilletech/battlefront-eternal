import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

// Short splashes where a character wades or lands in a basin, on the GPU; CPU
// work is the five fountain sources (shared with fountain-jets.js, which also
// throws the jets' own drops) and the eight wake slots.
export class FountainSpray {
  constructor(scene,wakes){
    const plane=new THREE.PlaneGeometry(1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=plane.index;geometry.attributes.position=plane.attributes.position;geometry.attributes.uv=plane.attributes.uv;
    const count=8*24,seed=new Float32Array(count*4);let state=137;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<count;i++)seed.set([Math.floor(i/24),random(),random(),random()],i*4);
    geometry.setAttribute('seed',new THREE.InstancedBufferAttribute(seed,4));geometry.instanceCount=count;
    const sources=Array.from({length:5},()=>new THREE.Vector4(0,0,0,0));
    const material=ownedEffectMaterial('effects/spray-s0', {transparent:true,depthWrite:false,toneMapped:false,uniforms:{sprayTime:{value:0},sources:{value:sources},
      wakes:{value:wakes.points},motion:{value:wakes.motion},sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,
      cameraRange:particleDepth.cameraRange,hasDepth:{value:0}}});
    this.sources=sources;this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='fountain-wake-spray';
    this.mesh.frustumCulled=false;this.mesh.visible=false;this.mesh.renderOrder=11;
    this.mesh.onBeforeRender=(r,s,c)=>{material.uniforms.hasDepth.value=particleDepth.capture(r,s,c,true)?1:0;material.uniformsNeedUpdate=true;};scene.add(this.mesh);
  }
  // Each fountain keeps its slot, so one leaving view doesn't reseed another.
  update(time,fountains){this.mesh.material.uniforms.sprayTime.value=time;this.mesh.visible=fountains.length>0;
    for(const s of this.sources)s.w=0;
    for(const f of fountains)this.sources[f.slot]?.set(f.position.x,f.position.y,f.position.z,1);}
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
