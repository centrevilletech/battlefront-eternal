// Vehicle feet are numeric native state; the page binds authored nodes and
// applies emitted stomp resources. Registration is cold and keeps fixed views.
import * as THREE from 'three';
import {classSetting} from '../vehicle-motion.js';
import {presentationModule} from './module.js';
import {FootFrame as F,FootInput as I,FootEvent as O} from './layout.js';

const xyz=p=>[p.x,p.y,p.z],xyzw=q=>[q.x,q.y,q.z,q.w];
// Bind transforms and clip tracks are immutable class resources. Keeping
// these cold imports apart from frame facts avoids resampling a JS rig.
export function footContent(v,feet){
  const bones=v.bones||[],cycle=Boolean(feet.length&&v.pose&&v.walkLength>0&&bones.length);
  const rules=v.def?.fx?.stomp||{};
  return {root:v.pose?.root??-1,length:cycle?v.walkLength:0,
    bones:bones.map((b,k)=>({parent:bones.indexOf(b.parent),scale:xyz(b.scale),
      quaternion:xyzw(v.bindQuaternions?.[k]||b.quaternion),position:xyz(v.bindPositions?.[k]||b.position)})),
    clips:cycle?['walk_leftfoot_rightfoot','walk_rightfoot_leftfoot'].map(name=>{
      const c=v.clips.get(name);return {frames:c.frames,fps:c.fps,rootMotion:Boolean(c.rootMotion),tracks:Array.from(c.tracks),data:Array.from(c.data)};
    }):[],
    feet:feet.map(foot=>({bone:bones.indexOf(v.byName?.[foot.bone]),point:foot.bottom||foot.at,turn:foot.turn||null})),
    detection:Boolean(Number.isFinite(rules.detection)?rules.detection:0),
    threshold:Number.isFinite(rules.threshold)?rules.threshold:.15,
    hydraulic:Number.isFinite(rules.hydraulic)?rules.hydraulic:1,
    centred:!feet.some(foot=>foot.bottom),rate:parseFloat(classSetting(v.def,'AnimationSpeed'))||1};
}
// Audio imports these immutable asset samples once. Live crossing policy is
// the same native detector, with independent audio and visual lifetimes.
export function cycleHeights(v,feet){return JSON.parse(presentationModule().module.PageFeet.heights(JSON.stringify(footContent(v,feet))));}
const UP=new THREE.Vector3(0,1,0);
export class VisualFeet {
  constructor(){this.loaded=presentationModule();this.kernel=new this.loaded.module.PageFeet();this.rows=new WeakMap();this.buffer=null;
    this.position=new THREE.Vector3();this.turn=new THREE.Quaternion();this.refresh();}
  // Resource birth may replace both arrays without growing linear memory.
  // The forced refresh therefore follows registration as well as memory growth.
  refresh(force=false){const b=this.loaded.memory.buffer;if(b===this.buffer&&!force)return;this.buffer=b;
    this.input=new Float64Array(b,this.kernel.input_ptr(),this.kernel.input_len());this.output=new Float64Array(b,this.kernel.output_ptr(),this.kernel.output_len());}
  register(v){let row=this.rows.get(v);if(row)return row;const feet=v.def?.fx?.feet||[];
    row={start:this.kernel.add(JSON.stringify(footContent(v,feet))),nodes:feet.slice(0,64).map(foot=>v.byName?.[foot.bone]||v.root)};
    this.rows.set(v,row);this.refresh(true);return row;}
  // Absent feet retain detector history but cannot emit another resource.
  // Clear presence for the whole phase before writing living hulls.
  begin(dt){this.refresh();this.input.fill(0);this.input[F.DT]=dt;}
  write(v){if(!v.alive||!v.def?.fx?.stomp?.effect)return;
    const row=this.register(v);this.refresh();const x=this.input;
    for(let k=0;k<row.nodes.length;k++){const at=F.STRIDE+(row.start+k)*I.STRIDE;
      x[at+I.PRESENT]=1;x[at+I.GENERATION]=v.aiTarget?.generation??0;
      x[at+I.PHASE]=v.shownWalkPhase??v.walkPhase??0;x[at+I.BLEND]=v.shownWalkBlend??v.walkBlend??0;
      x[at+I.ROOT_Y]=v.root.matrixWorld.elements[13];const m=row.nodes[k].matrixWorld.elements;
      for(let n=0;n<16;n++)x[at+I.MATRIX+n]=m[n];
    }}
  // A single call advances every hull and placed-object detector together.
  run(){this.kernel.frame();this.refresh();}
  // Only the resource owner plays effects; points, turns and multiplicities
  // are final native output and are not recomputed from drawn Three nodes.
  apply(v,effects){const row=this.rows.get(v);if(!row||!v.alive)return;const effect=v.def.fx.stomp.effect;
    for(let k=0;k<row.nodes.length;k++){const at=(row.start+k)*O.STRIDE,count=this.output[at+O.COUNT];if(!count)continue;
      this.position.fromArray(this.output,at+O.POSITION);const turn=this.output[at+O.HAS_TURN]?this.turn.fromArray(this.output,at+O.TURN):null;
      for(let n=0;n<count;n++)effects.play(effect,this.position,UP,null,turn);
    }}
}
