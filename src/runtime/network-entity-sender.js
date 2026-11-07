import { pageNetworkFor } from './page-network.js';
import { NetworkInputs } from './network-inputs.js';
import { EntityEdgeSender } from './network-entity-edges.js';
import { INPUT_ACTIONS } from './network-entity-input.js';
import {createWireInputBridge} from './wire-input.js';

const MAX_OUTCOMES=128,EMPTY_AIM=Object.freeze({aimYaw:0,aimPitch:0});

// Movement receipts release retransmission storage. Action outcomes are kept
// separately until an applied own correction includes their simulation tick.
export class EntityInputSender {
  constructor({registry,send,prepareAim=()=>{},getAim=()=>EMPTY_AIM,onFire=()=>{},snapshotAck=()=>null,serverTick=()=>null,stateIntervalTicks=1,kernel=pageNetworkFor(registry)}) {
    if(![1,2].includes(stateIntervalTicks))throw Error('Invalid input state interval');
    Object.assign(this,{registry,sendPacket:send,prepareAim,getAim,onFire,snapshotAck,serverTick,stateIntervalTicks});
    this.edges=new EntityEdgeSender();this.outcomes=new Map();this.resultAcks=[];this.id=0;this.droppedOutcomes=0;
    this.network=kernel;this.encoder=createWireInputBridge(kernel);this.policy={handle:0,interval:stateIntervalTicks};
    this.packet={epoch:NaN,generation:NaN,commands:null,edges:null,snapshotAck:null};this.single=[];
    this.fireCommand=null;this.triggerEdge=(button,pressed,held)=>held?this.fireHeld(button):this.fireEdge(button,pressed);
    this.heldPublication={kind:'fire',id:0,held:true,button:NaN,pressed:true,tick:NaN,epoch:NaN,generation:NaN,viewTick:NaN,aimYaw:NaN,aimPitch:NaN};
    this.input=new NetworkInputs({kernel:this.network,aim:count=>this.prepareAim(count),state:m=>this.publish(m.commands),events:m=>this.actions(m),fire:c=>this.fire(c)});
    this.policy.handle=this.input.handle;this.appliedTick=-1;this.edgeEpoch=NaN;this.edgeGeneration=NaN;
  }
  // The registry's packet encoder can outlive one connection's sender.
  // Release only this sender's control handle, leaving the shared owner live.
  dispose(){this.input.dispose();}
  get history(){return this.input.history;}
  ack(sequence){this.input.ack(sequence);}
  clearPending(){this.input.clearPending();}
  send(commands){
    let sent=false;
    for(let at=0;at<commands.length;){
      const command=commands[at];
      // Only reliable transport resources rotate here. Native Outbound owns
      // movement life, cumulative look, held transitions and packet cadence.
      if(command.epoch!==this.edgeEpoch||command.generation!==this.edgeGeneration){
        this.edgeEpoch=command.epoch;this.edgeGeneration=command.generation;this.edges.reset(command.epoch,command.generation);
        this.outcomes.clear();this.resultAcks.length=0;this.appliedTick=-1;
      }
      let end=at+1;while(end<commands.length&&commands[end].epoch===command.epoch&&commands[end].generation===command.generation)end++;
      sent=this.input.send(commands,at,end)||sent;at=end;
    }
    return sent;
  }
  // Outcomes wait for the server's result and an applied correction; a
  // lagging server can leave many waiting. At the cap the oldest is given up
  // (its edge still goes out until receipted; only its local replay stops)
  // rather than ending the session.
  add(command,edge){
    if(this.outcomes.size>=MAX_OUTCOMES){this.outcomes.delete(this.outcomes.keys().next().value);this.droppedOutcomes++;}
    const value={...edge,id:++this.id,tick:command.tick,epoch:command.epoch,generation:command.generation};
    this.edges.add(value);this.outcomes.set(value.id,{sequence:command.sequence??command.id,edge:value,result:null});return value;
  }
  actions(command){
    for(const action of command.actions)this.add(command,{kind:'action',action});
    for(const human of command.human||[]){
      const edge={...human};
      if(human.kind==='spawn'){
        edge.classId=this.registry.classIds.get(human.className);edge.postId=human.post===null?0:this.registry.postIds.get(human.post);
        if(!edge.classId||edge.postId===undefined)throw Error('Unknown spawn registry reference');
        delete edge.className;delete edge.post;
      }
      this.add(command,edge);
    }
    return true;
  }
  fire(command){this.fireCommand=command;this.input.eachFire(this.triggerEdge);return true;}
  fireEdge(button,pressed){
    const command=this.fireCommand,value=this.getAim(command),name=button?'fireSecondary':'firePrimary';
    const edge=this.add(command,{kind:'fire',button:name,pressed,viewTick:command.viewTick??command.tick,aimYaw:value?.aimYaw??0,aimPitch:value?.aimPitch??0});
    if(pressed)this.onFire({...edge,button},command);
  }
  fireHeld(button){
    const command=this.fireCommand,value=this.getAim(command);
    const event=this.heldPublication;event.button=button;event.tick=command.tick;event.epoch=command.epoch;event.generation=command.generation;event.viewTick=command.viewTick??command.tick;event.aimYaw=value?.aimYaw??0;event.aimPitch=value?.aimPitch??0;this.onFire(event,command);
  }
  publish(commands){
    const latest=commands.at(-1),edges=this.edges.batch(Math.min(latest.tick,this.serverTick()??latest.tick));
    // Every input datagram carries the newest complete snapshot ack; a lost
    // input simply means the next snapshot is encoded against an older one.
    const snapshotAck=this.snapshotAck();
    for(const report of this.edges.takeReports()){const item=this.outcomes.get(report.id);if(item&&!item.result)item.result={...report,tick:item.edge.tick+this.edges.maxAge+1};}
    this.policy.interval=this.stateIntervalTicks;
    const packet=this.packet;packet.epoch=latest.epoch;packet.generation=latest.generation;packet.snapshotAck=snapshotAck;
    let selected=commands;
    for(;;){
      packet.commands=selected;packet.edges=edges;
      const bytes=this.encoder.encode(packet,this.registry,INPUT_ACTIONS,true,this.policy);
      if(bytes===null){
        if(selected.length>1){this.single[0]=latest;this.single.length=1;selected=this.single;continue;}
        if(edges.length){edges.length--;continue;}
        return false;
      }
      return bytes.length>0&&this.sendPacket(bytes);
    }
  }
  feedback(message){
    if(message.epoch!==this.edges.life?.epoch||message.generation!==this.edges.life?.generation)return;
    if(!Array.isArray(message.received)||message.received.length>32||!Array.isArray(message.results)||message.results.length>32)throw Error('Invalid entity edge feedback');
    this.edges.receipt({epoch:message.epoch,generation:message.generation,ids:message.received});
    for(const result of message.results){
      if(!Number.isSafeInteger(result.id)||!Number.isSafeInteger(result.tick)||result.tick<0||!['consumed','expired','life-cancelled','cleared','rejected'].includes(result.status))throw Error('Invalid entity edge outcome');
      const pending=this.outcomes.get(result.id);if(pending)pending.result={...result};
      if(!this.resultAcks.includes(result.id))this.resultAcks.push(result.id);
    }
    this.resultAcks=this.resultAcks.slice(-32);
  }
  applied(tick){this.appliedTick=Math.max(this.appliedTick,tick);for(const[id,item]of this.outcomes)if(item.result&&item.result.tick<=this.appliedTick)this.outcomes.delete(id);}
  pendingActions(sequence){return [...this.outcomes.values()].filter(item=>item.sequence===sequence&&item.edge.kind==='action').map(item=>item.edge.action);}
  pendingHuman(sequence){return [...this.outcomes.values()].some(item=>item.sequence===sequence&&['spawn','team','suicide','cancel'].includes(item.edge.kind));}
}
