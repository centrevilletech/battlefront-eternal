import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import { EFFECT, EFFECT_TARGET } from './rust-render/effect-layout.js';
import { COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT } from './render-resources.js';
import * as THREE from 'three';
import { lighting } from './materials.js';

// Existing authored basin meshes still own their disabled appearance. The
// enhanced water is a native descriptor, not a replacement Three material.
export const fountainWaters=new WeakMap();
const ROW=EFFECT.FOUNTAIN_WATER;
export class FountainWater {
  constructor(graphics,basins,wakes,foamMap=null,tier=null) {
    this.graphics=graphics;this.basins=basins;this.wakes=wakes;this.foamMap=foamMap;this.hasTier=Boolean(tier);
    this.time=0;this.enabled=false;this.captured=false;this.resources=null;this.surface=null;
    this.values=new Float64Array(ROW.LENGTH);this.basinData=new Float64Array(20);this.samples=new Int32Array(4);this.samples.fill(-1);
    this.values[ROW.HAS_TIER]=tier?1:0;this.values[ROW.HAS_FOAM]=foamMap?1:0;
    for(let i=0;i<5;i++){const b=basins[i],at=i*4;this.basinData[at]=b?.center[0]??1e6;this.basinData[at+1]=b?.center[1]??1e6;this.basinData[at+2]=b?.center[2]??1e6;this.basinData[at+3]=b?.radius??0;}
    this.values.set(this.basinData,ROW.BASINS);this.tierData=this.basinData.subarray(0,Math.min(5,basins.length)*4);
    // EnvironmentVolume consumes these reflection facts. Analytic basin water
    // has no planar reflection image; it samples its native opaque copy.
    this.reflectionUniforms={reflection:{value:null},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0}};
    this.original=[];this.models=[];
    const source=(mesh,tier=false)=>{
      const row={mesh,tier,owner:this,active:false,center:null};mesh.userData.effectModel=row;
      fountainWaters.set(mesh,this);this.models.push(row);return row;
    };
    graphics.scene.traverse(mesh=>{if(mesh.isMesh&&basins.some(b=>b.texture===mesh.name)){
      this.original.push({mesh,visible:mesh.visible,order:mesh.renderOrder});
      // Classic and enhanced appearances have independent native identities.
      // Hiding the authored surface must not hide its replacement's source.
      const anchor=new THREE.Object3D();anchor.name=mesh.name;sceneProperty(anchor,"matrixAutoUpdate",false);
      sceneTransform(anchor.matrix.copy(mesh.matrix));sceneProperty(anchor,"visible",false);sceneProperty(anchor,"renderOrder",-17);
      mesh.parent.add(anchor);const row=source(anchor);sceneProperty(row,"geometry",mesh.geometry);
      row.original=mesh;
    }});
    if(tier){this.tier=new THREE.Object3D();this.tier.name='fountain-tier-water';sceneProperty(this.tier,"renderOrder",-16.9);
      sceneProperty(this.tier,"visible",false);source(this.tier,true);graphics.scene.add(this.tier);}
  }
  // Ports FountainWater's shared resource setup. Rust owns disc topology,
  // material slots, sampler units and the copy target's attachments.
  prepareResources(resources) {
    if(!this.resources){this.resources=resources;this.surface=resources.effectTarget(EFFECT_TARGET.FOUNTAIN);
      this.samples[1]=this.surface.texture;this.samples[2]=this.surface.depthTexture;
      if(this.foamMap)this.samples[3]=resources.staticSample(this.foamMap);}
    return this.samples;
  }
  setEnabled(enabled) {
    if(enabled===this.enabled)return;this.enabled=enabled;
    for(const row of this.models){row.active=enabled;sceneProperty(row.mesh,"visible",enabled);}
    for(const o of this.original){sceneProperty(o.mesh,"visible",enabled?false:o.visible);sceneProperty(o.mesh,"renderOrder",enabled?-17:o.order);}
    if(this.tier)sceneProperty(this.tier,"visible",enabled);
  }
  update(time) {
    this.time=time;this.captured=false;const v=this.values,top=lighting.ambientTop.value;
    v[ROW.WATER_TIME]=time;v[ROW.SKY]=Math.min(1,top.x*1.4+.15);v[ROW.SKY+1]=Math.min(1,top.y*1.4+.16);v[ROW.SKY+2]=Math.min(1,top.z*1.4+.2);
    lighting.sunDirection.value.toArray(v,ROW.WATER_SUN_DIRECTION);lighting.sunColor.value.toArray(v,ROW.WATER_SUN_COLOR);
    v.set(this.wakes.host.wakePoints,ROW.WAKES);v.set(this.wakes.host.wakeMotion,ROW.MOTION);
  }
  // The native transparent model preserves FountainWater.capture's ordered
  // colour/depth copy. Resizing changes storage, never logical sampler ids.
  capture(renderer,camera) {
    const source=renderer.getRenderTarget();if(this.captured||source!==this.graphics.sceneTarget)return;
    this.captured=true;const resized=this.surface.width!==source.width||this.surface.height!==source.height;
    this.resources.resizeEffectTarget(this.surface,source.width,source.height);
    // Immutable depth storage gets a new texture on resize. The copy happens
    // after view publication's epoch check; repair retained native samplers
    // here so this same frame cannot bind the deleted depth attachment.
    if(resized)for(let i=0;i<this.samples.length;i++)if(this.samples[i]>=0)this.graphics.rust.texture(this.samples[i],true);
    renderer.resources.copy(source,this.surface,COLOR_BUFFER_BIT|DEPTH_BUFFER_BIT);
    this.values[ROW.WATER_RESOLUTION]=source.width;this.values[ROW.WATER_RESOLUTION+1]=source.height;
    this.values[ROW.CAMERA_RANGE]=camera.near;this.values[ROW.CAMERA_RANGE+1]=camera.far;
  }
  reflect(){this.captured=false;}
  dispose(){this.setEnabled(false);for(const row of this.models){delete row.mesh.userData.effectModel;fountainWaters.delete(row.mesh);if(row.original)row.mesh.removeFromParent();}this.tier?.removeFromParent();if(this.resources)this.resources.disposeEffectTarget(this.surface);}
}
