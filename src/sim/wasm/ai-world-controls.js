// Native boarding creates an identity before a lease can name it. Finish the
// retained SeatUnit reader only after F2 has decoded that lease. This facade
// supplies drawing/packet getters and never decides, navigates or latches.
import { SeatUnit, blankSeatIntent } from '../../seats.js';
import { NativeFacades } from './ai-world-identities.js';
import { KIND_SEAT_UNIT } from '../gen/index.js';
import { AiControlViewMirror, AiControlViewLayout, syncAiControlView } from '../gen/ai-phase.js';
export class ControlFacades extends NativeFacades {
  constructor() {
    super(KIND_SEAT_UNIT);this.Mirror=SeatUnit;
    this.view=new AiControlViewMirror();this.version=-1;
  }
  born(event) {
    const known=this.find(event.slot,event.generation);if(known)return known;
    if(!this.accepts(event.slot,event.generation))return null;
    const prior=this.objects[event.slot];
    // A reused native slot follows the prior control's completed unbind.
    // Old journal/beam references retain that object's original lease.
    if(prior)prior.controller=null;
    const control=Object.create(SeatUnit.prototype);
    control.simSlot=event.slot;control.simGen=event.generation;control.native=true;
    control.nativeInitialized=false;control.intent=blankSeatIntent();
    this.replace(event.slot,control);return control;
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
