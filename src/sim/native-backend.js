// One Context owns the simulation banks. The linked simulation advertises its
// available entries; cold game attachments require their own owner admission.
import {NativeBlocks} from './block-view.js';
import {BlockBridge, Heap, StringTable} from './bridge-core.js';
import {BlockHeaderLayout as H} from './gen/boundary.js';
import {COMMAND_TABLE, EVENT_TABLE, KIND_TABLE, OPERATION_TABLE} from './gen/index.js';
import {HOST_SCHEMA} from './host-schema.js';
import {WorldTickInputLayout,WorldTickOutputLayout,WorldTickSetupLayout,putWorldTickInput,putWorldTickSetup} from './gen/world.js';

const OPERATION = 1, EXPORT = 2, EVENTS = 3, STEP = 4, PUBLISH = 5, READ = 6;
const MAX_WORDS = 4 * 1024 * 1024;
const MAX_PUBLICATION_BYTES=64*1024*1024,NO_SCALARS=Object.freeze([]);
const unavailable = () => { throw Error('Native world operations unavailable'); };
function capacity(words,maximum=MAX_WORDS) {
  if (!Number.isSafeInteger(words) || words < 0 || words > maximum) throw RangeError('Invalid native arena capacity');
  return words;
}

// Selection validates metadata before acquiring a Context. An advertised
// target must match its bundle even when its world bodies are unavailable.
export function nativeIdentity(addon, identity = {}) {
  const hello=addon.handshake();
  if (hello.addonAbi!==2 || hello.storeAbi!==1 || hello.napiMinimum!==8 ||
      identity.addonABI!==undefined && hello.addonAbi!==identity.addonABI ||
      identity.storeABI!==undefined && hello.storeAbi!==identity.storeABI ||
      identity.napiMinimum!==undefined && hello.napiMinimum!==identity.napiMinimum) throw Error('Native host ABI mismatch');
  if (hello.schema!==HOST_SCHEMA || identity.schema!==undefined && identity.schema!==HOST_SCHEMA) throw Error('Native host schema mismatch');
  if (identity.engineBuild!==undefined && hello.engineBuild!==identity.engineBuild) throw Error('Native host build mismatch');
  if (identity.operationTable!==undefined && hello.operationTable!==identity.operationTable) throw Error('Native operation table mismatch');
  return hello;
}

export class NativeBackend {
  constructor(addon, reserve = {}, identity = {}) {
    // Reject mismatched layouts before acquiring a Context. A factory error
    // stops startup; it never changes the selected provider.
    const hello = nativeIdentity(addon,identity);
    // Arena support is independent of world readiness. Do not infer a usable
    // world from an addon target or from methods this provider has not bound.
    this.capabilities = Object.freeze({backend:'native', nativeWorld:hello.nativeWorld===true, nativePublication:hello.nativePublication===true});
    this.identity = Object.freeze(hello);
    this.closed = this.closing = false; this.active = 0; this.admitting = true;
    this.pending = 0; this.pendingInput = null; this.first = this.second = 0; this.inputLength = 0;
    // Capacity is a borrowed outcome, like a BlockView. Input faults have
    // consumed nothing; pending output faults must be delivered, not rerun.
    this.fault = {ok:false, kind:'capacity', arena:'input', required:0, pending:false};
    this.input = new Float64Array(capacity(reserve.inputWords ?? 8192));
    this.inputViews = new Map(); this.blocks = new NativeBlocks(capacity(reserve.outputWords ?? 8192));
    this.packets=null; this.stepOutput=this.blocks.banks[STEP-1].words.subarray(H.words);
    this.publicationRequest={session:0,words:null};
    this.worldOperations=new Set();
    this.publicationBytes=capacity(reserve.publicationBytes??65536,MAX_PUBLICATION_BYTES);
    this.host = new addon.NativeHost(); this.owner = null;
    try {
      this.owner = this.host.create({inputWords:this.input.length, outputWords:this.blocks.view.words.length,publicationBytes:this.publicationBytes});
      // Bind once. Neither row loops nor repeated operations inspect addon
      // methods or choose a backend; Rust receives one complete numeric block.
      this.run = this.owner.operation.bind(this.owner);
      this.export = this.owner.exportKind.bind(this.owner);
      this.takeEvents = this.owner.events.bind(this.owner);
      // These entries exist even when Context returns Unavailable. Capability
      // flags describe real bodies, independently of the method shape.
      this.initStep=this.owner.stepInit.bind(this.owner);
      this.runStep=this.owner.step.bind(this.owner);
      this.runPublish=this.owner.publish.bind(this.owner);
      this.runRead=this.owner.readBank.bind(this.owner);
      this.bridge = new NativeBridge(this);
      if(this.input.length>=WorldTickInputLayout.words)this.span(WorldTickInputLayout.words);
    } catch (error) { this.owner?.close(); this.blocks.close(); throw error; }
  }
  getBridge(options = {}) { this.open();return this.bridge.identify(options); }
  readBlock(block) {this.open();return this.blocks.readBlock(block);}
  beginAdmission() {
    this.open(); if (this.active) throw Error('Native owner busy');
    this.admitting = true;
  }
  endAdmission() { this.open(); this.admitting = false; }
  // Reservation is a caller decision at admission. A short reply is retained
  // by the addon; reserve never invokes it and retry never replays its work.
  reserve({inputWords=this.input.length, outputWords=this.blocks.view.words.length,publicationBytes=this.publicationBytes} = {}) {
    this.open();
    if (!this.admitting || this.active) throw Error('Native arena growth requires admission');
    capacity(inputWords); capacity(outputWords);capacity(publicationBytes,MAX_PUBLICATION_BYTES);
    this.owner.reserve({inputWords,outputWords,publicationBytes});this.publicationBytes=Math.max(this.publicationBytes,publicationBytes);
    if (inputWords > this.input.length) {
      const input = new Float64Array(inputWords); input.set(this.input);
      this.input = input;
      for(const length of this.inputViews.keys()) this.inputViews.set(length,input.subarray(0,length));
      this.bridge.rows = new Float64Array(inputWords);
    }
    this.blocks.reserve(outputWords);
    this.stepOutput=this.blocks.banks[STEP-1].words.subarray(H.words);
  }
  // A retained short reply locks mutations on both sides of the boundary.
  // Checking here also protects its input bytes from the next encoder.
  open() { if (this.closing) throw Error('Native owner closed'); }
  idle() { this.open(); if (this.pending) throw Error('Busy: pending native result'); }
  short(arena, required) {
    this.fault.arena = arena; this.fault.required = required; this.fault.pending = this.pending !== 0;
    return this.fault;
  }
  // Exact typed views are cached during admission. Sealed operations may use
  // only those shapes, even when the larger backing already has enough room.
  span(length) {
    if (length > this.input.length) return this.short('input', length);
    let view = this.inputViews.get(length);
    if (!view) {
      if (!this.admitting || this.active) return this.short('input', length);
      view = this.input.subarray(0, length); this.inputViews.set(length, view);
    }
    return view;
  }
  // Block-relative heaps use the same generated header as WASM. Subarray
  // shapes are admitted once; subsequent transfers reuse their exact extents.
  arguments(rows, words, heap) {
    this.idle();
    const length = H.words + words + heap.length, block = this.span(length);
    if (block === this.fault) return block;
    block[H.rows] = 1; block[H.stride] = words; block[H.heap] = H.words + words;
    block[H.heapWords] = heap.length; block[H.retired] = 0;
    for (let i=0;i<words;i++) block[H.words+i] = rows[i];
    for (let i=0;i<heap.length;i++) block[H.words+words+i] = heap[i];
    this.inputLength = length;
    return block;
  }
  operation(code, block = null) {
    this.idle();
    if (block) {
      const words = block.words ?? block, base = block.base ?? 0, length = block.length;
      this.validate(words,base,length);
      const target = this.span(length); if (target === this.fault) return target;
      // Typed-array copies preserve arbitrary IEEE payload bits. Never turn
      // an output bank into JS Numbers merely to feed another native call.
      target.set(base===0&&length===words.length ? words : words.subarray(base, base+length)); this.inputLength = length;
    }
    // Keep the original view across input-bank growth: the retained native
    // request compares argument bits, including signed zero and NaN payloads.
    this.pending = OPERATION; this.first = code;
    this.pendingInput = this.inputViews.get(this.inputLength);
    return this.deliver();
  }
  exportKind(kind, mode = 0) {
    this.idle(); this.pending = EXPORT; this.first = kind; this.second = mode;
    return this.deliver();
  }
  events() { this.idle(); this.pending = EVENTS; return this.deliver(); }
  stepInit(setup) {
    this.idle();this.validate(setup,0,setup.length);
    return this.initStep(setup);
  }
  // World inputs are generated rows, not command blocks. Preserve the exact
  // request while a short output is retained so retry cannot repeat a tick.
  step(input) {
    this.idle();this.validate(input,0,input.length);
    const row=this.span(input.length);if(row===this.fault)return row;
    row.set(input);this.pendingInput=row;this.pending=STEP;return this.deliver();
  }
  // The numeric result is borrowed; packets owns exact ArrayBuffers. A caller
  // retains that array before the next publication, then transfers it freely.
  publish(session,facts) {
    this.idle();this.validate(facts,0,facts.length);
    const row=this.span(facts.length);if(row===this.fault)return row;
    row.set(facts);this.pendingInput=row;this.first=session;this.pending=PUBLISH;
    this.publicationRequest.session=session;this.publicationRequest.words=row;
    return this.deliver();
  }
  // Bank IDs select typed owner publications, never native addresses. Reads
  // have a separate loan so store/event observers cannot replace these words.
  readBank(bank) {this.idle();this.first=bank;this.pending=READ;return this.deliver();}
  // The addon rewrites WireSpan.offset to an index in its owned packet array.
  // Returning a view does not copy the packet again; its buffer can transfer
  // or outlive this owner without borrowing the encoder's next publication.
  packet(span) {
    this.open();const packet=this.packets?.[span.offset];
    if(!(packet instanceof ArrayBuffer)||packet.byteLength!==span.length)throw Error('Invalid native packet span');
    return new Uint8Array(packet);
  }
  deliver() {
    const bank=this.blocks.banks[this.pending-1],output=bank.words;
    // The owner leaves output untouched when it returns a required size.
    // Do not publish the previous bank as if this invocation had succeeded.
    let written;
    try {
      if (this.pending === OPERATION) written = this.run(this.first, this.pendingInput, output);
      else if (this.pending === EXPORT) written = this.export(this.first, this.second, output);
      else if(this.pending===EVENTS) written = this.takeEvents(output);
      else if(this.pending===STEP) {
        written=this.runStep(this.pendingInput,this.stepOutput);
        if(written>this.stepOutput.length)return this.short('output',written+H.words);
        if(written!==WorldTickOutputLayout.words)throw Error('Invalid native world result extent');
        // BlockBridge decodes every generated result through one header.
        output[H.rows]=1;output[H.stride]=WorldTickOutputLayout.words;
        output[H.heap]=H.words+written;output[H.heapWords]=0;output[H.retired]=0;
        written+=H.words;
      } else if(this.pending===PUBLISH) {
        const reply=this.runPublish(this.publicationRequest,output);
        written=reply.words;
        if(written<=output.length)this.packets=reply.packets;
      } else {
        written=this.runRead(this.first,output);
      }
    } catch (error) { this.pending = 0; this.pendingInput=null;this.publicationRequest.words=null;throw error; }
    if (written > output.length) return this.short('output', written);
    this.pending = 0; this.pendingInput = null;
    this.publicationRequest.words=null;
    return this.blocks.publish(written,bank);
  }
  // Replies, including completed steps, are retained. Delivery waits for
  // admission, so a retry can never repeat a completed tick or publication.
  retryPending() {
    this.open();
    if (!this.pending) throw Error('No pending native result');
    if (!this.admitting || this.active) return this.short('output', this.fault.required);
    return this.deliver();
  }
  // The addon validates again using engine metadata. This host check keeps
  // explicit section views from becoming a virtual linear-memory interface.
  validate(words,base,length) {
    if (!(words instanceof Float64Array) || !(words.buffer instanceof ArrayBuffer)) throw TypeError('Expected ordinary Float64Array');
    if (!Number.isSafeInteger(base) || !Number.isSafeInteger(length) || base<0 || length<0 || base+length>words.length) throw RangeError('Invalid native input extent');
  }
  // Rust validates the generated stride, flags, heap and slot bounds before
  // applying these cold content rows. There is no JS simulation decision here.
  importKind(kind, flags, block) {
    this.idle();
    const words = block.words ?? block, base = block.base ?? 0;
    this.validate(words,base,block.length);
    this.owner.importKind(kind, flags, (base===0&&block.length===words.length ? words : words.subarray(base, base+block.length)));
  }
  retire(kind, slot, generation) { this.idle(); return this.owner.retire(kind,slot,generation); }
  // IDs belong to this Context. New text is cold admission; cached IDs keep
  // repeated generated encoders from allocating or entering this method.
  intern(text) {
    this.idle();
    if (!this.admitting || this.active) throw Error('New native strings require admission');
    const units = new Array(text.length);
    for(let i=0;i<text.length;i++) units[i] = text.charCodeAt(i);
    return this.owner.intern(units);
  }
  // Cache callers retain text. Code units preserve both lone surrogates and
  // embedded NULs; converting through a Rust UTF-8 String would change them.
  string(id) {
    this.open(); const units = this.owner.string(id); let text = '';
    for(let i=0;i<units.length;i++) text += String.fromCharCode(units[i]);
    return text;
  }
  // These are actual owner counters, available even after close. The addon
  // deliberately excludes Context allocations it cannot account for yet.
  memoryDiagnostics() { return this.owner.diagnostics(); }
  prepare(category) {
    this.open();if(!this.capabilities.nativeWorld)return unavailable();
    throw Error(`Native owner admission unavailable: ${category}`);
  }
  world(phase) {
    this.open();if(!this.capabilities.nativeWorld)return unavailable();
    const operation=new NativeWorldOperation(this,phase);this.worldOperations.add(operation);return operation;
  }
  publisher() { this.open(); return unavailable(); }
  enter() { this.open(); this.active++; }
  leave() { if (--this.active === 0 && this.closing) this.release(); }
  // Observer spans share the factory's close protocol with WASM. The native
  // owner and borrowed views are invalidated together after the last span.
  close() {
    if (this.closing) return false;
    this.closing = true; if (!this.active) this.release(); return true;
  }
  release() {
    try {for(const operation of this.worldOperations)operation.close();this.owner.close();}
    finally {
      this.closed = true; this.pending = 0; this.pendingInput = null; this.inputViews.clear(); this.input = null;
      this.packets=null;this.stepOutput=null;
      this.publicationRequest.words=null;
      this.worldOperations.clear();
      this.blocks.close(); this.bridge.closed = true;
      this.bridge.rows = null; this.bridge.heap.clear();
      this.bridge.strings.ids.clear(); this.bridge.strings.list.length = 0;
      this.bridge.refs.length = 0; this.bridge.defs = {};
    }
  }
}

// WorldTick supplies only its generated facts. Context owns input ingress and
// phase completion; native observers take typed banks after the call returns.
class NativeWorldOperation {
  constructor(backend,{water}) {
    this.backend=backend;this.input=new Float64Array(WorldTickInputLayout.words);
    const setup=new Float64Array(WorldTickSetupLayout.words);
    putWorldTickSetup(backend.bridge,{water:Boolean(water),waterTime:water?.simulationTime??0,
      waterHeight:water?.height??0,waterBob:Boolean(water?.bobbing)},setup,0,backend.bridge.heap);
    backend.stepInit(setup);
  }
  step(row) {
    if(!this.backend)throw Error('Native world operation closed');
    putWorldTickInput(this.backend.bridge,row,this.input,0,this.backend.bridge.heap);
    return this.backend.step(this.input);
  }
  // No foreign phase loan is opened: World::step finishes its owned phases.
  finish() {if(!this.backend)throw Error('Native world operation closed');this.backend.open();}
  close() {
    if(!this.backend)return false;
    this.backend.worldOperations.delete(this);this.backend=null;this.input=null;return true;
  }
}

// The generated context is shared with WASM; this bridge only packs rows and
// resolves owner-local identities. It exposes no exports or virtual memory.
export class NativeBridge extends BlockBridge {
  constructor(backend) {
    super(backend.blocks); this.backend = backend; this.closed = false;
    // Reuse the same definition/reference tables and string cache as WASM.
    // Native rows resolve local keys, never individual property FFI calls.
    this.strings = new StringTable(this); this.heap = new Heap();
    this.rows = new Float64Array(backend.input.length);
    this.draining = false;this.busy = Object.freeze({ok:false,kind:'busy'});
    this.commandCodes = new Map(COMMAND_TABLE.filter(Boolean).map(c=>[c.name,c.code]));
    this.operationCodes = new Map(OPERATION_TABLE.filter(Boolean).map(o=>[o.name,o.code]));
  }
  callArgs(record, value) {
    this.backend.idle();
    if (record.words > this.rows.length) {
      if (!this.backend.admitting) return this.backend.short('input',H.words+record.words);
      this.rows = new Float64Array(this.backend.input.length);
      if (record.words > this.rows.length) return this.backend.short('input',H.words+record.words);
    }
    this.heap.clear(); record.put(this,value,this.rows,0,this.heap);
    return this.backend.arguments(this.rows,record.words,this.heap.words);
  }
  command(name, args, into = null) {
    const command = COMMAND_TABLE[typeof name === 'number' ? name : this.commandCodes.get(name)];
    if (!command) throw Error(`No native command ${name}`);
    const result = this.operation(command.name,NO_SCALARS,args);
    return result === this.backend.fault ? result : command.result ? this.decode(command.result,result,into) : undefined;
  }
  // Generated operation IDs differ from command IDs. The scalar prefix and
  // record share one relative heap; a call crosses N-API only after packing.
  operation(name, scalars, value = null) {
    this.backend.idle();
    const operation=OPERATION_TABLE[typeof name==='number'?name:this.operationCodes.get(name)];
    if(!operation)throw Error(`No native operation ${name}`);
    if(scalars.length!==operation.scalars.length)throw RangeError('Invalid native scalar count');
    const words=scalars.length+(operation.record?.words??0);
    if(words>this.rows.length)return this.backend.short('input',H.words+words);
    this.heap.clear();
    for(let i=0;i<scalars.length;i++)this.rows[i]=scalars[i];
    if(operation.record)operation.record.put(this,value,this.rows,scalars.length,this.heap);
    const input=this.backend.arguments(this.rows,words,this.heap.words);
    return input===this.backend.fault?input:this.backend.operation(operation.code);
  }
  // Raw blocks let cold content producers use the full generated layout. The
  // following convenience methods use those very same put/sync functions.
  write(kind, objects, full = false) { return this.importRows(kind,objects,full,false); }
  writeSome(kind, objects, full = false) { return this.importRows(kind,objects,full,true); }
  importRows(kind, objects, full, some) {
    this.backend.idle();
    const k = KIND_TABLE[kind], stride = full ? k.fullStride : k.importStride, count = objects.length;
    if (count*stride > this.rows.length) {
      if (!this.backend.admitting || count*stride > this.backend.input.length) return this.backend.short('input',H.words+count*stride);
      this.rows = new Float64Array(this.backend.input.length);
    }
    // Both row variants retain the generated slot/generation prefix. Heap
    // offsets stay relative to this block when several rows share lists.
    this.heap.clear(); const put = full ? k.putFull : k.put, rows = this.rows;
    for(let i=0;i<count;i++) {
      const at=i*stride, object=objects[i];
      if(k.singleton){rows[at]=0;rows[at+1]=0;}else this.refs[kind].put(object,rows,at);
      put(this,object,rows,at,this.heap);
    }
    const input = this.backend.arguments(rows,count*stride,this.heap.words);
    if(input===this.backend.fault)return input;
    input[H.rows]=count;input[H.stride]=stride;
    this.backend.importKind(kind,(full?1:0)|(some?2:0),input);
  }
  // One export copies the entire kind publication. All subsequent row and
  // identity reads are ordinary typed-array accesses inside this bank.
  exported(kind, mode, visit) {
    const block=this.backend.exportKind(kind,mode);if(block===this.backend.fault)return block;
    const f=block.words,rows=f[H.rows],stride=f[H.stride],heap=f[H.heap],retired=f[H.retired];
    for(let i=0;i<rows;i++){const at=H.words+i*stride;visit(f,at,heap,f[at],f[at+1],i<retired);}
    return rows;
  }
  // Mirrors keep their object identity. Retired rows are decoded into the
  // old generation before its pool can reuse the slot, as in the WASM bridge.
  sync(kind, target=null) {
    const k=KIND_TABLE[kind],pool=this.refs[kind];
    return this.exported(kind,0,(f,at,h,slot,gen,retired)=>k.sync(this,f,at,h,k.singleton?target:retired?pool.retire(slot,gen):pool.at(slot,gen)));
  }
  // Diagnostic full reads return owned objects. The steady mirror barrier
  // above instead updates the retained pool with generated sync functions.
  read(kind, make=()=>new KIND_TABLE[kind].Mirror()) {
    const out=[],k=KIND_TABLE[kind];
    const result=this.exported(kind,1,(f,at,h,slot,gen)=>out.push({slot,gen,object:k.syncFull(this,f,at,h,make())}));
    return result===this.backend.fault?result:out;
  }
  // Event storage is separate from operation/store replies. Recursive drains
  // defer delivery, leaving any newly queued native events intact for later.
  drain(handlers) {
    if(this.draining)return this.busy;
    const block=this.backend.events();if(block===this.backend.fault)return block;
    this.draining=true;
    try {
    const f=block.words,end=1+f[0];let count=0;
    for(let at=1;at<end;at+=2+f[at+1]) {
      const event=EVENT_TABLE[f[at]],record=event.record;
      const value=record.sync(this,f,at+2,at+2+record.words,new record.Mirror());
      handlers[event.name]?.(value);count++;
    }
    return count;
    } finally {this.draining=false;}
  }
  retire(kind,slot,generation) { return this.backend.retire(kind,slot,generation); }
  check(kind,target=null) {
    const k=KIND_TABLE[kind],pool=this.refs[kind],found=[];
    const result=this.exported(kind,2,(f,at,h,slot,gen,retired)=>{
      if(retired)return;
      const path=k.check(this,f,at,h,k.singleton?target:pool.at(slot,gen));
      if(path)found.push(`${k.name}${k.singleton?'':`[${slot}]`}${path}`);
    });
    return result===this.backend.fault?result:found;
  }
  world(...args) { return this.backend.world(...args); }
  publisher(...args) { return this.backend.publisher(...args); }
  close() { return this.backend.close(); }
}

// A factory invocation acquires one owner. Host selection has already checked
// readiness; cold probe callers can still exercise an unavailable Context.
export async function loadNativeSim(addon, reserve, identity) {
  return new NativeBackend(addon,reserve,identity);
}
