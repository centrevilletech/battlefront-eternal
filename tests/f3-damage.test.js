import {f3World} from './fixtures/f3-world.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { nativeCombat } from './fixtures/native-combat.js';
import { Soldier, ENGINE } from '../src/soldier.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { captureEntitySoldier, ENTITY_SOLDIER_STATES } from '../src/runtime/network-entity-capture.js';

const grenade = {Damage:320,DamageRadiusInner:4.5,DamageRadiusOuter:6.5,Push:10,PushRadiusInner:4.5,PushRadiusOuter:4.5};
const shove = {...grenade,Damage:0};
const near = (a,b,tolerance=1e-6) => assert.ok(Math.abs(a-b)<=tolerance, `${a} != ${b}`);

// PlayerCombat.explode measured from a posture-dependent sphere, subtracting
// half its radius. These independent numbers retain that historical rule.
test('native blast falloff keeps each posture sphere and explicit friendly-fire scale', async() => {
  const blast = {Damage:320,DamageRadiusInner:.75,DamageRadiusOuter:2};
  for (const [posture,middle,radius] of [['stand',1,1.4626],['crouch',.75,1.1023],['prone',.35,1.622]]) {
    const {combat} = await nativeCombat({posture});
    combat.explode(blast,new Vector3(2,.2,0));
    near(300-combat.health,320*(2-(Math.hypot(2,middle-.2)-radius/2))/1.25);
  }
  const {combat} = await nativeCombat();
  combat.explode(grenade,new Vector3(2,1,0),null,0);
  assert.equal(combat.health,300);
});

test('native blast cover uses the sight point and rejects unreachable bodies', async() => {
  for (const [height,health] of [[.7,0],[1.2,300]]) {
    const body = await nativeCombat({wall:{x:1.5,height}});
    body.combat.explode(grenade,new Vector3(3,.1,0));
    assert.equal(body.combat.health,health);
  }
  const body = await nativeCombat();
  body.combat.explode(grenade,new Vector3(40,1,0));
  assert.equal(body.combat.health,300);
  assert.deepEqual(body.soldier.velocity.toArray(),[0,0,0]);
});

test('native blast push preserves upward bias, falloff and the throw threshold', async() => {
  const body = await nativeCombat();
  body.combat.explode(shove,new Vector3(3,.1,0));
  const away = new Vector3(-3,.9,0).normalize();
  near(body.soldier.velocity.x,away.x*10);
  near(body.soldier.velocity.y,away.y*10+6);
  assert.equal(body.soldier.air,'thrown');
  assert.equal(body.soldier.grounded,false);
  assert.equal(body.soldier.stateTime,0);
  const half = await nativeCombat();
  half.combat.explode({Push:10,PushRadiusInner:2,PushRadiusOuter:6},new Vector3(4+1.4626/2,1,0));
  near(half.soldier.velocity.x,-5);near(half.soldier.velocity.y,3);
  const nudge = await nativeCombat();
  nudge.combat.explode({Push:1,PushRadiusInner:4,PushRadiusOuter:4},new Vector3(3,1,0));
  near(nudge.soldier.velocity.x,-1);near(nudge.soldier.velocity.y,.6);
  assert.equal(nudge.soldier.air,null);
});

test('a native lethal blast retains thrown flight, correction and final corpse state', async() => {
  const {world,unit,combat,soldier} = await nativeCombat();
  combat.explode(grenade,new Vector3(2.7,.1,0));
  assert.equal(combat.health,0);assert.equal(unit.dead,true);
  assert.deepEqual(combat.log,{deaths:1,apart:0,died:1});
  const state = () => ENTITY_SOLDIER_STATES[captureEntitySoldier(unit,{tick:1,classId:1}).state];
  assert.equal(state(),'thrownDead');
  const client = new Soldier(soldier.world,{});
  client.spawn(0,0,0,0);
  restoreCompactSoldier(client,captureCompactSoldier(soldier));
  assert.deepEqual(client.velocity.toArray(),soldier.velocity.toArray());
  const start = soldier.position.y, lift = soldier.velocity.y;
  let rise=0, landed=0;
  for (let tick=1;tick<=180;tick++) {
    world.phase.phase(1/60);
    rise=Math.max(rise,soldier.position.y-start);
    if (!soldier.air&&soldier.grounded) {landed=tick;break;}
  }
  near(rise,lift*lift/(2*ENGINE.thrownGravity),.15);
  assert.ok(landed>ENGINE.landLock*60);
  assert.ok(soldier.position.x < -3);
  assert.equal(state(),'dead');
});

test('native corpse blasts do not repeat death and blown-apart bodies receive no push', async() => {
  const corpse = await nativeCombat();
  corpse.combat.dead=true;corpse.combat.health=0;
  corpse.combat.explode(grenade,new Vector3(3,.1,0));
  assert.equal(corpse.soldier.air,'thrown');assert.equal(corpse.combat.log.deaths,0);
  const apart = await nativeCombat({chunks:1});
  apart.combat.random=apart.world.ordnance.random;
  apart.combat.explode(grenade,new Vector3(2.7,.1,0));
  assert.equal(apart.combat.dead,true);assert.equal(apart.combat.log.apart,1);
  assert.deepEqual(apart.soldier.velocity.toArray(),[0,0,0]);
  apart.combat.explode(grenade,new Vector3(2.7,.1,0));
  assert.deepEqual(apart.soldier.velocity.toArray(),[0,0,0]);
});

test('native explosions without complete push radii hurt without throwing', async() => {
  for (const push of [{},{Push:0,PushRadiusInner:0,PushRadiusOuter:0},{Push:10}]) {
    const body=await nativeCombat();
    body.combat.explode({Damage:50,DamageRadiusInner:2,DamageRadiusOuter:6,...push},new Vector3(2,.9,0));
    assert.equal(body.combat.health,250);
    assert.deepEqual(body.soldier.velocity.toArray(),[0,0,0]);
    assert.equal(body.soldier.air,null);
  }
});

test('native droideka shields absorb blasts before health without throwing the body', async() => {
  for (const value of [4000,10,0]) {
    const hits=[];
    const shieldDef={class:'shield',base:'shield',channel:1,MaxShield:4000};
    const body=await nativeCombat({base:'walkerdroid',health:100,weapons:[shieldDef],velocity:new Vector3(1,0,2),
      shield:{on:value>0,value,max:4000,drain:0,def:shieldDef},onShield:(unit,taken,source,kind)=>hits.push([taken,source.toArray(),kind])});
    body.combat.explode({...grenade,Damage:40,Push:30},new Vector3(2,.1,0));
    assert.equal(body.combat.health,100-Math.max(40-value,0));
    assert.deepEqual(body.soldier.velocity.toArray(),[1,0,2]);
    assert.equal(body.soldier.shield.value,Math.max(value-40,0));
    assert.deepEqual(hits,value?[ [Math.min(value,40),[2,.1,0],'explosion'] ]:[]);
  }
});

// Pure observers run outside the native phase and may issue another command.
// Their command must preserve the unread notifications from the original hit.
test('a damage observer command preserves the remaining native blast notices',async()=>{
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]}});
  const a=w.spawn('soldier',0,0),b=w.spawn('soldier',2,0),c=w.spawn('soldier',40,0);
  const seen=[];
  a.combat.onDamaged=amount=>{seen.push(['a',amount]);c.combat.health=1999;};
  b.combat.onDamaged=amount=>seen.push(['b',amount]);
  w.ordnance.explosion({class:'observer-blast',Damage:10,DamageRadiusInner:8,DamageRadiusOuter:8},new Vector3(1,1,0),null);
  assert.deepEqual(seen,[['a',10],['b',10]]);
  assert.equal(c.combat.health,1999);
  w.phase.phase(1/60);
  assert.deepEqual(seen,[['a',10],['b',10]],'consumed notices are never replayed');
});
