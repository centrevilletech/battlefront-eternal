import test from 'node:test';
import assert from 'node:assert/strict';
import {Group,Vector3} from 'three';
import {Unit} from '../src/units.js';
import {SoldierPresentation} from '../src/soldier-presentation.js';
import {SoldierAnimator,ANIM} from '../src/animator.js';
import {EntityTimeline} from '../src/runtime/network-entity-timeline.js';
import {EntityPresentation} from '../src/runtime/network-entity-presentation.js';
import {ENTITY_SOLDIER_STATES} from '../src/runtime/network-entity-codec.js';

const record=(tick,fireCounter,extra={})=>({kind:'soldier',id:1,generation:1,tick,position:[0,0,0],velocity:[0,0,0],yaw:.7,pitch:.2,health:100,posture:0,state:0,fireCounter,...extra});
function fixture() {
  const soldier={position:new Vector3(),velocity:new Vector3(),aimYaw:2,aimPitch:-.5,posture:'stand',air:null};
  const presentation=new SoldierPresentation(soldier);
  const animator={alert:-1,fire:SoldierAnimator.prototype.fire,holdAlert:SoldierAnimator.prototype.holdAlert,update(dt,state){this.drawnAim=[state.aimYaw,state.aimPitch];}};
  const forbidden=()=>assert.fail('counter recovery must not replay shot cosmetics or weapon simulation');
  const combat={presentProjectileShot:forbidden,update:forbidden,flash:forbidden};
  const unit=new Unit({def:{},actor:{root:new Group(),byName:{}},soldier,presentation,animator,combat});
  unit.removed=false;unit.alive=true;
  const timeline=new EntityTimeline(),view=new EntityPresentation(timeline);
  const push=(...records)=>timeline.push({epoch:1,records});
  const draw=tick=>{view.bindSoldier(1,timeline.entities.get(1).generation,presentation);view.update(tick);unit.present(1/60,1);};
  return {unit,soldier,presentation,animator,push,draw};
}

test('lost shot events recover remote aim stance only when the rendered sample reaches the shot counter',()=>{
  const f=fixture();
  f.push(record(10,5),record(20,6));
  f.draw(10);assert.equal(f.animator.alert,-1,'joining does not replay historic firing');
  f.draw(19);assert.equal(f.animator.alert,-1,'future authority cannot animate firing in the older pose');
  f.draw(20);assert.equal(f.animator.alert,ANIM.alertOn,'a shot count without any shot event restores aiming');
  assert.deepEqual(f.animator.drawnAim,[.7,.2]);
  assert.equal(f.soldier.aimYaw,2,'rendering does not change authority');
  f.animator.alert=-1;
  f.draw(20);f.draw(23);assert.equal(f.animator.alert,-1,'held sample never retriggers');
  f.push(record(30,6));f.draw(30);assert.equal(f.animator.alert,-1,'unchanged count never retriggers');
  f.push(record(40,9));f.draw(40);assert.equal(f.animator.alert,ANIM.alertOn,'missing multiple shots still causes just one stance update');
});

test('counter recovery preserves an ordinary first-shot blend and sustains alert without replaying weapon actions',()=>{
  const f=fixture();f.push(record(10,0));f.draw(10);
  f.animator.fire();f.animator.alert-=.05;
  f.push(record(20,1));f.draw(20);
  assert.equal(f.animator.alert,ANIM.alertOn-.05,'matching event and counter must not skip the onset blend');
  f.animator.alert=.1;f.push(record(30,2));f.draw(30);
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
  const draw=pose=>{f.presentation.networkPose={...pose,fromTick:pose.tick};f.unit.present(1/60,1);};
  draw(record(20,10));draw(record(10,11));draw(record(30,9));draw(record(40,10));
  assert.equal(f.animator.alert,-1);
  draw(record(50,11));assert.equal(f.animator.alert,ANIM.alertOn);
  f.animator.alert=-1;f.presentation.networkPose=null;f.unit.present(1/60,1);
  draw(record(60,20));assert.equal(f.animator.alert,-1,'leaving and returning to replica presentation establishes a baseline');
});
