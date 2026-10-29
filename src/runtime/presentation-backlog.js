// Effects have no durable gameplay state. Keep their order for active snapshot
// consumers, but do not replay an unattended world's old gunfire on first join.
export class PresentationBacklog {
  constructor({maxIdleTicks=6,maxCalls=4096}={}) {
    this.maxIdleTicks=maxIdleTicks;this.maxCalls=maxCalls;this.lastViewed=null;this.calls=[];
  }
  push(tick,calls) {
    if(this.lastViewed===null || tick-this.lastViewed>this.maxIdleTicks){this.calls.length=0;return true;}
    if(this.calls.length+calls.length>this.maxCalls){this.calls.length=0;return false;}
    this.calls.push(...calls);return true;
  }
  take(tick) {this.lastViewed=tick;const calls=this.calls;this.calls=[];return calls;}
}
