import {sceneProperty} from "./rust-render/scene-changes.js";
import * as THREE from 'three';
import { EFFECT } from './rust-render/effect-layout.js';
import { StormSky } from './storm-sky.js';
import { SkyProbe } from './sky-probe.js';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// EnhancedOcean's meshes, seeds and materials are native. StormSky still has
// actual screen materials until the shared cloud screen sampler is converted.
export class EnhancedOcean {
  constructor(owner,{storm=false,look={},surfaceHost}={}) {
    this.owner=owner;const graphics=owner.graphics;this.probe=storm?null:new SkyProbe(graphics);
    if(storm){
      // These uniforms belong to the existing cloud screen, not ocean draw
      // descriptors. Native ocean defaults compile independently from the look.
      const wind=new THREE.Vector2(...(look.WindDirection||[1,0])).normalize();if(!wind.lengthSq())wind.set(1,0);
      const amplitude=Math.min(2,Math.max(.15,(look.WindSpeed?.[0]||0)*.05));
      this.uniforms={wind:{value:wind},time:{value:0},amplitude:{value:amplitude},stormStrength:{value:Math.min(1,Math.max(.15,amplitude/1.25))},
        rainRipples:{value:1},hazeColor:{value:new THREE.Color(.065,.085,.11)},gridOffset:{value:new THREE.Vector2(...owner.data.offset)},gridWidth:{value:owner.data.cell*owner.data.grid},
        sunDirection:lighting.sunDirection,sunColor:lighting.sunColor};
      this.storm=new StormSky({graphics,uniforms:this.uniforms,surfaceHost});
    }else this.storm=null;
    const row=storm?EFFECT.OCEAN_SPRAY_STORM:EFFECT.OCEAN_SPRAY;
    const batch=this.batch={nativeKind:row.ID,family:'oceanSpray',variant:0,texture:null,count:384,capacity:384,arrays:{},values:new Float64Array(row.LENGTH),visibility:'anchor',waterOwner:owner};
    this.spray=batch.mesh=new THREE.Object3D();this.spray.userData.effectBatch=batch;this.spray.name='ocean-crest-spray';sceneProperty(this.spray,"frustumCulled",false);sceneProperty(this.spray,"renderOrder",19);
    batch.prepareResources=resources=>{owner.prepareResources(graphics.rust);return Int32Array.of(owner.maskSample);};
    this.haze=storm?owner.anchor(EFFECT.OCEAN_HAZE,true,10):null;
    // The source object is retained too; no per-frame wrapper allocation.
    const source={mesh:this.spray,row,values:batch.values};
    this.spray.onBeforeRender=(renderer,scene,camera)=>{particleDepth.capture(renderer,scene,camera);owner.prepareSource(source,camera);};
  }
  setEnabled(enabled){sceneProperty(this.spray,"visible",enabled);if(this.haze)sceneProperty(this.haze,"visible",enabled);this.storm?.setEnabled(enabled);if(this.probe)this.probe.enabled=enabled;}
  update(time,camera,height,result){if(this.uniforms)this.uniforms.time.value=time;this.storm?.update(time,camera,result);this.probe?.update(time,camera);}
  dispose(){this.storm?.dispose();this.probe?.dispose();this.spray.removeFromParent();this.haze?.removeFromParent();}
}
