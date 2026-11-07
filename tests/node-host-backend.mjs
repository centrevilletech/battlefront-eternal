// The real addon supplies every Context below. No world implementation or
// exports-shaped native substitute is installed by these content-free tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {NativeBackend,loadNativeSim} from '../src/sim/native-backend.js';
import {HOST_SCHEMA} from '../src/sim/host-schema.js';
import {hostSimulation} from '../src/sim/host-runtime.js';
import {BlockHeaderLayout as H} from '../src/sim/gen/boundary.js';
import {COMMAND_TABLE,KIND_TABLE,COMMAND_ECHO,KIND_BOLT} from '../src/sim/gen/index.js';
const addon=createRequire(import.meta.url)('../.host-addon/bfe-node.node');
const metadata=JSON.parse(readFileSync(new URL('../.host-addon/target.json',import.meta.url)));
const ECHO=COMMAND_TABLE[COMMAND_ECHO];
const echo=(events=0)=>({value:41,at:{x:1,y:2,z:3},count:0,note:null,events});
const bytes=block=>new Uint8Array(block.words.buffer,block.words.byteOffset+block.base*8,block.length*8).slice();
const snapshot=block=>block.words.slice(block.base,block.base+block.length);

test('native factory owns independent contexts and keeps world selection unavailable',async()=>{
  const first=await loadNativeSim(addon,{},metadata),second=await loadNativeSim(addon);
  try {
    assert.equal(first.getBridge(),first.bridge);
    assert.deepEqual(first.capabilities,{backend:'native',nativeWorld:false,nativePublication:false});
    assert.equal(first.exports,undefined);assert.equal(first.memory,undefined);
    const a=first.bridge.strings,b=second.bridge.strings;
    const text='owner\0\ud800\udc00\udfff';
    assert.equal(a.id(text),b.id('another owner'));
    assert.equal(a.at(a.id(text)),text);assert.equal(b.at(b.id('another owner')),'another owner');
    const uncached=first.intern('\ud800 lone \udfff');assert.equal(a.at(uncached),'\ud800 lone \udfff');
    assert.throws(()=>first.prepare('world'),/world operations unavailable/);
    assert.throws(()=>hostSimulation({schema:HOST_SCHEMA,worldBackend:'native'},metadata,addon),/unavailable/);
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
    backend.beginAdmission();backend.enter();assert.throws(()=>backend.reserve({outputWords:64}),/requires admission/);backend.leave();
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
