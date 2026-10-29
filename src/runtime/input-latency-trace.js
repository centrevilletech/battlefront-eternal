// Opt-in diagnostics only. All wall durations use this client's monotonic
// clock; server execution is compared in simulation ticks, never clock offsets.
// Render submission is not GPU completion or physical click-to-photon latency.
export class InputLatencyTrace {
  constructor({now=()=>performance.now(),timeOrigin=performance.timeOrigin,capacity=512}={}) {
    if(!Number.isSafeInteger(capacity)||capacity<1)throw Error('Invalid trace capacity');
    Object.assign(this,{now,timeOrigin,capacity});
    this.commands=new Map();this.shots=[];this.metrics=new Map();this.pending=null;this.life=null;
  }
  record(name,value) {
    if(!Number.isFinite(value)||value<0)return;
    let metric=this.metrics.get(name);if(!metric)this.metrics.set(name,metric={values:[],total:0});
    metric.total++;metric.values.push(value);if(metric.values.length>this.capacity)metric.values.shift();
  }
  noteInput(event) {
    const now=this.now();let at=Number(event?.timeStamp);
    if(at>1e12)at-=this.timeOrigin;
    if(!Number.isFinite(at)||at<0||at>now)at=now;
    this.record('eventDispatchMs',now-at);
    this.pending=this.pending===null?at:Math.min(this.pending,at);
  }
  command(command) {
    const life=`${command.epoch}:${command.generation}`;
    if(life!==this.life){this.commands.clear();this.shots=[];this.life=life;}
    if(this.commands.has(command.sequence))return;
    const sampled=this.now(),input=this.pending;this.pending=null;
    this.commands.set(command.sequence,{tick:command.tick,sampled,input});
    if(input!==null)this.record('inputToSampleMs',sampled-input);
    if(this.commands.size>this.capacity)this.commands.delete(this.commands.keys().next().value);
  }
  predicted(edge,command,metadata) {
    if(`${edge.epoch}:${edge.generation}`!==this.life)return;
    const sample=this.commands.get(command?.sequence),at=this.now();
    if(!sample)return;
    this.record('sampleToPredictionMs',at-sample.sampled);
    if(sample.input!==null)this.record('inputToPredictionMs',at-sample.input);
    this.shots.push({...metadata,at,sampled:sample.sampled,input:sample.input,tick:command.tick,submitted:false,confirmed:false});
    if(this.shots.length>this.capacity)this.shots.shift();
  }
  submit(drawn) {
    if(!drawn)return;
    const now=this.now();
    for(const shot of this.shots)if(!shot.submitted){
      shot.submitted=true;this.record('predictionToSubmitMs',now-shot.at);
      this.record('sampleToSubmitMs',now-shot.sampled);
      if(shot.input!==null)this.record('inputToSubmitMs',now-shot.input);
    }
  }
  ack(ack) {
    if(`${ack.epoch}:${ack.generation}`!==this.life)return;
    const sample=this.commands.get(ack.ack);
    if(sample&&!sample.acked){
      sample.acked=true;this.record('sampleToAckMs',this.now()-sample.sampled);
      this.record('executionLateTicks',Math.max(0,ack.ackTick-sample.tick));
    }
  }
  confirmed(metadata,{tick}={}) {
    const shot=this.shots.find(s=>!s.confirmed&&s.playerId===metadata.playerId&&s.generation===metadata.generation&&s.fireId===metadata.fireId&&s.channel===(metadata.channel??0));
    if(!shot)return;
    shot.confirmed=true;this.record('predictionToConfirmationMs',this.now()-shot.at);
    if(Number.isFinite(tick))this.record('shotExecutionLateTicks',Math.max(0,tick-shot.tick));
  }
  report() {
    const metrics={};for(const [name,{values,total}]of this.metrics){
      const sorted=[...values].sort((a,b)=>a-b),p=n=>sorted[Math.max(0,Math.ceil(n*sorted.length)-1)];
      metrics[name]={total,retained:sorted.length,p50:p(.5),p95:p(.95),p99:p(.99),max:sorted.at(-1)};
    }
    return {boundary:'Client render submission, not GPU completion or photons; ACK/confirmation durations include network round trip and scheduling.',life:this.life,commands:this.commands.size,shots:this.shots.length,metrics};
  }
}
