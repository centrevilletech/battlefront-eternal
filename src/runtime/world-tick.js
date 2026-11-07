// The host writes each owner's retained input views, calls the enclosing
// Rust coordinator once, then consumes its completed publication. These
// writers carry foreign facts only; Rust selects every simulation phase.
import { WorldTickSetupMirror,WorldTickSetupLayout,putWorldTickSetup,WorldTickInputMirror,WorldTickInputLayout,putWorldTickInput,
  WorldTickOutputMirror,WorldTickOutputLayout,syncWorldTickOutput } from '../sim/gen/world.js';
const CLOCK_FIELDS=['tick','elapsed','compensation','debt','alpha','overloads','dropped','stepCost'];
const OUTPUT={words:WorldTickOutputLayout.words,Mirror:WorldTickOutputMirror,sync:syncWorldTickOutput};
export class WorldTick {
  constructor(phase,clock){
    this.clock=clock;this.phase=phase;this.input=new WorldTickInputMirror();this.output=new WorldTickOutputMirror();
    const setup=new WorldTickSetupMirror(),water=phase.water,b=phase.bridge;
    setup.water=Boolean(water);setup.waterTime=water?water.simulationTime:0;setup.waterHeight=water?water.height:0;setup.waterBob=Boolean(water?.bobbing);
    b.callArgs({words:WorldTickSetupLayout.words,put:putWorldTickSetup},setup);
    this.address=phase.x.world_step_init()/8;
  }
  run(dt,tick,server,human,ai,live,camera,offlineCamera,squads){
    const p=this.phase,b=p.bridge,row=this.input;
    row.dt=dt;row.tick=tick;row.clock=this.clock;row.ai=ai;row.live=live;
    row.camera=camera;row.offlineCamera=offlineCamera;row.squads=squads;
    const water=p.water;
    row.units=p.preparePhase(dt,tick);
    if(live)p.liveOwner.prepareStep(dt,tick);
    p.tailOwner.prepareStep(dt,tick,server,human);
    b.views();putWorldTickInput(b,row,b.f,this.address,b.heap);
    if(b.boundaryCounters)b.boundaryCounters.inputWords+=WorldTickInputLayout.words;
    const previous=b.phaseCounters;b.phaseCounters=p.counters;p.active=true;
    let pointer;
    try{pointer=p.x.world_step_run();}finally{p.active=false;b.phaseCounters=previous;}
    b.callResult(OUTPUT,pointer,this.output);
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
    const p=this.phase;
    p.finishPhase();
    p.factory.context.ordnance.sim.refresh(this.output.ordnance);
  }
}
