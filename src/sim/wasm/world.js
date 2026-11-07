// The WASM world operation assembles retained foreign inputs in place. Native
// providers use their own typed arenas and never implement these export calls.
import { BlockView } from '../block-view.js';
import { WorldTickSetupMirror,WorldTickSetupLayout,putWorldTickSetup,WorldTickInputLayout,putWorldTickInput } from '../gen/world.js';
export class WorldOperation {
  constructor(bridge,phase,clock) {
    this.bridge=bridge;this.phase=phase;
    const setup=new WorldTickSetupMirror(),water=phase.water;
    setup.water=Boolean(water);setup.waterTime=water?water.simulationTime:0;
    setup.waterHeight=water?water.height:0;setup.waterBob=Boolean(water?.bobbing);
    bridge.callArgs({words:WorldTickSetupLayout.words,put:putWorldTickSetup},setup);
    this.input = new BlockView();this.address=phase.x.world_step_init();
    this.run=phase.x.world_step_run;
  }
  step(row,server,human) {
    const p=this.phase,b=this.bridge,dt=row.dt,tick=row.tick;
    row.units=p.preparePhase(dt,tick);
    if(row.live)p.liveOwner.prepareStep(dt,tick);
    p.tailOwner.prepareStep(dt,tick,server,human);
    const input=b.backend.blocks.readInto(this.address,WorldTickInputLayout.words,this.input);
    putWorldTickInput(b,row,input.words,input.base,b.heap);
    if(b.boundaryCounters)b.boundaryCounters.inputWords+=WorldTickInputLayout.words;
    const previous=b.phaseCounters;b.phaseCounters=p.counters;p.active=true;
    let pointer;
    try{pointer=this.run();}finally{p.active=false;b.phaseCounters=previous;}
    return b.readBlock(pointer);
  }
  close() {
    this.input.words=null;this.input.base=this.input.length=0;this.input.generation++;
    this.address=0;this.run=null;this.phase=null;this.bridge=null;
  }
  finish(output) {
    const p=this.phase;
    p.finishPhase();
    p.factory.context.ordnance.sim.refresh(output.ordnance);
  }
}
