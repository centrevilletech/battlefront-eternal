import { ownedEffectMaterial } from './rust-render/effect-material.js';
import { COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT } from "./render-resources.js";
import * as THREE from 'three';
import { lighting } from './materials.js';
import { JET_APEX, TIER } from './fountain-jets.js';




// The basins' and tiers' meshes drawn with the water's material, each to its
// FountainWater (a renderer that draws them itself finds them here).
export const fountainWaters = new WeakMap();

// Basin water uses the opaque scene copy and analytic sky/plume reflection.
// No shadow updates, per-basin scene renders, or synchronous GL queries.
export class FountainWater {
  // tier: Theed's two-tier basins (TIER, fountain-jets.js), whose inner tier
  // Enhanced fills to its lip; the stock basin mesh is one flat disc.
  constructor(graphics,basins,wakes,foamMap=null,tier=null) {
    this.graphics=graphics;this.basins=basins;this.time=0;this.enabled=false;
    this.surface=new THREE.WebGLRenderTarget(1,1,{depthTexture:new THREE.DepthTexture(1,1,THREE.UnsignedIntType)});
    // The unported shader still declares this sampler; bind a valid empty
    // texel while its reflection comes from the screen copy and analytic sky.
    this.emptyReflection=new THREE.DataTexture(new Uint8Array(4),1,1);this.emptyReflection.needsUpdate=true;
    this.reflectionUniforms={reflection:{value:this.emptyReflection},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0}};
    // A bright overcast stand-in for the sky the basins and jets reflect; the
    // full-scene mirror re-render (20 times a second) is no longer drawn.
    this.sky=new THREE.Vector3(.7,.72,.75);
    this.material=ownedEffectMaterial("effects/fountain-water", {transparent:true,depthWrite:false,side:THREE.DoubleSide,
      uniforms:{...this.reflectionUniforms,sky:{value:this.sky},waterTime:{value:0},sceneColor:{value:this.surface.texture},sceneDepth:{value:this.surface.depthTexture},
        foamMap:{value:foamMap},hasFoam:{value:foamMap?1:0},hasTier:{value:tier?1:0},waterSunDirection:lighting.sunDirection,waterSunColor:lighting.sunColor,
        waterResolution:{value:new THREE.Vector2()},cameraRange:{value:new THREE.Vector2()},wakes:{value:wakes.points},motion:{value:wakes.motion},
        basins:{value:Array.from({length:5},(_,i)=>new THREE.Vector4(...(basins[i]?.center||[1e6,1e6,1e6]),basins[i]?.radius||0))}},toneMapped:false});
    this.original=[];
    graphics.scene.traverse(mesh=>{if(mesh.isMesh&&basins.some(b=>b.texture===mesh.name))
      this.original.push({mesh,material:mesh.material,order:mesh.renderOrder,before:mesh.onBeforeRender});});
    // The tiers' pools, filled to their lips: 12-sided discs, one draw for all.
    if(tier){
      const position=[],index=[];
      for(const b of basins){
        const base=position.length/3,[x,y,z]=b.center;position.push(x,y+tier.rise,z);
        for(let k=0;k<=tier.sides;k++){const a=k/tier.sides*Math.PI*2;position.push(x+Math.cos(a)*tier.lip,y+tier.rise,z+Math.sin(a)*tier.lip);}
        for(let k=1;k<=tier.sides;k++)index.push(base,base+k+1,base+k);
      }
      const geometry=new THREE.BufferGeometry();geometry.setAttribute('position',new THREE.Float32BufferAttribute(position,3));geometry.setIndex(index);
      this.tier=new THREE.Mesh(geometry,this.material);this.tier.name='fountain-tier-water';this.tier.renderOrder=-16.9;
      this.tier.frustumCulled=false;this.tier.visible=false;this.tier.onBeforeRender=(renderer,scene,camera)=>this.capture(renderer,camera);
      graphics.scene.add(this.tier);fountainWaters.set(this.tier,this);
    }
  }
// This setting changes the basin's authored material, not its drawer.
// The original material and hooks are retained for the disabled appearance.
// Whichever appearance is selected still has to be claimed by Rust before
// the near scene can draw; changing water quality cannot enable a native pass.
// Tier pools share that material and participate in the same ordered capture.
  setEnabled(enabled){
    if(enabled===this.enabled)return;this.enabled=enabled;this.reflectionUniforms.hasReflection.value=0;
    for(const o of this.original){o.mesh.material=enabled?this.material:o.material;o.mesh.renderOrder=enabled?-17:o.order;
      o.mesh.onBeforeRender=enabled?(renderer,scene,camera)=>this.capture(renderer,camera):o.before;
      if(enabled)fountainWaters.set(o.mesh,this);else fountainWaters.delete(o.mesh);}
    if(this.tier)this.tier.visible=enabled;
  }
  update(time){this.time=time;this.material.uniforms.waterTime.value=time;this.captured=false;
    const top=lighting.ambientTop.value;this.sky.set(Math.min(1,top.x*1.4+.15),Math.min(1,top.y*1.4+.16),Math.min(1,top.z*1.4+.2));}
  // The composite samples the opaque colour/depth at its ordered boundary.
  // Copy only once for this frame and only from the main scene target; mirrors
  // have their own inputs and cannot reuse a partially drawn main surface.
  // The Rust resource bank owns the colour/depth copy; the transparent pass
  // samples it later at this surface's authored position in the draw order.
  capture(renderer,camera){
    const source=renderer.getRenderTarget();if(this.captured||source!==this.graphics.sceneTarget)return;
    this.captured=true;this.surface.setSize(source.width,source.height);
    renderer.resources.copy(source,this.surface,COLOR_BUFFER_BIT|DEPTH_BUFFER_BIT);
    this.material.uniforms.waterResolution.value.set(source.width,source.height);
    this.material.uniforms.cameraRange.value.set(camera.near,camera.far);
  }
  // Start another frame's opaque copy. Basin reflection samples that copy;
  // its analytic surface has no separate planar scene camera.
  reflect() {this.captured=false;this.reflectionUniforms.hasReflection.value=0;}
  // The meshes drawn with the water's material now (the basins' while it is
  // on, the tiers' pools).
  meshes(){return [...(this.enabled?this.original.map((o)=>o.mesh):[]),...(this.tier?[this.tier]:[])];}
  dispose(){this.setEnabled(false);this.tier?.removeFromParent();this.tier?.geometry.dispose();this.material.dispose();this.emptyReflection.dispose();this.surface.dispose();}
}
