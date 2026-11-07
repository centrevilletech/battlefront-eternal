// The host writes each owner's retained input views, calls the enclosing
// Rust coordinator once, then consumes its completed publication. These
// writers carry foreign facts only; Rust selects every simulation phase.
import { WorldTickInputMirror,
  WorldTickOutputMirror,WorldTickOutputLayout,syncWorldTickOutput } from '../sim/gen/world.js';
const CLOCK_FIELDS=['tick','elapsed','compensation','debt','alpha','overloads','dropped','stepCost'];
const OUTPUT={words:WorldTickOutputLayout.words,Mirror:WorldTickOutputMirror,sync:syncWorldTickOutput};
export class WorldTick {
  constructor(phase,clock){
    this.clock=clock;this.phase=phase;this.input=new WorldTickInputMirror();this.output=new WorldTickOutputMirror();
    this.operation=phase.bridge.world(phase,clock);
  }
  // at: the host's wall time for the tick (ms); mouseScale: the world's
  // setting when a player's command names none.
  run(dt,tick,server,human,ai,live,camera,offlineCamera,squads,at=0,mouseScale=1){
    const p=this.phase,b=p.bridge,row=this.input;
    row.dt=dt;row.tick=tick;row.clock=this.clock;row.ai=ai;row.live=live;
    row.camera=camera;row.offlineCamera=offlineCamera;row.squads=squads;row.at=at;row.mouseScale=mouseScale;
    const water=p.water;
    b.decode(OUTPUT,this.operation.step(row,server,human),this.output);
    if(!this.output.accepted)throw Error('native world tick rejected invalid state or input');
    if(water){water.simulationTime=this.output.waterTime;water.physicsWave=this.output.wave;}
  }
  // Clock completion follows Game.step's presentation observer slots, just
  // as FixedClock.advance incremented tick after its callback returned.
  complete(){
    const source=this.output.clock,target=this.clock;
    for(let index=0;index<CLOCK_FIELDS.length;index++){const key=CLOCK_FIELDS[index];target[key]=source[key];}
  }
  finish(){
    this.operation.finish(this.output);
  }
}
