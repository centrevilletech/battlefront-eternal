import test from 'node:test';
import assert from 'node:assert/strict';
import {Group,Vector3} from 'three';
import {Unit} from '../src/units.js';
import {SoldierPresentation} from '../src/soldier-presentation.js';
import {unitHost,presentUnit} from './helpers/presentation.js';
import {UnitOutput as O} from '../src/presentation/layout.js';
const ANIM={alertOn:2.5,alertHold:2};
import {createPageNetwork} from '../src/runtime/page-network.js';
import {SnapshotPresentation} from '../src/runtime/snapshot-timeline.js';
import {pushTimeline,sampledCounter} from './helpers/network-capture.js';
import {ENTITY_SOLDIER_STATES} from '../src/runtime/network-entity-codec.js';

const record=(tick,fireCounter,extra={})=>({kind:'soldier',id:1,generation:1,tick,position:[0,0,0],velocity:[0,0,0],yaw:.7,pitch:.2,health:100,posture:0,state:0,fireCounter,...extra});
function fixture() {
  const soldier={position:new Vector3(),velocity:new Vector3(),aimYaw:2,aimPitch:-.5,posture:'stand',air:null};
  const presentation=new SoldierPresentation(soldier);
  const forbidden=()=>assert.fail('counter recovery must not replay shot cosmetics or weapon simulation');
  const combat={presentProjectileShot:forbidden,update:forbidden,flash:forbidden};
  const unit=new Unit({def:{},actor:{root:new Group()},soldier,presentation,animator:{},combat});unit.id=1;unit.generation=1;unit.removed=false;unit.alive=true;
  const host=unitHost(unit),animator=unit.animator;
  Object.defineProperty(animator,'alert',{get(){return host.owner.output[host.row.id*O.STRIDE+O.ANIMATOR+6];}});
  Object.defineProperty(animator,'drawnAim',{get(){return [presentation.state.aimYaw,presentation.state.aimPitch];}});
  const timeline=createPageNetwork(),view=new SnapshotPresentation(timeline);let generation=1;
  const push=(...records)=>{for(const r of records){generation=r.generation;unit.generation=generation;pushTimeline(timeline,r.tick,new Map([[r.id,r]]),1);}};
  const draw=(tick,dt=0,counter)=>{view.bindSoldier(1,generation,presentation);view.update(tick);if(counter!==undefined)sampledCounter(timeline,1,counter);presentUnit(unit,dt,1);};
  return {unit,soldier,presentation,animator,push,draw,view};
}

test('lost shot events recover remote aim stance only when the rendered sample reaches the shot counter',()=>{
  const f=fixture();
  f.push(record(10,5),record(20,6));
  f.draw(10);assert.equal(f.animator.alert,-1,'joining does not replay historic firing');
  f.draw(19);assert.equal(f.animator.alert,-1,'future authority cannot animate firing in the older pose');
  f.draw(20);assert.equal(f.animator.alert,ANIM.alertOn,'a shot count without any shot event restores aiming');
  assert.deepEqual(f.animator.drawnAim,[.7,.2]);
  assert.equal(f.soldier.aimYaw,2,'rendering does not change authority');
  f.animator.reset(f.soldier.aimYaw);
  f.draw(20);f.draw(23);assert.equal(f.animator.alert,-1,'held sample never retriggers');
  f.push(record(30,6));f.draw(30);assert.equal(f.animator.alert,-1,'unchanged count never retriggers');
  f.push(record(40,9));f.draw(40);assert.equal(f.animator.alert,ANIM.alertOn,'missing multiple shots still causes just one stance update');
});

test('counter recovery preserves an ordinary first-shot blend and sustains alert without replaying weapon actions',()=>{
  const f=fixture();f.push(record(10,0));f.draw(10);
  f.animator.fire();f.draw(10,.05);
  f.push(record(20,1));f.draw(20);
  assert.equal(f.animator.alert,ANIM.alertOn-.05,'matching event and counter must not skip the onset blend');
  f.draw(20,2.35);f.push(record(30,2));f.draw(30);
  assert.equal(f.animator.alert,ANIM.alertHold,'ongoing fire sustains an already-alert torso');
});

test('new lives, presentation resets and dead sampled bodies do not recover historic firing',()=>{
  const f=fixture();f.push(record(10,100));f.draw(10);
  f.push(record(20,101,{health:0,state:ENTITY_SOLDIER_STATES.indexOf('dead')}));f.draw(20);
  assert.equal(f.animator.alert,-1,'death cannot raise the weapon');
  f.push(record(30,200,{generation:2}));f.draw(30);
  assert.equal(f.animator.alert,-1,'another generation establishes its own baseline');
  f.presentation.reset();f.push(record(40,201,{generation:2}));f.draw(40);
  assert.equal(f.animator.alert,-1,'reset suppresses a previously unseen historical increase');
  f.push(record(50,202,{generation:2}));f.draw(50);
  assert.equal(f.animator.alert,ANIM.alertOn,'the next shot of the new life still works');
});

test('rewound or decreasing sampled counters cannot trigger or lower the firing high-water mark',()=>{
  const f=fixture();
  const draw=pose=>{f.push(pose);f.draw(pose.tick,0,pose.fireCounter);};
  draw(record(20,10));draw(record(10,11));draw(record(30,9));draw(record(40,10));
  assert.equal(f.animator.alert,-1);
  draw(record(50,11));assert.equal(f.animator.alert,ANIM.alertOn);
  f.animator.reset(f.soldier.aimYaw);f.view.unbind(1);presentUnit(f.unit,0,1);
  draw(record(60,20));assert.equal(f.animator.alert,-1,'leaving and returning to replica presentation establishes a baseline');
});
