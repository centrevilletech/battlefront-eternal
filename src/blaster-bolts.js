import { presentationModule } from "./presentation/module.js";
import {sceneProperty} from "./rust-render/scene-changes.js";
import { OrdnanceLimits as L } from "./presentation/ordnance-layout.js";
import { EFFECT } from "./rust-render/effect-layout.js";
import * as THREE from "three";
import { texturePixels } from "./texture-pixels.js";
import { particleDepth } from "./particle-depth.js";


export const boltLook = { enhanced: false };

// A scene anchor and borrowed native attribute arrays are the entire host.
export class BlasterBatch {
  constructor(){
    this.count=0;this.capacity=L.BLASTERS;this.attributes={};
    this.nativeKind=EFFECT.BLASTER.ID;this.variant=0;this.texture=null;
    this.values=new Float64Array(EFFECT.BLASTER.LENGTH);this.values[EFFECT.BLASTER.PIXEL_ANGLE]=.002;
    this.mesh=new THREE.Object3D();this.mesh.name="blaster-bolts";
    sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",11);
    const size=new THREE.Vector2();
    this.mesh.onBeforeRender=(renderer,scene,camera)=>{
      particleDepth.capture(renderer,scene,camera);
      const target=renderer.getRenderTarget();
      this.values[EFFECT.BLASTER.PIXEL_ANGLE]=presentationModule().module.PageOrdnance.bolt_view(!!camera.isPerspectiveCamera,camera.fov??NaN,camera.zoom,target?target.height:renderer.getDrawingBufferSize(size).y,this.values[EFFECT.BLASTER.PIXEL_ANGLE]);
    };
  }
  begin(){this.count=0;}
  // Another presentation owner may grow the shared memory after ordnance
  // finished. Rebind before the renderer borrows its attributes.
  get arrays(){
    const owner=this.sourceOwner;
    if(owner&&this.buffer!==owner.loaded.memory.buffer){owner.refresh();this.load(owner.blasters,this.count,owner);}
    return this.attributes;
  }
  load(source,count,owner){
    this.sourceOwner=owner;this.buffer=source[0].buffer;
    this.attributes.iHead=source[0];this.attributes.iTail=source[1];this.attributes.iColor=source[2];this.attributes.iSize=source[3];this.count=count;
  }
  end(){sceneProperty(this.mesh,"visible",this.count>0);}
}

// The sheath colour of a LaserGlowColor: its hue at full strength, so a dim
// authored alpha doesn't make the bolt grey.
export function sheathColor(rgba = [255, 255, 255, 255]) {
  return Array.from(presentationModule().module.PageOrdnance.sheath(rgba));
}

// Deeper, film-like saturation for a sheath colour; the strongest channel
// stays at 1, so the hue is kept and pale textures still read as coloured.
export function vivid(rgb, power = 1.6) {
  return Array.from(presentationModule().module.PageOrdnance.vivid(rgb, power));
}

const hues = new WeakMap();
export function textureHue(texture) {
  if (hues.has(texture)) return hues.get(texture);
  const pixels = texturePixels(texture);
  if (!pixels) return undefined;
  const values = presentationModule().module.PageOrdnance.texture_hue(pixels.data);
  const hue = values.length ? Array.from(values) : null;
  hues.set(texture, hue);
  return hue;
}
