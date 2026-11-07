// InputMailbox's native owner keeps pending commands and all consumption
// watermarks. Object references below belong to the host's input producers;
// they never carry a serialized copy of the retained native queue.
import { publicationView } from '../../../runtime/input-view.js';
import { INPUT_CODES } from '../../../runtime/network-entity-input.js';
import { InputMailboxCallMirror, InputMailboxCallLayout, ImEntryMirror, ImResultMirror, ImStateMirror, ImTakenMirror, ImLookMirror, ImFireMirror, InputViewUpdateMirror, ImEntryLayout as E, ImOperation_INDEX, putImLook, putImFire, putImField, putWiView,
  InputMailboxSetupLayout, putInputMailboxSetup, putInputMailboxCall, putInputViewUpdate, syncImResult, syncImState, InputViewUpdateLayout, WireSpawnFactsMirror, WireSpawnFactsLayout, putWireSpawnFacts } from '../../gen/index.js';
import { BLOCK_HEADER_WORDS } from '../../gen/boundary.js';
function trim(array,length){while(array.length>length)array.pop();}
const keys = ['mouseScale','invertMouse','firstPerson','aspect','viewTick','flightSettings','gamepad'];
const result = { Mirror: ImResultMirror, sync: syncImResult }, inspected = { Mirror: ImStateMirror, sync: syncImState };
// JS checks shapes before generated projection can intern names or read lists.
// Rust owns stamp, ordering, range and capacity admission after this boundary.
function invalidCommand(c,codes) {
  if(!c||typeof c!=='object')return 'invalid input payload';
  // Refused stamps never advance the received watermark. In particular,
  // tick zero is invalid even while the world's startup window includes it.
  if(!Number.isSafeInteger(c.sequence)||c.sequence<1||!Number.isSafeInteger(c.tick)||c.tick<1||!Number.isSafeInteger(c.generation)||c.generation<0)return 'invalid input stamp';
  if(!Array.isArray(c.down)||c.down.length>64||!Array.isArray(c.actions)||c.actions.length>2048||!Array.isArray(c.look)||c.look.length>2048)return 'invalid input payload';
  // Check names before the generated boundary can intern client strings.
  // Held names belong to the connection registry, not a second private list.
  for(let i=0;i<c.down.length;i++)if(typeof c.down[i]!=='string'||c.down[i].length>64||!codes.includes(c.down[i]))return 'invalid input payload';
  for(let i=0;i<c.actions.length;i++)if(typeof c.actions[i]!=='string'||c.actions[i].length>64)return 'invalid input payload';
  for(let i=0;i<c.look.length;i++){const p=c.look[i];if(!Array.isArray(p)||p.length!==2||!Number.isFinite(p[0])||!Number.isFinite(p[1]))return 'invalid input payload';}
  // Cumulative look repairs lost packets. Keep its finite magnitude bounded
  // so subtraction remains usable after any paused or reordered burst.
  if(c.lookTotal!==undefined&&(!Array.isArray(c.lookTotal)||c.lookTotal.length!==2||!Number.isFinite(c.lookTotal[0])||!Number.isFinite(c.lookTotal[1])||Math.abs(c.lookTotal[0])>2**48||Math.abs(c.lookTotal[1])>2**48))return 'invalid cumulative look';
  if(c.mouseScale!==undefined&&(!Number.isFinite(c.mouseScale)||c.mouseScale<=0)||c.aspect!==undefined&&(!Number.isFinite(c.aspect)||c.aspect<=0)||c.invertMouse!==undefined&&typeof c.invertMouse!=='boolean'||c.firstPerson!==undefined&&typeof c.firstPerson!=='boolean')return 'invalid view input';
  if(c.viewTick!==undefined&&(!Number.isFinite(c.viewTick)||c.viewTick<0||c.viewTick>0xffffffff))return 'invalid view tick';
  // Optional settings preserve presence, including false and viewTick zero.
  // Binding names stay opaque until the existing gameplay consumer uses them.
  const fs=c.flightSettings;
  if(fs!==undefined){
    if(!fs||typeof fs!=='object'||Array.isArray(fs)||fs.sensitivity!==undefined&&(!Number.isFinite(fs.sensitivity)||fs.sensitivity<=0||fs.sensitivity>10)||fs.invert!==undefined&&typeof fs.invert!=='boolean'||fs.curve!==undefined&&(!Number.isFinite(fs.curve)||fs.curve<.1||fs.curve>4))return 'invalid flight settings';
    if(fs.bindings!==undefined){
      if(!fs.bindings||typeof fs.bindings!=='object'||Array.isArray(fs.bindings))return 'invalid flight settings';
      let count=0;for(const key in fs.bindings)if(Object.hasOwn(fs.bindings,key)){
        const values=fs.bindings[key];if(++count>32||key.length>64||!Array.isArray(values)||values.length>8)return 'invalid flight settings';
        for(let i=0;i<values.length;i++)if(typeof values[i]!=='string'||values[i].length>64)return 'invalid flight settings';
      }
    }
  }
  // A null pad means no update. A present centred pad is a real release,
  // so preserve its producer object and check every supplied button and axis.
  const pad=c.gamepad;
  if(pad!=null){
    if(!Array.isArray(pad.axes)||pad.axes.length>16||!Array.isArray(pad.buttons)||pad.buttons.length>32)return 'invalid gamepad input';
    for(let i=0;i<pad.axes.length;i++)if(!Number.isFinite(pad.axes[i])||Math.abs(pad.axes[i])>1)return 'invalid gamepad input';
    for(let i=0;i<pad.buttons.length;i++)if(!pad.buttons[i]||typeof pad.buttons[i].pressed!=='boolean')return 'invalid gamepad input';
  }
  // Trigger and spawn choices remain bounded independently of held keys.
  // Their opaque identities must fit the reserved per-slot token ranges.
  if(c.fire!==undefined){
    if(!Array.isArray(c.fire)||c.fire.length>16)return 'invalid trigger edges';
    for(let i=0;i<c.fire.length;i++){const e=c.fire[i];if(!e||!Number.isSafeInteger(e.id)||e.id<1||(e.button!==0&&e.button!==1)||typeof e.pressed!=='boolean')return 'invalid trigger edges';}
  }
  if(c.human!==undefined){
    if(!Array.isArray(c.human)||c.human.length>16)return 'invalid human input';
    for(let i=0;i<c.human.length;i++){
      const h=c.human[i];if(!h||typeof h!=='object')return 'invalid human input';
      if(h.kind==='team'){if(!Number.isInteger(h.team)||h.team<1||h.team>7)return 'invalid human input';}
      else if(h.kind==='spawn'){if(typeof h.className!=='string'||!h.className.length||h.className.length>64||h.post!=null&&(typeof h.post!=='string'||!h.post.length||h.post.length>64))return 'invalid human input';}
      else if(h.kind!=='suicide'&&h.kind!=='cancel')return 'invalid human input';
    }
  }
  return null;
}
export class InputMailbox {
  constructor(epoch = 1, capacity = 256, bridge = null, registry = {inputCodes:INPUT_CODES}) {
    if (!bridge) throw Error('InputMailbox requires a battle simulation bridge');
    const codes = registry.inputCodes;
    if (!Array.isArray(codes) || codes.length > 128 || new Set(codes).size !== codes.length ||
        codes.some(code => typeof code !== 'string' || !code.length || code.length > 64)) throw Error('invalid input-code registry');
    this.codes = Object.freeze(codes.slice());
    this.bridge = bridge;
    bridge.callArgs({words:InputMailboxSetupLayout.words,put:putInputMailboxSetup},{epoch,capacity,codes:this.codes});
    this.handle = this.bridge.x.input_mailbox_create();
    if (!this.handle) throw Error('input mailbox admission failed');
    this.epoch = epoch; this.capacity = capacity; this.received = this.ack = this.ackTick = this.discarded = this.generation = this.late = this.stateTick = 0;
    this.emptyDown = []; this.down = this.emptyDown; this.lookTotal = [0,0];
    // Each queue slot lends producer references until Rust retires its held
    // and view tokens. Eight extra slots cover the retained held/view owners.
    this.references = Array.from({length:capacity+8}, () => ({command:null,pending:false,held:false,human:[],humanRows:Array.from({length:16},()=>({team:{kind:'team',team:0},spawn:{kind:'spawn',className:'',post:null},suicide:{kind:'suicide'},cancel:{kind:'cancel'}}))}));
    this.retained = []; this.previousRetained = []; this.pendingLooks=0;this.pendingFires=0;
    this.cursor = 0; this.lastRejection = null;
    // take lends this command only until the next mailbox operation. The
    // world consumes it synchronously; historical callers must copy it. Keep
    // optional slots present as undefined to avoid repeated property churn.
    this.command = {epoch,sequence:0,tick:0,generation:0,down:this.down,actions:[],look:[],human:undefined,fire:undefined,
      mouseScale:undefined,invertMouse:undefined,firstPerson:undefined,aspect:undefined,viewTick:undefined,flightSettings:undefined,gamepad:undefined};
    this.humans = []; this.fires = []; this.update = new InputViewUpdateMirror();
    this.view = new Proxy(this.command, {
      set: (object,key,value) => {object[key]=value;this.changedView(key,true);return true;},
      deleteProperty: (object,key) => {object[key]=undefined;this.changedView(key,false);return true;},
    });
    this.change = new InputMailboxCallMirror(); this.entry = new ImEntryMirror(); this.exchange = new ImResultMirror();
    this.input = { words: InputMailboxCallLayout.words, put: putInputMailboxCall };
    // Reserve row identities once. Empty ticks may shrink the visible lists
    // without losing the objects that a later populated exchange will reuse.
    this.lookRows = Array.from({length:2048}, () => new ImLookMirror());
    this.viewRows = keys.map(key => ({key,value:0}));
    for(let i=0;i<keys.length;i++)bridge.strings.id(keys[i]);
    this.taken = new ImTakenMirror();
    this.resultLooks = Array.from({length:2048}, () => new ImLookMirror());
    this.resultFires = Array.from({length:16*capacity}, () => new ImFireMirror());
    this.fireRows = Array.from({length:16*capacity}, () => ({id:0,button:0,pressed:false}));
    this.stateViews = keys.map(key => ({key,value:0}));
    // Variable record sizes come from their generated writers too. Probe
    // them once, then write directly into the reserved native call block.
    const probe=[];
    putImLook(bridge,this.lookRows[0],probe,0,null);this.lookWords=probe.length;probe.length=0;
    putImFire(bridge,this.resultFires[0]??new ImFireMirror(),probe,0,null);this.fireWords=probe.length;probe.length=0;
    putImField(bridge,this.viewRows[0],probe,0,null);this.fieldWords=probe.length;
    this.viewInput = { words: InputViewUpdateLayout.words, put: putInputViewUpdate };
  }
  changedView(key, present) {
    const index = keys.indexOf(key); if (index < 0 || !this.handle) return;
    this.update.key = index; this.update.present = present; publicationView(this.command, this.update.view);
    this.bridge.callArgs(this.viewInput, this.update); this.bridge.x.input_mailbox_view(this.handle);
  }
  // InputMailbox.run bypasses the generic growing JS heap. The generated
  // record layouts and leaf writers still define every boundary position.
  writeCall() {
    const b=this.bridge,c=this.change,e=c.entry,C=InputMailboxCallLayout;
    const count=e?e.codes.length+e.actions.length+e.look.length*this.lookWords+e.view.length*this.fieldWords+e.human.length+e.fire.length*this.fireWords:0;
    // Intern before taking the writable view: a first use can grow memory
    // and detach it. Registry and view names were admitted during construction.
    if(e)for(let i=0;i<e.actions.length;i++)b.strings.id(e.actions[i]);
    const ptr=b.x.sim_call_room(C.words,count),block=b.readBlock(ptr),f=block.words,at=block.base+BLOCK_HEADER_WORDS;
    f.fill(0,at,at+C.words);
    f[at+C.op]=ImOperation_INDEX.get(c.op);f[at+C.entry]=e?0:1;
    f[at+C.sequence]=c.sequence;f[at+C.epochMatches]=c.epochMatches?1:0;f[at+C.tick]=c.tick;f[at+C.generation]=c.generation;
    if(!e)return;
    const row=at+C.entry+1,heap=at+C.words;let n=0;
    putWiView(b,e.publication,f,row+E.publication,null);
    f[row+E.identity]=e.identity;f[row+E.sequence]=e.sequence;f[row+E.tick]=e.tick;f[row+E.generation]=e.generation;f[row+E.down]=e.down;
    f[row+E.codes]=n;f[row+E.codesLength]=e.codes.length;
    for(let i=0;i<e.codes.length;i++)f[heap+n++]=b.strings.id(e.codes[i]);
    f[row+E.actions]=n;f[row+E.actionsLength]=e.actions.length;
    for(let i=0;i<e.actions.length;i++)f[heap+n++]=b.strings.id(e.actions[i]);
    f[row+E.look]=n;f[row+E.lookLength]=e.look.length;
    for(let i=0;i<e.look.length;i++){putImLook(b,e.look[i],f,heap+n,null);n+=this.lookWords;}
    f[row+E.lookTotal]=e.lookTotal===null?1:0;
    if(e.lookTotal!==null){f[row+E.lookTotal+1]=e.lookTotal[0];f[row+E.lookTotal+2]=e.lookTotal[1];}
    f[row+E.view]=n;f[row+E.viewLength]=e.view.length;
    for(let i=0;i<e.view.length;i++){putImField(b,e.view[i],f,heap+n,null);n+=this.fieldWords;}
    f[row+E.human]=n;f[row+E.humanLength]=e.human.length;
    for(let i=0;i<e.human.length;i++)f[heap+n++]=e.human[i];
    f[row+E.fire]=n;f[row+E.fireLength]=e.fire.length;
    for(let i=0;i<e.fire.length;i++){putImFire(b,e.fire[i],f,heap+n,null);n+=this.fireWords;}
  }
  run(op, tick = 0, generation = 0) {
    if (!this.handle) throw Error("input mailbox is disposed");
    const c = this.change; c.op = op; c.tick = tick; c.generation = generation;
    this.writeCall();
    const ptr = this.bridge.x.input_mailbox_owned_run(this.handle);
    if (!ptr) throw Error('input mailbox operation refused');
    // Generated readers shrink variable lists. Restore their reserved row
    // identities before decoding so alternating empty/full ticks reuse them.
    this.exchange.command = this.taken;
    if(op==='take'){
      for(let i=this.taken.look.length;i<Math.min(2048,this.pendingLooks);i++)this.taken.look[i]=this.resultLooks[i];
      for(let i=this.taken.fire.length;i<this.pendingFires;i++)this.taken.fire[i]=this.resultFires[i];
    }
    for(let i=this.exchange.state.view.length;i<7;i++)this.exchange.state.view[i]=this.stateViews[i];
    const out = this.bridge.callResult(result, ptr, this.exchange), s = out.state;
    this.received = s.received; this.ack = s.ack; this.ackTick = s.ackTick; this.discarded = s.discarded; this.generation = s.generation; this.late = s.late; this.stateTick = s.stateTick;
    this.down = s.down === null ? this.emptyDown : this.references[s.down-1].command.down;
    this.lookTotal[0] = s.lookTotal[0]; this.lookTotal[1] = s.lookTotal[1];
    // Rust retains view tokens and publishes their protocol projection. Only
    // borrowed producer values needed by gameplay appear on this command.
    for (let i=0;i<keys.length;i++) {
      const key=keys[i]; let field=null;
      for(let j=0;j<s.view.length;j++)if(s.view[j].key===key){field=s.view[j];break;}
      if(field){if(field.value>=8)this.command[key]=this.references[Math.floor(field.value/8)-1].command[key];}
      else this.command[key]=undefined;
    }
    for(let i=0;i<out.retired.length;i++){
      const row=this.references[out.retired[i]-1];row.pending=false;
      this.pendingLooks-=row.command.lookTotal?1:row.command.look.length;this.pendingFires-=row.command.fire?.length??0;
    }
    if(op==='clear'){for(let i=0;i<this.references.length;i++){const row=this.references[i];row.pending=false;if(!row.held)row.command=null;}this.pendingLooks=0;this.pendingFires=0;}
    this.lastRejection=out.error;
    return out;
  }
  // Only the previous held/view owners and newly retired packets can lose
  // their last reference. Pending queue slots need no per-tick full scan.
  cleanup() {
    const old=this.retained;this.retained=this.previousRetained;this.previousRetained=old;trim(this.retained,0);
    for(let i=0;i<old.length;i++)old[i].held=false;
    const s=this.exchange.state;
    if(s.down!==null){const row=this.references[s.down-1];row.held=true;this.retained[this.retained.length]=row;}
    for(let i=0;i<s.view.length;i++){
      if(s.view[i].value<8)continue;
      const row=this.references[Math.floor(s.view[i].value/8)-1];
      if(!row.held){row.held=true;this.retained[this.retained.length]=row;}
    }
    for(let i=0;i<old.length;i++){const row=old[i];if(!row.pending&&!row.held)row.command=null;}
    const retired=this.exchange.retired;
    for(let i=0;i<retired.length;i++){const row=this.references[retired[i]-1];if(!row.pending&&!row.held)row.command=null;}
  }
  get rejected() {return this.bridge.x.input_mailbox_rejected(this.handle);}
  reject(reason) {
    this.lastRejection=reason;this.bridge.x.input_mailbox_reject(this.handle);return false;
  }
  push(command) {
    if(!this.handle)return false;
    const reason=invalidCommand(command,this.codes);
    if(reason)return this.reject(reason);
    const c=this.change;c.entry=null;c.sequence=command.sequence;c.epochMatches=command.epoch===this.epoch;
    if(!this.run('admit').accepted)return false;
    let slot=-1;
    for(let i=0;i<this.references.length;i++) {
      const next=(this.cursor+i)%this.references.length;
      if(this.references[next].command===null){slot=next;break;}
    }
    if(slot<0)return this.reject('input queue full');
    this.cursor=(slot+1)%this.references.length;
    const id=slot+1,row=this.references[slot],e=this.entry;
    row.command=command;row.pending=true;
    // Only human choices need a cleaned value projection; reserve their
    // objects on the slot and refill them without copying the whole command.
    const human=command.human;
    const humanCount=human?.length??0;trim(row.human,humanCount);
    for(let i=0;i<humanCount;i++) {
      const source=human[i],target=row.humanRows[i][source.kind];row.human[i]=target;
      if(source.kind==='team')target.team=source.team;
      if(source.kind==='spawn'){target.className=source.className;target.post=source.post??null;}
    }
    e.identity=id;e.sequence=command.sequence;e.tick=command.tick;e.generation=command.generation;
    e.down=id;e.codes=command.down;e.actions=command.actions;e.lookTotal=command.lookTotal??null;
    // Token arithmetic refers to fixed slots rather than an ever-growing
    // serial or a Map entry allocated for each newly admitted command.
    trim(e.look,command.look.length);
    for(let i=0;i<command.look.length;i++){const look=this.lookRows[i];look.point=command.look[i];look.object=id*2048+i;e.look[i]=look;}
    trim(e.human,row.human.length);for(let i=0;i<row.human.length;i++)e.human[i]=id*16+i;
    e.fire=command.fire??this.emptyDown;trim(e.view,0);
    for(let i=0;i<keys.length;i++)if(command[keys[i]]!==undefined){const field=this.viewRows[i];field.value=id*8+i;e.view[e.view.length]=field;}
    publicationView(command,e.publication);c.entry=e;
    try {
      const accepted=this.run('push').accepted;if(!accepted){row.pending=false;row.command=null;}else{this.pendingLooks+=command.lookTotal?1:command.look.length;this.pendingFires+=command.fire?.length??0;}return accepted;
    } catch(error) {row.pending=false;row.command=null;return this.reject('invalid input payload');}
    finally {c.entry=null;this.cleanup();}
  }
  take(tick, generation) {
    const out=this.run('take',tick,generation),taken=out.command,command=this.command;
    command.epoch=this.epoch;command.sequence=tick;command.tick=tick;command.generation=generation;command.down=this.down;
    trim(command.actions,taken.actions.length);for(let i=0;i<taken.actions.length;i++)command.actions[i]=taken.actions[i];
    trim(command.look,taken.look.length);
    for(let i=0;i<taken.look.length;i++) {
      const row=taken.look[i];command.look[i]=row.object===null?row.point:this.references[Math.floor(row.object/2048)-1].command.look[row.object%2048];
    }
    // These lists borrow the reserved output rows. Truncation uses pop so
    // V8 keeps their backing storage across alternating empty/full ticks.
    trim(this.humans,taken.human.length);
    for(let i=0;i<taken.human.length;i++){const token=taken.human[i];this.humans[i]=this.references[Math.floor(token/16)-1].human[token%16];}
    command.human=this.humans.length?this.humans:undefined;
    trim(this.fires,taken.fire.length);
    for(let i=0;i<taken.fire.length;i++){const source=taken.fire[i],row=this.fireRows[i];row.id=source.id;row.button=source.button;row.pressed=source.pressed;this.fires[i]=row;}
    command.fire=this.fires.length?this.fires:undefined;
    this.cleanup();return command;
  }
  clear() { this.run('clear'); this.cleanup(); }
  get queue() {
    if (!this.handle) throw Error('input mailbox is disposed');
    const ptr = this.bridge.x.input_mailbox_inspect(this.handle);
    if (!ptr) throw Error('input mailbox inspection refused');
    return this.bridge.callResult(inspected, ptr).queue.map(row => this.references[row.identity-1].command);
  }
  dispose() { if (this.handle) { this.bridge.x.input_mailbox_drop(this.handle); this.handle = 0; this.references.length=0; } }
}

// Spawn requests belong to the connection, even when no snapshot is due.
// Keep their protocol projection at mutation points; content registration is
// cold and stable for this connection's lifetime.
const selections = new WeakMap();
export function bindSpawnSelection(actor, registry) {
  const previous = selections.get(actor);
  if (previous?.mailbox === actor.mailbox) return;
  const mailbox = actor.mailbox, row = new WireSpawnFactsMirror();
  const input = { words: WireSpawnFactsLayout.words, put: putWireSpawnFacts };
  function changed(value) {
    const pending = value.pending, auto = value.auto;
    row.pending = !!pending; row.auto = !!auto;
    row.pendingClass = pending ? registry.classIds.get(pending.className) : 0;
    row.pendingPost = pending?.post == null ? 0 : registry.postIds.get(pending.post);
    row.autoTeam = auto ? auto.team : 0;
    row.autoClass = auto ? registry.classIds.get(auto.className) : 0;
    row.autoPost = auto?.post == null ? 0 : registry.postIds.get(auto.post);
    row.valid = row.pendingClass !== undefined && row.pendingPost !== undefined && row.autoClass !== undefined && row.autoPost !== undefined;
    mailbox.bridge.callArgs(input, row); mailbox.bridge.x.input_mailbox_spawn(mailbox.handle);
  }
  const value = previous?.value ?? actor.spawn;
  actor.spawn = new Proxy(value, { set(target, key, value) {
    target[key] = value;
    if (key === 'pending' || key === 'auto') changed(target);
    return true;
  } });
  selections.set(actor, {mailbox,value}); changed(value);
}
