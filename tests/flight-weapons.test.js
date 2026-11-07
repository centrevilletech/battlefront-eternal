import { stepPrediction } from './helpers/hull-prediction.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fixture } from './flight-fixture.js';
import { CommandControls } from '../src/runtime/commands.js';
import { flightBindings, flightInput } from '../src/flight/controls.js';


test('independent flight bindings and gamepad produce the specified virtual controls',()=>{
 const c=new CommandControls();c.accept({down:['KeyW','ShiftLeft','KeyA','KeyZ'],actions:['takeoff','targetLock']});
 const input=flightInput(c,flightBindings());assert.equal(input.forward,1);assert.equal(input.boost,true);assert.equal(input.right,-1);assert.equal(input.rollLock,true);assert.equal(input.takeoff,true);assert.equal(input.targetLock,true);
 c.accept({down:['KeyR'],actions:[]});assert.equal(flightInput(c,flightBindings({throttle:['KeyR']})).forward,1);
 c.accept({down:[],actions:[]});const buttons=Array.from({length:12},()=>({pressed:false}));buttons[11].pressed=true;buttons[0].pressed=true;
 const pad=flightInput(c,flightBindings(),{axes:[.7,-.8,.6,-1],buttons});assert.equal(pad.stickX,.7);assert.equal(pad.stickY,.8);assert.equal(pad.right,.6);assert.equal(pad.forward,1);assert.equal(pad.boost,true);assert.equal(pad.trick,true);
});
test('trick cannot be interrupted by landing input and respects exhausted power/cooldown',()=>{
 const f=fixture();f.v.flightState='Flying';f.v.position.y=500;f.system.flightEnvironment={space:true};f.v.airspeed=65;
 const tick=i=>stepPrediction(f.system, 1/60,f.v,i);
 f.v.power=1;tick({immelmann:true});assert.equal(f.v.trick,'');tick({});f.v.power=100;tick({immelmann:true});assert.equal(f.v.trick,'immelmann');tick({takeoff:true});assert.equal(f.v.trick,'immelmann');
 for(let i=0;i<90;i++)tick({rollLock:true});assert.equal(f.v.trick,'');assert.ok(f.v.trickCooldown>0);tick({immelmann:true});assert.equal(f.v.trick,'');
});

test('Immelmann from a bank rolls smoothly into a reversed level exit',()=>{
 const f=fixture();f.v.flightState='Flying';f.v.position.y=500;f.v.airspeed=65;f.system.flightEnvironment={space:true};f.v.quaternion.set(0,0,Math.sin(Math.PI/4),Math.cos(Math.PI/4));let largest=0;
 for(let i=0;i<100;i++){const prev=f.v.quaternion.clone();stepPrediction(f.system, 1/60,f.v,{immelmann:i===0,rollLock:true});largest=Math.max(largest,prev.angleTo(f.v.quaternion));}
 const q=f.v.quaternion;assert.ok(largest<.12,'bank snapped at the end');assert.ok(Math.abs(q.x)<.001&&Math.abs(q.z)<.001&&Math.abs(q.y)>.999,'exit not reversed and level');
});
