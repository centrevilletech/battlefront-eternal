// rAF and GPU measurements are host facts. Native client policy publishes
// presentation deadlines and resolution scale without a timer or spin wait.
import {NativeTiming} from './fixed-clock.js';
export class FramePacer {
  constructor(limit=0){new NativeTiming(this,{},['limit','elapsed','presented','deadline']);this.setLimit(limit);}
  setLimit(limit){this.nativeTiming.run(12,Number(limit));}
  reset(){this.nativeTiming.run(13);}
  advance(seconds){const value=this.nativeTiming.run(14,seconds);return Number.isNaN(value)?null:value;}
}
export class ResolutionController {
  constructor(target=0){new NativeTiming(this,{},['target','scale','age','time','frames','slow','cpuBound','headroom','recovery']);this.setTarget(target);}
  setTarget(target){this.nativeTiming.run(15,Number(target));}
  reset(){this.nativeTiming.run(16);}
  sample(intervalMs,cpuMs,gpuMs=null,behind=null,latency=null){
    return this.nativeTiming.run(17,intervalMs,cpuMs,typeof gpuMs==='number'?gpuMs:NaN,typeof behind==='boolean'?+behind:NaN,typeof latency==='number'?latency:NaN);
  }
}
