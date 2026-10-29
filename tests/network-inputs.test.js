import test from 'node:test';import assert from 'node:assert/strict';
import {NetworkInputs,predictionCommands,expireMovement} from '../src/runtime/network-inputs.js';
const command=i=>({epoch:1,sequence:i,tick:i,generation:1,down:['KeyW'],look:[[i,-i]],actions:[]});
test('replaceable state retains cumulative look while actions use reliable lane',()=>{
  const states=[],events=[],fires=[],sender=new NetworkInputs({state:m=>{states.push(m);return true;},events:m=>{events.push(m);return true;},fire:c=>{fires.push(c.sequence);return true;}});
  for(let i=1;i<=6;i++)sender.send([{...command(i),actions:i===2?['jump','firePrimary']:[],...(i===3?{human:[{kind:'suicide'}]}:{})}]);
  assert.deepEqual(states.at(-1).commands.map(c=>c.sequence),[6]);
  assert.deepEqual(states.at(-1).commands.at(-1).lookTotal,[21,-21]);
  assert.ok(states.flatMap(m=>m.commands).every(c=>c.actions.length===0&&!c.human));
  assert.deepEqual(events.map(m=>[m.id,m.actions,m.human]),[[2,['jump'],undefined],[3,[],[{kind:'suicide'}]]]);
  assert.deepEqual(fires,[1,2,3,4,5,6]);
  sender.send([{...command(7),generation:2}]);assert.deepEqual(states.at(-1).commands[0].lookTotal,[7,-7]);
});
test('movement acknowledgements cannot discard an unacknowledged reliable jump',()=>{
  const old={...command(2),actions:['jump']},fresh=command(5);
  const pending=predictionCommands([fresh],[old],0,['KeyD']);
  assert.deepEqual(pending.map(c=>c.sequence),[2,5]);assert.deepEqual(pending[0].actions,['jump']);assert.deepEqual(pending[0].look,[]);assert.deepEqual(pending[0].down,['KeyD']);
  assert.deepEqual(predictionCommands([fresh],[old],2,[]).map(c=>c.sequence),[5]);
  assert.deepEqual(predictionCommands([old],[old],2,[])[0].actions,[]);
});

import {InputMailbox} from '../src/runtime/transport.js';
import {InputActions} from '../src/runtime/input-actions.js';
for(const rtt of [0,50,100,200])test(`real split sender recovers lost/reordered movement and delayed action at ${rtt}ms RTT`,()=>{
 const mailbox=new InputMailbox(),actions=new InputActions(),wire=[],reliable=[];let serial=0;
 const sender=new NetworkInputs({state:message=>{const n=++serial;if(n%20!==0&&!(n>=80&&n<=86))wire.push({at:n+Math.ceil(rtt/33.333)+(n%7===0?3:0),message});return true;},events:message=>{reliable.push({at:serial+1+Math.ceil(rtt/33.333)+9,message});return true;},fire:()=>true});
 let dx=0,dy=0,jumps=0;
 for(let tick=1;tick<=260;tick++){
  if(tick<=240)sender.send([{...command(tick),look:[[1,-2]],actions:tick===80?['jump']:[],down:tick<230?['KeyW']:[]}]);
  for(const item of wire.filter(x=>x.at===tick))for(const c of item.message.commands)mailbox.push(c);
  for(const item of reliable.filter(x=>x.at===tick)){actions.push(item.message,tick,1);actions.push(item.message,tick,1);}
  for(const [x,y] of mailbox.take(tick,1).look){dx+=x;dy+=y;}
  jumps+=actions.take(tick,1).actions.filter(x=>x==='jump').length;
 }
 // Last dropped state240 is repaired by a final neutral state241.
 sender.send([{...command(241),look:[],down:[]}]);
 for(const c of wire.at(-1).message.commands)mailbox.push(c);
 for(const [x,y] of mailbox.take(300,1).look){dx+=x;dy+=y;}
 assert.equal(dx,240);assert.equal(dy,-480);assert.equal(jumps,1);assert.equal(actions.ack,80);assert.deepEqual(mailbox.down,[]);
});

import {DEFAULTS} from '../src/settings.js';
test('default desktop state fits one small datagram even with a standard gamepad',()=>{
 let bytes;const sender=new NetworkInputs({state:message=>{bytes=Buffer.byteLength(JSON.stringify(message));return true;},events:()=>true,fire:()=>true});
 const c={...command(123456),viewTick:123450,mouseScale:1,invertMouse:false,firstPerson:true,aspect:16/9,
 flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings},
 gamepad:{axes:[.5,-.5,0,1],buttons:Array.from({length:17},()=>({pressed:false}))},flightTuning:{debugOnly:'x'.repeat(10000)}};
 sender.send([c]);assert.ok(bytes<=1200,`${bytes} bytes exceeds a small datagram`);console.log(`Default flight state: ${bytes} bytes, ${(bytes*60/1024).toFixed(1)} KiB/s at 60Hz before transport overhead`);
});

test('expired unacknowledgeable movement cannot block fresh input after a stall',()=>{
 const pending=Array.from({length:120},(_,i)=>command(i+1)),reliable=[{...command(10),actions:['jump']}];
 const kept=expireMovement(pending,300,1);assert.equal(kept.length,0);
 kept.push({...command(121),tick:301});assert.equal(expireMovement(kept,300,1).length,1);
 assert.equal(reliable.length,1,'movement expiry does not consume reliable actions');
 assert.equal(expireMovement(kept,300,2).length,0,'commands from a previous life are replaceable too');
});

test('held transition backup restores original execution tick without repeating look or actions',()=>{
 const wire=[],events=[],fires=[];
 const sender=new NetworkInputs({state:m=>{wire.push(m);return true;},events:m=>{events.push(m);return true;},fire:c=>{fires.push(c.sequence);return true;}});
 const mailbox=new InputMailbox();
 sender.send([{...command(1),down:[],look:[]}]);mailbox.push(wire[0].commands[0]);mailbox.take(1,1);
 sender.send([{...command(2),down:['KeyS'],actions:['jump','firePrimary'],look:[[3,4]]}]); // Lost original transition.
 sender.send([{...command(3),down:['KeyS'],look:[[2,1]]}]);
 const backup=wire.at(-1).commands;assert.deepEqual(backup.map(c=>c.sequence),[2,3]);
 assert.equal(backup[0],wire[1].commands[0],'original wire command retained');
 for(const c of backup)mailbox.push(c);
 const atTransition=mailbox.take(2,1);assert.deepEqual(atTransition.down,['KeyS']);assert.deepEqual(atTransition.look,[[3,4]]);
 assert.deepEqual(mailbox.take(3,1).look,[[2,1]]);
 sender.send([{...command(4),down:['KeyS'],look:[]}]);for(const c of wire.at(-1).commands)mailbox.push(c);
 assert.deepEqual(mailbox.take(4,1).look,[],'duplicate backup cannot repeat cumulative mouse counts');
 assert.ok(wire.flatMap(m=>m.commands).every(c=>c.actions.length===0));
 assert.equal(events.length,1);assert.deepEqual(events[0].actions,['jump']);assert.deepEqual(fires,[1,2,3,4]);
 sender.ack(2);sender.send([{...command(5),down:['KeyS']}]);assert.equal(wire.at(-1).commands.length,1);
});

test('held backups are bounded by age, newest transition, life and epoch',()=>{
 const wire=[],sender=new NetworkInputs({state:m=>{wire.push(m);return true;},events:()=>true,fire:()=>true});
 sender.send([{...command(1),down:[]}]);sender.send([command(2)]);sender.send([command(3)]);
 assert.deepEqual(wire.at(-1).commands.map(c=>c.sequence),[2,3]);
 sender.send([{...command(4),down:['KeyS']}]);sender.send([{...command(5),down:['KeyS']}]);
 assert.deepEqual(wire.at(-1).commands.map(c=>c.sequence),[4,5]);
 sender.send([{...command(29),down:['KeyS']}]);assert.equal(wire.at(-1).commands.length,1);
 sender.send([command(30)]);sender.send([{...command(31),generation:2}]);assert.equal(wire.at(-1).commands.length,1);
 sender.send([{...command(32),generation:2,down:[]}]);sender.send([{...command(33),epoch:2,generation:2,down:[]}]);assert.equal(wire.at(-1).commands.length,1);
 sender.reset();assert.equal(sender.transition,null);assert.equal(sender.history.length,0);
});

test('optional transition backups stay within a bounded application payload',()=>{
 const wire=[],sender=new NetworkInputs({state:m=>{wire.push(m);return true;},events:()=>true,fire:()=>true});
 const large={...command(1),down:[],gamepad:{axes:[0,0,0,0],buttons:Array.from({length:17},()=>({pressed:false}))},flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings},mouseScale:1,aspect:16/9,firstPerson:true};
 sender.send([large]);sender.send([{...large,sequence:2,tick:2,down:['KeyW']}]);sender.send([{...large,sequence:3,tick:3,down:['KeyW']}]);
 const message=wire.at(-1);assert.ok(Buffer.byteLength(JSON.stringify({...message,sequence:100000}))<=1200);assert.equal(message.commands.length,1,'oversized optional backup omitted');
});
