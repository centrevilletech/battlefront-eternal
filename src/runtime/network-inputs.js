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
  constructor({state,events,fire,transitionBackups=true}) {this.transitionBackups=transitionBackups;this.state=state;this.events=events;this.fire=fire;this.reset();}
  reset(){this.history=[];this.life=null;this.look=[0,0];this.sentLook=[0,0];this.sentSequence=0;this.unsent=[];this.transition=null;this.held=null;this.acked=0;}
  clearPending(){this.history=[];this.transition=null;this.held=null;this.unsent=[];}
  ack(sequence){
    if(!Number.isSafeInteger(sequence)||sequence<=this.acked)return;
    this.acked=sequence;
    if(this.transition?.sequence<=sequence)this.transition=null;
  }
  send(commands) {
    for(const command of commands) {
      const life=`${command.epoch}:${command.generation}`;
      if(life!==this.life){this.reset();this.life=life;}
      for(const [x,y] of command.look || []){this.look[0]+=x;this.look[1]+=y;}
      command.sent=false;command.worldLook=[];command.lookSent=false;this.unsent.push(command);
      const state={...command,actions:[],look:[],lookTotal:[...this.look]};for(const key of ['human','flightTuning','sent','worldLook','lookSent'])delete state[key];
      const held=JSON.stringify([...state.down].sort());
      if(this.held!==null&&held!==this.held)this.transition=state;
      this.held=held;
      if(this.transition&&(this.transition.sequence<=this.acked||this.transition.tick<state.tick-24))this.transition=null;
      this.history=[state];
      const actions=discreteActions(command);
      if(actions.length || command.human?.length) {
        if(!this.events({type:'actions',id:command.sequence,tick:command.tick,generation:command.generation,epoch:command.epoch,actions,...(command.human?.length?{human:command.human}:{})}))return false;
      }
      if(!this.fire(command))return false;
    }
    if(!commands.length)return false;
    const latest=this.history[0],backup=this.transition;
    const message={type:'inputs',commands:this.transitionBackups&&backup&&backup.sequence<latest.sequence?[backup,latest]:[latest]};
    // Leave room for the transport sequence envelope in a 1200-byte payload.
    // Large gamepad/custom bindings retain latest-only delivery instead of
    // introducing SCTP fragmentation merely for optional redundancy.
    if(message.commands.length>1&&new TextEncoder().encode(JSON.stringify(message)).length>1152)message.commands=[latest];
    const sent=this.state(message);
    if(sent)this.sent(message.commands,commands);
    return sent;
  }
  // The commands just sent: each newer than the last takes the change in the
  // running total since that one (the one before it carried none of its own),
  // and every command up to it has had its look go.
  sent(states,commands){
    for(const state of states){
      if(state.sequence<=this.sentSequence)continue;
      const dx=state.lookTotal[0]-this.sentLook[0],dy=state.lookTotal[1]-this.sentLook[1];
      const command=commands.find(c=>c.sequence===state.sequence);
      if(command){command.sent=true;command.worldLook=dx||dy?[[dx,dy]]:[];}
      this.sentLook=state.lookTotal;this.sentSequence=state.sequence;
    }
    this.unsent=this.unsent.filter(command=>{if(command.sequence>this.sentSequence)return true;command.lookSent=true;return false;});
  }
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
