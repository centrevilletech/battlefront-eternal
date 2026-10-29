import test from 'node:test';import assert from 'node:assert/strict';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES,decodeEntityInput} from '../src/runtime/network-entity-input.js';
const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES,classIds:new Map([['soldier',1]]),postIds:new Map([['base',1]])};
const command=(sequence,extra={})=>({epoch:1,generation:1,sequence,tick:sequence,down:[],look:[],actions:[],...extra});
function fixture(){const packets=[],fires=[];const sender=new EntityInputSender({registry,send:b=>{assert.ok(b.length<=1100);packets.push(decodeEntityInput(b,registry));return true;},getAim:()=>({aimYaw:1,aimPitch:.2}),onFire:e=>fires.push(e)});return{sender,packets,fires};}
test('compact state repeats edges through lost packets without repeating local fire',()=>{
 const{sender,packets,fires}=fixture();sender.send([command(1,{look:[[3,4]],actions:['jump','firePrimary']})]);
 sender.send([command(2,{look:[[2,1]]})]);assert.deepEqual(packets[1].commands.at(-1).lookTotal,[5,5]);assert.deepEqual(packets[1].edges.map(e=>e.id),[1,2,3]);assert.equal(fires.length,1);
 sender.feedback({epoch:1,generation:1,received:[1,2,3],results:[]});sender.send([command(3)]);assert.equal(packets[2].edges.length,0);assert.deepEqual(sender.pendingActions(1),['jump'],'receipt does not retire prediction');
 sender.feedback({epoch:1,generation:1,received:[],results:[{id:1,status:'consumed',tick:5}]});sender.applied(4);assert.deepEqual(sender.pendingActions(1),['jump']);sender.applied(5);assert.deepEqual(sender.pendingActions(1),[]);
});
test('independent outcomes do not retire an earlier delayed action',()=>{
 const{sender}=fixture();sender.send([command(1,{actions:['jump']}),command(2,{actions:['reload']})]);
 sender.feedback({epoch:1,generation:1,received:[2],results:[{id:2,status:'consumed',tick:3}]});sender.applied(3);
 assert.deepEqual(sender.pendingActions(1),['jump']);assert.deepEqual(sender.pendingActions(2),[]);
});
test('spawn references are canonical and life reset cancels old retained edges',()=>{
 const{sender,packets}=fixture();sender.send([command(1,{human:[{kind:'spawn',className:'soldier',post:'base'}]})]);assert.equal(packets[0].edges[0].classId,1);assert.equal(packets[0].edges[0].postId,1);
 sender.send([command(2,{generation:2,look:[[1,2]]})]);assert.equal(packets[1].edges.length,0);assert.deepEqual(packets[1].commands[0].lookTotal,[1,2]);assert.equal(sender.outcomes.size,0);
});
test('bounded packets retain omitted edges for a later datagram',()=>{
 const{sender,packets}=fixture();sender.send([command(1,{actions:Array(16).fill('firePrimary'),gamepad:{axes:[0,0,0,0],buttons:[]},mouseScale:1,aspect:1.5,viewTick:1,flightSettings:{bindings:{},sensitivity:1,curve:1.35}})]);assert.ok(packets[0].edges.length<32);assert.equal(sender.edges.pending.size,32);
 sender.feedback({epoch:1,generation:1,received:packets[0].edges.map(e=>e.id),results:[]});sender.send([command(2)]);assert.ok(packets[1].edges.length>0);assert.equal(new Set([...packets[0].edges,...packets[1].edges].map(e=>e.id)).size,32);
});
test('an undelivered expired edge retires only after authority passes its acceptance horizon',()=>{
 const{sender,packets}=fixture();sender.send([command(1,{actions:['jump']})]);sender.send([command(32)]);assert.equal(packets.at(-1).edges.length,0);assert.deepEqual(sender.pendingActions(1),['jump']);sender.applied(31);assert.deepEqual(sender.pendingActions(1),['jump']);sender.applied(32);assert.deepEqual(sender.pendingActions(1),[]);
});
test('upstream backpressure does not lose later discrete edges from the same input batch',()=>{
 const{sender}=fixture();sender.sendPacket=()=>false;sender.send([command(1,{actions:['jump']}),command(2,{actions:['reload']})]);assert.deepEqual([...sender.edges.pending.values()].map(e=>e.action),['jump','reload']);
});
test('online state coalescing keeps cumulative look and immediately sends movement starts/stops',()=>{
 const{sender,packets}=fixture();sender.stateIntervalTicks=2;
 sender.send([command(1,{look:[[1,2]]})]);sender.send([command(2,{look:[[3,4]]})]);assert.equal(packets.length,1);
 sender.send([command(3,{look:[[5,6]]})]);assert.equal(packets.length,2);assert.deepEqual(packets.at(-1).commands.at(-1).lookTotal,[9,12]);
 sender.send([command(4,{down:['KeyW']})]);assert.equal(packets.length,3,'movement press bypasses cadence');
 sender.send([command(5,{down:[]})]);assert.equal(packets.length,4,'movement release bypasses cadence');
});
test('coalescing does not delay discrete actions, fire retries or held local shot prediction',()=>{
 const{sender,packets,fires}=fixture();sender.stateIntervalTicks=2;
 sender.send([command(1)]);sender.send([command(2,{actions:['jump']})]);assert.equal(packets.length,2);
 sender.send([command(3)]);assert.equal(packets.length,3,'unreceipted actions keep their normal retry cadence');
 sender.feedback({epoch:1,generation:1,received:[1],results:[]});
 sender.send([command(4,{down:['Mouse0']})]);assert.equal(packets.length,4,'new fire edge bypasses cadence');
 sender.feedback({epoch:1,generation:1,received:[2],results:[]});
 sender.send([command(5,{down:['Mouse0']})]);assert.equal(packets.length,4);assert.equal(fires.length,2,'local held presentation still processes skipped wire frames');
 sender.send([command(6,{down:[]})]);assert.equal(packets.length,5);assert.equal(packets.at(-1).edges.at(-1).pressed,false);
});
test('every input packet carries the snapshot ack and failed admission does not advance cadence',()=>{
 const{sender,packets}=fixture();sender.stateIntervalTicks=2;sender.snapshotAck=()=>({epoch:1,tick:7});
 sender.send([command(1)]);sender.send([command(2)]);
 const send=sender.sendPacket;sender.sendPacket=()=>false;sender.send([command(3)]);sender.sendPacket=send;
 sender.send([command(4)]);sender.send([command(5)]);sender.send([command(6)]);sender.send([command(7)]);sender.send([command(8)]);
 assert.deepEqual(packets.map(p=>p.snapshotAck),[{epoch:1,tick:7},{epoch:1,tick:7},{epoch:1,tick:7},{epoch:1,tick:7}],'a lost input loses nothing: the next carries the ack again');
 sender.send([command(9,{generation:2})]);assert.equal(packets.at(-1).generation,2,'new life must not inherit old cadence');
});
test('input lead does not prematurely expire an edge before its authoritative acceptance window',()=>{
 const{sender,packets}=fixture();sender.serverTick=()=>10;sender.send([command(50,{actions:['jump']})]);sender.send([command(81)]);assert.equal(packets.at(-1).edges.length,1);sender.serverTick=()=>81;sender.send([command(100)]);assert.equal(packets.at(-1).edges.length,0);
});
test('catch-up frame coalesces state datagrams without dropping numbered actions or cumulative look',()=>{
 const{sender,packets}=fixture();sender.send(Array.from({length:8},(_,i)=>command(i+1,{look:[[1,2]],actions:i===2?['jump']:i===6?['reload']:[]})));
 assert.equal(packets.length,1);assert.equal(packets[0].commands.at(-1).sequence,8);assert.deepEqual(packets[0].commands.at(-1).lookTotal,[8,16]);assert.deepEqual(packets[0].edges.map(e=>e.action),['jump','reload']);
});
test('same-life catch-up preserves held press and release edges even when newest held state is neutral',()=>{
 const{sender,packets,fires}=fixture();sender.send([command(1,{down:['Mouse0']}),command(2,{down:[]})]);assert.equal(packets.length,1);assert.deepEqual(packets[0].commands.at(-1).down,[]);assert.deepEqual(packets[0].edges.map(e=>[e.button,e.pressed,e.tick]),[['firePrimary',true,1],['firePrimary',false,2]]);assert.equal(fires.length,1);
});
test('shield prediction retains pressed fire outcomes after movement ACK until exact correction applies',()=>{
 const{sender}=fixture();sender.send([command(1,{actions:['use','fireSecondary']})]);sender.ack(1);
 assert.deepEqual(sender.replayActions(true).actions.get(1),['use','fireSecondary']);
 assert.deepEqual(sender.replayActions(true).shieldCommands.map(c=>c.sequence),[1]);
 const pressed=[...sender.outcomes.values()].find(v=>v.edge.kind==='fire'&&v.edge.pressed).edge.id;
 sender.feedback({epoch:1,generation:1,received:[pressed],results:[{id:pressed,status:'consumed',tick:5}]});
 sender.applied(4);assert.equal(sender.replayActions(true).shieldCommands.length,1);
 sender.applied(5);assert.equal(sender.replayActions(true).shieldCommands.length,0);assert.deepEqual(sender.replayActions(true).actions.get(1),['use']);
});

test('rejected and expired shield edges retire only once authority includes the decision',()=>{
 for(const status of ['expired','rejected']){
  const{sender}=fixture();sender.send([command(1,{actions:['fireSecondary']})]);
  const edge=[...sender.outcomes.values()].find(v=>v.edge.pressed).edge;
  sender.feedback({epoch:1,generation:1,received:[],results:[{id:edge.id,status,tick:4}]});
  sender.applied(3);assert.equal(sender.replayActions(true).shieldCommands.length,1);
  sender.applied(4);assert.equal(sender.replayActions(true).shieldCommands.length,0);
 }
});

test('mounted primary and secondary presses survive movement ACK until their outcome correction',()=>{
 const {sender}=fixture();sender.send([command(1,{actions:['firePrimary','fireSecondary']})]);sender.ack(1);
 const replay=sender.replayActions(false,true);assert.deepEqual(replay.actions.get(1),['firePrimary','fireSecondary']);assert.equal(replay.shieldCommands.length,2);
 const ids=[...sender.outcomes].filter(([,v])=>v.edge.kind==='fire'&&v.edge.pressed).map(([id])=>id);
 sender.feedback({epoch:1,generation:1,received:ids,results:ids.map(id=>({id,status:'rejected',tick:4}))});
 sender.applied(3);assert.equal(sender.replayActions(false,true).shieldCommands.length,2);
 sender.applied(4);assert.equal(sender.replayActions(false,true).shieldCommands.length,0);
});
test('a server too far behind costs the oldest pending outcomes, never the session',()=>{
 const{sender,packets}=fixture();
 // Nothing is receipted or resolved: 200 jumps, one per input frame.
 for(let i=1;i<=200;i++)sender.send([command(i,{actions:['jump']})]);
 assert.equal(sender.outcomes.size,128);assert.equal(sender.droppedOutcomes,72);
 assert.deepEqual(Math.min(...sender.outcomes.keys()),73,'the newest outcomes are kept');
 assert.ok(sender.edges.pending.size<=32,'the resend buffer stays bounded');
 assert.ok(packets.at(-1).edges.every(edge=>edge.id>168),'only recent unreceipted edges are repeated');
 // Results for given-up outcomes are ignored; kept ones still resolve.
 sender.feedback({epoch:1,generation:1,received:[],results:[{id:5,status:'consumed',tick:10},{id:200,status:'consumed',tick:201}]});
 assert.equal(sender.outcomes.get(200).result.status,'consumed');sender.applied(201);assert.equal(sender.outcomes.has(200),false);
});
test('a full edge resend buffer expires its oldest edge instead of throwing',()=>{
 const{sender}=fixture();sender.serverTick=()=>1;
 // 40 edges inside one acceptance window, none receipted.
 sender.send([command(1,{actions:Array(40).fill('jump')})]);
 assert.equal(sender.edges.pending.size,32);assert.equal(Math.min(...sender.edges.pending.keys()),9);
 sender.send([command(2)]);
 for(let id=1;id<=8;id++)assert.equal(sender.outcomes.get(id).result.status,'expired');
 assert.equal(sender.outcomes.get(9).result,null);
});
