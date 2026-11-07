import test from 'node:test';
import assert from 'node:assert/strict';
import { BoxGeometry, Vector3 } from 'three';
import { f3World } from './fixtures/f3-world.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { ClassicRandom, randomBinding, uniformRandom } from '../src/ai/random.js';

const soldier = {class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]};

test('native blasts reach canonical hulls and finish their deaths depth first', {timeout:10000}, async() => {
  const chain = {class:'hull-chain',Damage:120,DamageRadiusInner:8,DamageRadiusOuter:8};
  const order = [];
  const world = await f3World({soldier},{prepareFleet:fleetFixture({explosion:chain}),
    blastNotice:(def)=>order.push(def.class)});
  const vehicles = world.factory.context.vehicles;
  const [a,b] = vehicles.vehicles;
  vehicles.onDamaged = (hull,amount,owner,sample)=>order.push([hull === a ? 'a' : 'b',amount,sample.life]);
  const owner = world.spawn('soldier',-20,0,true);
  world.ordnance.onExplode = ()=>{throw Error('legacy blast callback reached');};
  world.ordnance.explosion({class:'hull-root',Damage:100,DamageRadiusInner:.1,DamageRadiusOuter:.1},new Vector3(0,2,0),owner);
  vehicles.f2.sync();
  assert.equal(a.alive,false); assert.equal(b.alive,false);
  assert.equal(a.health,0); assert.equal(b.health,0);
  assert.equal(a.body.enabled,false); assert.equal(b.body.enabled,false);
  assert.deepEqual(order,['hull-root',['a',100,1],'hull-chain',['b',120,1],'hull-chain']);
});

test('owned repair keeps its canonical hull lock and publishes the repaired hull', {timeout:10000}, async() => {
  const tool = weaponDefinition('repair',0,{base:'repair',VehicleHealth:1,LockOnRange:12,LockOnAngle:30,
    HeatPerShot:0,HeatThreshold:1,ShotDelay:1/60});
  const world = await f3World({soldier:{...soldier,weapons:[tool]}},{prepareFleet:fleetFixture({positions:[0]})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  // The ordinary public command changes just this canonical hull before the
  // unit phase. Native repair must not reload its earlier JS health snapshot.
  hull.setHealth(95);
  const unit = world.spawn('soldier',0,-4,true);
  world.phase.begin(world.phase.bindings[unit.id],'team'); world.phase.change.value = 1; world.phase.commit();
  unit.intent.buttons[0] = 1;
  world.phase.bodiesIn(); world.phase.counters.reset(); world.phase.phase(1/60);
  vehicles.f2.sync();
  assert.ok(unit.combat.repairTarget === hull);
  assert.equal(unit.combat.repairLock,'v0'); assert.equal(hull.health,96);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('an instant native body hit uses the hull health scale without the JS hit adapter', {timeout:10000}, async() => {
  const gun = weaponDefinition('rifle',0,{TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,
    HeatPerShot:0,YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'hull-bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:25,VehicleScale:.5}});
  const world = await f3World({soldier:{...soldier,weapons:[gun]}},{prepareFleet:fleetFixture({positions:[0]})});
  const unit = world.spawn('soldier',0,-4,true);
  world.ordnance.onHit = ()=>{throw Error('legacy hull hit callback reached');};
  world.phase.bridge.handlers.riderMount=()=>{throw Error('native rider lookup returned to the host');};
  for (let tick=0;tick<=10;tick++) world.phase.history.capture(tick);
  unit.networkShot = {button:0,id:7,viewTick:8,late:0,aimYaw:0,aimPitch:0};
  unit.networkViewTick = 8; unit.intent.buttons[0] = 1;
  world.phase.bodiesIn(); world.phase.counters.reset(); world.phase.phase(1/60);
  world.factory.context.vehicles.f2.sync();
  assert.equal(world.factory.context.vehicles.vehicles[0].health,87.5);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('later instant shots pass through a hull disabled earlier in the same phase', {timeout:10000}, async() => {
  const gun = weaponDefinition('rifle',0,{TriggerSingle:1,ShotsPerSalvo:2,ShotDelay:1,
    SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,YawSpread:0,PitchSpread:0,
    StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'through-wreck',base:'bolt',Velocity:600,LifeSpan:1,Damage:25}});
  const world = await f3World({soldier:{...soldier,weapons:[gun]}},
    {prepareFleet:fleetFixture({positions:[[0,0],[0,3]],health:25})});
  const unit = world.spawn('soldier',0,-4,true);
  for (let tick=0;tick<=10;tick++) world.phase.history.capture(tick);
  unit.networkShot = {button:0,id:7,viewTick:8,late:0,aimYaw:0,aimPitch:0};
  unit.networkViewTick = 8; unit.intent.buttons[0] = 1;
  world.phase.bodiesIn(); world.phase.counters.reset(); world.phase.phase(1/60);
  const [first,second] = world.factory.context.vehicles.vehicles;
  assert.equal(first.alive,false); assert.equal(second.alive,false);
  assert.equal(first.health,0); assert.equal(second.health,0);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('an external hull command exposes committed credit before draining its damage notice', {timeout:10000}, async() => {
  const character = {}, order = [];
  let world;
  world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]}),
    prepareUnit:unit=>{unit.character = character;},
    hullDestroyedCredit:value=>{
      const hull = world.factory.context.vehicles.vehicles[0];
      assert.ok(value === character);
      assert.equal(hull.health,0); assert.equal(hull.alive,false);
      assert.equal(world.phase.active,true);
      order.push('credit');
    }});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const owner = world.spawn('soldier',-20,0,true);
  vehicles.onDamaged = ()=>{
    assert.equal(world.phase.active,false);
    order.push('damage');
  };
  hull.damage(100,owner);
  assert.deepEqual(order,['credit','damage']);
});

test('native crew death reads the current external drop stream and returns the detached body', {timeout:10000}, async() => {
  const deaths = [];
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]}),
    deathCredit:unit=>{
      assert.equal(unit.seat,null); assert.equal(unit.alive,false);
      deaths.push(unit.id);
    }});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const unit = world.spawn('soldier',0,0,true);
  world.phase.begin(world.phase.bindings[unit.id],'team');
  world.phase.change.value = 1; world.phase.commit();
  assert.equal(vehicles.board(unit,hull,0),true);
  assert.ok(unit.seat);
  // Foreign AI can advance a shared stream between unit phases. The native
  // drop consumes exactly one later draw, including an empty authored list.
  const draw = world.items.random;
  draw(); draw(); draw();
  const generator = randomBinding(draw).random;
  const expected = new ClassicRandom(generator.state); expected.uniform();
  hull.damage(Infinity);
  assert.equal(hull.alive,false); assert.equal(unit.dead,true); assert.equal(unit.seat,null);
  assert.deepEqual(deaths,[unit.id]);
  assert.equal(generator.state,expected.state);
});

// Vehicle.tick's fall death must use the same native crew/damage services as
// an incoming round, even when the fleet is the outer entry for this fixture.
test('native fleet physics completes an owned hull death without the old damage hooks', {timeout:10000}, async() => {
  const order = [];
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],height:-400})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  vehicles.world.damaged = vehicles.world.destroyed = vehicles.world.explosion = () => {
    throw Error('legacy hull damage callback reached');
  };
  vehicles.onDamaged = (_, amount) => {
    assert.equal(world.phase.active,false);
    order.push(amount);
  };
  vehicles.f2.command(hull,2);
  vehicles.update(1/60);
  assert.equal(hull.alive,false); assert.equal(hull.health,0);
  assert.equal(hull.body.enabled,false);
  assert.deepEqual(order,[Infinity]);
});

// Vehicle.fireSeat must observe flags before lazy bounds, and keep the first
// equal lock candidate in mixed-registry order. Geometry never goes through JS.
test('owned fleet locks keep ordered ties and reject deaths through native sensors', {timeout:10000}, async() => {
  const launcher = weaponDefinition('mounted-launcher',0,{base:'launcher',LockOnRange:80,
    LockOnAngle:45,LockOffAngle:50,LockTime:.01,TargetEnemy:1,TargetPerson:1,
    ordnance:{class:'mounted-seeker',base:'missile',Velocity:20,LifeSpan:2}});
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],weapon:launcher})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const crew = world.spawn('soldier',0,0,true);
  const first = world.spawn('soldier',0,16,true), second = world.spawn('soldier',0,16,true);
  for (const unit of [crew,first,second]) {
    world.phase.begin(world.phase.bindings[unit.id],'team');
    world.phase.change.value = unit === crew ? 1 : 2; world.phase.commit();
  }
  assert.equal(vehicles.board(crew,hull,0),true);
  const forbiddenGeometry = () => { throw Error('foreign fleet geometry reached'); };
  const handlers = world.phase.bridge.handlers;
  handlers.f2TargetFacts = handlers.vehAimPoint = handlers.vehAimPoints = handlers.vehRaycastUnits = forbiddenGeometry;
  world.phase.counters.reset();
  vehicles.update(1/60);
  const weapon = hull.seats[0].weapons[0].state;
  assert.equal(weapon.lockTargetId,first.id);
  assert.equal(weapon.lockGeneration,first.generation);
  assert.equal(weapon.lockComplete,true);
  assert.equal(world.phase.counters.importCalls,0,'fleet sensors and publication stay native');
  first.combat.damage(Infinity);
  vehicles.update(1/60);
  assert.equal(weapon.lockTargetId,second.id);
  assert.equal(weapon.lockGeneration,second.generation);
});

// Boarding must use the one construction policy and current native body.
// Foreign control rows may enter later; owned Unit/Soldier rows never do.
test('owned crews omit body inputs and retain construction boarding policy', {timeout:10000}, async() => {
  const world = await f3World({soldier,forbidden:{...soldier,class:'forbidden',NoEnterVehicles:1}},
    {prepareFleet:fleetFixture({positions:[0]})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const blocked = world.spawn('forbidden',0,0,true), crew = world.spawn('soldier',0,0,true);
  for (const unit of [blocked,crew]) {
    world.phase.begin(world.phase.bindings[unit.id],'team');
    world.phase.change.value = 1; world.phase.commit();
  }
  assert.equal(vehicles.board(blocked,hull,0),false);
  assert.equal(vehicles.board(crew,hull,0),true);
  const position = crew.position, velocity = crew.velocity;
  vehicles.f2.seats.prepare();
  assert.equal(vehicles.f2.seats.header.unitCount,0);
  assert.equal(vehicles.f2.seats.header.soldierCount,0);
  vehicles.update(1/60);
  assert.ok(crew.position === position); assert.ok(crew.velocity === velocity);
  assert.equal(crew.position.x,hull.position.x);
  assert.equal(crew.position.y,hull.position.y + hull.size.y*.5);
  assert.equal(crew.position.z,hull.position.z);
  assert.ok(crew.seat.vehicle === hull);
  vehicles.unboard(crew,{force:true,placement:false});
  assert.equal(crew.seat,null);
  assert.equal(crew.combat.mounted,false);
});

// Mounted defaults use the lazy seat stream; Classic controller binding can
// replace that callback with the AI generator before the next native shot.
for (const classic of [false,true]) test(`owned mounted fire keeps the ${classic ? 'Classic AI' : 'default seat'} stream`, {timeout:10000}, async() => {
  const gun = weaponDefinition('mounted-rifle',0,{ordnance:{class:'mounted-bolt',base:'bolt',Velocity:120,LifeSpan:1}});
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],weapon:gun})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const crew = world.spawn('soldier',0,0,true);
  world.phase.begin(world.phase.bindings[crew.id],'team'); world.phase.change.value = 1; world.phase.commit();
  assert.equal(vehicles.board(crew,hull,0),true);
  const weapon = hull.seats[0].weapons[0].state;
  if (classic) weapon.random = uniformRandom(new ClassicRandom(19));
  const draw = weapon.random, source = randomBinding(draw), clip = weapon.clip;
  const id = world.phase.random.register(draw), entry = world.phase.random.entries[id];
  const expected = new ClassicRandom(source.random?.state ?? entry.initial);
  // Spread is the difference of two draws per axis, pitch before yaw.
  for (let i=0;i<4;i++) expected.uniform();
  world.phase.bridge.handlers.f2WeaponDraw = () => { throw Error('mounted RNG draw imported JavaScript'); };
  world.phase.phase(0);
  assert.ok(weapon.random === draw,'an unrelated Unit publication retains the pending callback');
  crew.intent.buttons[0] = 1;
  world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.ok(weapon.clip < clip,'the normalized mounted clip fired once');
  assert.equal(source.random.state,expected.state);
  assert.ok(weapon.random === draw);
  assert.equal(world.phase.counters.importCalls,0,'mounted fire and publication stay native');
});

// Game.step's one owning entry includes fleet fire before the outer notice
// drain. No publication callback or JS round adapter may run inside it.
test('the owned unit span advances fleet fire and delivers sampled notices after return', {timeout:10000}, async() => {
  const gun = weaponDefinition('coordinated-rifle',0,{ordnance:{class:'coordinated-bolt',base:'bolt',Velocity:120,LifeSpan:1}});
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],weapon:gun})});
  const fleet = world.factory.context.vehicles, hull = fleet.vehicles[0];
  const crew = world.spawn('soldier',0,0,true), foot = world.spawn('soldier',20,0,true);
  world.phase.begin(world.phase.bindings[crew.id],'team'); world.phase.change.value = 1; world.phase.commit();
  const order = [];
  fleet.onBoard = (unit,object,index,sample)=>{
    assert.equal(world.phase.active,false); assert.ok(unit === crew); assert.ok(object === hull);
    order.push(['board',index,sample.source.generation,sample.target.generation]);
  };
  assert.equal(fleet.board(crew,hull,0),true);
  foot.onMoved = ()=>order.push(['aimer',fleet.time]);
  fleet.onFired = (object,index,shot,owner,position,direction,sample)=>{
    assert.equal(world.phase.active,false); assert.ok(owner === crew); assert.ok(object === hull);
    order.push(['shot',index,sample.source.generation,sample.mount.generation,sample.team]);
  };
  const forbidden = ()=>{throw Error('nested fleet publication callback reached');};
  world.phase.bridge.handlers.f2Posed = world.phase.bridge.handlers.f2Foreign = forbidden;
  crew.intent.buttons[0] = 1;
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.equal(fleet.time,1/60);
  assert.deepEqual(order,[['board',0,1,1],['aimer',0],['shot',0,1,1,1]]);
  assert.equal(world.phase.counters.phaseCalls,1); assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,1,'only the foot unit aimer crosses');
  assert.equal(world.phase.counters.rustAllocations,0); assert.equal(world.phase.counters.rustReallocations,0);
});

// Vehicles.presentRiders never exposes a corpse killed inside a destroyed
// enclosed hull. The same pooled body shows again in its next gameplay life.
test('owned enclosed riders and their destroyed corpses keep native visibility', {timeout:10000}, async() => {
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]})});
  const vehicles = world.factory.context.vehicles, hull = vehicles.vehicles[0];
  const crew = world.spawn('soldier',0,0,true);
  world.phase.begin(world.phase.bindings[crew.id],'team'); world.phase.change.value = 1; world.phase.commit();
  assert.equal(vehicles.board(crew,hull,0),true);
  assert.equal(crew.actor.root.visible,false);
  hull.damage(Infinity);
  assert.equal(crew.dead,true); assert.equal(crew.seat,null);
  assert.equal(crew.actor.root.visible,false);
  const root = crew.actor.root;
  crew.spawn(3,0,4,0);
  assert.ok(crew.actor.root === root);
  assert.equal(crew.generation,2);
  assert.equal(crew.actor.root.visible,true);
});

// Vehicle.spawn runs before foreign weapon input. An input prepared for its
// previous life must not restore the old occupant's RNG or spread overrides.
for (const base of ['hover','flyer']) test(`owned ${base} respawn resets mounted rows natively and rejects prior-life modifiers`, {timeout:10000}, async() => {
  const gun = weaponDefinition('respawn-rifle',0,{ordnance:{class:'respawn-bolt',base:'bolt',Velocity:120,LifeSpan:1}});
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],weapon:gun,respawnTime:.02,base})});
  const fleet = world.factory.context.vehicles, hull = fleet.vehicles[0], target = hull.aiTarget, position = hull.position;
  const mount = hull.seats[0].weapons[0], original = mount.state;
  const born = [original.clip,original.reserve,original.floor,original.spreadScale,original.sniperScale,original.yawSpreadOverride];
  original.random = uniformRandom(new ClassicRandom(41));
  original.floor = 4; original.spreadScale = .3; original.sniperScale = .6; original.yawSpreadOverride = 7;
  const forbidden = ()=>{throw Error('owned hull life reentered the JS constructor import');};
  world.phase.bridge.handlers.f2Life = forbidden;
  hull.damage(Infinity);
  world.phase.counters.reset();
  world.phase.phase(.01);
  assert.equal(hull.alive,false);
  // Change an old-life field on the exact tick native spawn runs. That
  // submitted row is stale even though its slab reference still matches.
  original.floor = 9;
  world.phase.phase(.01);
  assert.equal(hull.alive,true); assert.equal(target.generation,2);
  assert.ok(hull.aiTarget === target); assert.ok(hull.position === position);
  assert.ok(mount.state !== original); assert.equal(mount.state.random,null);
  assert.deepEqual([mount.state.clip,mount.state.reserve,mount.state.floor,mount.state.spreadScale,
    mount.state.sniperScale,mount.state.yawSpreadOverride],born);
  assert.equal(world.phase.counters.phaseCalls,2); assert.equal(world.phase.counters.wasmCalls,2);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0); assert.equal(world.phase.counters.rustReallocations,0);
});


test('a neutral pad activates its retained hull in mixed registry order without a constructor import', {timeout:10000}, async() => {
  const world = await f3World({soldier},{
    prepareFleet:fleetFixture({positions:[30,0],neutralPad:1}),passiveClasses:['soldier'],
  });
  const vs = world.factory.context.vehicles, collision = world.phase.collision;
  assert.equal(vs.vehicles.length,1);
  assert.equal(vs.slots[1].variants.size,0);
  assert.equal(collision.bodies.length,1);
  const retained = vs.f2.prepared[0], position = retained.position;
  assert.equal(world.units.list.length,2);
  const next = world.units.nextId, serial = collision.bodySerial;
  world.phase.bridge.handlers.f2Variant = ()=>{throw Error('native birth reached constructor import');};
  vs.posts.posts[0].team = 1;
  world.phase.bodiesIn(); world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(vs.vehicles.length,2); assert.ok(vs.vehicles[1] === retained);
  assert.ok(retained.position === position); assert.equal(retained.alive,true);
  assert.ok(vs.slots[1].variants.get('tank') === retained);
  assert.equal(retained.aiTarget.id,next);
  assert.ok(world.units.list[2] === retained.aiTarget);
  assert.ok(world.units.byId(next) === retained.aiTarget);
  assert.equal(collision.bodies.length,2); assert.ok(collision.bodies[1] === retained.body);
  assert.equal(collision.bodySerial,serial + 1);
  assert.equal(retained.body.offset,(serial + 1) * 0x1000000);
  assert.equal(world.phase.counters.wasmCalls,1); assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0); assert.equal(world.phase.counters.rustReallocations,0);
  world.phase.phase(1/60);
  assert.equal(vs.vehicles.length,2); assert.equal(world.units.list.length,3);
});


test('a newly boarded AI lease binds before its controllable import is decoded', async() => {
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]})});
  const vs = world.factory.context.vehicles, hull = vs.vehicles[0];
  const unit = world.spawn('soldier',0,0);
  unit.combat.team = 1;
  assert.equal(unit.team,1);
  assert.equal(vs.board(unit,hull,0),true);
  const lease = unit.seat;
  assert.ok(lease.controllable); assert.ok(lease.controllable.lease === lease);
  assert.ok(lease.controllable.occupant === unit);
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.ok(unit.seat === lease); assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.rustAllocations,0); assert.equal(world.phase.counters.rustReallocations,0);
});

test('Classic seat attachment publishes its generator before later native mirrors', async() => {
  const gun = weaponDefinition('ai-mounted',0,{ordnance:{class:'ai-bolt',base:'bolt',Velocity:120,LifeSpan:1}});
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[0],weapon:gun})});
  const vs = world.factory.context.vehicles, hull = vs.vehicles[0];
  const unit = world.spawn('soldier',0,0); unit.combat.team = 1;
  assert.equal(vs.board(unit,hull,0),true);
  const control = unit.seat.controllable, weapon = hull.seats[0].weapons[0].state;
  const random = new ClassicRandom(19), draw = uniformRandom(random), expected = new ClassicRandom(19);
  control.combat.arsenal.setRandom(draw);
  unit.combat.health = 1500;
  assert.equal(weapon.random,draw);
  world.phase.phase(0);
  assert.equal(weapon.random,draw);
  control.controller = {}; control.intent.buttons[0] = 1;
  for (let i = 0; i < 4; i++) expected.uniform();
  const clip = weapon.clip;
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.ok(weapon.clip < clip);
  assert.equal(random.state,expected.state);
  assert.equal(weapon.random,draw);
  assert.equal(world.phase.counters.commandCalls,0,'the consuming phase performs no setter exports');
});

test('prepared cold unit births fit the retained native flight target cache',async()=>{
  const gun=weaponDefinition('lock',0,{LockOnRange:100,LockOnAngle:90,LockOffAngle:100,LockTime:.1,
    ordnance:{class:'lock-round',base:'missile',Velocity:40,LifeSpan:1}});
  const w=await f3World({soldier},{prepareFleet:fleetFixture({base:'flyer',positions:[0],weapon:gun})});
  const hull=w.factory.context.vehicles.vehicles[0],pilot=w.spawn('soldier',0,0,true);
  assert.equal(w.factory.context.vehicles.board(pilot,hull),true);
  // Factory births bypass Units.add, and therefore must use its cold reserved
  // target capacity rather than the registry length at factory preparation.
  for(let i=0;i<3;i++){const u=w.spawn('soldier',i*3,30);u.team=u.combat.team=2;}
  pilot.intent.aim=new Vector3(0,0,1);
  for(let i=0;i<30;i++)w.phase.phase(1/60);
  assert.equal(hull.alive,true);assert.equal(pilot.seat.vehicle,hull);
});

test('retiring a live hull publishes dead registry facts before releasing its slot',async()=>{
  const w=await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]})});
  const vehicles=w.factory.context.vehicles,hull=vehicles.vehicles[0],target=hull.aiTarget;
  const out=[];w.units.query(hull.position,10,out);
  assert.ok(out.includes(target));
  vehicles.f2.retire(hull);
  w.units.query(target.position,10,out);
  assert.equal(out.includes(target),false);
});

// A legSide list starts absent and appears at the first ground sample. Both
// encoding buffers and every native working copy must already have capacity.
test('the first walker leg sample and its respawn reuse cold native storage',async()=>{
  const legs={rayLength:3,pairs:[{left:{at:[-.5,0,0],leg:'left'},right:{at:[.5,0,0],leg:'right'}}]};
  const world=await f3World({soldier},{prepareFleet:fleetFixture({base:'walker',legs,positions:[0],respawnTime:.2})});
  const hull=world.factory.context.vehicles.vehicles[0];
  assert.equal(hull.legSide,undefined);
  world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(hull.legSide.length,1);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  hull.damage(Infinity);world.phase.counters.reset();
  for(let i=0;i<120;i++)world.phase.phase(1/60);
  assert.equal(hull.alive,true);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);

});

// UnitFactory can add collision bodies after the fleet's initial topology.
// A real exit query must resolve that late body through F3's shared names.
test('native seat exit casts accept collision bodies named after fleet construction',async()=>{
  const world=await f3World({soldier},{prepareFleet:fleetFixture({positions:[0]})});
  const vehicles=world.factory.context.vehicles,hull=vehicles.vehicles[0];
  const unit=world.spawn('soldier',0,0,true);
  assert.equal(vehicles.board(unit,hull,0),true);
  const floor=new BoxGeometry(20,.5,20).translate(0,.25,0),collision=world.phase.collision;
  const body=collision.addBody(floor.attributes.position.array,floor.index.array,new Uint32Array(floor.index.count/3));
  body.enabled=true;
  world.phase.bodiesIn();
  assert.equal(vehicles.unboard(unit,{force:true,placement:true}),true);
  assert.equal(unit.seat,null);
});
