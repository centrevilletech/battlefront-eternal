import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Turret } from '../src/props.js';
import { TurretSeats } from '../src/seats.js';
import { ClassicRandom, randomBinding, uniformRandom } from '../src/ai/random.js';
import { f3World } from './fixtures/f3-world.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { weaponDefinition } from './fixtures/client-weapons.js';

const soldier = {class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]};

// The same drum, gun and seat hierarchy a built armedbuilding supplies. The
// complete owning phase needs neither a retail rig nor a drawing scene.
// Desktop bundling can rename constructors; class identity carries policy.
class GunEmplacement extends Turret {}

function liveTurret(collision,units,context,{health=100,exposed=false}={}) {
  const root = new THREE.Group(), drum = new THREE.Bone(), gun = new THREE.Bone();
  const seat = new THREE.Bone(), fire = new THREE.Bone();
  root.matrixAutoUpdate = false; root.matrix.makeTranslation(20,3,30);
  root.add(drum); drum.add(gun,seat); gun.add(fire);
  fire.position.z = 1; seat.position.z = -.5;
  const rig = {root,bones:[drum,gun,seat,fire],byName:{drum,gun,seat,fire},
    model:{box:[[-1,0,-1],[1,2,1]],sphere:[0,1,0,2]}};
  const weapon = weaponDefinition('owned-turret',0,{ShotDelay:.1,
    ordnance:{class:'owned-turret-bolt',base:'bolt',Velocity:40,LifeSpan:1,Damage:0}});
  const turret = new GunEmplacement({class:'owned-building',base:'armedbuilding',matrix:root.matrix.toArray(),weapon,
    settings:{MaxHealth:health,TurretNodeName:'drum',AimerNodeName:'gun',PilotPosition:'seat',FirePointName:'fire',
      PilotAnimation:exposed ? 'gunner' : undefined}},
    {built:rig},{},null);
  const live = {buildings:[turret],turrets:[turret]};
  const crews = new TurretSeats({live,collision,units,ordnance:context.ordnance});
  context.vehicles.f2.seats.attachTurrets(crews);
  return live;
}

test('native launchers observe a crew death earlier in the same unit span', async() => {
  const blade = {class:'blade',base:'melee',channel:0,AnimationBank:'saber',swings:[{duration:.2,hit:.05}]};
  const missile = weaponDefinition('crew-lock',0,{base:'launcher',LockOnRange:100,LockOnAngle:45,LockOffAngle:50,
    LockTime:1,TargetEnemy:1,TargetFriendly:0,TargetNeutral:0,TargetBuilding:1,TargetPerson:0,
    ordnance:{class:'crew-missile',base:'missile'}});
  const world = await f3World({
    hero:{...soldier,class:'hero',weapons:[blade]},
    crew:{...soldier,class:'crew',MaxHealth:500},
    launcher:{...soldier,class:'launcher',weapons:[missile]},
  }, {prepareFleet:fleetFixture({positions:[]}),prepareLive(collision,units,context) {
    // Keep the attacker standing beside the elevated seat. Falling blocks
    // infantry weapons, so empty space here would never exercise the strike.
    collision.addBody(new Float32Array([10,3,20,30,3,20,30,3,35,10,3,35]),
      new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]),null);
    return liveTurret(collision,units,context,{health:100000,exposed:true});
  }});
  const hero = world.spawn('hero',20,28.5), crew = world.spawn('crew',20,30);
  const launcher = world.spawn('launcher',20,25,true), turret = world.live.turrets[0];
  for (const unit of [hero,crew,launcher]) {
    world.phase.begin(world.phase.bindings[unit.id],'team');
    world.phase.change.value = unit === crew ? 2 : 1; world.phase.commit();
  }
  world.phase.placeBody(hero,20,3,28.5,0); world.phase.placeBody(launcher,20,3,25,0);
  assert.equal(turret.crew.man(crew,turret),true);
  hero.combat.arsenal.weapons[0].random = world.ordnance.random;
  hero.intent.buttons[0] = 1;
  world.phase.phase(1/60);
  assert.equal(launcher.combat.arsenal.weapons[0].lockTargetId,turret.aiTarget.id);
  for (let i=0;i<8 && crew.alive;i++) world.phase.phase(1/60);
  assert.equal(crew.dead,true);
  assert.equal(turret.alive,true);
  assert.equal(turret.aiTarget.team,0);
  assert.equal(launcher.combat.arsenal.weapons[0].lockTargetId,-1);
});

test('native turret damage samples its occupied target team instead of the authored prop team', async() => {
  const notices = [];
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[]}),
    prepareLive(collision,units,context) {
      const live = liveTurret(collision,units,context);
      live.turrets[0].onDamaged = (object,amount,attacker,hit,sample) => notices.push(sample.team);
      return live;
    },
  });
  const turret = world.live.turrets[0], crew = world.spawn('soldier',20,30,true);
  world.phase.begin(world.phase.bindings[crew.id],'team'); world.phase.change.value = 1; world.phase.commit();
  assert.equal(turret.crew.man(crew,turret),true);
  assert.equal(turret.team,0); assert.equal(turret.aiTarget.team,1);
  world.phase.bodiesIn();
  world.ordnance.explosion({class:'pulse',Damage:10,DamageRadiusInner:5,DamageRadiusOuter:5},
    new THREE.Vector3(20,4,30),crew);
  assert.deepEqual(notices,[1]);
  // Destruction releases the crew before Turret.damage notifies its host.
  // The phase-entry target team is still one, but this notice must use zero.
  world.ordnance.explosion({class:'fatal-pulse',Damage:5000,DamageRadiusInner:5,DamageRadiusOuter:5},
    new THREE.Vector3(20,4,30),crew);
  assert.equal(crew.dead,true);
  assert.deepEqual(notices,[1,0]);
});

// TurretSeats.drive retains the changed AI generator and modifier row, then
// emits bind, sound and fired after its direct native Ordnance.fire call.
test('owned stationary crews fire through the combined phase without geometry or RNG imports', {timeout:10000}, async() => {
  const world = await f3World({soldier},{prepareFleet:fleetFixture({positions:[]}),prepareLive:liveTurret});
  const turret = world.live.turrets[0], crews = turret.crew;
  const crew = world.spawn('soldier',20,30,true);
  world.phase.begin(world.phase.bindings[crew.id],'team'); world.phase.change.value = 1; world.phase.commit();
  assert.equal(crews.man(crew,turret),true);
  const draw = uniformRandom(new ClassicRandom(29)); turret.weapon.random = draw;
  turret.weapon.spreadScale = .5;
  const expected = new ClassicRandom(randomBinding(draw).random.state);
  for (let i=0;i<4;i++) expected.uniform();
  const forbidden = ()=>{throw Error('foreign stationary geometry or RNG reached');};
  const handlers = world.phase.bridge.handlers;
  handlers.f2TurretDraw = handlers.f2TurretTarget = handlers.f2Posed = handlers.f2Foreign = forbidden;
  const notices = [];
  crews.onFired = (object,index,shot,owner,origin,direction,sample)=>{
    assert.equal(world.phase.active,false); assert.ok(object === turret); assert.ok(owner === crew);
    notices.push([index,sample.source.generation,sample.mount.generation,sample.team]);
  };
  crew.intent.buttons[0] = 1;
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.deepEqual(notices,[[0,1,1,1]]);
  assert.equal(randomBinding(draw).random.state,expected.state);
  assert.ok(turret.weapon.random === draw); assert.equal(turret.weapon.spreadScale,.5);
  assert.equal(world.phase.counters.phaseCalls,1); assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0); assert.equal(world.phase.counters.rustReallocations,0);
});

// A mounted crew remains fixed until lethal damage releases its lease.
// PlayerCombat.explode applies the push after that immediate death transition.
test('native blasts release exposed turret crews before pushing their corpses', async() => {
  const world=await f3World({soldier:{...soldier,MaxHealth:300}}, {
    prepareFleet:fleetFixture({positions:[]}),
    prepareLive:(collision,units,context)=>liveTurret(collision,units,context,{health:100000,exposed:true}),
  });
  const turret=world.live.turrets[0],crew=world.spawn('soldier',20,30,true);
  assert.equal(turret.crew.man(crew,turret),true);
  const point=crew.position.clone().add(new THREE.Vector3(2,1,0));
  const shove={Damage:100,DamageRadiusInner:4.5,DamageRadiusOuter:6.5,Push:10,PushRadiusInner:4.5,PushRadiusOuter:4.5};
  crew.combat.explode(shove,point);
  assert.equal(crew.combat.health,200);assert.equal(crew.velocity.length(),0);
  assert.ok(crew.seat);
  crew.combat.explode({...shove,Damage:320},point);
  assert.equal(crew.combat.health,0);assert.equal(crew.seat,null);
  assert.ok(crew.velocity.length()>0);assert.equal(crew.soldier.air,'thrown');
});
