// Isolated movement/correction fixtures drive the same ordered preview phase
// as the page. Production soldiers expose no per-sample numeric adapter.
import { Soldier as Body } from '../../src/soldier.js';
import { BodyPreviewFrameLayout as F } from '../../src/sim/gen/units.js';
import {simModule,simImports} from '../../src/sim/module.js';
// Synthetic fixtures without a battle still require Rust. Compile once and
// supply their native owner explicitly instead of adding a production path.
const compiled=await simModule();let detached;
export function owner(world){
  if(world?.rays)return world.rays.x;
  if(!detached)detached=new WebAssembly.Instance(compiled,simImports({bridge:null}));
  return detached.exports;
}
import { BodyPreview, bodyNative } from '../../src/presentation/body.js';
// Driver handles are test resources. They do not change the body's own
// fields, which the replay-inventory test classifies independently.
const phases=new WeakMap();
export class Soldier extends Body {
  constructor(world,def){super(world,def,owner(world));}
  tick(input,dt){this.world.movement.tick(this,input,dt);}
  look(dx,dy,scale=1,invert=false) { this.sample(dx,dy,scale,invert,false); }
  lookRaw(dx,dy,scale=1,invert=false) { this.sample(dx,dy,scale,invert,true); }
  sample(dx,dy,scale,invert,raw) {
    let phase=phases.get(this);
    if(!phase){phase=new BodyPreview(this.world,bodyNative(this));phases.set(this,phase);}
    // Isolated method-shaped test inputs each form one complete phase.
    phase.begin(this);phase.append([[dx,dy]],1,scale,invert,raw);phase.run(this);
    if(!raw){this.lookX=phase.frame[F.lookX];this.lookY=phase.frame[F.lookY];}
  }
}
export {ENGINE} from '../../src/soldier.js';
