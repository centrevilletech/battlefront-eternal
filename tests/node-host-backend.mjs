// The real addon supplies every Context below. No world implementation or
// exports-shaped native substitute is installed by these content-free tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {createQualificationBackend,qualificationBackend} from '../tools/host-qualification/native.mjs';
import {NativeBackend,loadNativeSim} from '../src/sim/native-backend.js';
import {HOST_SCHEMA} from '../src/sim/host-schema.js';
import {hostSimulation} from '../src/sim/host-runtime.js';
import {BlockHeaderLayout as H} from '../src/sim/gen/boundary.js';
import {COMMAND_TABLE,KIND_TABLE,COMMAND_ECHO,KIND_BOLT} from '../src/sim/gen/index.js';
import {WorldTickInputLayout,WorldTickOutputLayout,WorldTickOutputMirror,WorldTickInputMirror,syncWorldTickInput,syncWorldTickOutput} from '../src/sim/gen/world.js';
import {WireWorldFactsLayout} from '../src/sim/gen/wire-store.js';
import {WirePublicationOutputMirror,syncWirePublicationOutput} from '../src/sim/gen/wire-publish.js';
const addon=createRequire(import.meta.url)('../.host-addon/bfe-node.node');
const metadata=JSON.parse(readFileSync(new URL('../.host-addon/target.json',import.meta.url)));
const ECHO=COMMAND_TABLE[COMMAND_ECHO];
const worldFixture=JSON.parse(readFileSync(new URL('../.host-addon/world-fixtures.json',import.meta.url)));
const worldRecord={Mirror:WorldTickOutputMirror,sync:syncWorldTickOutput};
const echo=(events=0)=>({value:41,at:{x:1,y:2,z:3},count:0,note:null,events});
const bytes=block=>new Uint8Array(block.words.buffer,block.words.byteOffset+block.base*8,block.length*8).slice();
const snapshot=block=>block.words.slice(block.base,block.base+block.length);

test('native factory owns independent contexts and reports the linked world capabilities',async()=>{
  const first=await loadNativeSim(addon,{},metadata),second=await loadNativeSim(addon);
  try {
    assert.equal(first.getBridge(),first.bridge);
    const hello=addon.handshake();
    assert.deepEqual(first.capabilities,{backend:'native',nativeWorld:hello.nativeWorld,nativePublication:hello.nativePublication});
    assert.equal(first.exports,undefined);assert.equal(first.memory,undefined);
    const a=first.bridge.strings,b=second.bridge.strings;
    const text='owner\0\ud800\udc00\udfff';
    assert.equal(a.id(text),b.id('another owner'));
    assert.equal(a.at(a.id(text)),text);assert.equal(b.at(b.id('another owner')),'another owner');
    const uncached=first.intern('\ud800 lone \udfff');assert.equal(a.at(uncached),'\ud800 lone \udfff');
    assert.throws(()=>first.prepare('world'),/owner admission unavailable/);
    first.close();assert.equal(second.bridge.command('echo',echo()).value,42);
  } finally {first.close();second.close();}
  assert.equal(first.host.liveOwners(),0);assert.equal(second.host.liveOwners(),0);
  assert.throws(()=>new NativeBackend(addon,{}, {schema:'different'}),/schema mismatch/);
  assert.throws(()=>new NativeBackend(addon,{}, {engineBuild:'different'}),/build mismatch/);
});

test('every stored kind round trips full blocks and rejects nonstored kinds explicitly',()=>{
  const backend=new NativeBackend(addon),b=backend.bridge;
  let stored=0,foreign=0;
  const empty=new Float64Array(H.words);empty[H.heap]=H.words;
  try {
    b.strings.id('');
    for(const kind of KIND_TABLE) {
      if(!kind)continue;
      if(!kind.stored) {
        assert.throws(()=>backend.exportKind(kind.code,1),/no stored fields/);
        assert.throws(()=>backend.importKind(kind.code,1,empty),/no stored fields/);
        foreign++;continue;
      }
      if(!kind.singleton)b.command('fill',{kind:kind.code,count:2});
      const block=backend.exportKind(kind.code,1),before=bytes(block),copy=snapshot(block);
      assert.equal(block.words[H.stride],kind.fullStride,kind.name);
      backend.importKind(kind.code,1,copy);
      assert.deepEqual(bytes(backend.exportKind(kind.code,1)),before,kind.name);
      // Import-only fields and sparse writes share their generated layouts.
      const partial=backend.exportKind(kind.code,2);
      assert.equal(partial.words[H.stride],kind.exportStride);
      stored++;
    }
    assert.equal(stored,25);assert.equal(foreign,11);
    assert.equal(backend.memoryDiagnostics().poisoned,false);
  } finally {backend.close();}
});

test('generated full-row readers and writers work over every native kind',()=>{
  const backend=new NativeBackend(addon),b=backend.bridge;
  try {
    b.strings.id('');
    for(const table of Object.values(b.defs))table.register({class:'synthetic definition'});
    for(const kind of KIND_TABLE) {
      if(!kind)continue;
      if(kind.stored&&!kind.singleton)b.own(kind.code);
      else if(!kind.stored) {
        // Generated defaults may name a foreign collision/body identity. Bind
        // its opaque key as cold content does; no native world is fabricated.
        const refs=b.refs[kind.code],object={id:0,index:0};
        if(refs.key){refs.objects[0]=object;refs.gens[0]=0;}else refs.bind(0,0,object);
      }
    }
    for(const kind of KIND_TABLE) {
      if(!kind?.stored)continue;
      if(!kind.singleton)b.command('fill',{kind:kind.code,count:2});
      const before=bytes(backend.exportKind(kind.code,1));
      const rows=b.read(kind.code);
      for(const row of rows){row.object.simSlot=row.slot;row.object.simGen=row.gen;}
      const objects=rows.map(row=>row.object);
      b.write(kind.code,objects,true);
      b.writeSome(kind.code,objects.slice(0,1),true);
      b.write(kind.code,objects,false);
      assert.deepEqual(bytes(backend.exportKind(kind.code,1)),before,kind.name);
    }
  } finally {backend.close();}
});

test('capacity is returned without resizing or replay and grows only at admission',()=>{
  const backend=new NativeBackend(addon,{inputWords:32,outputWords:1}),b=backend.bridge;
  try {
    const bank=backend.blocks.view,original=bank.words;
    const fault=b.command('echo',echo(2));assert.equal(fault.kind,'capacity');assert.equal(fault.required,13);assert.equal(fault.pending,true);
    assert.equal(bank.words,original);assert.equal(backend.memoryDiagnostics().lastFault,'Capacity');
    backend.endAdmission();assert.throws(()=>backend.reserve({outputWords:13}),/requires admission/);
    assert.equal(backend.retryPending(),fault);
    assert.throws(()=>b.command('fill',{kind:KIND_BOLT,count:1}),/Busy/);
    backend.beginAdmission();backend.reserve({inputWords:64,outputWords:13});
    const result=backend.retryPending();assert.equal(result,bank);assert.notEqual(result.words,original);
    assert.equal(b.decode(ECHO.result,result).value,42);
    // The first call emitted exactly two events. Delivery of its saved reply
    // must not emit another pair even when input storage grew meanwhile.
    const short=backend.events();assert.equal(short.required,21);
    backend.reserve({outputWords:21});const events=backend.retryPending();assert.equal(events.words[0],20);
    assert.equal(backend.events().length,1);
    backend.endAdmission();const words=bank.words;
    for(let i=0;i<100;i++)assert.equal(b.command('echo',echo()).value,42);
    assert.equal(bank.words,words);
  } finally {backend.close();}
});

test('input admission faults execute no command and numeric arenas keep payload bits',()=>{
  const backend=new NativeBackend(addon,{inputWords:6,outputWords:32}),b=backend.bridge;
  try {
    const fault=b.command('echo',echo(2));assert.equal(fault.arena,'input');assert.equal(fault.required,13);assert.equal(fault.pending,false);
    assert.equal(backend.memoryDiagnostics().usedWords,0);
    backend.reserve({inputWords:32});
    for(const value of [0n,0x8000000000000000n,1n,0x7ff8000000000042n,0x7ff0000000000042n,0xfff8000000000017n]) {
      const valueRow=echo();valueRow.at.x=12345;
      const input=b.callArgs(ECHO.args,valueRow),offset=input.indexOf(12345);
      assert.ok(offset>=H.words);
      new BigUint64Array(input.buffer,input.byteOffset,input.length)[offset]=value;
      const result=backend.operation(COMMAND_ECHO,input);
      assert.equal(new BigUint64Array(result.words.buffer)[offset],value);
    }
    assert.throws(()=>backend.operation(COMMAND_ECHO,new Float32Array(13)),/Float64Array/);
    assert.throws(()=>backend.importKind(KIND_BOLT,1,new Float64Array(new SharedArrayBuffer(104))),/Float64Array/);
    const input=b.callArgs(ECHO.args,echo());backend.endAdmission();const view=backend.blocks.view;
    assert.equal(backend.operation(COMMAND_ECHO,input),view);
    backend.beginAdmission();backend.enter();assert.throws(()=>backend.reserve({outputWords:64}),/requires admission/);
    assert.equal(backend.span(1),backend.fault);assert.equal(backend.fault.pending,false);
    assert.throws(()=>backend.intern('active admission'),/require admission/);backend.leave();
  } finally {backend.close();}
});

test('retirement replies survive admission retries and close invalidates the borrowed bank',()=>{
  const backend=new NativeBackend(addon,{outputWords:5}),b=backend.bridge;
  b.command('fill',{kind:KIND_BOLT,count:1});
  const required=backend.exportKind(KIND_BOLT,1).required;
  backend.reserve({outputWords:required});const full=backend.retryPending(),copied=snapshot(full);
  const slot=full.words[H.words],generation=full.words[H.words+1];
  assert.equal(b.retire(KIND_BOLT,slot,generation),true);
  b.command('fill',{kind:KIND_BOLT,count:40});
  const fault=backend.exportKind(KIND_BOLT,0);assert.equal(fault.kind,'capacity');
  backend.reserve({outputWords:fault.required});
  assert.equal(backend.retryPending().words[H.retired],1);
  assert.equal(backend.exportKind(KIND_BOLT,0).words[H.retired],0);
  const bank=backend.blocks.view,version=bank.generation;
  backend.enter();assert.equal(backend.close(),true);assert.equal(backend.close(),false);
  assert.equal(backend.memoryDiagnostics().closed,false);backend.leave();
  assert.equal(backend.memoryDiagnostics().closed,true);assert.equal(backend.memoryDiagnostics().reservedWords,0);
  assert.equal(backend.host.liveOwners(),0);assert.equal(bank.words,null);assert.ok(bank.generation>version);
  assert.ok(copied.byteLength>0);assert.throws(()=>backend.getBridge(),/closed/);
});

test('event delivery survives nested operation and store reads without losing order',()=>{
  const backend=new NativeBackend(addon),b=backend.bridge,values=[];
  try {
    b.command('echo',echo(3));
    assert.equal(b.drain({echoed(value){
      values.push(value.value);
      b.command('echo',echo());backend.exportKind(KIND_BOLT,1);
      assert.equal(b.drain({}),b.busy);
    }}),3);
    assert.equal(values.length,3);assert.equal(b.drain({}),0);
  } finally {backend.close();}
});

test('host selection rejects an unavailable advertised native target before owner acquisition',()=>{
  const hello=addon.handshake(),target='linux-x64-gnu';
  const manifest={schema:HOST_SCHEMA,engineBuild:hello.engineBuild,addonABI:hello.addonAbi,
    operationTable:hello.operationTable,targets:[{target,addonABI:hello.addonAbi}]};
  let acquired=0;
  const observed={handshake:()=>({...hello,nativeWorld:false,nativePublication:false}),get NativeHost(){acquired++;throw Error('must not acquire');}};
  const before=addon.lifecycleCounts();
  assert.throws(()=>hostSimulation(manifest,target,observed),/Native world operations unavailable/);
  assert.equal(acquired,0);assert.deepEqual(addon.lifecycleCounts(),before);
  const unsupported=hostSimulation(manifest,'linux-arm64',{handshake(){throw Error('unsupported target inspected addon');}});
  assert.equal(typeof unsupported,'function');
  assert.equal(unsupported,hostSimulation({schema:HOST_SCHEMA,targets:[]},'linux-arm64'));
  assert.throws(()=>hostSimulation(manifest,target,null),/addon missing/);
  assert.throws(()=>hostSimulation({...manifest,engineBuild:'wrong'},target,observed),/build mismatch/);
  assert.throws(()=>hostSimulation({...manifest,schema:'wrong'},target,observed),/schema mismatch/);
  assert.throws(()=>hostSimulation({...manifest,operationTable:'wrong'},target,observed),/operation table mismatch/);
});

test('real native entries refuse unadmitted ticks and sessions without poisoning the owner',()=>{
  const backend=new NativeBackend(addon),b=backend.bridge,input=Float64Array.from(worldFixture.input);
  try {
    backend.stepInit(Float64Array.from(worldFixture.setup));
    const output=b.decode(worldRecord,backend.step(input));
    assert.equal(output.accepted,false,'an empty Context must not invent an accepted tick');
    assert.deepEqual(output.clock,syncWorldTickInput(b,input,0,0,new WorldTickInputMirror()).clock);
    assert.equal(backend.pending,0);
    assert.throws(()=>backend.publish(1,new Float64Array(WireWorldFactsLayout.words)),/InvalidKind|no such publication session/);
    assert.equal(backend.pending,0);assert.equal(backend.readBank(1).length,0);
    assert.equal(b.command('echo',echo()).value,42);
    const views=backend.blocks.banks.slice();
    assert.equal(backend.close(),true);assert.equal(backend.close(),false);
    for(const view of views){assert.equal(view.words,null);assert.equal(view.length,0);}
    assert.equal(backend.host.liveOwners(),0);
    assert.throws(()=>backend.step(input),/closed/);assert.throws(()=>backend.publish(1,input),/closed/);
    assert.throws(()=>backend.readBank(1),/closed/);
  } finally {backend.close();}
});

test('native qualification and its production-worker entry refuse before gameplay import or readiness',async()=>{
  const root=await mkdtemp(join(tmpdir(),'native-host-')),engine=join(root,'engine');
  let worker;
  try {
    await mkdir(join(root,'server/src'),{recursive:true});await mkdir(engine);
    await writeFile(join(root,'package.json'),'{"type":"module"}');
    const hello=addon.handshake(),target='linux-x64-gnu';
    const manifest={schema:HOST_SCHEMA,engineBuild:hello.engineBuild,addonABI:hello.addonAbi,
      operationTable:hello.operationTable,targets:[{target,addonABI:hello.addonAbi}]};
    // The fixture supplies only the package loader. The real selection seam
    // must refuse before the provider can import any game or content module.
    const source=`import {createRequire} from 'node:module';
import {hostSimulation} from ${JSON.stringify(new URL('../src/sim/host-runtime.js',import.meta.url).href)};
const real=createRequire(import.meta.url)(${JSON.stringify(fileURLToPath(new URL('../.host-addon/bfe-node.node',import.meta.url)))});
const addon={NativeHost:real.NativeHost,handshake:()=>({...real.handshake(),nativeWorld:false,nativePublication:false})};
export async function loadHostRuntime(){const manifest=${JSON.stringify(manifest)};
return {manifest,simulationFactory:hostSimulation(manifest,${JSON.stringify(target)},addon),worldBackend:'native'};}`;
    await writeFile(join(root,'server/src/host-runtime.js'),source);
    const before=addon.lifecycleCounts();
    assert.equal(qualificationBackend,'native');
    await assert.rejects(createQualificationBackend({engine,manifest}),/Native world operations unavailable/);
    worker=new Worker(new URL('../tools/host-qualification/native.mjs',import.meta.url),{workerData:{engine}});
    const [message]=await Promise.race([once(worker,'message'),once(worker,'error').then(([error])=>{throw error;})]);
    assert.equal(message.type,'error');assert.match(message.message,/Native world operations unavailable/);
    assert.deepEqual(addon.lifecycleCounts(),before);
  } finally {await worker?.terminate();await rm(root,{recursive:true,force:true});}
});

test('a short step reply is delivered once after explicit admission growth',()=>{
  const backend=new NativeBackend(addon,{inputWords:WorldTickInputLayout.words,outputWords:H.words});
  const input=Float64Array.from(worldFixture.input);
  try {
    backend.stepInit(Float64Array.from(worldFixture.setup));
    const required=H.words+WorldTickOutputLayout.words,result=backend.step(input);
    assert.equal(result,backend.fault);assert.equal(result.required,required);assert.equal(result.pending,true);
    assert.throws(()=>backend.readBank(1),/Busy/);
    backend.endAdmission();assert.equal(backend.retryPending(),result);
    assert.throws(()=>backend.reserve({outputWords:required}),/requires admission/);
    backend.beginAdmission();backend.reserve({inputWords:64,outputWords:required});
    const output=backend.bridge.decode(worldRecord,backend.retryPending());
    assert.equal(output.accepted,false);assert.equal(backend.pending,0);
    assert.equal(backend.memoryDiagnostics().poisoned,false);
  } finally {backend.close();}
  assert.equal(backend.host.liveOwners(),0);
});

test('owned publication packets pass through the bank, admission retry, close and transfer',()=>{
  const backend=new NativeBackend(addon,{outputWords:1}),facts=new Float64Array(WireWorldFactsLayout.words);
  // This is the addon's packet-delivery fixture, not an executable world or a
  // readiness override. Production publish still calls Context::world_publish.
  backend.runPublish=(_request,out)=>backend.owner.probePacketBuffers(out,false);
  try {
    const fault=backend.publish(0,facts);
    assert.equal(fault,backend.fault);assert.equal(fault.pending,true);assert.ok(fault.required>1);
    assert.equal(backend.packets,null);
    assert.throws(()=>backend.step(new Float64Array(WorldTickInputLayout.words)),/Busy/);
    backend.endAdmission();assert.equal(backend.retryPending(),fault);
    backend.beginAdmission();backend.reserve({inputWords:16384,outputWords:fault.required});
    const block=backend.retryPending(),packets=backend.packets;
    const output=backend.bridge.decode({Mirror:WirePublicationOutputMirror,sync:syncWirePublicationOutput},block);
    const part=output.shared[0].parts[0],packet=backend.packet(part);
    assert.equal(packet.buffer,packets[part.offset]);assert.deepEqual([...packet],[0,1,255]);
    assert.equal(backend.packet(output.own[0].span).buffer,packet.buffer);
    assert.equal(backend.packet(output.records[0].payload).byteLength,2);
    assert.ok(Object.is(output.epoch,-0));
    const saved=bytes(block);backend.bridge.command('echo',echo());assert.deepEqual(bytes(block),saved);
    assert.throws(()=>backend.packet({offset:part.offset,length:part.length+1}),/Invalid native packet/);
    backend.close();assert.deepEqual([...packet],[0,1,255]);
    const moved=structuredClone(packets,{transfer:packets});
    assert.ok(packets.every(buffer=>buffer.byteLength===0));
    assert.deepEqual(moved.map(buffer=>[...new Uint8Array(buffer)]),[[0,1,255],[3,4],[5],[6]]);
  } finally {backend.close();}
});

test('native world faults clear bridge loans and poisoned owners still close once',()=>{
  const backend=new NativeBackend(addon);
  try {
    assert.throws(()=>backend.owner.probeContextPanic(),{code:'GenericFailure'});
    for(const invoke of [()=>backend.step(new Float64Array(WorldTickInputLayout.words)),
      ()=>backend.publish(0,new Float64Array(WireWorldFactsLayout.words)),()=>backend.readBank(1)]) {
      assert.throws(invoke,/Poisoned/);assert.equal(backend.pending,0);assert.equal(backend.pendingInput,null);
    }
    assert.equal(backend.close(),true);assert.equal(backend.close(),false);
    assert.equal(backend.host.liveOwners(),0);
  } finally {backend.close();}
});

test('target selection binds a fresh native owner for each battle and never retries WASM',async()=>{
  const hello=addon.handshake(),target='linux-x64-gnu';
  const manifest={schema:HOST_SCHEMA,engineBuild:hello.engineBuild,addonABI:hello.addonAbi,
    operationTable:hello.operationTable,targets:[{target,addonABI:hello.addonAbi}]};
  const before=addon.lifecycleCounts(),factory=hostSimulation(manifest,target,addon);
  assert.deepEqual(addon.lifecycleCounts(),before);
  const first=await factory(),second=await factory();
  try {assert.notEqual(first.owner,second.owner);assert.notEqual(first.bridge,second.bridge);}
  finally {first.close();second.close();}
  const failed=hostSimulation(manifest,target,{handshake:()=>hello,NativeHost:class {constructor(){throw Error('Native acquisition refused');}}});
  await assert.rejects(failed(),/Native acquisition refused/);
  assert.equal(first.host.liveOwners(),0);assert.equal(second.host.liveOwners(),0);
});

test('WorldTick uses the generated native row and releases its operation on close',()=>{
  const backend=new NativeBackend(addon);
  try {
    const row=syncWorldTickInput(backend.bridge,Float64Array.from(worldFixture.input),0,0,new WorldTickInputMirror());
    const operation=backend.world({water:null}),input=operation.input;
    const result=backend.bridge.decode(worldRecord,operation.step(row));
    assert.equal(result.accepted,false);assert.equal(operation.input,input);
    operation.finish();backend.close();assert.equal(operation.input,null);
    assert.throws(()=>operation.step(row),/closed/);
    assert.equal(backend.host.liveOwners(),0);
  } finally {backend.close();}
});

test('generated native operations use operation IDs and reserve packet bytes only at admission',()=>{
  const backend=new NativeBackend(addon,{publicationBytes:16}),b=backend.bridge;
  try {
    assert.equal(b.decode(ECHO.result,b.operation('echo',[],echo())).value,42);
    assert.throws(()=>b.command('relay',echo()),/No native operation relay/);
    assert.throws(()=>b.operation('wireHostOp',[]),/scalar count/);
    const before=backend.memoryDiagnostics().publicationReservedBytes;
    backend.reserve({publicationBytes:128*1024});
    assert.ok(backend.memoryDiagnostics().publicationReservedBytes>before);
    backend.endAdmission();assert.throws(()=>backend.reserve({publicationBytes:256*1024}),/requires admission/);
  } finally {backend.close();}
});
