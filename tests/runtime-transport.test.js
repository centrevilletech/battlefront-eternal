import test from 'node:test';
import assert from 'node:assert/strict';
import {TransferPool,ReliableOutbox,InputMailbox} from '../src/runtime/transport.js';
import {CommandHistory,CommandControls} from '../src/runtime/commands.js';
const command=(sequence,overrides={})=>({epoch:1,sequence,tick:sequence,generation:2,down:[],actions:[],look:[],...overrides});

test('transfer pressure skips snapshots and refuses foreign or duplicate returns',()=>{
 const pool=new TransferPool(64,3), a=pool.acquire(),b=pool.acquire(),c=pool.acquire();
 assert.equal(pool.acquire(),null);assert.equal(pool.skipped,1);assert.equal(pool.inFlight.size,3);
 assert.equal(pool.recycle({...a,epoch:2}),false);assert.equal(pool.recycle({...a,epoch:1,buffer:new ArrayBuffer(8)}),false);
 assert.equal(pool.recycle({...a,epoch:1}),true);assert.equal(pool.recycle({...a,epoch:1}),false);
 assert.equal(pool.acquire().buffer,a.buffer);assert.equal(pool.inFlight.size,3);
});
test('reliable events retain unacknowledged records independently of transforms',()=>{
 const box=new ReliableOutbox(3,2);box.push('spawn');box.push('fire');box.push('death');
 assert.throws(()=>box.push('respawn'),/backlog/);assert.deepEqual(box.batch().map(e=>e.value),['spawn','fire']);
 assert.equal(box.batch(),null);assert.equal(box.ack(5),false);assert.equal(box.ack(2),true);
 box.push('respawn');assert.deepEqual(box.batch().map(e=>e.value),['death','respawn']);
});
test('commands preserve fast edges, held state and mouse samples without replaying them',()=>{
 const box=new InputMailbox();box.push(command(1,{down:['Mouse0'],actions:['firePrimary','reload'],look:[[100,0]]}));
 box.push(command(2,{tick:1,actions:['firePrimary'],look:[[25,-5]]}));
 const tick=box.take(1,2);assert.deepEqual(tick.actions,['firePrimary','reload','firePrimary']);assert.deepEqual(tick.look,[[100,0],[25,-5]]);
 assert.deepEqual(tick.down,[]);assert.equal(box.ack,2);assert.deepEqual(box.take(2,2).actions,[]);
 assert.equal(box.push(command(2)),false);assert.equal(box.push(command(3,{epoch:0})),false);
 box.push(command(3,{generation:1,down:['KeyW'],actions:['jump']}));assert.deepEqual(box.take(3,2).actions,[]);assert.deepEqual(box.take(4,2).down,[]);
});
test('history reports overrun and generation/epoch boundaries reject stale controls',()=>{
 const h=new CommandHistory(2);for(let i=1;i<=3;i++)assert.equal(h.put(command(i)),true);
 assert.equal(h.after(0),null);assert.equal(h.after(1).length,2);assert.equal(h.put(command(3)),false);
 const c=new CommandControls();assert.equal(c.accept(command(1,{actions:['jump','jump'],down:['KeyW']})),true);
 assert.equal(c.take('jump'),true);assert.equal(c.take('jump'),true);assert.equal(c.axis('forward'),1);
 assert.equal(c.accept(command(2,{generation:1})),false);c.clear();assert.equal(c.axis('forward'),0);
 h.reset(2);assert.equal(h.get(3),null);assert.equal(h.put(command(4)),false);
});

test('worker commands retain view settings between packets and clear held input on suspend',()=>{
 const box=new InputMailbox();
 box.push(command(1,{mouseScale:2,invertMouse:true,firstPerson:true,aspect:2,down:['KeyW']}));
 const first=box.take(1,2), next=box.take(2,2);
 for(const key of ['mouseScale','invertMouse','firstPerson','aspect'])assert.equal(next[key],first[key]);
 box.push(command(2,{tick:3,actions:['jump'],down:['Mouse0']}));box.clear();
 assert.equal(box.ack,1);assert.equal(box.discarded,2);assert.equal(box.ackTick,1);assert.deepEqual(box.take(3,2).down,[]);assert.deepEqual(box.take(4,2).actions,[]);
 assert.throws(()=>box.push(command(3,{look:[null]})),/invalid input payload/);
 assert.throws(()=>box.push(command(3,{aspect:NaN})),/invalid view input/);
});

test('flight input and rendered view stamp survive mailbox coalescing and reject malformed values',()=>{
 const box=new InputMailbox();
 const flightSettings={sensitivity:2,invert:true,curve:1.35,bindings:{boost:['ShiftLeft']}};
 const gamepad={axes:[.25,-1],buttons:[{pressed:true}]};
 box.push(command(1,{flightSettings,gamepad,viewTick:0}));
 const value=box.take(1,2);assert.deepEqual(value.flightSettings,flightSettings);assert.deepEqual(value.gamepad,gamepad);assert.equal(value.viewTick,0);
 assert.deepEqual(box.take(2,2).gamepad,gamepad);
 for(const overrides of [{viewTick:NaN},{viewTick:-1},{flightSettings:{sensitivity:Infinity}},{flightSettings:{curve:'linear'}},{flightSettings:{curve:Infinity}},{flightSettings:{bindings:{boost:'bad'}}},{gamepad:{axes:[2],buttons:[]}},{gamepad:{axes:[],buttons:[1]}}])assert.throws(()=>new InputMailbox().push(command(1,overrides)),/invalid/);
});

test('input silence drops held gamepad axes and old view time but retains user settings',()=>{
 const box=new InputMailbox();box.push(command(1,{viewTick:1,flightSettings:{sensitivity:2},gamepad:{axes:[1,1],buttons:[{pressed:true}]}}));
 box.take(1,2);box.clear();const after=box.take(2,2);
 assert.equal(after.gamepad,undefined);assert.equal(after.viewTick,undefined);assert.equal(after.flightSettings.sensitivity,2);assert.deepEqual(after.down,[]);
});

test('the shipped client flight settings produce a valid online command',async()=>{
 const {DEFAULTS}=await import('../src/settings.js');const box=new InputMailbox();
 assert.equal(box.push(command(1,{gamepad:null,flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings}})),true);
 assert.equal(box.take(1,2).flightSettings.curve,DEFAULTS.flightResponseCurve);
});

test('trigger edges ride a command through the mailbox, in order, into the tick that takes it',()=>{
  const box=new InputMailbox();
  box.push(command(1,{tick:5,fire:[{id:1,button:0,pressed:true}]}));
  box.push(command(2,{tick:5,fire:[{id:2,button:0,pressed:false},{id:3,button:1,pressed:true}]}));
  box.push(command(3,{tick:6}));
  assert.deepEqual(box.take(5,2).fire,[{id:1,button:0,pressed:true},{id:2,button:0,pressed:false},{id:3,button:1,pressed:true}]);
  assert.equal(box.take(6,2).fire,undefined,'a tick with no edges carries none');
  for(const fire of [{},[{id:0,button:0,pressed:true}],[{id:1,button:2,pressed:true}],[{id:1,button:0,pressed:1}],Array.from({length:17},(_,i)=>({id:i+1,button:0,pressed:true}))])
    assert.throws(()=>box.push(command(9,{tick:7,fire})),/invalid trigger edges/);
});

test('future discarded and wrong-life inputs never masquerade as executed or update lead timing',()=>{const box=new InputMailbox();box.push(command(1,{down:['KeyW'],lookTotal:[4,0]}));box.take(1,2);box.push(command(2,{tick:40,down:['KeyS'],lookTotal:[9,0]}));box.clear();assert.equal(box.ack,1);assert.equal(box.ackTick,1);assert.equal(box.discarded,2);assert.deepEqual(box.take(40,2).down,[]);box.push(command(3,{tick:41,generation:1}));box.take(41,2);assert.equal(box.ack,1);assert.equal(box.ackTick,1);assert.equal(box.discarded,3);box.push(command(4,{tick:42,lookTotal:[12,0]}));assert.deepEqual(box.take(42,2).look,[[8,0]]);assert.equal(box.ack,4);assert.equal(box.ackTick,42);});
