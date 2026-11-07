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
