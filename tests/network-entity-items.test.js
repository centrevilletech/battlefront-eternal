import test from 'node:test';import assert from 'node:assert/strict';
import {Scene,Vector3,Object3D,Group,Mesh,BoxGeometry} from 'three';
import {Items} from '../src/items.js';
import {EntityItemCapture,EntityItemPresentation} from '../src/runtime/network-entity-items.js';
import {encodeEntityRecord,encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {EntityScheduler,EntityReceiver} from '../src/runtime/network-entity-stream.js';
import {SnapshotClient} from '../src/runtime/snapshot-client.js';
import {PlayoutClock} from '../src/runtime/playout.js';
import {createSnapshotSender} from './snapshot-send.js';
import {entryFromRecord,recordFromEntry} from '../src/runtime/snapshot-records.js';
const mine={base:'mine',GeometryName:'mine',TriggerRadius:2,SuppressRadius:1,ExplosionTrigger:{Damage:100},LightRadius:1},pickup={base:'powerupitem',GeometryName:'pickup',LifeSpan:30};
// A pilot's dispenser health: Gravity 1, Rebound 0, Friction 1.
const health={base:'powerupitem',class:'com_item_powerup_dual',GeometryName:'dual',Gravity:1,Rebound:0,Friction:1,LifeSpan:30};
// Flat ground at y = 0, or none.
const flat={cast(start,dir,length,hit){if(dir.y>=0||start.y<0)return null;const t=start.y/-dir.y;if(t>length)return null;hit.distance=t;hit.point.copy(start).addScaledVector(dir,t);hit.normal.set(0,1,0);return hit;}},none={cast:()=>null};
function fixture({ground=none,clientGround=ground,pools={mine:1,powerupitem:1},model=()=>()=>new Object3D()}={}){let explosions=0;const make=collision=>new Items({scene:new Scene(),collision,effects:{},lights:{add(){}},model,explode(){explosions++;},pools}),authority=make(ground),client=make(clientGround);
 const definitions=new Map([[1,mine],[2,pickup],[3,health]]),registry={definitions,definitionId:def=>def===mine?1:def===pickup?2:3};
 for(const name of ['update','watch','offer','expire','destroy','hurt'])client[name]=()=>{throw Error('client gameplay forbidden');};
 return{authority,client,capture:new EntityItemCapture(authority,registry),presentation:new EntityItemPresentation(client,registry),explosions:()=>explosions};}
let serial=0;
const transfer=record=>decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:++serial,tick:record.tick,records:[record]})).records[0];
const spawn=(f,def=mine)=>{f.authority.update(0);return f.authority.spawn(def,new Vector3(2,0,3),new Vector3(),.3);};
const rng=seed=>()=>{seed=seed+0x6d2b79f5|0;let t=Math.imul(seed^seed>>>15,1|seed);t=t+Math.imul(t^t>>>7,61|t)^t;return((t^t>>>14)>>>0)/4294967296;};
// Online as game.js runs it: a snapshot every 2 ticks through the real
// snapshot codec (tests/snapshot-send.js, SnapshotClient), arriving 45 to
// 75 ms after its tick in order, and the page's playout clock.
function online(f,seed=1){
 const clock=new PlayoutClock({minDelay:4,maxDelay:10}),queue=[],random=rng(seed);let latest=0,now=0,arrival=0;
 const adapter={receiver:new SnapshotClient(),receive(bytes){const packet=this.receiver.receive(bytes);if(!packet)return;for(const record of packet.records)if(record.kind==='item')f.presentation.receive(record,packet.ack.epoch);if(packet.tick>latest){latest=packet.tick;clock.arrive(packet.tick,now,packet.interval);}}};
 const sender=createSnapshotSender(adapter);
 return{publish(tick){const datagrams=sender.datagrams(tick,f.capture.capture(tick));arrival=Math.max(arrival,tick/60+.045+random()*.03);queue.push({at:arrival,datagrams});},
  frame(dt){now+=dt;while(queue.length&&queue[0].at<=now)for(const bytes of queue.shift().datagrams)adapter.receive(bytes);const tick=clock.advance(dt);f.presentation.present(tick);return{tick,latest};}};
}
const along=(path,tick)=>{const base=Math.floor(tick);return path[base].clone().lerp(path[base+1]??path[base],tick-base);};

test('actual mine is visible until shared authority triggers it; client never checks targets',()=>{
 const f=fixture(),item=spawn(f);item.landed=true;const live=f.capture.capture(1)[0];f.presentation.receive(transfer(live),1);f.presentation.present(1.5);
 const view=f.presentation.slots.get(live.id).view;assert.ok(view.mesh.parent===f.client.scene);assert.ok(view.mesh.position.distanceTo(item.position)<.004);
 f.authority.watch(item,[{relation:()=>-1,center:item.position.clone(),velocity:new Vector3(),radius:.3}]);assert.equal(f.explosions(),1);
 const dead=f.capture.capture(2)[0];assert.equal(dead.active,false);f.presentation.receive(transfer(dead),1);
 f.presentation.present(1.9);assert.ok(view.mesh.parent===f.client.scene,'drawn until the playout clock reaches its removal');
 f.presentation.present(2);assert.equal(view.mesh.parent,null);assert.equal(f.presentation.slots.get(live.id).view,null);
 assert.equal(f.presentation.receive(transfer(live),1),false);assert.equal(f.explosions(),1);
});

test('actual pickup grants on authority once and disappears without any client grant',()=>{
 const f=fixture(),item=spawn(f,pickup);item.landed=true;let grants=0;const live=f.capture.capture(1)[0];f.presentation.receive(transfer(live),1);f.presentation.present(1);
 f.authority.offer(item,[{position:item.position.clone(),give(){grants++;return true;}}]);assert.equal(grants,1);
 const removed=transfer(f.capture.capture(2)[0]);f.presentation.receive(removed,1);f.presentation.receive(removed,1);assert.ok(f.presentation.slots.get(live.id).view);
 f.presentation.present(2);assert.equal(grants,1);assert.equal(f.presentation.slots.get(live.id).view,null);assert.equal(f.client.views.size,0);
});

test('slot reuse increments generation and ignores old terminal, including same-tick retirement',()=>{
 const f=fixture(),first=spawn(f),one=f.capture.capture(1)[0];f.presentation.receive(transfer(one),1);
 f.authority.remove(first);const gone=f.capture.capture(1)[0];assert.equal(f.presentation.receive(transfer(gone),1),true);
 spawn(f,pickup);const two=f.capture.capture(2)[0];assert.equal(two.id,one.id);assert.equal(two.generation,one.generation+1);f.presentation.receive(transfer(two),1);
 assert.equal(f.presentation.receive(transfer(gone),1),false);f.presentation.present(2);assert.equal(f.presentation.slots.get(two.id).view.def,pickup);assert.equal(f.client.views.size,1);
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
 f.presentation.receive(transfer(record),1);f.presentation.present(1);const mesh=f.presentation.slots.get(record.id).view.mesh;f.presentation.reset(2);assert.equal(mesh.parent,null);assert.equal(f.presentation.slots.size,0);assert.equal(f.presentation.receive(transfer(record),1),false);
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

test('a dropped health item falls smoothly on the page, on the world\'s path, and rests where the world settles it',()=>{
 const f=fixture({ground:flat}),net=online(f,7),path=[],frames=[],THROW=181;
 // The world ticks at 60 Hz with a snapshot every 2 ticks, three seconds of
 // battle before the throw; the page draws at 90 frames a second. A walking
 // pilot's throw: 4 m/s along his aim, a little down, plus his own velocity,
 // from the gun.
 let tick=0,item=null,landed=null;
 for(let frame=1;frame<=420;frame++){
  while(tick+1<=frame*60/90){tick++;if(tick===THROW)item=f.authority.dispense('dispenser',4,health,new Vector3(100.37,1.32,-45.81),new Vector3(3.5,-.6,4),.6,{id:1,generation:1},1);
   f.authority.update(1/60,[]);if(item){path[tick]=item.position.clone();if(item.landed)landed??=tick;}if(tick%2===0)net.publish(tick);}
  const shown=net.frame(1/90),view=f.presentation.slots.get(0)?.view;
  if(view)frames.push({tick:shown.tick,behind:shown.latest-shown.tick,drawn:view.mesh.position.clone()});
 }
 assert.ok(landed>THROW+10&&frames[0].tick>=THROW&&frames.at(-1).tick>landed+60,'the world landed it and the page saw it rest');
 const flying=frames.filter(frame=>frame.tick<landed);
 // Behind the newest snapshot by 2 to 5 ticks: the old pair of records held the item still.
 assert.ok(flying.length>30&&flying.every(frame=>frame.behind>=1.5),'drawn well behind the newest snapshot');
 for(let i=1;i<flying.length;i++){
  const {drawn,tick}=flying[i],last=flying[i-1];
  assert.ok(drawn.distanceTo(last.drawn)>1e-4,`frame at tick ${tick.toFixed(2)} moved`);
  assert.ok(drawn.distanceTo(along(path,tick))<.02,`within 2 cm of the world's path at tick ${tick.toFixed(2)}`);
  assert.ok(drawn.distanceTo(last.drawn)<along(path,tick).distanceTo(along(path,last.tick))+.01,'no jump beyond the motion');
 }
 const rest=frames.at(-1).drawn;
 assert.ok(Math.abs(rest.x-item.position.x)<.02&&Math.abs(rest.z-item.position.z)<.02&&Math.abs(rest.y-item.position.y)<.006,'at rest where the world settled it');
});

test('an item appears when the clock reaches its first record and goes when it reaches its removal',()=>{
 const f=fixture(),item=spawn(f,pickup);item.landed=true;const live=transfer(f.capture.capture(10)[0]);f.presentation.receive(live,1);
 f.presentation.present(9.5);assert.equal(f.presentation.slots.get(live.id).view,null,'not before its first record');assert.equal(f.client.views.size,0);
 f.presentation.present(10);const view=f.presentation.slots.get(live.id).view;assert.ok(view.mesh.parent===f.client.scene&&f.client.views.has(view));
 f.authority.remove(item);f.presentation.receive(transfer(f.capture.capture(14)[0]),1);
 f.presentation.present(13.9);assert.ok(view.mesh.parent===f.client.scene,'drawn until the clock reaches its removal');assert.equal(f.presentation.slots.get(live.id).view,view);
 f.presentation.present(14);assert.equal(view.mesh.parent,null);assert.equal(f.presentation.slots.get(live.id).view,null);assert.equal(f.client.views.size,0);
});

test('a slot reused inside the playout delay swaps its items on the clock',()=>{
 const f=fixture({pools:{mine:2,powerupitem:2}}),first=spawn(f,pickup);first.landed=true;const one=transfer(f.capture.capture(10)[0]);
 // Gone and replaced in the same tick: the next generation's first record is the only end the first item gets.
 f.authority.remove(first);spawn(f,mine).landed=true;const two=transfer(f.capture.capture(12)[0]);assert.equal(two.id,one.id);assert.equal(two.generation,one.generation+1);
 f.presentation.receive(one,1);f.presentation.receive(two,1);f.presentation.present(11);
 const entry=f.presentation.slots.get(one.id);assert.equal(entry.view,null,'the next item waits for the clock');assert.deepEqual([...f.client.views].map(view=>view.def),[pickup],'the one before it is still drawn');
 f.presentation.present(12);assert.equal(entry.view.def,mine);assert.deepEqual([...f.client.views].map(view=>view.def),[mine]);
});

test('a jump of the clock starts again from the newest record at or before it, without flying the gap',()=>{
 const f=fixture({ground:flat}),item=f.authority.spawn(health,new Vector3(0,20,0),new Vector3(2,8,1),0),path=[];
 for(let tick=1;tick<=60;tick++){f.authority.update(1/60,[]);path[tick]=item.position.clone();if(tick%2===0)f.presentation.receive(transfer(f.capture.capture(tick)[0]),1);}
 let flights=0;const fly=f.client.fly.bind(f.client);f.client.fly=(...args)=>{flights++;return fly(...args);};
 f.presentation.present(10.5);const view=f.presentation.slots.get(0).view;assert.ok(view.position.distanceTo(along(path,10.5))<.01);
 flights=0;f.presentation.present(40.25);assert.ok(flights<=2,`flew ${flights} ticks for a 30-tick jump`);assert.ok(view.position.distanceTo(along(path,40.25))<.01);
 // A suspended page's clock starts again from 0: the item holds where it was drawn.
 const held=view.position.clone();f.presentation.present(0);assert.ok(view.mesh.parent===f.client.scene&&view.position.equals(held));
 f.presentation.present(50.5);assert.ok(view.position.distanceTo(along(path,50.5))<.01);
});

test('a placed power-up and a stuck mine rest where the world says, even with no ground on the page',()=>{
 const box=()=>{const group=new Group(),geometry=new BoxGeometry(.4,.12,.4);geometry.computeBoundingSphere();group.add(new Mesh(geometry));return group;};
 const f=fixture({ground:flat,clientGround:none,pools:{mine:2,powerupitem:2},model:()=>box});
 const placed=f.authority.spawn(pickup,new Vector3(5,0,5),new Vector3(),1),thrown=f.authority.spawn(mine,new Vector3(-3,1.2,2),new Vector3(2,1,0),.5);
 for(let tick=1;tick<=90;tick++){f.authority.update(1/60,[]);if(tick%2===0)for(const record of f.capture.capture(tick))f.presentation.receive(transfer(record),1);}
 assert.ok(placed.landed&&thrown.landed&&thrown.position.y>.05,'the world stuck the mine on the ground');
 const views=()=>[...f.presentation.slots.values()].map(entry=>entry.view);let low=Infinity;
 for(let tick=2;tick<=120;tick+=.75){f.presentation.present(tick);const [a,b]=views();assert.ok(a.position.distanceTo(placed.position)<.004,'the placed power-up never moves');low=Math.min(low,b.position.y);}
 const [, view]=views();assert.equal(view.radius,thrown.radius);assert.ok(view.position.distanceTo(thrown.position)<.004,'the mine lies where the world stuck it');
 assert.ok(low>-.25,`dipped to ${low.toFixed(3)} at most before the world's rest pulled it up`);
});

test('a mine sinking in water keeps sinking at its rate on the page',()=>{
 const f=fixture();f.presentation.receive({kind:'item',id:0,generation:1,tick:10,active:true,definitionId:1,position:[2,-1,3],velocity:[0,-.1875,0],yaw:0,up:[0,1,0]},1);
 f.presentation.present(10);f.presentation.present(70);assert.ok(Math.abs(f.presentation.slots.get(0).view.position.y+1.2)<.01);
});

test('an item\'s velocity travels at 1/32 m/s, so a slow slide is not rounded to rest',()=>{
 const slide=[.045,0,.051],item={kind:'item',id:0,generation:1,tick:2,active:true,definitionId:3,position:[1,.02,1],velocity:slide,yaw:0,up:[0,1,0]};
 const entry=entryFromRecord(item);assert.equal(entry.vel.wide,true);assert.deepEqual(recordFromEntry(entry,2).velocity,[.03125,0,.0625]);
 assert.equal(entryFromRecord({kind:'soldier',id:1,generation:1,tick:2,classId:1,team:1,position:[0,0,0],velocity:slide,yaw:0,pitch:0,health:1,posture:0,state:0,weapon:0}).vel.wide,false,'other classes keep the narrow form');
});
