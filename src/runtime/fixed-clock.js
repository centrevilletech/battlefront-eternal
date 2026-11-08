// Browser clock I/O. Rust retains compensated elapsed time, tick order,
// budget decisions and catch-up debt; callbacks only execute requested ticks.
import {simImports} from '../sim/module.js';
import {SIM_WASM} from '../sim-wasm.js';
import {ClientTimingLayout as L} from '../sim/gen/client-timing.js';
// Timing is needed before asynchronous battle admission, including workers.
// Compile this private client owner once; no frame creates another instance.
function loadClientTiming(){
  const bytes=Uint8Array.fromBase64?Uint8Array.fromBase64(SIM_WASM):Uint8Array.from(atob(SIM_WASM),c=>c.charCodeAt(0));
  return new WebAssembly.Instance(new WebAssembly.Module(bytes),simImports({bridge:null})).exports;
}
export const clientExports=loadClientTiming();
const finalizers = new FinalizationRegistry(handle => clientExports.client_timing_drop(handle));
const now = () => globalThis.performance ? performance.now() : Date.now();

// A fixed generated record is both the command input and publication view.
// Memory growth invalidates views, so refresh only at native call boundaries.
export class NativeTiming {
  constructor(source, options, fields = []) {
    Object.defineProperty(source,'nativeTiming',{value:this});
    this.handle=clientExports.client_timing_create();this.pointer=clientExports.client_timing_words(this.handle);this.refresh();
    for(const [name,value] of Object.entries(options))this.words[L[name]]=value;
    clientExports.client_timing_setup(this.handle);this.refresh();
    this.offsetsPointer=clientExports.client_timing_offsets(this.handle);this.gapsPointer=clientExports.client_timing_gaps(this.handle);
    if(options.window!==undefined)Object.defineProperty(source,'offsets',{get:()=>this.ring(false)});
    if(options.gapWindow!==undefined)Object.defineProperty(source,'gaps',{get:()=>this.ring(true)});
    for(const name of new Set([...Object.keys(options),...fields]))Object.defineProperty(source,name==='recovery'?'recover':name,{configurable:true,enumerable:true,
      get:()=>{this.refresh();const n=this.words[L[name]];return name==='capped'||name==='started'||name==='recover'||name==='extrapolate'?!!n:(name==='offset'||name==='arrivedAt'||name==='dropped')&&Number.isNaN(n)?null:n;},
      set:value=>{this.refresh();this.words[L[name]]=value===null?NaN:value;}});
    finalizers.register(this,this.handle);
  }
  refresh(){if(!this.words||this.words.length===0)this.words=new Float64Array(clientExports.memory.buffer,this.pointer,L.words);}
  ring(gaps){this.refresh();const key=gaps?'gapWords':'offsetWords';if(!this[key]||this[key].length===0)this[key]=new Float64Array(clientExports.memory.buffer,gaps?this.gapsPointer:this.offsetsPointer,this.words[gaps?L.gapWindow:L.window]);return this[key];}
  run(op,a=0,b=0,c=0,d=0,e=0){const result=clientExports.client_timing_run(this.handle,op,a,b,c,d,e);this.refresh();return result;}
}
export class FixedClock {
  constructor({hz=60,maxSteps=8,budget=Infinity,maxDebt=Infinity}={}){
    new NativeTiming(this,{hz,maxSteps,budget,maxDebt},['dt','tick','elapsed','compensation','debt','alpha','overloads','dropped','stepCost']);
    if(!this.nativeTiming.run(21))throw Error('invalid fixed clock');
    this.reset();
  }
  reset(){this.nativeTiming.run(19);}
  advance(seconds,step,limited=true){
    const owner=this.nativeTiming,x=clientExports;
    // Wall-clock reads stay in their original callback order. Unlimited
    // replay does not sample the host clock or apply the live debt limits.
    const timed=limited&&this.budget!==Infinity;
    const count=x.client_clock_begin(owner.handle,typeof seconds==='number'?seconds:NaN,+!!limited);
    owner.refresh();if(count===-2)return 0;if(count<0)throw Error('invalid elapsed time');
    let finished=false;
    try{
      x.client_clock_start(owner.handle,timed?now():0);
      if(count>0){let more;do{const start=timed?now():0;step(this.dt,owner.run(23));x.client_clock_complete(owner.handle);owner.refresh();more=x.client_clock_stepped(owner.handle,start,timed?now():0);owner.refresh();}while(more);}
      const ran=x.client_clock_finish(owner.handle,+!!limited);finished=true;owner.refresh();return ran;
    }finally{if(!finished)x.client_clock_abort(owner.handle);}
  }

  rebase(){this.nativeTiming.run(20);}
  get time(){return this.nativeTiming.run(22);}
}
