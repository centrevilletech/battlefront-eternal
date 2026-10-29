import { NetworkInputs } from './network-inputs.js';
import { EntityEdgeSender } from './network-entity-edges.js';
import { encodeEntityInput } from './network-entity-input.js';
import { FireEdges } from './fire-commands.js';

const MAX_OUTCOMES=128;

// Movement receipts release retransmission storage. Action outcomes are kept
// separately until an applied own correction includes their simulation tick.
export class EntityInputSender {
  constructor({registry,send,getAim=()=>({}),onFire=()=>{},snapshotAck=()=>null,serverTick=()=>null,stateIntervalTicks=1}) {
    if(![1,2].includes(stateIntervalTicks))throw Error('Invalid input state interval');
    Object.assign(this,{registry,sendPacket:send,getAim,onFire,snapshotAck,serverTick,stateIntervalTicks});
    this.edges=new EntityEdgeSender();this.outcomes=new Map();this.resultAcks=[];this.id=0;this.droppedOutcomes=0;
    this.input=new NetworkInputs({state:m=>this.publish(m.commands),events:m=>this.actions(m),fire:c=>this.fire(c)});
    this.life=null;this.trigger=new FireEdges();this.appliedTick=-1;this.lastSentState=null;
  }
  get history(){return this.input.history;}
  ack(sequence){this.input.ack(sequence);}
  clearPending(){this.input.clearPending();this.trigger.reset();this.lastSentState=null;}
  send(commands){
    let sent=false;
    for(let at=0;at<commands.length;){
      const command=commands[at],life=`${command.epoch}:${command.generation}`;
      if(life!==this.life){this.life=life;this.edges.reset(command.epoch,command.generation);this.outcomes.clear();this.resultAcks=[];this.trigger.reset();this.appliedTick=-1;this.lastSentState=null;}
      let end=at+1;while(end<commands.length&&`${commands[end].epoch}:${commands[end].generation}`===life)end++;
      // Process every edge/look sample, but offer only the newest held state
      // and its bounded transition backup once per simulation frame batch.
      sent=this.input.send(commands.slice(at,end))||sent;at=end;
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
  fire(command){
    const aim=()=>{const value=this.getAim(command)||{};return {aimYaw:value.aimYaw??0,aimPitch:value.aimPitch??0};};
    return this.trigger.each(command,(button,pressed)=>{
      const {aimYaw,aimPitch}=aim(),name=button?'fireSecondary':'firePrimary';
      const edge=this.add(command,{kind:'fire',button:name,pressed,viewTick:command.viewTick??command.tick,aimYaw,aimPitch});
      if(pressed)this.onFire({...edge,button},command);
    },button=>{
      // A trigger still held sends no edge (the server keeps firing on its own
      // cadence); the cosmetic presentation gets the chance to show those
      // rounds too. Nothing is added to the reliable edge lane.
      const {aimYaw,aimPitch}=aim();
      this.onFire({kind:'fire',id:0,held:true,button,pressed:true,tick:command.tick,epoch:command.epoch,generation:command.generation,viewTick:command.viewTick??command.tick,aimYaw,aimPitch},command);
    });
  }
  publish(commands){
    const latest=commands.at(-1),edges=this.edges.batch(Math.min(latest.tick,this.serverTick()??latest.tick));
    // Every input datagram carries the newest complete snapshot ack; a lost
    // input simply means the next snapshot is encoded against an older one.
    const snapshotAck=this.snapshotAck();
    for(const report of this.edges.takeReports()){const item=this.outcomes.get(report.id);if(item&&!item.result)item.result={...report,tick:item.edge.tick+this.edges.maxAge+1};}
    // Process/predict every command, but online unchanged held state needs one
    // datagram every two simulation ticks. Starts, stops and all unreceipted
    // edges bypass this coalescing; cumulative look retains skipped samples.
    const held=JSON.stringify([...latest.down].sort()),previous=this.lastSentState;
    if(!edges.length&&previous&&held===previous.held&&latest.tick>=previous.tick&&latest.tick-previous.tick<this.stateIntervalTicks)return false;
    // Every accepted edge is repeated until its explicit receipt. A packet may
    // omit later edges to stay below the fixed datagram bound; none are retired.
    let bytes,chosen=edges;
    for(;;){
      try{bytes=encodeEntityInput({epoch:latest.epoch,generation:latest.generation,commands,edges:chosen,snapshotAck},this.registry);break;}
      catch(error){if(!String(error.message).includes('exceeds bounds'))throw error;if(commands.length>1){commands=[latest];continue;}if(!chosen.length)throw error;chosen=chosen.slice(0,-1);}
    }
    const sent=this.sendPacket(bytes);
    if(sent)this.lastSentState={held,tick:latest.tick};
    return sent;
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
  replayActions(shield=false,mounted=false){
    const actions=new Map(), shieldCommands=[];
    for(const {sequence,edge} of this.outcomes.values()) {
      const shieldPress=(mounted || shield&&edge.button==='fireSecondary')&&edge.kind==='fire'&&edge.pressed;
      if(edge.kind!=='action'&&!shieldPress)continue;
      let list=actions.get(sequence);if(!list)actions.set(sequence,list=[]);
      list.push(shieldPress?edge.button:edge.action);
      if(shieldPress)shieldCommands.push({sequence,epoch:edge.epoch,generation:edge.generation,tick:edge.tick,down:[],look:[],actions:list});
    }
    return {actions,shieldCommands};
  }
  pendingHuman(sequence){return [...this.outcomes.values()].some(item=>item.sequence===sequence&&['spawn','team','suicide','cancel'].includes(item.edge.kind));}
}
