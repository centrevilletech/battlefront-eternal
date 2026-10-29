import { INPUT_ACTIONS } from './network-entity-input.js';

// Only what the wire registry can name goes down the action lane. Held
// flight names (rollLeft, throttle, ...) travel as key codes in `down`;
// letting one through here made encodeEntityInput throw every frame.
export const discreteActions = command => (command.actions || []).filter(action => INPUT_ACTIONS.includes(action));

// Latest held state is expendable. Discrete actions and fire edges are not.
// Cumulative mouse counts recover look after a lost burst. One bounded held
// transition backup preserves its original execution tick when that packet is
// lost; actions/fire are emitted once on their independent reliable lanes.
export class NetworkInputs {
  constructor({state,events,fire,transitionBackups=true}) {this.transitionBackups=transitionBackups;this.state=state;this.events=events;this.fire=fire;this.reset();}
  reset(){this.history=[];this.life=null;this.look=[0,0];this.transition=null;this.held=null;this.acked=0;}
  clearPending(){this.history=[];this.transition=null;this.held=null;}
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
      const state={...command,actions:[],look:[],lookTotal:[...this.look]};delete state.human;delete state.flightTuning;
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
    return this.state(message);
  }
}

// A movement acknowledgement must not erase a reliable action still in flight.
export function predictionCommands(pending, actions, ackActions, held) {
  const bySequence=new Map(pending.map(command=>[command.sequence,{...command,actions:command.sequence>ackActions?command.actions:[]} ]));
  for(const command of actions)if(command.sequence>ackActions&&!bySequence.has(command.sequence))bySequence.set(command.sequence,{...command,down:held,look:[]});
  return [...bySequence.values()].sort((a,b)=>a.sequence-b.sequence);
}

// The server rejects movement more than two seconds behind its current tick.
// Such replaceable commands cannot ever be acknowledged; retaining them would
// fill the prediction history and prevent fresh input after a rendering stall.
export function expireMovement(pending,tick,generation){
  return pending.filter(command=>command.generation===generation&&command.tick>=tick-120);
}
