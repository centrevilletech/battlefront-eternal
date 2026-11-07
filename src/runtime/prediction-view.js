// CorrectionSmoother is now a command/publication view. Numerical offsets,
// snap decisions, exponential decay and statistics belong to the native
// network presentation frame; these methods only queue event facts.
import {NetSmoothInput as I,NetSmoothCommand as C,NetSmoothOutput as O,NetSmoothKind as K,NetLimits as L} from './network-layout.js';
export class CorrectionSmoother{
  // Field representations are fixed before native floating-point publication.
  constructor({snap=1,tau=.03}={}){
    this.snap=snap;this.tau=tau;this.x=this.y=this.z=.1;this.x=this.y=this.z=0;
    this.length=this.count=this.snaps=this.worst=this.lastSize=.1;
    this.length=this.count=this.snaps=this.worst=this.lastSize=0;
    this.commands=new Float64Array(L.SMOOTH_COMMANDS*C.STRIDE);this.commandCount=0;this.start=0;this.nativeOwner=null;
  }
  // A long paused producer discards its oldest replaceable visual requests.
  // The ring is fixed; no overflow can stop a drawn frame.
  reserve(kind){
    if(this.commandCount===L.SMOOTH_COMMANDS){this.start=(this.start+1)%L.SMOOTH_COMMANDS;this.commandCount--;}
    const at=((this.start+this.commandCount++)%L.SMOOTH_COMMANDS)*C.STRIDE;this.commands[at+C.KIND]=kind;return at;
  }
  reset(){this.reserve(K.RESET);this.x=this.y=this.z=this.length=0;}
  note(ox,oy,oz,nx,ny,nz){const at=this.reserve(K.NOTE),x=this.commands;
    x[at+C.OLD]=ox;x[at+C.OLD+1]=oy;x[at+C.OLD+2]=oz;x[at+C.NEW]=nx;x[at+C.NEW+1]=ny;x[at+C.NEW+2]=nz;
  }
  decay(dt){this.commands[this.reserve(K.DECAY)+C.VALUE]=dt;}
  // A newly admitted owner receives the prior public metrics once. Later
  // frames transfer only ordered commands, preserving event/reset ordering.
  prepare(owner,index){
    owner.refresh();const x=owner.smoothInput,at=index*I.STRIDE;
    x[at+I.SEED]=this.nativeOwner===owner?0:1;
    if(this.nativeOwner!==owner){this.nativeOwner=owner;x[at+I.X]=this.x;x[at+I.Y]=this.y;x[at+I.Z]=this.z;x[at+I.COUNT]=this.count;x[at+I.SNAPS]=this.snaps;x[at+I.WORST]=this.worst;}
    x[at+I.SNAP]=this.snap;x[at+I.TAU]=this.tau;x[at+I.COMMANDS]=this.commandCount;
    const output=owner.smoothCommands,base=index*L.SMOOTH_COMMANDS*C.STRIDE;
    for(let n=0;n<this.commandCount;n++){const from=((this.start+n)%L.SMOOTH_COMMANDS)*C.STRIDE,to=base+n*C.STRIDE;
      for(let k=0;k<C.STRIDE;k++)output[to+k]=this.commands[from+k];}
  }
  publish(owner,index){const x=owner.smoothOutput,at=index*O.STRIDE;
    this.x=x[at+O.X];this.y=x[at+O.Y];this.z=x[at+O.Z];this.length=x[at+O.LENGTH];this.count=x[at+O.COUNT];this.snaps=x[at+O.SNAPS];this.worst=x[at+O.WORST];this.lastSize=x[at+O.LAST_SIZE];this.commandCount=0;this.start=0;
  }
}

// Dedicated scalar views keep double fields from sharing generic object maps
// with unrelated resource records that also carry integer or absent values.
// Each frame copies floats into these fields without allocating HeapNumbers.
export class OwnPreview{
  constructor(){
    this.yaw=this.pitch=this.hudYaw=this.renderedYaw=this.renderedPitch=.1;
    this.yaw=this.pitch=this.hudYaw=this.renderedYaw=this.renderedPitch=0;
    this.ready=false;
  }
}
