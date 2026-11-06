import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

// Sparse, resolved wisps over the volumetric body retain the original steam's
// small curls. Their motion is local to each vent, including the angled pipes.
export class SteamWisps {
  constructor(scene,wakes,texture){
    const plane=new THREE.PlaneGeometry(1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=plane.index;geometry.attributes.position=plane.attributes.position;geometry.attributes.uv=plane.attributes.uv;
    const count=9*16,seeds=new Float32Array(count*4);let state=317;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<count;i++)seeds.set([Math.floor(i/16),random(),random(),random()],i*4);
    geometry.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));geometry.instanceCount=count;
    this.sources=Array.from({length:9},()=>new THREE.Vector4());this.directions=Array.from({length:9},()=>new THREE.Vector4());
    const material=ownedEffectMaterial('effects/wisps', {transparent:true,depthWrite:false,toneMapped:false,
      uniforms:{map:{value:texture},wispTime:{value:0},sources:{value:this.sources},directions:{value:this.directions},
        wakes:{value:wakes.points},motion:{value:wakes.motion},sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,
        cameraRange:particleDepth.cameraRange,hasDepth:{value:0}}});
    this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='chamber-steam-wisps';this.mesh.frustumCulled=false;
    this.mesh.visible=false;this.mesh.renderOrder=11;
    this.mesh.onBeforeRender=(r,s,c)=>{material.uniforms.hasDepth.value=particleDepth.capture(r,s,c,true)?1:0;material.uniformsNeedUpdate=true;};scene.add(this.mesh);
  }
  update(time,volumes){
    this.mesh.material.uniforms.wispTime.value=time;this.mesh.visible=volumes.length>0;
    for(let i=0;i<9;i++){const v=volumes[i];if(v){this.sources[i].set(v.position.x,v.position.y,v.position.z,1);this.directions[i].set(v.direction.x,v.direction.y,v.direction.z,v.kind);}else this.sources[i].w=0;}
  }
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
