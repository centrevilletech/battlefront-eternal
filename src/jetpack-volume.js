import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
const ZERO = new THREE.Vector3();

// A small, advected 3D density field. Linear hardware filtering replaces dozens
// of procedural noise operations at each integration step. Shared by both jets.
function noiseTexture() {
  const size = 32, data = new Uint8Array(size ** 3);
  let seed = 0x6d2b79f5;
  for (let i = 0; i < data.length; i++) {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    data[i] = seed >>> 24;
  }
  const texture = new THREE.Data3DTexture(data, size, size, size);
  texture.format = THREE.RedFormat;
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.wrapS = texture.wrapT = texture.wrapR = THREE.RepeatWrapping;
  texture.unpackAlignment = 1; texture.needsUpdate = true;
  return texture;
}




// The volumes by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a volume is `external`, three is shown
// it never and uploads it nothing. Its clock is its own (flameTime, not the
// frame's time).
export const jetpackVolumes = new WeakMap();

export class JetpackVolume {
  constructor({scene,count}) {
    this.external=false;this.count=0;
    const box=new THREE.BoxGeometry(1,1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=box.index;geometry.attributes.position=box.attributes.position;
    for(const [name,size] of Object.entries({nozzle:3,direction:3,bend:3,nozzleRadius:1,phase:1}))
      geometry.setAttribute(name,new THREE.InstancedBufferAttribute(new Float32Array(count*size),size).setUsage(THREE.DynamicDrawUsage));
    for(let i=0;i<count;i++)geometry.attributes.phase.array[i]=i*17.31;
    this.noise=noiseTexture();
    const material=ownedEffectMaterial('effects/flame', {uniforms:{flameTime:{value:0},power:{value:1},noiseField:{value:this.noise},
      sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange,hasDepth:{value:0}},
      side:THREE.DoubleSide,forceSinglePass:true,transparent:true,premultipliedAlpha:true,depthWrite:false,toneMapped:false});
    this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='jetpack-volumetric-combustion';
    this.mesh.frustumCulled=false;this.mesh.renderOrder=10;this.mesh.visible=false;geometry.instanceCount=0;
    this.mesh.onBeforeRender=(renderer,scene,camera)=>{
      material.uniforms.hasDepth.value=particleDepth.capture(renderer,scene,camera,true)?1:0;
      material.uniformsNeedUpdate=true;
    };
    jetpackVolumes.set(this.mesh,this);
    scene.add(this.mesh);
  }
  update(time,power,nozzles) {
    const mesh=this.mesh,a=mesh.geometry.attributes,shown=power>0.015;
    this.count=shown?nozzles.length:0;
    mesh.visible=shown&&!this.external;mesh.geometry.instanceCount=this.count;
    if(!shown)return;
    mesh.material.uniforms.flameTime.value=time;mesh.material.uniforms.power.value=Math.max(power,0.025);
    for(let i=0;i<nozzles.length;i++) {
      const n=nozzles[i];n.position.toArray(a.nozzle.array,i*3);n.direction.toArray(a.direction.array,i*3);
      (n.bendWorld||ZERO).toArray(a.bend.array,i*3);a.nozzleRadius.array[i]=n.radius||0.034;
    }
    // Drawn by another renderer (external), three uploads it nothing.
    if(!this.external)for(const [name,attribute] of Object.entries(a))if(attribute.isInstancedBufferAttribute&&name!=='phase')attribute.needsUpdate=true;
  }
  clear(){this.count=0;this.mesh.visible=false;this.mesh.geometry.instanceCount=0;}
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();this.noise.dispose();}
}
