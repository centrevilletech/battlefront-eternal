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
      assert.equal(box.push({epoch:1,sequence:tick,tick:0,generation:2,down:[],actions:[],look:[]}),false);
      assert.equal(box.push({epoch:1,sequence:tick,tick,generation:2,down:['KeyW'],actions:['jump'],
        look:[],lookTotal:[tick*2,-tick],mouseScale:1,firstPerson:true}),true);
      if(tick>1)assert.equal(box.push({epoch:1,sequence:tick+1,tick:tick-1,generation:2,down:[],actions:[],look:[]}),false);
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
    assert.equal(box.push({epoch:1,sequence:1,tick:1,generation:2,down:[code],actions:[],look:[]}),false);
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
  for(const call of [()=>box.take(1,2),()=>box.clear(),()=>box.queue])
    assert.throws(call,/disposed/);
  assert.equal(box.push({sequence:1}),false);
  const stale=new InputMailbox();stale.bridge.x.input_mailbox_drop(stale.handle);
  assert.throws(()=>stale.take(1,2),/operation refused/);
  assert.throws(()=>stale.queue,/inspection refused/);stale.dispose();
});
test('mailbox admits the registry order instead of a private key-code copy',()=>{
  const codes=['Mouse0','KeyW'],box=new InputMailbox(1,4,undefined,{inputCodes:codes});
  codes.reverse();
  assert.equal(box.push({epoch:1,sequence:1,tick:1,generation:2,down:['Mouse0'],actions:[],look:[]}),true);
  assert.deepEqual(box.take(1,2).down,['Mouse0']);
  assert.equal(box.push({epoch:1,sequence:2,tick:2,generation:2,down:['KeyS'],actions:[],look:[]}),false);
  box.dispose();
});

// Malformed values are connection-local refusals, including tick zero during
// startup and a later sequence stamped earlier than the newest queued input.
test('malformed mailbox shapes are counted and the next command still runs',()=>{
  const box=new InputMailbox(1,4),valid={epoch:1,sequence:1,tick:1,generation:2,down:[],actions:[],look:[]};
  const shapes=[null, {tick:0},{tick:-1},{tick:NaN},{tick:Infinity},{tick:.5},
    {generation:NaN},{generation:-1},{sequence:NaN},{sequence:Infinity},
    {down:null},{down:Array(65).fill('KeyW')},{down:['unknown']},
    {actions:null},{actions:[null]},{actions:Array(2049).fill('jump')},
    {look:null},{look:[null]},{look:[[1]]},{look:[[NaN,0]]},{look:Array(2049).fill([0,0])},
    {lookTotal:null},{lookTotal:[1]},{lookTotal:[Infinity,0]},{lookTotal:[2**49,0]},
    {mouseScale:0},{aspect:NaN},{invertMouse:1},{firstPerson:1},{viewTick:-1},{viewTick:2**32},
    {flightSettings:null},{flightSettings:{sensitivity:11}},{flightSettings:{curve:0}},
    {flightSettings:{bindings:{boost:'bad'}}},{gamepad:{axes:[2],buttons:[]}},
    {gamepad:{axes:[],buttons:[1]}},{fire:{}},{fire:[{id:0,button:0,pressed:true}]},
    {fire:[{id:1,button:2,pressed:true}]},{fire:[{id:1,button:0,pressed:1}]},
    {human:{}},{human:[{kind:'spawn'}]},{human:Array(17).fill({kind:'suicide'})}];
  try {
    for(const shape of shapes){const before=box.rejected;assert.equal(box.push(shape===null?null:{...valid,...shape}),false);assert.equal(box.rejected,before+1);}
    assert.equal(box.received,0);assert.deepEqual(box.queue,[]);
    assert.equal(box.push({...valid,tick:3}),true);
    assert.equal(box.push({...valid,sequence:2,tick:2}),false);
    assert.equal(box.lastRejection,'input tick moved backwards');
    assert.equal(box.received,1);assert.equal(box.queue.length,1);
    assert.equal(box.push({...valid,sequence:2,tick:3}),true);
    assert.equal(box.take(3,2).tick,3);assert.equal(box.ack,2);assert.deepEqual(box.queue,[]);
  }finally{box.dispose();}
});

test('mailbox lends one command and reuses look and fire rows across empty ticks',()=>{
  const box=new InputMailbox(1,2),c={epoch:1,generation:2,sequence:1,tick:1,down:['KeyW'],actions:['jump'],look:[],lookTotal:[2,-1],fire:[{id:1,button:0,pressed:true}]};
  try {
    assert.equal(box.push(c),true);const first=box.take(1,2),look=first.look,point=first.look[0],fire=first.fire[0],actions=first.actions;
    assert.equal(box.take(2,2),first);assert.equal(first.fire,undefined);assert.equal(first.look.length,0);
    assert.equal(box.push({...c,sequence:2,tick:3,lookTotal:[4,-2]}),true);
    const next=box.take(3,2);assert.equal(next,first);assert.equal(next.look,look);assert.equal(next.look[0],point);assert.equal(next.fire[0],fire);assert.equal(next.actions,actions);
    assert.equal(box.values,undefined);assert.equal(box.tokens,undefined);
    for(let tick=4;tick<1000;tick++){assert.equal(box.push({...c,tick,sequence:tick,lookTotal:[tick,0]}),true);box.take(tick,2);}
    assert.equal(box.references.filter(row=>row.command!==null).length,1);
  }finally{box.dispose();}
});

// Profile the host's retained command boundary after both empty and populated
// exchanges are warm; producer packets and the test harness are outside it.
test('warmed mailbox push and take allocate no JavaScript transport objects',async()=>{
  const {Session}=await import('node:inspector/promises');
  const box=new InputMailbox(1,4),profiler=new Session();profiler.connect();
  const command={epoch:1,generation:2,sequence:0,tick:0,down:['KeyW'],actions:['jump'],look:[],lookTotal:[0,0],mouseScale:1,firstPerson:true,
    flightSettings:{sensitivity:1,curve:1,bindings:{}},gamepad:{axes:[0,0,0,0],buttons:[{pressed:false}]},human:[{kind:'team',team:1}],fire:[{id:1,button:0,pressed:true}]};
  const frame=()=>{command.sequence++;command.tick++;command.lookTotal[0]++;box.push(command);box.take(command.tick,2);box.take(command.tick,2);};
  const warm=async()=>{for(let batch=0;batch<64;batch++){for(let n=0;n<128;n++)frame();await new Promise(resolve=>setTimeout(resolve,1));}};
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<128;n++)frame();await profiler.post('HeapProfiler.stopSampling');
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<512;n++)frame();const {profile}=await profiler.post('HeapProfiler.stopSampling'),allocations=[];
    const visit=node=>{if(node.selfSize&&/\/src\/sim\/wasm\/runtime\/input-mailbox\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};
    visit(profile.head);assert.deepEqual(allocations,[]);
  }finally{profiler.disconnect();box.dispose();}
});

test('native view tokens retain direct edits until a new command replaces them',()=>{
  const box=new InputMailbox();
  try{
    box.push({epoch:1,sequence:1,tick:1,generation:2,down:[],actions:[],look:[],aspect:2});box.take(1,2);
    box.view.aspect=3;box.view.mouseScale=4;
    assert.equal(box.take(2,2).aspect,3);assert.equal(box.view.mouseScale,4);
    delete box.view.aspect;assert.equal(box.take(3,2).aspect,undefined);
    box.push({epoch:1,sequence:2,tick:4,generation:2,down:[],actions:[],look:[],aspect:5});
    assert.equal(box.take(4,2).aspect,5);assert.equal(box.view.mouseScale,4);
  }finally{box.dispose();}
});

// Keep the allocation-free live writer byte-for-byte compatible with the
// generated codec across both populated and empty optional records.
test('retained mailbox writer uses the generated command layout exactly',async()=>{
  const {BlockHeaderLayout:H}=await import('../src/sim/gen/boundary.js');
  const box=new InputMailbox(),b=box.bridge,c=box.change,e=box.entry;
  const words=()=>{const block=b.readBlock(b.x.sim_calls()),f=block.words,at=block.base;return Array.from(f.subarray(at,at+H.words+f[at]*f[at+1]+f[at+3]));};
  try{
    Object.assign(e,{identity:1,down:1,sequence:1,tick:2,generation:3,codes:['KeyW'],actions:['jump'],look:[{point:[1.25,-2],object:2048}],lookTotal:[9,8],view:[{key:'aspect',value:11}],human:[16],fire:[{id:1,button:0,pressed:true}]});
    Object.assign(c,{op:'push',entry:e,sequence:1,epochMatches:true,tick:0,generation:0});
    for(const entry of [e,null]){
      c.entry=entry;b.callArgs(box.input,c);const expected=words();box.writeCall();assert.deepEqual(words(),expected);
    }
  }finally{box.dispose();}
});
