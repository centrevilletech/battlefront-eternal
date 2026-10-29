import test from 'node:test';
import assert from 'node:assert/strict';
import {Arsenal} from '../src/weapons.js';
import {PlayerCombat} from '../src/combat.js';
import {prepareMelee} from '../src/heroes.js';
import {WeaponPrediction,predictedPress} from '../src/runtime/weapon-prediction.js';
import {captureWeaponState} from '../src/runtime/network-weapon-state.js';
import {captureArsenalWeapon,validateArsenalWeapon,applyArsenalWeapon} from '../src/runtime/network-arsenal-state.js';

const weapon=(name,channel=0,extra={})=>({class:name,base:'cannon',channel,RoundsPerClip:10,ammo:4,ShotDelay:.1,ReloadTime:.1,InitialSalvoDelay:0,SalvoCount:1,ShotsPerSalvo:1,SalvoDelay:0,ordnance:{base:'bolt',Velocity:100,LifeSpan:1},...extra});
function fixture(definitions=[weapon('rifle'),weapon('pistol'),weapon('secondary',1)]){
  const soldier={posture:'stand',aimYaw:0,aimPitch:0};
  const combat=Object.create(PlayerCombat.prototype);
  Object.assign(combat,{arsenal:prepareMelee(new Arsenal(definitions,{random:()=>.5})),soldier,zoomStep:0,dead:false});
  return {id:1,generation:1,alive:true,soldier,combat};
}
const command=(actions=[],down=[])=>({actions,down});
const bytes=arsenal=>arsenal.weapons.map(captureWeaponState);

test('shared replay finishes an authoritative reload and fires without waiting for another correction',()=>{
  const unit=fixture(),authority=unit.combat.arsenal,gun=authority.current(0);
  Object.assign(gun,{clip:0,reserve:2,reloadLeft:2/60});
  const before=bytes(authority),prediction=new WeaponPrediction(),fired=[];
  prediction.begin(unit,{tick:10});
  for(let tick=11;tick<=14;tick++)prediction.step(command([],['Mouse0']),tick,(shot,c,t)=>fired.push(t));
  assert.deepEqual(fired,[13],'Weapon.update ends reload before accepting fire on the following tick');
  assert.equal(unit.combat.arsenal.current(0).reloading,false);
  assert.ok(unit.combat.arsenal.current(0).clip<1);
  assert.deepEqual(bytes(authority),before,'replay cannot spend authoritative ammo or advance its timers');
  prediction.restore();assert.equal(unit.combat.arsenal,authority);assert.deepEqual(bytes(authority),before);
});

test('switching cancels reload through shared Arsenal rules and both channels resume independently',()=>{
  const unit=fixture(),authority=unit.combat.arsenal,prediction=new WeaponPrediction();
  Object.assign(authority.current(0),{clip:0,reserve:3,reloadLeft:.08});
  prediction.begin(unit,{tick:0});
  const shots=[];
  prediction.step(command(['nextPrimary','firePrimary'],['Mouse0']),1,shot=>shots.push(shot.weapon.name));
  assert.equal(unit.combat.arsenal.current(0).name,'pistol');
  assert.equal(unit.combat.arsenal.weapons[0].reloadLeft,0);
  prediction.step(command(['fireSecondary'],['Mouse0','Mouse2']),2,shot=>shots.push(shot.weapon.name));
  assert.deepEqual(shots,['pistol','secondary'],'secondary press takes priority through Arsenal.update');
  prediction.step(command(['previousPrimary']),3);
  assert.equal(unit.combat.arsenal.current(0).name,'rifle');
  assert.ok(unit.combat.arsenal.current(0).reloading,'returning to an empty weapon restarts its reload');
  prediction.restore();assert.equal(authority.current(0).name,'rifle');assert.equal(authority.current(0).reloadLeft,.08);
});

test('corrections replay lost snapshots and held fire without duplicate cosmetic shots',()=>{
  const unit=fixture(),prediction=new WeaponPrediction(),emitted=[];
  prediction.begin(unit,{tick:0});
  for(let tick=1;tick<=12;tick++)prediction.step(command(tick===1?['firePrimary']:[],['Mouse0']),tick,(shot,c,t)=>emitted.push(t));
  const first=emitted.slice(),checkpoint=prediction.checkpoint(),expected=bytes(unit.combat.arsenal);
  prediction.restore();prediction.begin(unit,{tick:0});
  for(let tick=1;tick<=12;tick++)prediction.step(command(tick===1?['firePrimary']:[],['Mouse0']),tick,(shot,c,t)=>emitted.push(t));
  assert.deepEqual(emitted,first,'replaying a correction never repeats already-presented shots');
  assert.deepEqual(bytes(unit.combat.arsenal),expected);
  prediction.restore();prediction.begin(unit,{tick:0,checkpoint});
  for(let tick=13;tick<=30;tick++)prediction.step(command([],['Mouse0']),tick,(shot,c,t)=>emitted.push(t));
  assert.deepEqual(emitted,[1,7,13,19,25],'checkpoint continuation preserves shared fractional cadence');
  prediction.restore();
  // A newer authoritative checkpoint rejects the previous speculative ammo.
  unit.combat.arsenal.current(0).clip=.2;
  prediction.begin(unit,{tick:30});
  assert.equal(unit.combat.arsenal.current(0).clip,.2);
  prediction.step(command([],['Mouse0']),31,(shot,c,t)=>emitted.push(t));
  assert.equal(emitted.at(-1),31);
  prediction.restore();
});

test('lossless complete weapon checkpoints preserve inactive heat, charge, salvo and melee timelines',()=>{
  const unit=fixture([weapon('charged',0,{MaxPressedTime:.6}),weapon('burst',0,{SalvoCount:3,SalvoDelay:.07}),weapon('blade',1,{base:'melee',swings:[{duration:1,hit:.3,sound:null}]})]);
  const target=fixture(unit.combat.arsenal.weapons.map(w=>w.levels[0]));
  const source=unit.combat.arsenal;
  Object.assign(source.weapons[0],{charging:true,held:.234567890123,heat:.4,wait:Number.EPSILON,limit:.6});
  Object.assign(source.weapons[1],{salvosLeft:2,salvoWait:.023,clip:.6,overheated:true,pattern:2});
  source.weapons[2].update(1/60,true,{pressed:true,held:true});
  source.weapons[2].update(1/60,true,{pressed:false,held:true});
  for(let i=0;i<source.weapons.length;i++)applyArsenalWeapon(validateArsenalWeapon(target.combat.arsenal.weapons[i],captureArsenalWeapon(source.weapons[i])));
  assert.deepEqual(bytes(target.combat.arsenal),bytes(source));
  assert.deepEqual(target.combat.arsenal.weapons[2].melee.captureState(),source.weapons[2].melee.captureState());
  const prediction=new WeaponPrediction();prediction.begin(unit,{tick:10});
  prediction.step(command(),11);
  assert.ok(unit.combat.arsenal.weapons[2].melee.time>source.weapons[2].melee.time);
  prediction.restore();
});

test('private shared rules never call authority admission callbacks or its random stream',()=>{
  const unit=fixture(),authority=unit.combat.arsenal;
  for(const weapon of authority.weapons){weapon.random=()=>assert.fail('authority RNG used');weapon.admitShot=()=>assert.fail('authority admission used');}
  const before=bytes(authority),prediction=new WeaponPrediction();prediction.begin(unit,{tick:0});
  for(let tick=1;tick<60;tick++)prediction.step(command(tick===1?['firePrimary']:[],['Mouse0']),tick);
  prediction.restore();assert.deepEqual(bytes(authority),before);
});

test('cosmetic press identity excludes prior rejected taps and ambiguous collapsed clicks',async()=>{
  const {replayTicks}=await import('../src/runtime/reconcile.js');
  const edges=[{id:11,sequence:1,button:'firePrimary'},{id:13,sequence:2,button:'firePrimary'},{id:14,sequence:2,button:'fireSecondary'}];
  const commands=[1,2].map(sequence=>({sequence,tick:9+sequence,generation:1,down:['Mouse0'],actions:['firePrimary'],look:[]}));
  replayTicks(commands,12,1,[],(tick,command,sequences)=>{
    assert.equal(predictedPress(edges,sequences,'firePrimary'),false,'no false claim for the last of two collapsed taps');
    assert.equal(predictedPress(edges,sequences,'fireSecondary').id,14,'other channel remains uniquely attributable');
  });
  assert.equal(predictedPress(edges,[2],'firePrimary').id,13);
  assert.equal(predictedPress(edges,[3],'firePrimary'),null,'old rejected press cannot claim a later held shot');
});
