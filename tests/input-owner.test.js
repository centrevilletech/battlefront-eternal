import test from 'node:test';
import assert from 'node:assert/strict';
import { InputMailbox } from './fixtures/input-mailbox.js';
import { WireCensusLayout as C } from '../src/sim/gen/wire-census.js';

// Input packets allocate at admission. Consuming their actions and cumulative
// look, including the first command, borrows the owner's reserved exchanges.
test('first and warmed mailbox consumption use retained native result buffers',()=>{
  const box=new InputMailbox(),b=box.bridge;
  try {
    for(let tick=1;tick<=120;tick++) {
      assert.equal(box.push({epoch:1,sequence:tick,tick,generation:2,down:['KeyW'],actions:['jump'],
        look:[],lookTotal:[tick*2,-tick],mouseScale:1,firstPerson:true}),true);
      b.x.wire_census_begin();
      const command=box.take(tick,2),ptr=b.x.wire_census_end();b.views();
      assert.equal(command.actions[0],'jump');assert.deepEqual(command.look,[[2,-1]]);
      assert.equal(b.f[ptr/8+C.allocations],0,`allocations at command ${tick}`);
      assert.equal(b.f[ptr/8+C.reallocations],0,`reallocations at command ${tick}`);
    }
  } finally {box.dispose();}
});

// Corrupt descriptors exercise the native boundary without a JS validator.
// A refused command must leave the same mailbox usable for the next packet.
test('owned mailbox rejects malformed list spans before decoding',async()=>{
  const {InputMailboxCallMirror,ImEntryMirror,InputMailboxCallLayout:C,ImEntryLayout:E}=await import('../src/sim/gen/input.js');
  const {BlockHeaderLayout:H}=await import('../src/sim/gen/boundary.js');
  const box=new InputMailbox(),b=box.bridge,call=new InputMailboxCallMirror();
  call.op='push';call.entry=new ImEntryMirror();call.epochMatches=true;
  try {
    for (const field of ['codes','actions','look','view','human','fire']) for (const value of [NaN,Infinity,-1,.5,0xffffffff]) {
      b.callArgs(box.input,call);b.views();
      const row=b.x.sim_calls()/8+H.words;
      b.f[row+C.entry+1+E[`${field}Length`]]=value;
      assert.equal(b.x.input_mailbox_owned_run(box.handle),0,`${field} ${value}`);
    }
    assert.equal(box.push({epoch:1,sequence:1,tick:1,generation:2,down:[],actions:[],look:[]}),true);
    assert.equal(box.take(1,2).sequence,1);
  } finally {box.dispose();}
});

// Reject before generated put() interns any client-controlled held name.
test('held input rejects unknown and oversized names without interning',()=>{
  const box=new InputMailbox(),b=box.bridge;
  const before=b.strings.list.length;
  for(let i=0;i<128;i++)for(const code of [`Unknown${i}`, 'KeyW'.repeat(1000)])
    assert.throws(()=>box.push({epoch:1,sequence:1,tick:1,generation:2,down:[code],actions:[],look:[]}),/invalid input payload/);
  assert.equal(b.strings.list.length,before);
  assert.equal(box.received,0);assert.deepEqual(box.queue,[]);
  box.dispose();
});

test('native mailbox refuses unregistered held identities',async()=>{
  const {InputMailboxCallMirror,ImEntryMirror}=await import('../src/sim/gen/input.js');
  const box=new InputMailbox(),b=box.bridge,call=new InputMailboxCallMirror();
  call.op='push';call.epochMatches=true;call.entry=new ImEntryMirror();
  Object.assign(call.entry,{sequence:1,tick:1,generation:2,codes:['Unregistered']});
  b.callArgs(box.input,call);
  const ptr=b.x.input_mailbox_owned_run(box.handle);
  assert.ok(ptr);assert.deepEqual(box.queue,[]);assert.equal(box.take(1,2).down.length,0);
  box.dispose();
});

test('disposed and stale mailboxes refuse calls before decoding address zero',()=>{
  const box=new InputMailbox();box.dispose();
  for(const call of [()=>box.take(1,2),()=>box.clear(),()=>box.queue,()=>box.push({sequence:1})])
    assert.throws(call,/disposed/);
  const stale=new InputMailbox();stale.bridge.x.input_mailbox_drop(stale.handle);
  assert.throws(()=>stale.take(1,2),/operation refused/);
  assert.throws(()=>stale.queue,/inspection refused/);stale.dispose();
});
test('mailbox admits the registry order instead of a private key-code copy',()=>{
  const codes=['Mouse0','KeyW'],box=new InputMailbox(1,4,undefined,{inputCodes:codes});
  codes.reverse();
  assert.equal(box.push({epoch:1,sequence:1,tick:1,generation:2,down:['Mouse0'],actions:[],look:[]}),true);
  assert.deepEqual(box.take(1,2).down,['Mouse0']);
  assert.throws(()=>box.push({epoch:1,sequence:2,tick:2,generation:2,down:['KeyS'],actions:[],look:[]}),/invalid input payload/);
  box.dispose();
});
