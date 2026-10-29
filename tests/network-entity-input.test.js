import test from 'node:test';import assert from 'node:assert/strict';
import {INPUT_CODES,encodeEntityInput,decodeEntityInput} from '../src/runtime/network-entity-input.js';
import {EntityEdgeSender,EntityEdgeReceiver} from '../src/runtime/network-entity-edges.js';
const command=(sequence=1)=>({sequence,tick:sequence+5,down:['KeyW','Mouse0'],lookTotal:[100.5,-55],mouseScale:1,aspect:16/9,flightSettings:{sensitivity:1,curve:1.35,invert:false,bindings:{}},gamepad:{axes:[.1,.2,.3,.4],buttons:[{pressed:true}]}});
test('fractional view ticks survive input and reliable fire encoding without changing the ABI header',async()=>{
 const {InputMailbox}=await import('../src/runtime/transport.js');
 const {FireCommands}=await import('../src/runtime/fire-commands.js');
 const viewTick=4.987654321,edge={id:1,tick:6,kind:'fire',button:'fireSecondary',pressed:true,viewTick,aimYaw:0,aimPitch:0};
 const bytes=encodeEntityInput({epoch:1,generation:3,commands:[{...command(),viewTick}],edges:[edge]});
 const header=new DataView(bytes.buffer);assert.equal(header.getUint32(0,true),0x31494e42);assert.equal(header.getUint32(4,true),1);assert.equal(header.getUint32(8,true),3);
 const decoded=decodeEntityInput(bytes);assert.equal(decoded.commands[0].viewTick,viewTick);assert.equal(decoded.edges[0].viewTick,viewTick);
 const mailbox=new InputMailbox();assert.equal(mailbox.push(decoded.commands[0]),true);
 const fire=new FireCommands();assert.equal(fire.push({...decoded.edges[0],button:1},6,3),true);assert.equal(fire.take(6,3).shots[0].viewTick,viewTick);
 for(const invalid of [-.1,Infinity,NaN,0x100000000]){
  assert.throws(()=>encodeEntityInput({epoch:1,generation:3,commands:[{...command(),viewTick:invalid}]}));
  assert.throws(()=>encodeEntityInput({epoch:1,generation:3,edges:[{...edge,viewTick:invalid}]}));
 }
 assert.equal(new FireCommands().push({...edge,epoch:1,generation:3,button:1,viewTick:8.001},6,3),false,'future budget remains two ticks');
});
test('input scalar optimization preserves the established little-endian wire vector and truncation guards',()=>{
 const packet={epoch:0x01020304,generation:0x05060708,commands:[{sequence:0x090a0b0c,tick:0x0d0e0f10,down:['KeyW'],lookTotal:[1.25,-2.5]}],edges:[{id:1,tick:2,kind:'spawn',classId:0x1122,postId:0x3344}],snapshotAck:{epoch:0xfffe,tick:0xffffffff}};
 // Established BNI1 bytes from d6013e7 with the packet-receipt tail replaced
 // by the snapshot ack (epoch u16 + tick u32), independent of the reader.
 const bytes=Buffer.from('424e4931040302010807060501010101000000000c0b0a09100f0e0d0000004000000000000000000000000000000000000000f43f00000000000004c001000000020000000322114433feffffffffff','hex');
 assert.deepEqual(Buffer.from(encodeEntityInput(packet)),bytes);
 const decoded=decodeEntityInput(bytes);assert.equal(decoded.epoch,packet.epoch);assert.equal(decoded.generation,packet.generation);assert.deepEqual(decoded.commands[0].lookTotal,[1.25,-2.5]);assert.equal(decoded.edges[0].classId,0x1122);assert.equal(decoded.edges[0].postId,0x3344);assert.deepEqual(decoded.snapshotAck,packet.snapshotAck);
 for(let length=0;length<bytes.length;length++)assert.throws(()=>decodeEntityInput(bytes.subarray(0,length)));
 for(const epoch of [-1,0x100000000,1.5,NaN,Infinity,undefined])assert.throws(()=>encodeEntityInput({...packet,epoch}),/invalid input scalar/);
 for(const classId of [-1,65536,1.5,NaN])assert.throws(()=>encodeEntityInput({...packet,edges:[{...packet.edges[0],classId}]}),/invalid input scalar/);
 for(const look of [NaN,Infinity,-Infinity])assert.throws(()=>encodeEntityInput({...packet,commands:[{...packet.commands[0],lookTotal:[look,0]}]}),/invalid input scalar/);
});
test('compact input shares view context, preserves exact cumulative look and discrete short click',()=>{assert.ok(INPUT_CODES.length<=128);const edges=[true,false].map((pressed,i)=>({id:i+1,tick:6,kind:'fire',button:'firePrimary',pressed,viewTick:4,aimYaw:1,aimPitch:.2}));const bytes=encodeEntityInput({epoch:1,generation:3,commands:[command(),command(2)],edges,snapshotAck:{epoch:1,tick:7}});const decoded=decodeEntityInput(bytes);assert.deepEqual(decoded.snapshotAck,{epoch:1,tick:7});assert.equal(decoded.commands.length,2);assert.deepEqual(decoded.commands[0].lookTotal,[100.5,-55]);assert.deepEqual(decoded.edges.map(e=>e.pressed),[true,false]);assert.equal(decoded.commands[1].flightSettings.curve,1.35);assert.ok(bytes.length<300);console.log(`Two full controller commands + short fire click: ${bytes.length} bytes`);assert.throws(()=>decodeEntityInput(bytes.subarray(0,bytes.length-1)));assert.throws(()=>encodeEntityInput({epoch:1,generation:1,commands:Array.from({length:9},()=>command())}));});
test('all numbered human and action edges roundtrip; malformed numeric input fails',()=>{const edges=[{kind:'action',action:'jump'},{kind:'team',team:2},{kind:'spawn',classId:4,postId:7},{kind:'suicide'},{kind:'cancel'}].map((e,i)=>({...e,id:i+1,tick:5}));assert.equal(decodeEntityInput(encodeEntityInput({epoch:1,generation:1,edges})).edges.length,5);assert.throws(()=>encodeEntityInput({epoch:1,generation:1,commands:[{...command(),mouseScale:NaN}]}));assert.throws(()=>encodeEntityInput({epoch:1,generation:1,commands:[{...command(),down:['UnknownKey']}]}));});
test('burst loss, reorder and duplicate edges execute once; movement ACK does not retire edges',()=>{const sender=new EntityEdgeSender(),receiver=new EntityEdgeReceiver();sender.reset(1,2);receiver.reset(1,2);for(let id=1;id<=4;id++)sender.add({epoch:1,generation:2,id,tick:10,kind:'action',action:'jump'});for(let tick=10;tick<16;tick++)assert.equal(sender.batch(tick).length,4);const batch=sender.batch(16).reverse(),accepted=receiver.receive(batch,16);assert.equal(receiver.receive(batch,16).receipts.length,4);assert.equal(receiver.take(16).length,4);assert.equal(receiver.take(17).length,0);sender.receipt({epoch:1,generation:2,ids:[1,3]});assert.deepEqual(sender.batch(17).map(e=>e.id),[2,4]);sender.receipt({epoch:1,generation:2,ids:accepted.receipts});assert.equal(sender.batch(18).length,0);});
test('expiry, life cancellation and capacity rejection are explicit and never receipt-ACK discarded work',()=>{const s=new EntityEdgeSender({capacity:1}),r=new EntityEdgeReceiver({capacity:1});s.reset(1,1);r.reset(1,1);s.add({id:1,tick:1,epoch:1,generation:1});assert.equal(s.add({id:2,tick:1,epoch:1,generation:1}),true,'a full resend buffer expires its oldest edge');assert.deepEqual(s.takeReports(),[{id:1,status:'expired'}]);assert.equal(s.batch(32).length,0);assert.equal(s.takeReports()[0].status,'expired');s.add({id:3,tick:33,epoch:1,generation:1});s.reset(1,2);assert.equal(s.takeReports()[0].status,'life-cancelled');const result=r.receive([{id:1,tick:40,epoch:1,generation:1},{id:2,tick:40,epoch:1,generation:1}],40);assert.deepEqual(result.receipts,[1]);assert.equal(result.rejected[0].status,'capacity');assert.equal(r.receive([{id:3,tick:1,epoch:1,generation:1}],40).rejected[0].status,'expired');});

test('receipt cannot permit edge ID reuse and expired duplicate stays deduplicated',()=>{const s=new EntityEdgeSender(),r=new EntityEdgeReceiver();s.reset(1,1);r.reset(1,1);const e={id:1,tick:1,epoch:1,generation:1};s.add(e);s.receipt({epoch:1,generation:1,ids:[1]});assert.equal(s.add(e),false);r.receive([e],1);r.take(1);assert.deepEqual(r.receive([e],100).receipts,[1]);assert.equal(r.take(100).length,0);});

test('registered custom binding profiles compare by mapping rather than object insertion order',()=>{const registry={inputCodes:INPUT_CODES,bindingProfiles:new Map([[0,{}],[1,{brake:['KeyS'],throttle:['KeyW']}]])},c={...command(),flightSettings:{sensitivity:1,curve:1.35,bindings:{throttle:['KeyW'],brake:['KeyS']}}};const decoded=decodeEntityInput(encodeEntityInput({epoch:1,generation:1,commands:[c]},registry),registry);assert.deepEqual(decoded.commands[0].flightSettings.bindings,registry.bindingProfiles.get(1));});
test('decoded shared view contexts remain independent across keyboard and controller commands',()=>{
 for(const controller of [false,true]){
  const first=command(),second=command(2);
  if(!controller)for(const c of [first,second]){delete c.flightSettings;delete c.gamepad;}
  const {commands}=decodeEntityInput(encodeEntityInput({epoch:1,generation:1,commands:[first,second]}));
  commands[0].mouseScale=2;commands[0].down.push('KeyX');assert.equal(commands[1].mouseScale,1);assert(!commands[1].down.includes('KeyX'));
  if(controller){commands[0].flightSettings.bindings.test='changed';commands[0].gamepad.axes[0]=1;commands[0].gamepad.buttons[0].pressed=false;assert.equal(commands[1].flightSettings.bindings.test,undefined);assert.equal(commands[1].gamepad.axes[0],.1);assert.equal(commands[1].gamepad.buttons[0].pressed,true);}
 }
});
test('decoded custom binding arrays remain independent of each other and the authenticated registry',()=>{
 const bindings={brake:['KeyS'],throttle:['KeyW','ShiftLeft']};
 const registry={inputCodes:INPUT_CODES,bindingProfiles:new Map([[1,bindings]])};
 const first={...command(),flightSettings:{bindings}},second={...command(2),flightSettings:{bindings}};
 const bytes=encodeEntityInput({epoch:1,generation:1,commands:[first,second]},registry);
 const decoded=decodeEntityInput(bytes,registry);
 decoded.commands[0].flightSettings.bindings.throttle[0]='KeyX';
 decoded.commands[0].gamepad.buttons[0].pressed=false;
 assert.deepEqual(decoded.commands[1].flightSettings.bindings.throttle,['KeyW','ShiftLeft']);
 assert.equal(decoded.commands[1].gamepad.buttons[0].pressed,true);
 assert.deepEqual(bindings.throttle,['KeyW','ShiftLeft']);
 assert.deepEqual(decodeEntityInput(bytes,registry).commands[0].flightSettings.bindings,bindings);
});
