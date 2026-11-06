import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { StormSky } from './storm-sky.js';
import { SkyProbe } from './sky-probe.js';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// Concentric square rings share every edge. Detail follows the viewer, while
// waves and foam stay in world space. Cost does not depend on the map's size.
export function stormGeometry() {
  const sides = 64, rings = 128, stride = sides * 4;
  const positions = [0, 0, 0], indices = [];
  for (let ring = 1; ring <= rings; ring++) {
    const radius = ring <= 64 ? ring * 3 : 192 * (4096 / 192) ** ((ring - 64) / 64);
    for (let side = 0; side < 4; side++) for (let i = 0; i < sides; i++) {
      const t = -1 + 2 * i / sides;
      const x = [t, 1, -t, -1][side], z = [-1, t, 1, -t][side];
      positions.push(x * radius, 0, z * radius);
      const next = (side * sides + i + 1) % stride;
      const a = 1 + (ring - 1) * stride + side * sides + i, b = 1 + (ring - 1) * stride + next;
      if (ring === 1) indices.push(0, b, a);
      else { const c = a - stride, d = b - stride; indices.push(c, b, a, c, d, b); }
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  g.setIndex(indices);
  return g;
}

// Deep-water dispersion. Horizontal steepness sums to < 1, so the surface
// cannot fold over itself. Derivatives supply smooth geometric normals and
// compression, which is also used by spray at its emission time.








// A horizon veil only in the far scene. Depth-tested behind scenery; the ocean
// uses the same extinction colour so water and sky meet without a black seam.



// The Enhanced ocean: waves for any sea, as big as its look's wind makes
// them, with crest spray. On a sea the map's weather rains on (Kamino) the
// storm comes with them: the storm sky's panorama and lightning
// (storm-sky.js), the horizon veil, rain rings and the storm's dark palette.
// A sea under a dry sky (Kashyyyk: Islands) keeps the map's own sky, which it
// reflects and fades into (sky-probe.js), in its look's colours and its sun.
export class EnhancedOcean {
 constructor(shared,graphics,{storm=false,look={}}={}) {
   this.uniforms={...shared,stormStrength:{value:Math.min(1,Math.max(.15,shared.amplitude.value/1.25))},rainRipples:{value:storm?1:0},
     hazeColor:{value:new THREE.Color(.065,.085,.11)},eye:{value:new THREE.Vector2()},seaLevel:{value:0},
     sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange};
   const defines=storm?{STORM:''}:{};
   this.storm=storm?new StormSky({graphics,uniforms:this.uniforms}):null;
   this.probe=storm?null:new SkyProbe(graphics);
   if(this.probe){
     const [r,g,b]=look.RefractionColor||[45,53,74],reflect=look.ReflectionColor?.[3]??255;
     Object.assign(this.uniforms,{skyProbe:{value:this.probe.texture},waterColor:{value:new THREE.Color(r/255,g/255,b/255)},
       reflectAmount:{value:reflect/255},ambient:lighting.ambient});
   }
   this.geometry=stormGeometry();
   this.material=ownedEffectMaterial('effects/ocean', {defines,uniforms:this.uniforms,side:THREE.DoubleSide});
   const quad=new THREE.PlaneGeometry(2,2),g=new THREE.InstancedBufferGeometry();
   g.setIndex(quad.index.clone());g.setAttribute('position',quad.attributes.position.clone());g.setAttribute('uv',quad.attributes.uv.clone());quad.dispose();
   const seeds=new Float32Array(384*4);
   for(let i=0;i<384;i++)seeds.set([(i%24+.5)/24,(Math.floor(i/24)+.5)/16,(i*.6180339)%1,(i*.7548776)%1],i*4);
   g.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));g.instanceCount=384;
   this.spray=new THREE.Mesh(g,ownedEffectMaterial('effects/ocean-spray', {defines,uniforms:this.uniforms,transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true}));
   this.spray.name='ocean-crest-spray';this.spray.frustumCulled=false;this.spray.renderOrder=19;
   this.spray.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);
   this.haze=null;
   if(storm){
     this.haze=new THREE.Mesh(new THREE.SphereGeometry(850,32,12),ownedEffectMaterial('effects/ocean-haze', {uniforms:this.uniforms,transparent:true,depthWrite:false,side:THREE.BackSide}));
     this.haze.name='ocean-horizon-haze';this.haze.frustumCulled=false;this.haze.renderOrder=10;
   }
 }
 setEnabled(enabled){
   this.spray.visible=enabled;if(this.haze)this.haze.visible=enabled;
   this.storm?.setEnabled(enabled);if(this.probe)this.probe.enabled=enabled;
 }
 update(camera,height){
   this.uniforms.eye.value.set(camera.position.x,camera.position.z);this.uniforms.seaLevel.value=height;
   this.storm?.update(this.uniforms.time.value,camera);this.probe?.update(this.uniforms.time.value,camera);
 }
 dispose(){
   this.storm?.dispose();this.probe?.dispose();
   for(const mesh of [this.spray,this.haze])if(mesh){mesh.removeFromParent();mesh.geometry.dispose();mesh.material.dispose();}
   this.geometry.dispose();this.material.dispose();
 }
}
