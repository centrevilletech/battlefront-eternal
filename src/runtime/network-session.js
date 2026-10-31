import { EntityInputSender } from './network-entity-sender.js';
import { encodeSnapshotAckKeepalive } from './network-entity-input.js';
import { contentHash } from './content-hash.js';
import { battleParams } from './battle-query.js';

// Where a page loads the server's battle: its query (the battle's own keys
// only, src/runtime/battle-query.js), with this page's connection (the
// server, its address, the relay) and diagnostics kept.
function battleUrl(query) {
  const next=battleParams(query),current=new URLSearchParams(globalThis.location?.search);
  for(const name of ['network','server','address','port','relay','traceInput','profileRuntime'])if(current.has(name))next.set(name,current.get(name));
  return '/?'+next;
}
// Whether the page built another battle than the server plays: a value the
// server's query names (map, side, ai, seed) that the page's differs in.
// The server browser launches from the listing, which names neither the
// bots nor the seed; a bots-off server's soldiers then land on the page's
// bots ("Unsupported entity soldier body"). The match token only tells
// one launch from the next.
export function otherBattle(page, server) {
  const mine=new URLSearchParams(page);
  for(const [key,value] of battleParams(server))if(key!=='match'&&mine.get(key)!==value)return true;
  return false;
}

// One gameplay session for local and remote battles. The transport provides the
// state/events/bulk channels; every world supplies only entity records.
export class NetworkSession {
  constructor({transport, onError=()=>{}, onChat=()=>{},
    onEntityRegistry, onEntities, onGameplayEvents=()=>{}, onEntityDiagnostics=()=>{}, onInputAck=()=>{},
    onFire=()=>{}, getAim=()=>({}), playerName='', lifecycle=globalThis.window,
    rotationPollMs=1000} = {}) {
    Object.assign(this, {transport,onError,onChat,onEntityRegistry,onEntities,onGameplayEvents,onEntityDiagnostics,onInputAck,onFire,getAim,lifecycle,rotationPollMs});
    this.playerName=String(playerName || '').replace(/[\x00-\x1f\x7f]/g,'').trim().slice(0,32);
    this.channels=new Map();this.pending=new Map();this.request=0;this.stateSequence=0;
    this.entityQueue=[];this.snapshotAck=null;this.entityWaiters=[];this.closed=false;this.authenticated=false;
    this.onPageHide=()=>this.dispose();this.lifecycle?.addEventListener('pagehide',this.onPageHide);
  }
  get path() { return this.transport?.path; }
  get worker() { return this.transport?.worker; }
  async init({query=globalThis.location?.search || '', settings}={}) {
    if (!this.onEntities || !this.onEntityRegistry) throw Error('Battle requires the current game adapter');
    this.query=query;this.content=await contentHash(query);
    const ready=new Promise((resolve,reject)=>{this.resolveReady=resolve;this.rejectReady=reject;});
    this.timeout=setTimeout(()=>this.fail(Error('Battle connection timed out')),30000);
    try {
      await this.transport.connect({query,settings,content:this.content,playerName:this.playerName,channels:this.channels,
        receive:(label,raw,at)=>this.receive(label,raw,at),fail:error=>this.fail(error),status:text=>this.status(text)});
    } catch(error) { this.fail(error); }
    return ready;
  }
  // `at` is the transport's receipt time (performance.now() milliseconds)
  // when it knows better than this thread when a packet actually arrived.
  async receive(label, raw, at) {
    if (this.closed) return;
    if (raw instanceof ArrayBuffer) {
      const magic=raw.byteLength>=4?new DataView(raw).getUint32(0,true):0;
      if (label!=='state'||raw.byteLength<20||raw.byteLength>1100||(magic!==0x31534e42&&magic!==0x314f4e42)) throw Error('Invalid entity packet');
      this.entityQueue.push({bytes:new Uint8Array(raw),at:(Number.isFinite(at)?at:performance.now())/1000});
      for(const waiter of this.entityWaiters.splice(0)){clearTimeout(waiter.timer);waiter.resolve();}
      // Local delivery is reliable: never silently discard a paused world's final tick.
      if(this.entityQueue.length>(this.entityMode?4096:128))throw Error('Entity receive backlog exceeded');
      return;
    }
    if (typeof raw!=='string'||raw.length>262144) throw Error('Invalid server message');
    const message=JSON.parse(raw);
    if (['gameplay-error','version-error'].includes(message.type)) throw Error(String(message.message || 'Battle failed').slice(0,240));
    if (message.type==='entity-registry') {
      if(this.entityRegistry)throw Error('Unexpected entity registry');
      this.entityRegistry=this.onEntityRegistry(message.registry);
      this.entityInput=new EntityInputSender({registry:this.entityRegistry,send:bytes=>this.sendEntityInput(bytes),getAim:command=>this.getAim(command),onFire:(edge,command)=>this.onFire(edge,command),snapshotAck:()=>this.snapshotAck,serverTick:()=>this.transportAck?Math.floor(this.transportAck.serverTick+(performance.now()/1000-this.transportAck.receivedAt)*60):null,stateIntervalTicks:this.transport?.local?1:2});
      return;
    }
    if (message.type==='welcome') {
      // The welcome names the battle the server plays (its /info query). A
      // page that built another (the server rotated while this one loaded,
      // or its listing lagged) loads the server's instead, before any of
      // its entities lands; this battle never starts. Decided before the
      // registry check: a spectator's welcome is still the right one.
      if(typeof message.query==='string'&&message.query.length<=2048&&otherBattle(this.query,message.query)&&globalThis.location?.assign){
        clearTimeout(this.timeout);this.status('Loading the server\'s battle...');globalThis.location.assign(battleUrl(message.query));return;
      }
      if(message.snapshotMode!=='snapshot-delta-v1')throw Error('Multiplayer version mismatch. Update the desktop client and server.');
      // The registry precedes the welcome (server game-session.js); one
      // that never came was dropped on the way, not a version difference.
      if(!this.entityRegistry)throw Error('The server did not send its battle description. Try joining again.');
      this.authenticated=true;this.entityMode=this.splitInputs=true;this.epoch=message.epoch||1;this.welcome=message;
      this.ackTimer=setInterval(()=>{this.sendEntityAcknowledgement();this.sendEntityEdgeAcknowledgement();},100);
      if(message.spectator)this.status('Connected as a spectator.');
      clearTimeout(this.timeout);this.resolveReady({...message,type:'ready',shared:false});return;
    }
    if (message.type==='entity-diagnostics') {
      if(!Array.isArray(message.actors)||message.actors.length>256)throw Error('Invalid entity diagnostics');
      this.entityDiagnostics=message.actors;this.onEntityDiagnostics(message.actors);return;
    }
    if (message.type==='gameplay-events') {
      if(!Array.isArray(message.records)||message.records.length>256)throw Error('Invalid gameplay events');
      const records=message.records.map(record=>({...record,payload:Uint8Array.from(atob(record.payload),c=>c.charCodeAt(0))}));
      this.onGameplayEvents(records, message.tick??0);return;
    }
    if (message.type==='entity-edge-results') {
      if(!this.entityMode)throw Error('Unexpected entity edge results');
      this.entityInput.feedback(message);this.sendEntityEdgeAcknowledgement();return;
    }
    if(message.type==='input-ack'){this.receiveInputAck(message);return;}
    if(message.type==='reply'){
      const job=this.pending.get(message.id);if(job){clearTimeout(job.timer);this.pending.delete(message.id);message.error?job.reject(Error(message.error)):job.resolve(message);}return;
    }
    if(message.type==='ping'){this.send({type:'pong',time:message.time});return;}
    if(message.type==='chat'){
      if(typeof message.text!=='string'||message.text.length>240||typeof message.sender!=='string'||message.sender.length>80||typeof message.team!=='boolean')throw Error('Invalid chat message');
      this.onChat({text:message.text,name:message.admin===true?'':message.sender,channel:message.admin===true?'server':message.team?'team':'global'});return;
    }
    if(message.type==='match'){
      if(typeof message.query!=='string'||message.query.length>2048)throw Error('Invalid next match');
      if(!/^[a-f0-9]{32}$/.test(new URLSearchParams(message.query).get('match') || ''))throw Error('Multiplayer version mismatch. Update the server for automatic map rotation.');
      const delay=Number.isFinite(message.delayMs)?Math.min(120000,Math.max(0,message.delayMs)):5000;
      this.rotation={url:battleUrl(message.query),query:message.query,readyAt:performance.now()+delay,deadline:performance.now()+delay+120000,ready:false};
      this.status('Next battle loading...');this.scheduleRotation();return;
    }
    throw Error('Unexpected server message: '+message.type);
  }
  send(message) {
    const channel=this.channels.get('events');if(this.closed||!this.authenticated||channel?.readyState!=='open')return false;
    const value=JSON.stringify(message);if(value.length>32768||channel.bufferedAmount>65536){this.fail(Error('Network send backlog exceeded'));return false;}channel.send(value);return true;
  }
  sendState(message) {
    const channel=this.channels.get('state');if(this.closed||!this.authenticated||channel?.readyState!=='open'||channel.bufferedAmount>32768)return false;
    const value=JSON.stringify({...message,sequence:++this.stateSequence});if(value.length>32768)return false;channel.send(value);return true;
  }
  receiveInputAck(message) {
    for(const key of ['epoch','serverTick','generation','ack','ackTick','ackActions'])if(!Number.isSafeInteger(message[key])||message[key]<0)throw Error('Invalid input acknowledgement');
    if(message.dropped!==undefined&&(!Number.isSafeInteger(message.dropped)||message.dropped<0))throw Error('Invalid input acknowledgement');
    if(message.epoch!==1 || message.ackTick>message.serverTick)throw Error('Invalid input acknowledgement');
    const previous=this.transportAck;
    if(previous && (message.serverTick<previous.serverTick || message.ack<previous.ack))return;
    this.transportAck={...message,receivedAt:performance.now()/1000};
    if(this.entityInput?.life===`${message.epoch}:${message.generation}`)this.entityInput.ack(message.ack);
    // This is transport feedback, never permission to discard replay commands.
    this.onInputAck(this.transportAck);
  }
  // Entity packets are applied by the frame loop, not in the channel
  // message handler: a whole publication decoding and mutating the world
  // between frames made arrival processing part of the frame hitch it was
  // being measured with. Receipt times travel with each packet so the
  // arrival clocks still see when it actually landed.
  waitForEntities() {
    if(this.closed)return Promise.reject(Error('Battle closed'));
    if(this.entityQueue.length)return Promise.resolve();
    return new Promise((resolve,reject)=>{const waiter={resolve,reject};waiter.timer=setTimeout(()=>{this.entityWaiters=this.entityWaiters.filter(w=>w!==waiter);reject(Error('Initial entity state timed out'));},30000);this.entityWaiters.push(waiter);});
  }
  drainEntities() {
    if(!this.entityQueue.length||this.closed)return;
    const queue=this.entityQueue;this.entityQueue=[];
    for(const {bytes,at} of queue){if(this.closed)return;try{this.onEntities(bytes,at);}catch(error){this.fail(error);return;}}
  }
  sendEntityInput(bytes) {
    if(this.welcome?.spectator)return false;
    const channel=this.channels.get('state');if(this.closed||!this.authenticated||channel?.readyState!=='open'||channel.bufferedAmount>4096)return false;
    channel.send(bytes);this.lastInputSentAt=performance.now();return true;
  }
  sendEntityEdgeAcknowledgement(){
    const input=this.entityInput;if(!this.entityMode||!input?.resultAcks.length)return false;
    const sent=this.sendState({type:'entity-edge-ack',epoch:input.edges.life.epoch,generation:input.edges.life.generation,ids:input.resultAcks});
    // The server repeats outcomes until it receives this ACK. A lost ACK
    // therefore repopulates the list on feedback; failed admission keeps it.
    if(sent)input.resultAcks=[];
    return sent;
  }
  // The newest complete snapshot this client holds. Input datagrams carry
  // it from here; the baseline never moves backward or across epochs.
  setSnapshotAck(ack) {
    if(!this.entityMode || !Number.isSafeInteger(ack?.epoch)||!Number.isSafeInteger(ack?.tick)||ack.epoch<1||ack.tick<0)throw Error('Invalid snapshot acknowledgement');
    if(this.snapshotAck && (ack.epoch<this.snapshotAck.epoch || ack.epoch===this.snapshotAck.epoch&&ack.tick<=this.snapshotAck.tick))return;
    this.snapshotAck={epoch:ack.epoch,tick:ack.tick};
    this.sendEntityAcknowledgement();
  }
  // With no recent input to carry the ack (spawn screen, spectator), send
  // the 10-byte keepalive instead. Spectators send no input but still ack.
  sendEntityAcknowledgement() {
    const now=performance.now();
    if(!this.entityMode||!this.snapshotAck||now-(this.lastEntityAckAt??-Infinity)<100||now-(this.lastInputSentAt??-Infinity)<100)return false;
    const channel=this.channels.get('state');if(this.closed||!this.authenticated||channel?.readyState!=='open'||channel.bufferedAmount>32768)return false;
    channel.send(encodeSnapshotAckKeepalive(this.snapshotAck));this.lastEntityAckAt=performance.now();return true;
  }
  call(method, args = {}) { if (method === 'suspend' || method === 'lockstep') { this.entityInput?.clearPending(); } if (this.closed || this.pending.size >= 32) return Promise.reject(Error('Network host unavailable')); const id = ++this.request; return new Promise((resolve, reject) => { const timer = setTimeout(() => { this.pending.delete(id); reject(Error(`Server request timed out: ${method}`)); }, 10000); this.pending.set(id, { resolve, reject, timer }); if (!this.send({ type: 'call', id, method, args })) { clearTimeout(timer); this.pending.delete(id); reject(Error('Server is not connected')); } }); }
  chat(message) { return this.send({ type: 'chat', text: String(message.text || '').slice(0, 240), team: message.channel === 'team' }); }
  input(command) { return this.inputs([command]); }
  inputs(commands) { return !this.welcome?.spectator && !!this.entityInput && this.entityInput.send(commands); }
  inputAcknowledged(command) { this.input(command); return this.call('inspect'); }
  settings(values) { if(this.transport.local)this.call('settings',{values}).catch(error=>this.fail(error)); }
  poll() { return 0; }
  resync() { return this.call('resync'); }
  status(text, failed = false) {
    const rotating = this.rotation && !this.rotation.cancelled && !this.rotation.ready;
    if(rotating){failed=Boolean(this.rotation.warned);text=failed?'The next battle is taking too long to start. Still retrying; you can reconnect or return to the main menu.':'Next battle loading...';}
    const spectator = this.welcome?.spectator && !failed && !this.closed && !rotating;
    if(spectator)text = 'Cannot join as a player: game data differs from the server. Update the game on both ends; if it persists, re-prepare game data. ' + text.replace(/^Spectating \(content differs\) · /, '');
    this.statusText = text;
    if (typeof document === 'undefined') return;
    if (!this.statusElement) {
      this.statusElement = document.createElement('div');
      this.statusElement.setAttribute('role', 'status');
      document.body.append(this.statusElement);
    }
    this.statusElement.style.cssText = spectator
      ? 'position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:50;background:#06101af2;color:#dbeaff;padding:24px;font:16px/1.5 sans-serif;width:calc(100% - 64px);max-width:560px;box-sizing:border-box;pointer-events:auto'
      : 'position:fixed;right:12px;top:10px;z-index:50;background:#06101acc;color:#dbeaff;padding:7px 10px;font:12px sans-serif;max-width:400px;pointer-events:none';
    this.statusElement.replaceChildren(document.createTextNode(text));
    if (failed || spectator) {
      this.statusElement.style.pointerEvents = 'auto';
      const retry = document.createElement('button'); retry.textContent = 'Reconnect'; retry.style.marginLeft = '10px'; retry.onclick = () => this.rotation ? location.assign(this.rotation.url) : location.reload();
      const leave = document.createElement('button'); leave.textContent = 'Main menu'; leave.style.marginLeft = '5px'; leave.onclick = () => location.assign('/?screen=main');
      this.statusElement.append(retry, leave);
    }
  }
  get rotationReady() { return this.rotation?.ready === true && !this.rotation.cancelled; }
  proceed() { if (!this.rotationReady) return false; this.scheduleRotation(); return true; }
  scheduleRotation() {
    const rotation = this.rotation; if (!rotation || rotation.cancelled) return;
    clearTimeout(this.rotationTimer);
    if(rotation.ready){
      if(!rotation.navigated){rotation.navigated=true;globalThis.location?.assign(rotation.url);}
      return;
    }
    this.rotationTimer = setTimeout(async () => {
      let ready=false;
      try { ready=await this.transport.matchReady(rotation.query); } catch(error) { rotation.lastError=String(error?.message || error).slice(0,240); }
      if(this.rotation!==rotation || rotation.cancelled)return;
      if(ready){rotation.ready=true;this.status('Next battle ready.');}
      else {
        rotation.failures=(rotation.failures || 0)+1;
        rotation.retryMs=Math.min(30000,this.rotationPollMs*2**Math.min(rotation.failures-1,5));
        rotation.readyAt=performance.now()+rotation.retryMs;
        if(!rotation.warned && performance.now()>rotation.deadline){rotation.warned=true;this.status('The next battle is taking too long to start. Still retrying; you can reconnect or return to the main menu.',true);}
      }
      this.scheduleRotation();
    }, Math.max(0, rotation.readyAt - performance.now()));
  }
  fail(error) {
    if (this.closed) return;
    // The server closing this battle for its next one is not a failure.
    if (this.rotation) { this.dispose(error, { preserveRotation: true }); this.status('Next battle loading...'); return; }
    this.dispose(error); this.status(error.message, true); this.onError(error);
  }
  dispose(error=Error('Disconnected from server'), { preserveRotation=false }={}) {
    if(!preserveRotation){clearTimeout(this.rotationTimer);if(this.rotation)this.rotation.cancelled=true;this.transport?.cancelMatchProbe?.();this.lifecycle?.removeEventListener('pagehide',this.onPageHide);}
    if(this.closed)return;this.closed=true;
    clearTimeout(this.timeout);clearInterval(this.ackTimer);
    this.statusElement?.remove();this.statusElement=null;this.rejectReady?.(error);this.transport?.dispose();
    for(const waiter of this.entityWaiters.splice(0)){clearTimeout(waiter.timer);waiter.reject(error);}
    for(const job of this.pending.values()){clearTimeout(job.timer);job.reject(error);}this.pending.clear();this.entityQueue=[];
  }
}
