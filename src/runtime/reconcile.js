// Client timing views. Arrival windows, lead recovery and input phase locking
// belong to sim::client::timing; transport supplies packet/ACK timestamps.
import {ClientTimingLayout as L} from '../sim/gen/client-timing.js';
import {NativeTiming} from './fixed-clock.js';
export class WorldTickEstimator {
  constructor({hz=60,window=60}={}){new NativeTiming(this,{hz,window},['at','count','offset','dropped']);this.reset();}
  reset(){this.nativeTiming.run(0);}
  arrive(tick,now,dropped=0){this.nativeTiming.run(1,tick,now,dropped);}
  estimate(now){const value=this.nativeTiming.run(2,now);return Number.isNaN(value)?null:value;}
}
function estimatorView(owner){
  const estimator={reset:()=>owner.run(0),arrive:(tick,now,dropped=0)=>owner.run(1,tick,now,dropped),estimate:now=>{const value=owner.run(2,now);return Number.isNaN(value)?null:value;}};
  for(const name of ['hz','window','at','count','offset','dropped'])Object.defineProperty(estimator,name,{get(){owner.refresh();const n=owner.words[L[name]];return Number.isNaN(n)?null:n;},set(value){owner.refresh();owner.words[L[name]]=value===null?NaN:value;}});
  Object.defineProperty(estimator,'offsets',{get:()=>owner.ring(false)});return estimator;
}
export class InputLead {
  constructor({hz=60,window=60,gapWindow=120,margin=1,maxLead=12,cooldown=30,recover=false,calm=recover?60:600,extrapolate=false}={}){
    new NativeTiming(this,{hz,window,gapWindow,margin,maxLead,cooldownAcks:cooldown,calmAcks:calm,recover,extrapolate},['gapAt','gapCount','arrivedAt','arrivedTick','latest','lead','late','worstGap','extra','cooldown','calm','measuredKey','capped']);this.estimator=estimatorView(this.nativeTiming);this.reset();
  }
  reset(){this.nativeTiming.run(3);}
  arrive(tick,now,dropped=0){this.nativeTiming.run(4,tick,now,dropped);}
  acknowledge(late,elapsedTicks=1,capped=false){this.nativeTiming.run(5,late,elapsedTicks,+!!capped);}
  measure(){this.nativeTiming.run(6);}
  target(now,latest=this.latest,roundTripSeconds=0){return this.nativeTiming.run(7,now,latest,roundTripSeconds);}
}
export function advanceInputClock(clock,target,dt,step,bound,options){
  return clock.advance(clock.nativeTiming.run(18,target,dt,bound===undefined?NaN:bound,options?.lock??0),step,false);
}
