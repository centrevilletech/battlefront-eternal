import {sceneProperty} from "./rust-render/scene-changes.js";
import * as THREE from 'three';
import {EFFECT} from './rust-render/effect-layout.js';
import {ArcLightning} from './arc-lightning.js';
import {LightningCounts as H} from './presentation/lightning-layout.js';
// The scene registry consumes only Rust-produced Classic ribbon buffers and
// asset identities. Enhanced capsules use the same persistent native owner.
export const classicLightning=new WeakMap();
export class Lightning {
 constructor(scene,texture,options={}){
  this.scene=scene;this.texture=texture;this.options=options;this.settings=options.settings||null;this.enhanced=null;
  this.owner=new ArcLightning(scene,options,false);this.maps=new Map();this.map=null;
  this.nativeKind=EFFECT.LIGHTNING.ID;this.values=new Float64Array(0);this.arrays={};for(const [name,key]of [["position","positions"],["uv","uvs"],["color","colors"]])Object.defineProperty(this.arrays,name,{get:()=>{this.owner.refresh();return this.owner[key];}});
  this.mesh=new THREE.Object3D();sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",5);sceneProperty(this.mesh,"visible",false);scene.add(this.mesh);
  let root=scene;while(root.parent)root=root.parent;classicLightning.set(root,this);this.vertex=0;this.index=0;this.refresh();if(this.isEnhanced())this.arcs();
 }
 refresh(){this.owner.refresh();}
 get positions(){this.owner.refresh();return this.owner.positions;}
 get uvs(){this.owner.refresh();return this.owner.uvs;}
 get colors(){this.owner.refresh();return this.owner.colors;}
 get indices(){this.owner.refresh();return this.owner.indices;}
 textureFor(name){if(!this.maps.has(name))this.maps.set(name,this.texture(name));return this.maps.get(name);}
 spawn(def,start,end,charge,follow=null){this.textureFor(def.Texture);const enhanced=this.isEnhanced();if(enhanced)this.arcs();this.owner.spawn(def,start,end,charge,follow,true,enhanced);this.refresh();}
 isEnhanced(){return (this.settings?.get?.('arcCasterEffects')??'enhanced')!=='classic';}
 arcs(){if(!this.enhanced){this.owner.enable();this.enhanced=this.owner;}return this.enhanced;}
 update(dt,camera){const enhanced=this.isEnhanced();if(enhanced)this.arcs();this.owner.update(dt,camera,enhanced);this.refresh();this.vertex=this.owner.counts[H.VERTICES];this.index=this.owner.counts[H.INDICES];const def=this.owner.defs[this.owner.counts[H.TEXTURE]];if(def)this.map=this.textureFor(def.Texture);}
}
