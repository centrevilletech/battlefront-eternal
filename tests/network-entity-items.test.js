import test from 'node:test';import assert from 'node:assert/strict';
import {Scene,Vector3,Object3D} from 'three';
import {Items} from '../src/items.js';
import {EntityItemCapture,EntityItemPresentation} from '../src/runtime/network-entity-items.js';
import {encodeEntityRecord,encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {EntityScheduler,EntityReceiver} from '../src/runtime/network-entity-stream.js';
const mine={base:'mine',GeometryName:'mine',TriggerRadius:2,SuppressRadius:1,ExplosionTrigger:{Damage:100},LightRadius:1},pickup={base:'powerupitem',GeometryName:'pickup',LifeSpan:30};
function fixture(){let explosions=0;const make=()=>new Items({scene:new Scene(),collision:{cast:()=>null},effects:{},lights:{add(){}},model:()=>()=>new Object3D(),explode(){explosions++;},pools:{mine:1,powerupitem:1}}),authority=make(),client=make();
 const definitions=new Map([[1,mine],[2,pickup]]),registry={definitions,definitionId:def=>def===mine?1:2};
 client.update=client.watch=client.offer=()=>{throw Error('client gameplay forbidden');};
 return{authority,client,capture:new EntityItemCapture(authority,registry),presentation:new EntityItemPresentation(client,registry),explosions:()=>explosions};}
let serial=0;
const transfer=record=>decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:++serial,tick:record.tick,records:[record]})).records[0];
const spawn=(f,def=mine)=>{f.authority.update(0);return f.authority.spawn(def,new Vector3(2,0,3),new Vector3(),.3);};

test('actual mine is visible until shared authority triggers it; client never checks targets',()=>{
 const f=fixture(),item=spawn(f);item.landed=true;const live=f.capture.capture(1)[0];f.presentation.receive(transfer(live),1);f.presentation.present(4);
 const view=f.presentation.slots.get(live.id).view;assert.ok(view.mesh.parent===f.client.scene);assert.ok(view.mesh.position.distanceTo(item.position)<.004);
 f.authority.watch(item,[{relation:()=>-1,center:item.position.clone(),velocity:new Vector3(),radius:.3}]);assert.equal(f.explosions(),1);
 const dead=f.capture.capture(2)[0];assert.equal(dead.active,false);f.presentation.receive(transfer(dead),1);assert.equal(view.mesh.parent,null);
 assert.equal(f.presentation.receive(transfer(live),1),false);assert.equal(f.explosions(),1);
});

test('actual pickup grants on authority once and disappears without any client grant',()=>{
 const f=fixture(),item=spawn(f,pickup);item.landed=true;let grants=0;const live=f.capture.capture(1)[0];f.presentation.receive(transfer(live),1);
 f.authority.offer(item,[{position:item.position.clone(),give(){grants++;return true;}}]);assert.equal(grants,1);
 const removed=transfer(f.capture.capture(2)[0]);f.presentation.receive(removed,1);f.presentation.receive(removed,1);assert.equal(grants,1);assert.equal(f.presentation.slots.get(live.id).view,null);
});

test('slot reuse increments generation and ignores old terminal, including same-tick retirement',()=>{
 const f=fixture(),first=spawn(f),one=f.capture.capture(1)[0];f.presentation.receive(transfer(one),1);
 f.authority.remove(first);const gone=f.capture.capture(1)[0];assert.equal(f.presentation.receive(transfer(gone),1),true);
 spawn(f,pickup);const two=f.capture.capture(2)[0];assert.equal(two.id,one.id);assert.equal(two.generation,one.generation+1);f.presentation.receive(transfer(two),1);
 assert.equal(f.presentation.receive(transfer(gone),1),false);assert.equal(f.presentation.slots.get(two.id).view.def,pickup);
});

test('resting item captures keep identity and ACKed state only sends bounded heartbeat',()=>{
 const f=fixture();spawn(f);const first=f.capture.capture(1)[0];assert.equal(f.capture.capture(4)[0],first);
 const scheduler=new EntityScheduler();scheduler.update(first);const packet=scheduler.packet({tick:1});scheduler.admitted(packet.packetId);scheduler.ack({epoch:1,packetId:packet.packetId});
 assert.equal(scheduler.update(first),false);assert.equal(scheduler.packet({tick:4}),null);assert.ok(scheduler.packet({tick:121}));
});

test('item receiver namespace and bounded slots survive repeated pickup lifetimes',()=>{
 const f=fixture(),receiver=new EntityReceiver({maxEntities:4});
 for(let i=0;i<100;i++){const item=spawn(f);const live=f.capture.capture(i*2+1)[0];receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:live.tick,records:[live]}));f.authority.remove(item);const dead=f.capture.capture(i*2+2)[0];receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:dead.tick,records:[dead]}));}
 assert.equal(f.capture.slots.length,2);assert.equal(f.capture.objects.size,0);assert.equal(receiver.records.size,1);
 receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:201,records:[{kind:'tombstone',id:0,generation:999,tick:201}]}));assert.equal(receiver.records.size,2);assert.equal(receiver.get('item',0).generation,100);
});

test('lost removal is retransmitted until ACK and cannot retire the next generation',()=>{
 const f=fixture(),first=spawn(f),scheduler=new EntityScheduler();scheduler.update(f.capture.capture(1)[0]);f.authority.remove(first);scheduler.update(f.capture.capture(2)[0]);
 const old=scheduler.packet({tick:2});scheduler.admitted(old.packetId);assert.ok(scheduler.packet({tick:3}));spawn(f,pickup);scheduler.update(f.capture.capture(4)[0]);scheduler.ack({epoch:1,packetId:old.packetId});
 assert.equal(scheduler.records.get('item:0').record.generation,2);
});

test('item IDs cannot revoke an unrelated actor seat with the same numeric ID',()=>{
 const seat={kind:'seat',id:0,generation:1,tick:1,vehicleId:10,vehicleGeneration:1,seatIndex:1,leaseVersion:1},f=fixture();spawn(f);const item={...f.capture.capture(1)[0],generation:99};
 const scheduler=new EntityScheduler();scheduler.update(seat);scheduler.update(item);assert.ok(scheduler.records.has('seat:0'));
 const receiver=new EntityReceiver();receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[seat,item]}));assert.equal(receiver.seatOwner(10,1,1).id,0);
});

test('world capture includes actual item pools and new epoch removes old visual instances',async()=>{
 const {EntityWorldCapture}=await import('../src/runtime/network-entity-capture.js');const f=fixture();spawn(f);
 const registry={classes:new Map(),weapons:new Map(),combat:f.capture.registry};const capture=new EntityWorldCapture({items:f.authority,units:{all:()=>[]},vehicles:{vehicles:[]}},registry);
 const record=capture.capture(1)[0];assert.equal(record.kind,'item');assert.equal(encodeEntityPacket({epoch:1,packetId:1,tick:1,records:[record]}).length,74);
 f.presentation.receive(transfer(record),1);const mesh=f.presentation.slots.get(record.id).view.mesh;f.presentation.reset(2);assert.equal(mesh.parent,null);assert.equal(f.presentation.slots.size,0);assert.equal(f.presentation.receive(transfer(record),1),false);
});

 test('ACK for live item cannot retire a same-tick removal',()=>{
 const f=fixture(),item=spawn(f),scheduler=new EntityScheduler();scheduler.update(f.capture.capture(1)[0]);const live=scheduler.packet({tick:1});scheduler.admitted(live.packetId);
 f.authority.remove(item);scheduler.update(f.capture.capture(1)[0]);scheduler.ack({epoch:1,packetId:live.packetId});assert.equal(scheduler.records.get('item:0').record.active,false);
 const terminal=scheduler.packet({tick:2});scheduler.admitted(terminal.packetId);scheduler.ack({epoch:1,packetId:terminal.packetId});assert.equal(scheduler.records.has('item:0'),false);
});

test('full item namespace does not consume actor capacity',()=>{
 const scheduler=new EntityScheduler({maxEntities:1}),receiver=new EntityReceiver({maxEntities:1});
 for(let id=0;id<256;id++){const item={kind:'item',id,generation:1,tick:1,active:false};scheduler.update(item);receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[item]}));}
 const actor={kind:'tombstone',id:1,generation:1,tick:1};scheduler.update(actor);receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[actor]}));assert.equal(receiver.records.size,257);assert.equal(scheduler.itemSlots.size,256);
 for(let generation=2;generation<5;generation++){for(let id=0;id<256;id++){const item={kind:'item',id,generation,tick:generation,active:false};scheduler.update(item);receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:generation,records:[item]}));}for(let n=0;n<8;n++){const packet=scheduler.packet({tick:generation});if(packet){scheduler.admitted(packet.packetId);scheduler.ack({epoch:1,packetId:packet.packetId});}}assert.equal(scheduler.itemSlots.size,256);assert.equal(receiver.itemCount,256);}
 assert.throws(()=>scheduler.update({...actor,id:2}),/capacity/);assert.throws(()=>receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[{...actor,id:2}]})),/capacity/);
});
test('item encoder rejects invalid surface normals before transmission',()=>{
 const f=fixture();spawn(f);const record=f.capture.capture(1)[0];assert.throws(()=>encodeEntityRecord({...record,up:[0,0,0]}),/normal/);
});
