import test from 'node:test';
import assert from 'node:assert/strict';
import { fixture } from './flight-fixture.js';
import { CommandControls } from '../src/runtime/commands.js';
import { flightBindings, flightInput } from '../src/flight/controls.js';

test('flight secondary emission dumbfires without a completed explicit lock',()=>{
 const f=fixture();f.v.flightState='Flying';
 const emitted=[];f.system.ordnance={lights:{flash(){}},fire(def,origin,direction,owner,unused,body,emitter,shot){const r={kind:'missile',velocity:direction.clone(),targetId:shot.targetId};emitted.push(r);return r;}};
 const weapon={def:{ordnance:{base:'missile'}}};f.v.direction.set(0,0,1);
 for(const state of ['', 'Locking','Locked']) {f.v.flightLockState=state;f.v.flightLockId=4;f.v.emitShot({weapon},0,null);}
 assert.deepEqual(emitted.map(r=>r.targetId),[-1,-1,4]);
 for(const r of emitted){assert.equal(r.flightMissile,true);assert.equal(r.velocity.length(),180);assert.equal(r.life,8);}
});
test('independent flight bindings and gamepad produce the specified virtual controls',()=>{
 const c=new CommandControls();c.accept({down:['KeyW','ShiftLeft','KeyA','KeyZ'],actions:['takeoff','targetLock']});
 const input=flightInput(c,flightBindings());assert.equal(input.forward,1);assert.equal(input.boost,true);assert.equal(input.right,-1);assert.equal(input.rollLock,true);assert.equal(input.takeoff,true);assert.equal(input.targetLock,true);
 c.accept({down:['KeyR'],actions:[]});assert.equal(flightInput(c,flightBindings({throttle:['KeyR']})).forward,1);
 c.accept({down:[],actions:[]});const buttons=Array.from({length:12},()=>({pressed:false}));buttons[11].pressed=true;buttons[0].pressed=true;
 const pad=flightInput(c,flightBindings(),{axes:[.7,-.8,.6,-1],buttons});assert.equal(pad.stickX,.7);assert.equal(pad.stickY,.8);assert.equal(pad.right,.6);assert.equal(pad.forward,1);assert.equal(pad.boost,true);assert.equal(pad.trick,true);
});
test('trick cannot be interrupted by landing input and respects exhausted power/cooldown',()=>{
 const f=fixture();f.v.flightState='Flying';f.v.position.y=500;f.system.flightEnvironment={space:true};f.v.airspeed=65;
 const tick=i=>f.system.update(1/60,f.v,i);
 f.v.power=1;tick({immelmann:true});assert.equal(f.v.trick,'');tick({});f.v.power=100;tick({immelmann:true});assert.equal(f.v.trick,'immelmann');tick({takeoff:true});assert.equal(f.v.trick,'immelmann');
 for(let i=0;i<90;i++)tick({rollLock:true});assert.equal(f.v.trick,'');assert.ok(f.v.trickCooldown>0);tick({immelmann:true});assert.equal(f.v.trick,'');
});

test('AI landing intent descends into the landing envelope through virtual pitch',async()=>{
 const {pilotInput}=await import('../src/flight/ai.js');const f=fixture();f.v.flightState='Flying';f.v.position.y=35;f.v.airspeed=65;f.system.flightEnvironment={space:false,ceiling:200};const unit={vehicle:f.v,steer:{land:true}};
 for(let i=0;i<600&&f.v.flightState!=='Landed';i++){const intent=pilotInput(unit,null,null,1/60,{},{});f.system.update(1/60,f.v,{forward:intent.forward,stickY:intent.pitch,takeoff:intent.down});}
 assert.equal(f.v.flightState,'Landed');assert.equal(f.v.health,f.v.maxHealth);
});

test('Immelmann from a bank rolls smoothly into a reversed level exit',()=>{
 const f=fixture();f.v.flightState='Flying';f.v.position.y=500;f.v.airspeed=65;f.system.flightEnvironment={space:true};f.v.quaternion.set(0,0,Math.sin(Math.PI/4),Math.cos(Math.PI/4));let largest=0;
 for(let i=0;i<100;i++){const prev=f.v.quaternion.clone();f.system.update(1/60,f.v,{immelmann:i===0,rollLock:true});largest=Math.max(largest,prev.angleTo(f.v.quaternion));}
 const q=f.v.quaternion;assert.ok(largest<.12,'bank snapped at the end');assert.ok(Math.abs(q.x)<.001&&Math.abs(q.z)<.001&&Math.abs(q.y)>.999,'exit not reversed and level');
});
