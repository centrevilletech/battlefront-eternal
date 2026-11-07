import {pageNetworkFor} from './page-network.js';
import {NetOutbound as O,NetAckResult as A,NetSentResult as S} from './network-layout.js';
import { INPUT_ACTIONS } from './network-entity-input.js';

// Only what the wire registry can name goes down the action lane. Held
// flight names (rollLeft, throttle, ...) travel as key codes in `down`;
// letting one through here made encodeEntityInput throw every frame.
export const discreteActions = command => (command.actions || []).filter(action => INPUT_ACTIONS.includes(action));

// Latest held state is expendable. Discrete actions and fire edges are not.
// Cumulative mouse counts recover look after a lost burst. One bounded held
// transition backup preserves its original execution tick when that packet is
// lost; actions/fire are emitted once on their independent reliable lanes.
//
// Each command passed in is marked with whether it went to the world itself
// (`sent`), the look the world will take with it (`worldLook`) and whether
// its own look has gone yet (`lookSent`): the world gets a frame's last
// command (and a held transition), takes its keys, and takes the change in
// the running look total since the last one it took as one sample on that
// command's tick (InputMailbox.take). The page's replay does the same
// (reconcile.js replayTicks); look that has not gone yet is only shown.
export class NetworkInputs {
  constructor({state,events,fire,transitionBackups=true,kernel=pageNetworkFor(null)}) {
    this.kernel=kernel;this.handle=kernel.createOutbound();this.transitionBackups=transitionBackups;this.state=state;this.events=events;this.fire=fire;this.reset();
  }
  resetLocal(){this.history=[];this.life=null;this.look=[0,0];this.sentLook=[0,0];this.sentSequence=0;this.unsent=[];this.transition=null;this.held=null;this.acked=0;}
  reset(){const k=this.kernel;k.begin();k.run('outboundReset',this.handle);this.resetLocal();}
  clearPending(){const k=this.kernel;k.begin();k.run('outboundClear',this.handle);this.history=[];this.transition=null;this.held=null;this.unsent=[];}
  ack(sequence){const k=this.kernel;k.begin();k.put(typeof sequence==='number'?sequence:NaN);k.run('outboundAck',this.handle);this.acked=k.numbers[A.ACKED];const transition=k.numbers[A.TRANSITION];if(!transition)this.transition=null;}
  send(commands){
    const k=this.kernel;
    for(const command of commands){
      // Held names still belong to the page registry. Their canonical source
      // string crosses as UTF-16; Rust compares it without allocating a key.
      const held=JSON.stringify([...command.down].sort()),look=command.look||[];
      k.begin();k.put(command.epoch);k.put(command.generation);k.put(command.sequence);k.put(command.tick);k.put(held.length);for(let i=0;i<held.length;i++)k.put(held.charCodeAt(i));k.put(look.length);for(const point of look){k.put(point[0]);k.put(point[1]);}k.run('outboundCommand',this.handle);
      const reset=k.numbers[O.RESET],changed=k.numbers[O.CHANGED],transition=k.numbers[O.TRANSITION],x=k.numbers[O.LOOK_X],y=k.numbers[O.LOOK_Y];
      if(reset)this.resetLocal();this.life=`${command.epoch}:${command.generation}`;this.look[0]=x;this.look[1]=y;
      command.sent=false;command.worldLook=[];command.lookSent=false;
      // A paused transport cannot retain an unbounded chain of page commands.
      // Its newest cumulative look repairs every dropped replaceable sample.
      if(this.unsent.length===4096)this.unsent.shift();this.unsent.push(command);
      const state={...command,actions:[],look:[],lookTotal:[x,y]};for(const key of ['human','flightTuning','sent','worldLook','lookSent'])delete state[key];
      this.held=held;if(changed)this.transition=state;if(!transition)this.transition=null;this.history=[state];
      const actions=discreteActions(command);
      if(actions.length||command.human?.length)if(!this.events({type:'actions',id:command.sequence,tick:command.tick,generation:command.generation,epoch:command.epoch,actions,...(command.human?.length?{human:command.human}:{})}))return false;
      if(!this.fire(command))return false;
    }
    if(!commands.length)return false;
    const latest=this.history[0],backup=this.transition;
    const message={type:'inputs',commands:this.transitionBackups&&backup&&backup.sequence<latest.sequence?[backup,latest]:[latest]};
    if(message.commands.length>1&&new TextEncoder().encode(JSON.stringify(message)).length>1152)message.commands=[latest];
    const sent=this.state(message);if(sent)this.sent(message.commands,commands);return sent;
  }
  sent(states,commands){
    const k=this.kernel;
    for(const state of states){
      k.begin();k.put(state.sequence);k.put(state.lookTotal[0]);k.put(state.lookTotal[1]);k.run('outboundSent',this.handle);const accepted=k.numbers[S.ACCEPTED],dx=k.numbers[S.DX],dy=k.numbers[S.DY];
      if(!accepted)continue;
      for(const command of commands)if(command.sequence===state.sequence){command.sent=true;command.worldLook=dx||dy?[[dx,dy]]:[];break;}
      this.sentLook=state.lookTotal;this.sentSequence=state.sequence;
    }
    let retained=0;for(const command of this.unsent){if(command.sequence>this.sentSequence)this.unsent[retained++]=command;else command.lookSent=true;}this.unsent.length=retained;
  }
  dispose(){if(this.handle){this.kernel.dropOutbound(this.handle);this.handle=0;}}
}

// A movement acknowledgement must not erase a reliable action still in flight.
export function predictionCommands(pending, actions, ackActions, held) {
  const bySequence=new Map(pending.map(command=>[command.sequence,{...command,actions:command.sequence>ackActions?command.actions:[]} ]));
  for(const command of actions)if(command.sequence>ackActions&&!bySequence.has(command.sequence))bySequence.set(command.sequence,{...command,down:held,look:[],worldLook:[]});
  return [...bySequence.values()].sort((a,b)=>a.sequence-b.sequence);
}

// The server rejects movement more than two seconds behind its current tick.
// Such replaceable commands cannot ever be acknowledged; retaining them would
// fill the prediction history and prevent fresh input after a rendering stall.
export function expireMovement(pending,tick,generation){
  return pending.filter(command=>command.generation===generation&&command.tick>=tick-120);
}
