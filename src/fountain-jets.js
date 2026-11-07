import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT, EFFECT_TARGET } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

// A fountain in Enhanced: the stock plume's size and timing, drawn as water
// lit by the scene. The stock jet (effects.json `waterfountain`, its water1
// layer) leaves the nozzle at 11.6-13 m/s and loses 45 m/s over a 3 s key,
// 15 m/s², so its water stands 4.5-5.6 m, and its sprites, ten times larger
// by then, show the plume to about 7.5 m. Here the water leaves at that
// speed, 12.5 m/s, under ordinary gravity, so it rises 8 m and opens into an
// umbrella that falls back into the inner tier, which fills to its lip and
// overflows into the outer pool (fountain-water.js draws the raised pool).
//
//   column  the jet itself: glassy where it leaves the water, a churning
//           white column above, its width steady; a dome of heaving foam
//           where it meets the pool
//   ropes   the umbrella: ropes of water arcing out of the crown in bundles,
//           more of them close around the jet, and fine strands, falling
//           into the inner tier; and a sparse outer ring of long arcs that
//           clear the tier and land in the outer pool. Each is a ribbon along
//           the path of a stream whose direction, reach and thickness wander
//           slowly, so it sways but holds together; it thins as it speeds
//           up, swells and necks, and breaks into drops.
//   film    the overflow, a sheet pouring over the tier's lip and running
//           down its sloping face into the outer pool
//   clump   aerated water boiling up the column and churning in the crown
//   froth   spray thrown off where the jet breaks at the crown
//   skirt   spray thrown up where the water lands
//   spray   fine spray torn from the ropes, falling with them
//   splash  where a stream strikes a pool: a crown, a spike, droplets
//   drop    drops shaken off the streams and thrown from the crown
//   mist    a faint low haze drifting downwind
//   ring    ripples spreading on the pools from the strikes
//   foam    foam riding outward from where the water lands
// Everything is placed analytically from the time on the GPU.
//
// Clear water shows the scene behind it, bent, softened and faintly tinted
// (a copy of the frame taken just before the jets draw, read through its
// mipmaps), and the sky and square at grazing angles; backlit, the sun bent
// through it brightens its sides. Aerated water is lit like cloud: sunward
// faces bright, its shade lit by the sky, shadowed by the plume between it
// and the sun, glowing through its thin parts against the sun. Spray is lit
// by the shape it hangs in, and laid on as light (compositeFragment). Glints
// go on top; highlights roll off rather than clip.
//
// The soft spray (mist, froth, skirt, spray) is drawn at half the frame's
// width and height and laid over it with depth-aware upsampling, as the
// steam volumes are: a quarter of the fragments. Twelve draws for all five
// fountains, far to near: what lies on the pools; the overflow beyond the
// jets' axes from the eye; then beyond the axes, the soft spray (two draws:
// into its target, then over the frame), the ropes, the drops, splashes and
// aerated water; the columns; before the axes the same in reverse, the
// overflow before the ropes. Each draw holds only the fountains in view.
// Nothing is allocated per frame.
// Cold placement facts also consumed by the basin's raised water level.
export const TIER={lip:4,sides:12,rise:.797,foot:4.82,outer:-.05};
export const fountainJets=new WeakMap();
const FOUNTAINS=5;
const ROW=EFFECT.JET_PARTICLES;
export class FountainJets {
  constructor(scene,spray,graphics,{foam=null,tier=false}={}) {
    this.scene=scene;this.graphics=graphics;this.spray=spray;this.foam=foam;this.tier=tier;
    this.levels=new Float32Array(FOUNTAINS);this.lows=new Float32Array(FOUNTAINS);sceneProperty(this,"visible",new Float32Array(FOUNTAINS));
    this.resources=null;this.behind=null;this.atlas=-1;this.samples=new Int32Array(3);this.samples.fill(-1);
    this.batches=[];this.meshes=[];this.targets=[];
    // FountainJets used geometry.userData.perFountain to select the active
    // instance prefix. Native geometry now owns that multiplier and its seeds.
    const part=(name,kind,variant)=>{
      const batch={nativeKind:kind,family:'jets',variant,texture:null,count:0,capacity:FOUNTAINS,visibility:'anchor',arrays:{},
        values:new Float64Array(ROW.LENGTH),jetsOwner:this,prepareResources:resources=>this.prepareResources(resources)};
      const mesh=batch.mesh=new THREE.Object3D();mesh.userData.effectBatch=batch;mesh.name=name;sceneProperty(mesh,"frustumCulled",false);sceneProperty(mesh,"visible",false);
      this.batches.push(batch);return batch;
    };
    const particles=(name,group,before=false)=>part(name,EFFECT.JET_PARTICLES.ID,(group<<2)|(before?1:0)|(tier?2:0));
    const ropes=(name,before=false)=>part(name,EFFECT.JET_ROPES.ID,(before?1:0)|(tier?2:0));
    const film=(name,before=false)=>part(name,EFFECT.JET_FILM.ID,before?1:0);
    // Each soft half keeps its own offscreen target. Its composite and seed
    // batch are native resources; this object only coordinates viewport facts.
    const soft=(name,before)=>{
      const inner=particles(name+'-body',1,before);inner.family='jetsSpray';
      const composite={nativeKind:EFFECT.JET_COMPOSITE.ID,family:'jetComposite',variant:0,texture:null,count:1,capacity:1,visibility:'anchor',arrays:{},
        values:new Float64Array(EFFECT.JET_COMPOSITE.LENGTH),volumeBatch:inner,target:null,
        prepareResources:resources=>{this.prepareResources(resources);composite.target=resources.effectTarget(EFFECT_TARGET.SPRAY);this.targets.push(composite.target);return Int32Array.of(composite.target.texture);},
        prepare:(renderer,scene,camera)=>this.prepareSpray(renderer,camera,composite)};
      const mesh=composite.mesh=new THREE.Object3D();mesh.userData.effectBatch=composite;mesh.name=name;sceneProperty(mesh,"frustumCulled",false);sceneProperty(mesh,"visible",false);
      composite.owner=this;inner.composite=composite;return composite;
    };
    const rows=[particles('fountain-pool',0)];
    if(tier)rows.push(film('fountain-overflow'));
    rows.push(soft('fountain-spray-beyond',false),ropes('fountain-ropes'),particles('fountain-jets',2));
    const core=part('fountain-jets-core',EFFECT.JET_CORE.ID,0);this.core=core.mesh;rows.push(core);
    rows.push(particles('fountain-jets-near',2,true));
    if(tier)rows.push(film('fountain-overflow-near',true));
    rows.push(ropes('fountain-ropes-near',true),soft('fountain-spray-before',true));
    for(let k=0;k<rows.length;k++){const batch=rows[k],mesh=batch.mesh;sceneProperty(mesh,"renderOrder",9+k*.05);this.meshes.push(mesh);scene.add(mesh);fountainJets.set(mesh,this);}
    this.rows=rows;
  }
  // All procedural geometry, atlas bytes, shader selection, constants and
  // sampling policy are admitted in Rust. Late foam remains an asset source.
  prepareResources(resources) {
    if(!this.resources){this.resources=resources;this.behind=resources.effectTarget(EFFECT_TARGET.JET_COPY);this.atlas=resources.gpu.fountain_atlas();
      this.samples[0]=this.atlas;this.samples[1]=this.foam?resources.staticSample(this.foam):-1;this.samples[2]=this.behind.texture;}
    return this.samples;
  }
  prepare(renderer,camera) {
    const source=renderer.getRenderTarget(),enabled=source===this.graphics.sceneTarget;
    const depth=enabled&&particleDepth.capture(renderer,null,camera,true)?1:0;
    if(enabled)this.resources.resizeEffectTarget(this.behind,source.width,source.height);
    for(const batch of this.batches){const v=batch.values;v[ROW.HAS_DEPTH]=depth;v[ROW.HAS_SCENE]=enabled?1:0;
      if(enabled){v[ROW.SCENE_RESOLUTION]=source.width;v[ROW.SCENE_RESOLUTION+1]=source.height;}}
  }
  // Ports prepareSpray without target shells or named uniform dispatch.
  // Both low-resolution halves retain their own current colour independently.
  prepareSpray(renderer,camera,composite) {
    const source=renderer.getRenderTarget();
    const enabled=source===this.graphics.sceneTarget&&particleDepth.capture(renderer,null,camera,true);
    composite.values[EFFECT.JET_COMPOSITE.ENABLED]=enabled?1:0;if(!enabled)return false;
    this.resources.resizeEffectTarget(composite.target,Math.max(1,Math.ceil(source.width/2)),Math.max(1,Math.ceil(source.height/2)));
    const width=composite.target.width,height=composite.target.height,v=composite.volumeBatch.values;
    composite.values[EFFECT.JET_COMPOSITE.SPRAY_RESOLUTION]=width;composite.values[EFFECT.JET_COMPOSITE.SPRAY_RESOLUTION+1]=height;
    this.prepare(renderer,camera);v[ROW.FRAG_SCALE]=source.width/width;v[ROW.FRAG_SCALE+1]=source.height/height;v[ROW.HAS_DEPTH]=1;
    return true;
  }
  // The visible source slots are native environment outputs; no instance
  // topology, descriptor version or material property is read on this frame.
  update(time,fountains) {
    let shown=0;for(let i=0;i<fountains.length&&shown<FOUNTAINS;i++){const f=fountains[i];if(f.slot>=FOUNTAINS)continue;this.visible[shown++]=f.slot;this.levels[f.slot]=f.level;this.lows[f.slot]=f.low??f.level;}
    for(const batch of this.batches){const v=batch.values;batch.count=shown;v[ROW.JET_TIME]=time;
      for(let i=0;i<FOUNTAINS;i++)this.spray.sources[i].toArray(v,ROW.SOURCES+i*4);
      v.set(this.levels,ROW.LEVELS);v.set(this.lows,ROW.LOWS);v.set(this.visible,ROW.VISIBLE);v[ROW.FRAG_SCALE]=v[ROW.FRAG_SCALE+1]=1;
    }
    for(const row of this.rows)sceneProperty(row.mesh,"visible",shown>0);
  }
  dispose() {
    for(const mesh of this.meshes)mesh.removeFromParent();
    if(this.resources){this.resources.gpu.dispose_sample(this.atlas);this.resources.disposeEffectTarget(this.behind);for(const target of this.targets)this.resources.disposeEffectTarget(target);}
  }
}
