export class FireCommands {
  constructor() { this.received=0;this.queue=[];this.buttons=[0,0];this.pressTicks=[0,0];this.generation=null;this.outcomes=null; }
  enableOutcomes(){this.outcomes ||= [];return this;}
  drainOutcomes(){return this.outcomes?.splice(0)||[];}
  outcome(id,status){if(this.outcomes){if(this.outcomes.length>=64)throw Error('fire outcome backlog exceeded');this.outcomes.push({id,status});}}
  push(edge,tick,generation) {
    if(!edge || edge.epoch!==1 || !Number.isSafeInteger(edge.id) || edge.id<=this.received ||
      !Number.isSafeInteger(edge.tick) || edge.tick<1 || edge.tick>tick+120 ||
      !Number.isSafeInteger(edge.generation) || edge.generation!==generation ||
      ![0,1].includes(edge.button) || typeof edge.pressed!=='boolean')return false;
    // Late releases are safe and prevent a stuck trigger. Old presses cannot fire.
    if(edge.pressed && edge.tick<tick-30)return false;
    for(const key of ['aimYaw','aimPitch'])if(edge[key]!==undefined && (!Number.isFinite(edge[key]) || Math.abs(edge[key])>1000000))return false;
    if(edge.viewTick!==undefined && (!Number.isFinite(edge.viewTick)||edge.viewTick<0||edge.viewTick>tick+2))return false;
    if(edge.externalId!==undefined&&(!Number.isSafeInteger(edge.externalId)||edge.externalId<1||edge.externalId>0xffffffff))return false;
    if(this.queue.length>=32)return false;
    this.received=edge.id;
    this.queue.push({id:edge.id,...(edge.externalId===undefined?{}:{externalId:edge.externalId}),tick:edge.tick,generation,button:edge.button,pressed:edge.pressed,
      ...(edge.aimYaw===undefined?{}:{aimYaw:Math.atan2(Math.sin(edge.aimYaw),Math.cos(edge.aimYaw))}),
      ...(edge.aimPitch===undefined?{}:{aimPitch:Math.max(-Math.PI/2,Math.min(Math.PI/2,edge.aimPitch))}),
      ...(edge.viewTick===undefined?{}:{viewTick:edge.viewTick})});
    return true;
  }
  take(tick,generation,state=null) {
    if(this.generation!==generation){this.buttons=[0,0];this.pressTicks=[0,0];this.generation=generation;this.queue=this.queue.filter(edge=>{if(edge.generation===generation)return true;this.outcome(edge.id,'life-cancelled');return false;});}
    // A reliable release can stall behind retransmissions while newer movement
    // keeps arriving. A consumed state at or after the press may release it.
    // Recover before taking new edges: even a delayed short click gets one tick.
    for(let button=0;button<2;button++)if(this.buttons[button] && state?.tick>=this.pressTicks[button] && !state.buttons[button])this.buttons[button]=0;
    const used=new Set(),shots=[];
    for(let i=0;i<this.queue.length;) {
      const edge=this.queue[i];
      if(edge.generation!==generation || edge.pressed && edge.tick<tick-30){this.outcome(edge.id,edge.generation!==generation?'life-cancelled':'expired');this.queue.splice(i,1);continue;}
      if(used.has(edge.button)){i++;continue;}
      if(edge.tick>tick){used.add(edge.button);i++;continue;}
      this.outcome(edge.id,'consumed');this.queue.splice(i,1);used.add(edge.button);
      if(edge.pressed && !this.buttons[edge.button])shots.push(edge.externalId===undefined?edge:{...edge,id:edge.externalId});
      if(edge.pressed)this.pressTicks[edge.button]=edge.tick;
      this.buttons[edge.button]=edge.pressed?1:0;
    }
    return {buttons:this.buttons,shots};
  }
  clear(){for(const edge of this.queue)this.outcome(edge.id,'cleared');this.queue.length=0;this.buttons=[0,0];this.pressTicks=[0,0];}
}
