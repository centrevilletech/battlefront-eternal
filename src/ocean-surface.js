import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { lighting } from './materials.js';
import { EnhancedOcean } from './ocean-storm.js';
import { SurfaceResult as RESULT } from './environment/layout.js';

// Whether the Enhanced ocean storms: when the map's weather rains (any
// Precipitation but Type("Quads"), Hoth's snow, as game.js reads it). Kamino
// rains on its sea; Kashyyyk: Islands has no weather, and keeps its sky.
export function storms(weather) {
  const type=weather?.precipitation?.Type?.[0];
  return Boolean(weather?.precipitation)&&String(type??'').toLowerCase()!=='quads';
}

// Fixed-size, camera-following ocean: detailed nearby waves and a flat outer
// skirt. The authored water mask still determines where water exists.
export function oceanGeometry(reach = 4096) {
  const grid = new THREE.PlaneGeometry(512, 512, 64, 64).rotateX(-Math.PI / 2);
  const positions = Array.from(grid.attributes.position.array), indices = Array.from(grid.index.array);
  const inner = [[-256,-256],[256,-256],[256,256],[-256,256]];
  const outer = [[-reach,-reach],[reach,-reach],[reach,reach],[-reach,reach]];
  for (let i=0;i<4;i++) {
    const j=(i+1)%4, base=positions.length/3;
    for(const p of [inner[i],inner[j],outer[j],outer[i]])positions.push(p[0],0,p[1]);
    indices.push(base,base+2,base+1,base,base+3,base+2);
  }
  grid.dispose();
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions,3));geometry.setIndex(indices);
  return geometry;
}



export class OceanSurface {
  constructor({data,texture,graphics,weather=null,surfaceHost}) {
    this.host=surfaceHost;
    this.graphics=graphics;this.height=data.height;this.time=0;this.look=data.look||{};this.stormy=storms(weather);
    const look=data.look;
    const cells=new Uint8Array(data.grid*data.grid);
    for(const [x,z] of data.cells)cells[z*data.grid+x]=255;
    this.mask=new THREE.DataTexture(cells,data.grid,data.grid,THREE.RedFormat);this.mask.needsUpdate=true;
    this.mask.minFilter=this.mask.magFilter=THREE.NearestFilter;
    const wind=new THREE.Vector2(...(look.WindDirection || [1,0])).normalize();if(!wind.lengthSq())wind.set(1,0);
    this.material=ownedEffectMaterial('effects/ocean-classic', {side:THREE.DoubleSide,
      uniforms:{waterMap:{value:texture(look.MainTexture?.[0])},foamMap:{value:texture(look.FoamTexture?.[0]) || texture(look.MainTexture?.[0])},mask:{value:this.mask},
        tile:{value:new THREE.Vector2(...(look.Tile || [1,1]))},foamTile:{value:new THREE.Vector2(...(look.FoamTile || [1,1]))},
        velocity:{value:new THREE.Vector2(...(look.Velocity || [0,0]))},wind:{value:wind},time:{value:0},
        amplitude:{value:Math.min(2,Math.max(.15,(look.WindSpeed?.[0] || 0)*.05))},
        cell:{value:data.cell},gridWidth:{value:data.cell*data.grid},gridOffset:{value:new THREE.Vector2(...data.offset)},
        fogRange:lighting.fogRange,fogColor:lighting.fogColor,sunColor:lighting.sunColor,sunDirection:lighting.sunDirection}});
    this.classicMaterial=this.material;
    this.geometry=oceanGeometry();this.classicGeometry=this.geometry;
    this.near=new THREE.Mesh(this.geometry,this.material);this.far=new THREE.Mesh(this.geometry,this.material);
    for(const mesh of [this.near,this.far]){mesh.name='native-ocean';mesh.frustumCulled=false;mesh.position.y=this.height;}
    graphics.scene.add(this.near);graphics.farScene.add(this.far);
    this.applyMode();
    this.unsubscribe=graphics.settings?.onChange(key=>{if(key==='oceanEffects')this.applyMode();});
  }
  applyMode() {
    this.enhanced=this.graphics.settings?.get('oceanEffects')==='enhanced';
    if(this.enhanced&&!this.waves){
      this.waves=new EnhancedOcean(this.classicMaterial.uniforms,this.graphics,{storm:this.stormy,look:this.look,surfaceHost:this.host});
      this.near.add(this.waves.spray);if(this.waves.haze)this.far.add(this.waves.haze);
    }
    this.geometry=this.enhanced?this.waves.geometry:this.classicGeometry;
    this.material=this.enhanced?this.waves.material:this.classicMaterial;
    for(const mesh of [this.near,this.far]){mesh.geometry=this.geometry;mesh.material=this.material;}
    this.waves?.setEnabled(this.enhanced);
  }
  // OceanSurface.update's clock and grid snap come from the surface phase.
  // The sky probe and storm draws retain their renderer-side resource hooks.
  publishFrame(result,camera) {
    this.time=result[RESULT.OCEAN_TIME];this.material.uniforms.time.value=this.time;
    this.near.position.set(result[RESULT.OCEAN_X],this.height,result[RESULT.OCEAN_Z]);
    this.far.position.copy(this.near.position);
    if(this.enhanced)this.waves.update(camera,this.height,result);
  }
  dispose(){this.near.removeFromParent();this.far.removeFromParent();this.unsubscribe?.();this.waves?.dispose();this.classicGeometry.dispose();this.classicMaterial.dispose();this.mask.dispose();}
}
