// Native boarding creates an identity before a lease can name it. Finish the
// retained SeatUnit reader only after F2 has decoded that lease. This facade
// supplies drawing/packet getters and never decides, navigates or latches.
import { SeatUnit, blankSeatIntent } from './seats.js';
import { MirrorPool } from './sim/runtime.js';
import { KIND_SEAT_UNIT } from './sim/gen/index.js';
import { AiControlViewMirror, AiControlViewLayout, syncAiControlView } from './sim/gen/ai-phase.js';
export class ControlFacades extends MirrorPool {
  constructor() {
    super(KIND_SEAT_UNIT);this.Mirror=SeatUnit;this.lives=[];
    this.view=new AiControlViewMirror();this.version=-1;
  }
  born(event) {
    const prior=this.objects[event.slot];
    if(prior?.simGen===event.generation)return prior;
    // A reused native slot follows the prior control's completed unbind.
    // Old journal/beam references retain that object's original lease.
    if(prior)prior.controller=null;
    const control=Object.create(SeatUnit.prototype);
    control.simSlot=event.slot;control.simGen=event.generation;control.native=true;
    control.nativeInitialized=false;control.intent=blankSeatIntent();
    this.objects[event.slot]=control;this.lives.push(control);return control;
  }
  at(slot,generation) {
    const current=this.objects[slot];if(current?.simGen===generation)return current;
    for(let index=0;index<this.lives.length;index++) {
      const old=this.lives[index];if(old.simSlot===slot && old.simGen===generation)return old;
    }
    throw Error('AI control reference precedes its native birth');
  }
  sync(bridge,layout) {
    if(this.version===layout.controlVersion)return;
    this.version=layout.controlVersion;
    let at=layout.controls/8;
    for(let index=0;index<layout.controlCount;index++,at+=AiControlViewLayout.words) {
      const row=syncAiControlView(bridge,bridge.f,at,0,this.view),control=row.control;
      if(!control.nativeInitialized) {control.initialize(row.lease);control.nativeInitialized=true;}
      control.controller=row.controller;

    }
    if(layout.controlAck)bridge.f[layout.controlAck/8]=layout.controlVersion;
  }
}
