import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

export const WAKE_COUNT = 8;
// Fixed storage shared by water and fog. Sampling actor motion never allocates
// in the render loop, and stationary actors do not continuously excavate fog.
export class EnvironmentWakes {
  constructor() {
    this.points = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4(0, 0, 0, -100));
    this.motion = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4());
    this.states = new WeakMap(); this.cursor = 0; this.time = 0;
  }
  add(position, velocity, strength = 1) {
    const i = this.cursor++ % WAKE_COUNT;
    this.points[i].set(position.x, position.y, position.z, this.time);
    this.motion[i].set(velocity.x, velocity.y, velocity.z, strength);
  }
  update(dt, actors, relevant) {
    this.time += dt;
    for (const actor of actors) {
      if (!actor?.position) continue;
      let s = this.states.get(actor);
      if (!s) { s = {position: actor.position.clone(), velocity: new THREE.Vector3(), gap: 0}; this.states.set(actor, s); }
      s.velocity.copy(actor.position).sub(s.position).divideScalar(Math.max(dt, 1e-5));
      const speed = s.velocity.length();
      s.gap += dt;
      if (dt > 0 && s.gap >= .10 && speed > .3 && speed < 60 && relevant(actor.position)) {
        this.add(actor.position, s.velocity, Math.min(1.6, .4 + speed * .1)); s.gap = 0;
      }
      s.position.copy(actor.position);
    }
  }
  clear() { for (const p of this.points) p.w = -100; this.states = new WeakMap(); }
}

// Periodic, correlated fractal density. White noise averaged along a ray
// becomes an almost uniform haze; this field retains visible rolling eddies.
function noiseTexture() {
  const n=32,bytes=new Uint8Array(n**3);let seed=145231;
  const random=()=>{seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;return(seed>>>0)/4294967296;};
  const layers=[8,16,32].map(size=>({size,data:Float32Array.from({length:size**3},random)}));
  const smooth=t=>t*t*(3-2*t);
  function sample(layer,x,y,z){
    const size=layer.size,scale=size/n;x*=scale;y*=scale;z*=scale;
    const ix=Math.floor(x),iy=Math.floor(y),iz=Math.floor(z),fx=smooth(x-ix),fy=smooth(y-iy),fz=smooth(z-iz);
    let v=0;
    for(let k=0;k<2;k++)for(let j=0;j<2;j++)for(let i=0;i<2;i++)
      v+=layer.data[((iz+k)%size*size+(iy+j)%size)*size+(ix+i)%size]*(i?fx:1-fx)*(j?fy:1-fy)*(k?fz:1-fz);
    return v;
  }
  for(let z=0;z<n;z++)for(let y=0;y<n;y++)for(let x=0;x<n;x++){
    const v=.60*sample(layers[0],x,y,z)+.30*sample(layers[1],x,y,z)+.10*sample(layers[2],x,y,z);
    bytes[(z*n+y)*n+x]=Math.round(255*THREE.MathUtils.clamp(.5+(v-.5)*1.65,0,1));
  }
  const t=new THREE.Data3DTexture(bytes,n,n,n);t.format=THREE.RedFormat;
  t.minFilter=t.magFilter=THREE.LinearFilter;t.wrapS=t.wrapT=t.wrapR=THREE.RepeatWrapping;
  t.unpackAlignment=1;t.needsUpdate=true;return t;
}





// The composites' meshes, each to its volume (a renderer that draws the
// volumes itself finds them here).
export const environmentVolumes = new WeakMap();

export class EnvironmentVolume {
  constructor(scene, capacity, wakes, reflectionUniforms = {}) {
    const box = new THREE.BoxGeometry(1, 1, 1), geometry = new THREE.InstancedBufferGeometry();
    geometry.index = box.index; geometry.attributes.position = box.attributes.position;
    for (const [key, size] of Object.entries({origin:3,direction:3,extent:3,kind:1}))
      geometry.setAttribute(key, new THREE.InstancedBufferAttribute(new Float32Array(capacity * size), size).setUsage(THREE.DynamicDrawUsage));
    this.noise = noiseTexture();
    this.target = new THREE.WebGLRenderTarget(1,1,{depthBuffer:false,minFilter:THREE.NearestFilter,magFilter:THREE.NearestFilter});
    this.resolution={value:new THREE.Vector2(1,1)};
    const material = ownedEffectMaterial("effects/volume", {
      uniforms:{noiseField:{value:this.noise},volumeTime:{value:0},sceneDepth:particleDepth.map,
        volumeResolution:this.resolution,cameraRange:particleDepth.cameraRange,hasDepth:{value:1},
        wakes:{value:wakes.points},motion:{value:wakes.motion},reflection:{value:null},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0},...reflectionUniforms},
      transparent:true,depthWrite:false,depthTest:false,side:THREE.BackSide,forceSinglePass:true,premultipliedAlpha:true,toneMapped:false});
    material.userData.bfeNoShadows = true;
    this.volumeMesh = new THREE.Mesh(geometry, material);this.volumeMesh.frustumCulled=false;
    this.volumeScene=new THREE.Scene();this.volumeScene.add(this.volumeMesh);
    this.mesh=new THREE.Mesh(new THREE.PlaneGeometry(2,2),ownedEffectMaterial("effects/volume-composite", {

      uniforms:{volumeMap:{value:this.target.texture},sceneDepth:particleDepth.map,volumeResolution:this.resolution,
        depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange,enabled:{value:0}},
      transparent:true,premultipliedAlpha:true,depthWrite:false,depthTest:false,toneMapped:false}));
    this.mesh.name='environment-volumes';this.mesh.frustumCulled=false;this.mesh.renderOrder=10;this.mesh.visible=false;
    this.scale=.5;
    this.mesh.onBeforeRender=(renderer,scene,camera)=>this.prepare(renderer,scene,camera);
    environmentVolumes.set(this.mesh,this);
    scene.add(this.mesh);
  }
  // The scene's depth taken (whatever the setting), the composite told
  // whether it has it, and the volumes' target sized to the frame's: false
  // when there is nothing to draw (another render's, a mirror's).
  prepare(renderer,scene,camera){
    const composite=this.mesh.material;
    const captured=particleDepth.capture(renderer,scene,camera,true);
    composite.uniforms.enabled.value=captured?1:0;composite.uniformsNeedUpdate=true;
    if(!captured)return false;
    const source=renderer.getRenderTarget(),width=Math.max(1,Math.ceil(source.width*this.scale)),height=Math.max(1,Math.ceil(source.height*this.scale));
    this.target.setSize(width,height);this.resolution.value.set(width,height);
    return true;
  }

  update(time, volumes) {
    this.mesh.visible=volumes.length>0;this.volumeMesh.geometry.instanceCount=volumes.length;
    this.volumeMesh.material.uniforms.volumeTime.value=time;
    const attributes=this.volumeMesh.geometry.attributes;
    for(let i=0;i<volumes.length;i++){
      const v=volumes[i];v.position.toArray(attributes.origin.array,i*3);
      v.direction.toArray(attributes.direction.array,i*3);v.extent.toArray(attributes.extent.array,i*3);
      attributes.kind.array[i]=v.kind;
    }
    if(volumes.length)for(const a of Object.values(attributes))if(a.isInstancedBufferAttribute)a.needsUpdate=true;
  }
  dispose(){
    this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();
    this.volumeMesh.geometry.dispose();this.volumeMesh.material.dispose();this.noise.dispose();this.target.dispose();
  }
}
