// The page exposes the native client's playout clock. Packets and wall time
// enter here; delay measurement, starvation and trimming stay with Rust.
import {NativeTiming} from './fixed-clock.js';
export class PlayoutClock {
  constructor({hz=60,minDelay=2,maxDelay=10,margin=1,window=120,trim=.1,snap=12}={}){
    new NativeTiming(this,{hz,minDelay,maxDelay,margin,gapWindow:window,window,trim,snap},['clock','latest','delay','rate','gapAt','gapCount','arrivedAt','arrivedTick','started','waits','measuredKey']);this.reset();
  }
  reset(){this.nativeTiming.run(8);}
  arrive(tick,now,interval=1){this.nativeTiming.run(9,tick,now,interval);}
  measure(){return this.nativeTiming.run(10);}
  advance(dt){return this.nativeTiming.run(11,dt);}
  get shownTick(){return this.nativeTiming.run(24);}
}
