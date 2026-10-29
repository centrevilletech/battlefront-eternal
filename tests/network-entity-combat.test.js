import test from 'node:test';import assert from 'node:assert/strict';
import {Scene,Vector3} from 'three';
import {Ordnance} from '../src/ordnance.js';
import {CombatCapture,CombatPresentation,decodeCombat} from '../src/runtime/network-entity-combat.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {PredictedShots} from '../src/runtime/predicted-shots.js';
import {presentEntityEvent,presentationRecord} from '../src/runtime/entity-presentation-events.js';
const def={Velocity:60,LifeSpan:.07123,LaserLength:2,ImpactEffectStatic:'spark'};
function fixture(){let tick=10;const capture=new CombatCapture({tick:()=>tick,definitionId:()=>1});const effects=[];
 const authority=new Ordnance({scene:new Scene(),collision:{cast:()=>({distance:3,point:new Vector3(0,0,3),normal:new Vector3(0,0,-1)})},effects:{play(){}},lights:{add(){}},texture:()=>null});
 authority.onLaunch=b=>capture.launch(b);authority.onImpact=(b,h)=>capture.impact(b,h);
 const client=new Ordnance({scene:new Scene(),collision:{cast(){throw Error('client collision forbidden');}},effects:{play:(...x)=>effects.push(x)},lights:{add(){}},texture:()=>null});
 client.onHit=client.onHitUnit=()=>{throw Error('client damage forbidden');};
 let confirmed=0,predict=false,muzzles=[];const presentation=new CombatPresentation({ordnance:client,definition:()=>def,predictedShots:{claim(){confirmed++;return predict;}},muzzle:(event,d)=>{muzzles.push([event.ownerId,d]);return muzzle;}});let muzzle=null;
 return{authority,client,capture,presentation,effects,muzzles,setTick:n=>tick=n,confirmed:()=>confirmed,predict:value=>{predict=value;},muzzle:at=>{muzzle=at;}};}
const transfer=record=>decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:1,tick:record.tick,records:[record]})).records[0];

function heldFixture(){
 const f=fixture();let now=0;
 const weapon={name:'rifle',def:{ordnance:{...def,base:'bolt'},ShotDelay:.1},canShoot:true,wait:0,levels:[{}]};
 const unit={id:7,generation:2,alive:true,combat:{origin:new Vector3(),arsenal:{current:()=>weapon,weapons:[weapon]},firePoint(){},flash(){},presentProjectileShot(){throw Error('predicted flash replayed');}}};
 const shots=new PredictedShots(new Scene(),null,()=>now);
 for(let i=0;i<4;i++){now=i*.11;assert.equal(shots.fire({pressed:true,held:true,button:0,id:0,aimYaw:0,aimPitch:0},unit),true);}
 f.presentation.predictedShots=shots;
 const game={units:{byId:id=>id===unit.id?unit:null},predictedShots:shots},registry={weapons:new Map([[1,'rifle']])};
 f.presentation.onPresentation=event=>presentEntityEvent(game,event,registry);
 f.authority.collision.cast=()=>null;
 const launch=(executionTick,viewTick=90)=>{
  f.setTick(executionTick);f.authority.onLaunch=bolt=>f.capture.launch(bolt,{tick:viewTick});
  const bolt=f.authority.bolt({...def,LifeSpan:2},new Vector3(),new Vector3(0,0,1),unit);
  return {bolt,record:transfer(f.capture.drain()[0].record)};
 };
 const sound=(executionTick,id=1000)=>presentationRecord({id,generation:1,tick:executionTick,event:{kind:'shot',ownerId:7,ownerGeneration:2,weaponId:1,fireId:0}});
 const claimed=()=>[...shots.shots.values()].filter(shot=>shot.confirmedTick!==null).length;
 return {...f,shots,launch,sound,claimed};
}

test('a held bolt, shot presentation and impact claim one cosmetic despite lag compensation',()=>{
 for(const soundFirst of [false,true]) {
  const f=heldFixture(),{bolt,record}=f.launch(100);
  assert.equal(decodeCombat(record).launchTick,90);assert.equal(decodeCombat(record).executionTick,100);
  const sound=f.sound(100),receive=record=>f.presentation.receive(record,{epoch:1,tick:100});
  for(const event of soundFirst?[sound,record]:[record,sound])receive(event);
  assert.equal(f.claimed(),1);
  f.setTick(105);f.capture.impact(bolt,{point:new Vector3(0,0,3),normal:new Vector3(0,0,-1),surface:5});
  const terminal=transfer(f.capture.drain()[0].record);
  assert.equal(decodeCombat(terminal).executionTick,100);assert.equal(terminal.tick,105);
  f.presentation.receive(terminal,{epoch:1,tick:105});assert.equal(f.claimed(),1);
  f.shots.clear();
 }
});

test('automatic rounds sharing a stale view tick remain distinct while pellets share one claim',()=>{
 const f=heldFixture();let flashes=0;f.presentation.onLaunch=()=>flashes++;
 for(const executionTick of [100,106]) {
  for(let pellet=0;pellet<3;pellet++) {
   const {record}=f.launch(executionTick,90);
   f.presentation.receive(record,{epoch:1,tick:executionTick});
  }
  f.presentation.receive(f.sound(executionTick,1000+executionTick),{epoch:1,tick:executionTick});
 }
 assert.equal(f.claimed(),2);assert.equal(flashes,2);assert.equal(f.presentation.records.size,8);
 f.shots.clear();
});

test('terminal arriving before launch and sound claims the original held round once',()=>{
 const f=heldFixture(),{bolt,record}=f.launch(100);
 f.setTick(105);f.capture.impact(bolt,{point:new Vector3(),normal:new Vector3(0,1,0),surface:5});
 f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:105});
 assert.equal(f.claimed(),1);
 assert.equal(f.presentation.receive(record,{epoch:1,tick:105}),false);
 f.presentation.receive(f.sound(100),{epoch:1,tick:105});assert.equal(f.claimed(),1);
 f.shots.clear();
});

test('held dynamic projectile updates share execution identity with the shot presentation',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');
 const f=heldFixture();
 for(const tick of [100,103,106]) {
  const record=projectileRecord({id:1,generation:1,tick,type:'beam',definitionId:1,ownerId:7,ownerGeneration:2,fireId:0,launchTick:100,position:[0,0,0],life:1});
  f.presentation.receive(record,{epoch:1,tick});
 }
 f.presentation.receive(f.sound(100),{epoch:1,tick:106});assert.equal(f.claimed(),1);f.shots.clear();
});

test('simulation 19 rejects old bolt payloads and preserves the fractional physical flight clock',()=>{
 const f=heldFixture(),{record}=f.launch(100,90.25);
 assert.equal(record.payload.length,130);assert.equal(record.tick,100);
 for(const length of [121,125])assert.throws(()=>decodeCombat({...record,payload:record.payload.slice(0,length)}),/invalid combat payload/);
 const invalid={...record,payload:record.payload.slice()};new DataView(invalid.payload.buffer).setUint32(125,89,true);
 assert.throws(()=>decodeCombat(invalid),/execution tick/);
 f.presentation.predictedShots=null;f.presentation.receive(record,{epoch:1,tick:100});
 const bolt=f.presentation.records.get(record.id).bolt;assert.equal(bolt.networkFlightTick,90.25);
 f.presentation.present(90);f.presentation.present(100);assert.equal(f.client.drawnAt.z,9.75,'flight preserves the fraction of the rewound origin');
 // A launch this page's playout clock has already passed (rewound further
 // back than the playout delay) starts from the gun when first drawn.
 const late=f.launch(101,90.25).record;f.presentation.receive(late,{epoch:1,tick:101});
 f.presentation.present(96.5);assert.equal(f.presentation.records.get(late.id).bolt.networkFlightTick,96.5);assert.equal(f.client.drawnAt.z,0);
 f.presentation.present(98.5);assert.equal(f.client.drawnAt.z,2);
 f.shots.clear();
});

test('simultaneous primary and secondary presses retain independent IDs through bolt and sound capture',async()=>{
 const {FireCommands}=await import('../src/runtime/fire-commands.js');
 const {attachPresentationCapture}=await import('../src/runtime/entity-presentation-events.js');
 const {SimEvents}=await import('../src/sim-events.js');
 const f=fixture();f.authority.collision.cast=()=>null;
 const weapons=[0,1].map(channel=>({name:`gun${channel}`,channel,def:{...def,base:'bolt'}}));
 const owner={id:7,generation:2,alive:true,combat:{arsenal:{weapons}}},fire=new FireCommands(),events=new SimEvents();
 for(const channel of [0,1])assert.equal(fire.push({epoch:1,id:11+channel,tick:10,generation:2,button:channel,pressed:true,viewTick:8.5,aimYaw:channel,aimPitch:0},10,2),true);
 owner.networkShots=fire.take(10,2).shots;
 const dispose=attachPresentationCapture({simEvents:events},f.capture,{weaponIds:new Map([['gun0',1],['gun1',2]])});
 for(const weapon of weapons){
  f.authority.fire(weapon.def,new Vector3(),new Vector3(0,0,1),owner,null,null,null,{weapon});
  events.emit('shot',{source:{unit:owner,generation:2},weapon:weapon.name,position:new Vector3()});
 }
 const records=f.capture.drain().map(({record})=>decodeCombat(transfer(record)));
 assert.deepEqual(records.map(r=>[r.channel,r.fireId]),[[0,11],[0,11],[1,12],[1,12]]);
 dispose();
});
test('actual authoritative bolt launches, renders cosmetic flight, impacts once without client simulation',()=>{
 const f=fixture(),bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),{id:7,generation:2,networkShot:{id:9}});
 const launch=transfer(f.capture.drain()[0].record);assert.equal(decodeCombat(launch).life,.05);
 f.presentation.receive(launch,{epoch:1,tick:10});f.presentation.present(10);f.presentation.present(11);assert.equal(f.client.drawnAt.z,1);assert.equal(f.confirmed(),1);
 assert.deepEqual(f.muzzles,[[7,def]],'drawn from the shooter\'s gun as this page draws it');
 f.setTick(13);f.authority.stepBolt(bolt,.06,false);const terminal=transfer(f.capture.drain()[0].record);
 // The terminal arrives before this page's clock reaches the impact: the
 // bolt flies on to the impact point and the impact shows when it gets there.
 f.presentation.receive(terminal,{epoch:1,tick:13});assert.equal(f.effects.length,0);f.presentation.receive(terminal,{epoch:1,tick:14});
 f.client.drawnAt.set(0,0,0);f.presentation.present(12);assert.ok(Math.abs(f.client.drawnAt.z-2)<1e-9,'still in flight');assert.equal(f.effects.length,0);
 f.presentation.present(13);assert.equal(f.effects.length,1);f.presentation.present(14);assert.equal(f.effects.length,1);
 assert.equal(f.presentation.records.get(launch.id).schema,2);assert.ok(f.presentation.records.get(launch.id).bolt,'the terminal keeps the bolt it ends');
});
test('a launch the player predicted is not drawn twice: his cosmetic round flies as drawn',()=>{
 const f=fixture();f.predict(true);f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),{id:7,generation:2,networkShot:{id:9}});
 const launch=transfer(f.capture.drain()[0].record);f.presentation.receive(launch,{epoch:1,tick:10});
 f.client.drawnAt.set(9,9,9);f.presentation.present(11);assert.deepEqual(f.client.drawnAt.toArray(),[9,9,9],'no authoritative bolt drawn');
 assert.equal(f.presentation.records.get(launch.id).predicted,true);assert.equal(f.presentation.records.get(launch.id).bolt,undefined);assert.equal(f.muzzles.length,0);
});
test('a remote launch leaves the gun where this page draws it and joins its path; a muzzle far from the launch is ignored',()=>{
 const f=fixture();f.muzzle(new Vector3(.3,-.2,.5));f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);
 const launch=transfer(f.capture.drain()[0].record);f.presentation.receive(launch,{epoch:1,tick:10});
 f.presentation.present(10);const bolt=f.presentation.records.get(launch.id).bolt;assert.deepEqual(bolt.offset.toArray(),[.3,-.2,.5]);assert.equal(bolt.visualMuzzle,null,'resolved once');
 assert.ok(Math.abs(f.client.drawnAt.x-.3)<1e-9,'at launch the bolt is drawn at the muzzle');
 f.muzzle(new Vector3(0,0,-30));f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);const far=transfer(f.capture.drain()[0].record);f.presentation.receive(far,{epoch:1,tick:10});f.presentation.present(10);
 assert.deepEqual(f.presentation.records.get(far.id).bolt.offset.toArray(),[0,0,0]);
});
test('terminal before launch prevents revival, survives cosmetic TTL, and expires bounded identity',()=>{
 const f=fixture(),bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);const launch=f.capture.drain()[0].record;
 f.setTick(13);f.authority.stepBolt(bolt,.06,false);const terminal=f.capture.drain()[0].record;
 f.presentation.receive(terminal,{epoch:1,tick:40});assert.equal(f.effects.length,0);
 assert.equal(f.presentation.receive(launch,{epoch:1,tick:40}),false);assert.equal(f.presentation.records.size,1);
 f.presentation.present(200);assert.equal(f.presentation.records.size,0);assert.equal(f.presentation.receive(launch,{epoch:1,tick:200}),false);
});
test('pellets sharing one fire edge have separate projectile identities and exact fractional lifetime',()=>{
 const f=fixture();f.authority.collision.cast=()=>null;
 for(let i=0;i<2;i++)f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),{id:7,generation:2,networkShot:{id:9}});
 const records=f.capture.drain().map(x=>decodeCombat(transfer(x.record)));assert.notEqual(records[0].id,records[1].id);assert.equal(records[0].fireId,records[1].fireId);assert.equal(records[0].life,.07123);
});

test('authoritative hit feedback survives packet duplication and never changes target health',async()=>{
 const {presentFeedback}=await import('../src/runtime/network-combat-feedback.js');
 const f=fixture(),attacker={id:7,generation:2,team:1},target={id:8,generation:3,team:2,combat:{health:70}};let hits=0,kills=0;
 const game={playerUnit:attacker,units:{byId:id=>id===7?attacker:target,relation:()=>-1},hud:{hit(){hits++;},killMessage(){kills++;}}};
 f.presentation.onFeedback=event=>presentFeedback(game,event);
 for(const kind of ['damage','death']){
  f.capture.event({kind,target:{unit:target,generation:3},attacker:{unit:attacker,generation:2},team:2,attackerTeam:1,position:[1,2,3]});
  const record=transfer(f.capture.drain()[0].record);f.presentation.receive(record,{epoch:1,tick:10});f.presentation.receive(record,{epoch:1,tick:10});
 }
 assert.equal(hits,1);assert.equal(kills,1);assert.equal(target.combat.health,70);
});

test('late old packet cannot resurrect an expired identity after bounded GC',()=>{
 const f=fixture();f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);const launch=f.capture.drain()[0].record;
 f.presentation.receive(launch,{epoch:1,tick:10});f.presentation.present(200);
 assert.equal(f.presentation.receive(launch,{epoch:1,tick:10}),false);assert.equal(f.presentation.records.size,0);
 f.presentation.reset(2);assert.equal(f.presentation.receive(launch,{epoch:1,tick:10}),false);
});
test('capture failures report bounded diagnostics without changing shared simulation',async()=>{
 const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');const {SimEvents}=await import('../src/sim-events.js');
 const f=fixture();f.capture.definitionId=()=>{throw Error('unknown definition');};
 // Restore ordinary hooks, then attach the networking observer as production does.
 f.authority.onLaunch=()=>{};f.authority.onImpact=()=>{};
 const dispose=attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 for(let i=0;i<12;i++)assert.doesNotThrow(()=>f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null));
 assert.equal(f.authority.bolts.length,12);assert.equal(f.capture.drainFaults().length,8);dispose();
});

test('multi-pellet volley draws every bolt but emits its muzzle presentation once',()=>{
 const f=fixture();f.authority.collision.cast=()=>null;let flashes=0;f.presentation.onLaunch=()=>flashes++;
 for(let i=0;i<3;i++)f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),{id:7,generation:2,networkShot:{id:9}});
 for(const {record}of f.capture.drain())f.presentation.receive(transfer(record),{epoch:1,tick:10});
 assert.equal(f.presentation.records.size,3);assert.equal(flashes,1);
});

test('shared Ordnance impact and shared Combat.damage produce feedback without replaying client damage',async()=>{
 const {PlayerCombat}=await import('../src/combat.js');const {presentFeedback}=await import('../src/runtime/network-combat-feedback.js');
 const f=fixture(),attacker={id:7,generation:2,team:1},target={id:8,generation:3,team:2,classDef:{}};let died=0,hits=0,kills=0;
 const combat=Object.create(PlayerCombat.prototype);target.combat=combat;
 Object.assign(combat,{soldier:{air:null},health:10,dead:false,chunks:0,animator:{die(){died++;}},updateHeld(){},onDamaged(amount,source){f.capture.event({kind:'damage',target:{unit:target,generation:3},attacker:{unit:source,generation:2},team:2,attackerTeam:1,position:[0,0,1]});},onDeath(){f.capture.event({kind:'death',target:{unit:target,generation:3},attacker:{unit:attacker,generation:2},team:2,attackerTeam:1,position:[0,0,1]});}});
 f.authority.castUnits=()=>({unit:target,point:new Vector3(0,0,1),normal:new Vector3(0,0,-1),part:'body'});
 f.authority.onHitUnit=(unit,def,point,owner)=>unit.combat.damage(20,20,point,owner);
 const bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),attacker);f.capture.drain();f.setTick(11);f.authority.stepBolt(bolt,1/60,false);
 assert.equal(combat.health,0);assert.equal(died,1);
 f.presentation.onFeedback=event=>presentFeedback({playerUnit:attacker,units:{byId:id=>id===7?attacker:target,relation:()=>-1},hud:{hit(){hits++;},killMessage(){kills++;}}},event);
 for(const {record}of f.capture.drain())f.presentation.receive(transfer(record),{epoch:1,tick:11});
 assert.equal(hits,1);assert.equal(kills,1);assert.equal(combat.health,0);assert.equal(died,1);
});

test('shared projectile presentation changes visuals without advancing weapons or recoil',async()=>{
 const {PlayerCombat}=await import('../src/combat.js');const calls=[];
 const combat=Object.create(PlayerCombat.prototype),weapon={bank:'rifle',thrown:false};
 Object.assign(combat,{soldier:{posture:'stand'},kick:{rate:2,offset:1,hold:.1},firstPerson:true,animator:{weaponAction(...x){calls.push(['animation',...x]);},fire(){calls.push(['fire']);}},view:{play(...x){calls.push(['view',...x]);},flash(...x){calls.push(['viewFlash',...x]);}},flash(...x){calls.push(['flash',...x]);},ordnance:{fire(){throw Error('must not create projectile');}}});
 const kick={...combat.kick};combat.presentProjectileShot(weapon);
 assert.equal(calls.length,5);assert.deepEqual(combat.kick,kick);assert.deepEqual(weapon,{bank:'rifle',thrown:false});
});

test('same-tick terminal ends an already displayed launch where it hits, not before it is drawn',()=>{
 const f=fixture(),bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);
 const launch=f.capture.drain()[0].record;f.presentation.receive(transfer(launch),{epoch:1,tick:10});
 f.authority.stepBolt(bolt,.06,false);const terminal=f.capture.drain()[0].record;assert.equal(terminal.tick,launch.tick);
 assert.equal(f.presentation.receive(transfer(terminal),{epoch:1,tick:10}),true);assert.equal(f.presentation.records.get(launch.id).schema,2);assert.equal(f.effects.length,0);
 f.presentation.present(10);f.presentation.present(11);assert.equal(f.client.drawnAt.z,1,'drawn on its way to the impact');
 f.presentation.present(13);assert.equal(f.effects.length,1);
});
test('a bolt that ends before the next drain keeps its launch beside its terminal',()=>{
 const f=fixture(),bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);f.authority.stepBolt(bolt,.06,false);
 const records=f.capture.drain().map(x=>transfer(x.record));assert.deepEqual(records.map(r=>r.schema),[1,2]);assert.equal(records[0].id,records[1].id);
 for(const record of records)f.presentation.receive(record,{epoch:1,tick:10});
 f.presentation.present(10);f.presentation.present(11);assert.equal(f.client.drawnAt.z,1,'the short shot is drawn, not just its impact');
 f.presentation.present(13);assert.equal(f.effects.length,1);
 // A launch repeated after its terminal (its first copy lost) is not drawn twice.
 assert.equal(f.presentation.receive(records[0],{epoch:1,tick:12}),false);
});
test('a launch lost and sent again after its terminal still flies to that impact',()=>{
 const f=fixture(),bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);f.authority.stepBolt(bolt,.06,false);
 const [launch,terminal]=f.capture.drain().map(x=>transfer(x.record));
 f.presentation.receive(terminal,{epoch:1,tick:12});f.presentation.present(8);assert.equal(f.effects.length,0,'the impact waits for its tick');
 assert.equal(f.presentation.receive(launch,{epoch:1,tick:14}),true);
 f.presentation.present(9);f.client.drawnAt.set(9,9,9);f.presentation.present(10);assert.equal(f.client.drawnAt.z,0);f.presentation.present(11);assert.equal(f.client.drawnAt.z,1);
 f.presentation.present(13);assert.equal(f.effects.length,1,'its impact shows once, as it arrives');
});

test('impact-only short shot still presents its muzzle once, late expiry does not',()=>{
 const f=fixture();let flashes=0;f.presentation.onLaunch=()=>flashes++;
 const bolt=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);f.authority.stepBolt(bolt,.06,false);
 const terminal=f.capture.drain()[1].record;assert.equal(terminal.schema,2);f.presentation.receive(transfer(terminal),{epoch:1,tick:10});assert.equal(flashes,1);
 const later=f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);f.capture.drain();f.setTick(40);f.authority.stepBolt(later,.06,false);
 assert.equal(f.capture.pending.size,1,'a drained launch leaves its terminal alone');
 f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:40});assert.equal(flashes,1);
});

test('replicated tracers join the shared renderer batch before it is finalized',()=>{
 const f=fixture();f.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);f.presentation.receive(f.capture.drain()[0].record,{epoch:1,tick:10});
 let active=false,draws=0;const begin=f.client.renderer.begin.bind(f.client.renderer),end=f.client.renderer.end.bind(f.client.renderer),draw=f.client.drawBolt.bind(f.client);
 f.client.renderer.begin=()=>{active=true;begin();};f.client.renderer.end=()=>{assert.equal(draws,1);end();active=false;};
 f.client.drawBolt=(...args)=>{assert.equal(active,true,'draw must survive the same-frame batch reset');draws++;return draw(...args);};
 f.client.present(1,11,()=>f.presentation.present(11));assert.equal(active,false);
});

test('damage feedback snapshots the actual source and shares single-player HUD orientation',async()=>{
 const {PlayerCombat}=await import('../src/combat.js');
 const {SimEvents}=await import('../src/sim-events.js');
 const {presentFeedback}=await import('../src/runtime/network-combat-feedback.js');
 const f=fixture(),events=new SimEvents(),target={id:8,generation:3,team:2,position:new Vector3(100,0,100),soldier:{aimYaw:0}},angles=[];
 events.listen(event=>f.capture.event(event));
 const combat=Object.create(PlayerCombat.prototype);Object.assign(combat,{soldier:{},health:100,dead:false,player:false,onDamaged(amount,attacker,info){events.emit('damage',{target:{unit:target,generation:3},team:2,position:new Vector3(0,0,0),source:info.source});}});
 const source=new Vector3(1,0,0);combat.damage(1,1,source,null);source.set(-10,0,0);
 const record=transfer(f.capture.drain()[0].record),decoded=decodeCombat(record);assert.deepEqual(decoded.source,[1,0,0]);
 const game={playerUnit:target,units:{byId:id=>id===8?target:null},hud:{damageFrom:angle=>angles.push(angle)}};
 presentFeedback(game,decoded);assert.equal(angles[0],-Math.PI/2,'right-side world source uses shared HUD convention despite target moving and attacker absent');
 target.soldier.aimYaw=Math.PI/2;presentFeedback(game,decoded);assert.ok(Math.abs(angles[1])<1e-12,'current view rotates the saved source direction');
 assert.equal(combat.health,99);
});

test('shared explosion presentation replicates effects and lights without client blast damage',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');
 const f=fixture(),def={Effect:'blast',WaterEffect:'wet-blast',Damage:100,DamageRadiusOuter:10,LightRadius:4,LightDuration:.2};let blasts=0,flashes=0,ripples=0;
 f.authority.onExplode=()=>blasts++;f.authority.lights.flash=()=>{};
 f.authority.water={level:0,over:()=>true,ripple(){ripples++;}};
 attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 f.client.lights.flash=()=>flashes++;f.client.onExplode=()=>{throw Error('network cosmetic must never blast damage');};
 f.presentation.definition=()=>def;
 f.authority.explosion(def,new Vector3(1,-1,2),null);
 assert.equal(blasts,1);assert.equal(ripples,1);
 const record=transfer(f.capture.drain()[0].record);assert.equal(record.schema,5);assert.equal(decodeCombat(record).wet,true);
 assert.equal(f.presentation.receive(record,{epoch:1,tick:10}),true);assert.equal(flashes,0,'shown when the playout clock reaches it');
 f.presentation.present(9);assert.equal(flashes,0);f.presentation.present(10);
 assert.equal(f.effects[0][0],'wet-blast','authority water choice survives missing client water');assert.equal(flashes,1);
 assert.equal(f.presentation.receive(record,{epoch:1,tick:10}),false);f.presentation.present(11);assert.equal(flashes,1);
 const late=fixture();late.presentation.definition=()=>def;late.client.lights.flash=()=>{throw Error('late explosion flashed');};
 assert.equal(late.presentation.receive(record,{epoch:1,tick:29}),false);
});

test('death feedback expires with other short cosmetics instead of replaying after a stall',()=>{
 const f=fixture();f.capture.event({kind:'death',target:{unit:{id:8},generation:1},position:[0,0,0]});
 const record=transfer(f.capture.drain()[0].record);assert.equal(decodeCombat(record).expiryTick,28);
 assert.equal(f.presentation.receive(record,{epoch:1,tick:29}),false);
});

test('matched combat registry includes projectile and nested explosion definitions',async()=>{
 const {createCombatRegistry}=await import('../src/runtime/network-combat-registry.js');
 const explosion={Effect:'blast',Damage:100},projectile={Velocity:20,LifeSpan:2,ExplosionExpire:explosion};
 const registry=createCombatRegistry({projectile});
 assert.equal(registry.definitions.get(registry.definitionId(explosion)),explosion);
 assert.equal(registry.definitions.get(registry.definitionId(projectile)),projectile);
});

test('actual grenade and shell samples render authoritative bounce/flight state without client simulation',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');const {Object3D}=await import('three');
 const f=fixture();f.authority.collision.cast=()=>null;f.client.model=()=>()=>new Object3D();
 attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 const definition={base:'grenade',GeometryName:'grenade',Velocity:6,LifeSpan:1,Gravity:1};f.presentation.definition=()=>definition;f.presentation.dynamic.definition=()=>definition;
 f.authority.throw(definition,new Vector3(0,10,0),new Vector3(0,0,1),null);
 const grenade=f.authority.grenades[0];let record=f.capture.drain()[0].record;
 assert.equal(record.schema,6);f.presentation.receive(transfer(record),{epoch:1,tick:10});
 f.authority.stepGrenade(grenade,.05);f.setTick(13);record=f.capture.drain()[0].record;
 const decoded=decodeCombat(transfer(record));assert.ok(Math.abs(decoded.position[1]-grenade.position.y)<1e-6);assert.ok(Math.abs(decoded.quaternion[3])<1,'headless authority retains visible grenade tumble');
 f.presentation.receive(transfer(record),{epoch:1,tick:13});f.presentation.present(13);
 const view=f.presentation.dynamic.objects.get(record.id);assert.ok(view.mesh.position.distanceTo(grenade.position)<1e-6);
 f.authority.explode(grenade);f.setTick(14);const terminal=f.capture.drain()[0].record;assert.equal(terminal.schema,7);
 f.presentation.receive(transfer(terminal),{epoch:1,tick:14});assert.equal(f.presentation.dynamic.objects.size,1,'ends when the playout clock reaches it');
 f.presentation.present(14);assert.equal(f.presentation.dynamic.objects.size,0);assert.equal(view.mesh.parent,null);
 assert.equal(f.presentation.receive(transfer(record),{epoch:1,tick:16}),false,'late sample cannot revive retired projectile');
 const shellDef={...definition,base:'shell'};const shell=f.authority.round(shellDef,new Vector3(0,10,0),new Vector3(0,0,1),null);f.capture.drain();f.authority.stepRound(shell,.05);f.setTick(17);
 const shellSample=f.capture.drain().map(x=>decodeCombat(x.record)).find(x=>x.type==='round');assert.ok(shellSample.velocity[1]<0);assert.ok(Math.abs(shellSample.position[2]-shell.position.z)<1e-6);
});

test('beam uses sampled authoritative endpoints and disappears on lifecycle terminal',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');
 const f=fixture();f.authority.collision.cast=()=>({distance:20,point:new Vector3(0,0,20),normal:new Vector3(0,0,-1)});attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 const definition={base:'beam',LifeSpan:.2};f.presentation.dynamic.definition=()=>definition;
 f.authority.beam(definition,new Vector3(),new Vector3(0,0,1),null,null);const beam=f.authority.beams[0];f.authority.stepBeam(beam,1/60,false);
 const record=f.capture.drain()[0].record;f.presentation.receive(transfer(record),{epoch:1,tick:10});
 let draws=0;f.client.drawBeam=sample=>{draws++;assert.equal(sample.end.z,20);};f.presentation.present(10);assert.equal(draws,1);
 beam.life=0;f.setTick(11);f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:11});f.presentation.present(11);assert.equal(draws,1);
});

test('lost dynamic terminal expires mesh and late samples cannot restart expired flight',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');const {Object3D}=await import('three');
 const f=fixture();f.client.model=()=>()=>new Object3D();f.presentation.dynamic.definition=()=>({GeometryName:'missile'});
 const record=projectileRecord({id:1,generation:1,tick:10,type:'round',definitionId:1,position:[0,0,0],life:2});
 f.presentation.receive(transfer(record),{epoch:1,tick:10});const mesh=f.presentation.dynamic.objects.get(1).mesh;
 f.presentation.present(29);assert.equal(f.presentation.dynamic.objects.size,0);assert.equal(mesh.parent,null);
 assert.equal(f.presentation.receive(transfer(record),{epoch:1,tick:10}),false);assert.equal(f.presentation.dynamic.objects.size,0);
});

test('dynamic shot confirmation keeps the original fire identity across later samples',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');
 const f=fixture();f.authority.collision.cast=()=>null;attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 const owner={id:7,generation:2,networkShot:{id:9}},definition={base:'missile',Velocity:20,LifeSpan:2};let flashes=0;f.presentation.onLaunch=()=>flashes++;
 f.authority.round(definition,new Vector3(),new Vector3(0,0,1),owner);owner.networkShot={id:10};
 let record=f.capture.drain()[0].record;assert.equal(decodeCombat(record).fireId,9);f.presentation.receive(transfer(record),{epoch:1,tick:10});
 f.setTick(13);record=f.capture.drain()[0].record;f.presentation.receive(transfer(record),{epoch:1,tick:13});
 assert.equal(flashes,1);assert.equal(f.confirmed(),2,'both samples refer to the same original prediction identity');
 f.presentation.reset(2);assert.equal(f.presentation.dynamic.objects.size,0);
});

test('actual missile snapshots recover after lost intermediate samples without replaying guidance locally',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');const {Object3D}=await import('three');
 const f=fixture();f.authority.collision.cast=()=>null;attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);f.client.model=()=>()=>new Object3D();
 const definition={base:'missile',GeometryName:'missile',Velocity:20,LifeSpan:2,TurnRate:20,WaverTurn:2,ScaleTime:1};f.presentation.dynamic.definition=()=>definition;
 const missile=f.authority.round(definition,new Vector3(0,10,0),new Vector3(0,0,1),null);
 const first=f.capture.drain()[0].record;f.presentation.receive(transfer(first),{epoch:1,tick:10});
 for(let i=1;i<=6;i++){f.setTick(10+i);f.authority.stepRound(missile,1/60);if(i===3)f.capture.drain();}
 const latest=f.capture.drain()[0].record;f.presentation.receive(transfer(latest),{epoch:1,tick:16});f.presentation.present(16);
 const visible=f.presentation.dynamic.objects.get(latest.id);assert.ok(visible.mesh.position.distanceTo(missile.position)<1e-5);
 assert.equal(f.presentation.receive(transfer(first),{epoch:1,tick:19}),false);assert.ok(visible.mesh.position.distanceTo(missile.position)<1e-5);
 assert.equal(f.capture.drainFaults().length,0);
});

test('dynamic presentation brakes after three ticks and caps its short coast at six',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');const f=fixture();
 const record=projectileRecord({id:1,generation:1,tick:10,type:'round',definitionId:1,position:[0,0,0],velocity:[60,0,0],life:2});
 f.presentation.receive(transfer(record),{epoch:1,tick:10});f.presentation.present(12);assert.equal(f.presentation.dynamic.objects.get(1).position.x,2);
 f.presentation.present(14);assert.ok(Math.abs(f.presentation.dynamic.objects.get(1).position.x-23/6)<1e-9);
 f.presentation.present(20);assert.equal(f.presentation.dynamic.objects.get(1).position.x,4.5);
 f.presentation.present(27);assert.equal(f.presentation.dynamic.objects.get(1).position.x,4.5);
});

test('a twelve-tick missile sample gap with six-tick playout delay resumes without a multi-metre snap',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');const f=fixture();
 const record=(tick,x)=>projectileRecord({id:1,generation:1,tick,type:'round',definitionId:1,position:[x,0,0],velocity:[60,0,0],life:2});
 f.presentation.receive(transfer(record(10,0)),{epoch:1,tick:10});
 for(let tick=10;tick<=15;tick++)f.presentation.present(tick);
 const p=f.presentation.dynamic.objects.get(1),before=p.position.x;
 f.presentation.receive(transfer(record(22,12)),{epoch:1,tick:22});f.presentation.present(16);
 assert.ok(p.position.x>before&&p.position.x-before<.5,'continue prior braking motion, not a three-metre arrival jump');
 assert.ok(p.offset.length()>1&&p.offset.length()<=6,'visual correction is spatially bounded');
 assert.equal(p.samples.at(-1).position[0],12,'retained authority sample is untouched');
 let previous=p.position.x;
 for(let tick=17;tick<=38;tick++){f.presentation.present(tick);assert.ok(p.position.x>=previous&&p.position.x-previous<1.5);previous=p.position.x;}
 assert.ok(Math.abs(p.position.x-16.5)<.02,'correction converges onto the bounded authoritative coast');
});

test('grenade bounce, authoritative stop and terminal discard prior trajectory smoothing',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');const {Object3D}=await import('three');
 for(const velocity of [[-60,0,0],[0,0,0]]){
  const f=fixture();f.client.model=()=>()=>new Object3D();f.presentation.dynamic.definition=()=>({GeometryName:'grenade'});
  const record=(tick,x,v=[60,0,0],terminal=false)=>projectileRecord({id:1,generation:1,tick,type:'grenade',definitionId:1,position:[x,0,0],velocity:v,life:2,terminal});
  f.presentation.receive(transfer(record(10,0)),{epoch:1,tick:10});f.presentation.present(15);
  f.presentation.receive(transfer(record(22,12)),{epoch:1,tick:22});f.presentation.present(16);
  const p=f.presentation.dynamic.objects.get(1);assert.ok(p.offset.length()>0);
  f.presentation.receive(transfer(record(24,13,velocity)),{epoch:1,tick:24});f.presentation.present(24);
  assert.equal(p.position.x,13);assert.equal(p.offset.length(),0,'bounce/stop cannot inherit the old trajectory offset');
  f.presentation.present(25);assert.ok(p.position.x<=13,'no continuation into the surface after bounce/stop');
  f.presentation.receive(transfer(record(26,13,velocity,true)),{epoch:1,tick:26});f.presentation.present(26);
  assert.equal(f.presentation.dynamic.objects.size,0);assert.equal(p.mesh.parent,null,'terminal removes the smoothed mesh at its tick');
  assert.equal(f.presentation.receive(transfer(record(25,12,velocity)),{epoch:1,tick:27}),false,'reordered sample cannot resurrect terminal');
 }
});

test('dynamic coast respects remaining lifetime and large corrections do not drag a stale trail',async()=>{
 const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');const f=fixture();
 const record=(tick,x,life=2)=>projectileRecord({id:1,generation:1,tick,type:'round',definitionId:1,position:[x,0,0],velocity:[60,0,0],life});
 f.presentation.receive(transfer(record(10,0,1/60)),{epoch:1,tick:10});f.presentation.present(15);
 const p=f.presentation.dynamic.objects.get(1);assert.ok(Math.abs(p.position.x-1)<1e-6);
 f.presentation.receive(transfer(record(22,100)),{epoch:1,tick:22});f.presentation.present(22);
 assert.equal(p.position.x,100);assert.equal(p.offset.length(),0);
});

test('actual shell collision replaces its sampled body with one authoritative impact',async()=>{
 const {SimEvents}=await import('../src/sim-events.js');const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');
 const f=fixture();f.authority.collision.cast=()=>null;attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},f.capture);
 const definition={base:'shell',Velocity:20,LifeSpan:2,ImpactEffectStatic:'spark'};f.presentation.definition=()=>definition;f.presentation.dynamic.definition=()=>definition;
 const shell=f.authority.round(definition,new Vector3(0,10,0),new Vector3(0,0,1),null);const sample=f.capture.drain()[0].record;f.presentation.receive(transfer(sample),{epoch:1,tick:10});
 f.authority.collision.cast=()=>({distance:.1,point:new Vector3(0,10,.1),normal:new Vector3(0,0,-1)});f.setTick(11);f.authority.stepRound(shell,1/60);
 const terminal=f.capture.drain()[0].record;assert.equal(terminal.schema,2);assert.equal(terminal.id,sample.id);
 f.presentation.receive(transfer(terminal),{epoch:1,tick:11});assert.equal(f.effects.length,0);
 f.presentation.present(10.5);const body=f.presentation.dynamic.objects.get(sample.id);assert.ok(body.position.z>0&&body.position.z<.1,'flown on toward the impact point');
 f.presentation.present(11);assert.equal(f.presentation.dynamic.objects.size,0);assert.equal(f.effects.length,1);
 assert.equal(f.presentation.receive(transfer(sample),{epoch:1,tick:12}),false);f.presentation.present(12);assert.equal(f.effects.length,1);
});

test('joining mid-fight replaces frozen bootstrap ordnance with current semantic samples',async()=>{
 const {Object3D}=await import('three');const {projectileRecord}=await import('../src/runtime/network-combat-projectile.js');
 const f=fixture(),mesh=new Object3D();let stopped=0;f.client.scene.add(mesh);
 f.client.grenades.push({mesh,trail:{stop(){stopped++;}}});f.client.bolts.push({});f.client.beams.push({});
 f.client.model=()=>()=>new Object3D();f.presentation.dynamic.definition=()=>({GeometryName:'grenade'});
 const record=projectileRecord({id:1,generation:1,tick:100,type:'grenade',definitionId:1,position:[4,5,6],life:1,launchTick:80});
 f.presentation.receive(transfer(record),{epoch:1,tick:100});f.presentation.present(103);
 assert.equal(mesh.parent,null);assert.equal(stopped,1);assert.equal(f.client.grenades.length,0);assert.equal(f.client.bolts.length,0);assert.equal(f.client.beams.length,0);
 assert.deepEqual(f.presentation.dynamic.objects.get(1).mesh.position.toArray(),[4,5,6]);
});

test('network baseline clears frozen projectiles even before the first combat datagram',async()=>{
 const {EntityGameState}=await import('../src/runtime/network-entity-game.js');const f=fixture();
 f.client.bolts.push({});const adapter=Object.assign(Object.create(EntityGameState.prototype),{game:{ordnance:f.client},ready:false,slots:new Map(),pendingSeats:new Map()});
 EntityGameState.prototype.baseline.call(adapter);assert.equal(adapter.ready,true);assert.equal(f.client.bolts.length,0);
 assert.equal(f.authority.bolts.length,0,'ordinary authority instance is not affected');
});

test('enhanced destruction suppresses local sprites while replicating the canonical explosion definition',async()=>{
 const {Destruction}=await import('../src/destruction.js');
 const {createCombatRegistry}=await import('../src/runtime/network-combat-registry.js');
 const {attachCombatCapture}=await import('../src/runtime/network-entity-combat.js');
 const {SimEvents}=await import('../src/sim-events.js');
 const explosion={class:'com_inf_droid_exp',base:'explosion',Effect:'droid-blast'};
 const registry=createCombatRegistry({explosion});
 const quiet=Destruction.prototype.explosion.call({enhanced:true,quiet:new WeakMap()},explosion);
 assert.notEqual(quiet,explosion);assert.equal(quiet.Effect,null);
 const f=fixture(),effects=[];f.authority.lights.flash=()=>{};f.authority.effects.play=(...args)=>effects.push(args);
 const capture=new CombatCapture({tick:()=>10,definitionId:registry.definitionId});
 attachCombatCapture({ordnance:f.authority,simEvents:new SimEvents()},capture);
 f.authority.explosion(explosion,new Vector3(1,2,3),null,quiet);
 assert.equal(effects.length,0);assert.deepEqual(capture.drainFaults(),[]);
 const event=decodeCombat(transfer(capture.drain()[0].record));
 assert.equal(event.schema,5);assert.equal(registry.definitions.get(event.definitionId),explosion);
 assert.deepEqual(event.position,[1,2,3]);assert.equal(explosion.Effect,'droid-blast');
 f.authority.explosion(explosion,new Vector3(),null);
 assert.equal(effects.length,1);assert.deepEqual(capture.drainFaults(),[]);
});

test('a class with a FadeOutTime keeps its bolt past the terminal, cut at the impact, and fades there',()=>{
 const f=fixture(),sniper={...def,Velocity:2000,LifeSpan:.15,LaserLength:20,LaserWidth:.15,GlowLength:24,BlurLength:32,FadeOutTime:1};
 f.presentation.definition=()=>sniper;f.authority.collision.cast=()=>null;
 const bolt=f.authority.bolt(sniper,new Vector3(),new Vector3(0,0,1),null);
 const launch=transfer(f.capture.drain()[0].record);assert.equal(decodeCombat(launch).life,.15);
 f.presentation.receive(launch,{epoch:1,tick:10});
 // A soldier 50 m out stops it on the second tick; the terminal arrives
 // before the page has drawn that far.
 f.setTick(12);f.capture.impact(bolt,{point:new Vector3(0,0,50),normal:new Vector3(0,0,-1),surface:2});
 f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:12});
 const kept=f.presentation.records.get(launch.id).bolt;assert.ok(kept,'drawn on past the terminal');
 assert.ok(Math.abs(kept.life-.025)<1e-12,'its flight ends at the soldier');
 f.presentation.present(10);
 const at=tick=>{f.client.drawnAt.set(-1,-1,-1);f.presentation.present(tick);return f.client.drawnAt.z;};
 assert.ok(Math.abs(at(11)-2000/60)<1e-9,'still in flight');
 assert.equal(at(12),50);assert.equal(at(40),50,'fading where it stopped');
 assert.equal(at(80),-1,'gone once its FadeOutTime has run');
 assert.equal(f.effects.length,1,'one impact effect');
});

test('a FadeOutTime terminal whose launch never arrived is drawn from the shooter\'s gun to where it stopped',()=>{
 const f=fixture(),sniper={...def,Velocity:2000,LifeSpan:.15,LaserLength:20,GlowLength:24,BlurLength:32,FadeOutTime:1};
 f.presentation.definition=()=>sniper;f.authority.collision.cast=()=>null;f.muzzle(new Vector3(1,0,0));
 // Fired and stopped 30 m out within one tick: only the terminal is sent.
 const bolt=f.authority.bolt(sniper,new Vector3(),new Vector3(0,0,1),{id:7,generation:2,networkShot:{id:9}});f.capture.drain();
 f.setTick(11);f.capture.impact(bolt,{point:new Vector3(1,0,30),normal:new Vector3(0,0,-1),surface:1});
 f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:11});
 const drawn=f.presentation.records.get(1).bolt;assert.ok(drawn,'drawn from the terminal alone');
 assert.deepEqual(drawn.position.toArray(),[1,0,0]);assert.deepEqual(drawn.direction.toArray(),[0,0,1]);
 assert.ok(Math.abs(drawn.life-30/2000)<1e-12);
 f.presentation.present(11);f.client.drawnAt.set(-1,-1,-1);f.presentation.present(30);assert.deepEqual(f.client.drawnAt.toArray(),[1,0,30],'fading where it stopped');
 // A class without a FadeOutTime is gone with its terminal, as before.
 const g=fixture();g.muzzle(new Vector3(1,0,0));const plain=g.authority.bolt(def,new Vector3(),new Vector3(0,0,1),null);g.capture.drain();
 g.setTick(11);g.capture.impact(plain,{point:new Vector3(0,0,3),normal:new Vector3(0,0,-1),surface:1});
 g.presentation.receive(transfer(g.capture.drain()[0].record),{epoch:1,tick:11});assert.equal(g.presentation.records.get(1).bolt,undefined);
});


test('newer snapshots do not expire an accepted explosion ahead of the playout clock',()=>{
 const f=fixture(), blast={Effect:'blast'};
 f.presentation.definition=()=>blast;f.client.lights.flash=()=>{};
 f.capture.explosion(blast,new Vector3());
 const record=transfer(f.capture.drain()[0].record);
 f.presentation.receive(record,{epoch:1,tick:10});f.presentation.present(9);
 // A fresh record arrives while adaptive playout still trails its snapshot.
 f.setTick(40);f.capture.explosion(blast,new Vector3(1,0,0));
 f.presentation.receive(transfer(f.capture.drain()[0].record),{epoch:1,tick:40});
 f.presentation.present(10);
 assert.equal(f.effects.length,1,'the first explosion survives until its visual time');
 f.presentation.present(40);assert.equal(f.effects.length,2);
 f.presentation.present(41);assert.equal(f.effects.length,2,'neither explosion repeats');
});


test('a timely accepted explosion is displayed once after a slow render frame',()=>{
 const f=fixture(),blast={Effect:'blast'};f.presentation.definition=()=>blast;f.client.lights.flash=()=>{};
 f.capture.explosion(blast,new Vector3());const record=transfer(f.capture.drain()[0].record);
 f.presentation.receive(record,{epoch:1,tick:10});f.presentation.present(9);
 f.presentation.present(40);assert.equal(f.effects.length,1);
 f.presentation.present(41);assert.equal(f.effects.length,1);
 assert.equal(f.presentation.receive(record,{epoch:1,tick:41}),false,'expired retransmits stay rejected');
});
