import {pageNetworkFor} from './page-network.js';
import {NetPreviewFrame as PF,NetOutbound as O,NetOutboundBatch as B,NetOutboundFire as F,NetAckResult as A,NetSentResult as S,NetLimits as L} from './network-layout.js';
import {INPUT_ACTIONS} from './network-entity-input.js';
const EMPTY=Object.freeze([]),RECEIPTS=4096;
// DOM event routing only needs membership here; the native outbound owner
// constructs the actual reliable action list without a filtered JS array.
export function hasDiscreteActions(command){for(const action of command.actions||EMPTY)if(INPUT_ACTIONS.includes(action))return true;return false;}
// Publications borrow DOM command metadata only until the callback returns.
// Rust owns their look totals and transition choice. Three retained views
// cover the newest state, its backup and the state currently being prepared.
class InputPublication {
  constructor(){this.command=null;this.revision=NaN;this.actions=EMPTY;this.look=EMPTY;this.lookTotal=new Float64Array(2);}
  // Explicit cold captures own their values. Live binary transport reads the
  // getters directly and never constructs this diagnostic object.
  toJSON(){const state={...this.command,actions:[],look:[],lookTotal:Array.from(this.lookTotal)};for(const key of ['human','flightTuning','sent','worldLook','lookSent'])delete state[key];return state;}
}
for(const key of ['epoch','generation','sequence','tick','down','mouseScale','aspect','viewTick','invertMouse','firstPerson','flightSettings','gamepad'])Object.defineProperty(InputPublication.prototype,key,{get(){return this.command[key];}});
class ActionPublication {
  constructor(){this.command=null;this.type='actions';this.actions=[];}
  get id(){return this.command.sequence;}get tick(){return this.command.tick;}get generation(){return this.command.generation;}get epoch(){return this.command.epoch;}get human(){return this.command.human;}
  toJSON(){return {type:'actions',id:this.id,tick:this.tick,generation:this.generation,epoch:this.epoch,actions:this.actions,...(this.human?.length?{human:this.human}:{})};}
}
export class NetworkInputs {
  constructor({state,events,fire,aim=()=>{},transitionBackups=true,kernel=pageNetworkFor(null)}){
    this.kernel=kernel;this.handle=kernel.createOutbound();this.transitionBackups=transitionBackups;this.state=state;this.events=events;this.fire=fire;this.aim=aim;this.currentRow=0;
    this.latest=null;this.backup=null;this.queue=new Array(RECEIPTS).fill(null);this.head=0;this.count=0;
    this.publications=Array.from({length:3},()=>new InputPublication());this.singleMessage={type:'inputs',commands:[null]};this.backupMessage={type:'inputs',commands:[null,null]};this.event=new ActionPublication();
    // Receipt arrays retain their single point even while the command shows
    // the shared empty look. Reusing a slot never truncates its backing store.
    this.receipts=Array.from({length:RECEIPTS},()=>{const point=[NaN,NaN];return {point,look:[point]};});this.reset();
  }
  // Accounting getters are cold fixture inspection. Production never reads
  // them to make a send decision, accumulate look or acknowledge a command.
  capture(){return JSON.parse(this.kernel.kernel.outbound_capture(this.handle));}
  get life(){return this.capture().life;}get look(){return this.capture().look;}get sentLook(){return this.capture().sentLook;}
  get sentSequence(){return this.capture().sentSequence;}get held(){return this.capture().held;}get acked(){return this.capture().acked;}
  get transition(){return this.backup;}
  get history(){return this.latest?[this.latest]:[];}
  get unsent(){const out=[];for(let i=0;i<this.count;i++)out.push(this.queue[(this.head+i)%RECEIPTS]);return out;}
  resetLocal(){this.latest=null;this.backup=null;this.queue.fill(null);this.head=0;this.count=0;}
  reset(){const k=this.kernel;if(this.handle&&!k.disposed){k.begin();k.run('outboundReset',this.handle);}this.resetLocal();}
  clearPending(){const k=this.kernel;if(this.handle&&!k.disposed){k.begin();k.run('outboundClear',this.handle);}this.resetLocal();}
  ack(sequence){const k=this.kernel;if(!this.handle||k.disposed)return;k.begin();k.put(typeof sequence==='number'?sequence:NaN);k.run('outboundAck',this.handle);if(!k.numbers[A.TRANSITION])this.backup=null;}
  send(commands,start=0,end=commands.length){
    const k=this.kernel;if(k.disposed||!this.handle||start>=end)return false;
    // The command clock bounds ordinary frame batches. Excess foreign input
    // keeps the newest bounded window instead of growing an exchange or pool.
    start=Math.max(start,end-L.PREVIEW_COMMANDS);k.begin();
    for(let index=start;index<end;index++){
      const c=commands[index],prefix=k.count;k.put(0);k.put(c.mouseScale??k.previewFrame?.[PF.SCALE]??1);k.put(c.invertMouse??!!k.previewFrame?.[PF.INVERT]?1:0);
      const at=k.count;k.put(c.epoch);k.put(c.generation);k.put(c.sequence);k.put(c.tick);
      const heldAt=k.count;k.put(0);const names=c.down||EMPTY;
      for(const name of names){k.put(name.length);for(let i=0;i<name.length;i++)k.put(name.charCodeAt(i));}
      k.input[heldAt]=k.count-heldAt-1;
      const look=c.look||EMPTY;k.put(look.length);for(const point of look){k.put(point[0]);k.put(point[1]);}
      const actions=c.actions||EMPTY;k.put(actions.length);for(const action of actions)k.put(action==='firePrimary'?21:action==='fireSecondary'?22:INPUT_ACTIONS.indexOf(action));
      k.input[prefix+B.WORD_COUNT]=k.count-at;
    }
    k.outboundBatch(this.handle,end-start);this.aim(end-start);
    for(let index=start;index<end;index++){
      const c=commands[index],at=(index-start)*O.STRIDE,x=k.outboundResults;
      if(x[at+O.RESET])this.resetLocal();
      c.sent=false;c.worldLook=EMPTY;c.lookSent=false;
      if(this.count===RECEIPTS){this.queue[this.head]=null;this.head=(this.head+1)%RECEIPTS;this.count--;}
      this.queue[(this.head+this.count++)%RECEIPTS]=c;
      let state;for(const row of this.publications)if(row!==this.backup&&row!==this.latest){state=row;break;}
      state.command=c;state.lookTotal[0]=x[at+O.LOOK_X];state.lookTotal[1]=x[at+O.LOOK_Y];state.revision=x[at+O.REVISION];
      if(x[at+O.CHANGED])this.backup=state;if(!x[at+O.TRANSITION])this.backup=null;
      this.latest=state;
      // Fire transport reads the corresponding command's native aim. Other
      // callbacks may reuse the scalar result span, so restore just this pair.
      k.numbers[O.BODY_YAW]=x[at+O.BODY_YAW];k.numbers[O.BODY_PITCH]=x[at+O.BODY_PITCH];
      const event=this.event;event.command=c;event.actions.length=0;
      const actionStart=x[at+O.ACTION_START],actionCount=x[at+O.ACTION_COUNT];for(let i=0;i<actionCount;i++)event.actions.push(INPUT_ACTIONS[k.outboundActions[actionStart+i]]);
      if((event.actions.length||c.human?.length)&&!this.events(event))return false;
      this.currentRow=at;if(!this.fire(c))return false;
    }
    // Both packet shapes have fixed array lengths. Callbacks consume them
    // synchronously, before another frame can replace their borrowed views.
    const latest=this.latest,last=(end-start-1)*O.STRIDE,useBackup=this.transitionBackups&&this.backup&&k.outboundResults[last+O.BACKUP];
    const message=useBackup?this.backupMessage:this.singleMessage;message.commands[0]=useBackup?this.backup:latest;if(useBackup)message.commands[1]=latest;
    const sent=this.state(message);if(sent)this.sent(message.commands,commands,start,end);return sent;
  }
  eachFire(visitor){const k=this.kernel,x=k.outboundResults,start=x[this.currentRow+O.FIRE_START],count=x[this.currentRow+O.FIRE_COUNT];for(let i=0;i<count;i++){const at=(start+i)*F.STRIDE;visitor(k.outboundFires[at+F.BUTTON],!!k.outboundFires[at+F.PRESSED],!!k.outboundFires[at+F.HELD]);}}
  sent(states,commands,start=0,end=commands.length){
    const k=this.kernel;let sequence=0;
    for(const state of states){
      k.begin();k.put(state.sequence);k.put(state.lookTotal[0]);k.put(state.lookTotal[1]);k.put(state.tick);k.put(state.revision);k.run('outboundSent',this.handle);
      sequence=k.numbers[S.SEQUENCE];if(!k.numbers[S.ACCEPTED])continue;
      const dx=k.numbers[S.DX],dy=k.numbers[S.DY];
      for(let index=start;index<end;index++){const c=commands[index];if(c.sequence!==state.sequence)continue;
        const receipt=this.receipts[c.sequence%RECEIPTS];receipt.point[0]=dx;receipt.point[1]=dy;c.worldLook=dx||dy?receipt.look:EMPTY;c.sent=true;break;}
    }
    // The command clock owns these identities. Delivery releases their
    // transport references using the sequence published by the Rust receipt.
    let retained=0;for(let i=0;i<this.count;i++){const c=this.queue[(this.head+i)%RECEIPTS];if(c.sequence>sequence)this.queue[(this.head+retained++)%RECEIPTS]=c;else c.lookSent=true;}this.count=retained;
  }
  dispose(){if(this.handle){this.kernel.dropOutbound(this.handle);this.handle=0;}}
}
// Commands too old for the server are transport expiry, independent from
// reliable actions retained until an applied authoritative correction.
export function expireMovement(pending,tick,generation){return pending.filter(command=>command.generation===generation&&command.tick>=tick-120);}
