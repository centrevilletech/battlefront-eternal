import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {EnvironmentAdmission as ADMISSION,EnvironmentDraw as DRAW,FountainModel as MODEL} from "./environment/layout.js";
import {admitEnvironment,drawValues,environmentTransfer} from './environment/module.js';
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
    const recipe=admitEnvironment(ADMISSION.FOUNTAIN,{basins,tier:Boolean(tier),foam:Boolean(foamMap),sky:lighting.ambientTop.value.toArray()},wakes.host.owner);
    drawValues(wakes.host,this,DRAW.FOUNTAIN,ROW);this.samples=new Int32Array(4);this.samples.fill(-1);
    this.basinData=this.values.subarray(ROW.BASINS,ROW.BASINS+20);this.tierData=this.basinData.subarray(0,recipe.count*4);
    wakes.host.bindArray(this,'basinData',()=>this.values.subarray(ROW.BASINS,ROW.BASINS+20));
    wakes.host.bindArray(this,'tierData',()=>this.basinData.subarray(0,recipe.count*4));
    const basinTextures=new Set(recipe.textures);
    // EnvironmentVolume consumes these reflection facts. Analytic basin water
    // has no planar reflection image; it samples its native opaque copy.
    this.reflectionUniforms={reflection:{value:null},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0}};
    this.original=[];this.models=[];
    const source=(mesh,tier=false)=>{
      const row={mesh,tier,owner:this,active:false,center:null};mesh.userData.effectModel=row;
      fountainWaters.set(mesh,this);this.models.push(row);return row;
    };
    graphics.scene.traverse(mesh=>{if(mesh.isMesh&&basinTextures.has(mesh.name)){
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
    environmentTransfer(wakes.host.owner,'page_fountain_models',this.original.map(o=>({visible:o.visible,order:o.order})));
    wakes.host.bindArray(this,'modelValues',h=>new Float64Array(h.buffer,h.x.page_fountain_models_ptr(),this.original.length*MODEL.STRIDE));
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
    if(!this.wakes.host.x.page_fountain_mode(enabled?1:0))return;this.enabled=enabled;
    for(const row of this.models){row.active=enabled;sceneProperty(row.mesh,"visible",enabled);}
    for(let k=0;k<this.original.length;k++){const o=this.original[k],at=k*MODEL.STRIDE;sceneProperty(o.mesh,'visible',Boolean(this.modelValues[at+MODEL.VISIBLE]));sceneProperty(o.mesh,'renderOrder',this.modelValues[at+MODEL.ORDER]);}
    if(this.tier)sceneProperty(this.tier,"visible",enabled);
  }
  // The enclosing native phase supplies time. Isolated resource consumers
  // can publish that same scalar without advancing an environment clock.
  update(time) {
    this.time=time;this.captured=false;this.values[ROW.WATER_TIME]=time;
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
