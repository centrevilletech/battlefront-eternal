import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { PredictedShots } from '../src/runtime/predicted-shots.js';
import { Vehicle } from '../src/vehicles.js';

function fixture(ordnance=null) {
  let now=0,flashes=0;
  const scene=new THREE.Scene(), weapon={def:{ordnance:{base:'bolt',Velocity:100,LifeSpan:.3},ShotDelay:.1},canShoot:true,wait:0,clip:1,levels:[{}]};
  const unit={id:8,generation:2,alive:true,combat:{dead:false,origin:new THREE.Vector3(),arsenal:{current:()=>weapon},firePoint:()=>null,flash:()=>flashes++,animator:{fire(){}},firstPerson:false}};
  const shots=new PredictedShots(scene,{cast:()=>null},()=>now,ordnance);
  const edge={id:1,generation:2,pressed:true,button:0,aimYaw:0,aimPitch:0};
  return {scene,weapon,unit,shots,edge,get flashes(){return flashes;},setTime:v=>now=v};
}
test('predicted shot appears immediately without consuming ammo and reconciles once',()=>{
  const f=fixture();assert.equal(f.shots.fire(f.edge,f.unit),true);
  assert.equal(f.scene.children.length,1);assert.equal(f.flashes,1);assert.equal(f.weapon.clip,1);assert.equal(f.weapon.wait,0);
  f.setTime(.1);f.shots.update();assert.equal(f.scene.children[0].position.z,10);
  const tag={playerId:8,generation:2,fireId:1};
  assert.equal(f.shots.confirm(tag),true);assert.equal(f.scene.children.length,0);
  assert.equal(f.shots.confirm(tag),true);
  f.setTime(1.1);f.shots.update();assert.equal(f.shots.shots.size,0);assert.equal(f.shots.confirm(tag),false);
});
test('rejected or unconfirmed cosmetics expire and cannot trigger authoritative hits',()=>{
  const f=fixture();f.shots.fire(f.edge,f.unit);
  assert.equal(f.shots.fire({...f.edge,id:2},f.unit),false);
  f.setTime(.4);f.shots.update();assert.equal(f.scene.children.length,0);assert.equal(f.weapon.clip,1);
  f.shots.clear();assert.equal(f.shots.shots.size,0);
});
test('mounted cosmetic shots follow the actual barrel while its servo is still turning',()=>{
  const f=fixture(),node=new THREE.Object3D();node.position.set(2,3,4);node.updateMatrixWorld();
  f.unit.soldier={air:'fall',lock:1,posture:'prone'};
  const aimer={bone:node,posed:false,onTarget:false,facing:new THREE.Vector3(1,0,0)};
  const mounted={state:f.weapon,def:f.weapon.def,barrel:0,muzzles:[{fireNode:node,sourceAimer:aimer}]};
  f.weapon.channel=0;
  const vehicle={byName:{},seats:[{weapons:[mounted],target:new THREE.Vector3(2,3,100)}],shotDirection:Vehicle.prototype.shotDirection};
  f.unit.seat={vehicle,index:0};
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  const shot=[...f.shots.shots.values()][0];
  assert.deepEqual(shot.origin.toArray(),[2,3,4]);
  assert.deepEqual(shot.direction.toArray(),[1,0,0],'cosmetic fire respects barrel direction instead of the newer crosshair');
  assert.equal(mounted.barrel,0);assert.equal(f.weapon.clip,1);
  f.shots.clear();
});

test('cosmetic infantry fire respects authoritative posture and environment blocks',()=>{
  for (const soldier of [{air:'jump'}, {air:'fall'}, {dive:true}, {lock:.1}, {rolled:true}]) {
    const f=fixture(); f.unit.soldier=soldier;
    assert.equal(f.shots.fire(f.edge,f.unit),false,JSON.stringify(soldier));
    assert.equal(f.scene.children.length,0); assert.equal(f.flashes,0); assert.equal(f.weapon.clip,1);
  }
  const f=fixture(); f.unit.combat.gunUnderwater=true;
  assert.equal(f.shots.fire(f.edge,f.unit),false); assert.equal(f.flashes,0);
});

test('prone cosmetic fire checks held movement, including movement against a wall',()=>{
  for (const down of [['KeyW'], ['ArrowRight'], ['KeyS'], ['KeyA']]) {
    const f=fixture(); f.unit.soldier={posture:'prone',velocity:new THREE.Vector3()};
    assert.equal(f.shots.fire(f.edge,f.unit,{down}),false);
    assert.equal(f.flashes,0);
  }
  for (const soldier of [{posture:'prone'}, {air:'jet'}, {posture:'stand'}]) {
    const f=fixture(); f.unit.soldier=soldier;
    assert.equal(f.shots.fire(f.edge,f.unit,{down:[]}),true);
    assert.equal(f.weapon.clip,1); f.shots.clear();
  }
});

test('Droideka fire input samples shared predicted aim without infantry lookRaw or consuming look counts', async()=>{
  const { commandBodyAim } = await import('../src/runtime/command-body-aim.js');
  const { EntityInputSender } = await import('../src/runtime/network-entity-sender.js');
  const { Droideka } = await import('../src/droideka.js');
  const f=fixture();
  const body=Object.create(Droideka.prototype);
  Object.assign(body,{aimYaw:.4,aimPitch:-.2,lookX:7,lookY:9,mode:0});
  f.unit.soldier=body;
  const preview=Object.create(body), command={tick:10,epoch:1,generation:2,sequence:1,down:['Mouse0'],actions:[],look:[[100,200]],mouseScale:2,invertMouse:true};
  const sender=new EntityInputSender({registry:{},send(){},getAim:c=>commandBodyAim(body,true,preview,c,1,1,false),onFire:(edge,c)=>f.shots.fire(edge,f.unit,c)});
  sender.edges.reset(command.epoch,command.generation);
  sender.fire(command);
  assert.equal(f.shots.shots.size,1);
  const shot=[...f.shots.shots.values()][0];
  assert.ok(shot.direction.distanceTo(new THREE.Vector3(Math.sin(.4)*Math.cos(-.2),Math.sin(-.2),Math.cos(.4)*Math.cos(-.2)))<1e-12);
  assert.equal(body.lookX,7);assert.equal(body.lookY,9);assert.equal(body.aimYaw,.4);assert.equal(body.aimPitch,-.2);
  f.shots.clear();
});

test('mounted local cosmetic fire observes the authority takeoff and landing gate',()=>{
 const f=fixture();f.unit.seat={vehicle:{flying:true,flightState:'TakingOff'}};
 assert.equal(f.shots.fire(f.edge,f.unit),false);f.unit.seat.vehicle.flightState='Landing';
 assert.equal(f.shots.fire(f.edge,f.unit),false);assert.equal(f.scene.children.length,0);
});

test('a confirmed cosmetic flies its whole path as drawn; only an immediate confirmation (a local round) replaces it',()=>{
  const f=fixture();assert.equal(f.shots.fire(f.edge,f.unit),true);
  const tag={playerId:8,generation:2,fireId:1};
  assert.equal(f.shots.confirm(tag,{tick:100}),true);
  f.setTime(.1);f.shots.update(90);assert.equal(f.scene.children.length,1,'no gap before the authoritative round is known');
  assert.equal(f.scene.children[0].position.z,10);
  f.shots.update(100);assert.equal(f.scene.children.length,1,'the authoritative round arriving does not move the bolt back to the muzzle');
  f.setTime(.25);f.shots.update(110);assert.equal(f.scene.children[0].position.z,25);
  f.setTime(.5);f.shots.update(120);assert.equal(f.scene.children.length,0,'it ends at its lifespan');assert.equal(f.shots.shots.size,1);
  f.setTime(1.1);f.shots.update(120);assert.equal(f.shots.shots.size,0);
  f.setTime(1.2);assert.equal(f.shots.fire({...f.edge,id:3},f.unit),true);assert.equal(f.shots.confirm({...tag,fireId:3}),true);f.shots.update(0);assert.equal(f.shots.shots.size,0,'confirmed without a tick, it goes at once');
  f.setTime(1.4);assert.equal(f.shots.fire({...f.edge,id:2},f.unit),true);
  f.setTime(1.8);f.shots.update(0);assert.equal(f.scene.children.length,0,'past its lifespan the round is no longer drawn');
  assert.equal(f.shots.shots.size,1,'but still accepts its confirmation');
  f.setTime(2.5);f.shots.update(0);assert.equal(f.shots.shots.size,0);
});
test('rounds on a held trigger follow the local cadence and confirm oldest first',()=>{
  const f=fixture(),held={...f.edge,id:0,held:true};
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  f.setTime(.05);assert.equal(f.shots.fire(held,f.unit),false,'the local cadence spaces held rounds');
  f.setTime(.1);f.weapon.wait=.09;assert.equal(f.shots.fire(held,f.unit),true,'a stale authoritative cooldown does not gate held rounds');
  f.setTime(.2);assert.equal(f.shots.fire(held,f.unit),true);
  assert.equal(f.shots.shots.size,3);assert.equal(f.flashes,3);
  assert.equal(f.shots.confirm({playerId:8,generation:2,fireId:0}),true);
  const remaining=[...f.shots.shots.values()].filter(shot=>shot.confirmedTick===null);
  assert.equal(remaining.length,2);assert.ok(remaining.some(shot=>!shot.held),'the press keeps waiting for its own id');
  assert.equal(f.shots.confirm({playerId:8,generation:2,fireId:1}),true);
  assert.equal(f.shots.confirm({playerId:9,generation:2,fireId:0}),false,'another life cannot claim a held round');
  f.weapon.def.TriggerSingle=1;f.setTime(.4);assert.equal(f.shots.fire(held,f.unit),false,'a single-shot trigger fires once per press');
  f.shots.clear();
});

test('a fresh click uses the known local deadline despite a delayed authority cooldown',()=>{
  const f=fixture();f.weapon.def.ShotDelay=.2;
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  f.weapon.wait=.025;
  f.setTime(.15);assert.equal(f.shots.fire({...f.edge,id:2},f.unit),false,'local cooldown still blocks an early click');
  f.setTime(.25);assert.equal(f.shots.fire({...f.edge,id:3},f.unit),true,'elapsed local cooldown permits immediate feedback');
  assert.equal(f.flashes,2);assert.equal(f.weapon.clip,1);assert.equal(f.weapon.wait,.025,'authoritative state is untouched');
  f.shots.clear();
  f.setTime(.5);assert.equal(f.shots.fire({...f.edge,id:4},f.unit),false,'after reset an unknown cooldown follows authority');
});

test('automatic cosmetics retain cadence across frame rates without accumulating sample delay',()=>{
  for(const delay of [.2,.12])for(const fps of [30,60,75,144]) {
    const f=fixture();f.weapon.def.ShotDelay=delay;
    const times=[];
    for(let frame=0;frame<10*fps;frame++) {
      const now=frame/fps;f.setTime(now);f.shots.update();
      if(f.shots.fire({...f.edge,id:frame?0:1,held:frame>0},f.unit))times.push(now);
    }
    assert.equal(times.length,Math.ceil(10/delay),`${delay}s at ${fps}fps`);
    for(let shot=0;shot<times.length;shot++)assert.ok(Math.abs(times[shot]-shot*delay)<1/fps+1e-6,'sampling error stays within one frame');
    assert.equal(f.weapon.clip,1);assert.equal(f.weapon.wait,0);f.shots.clear();
  }
});

test('an automatic trigger resumes after a stall without replaying missed cosmetic rounds',()=>{
  const f=fixture(),held={...f.edge,id:0,held:true};
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  f.setTime(.55);f.shots.update();
  assert.equal(f.shots.fire(held,f.unit),true);
  for(let sample=0;sample<8;sample++)assert.equal(f.shots.fire(held,f.unit),false,'batched input cannot emit a catch-up burst');
  f.setTime(.65);assert.equal(f.shots.fire(held,f.unit),true);
  assert.equal(f.flashes,3);f.shots.clear();
});

test('with the ordnance, cosmetic rounds fly along their path through the real bolt renderer',()=>{
  const drawn=[],f=fixture({drawBolt:bolt=>drawn.push([bolt.position.z,bolt.flown,bolt.drawn])});
  assert.equal(f.shots.fire(f.edge,f.unit),true);assert.equal(f.scene.children.length,0,'no stand-in line');
  f.setTime(.1);f.shots.update(0);f.shots.draw();
  assert.deepEqual(drawn.at(-1),[10,10,1],'drawn where the round is now, ten metres down range');
  const shot=[...f.shots.shots.values()][0];assert.equal(shot.origin.z,0,'the muzzle is kept for the first streak');
  f.setTime(.2);f.shots.update(0);f.shots.draw();assert.equal(drawn.at(-1)[0],20);
  f.setTime(.5);f.shots.update(0);f.shots.draw();assert.equal(drawn.length,2,'past its lifespan nothing is drawn');
  f.shots.clear();
});

test('a world worker\'s copies of his shot claim its cosmetic once: the sound, the flash and the bolt alike',()=>{
  const f=fixture(),held={...f.edge,id:0,held:true};f.weapon.def.ordnance.LifeSpan=1;
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  f.setTime(.15);assert.equal(f.shots.fire(held,f.unit),true);
  f.setTime(.3);assert.equal(f.shots.fire(held,f.unit),true);
  const press={playerId:8,generation:2,fireId:1},round={playerId:8,generation:2,fireId:0};
  const first=f.shots.claim(press,{tick:40});
  assert.ok(first?.bolt);assert.equal(f.shots.claim(press,{tick:41}),first,'the bolt after the sound takes the same cosmetic');
  const a=f.shots.claim(round,{tick:50}),b=f.shots.claim(round,{tick:50}),c=f.shots.claim(round,{tick:59});
  assert.ok(a&&c&&a!==c);assert.equal(a,b,'one held round a tick, whichever copy arrives first');
  assert.equal(f.shots.claim(round,{tick:70}),null,'more held rounds than cosmetics are drawn by the world');
  f.setTime(.35);f.shots.update(80);assert.equal(f.scene.children.length,3,'claimed cosmetics fly on as drawn');
  f.shots.clear();
});

test('remote combat cannot evict an own held round before its later copies arrive',()=>{
  const f=fixture(),held={...f.edge,id:0,held:true},own={playerId:8,generation:2,fireId:0};
  f.shots.fire(held,f.unit);f.setTime(.1);f.shots.fire(held,f.unit);
  const first=f.shots.claim(own,{tick:100});
  for(let tick=100;tick<110;tick++)for(let playerId=9;playerId<73;playerId++)assert.equal(f.shots.claim({playerId,generation:2,fireId:0},{tick}),null);
  assert.equal(f.shots.claims.size,1);
  assert.equal(f.shots.claim(own,{tick:100}),first,'a later terminal still belongs to the first cosmetic');
  const second=f.shots.claim(own,{tick:106});assert.ok(second&&second!==first);
  f.shots.clear();
});

test('a new epoch clears cosmetic identity even when player ID and generation are reused',()=>{
  const f=fixture();f.shots.fire({...f.edge,epoch:1},f.unit);
  const metadata={playerId:8,generation:2,fireId:1},first=f.shots.claim(metadata,{tick:100});
  assert.equal(f.shots.fire({...f.edge,epoch:2},f.unit),true,'the old epoch cooldown cannot block the new one');
  const second=f.shots.claim(metadata,{tick:1});assert.ok(second&&second!==first);assert.equal(f.shots.shots.size,1);
  f.shots.clear();
});

test('simultaneous held channels sharing one ordnance definition claim separate cosmetics',()=>{
  const f=fixture(),secondary={...f.weapon,channel:1};f.weapon.channel=0;
  f.unit.combat.arsenal.current=channel=>channel?secondary:f.weapon;
  for(const button of [0,1])assert.equal(f.shots.fire({...f.edge,button,id:0,held:true},f.unit),true);
  const own={playerId:8,generation:2,fireId:0};
  const secondaryShot=f.shots.claim({...own,channel:1},{tick:100}),primaryShot=f.shots.claim({...own,channel:0},{tick:100});
  assert.ok(primaryShot&&secondaryShot&&primaryShot!==secondaryShot);
  assert.equal(primaryShot.metadata.channel,0);assert.equal(secondaryShot.metadata.channel,1);
  assert.equal(f.shots.claim({...own,channel:1},{tick:100}),secondaryShot);
  assert.equal(f.shots.claim({...own,channel:0},{tick:100}),primaryShot);
  f.shots.clear();
});

test('shared Weapon replay shot can present after its private ammo and cooldown were spent',()=>{
  const f=fixture();f.weapon.canShoot=false;f.weapon.wait=.1;f.weapon.clip=0;
  assert.equal(f.shots.fire(f.edge,f.unit),false);
  assert.equal(f.shots.fire(f.edge,f.unit,{}, {weapon:f.weapon,yaw:.2,pitch:0,charge:.5}),true);
  const shot=[...f.shots.shots.values()][0];
  assert.ok(shot.direction.distanceTo(new THREE.Vector3(Math.sin(.2),0,Math.cos(.2)))<1e-12);
  assert.equal(f.weapon.clip,0);assert.equal(f.weapon.wait,.1);f.shots.clear();
});

test('optional shot timing hooks observe successful prediction and first confirmation only',()=>{
  const f=fixture(),predictions=[],confirmations=[];
  f.shots.onPredicted=(...args)=>predictions.push(args);f.shots.onConfirmed=(...args)=>confirmations.push(args);
  const command={tick:10};assert.equal(f.shots.fire(f.edge,f.unit,command),true);
  assert.equal(f.shots.fire({...f.edge,id:2},f.unit,command),false);
  assert.equal(predictions.length,1);assert.equal(predictions[0][0],f.edge);assert.equal(predictions[0][1],command);
  const metadata=predictions[0][2];assert.equal(metadata.channel,0);
  f.shots.claim(metadata,{tick:12});f.shots.claim(metadata,{tick:12});f.shots.confirm(metadata,{tick:13});
  assert.equal(confirmations.length,1);assert.equal(confirmations[0][1].tick,12);f.shots.clear();
});

test('shared multi-pellet volleys retain every cosmetic and confirm the whole volley once',()=>{
  for(const held of [false,true]){
    const f=fixture(),edge={...f.edge,id:held?0:1,held,tick:10};let confirmations=0,predictions=0;f.shots.onConfirmed=()=>confirmations++;f.shots.onPredicted=()=>predictions++;
    for(const yaw of [-.1,0,.1])assert.equal(f.shots.fire(edge,f.unit,{}, {weapon:f.weapon,yaw,pitch:0}),true);
    assert.equal(f.shots.shots.size,3);assert.equal(f.scene.children.length,3);
    assert.equal(predictions,1,'trace counts the volley once, retaining every pellet cosmetic');
    const metadata={playerId:8,generation:2,fireId:edge.id,channel:0};
    const volley=f.shots.claim(metadata,{tick:12});assert.ok(volley);
    assert.ok([...f.shots.shots.values()].every(shot=>shot.confirmedTick===12));
    assert.equal(f.shots.claim(metadata,{tick:12}),volley);assert.equal(confirmations,1);
    f.setTime(.2);assert.equal(f.shots.fire({...edge,id:0,held:true,tick:22},f.unit,{}, {weapon:f.weapon,yaw:0,pitch:0}),true);
    assert.equal(predictions,2,'the next automatic volley has its own trace');
    const next=f.shots.claim({...metadata,fireId:0},{tick:24});assert.ok(next&&next!==volley,'next automatic round skips all confirmed pellets');
    f.shots.clear();assert.equal(f.scene.children.length,0);
  }
});

test('only a bolt is drawn ahead of the authority; a rocket shows its flash and sound at once',async()=>{
  const {listenSounds}=await import('../src/audio/events.js');
  const heard=[],stop=listenSounds(type=>heard.push(type));
  try {
    const f=fixture();f.weapon.def.ordnance={base:'missile',Velocity:40,LifeSpan:3};
    assert.equal(f.shots.fire(f.edge,f.unit),true);
    assert.equal(f.flashes,1);assert.deepEqual(heard,['fire']);
    assert.equal(f.scene.children.length,0,'no streak stands in for the missile');
    const shot=f.shots.claim({playerId:8,generation:2,fireId:1},{tick:40});
    assert.ok(shot&&!shot.bolt,'the world\'s sound and flash are not repeated, its missile is drawn');
    f.setTime(.5);f.shots.update(0);assert.equal(f.shots.shots.size,1);
    f.setTime(1.1);f.shots.update(0);assert.equal(f.shots.shots.size,0);
  } finally {stop();}
});

test('a droideka cosmetic leaves its next visible gun, converging on the reticle',async()=>{
  const { Droideka } = await import('../src/droideka.js');
  const f=fixture();
  const root=new THREE.Group(); root.position.set(10,0,10); f.scene.add(root);
  const mk=(x)=>{const b=new THREE.Bone(); b.position.set(x,1,0.3); root.add(b); return b;};
  const left=mk(0.5), right=mk(-0.5);
  const body=Object.create(Droideka.prototype);
  Object.assign(body,{position:new THREE.Vector3(10,0,10),velocity:new THREE.Vector3(),aimYaw:0,aimPitch:0,mode:0,eyeHeight:1.6,gun:0,lastGun:null,guns:[left,right],gunBarrels:[[left],[right]],physicalBarrels:null});
  f.unit.soldier=body;
  // The page combat's gun origin is never stepped: using it put every
  // cosmetic at the world origin and the claimed authoritative round
  // stayed undrawn, so the droideka showed no bolts at all.
  f.unit.combat.origin.set(999,999,999);
  f.unit.combat.soldier=body;
  const predicted={weapon:f.weapon,yaw:0,pitch:0};
  assert.equal(f.shots.fire(f.edge,f.unit,{},predicted),true);
  f.setTime(.1);
  assert.equal(f.shots.fire({...f.edge,id:2},f.unit,{},predicted),true);
  const shots=[...f.shots.shots.values()];
  assert.deepEqual(shots[0].origin.toArray(),[10.5,1,10.3],'first round from the first gun');
  assert.deepEqual(shots[1].origin.toArray(),[9.5,1,10.3],'second round from the other gun');
  for(const shot of shots){
    // Converged on the eye ray's far point, as the authority fires it:
    // the bolt's line passes through (10, 1.6, 10 + 512).
    const want=new THREE.Vector3(10,1.6,522).sub(shot.origin).normalize();
    assert.ok(shot.direction.distanceTo(want)<1e-9,`bolt converges on the reticle ray, dir ${shot.direction.toArray()}`);
  }
});

test('a predicted bolt stops at the first soldier it crosses as the page shows him', () => {
  // A soldier 10 m down the line of fire; castUnits is Ordnance's own test.
  const asked = [];
  const ordnance = { drawBolt() {}, castUnits(origin, direction, far, owner) {
    asked.push(owner);
    const t = 10 - origin.z;
    return t >= 0 && t <= far ? { distance: t } : null;
  } };
  const f = fixture(ordnance);
  assert.equal(f.shots.fire(f.edge, f.unit), true);
  const shot = [...f.shots.shots.values()][0];
  f.setTime(.05); f.shots.update();
  assert.equal(shot.flying, true); assert.equal(shot.bolt.flown, 5);
  // The next step would carry it to 15 m: it is drawn at the man and ends.
  f.setTime(.15); f.shots.update();
  assert.equal(shot.bolt.flown, 10); assert.ok(Math.abs(shot.life - .1) < 1e-9);
  assert.equal(shot.bolt.position.z, 10);
  f.setTime(.2); f.shots.update();
  assert.equal(shot.flying, false);
  assert.ok(asked.length >= 2 && asked.every((owner) => owner === f.unit), 'the shooter is never his own target');
  assert.equal(f.weapon.clip, 1, 'nothing authoritative is spent');
});

test('a cosmetic sniper round is drawn on where it stopped for its FadeOutTime, cut at the authority\'s impact',()=>{
  const drawn=[],ordnance={drawBolt:bolt=>drawn.push({flown:bolt.flown,ended:bolt.ended,fade:bolt.fade})};
  const f=fixture(ordnance);
  f.weapon.def.ordnance={base:'bolt',Velocity:2000,LifeSpan:.15,LaserLength:20,GlowLength:24,BlurLength:32,FadeOutTime:1};
  assert.equal(f.shots.fire(f.edge,f.unit),true);
  const frame=t=>{f.setTime(t);f.shots.update();drawn.length=0;f.shots.draw();return drawn[0];};
  assert.deepEqual(frame(.1),{flown:200,ended:false,fade:1});
  let shot=frame(.4);assert.equal(shot.flown,300);assert.equal(shot.ended,true);assert.ok(Math.abs(shot.fade-.75)<1e-9);
  // The authority's round hit a soldier 50 m out.
  const tag={playerId:8,generation:2,fireId:1,channel:0};
  assert.ok(f.shots.claim(tag,{tick:5}));f.shots.stop(tag,{tick:5,point:[0,0,50]});
  shot=frame(.5);assert.equal(shot.flown,50);assert.ok(Math.abs(shot.fade-.525)<1e-9);
  assert.equal(frame(1.1),undefined,'gone once its FadeOutTime has run');
  f.setTime(1.2);f.shots.update();assert.equal(f.shots.shots.size,0);
});
