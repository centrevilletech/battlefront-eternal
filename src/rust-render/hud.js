// BattleHud and overlay batches share the native HUD bank. Rust owns topology,
// program/raster/sampler descriptors and camera uniform packing. The page sends
// live camera facts and dirty vertex ranges; it never inspects resource shells.
import * as THREE from 'three';
import {HudRenderFrame as L,HudDraw as D} from '../hud-layout.js';
import {UI_RESOURCE as U} from './interface-resource-layout.js';
import {imageSample} from '../interface/images.js';
const ARRAYS=[['position','hudPosition',3,'hud_position'],['uv','hudUv',2,'hud_uv'],['maskUv','hudMaskUv',2,'hud_mask_uv'],['tint','hudTint',4,'hud_tint']];
const MAX_GROUPS=U.LIMIT.HUD_GROUPS;
class HudPass {
 constructor(core){this.core=core;this.name='hud';this.phase='hud';this.pending=null;this.complete=false;this.full=true;this.init=false;this.viewport=new THREE.Vector4();this.looks=new WeakMap();}
 cover(renderer,hud){return this.drawBatch(renderer,hud);}
 // Cold appearances finish admission before the live shared views are taken.
 // Changing batch identity forces the full active prefix, including after loss.
 drawBatch(renderer,hud){const batch=hud.batch;if(batch.count===0&&hud.nativeMode===undefined)return true;if(this.batch!==batch||this.nativeMode!==hud.nativeMode){this.full=true;this.batch=batch;this.nativeMode=hud.nativeMode;}
  this.ensure();if(hud.nativeMode===undefined)for(let i=0;i<batch.drawGroups.length;i++)this.appearance(batch.looks[batch.drawGroups[i].materialIndex]);this.pending=hud;this.complete=false;this.core.publishView('hud',renderer,hud.scene,hud.camera);this.pending=null;return this.complete;
 }
 ensure(){if(this.init)return;const core=this.core;core.rust.hud_descriptor_init();for(let i=0;i<ARRAYS.length;i++){const a=ARRAYS[i],binding=a[3];core.addView(a[1],Float32Array,()=>core.rust[binding+'_ptr'](),()=>core.rust[binding+'_len']());}core.addView('hudInput',Float64Array,()=>core.rust.hud_input_ptr(),()=>core.rust.hud_input_len());this.init=true;}
 appearance(look){if(!look)return null;let record=this.looks.get(look);if(record)return record;const core=this.core,map=imageSample(core.resources,look.map),mask=imageSample(core.resources,look.mask??look.map);const id=core.rust.hud_appearance(map<0?-1:core.texture(map),mask<0?-1:core.texture(mask),look.masked,map,mask);record={id:id??-1};this.looks.set(look,record);core.buffer=null;core.views();return record;}
 draw(){const hud=this.pending;if(!hud)return;const core=this.core,batch=hud.batch;core.views();const input=core.hudInput;input[L.groups]=0;input[L.upload]=input[L.upload+1]=0;
  input.set(hud.camera.matrixWorldInverse.elements,L.camera+U.HUD.VIEW);input.set(hud.camera.projectionMatrix.elements,L.camera+U.HUD.PROJECTION);
  core.renderer.getViewport(this.viewport).multiplyScalar(core.renderer.getPixelRatio()).floor();input[L.viewport]=this.viewport.z;input[L.viewport+1]=this.viewport.w;
  if(hud.nativeMode===undefined){const first=this.full?0:batch.first,last=this.full?batch.count:batch.last;const start=first*4,end=last*4;
   if(end>start)for(let k=0;k<ARRAYS.length;k++){const a=ARRAYS[k],source=batch[a[0]],out=core[a[1]],size=a[2];for(let i=start*size;i<end*size;i++)out[i]=source[i];}
   input[L.upload]=start;input[L.upload+1]=end;
   let n=0;for(let i=0;i<batch.drawGroups.length&&n<MAX_GROUPS;i++){const group=batch.drawGroups[i],record=this.looks.get(batch.looks[group.materialIndex]);if(!record||record.id<0)continue;const first=group.start,last=Math.min(group.start+group.count,batch.count*6);if(last<=first)continue;const at=L.words+n++*D.words;input[at]=first;input[at+1]=last-first;input[at+2]=U.KIND.HUD;input[at+3]=record.id;input[at+4]=0;}input[L.groups]=n;
  }
  // The same coarse call handles native-model and shell-overlay quads. Its
  // expanded packet is captured by the existing frame owner without callbacks.
  core.rust.hud_resources_frame(core.resources.gpu,hud.nativeMode??-1,this.full);this.full=false;this.complete=true;
 }
}
export function hudPass(core){return new HudPass(core);}
