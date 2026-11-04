import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { attachNetworkSeatsSim, networkSeatsPort } from '../src/runtime/network-seats-sim.js';
import { cycleNetworkSeat } from '../src/runtime/network-seat.js';
import { networkVehicleInput } from '../src/runtime/network-vehicle.js';
import { CommandControls } from '../src/runtime/commands.js';
import { commandControlPorts } from '../src/runtime/command-controls-sim.js';
import { predictionVehicle, predictVehicle } from '../src/runtime/vehicle-prediction.js';
import { createVehiclePredictionBridge } from '../src/runtime/vehicle-prediction-sim.js';
import { syntheticHull, hullCommand, hullObservation } from './fixtures/prediction-hull-cases.js';

const instance = await loadSim(), bridge = bridgeFor(instance);
const bind = (f, native) => { if (native) attachNetworkSeatsSim(f.system, bridge); return f; };
function grow(native) { if (native) { bridge.x.sim_call_room(30000, 0); bridge.x.memory.grow(1); } }
function seat() { return { aimYaw: 1, pitch: .1, lookYaw: null, lookPitch: undefined, lease: null }; }
function cycleFixture(native, hook = () => {}) {
  const trace = [], human = { networkHuman: true }, bot = { networkHuman: false }, other = { networkHuman: false };
  const vehicle = { alive: true, seats: [seat(), seat(), seat()], stickX: 1, stickY: -1,
    clampAim(i) { trace.push(['clamp', i]); hook('clamp', f, i); }, restAim(i) { trace.push(['rest', i]); hook('rest', f, i); } };
  const leases = [human, bot, other].map((unit, index) => ({ name: ['human','bot','other'][index], unit, index, vehicle }));
  for (const l of leases) { l.unit.seat = l; vehicle.seats[l.index].lease = l; }
  const system = { move(l, i) {
    trace.push(['move', l.name, i]);
    if (vehicle.seats[l.index]?.lease === l) vehicle.seats[l.index].lease = null;
    vehicle.seats[i].lease = l; l.index = i; hook('move', f, l);
  } };
  const f = { system, vehicle, human, bot, other, leases, trace, native };
  return bind(f, native);
}
function cycleState(f) {
  return { trace: f.trace, seats: f.vehicle.seats.map(s => [s.lease?.name, s.aimYaw, s.pitch, s.lookYaw, s.lookPitch]),
    indices: f.leases.map(l => l.index), sticks: [f.vehicle.stickX, f.vehicle.stickY] };
}
function inputFixture(native, { flying = false, index = 0, look = [], clamp = () => {} } = {}) {
  const system = {}, trace = [], vehicle = { system, flying, seats: [seat(),seat()], stickX: 0, stickY: 0,
    clampAim(i) { trace.push(['clamp', i, this.seats[i].aimYaw, this.seats[i].lookYaw]); clamp(f); } };
  const controls = { axis(name) { trace.push(['axis',name]); return 0; }, down: { has(code) { trace.push(['down',code]); return false; } }, take(name) { trace.push(['take',name]); return false; } };
  const command = { look }, f = { system, vehicle, controls, command, index, trace, native };
  return bind(f,native);
}
function input(f) { return networkVehicleInput(f.vehicle,f.index,f.controls,f.command); }
function observed(f, out) { return { trace: f.trace, out, keys: Object.keys(out), seats: f.vehicle.seats, sticks: [f.vehicle.stickX,f.vehicle.stickY] }; }

test('seat cycles retain the original occupant across same-rider reentry and memory growth', () => {
  const run = native => {
    let nested = false;
    const f = cycleFixture(native, (phase, f) => {
      if (phase === 'move' && !nested) { nested = true; grow(native); assert.equal(cycleNetworkSeat(f.system,f.human),true); }
    });
    assert.equal(cycleNetworkSeat(f.system,f.human),true);
    assert.equal(f.human.seat,f.leases[0]); assert.equal(f.human.seat.index,2);
    assert.equal(f.vehicle.seats[0].lease,f.leases[1]);
    return cycleState(f);
  };
  assert.deepEqual(run(true),run(false));
});

test('seat guards, human protection, reverse and zero directions preserve field tags and method order', () => {
  for (const direction of [1,-1,2,-2,0,8]) {
    for (const mode of ['ordinary','human','driver','dead','released','single']) {
      const run = native => {
        const f = cycleFixture(native);
        f.vehicle.seats[0].lookPitch = false;
        if (mode === 'human') f.bot.networkHuman = true;
        if (mode === 'driver') { f.vehicle.driver = {}; f.vehicle.seatIndex = 1; }
        if (mode === 'dead') f.vehicle.alive = false;
        if (mode === 'released') f.human.seat.released = true;
        if (mode === 'single') f.vehicle.seats.length = 1;
        const result = cycleNetworkSeat(f.system,f.human,direction);
        return { result, ...cycleState(f) };
      };
      assert.deepEqual(run(true),run(false),`${mode} ${direction}`);
    }
  }
  assert.equal(networkSeatsPort(bridge),networkSeatsPort(bridge));
  assert.equal(networkSeatsPort(bridge).cycle({},null),false);
});

test('seat callback throws retain partial swaps and release invocation contexts', () => {
  for (const stop of [1,2,3]) for (const thrown of [undefined,null,false,'failure']) {
    const run = native => {
      let count = 0, caught = false;
      const f = cycleFixture(native, () => { if (++count === stop) { grow(native); throw thrown; } });
      try { cycleNetworkSeat(f.system,f.human); } catch (e) { caught = true; assert.equal(e,thrown); }
      assert.equal(caught,true);
      return cycleState(f);
    };
    assert.deepEqual(run(true),run(false));
  }
  assert.equal(cycleNetworkSeat(cycleFixture(true).system,null),false);
});

test('look callbacks can append samples and mutate fields before later controls', () => {
  const run = native => {
    let first = true;
    const f = inputFixture(native,{look:[[2048,0]],clamp(f) { if (first) { first=false; f.command.look.push([2048,0]); grow(native); } }});
    f.controls.axis = name => { f.trace.push(['axis',name]); if(name==='forward')f.vehicle.seats[0].aimYaw=42; return 0; };
    const out = input(f); assert.equal(f.vehicle.seats[0].lookYaw,-3); assert.equal(f.vehicle.seats[0].aimYaw,42);
    return observed(f,out);
  };
  assert.deepEqual(run(true),run(false));
});

test('look iteration retains next and closes on body/destructuring throws with original exception precedence', () => {
  for (const failure of ['none','body','next','destructure']) {
    const run = native => {
      let step=0, iterator, first=true;
      const original = { reason: failure }, cleanup = { reason:'cleanup' };
      const f = inputFixture(native,{clamp(f) {
        if(first) { first=false; Object.defineProperty(iterator,'next',{value(){throw Error('reloaded next');},configurable:true}); grow(native); }
        if(failure==='body')throw original;
      }});
      f.command.look = { [Symbol.iterator]() {
        f.trace.push(['iterator']); iterator = { get next() { f.trace.push(['getNext']); return function() {
          f.trace.push(['next',step]); if(failure==='next' && step===1)throw original;
          if(step++===2)return {done:true};
          if(failure==='destructure')return {value:{[Symbol.iterator](){throw original;}}};
          return {value:[100,50]};
        }; }, return() { f.trace.push(['return']); grow(native); networkSeatsPort(bridge).cycle({},null); throw cleanup; } };
        return iterator;
      } };
      let out, caught=false;
      try { out=input(f); } catch(e) { caught=true; assert.equal(e,original); }
      assert.equal(caught,failure!=='none');
      return {trace:f.trace,seats:f.vehicle.seats,out};
    };
    assert.deepEqual(run(true),run(false),failure);
  }
});

test('flight callbacks observe mutable binding entries, retained axes, pad buttons and late curve', () => {
  const run = native => {
    const f=inputFixture(native,{flying:true}), axes=[0,0,0,0];
    const bindings={throttle:['A','B'],brake:[],rollRight:[],rollLeft:[],boost:[],rollLock:[]};
    const fs={bindings,curve:undefined}, pad={axes,buttons:[]}; Object.assign(f.command,{flightSettings:fs,gamepad:pad});
    f.controls.down.has=code=>{f.trace.push(['down',code]);if(code==='A'){bindings.throttle[1]='C';bindings.throttle.push('ignored');pad.axes=[0,0,0,-.1];axes[3]=-.9;grow(native);}return false;};
    f.controls.take=name=>{f.trace.push(['take',name]);if(name==='trick')pad.buttons[0]={pressed:true};if(name==='targetLock'){fs.curve=null;f.command.flightSettings={curve:2};}return false;};
    const out=input(f);assert.equal(out.forward,.9);assert.equal(out.trick,true);assert.equal(out.responseCurve,null);assert.equal('stickX' in out,false);
    return observed(f,out);
  };
  assert.deepEqual(run(true),run(false));
});

test('flight and drive callback return tags and object identities survive the ABI', () => {
  for(const native of [false,true]) {
    const marker={identity:true}, f=inputFixture(native,{flying:true});
    f.command.flightSettings={curve:marker};
    f.controls.take=name=>({takeoff:undefined,trick:marker,immelmann:null,targetLock:false})[name];
    const out=input(f);
    assert.equal(out.takeoff,undefined);assert.equal(out.trick,marker);assert.equal(out.immelmann,null);assert.equal(out.targetLock,false);
    assert.equal(out.responseCurve,marker);assert.ok(Object.hasOwn(out,'takeoff'));assert.equal(out.steeringAssist,false);
    f.vehicle.flying=false;f.controls.axis=name=>({forward:null,right:false,turn:undefined})[name];f.controls.down.has=()=>marker;
    assert.deepEqual(input(f),{forward:null,right:false,turn:undefined,brake:marker});
  }
});

test('flight result and duplicate action queues match source across bounds, bindings and pad dead zones', () => {
  let seed=77;
  const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296;};
  const special=[undefined,null,NaN,-0,0,.12,-.12,.1200001,-1,1,Infinity];
  const options=Array.from({length:350},(_,i)=>({ i,look:[[random()*400-200,random()*400-200],[random()*200-100,random()*200-100]],
    mouseScale:special[i%special.length],sensitivity:special[(i+3)%special.length],curve:special[(i+7)%special.length],
    axes:Array.from({length:4},(_,k)=>i%3?random()*2-1:special[(i+k)%special.length]),buttons:[{pressed:i%3===0}],
    down:['KeyW','KeyS','KeyD','KeyA','ShiftLeft','KeyZ'].filter(()=>random()<.3),
    actions:['takeoff','trick','immelmann','targetLock','takeoff','trick','use'].filter(()=>random()<.5) }));
  for(const scenario of options) {
    const run=native=>{
      const f=inputFixture(native,{flying:true,look:scenario.look}), c=f.controls=new CommandControls();
      if(native)commandControlPorts(bridge,new WeakSet()).controls(c);
      c.accept({epoch:1,generation:1,sequence:1,down:scenario.down,actions:scenario.actions});
      const trace=[],take=c.take.bind(c);c.take=name=>{const result=take(name);trace.push([name,result]);return result;};
      Object.assign(f.command,{mouseScale:scenario.mouseScale,invertMouse:scenario.i%2,flightSettings:{sensitivity:scenario.sensitivity,invert:scenario.i%3,curve:scenario.curve,
        bindings:scenario.i%2?{throttle:['KeyW',,'KeyZ'],brake:[],boost:['KeyZ','ShiftLeft']}:undefined},gamepad:{axes:scenario.axes,buttons:scenario.buttons}});
      const out=input(f), rest=[];for(const name of ['takeoff','trick','immelmann','targetLock','use'])while(c.take(name))rest.push(name);
      return {out,keys:Object.keys(out),trace,rest,sticks:[f.vehicle.stickX,f.vehicle.stickY]};
    };
    assert.deepEqual(run(true),run(false),`case ${scenario.i}`);
  }
});

test('clamp reentry can change seat mode and nested controls share this same module', () => {
  const run=native=>{
    let first=true;const f=inputFixture(native,{look:[[12,5],[80,-20]],clamp(f){if(first){first=false;f.vehicle.flying=true;
      const other=inputFixture(native,{index:1,look:[]});input(other);grow(native);}}});
    return observed(f,input(f));
  };
  assert.deepEqual(run(true),run(false));
});

test('input mutates the actual private clone without publishing authority seats, leases or weapons', () => {
  for(const kind of ['hover','flyer']) {
    const f=syntheticHull(kind);
    try {
      attachNetworkSeatsSim(f.system,bridge);
      const source=f.v, before=hullObservation(source), seats=source.seats.map(s=>[s.aimYaw,s.pitch,s.lookYaw,s.lookPitch,s.lease]), body=source.body.matrix.toArray();
      const clone=predictionVehicle(source);clone.nativeSession={bridge};
      const controls=new CommandControls();controls.accept({epoch:1,generation:3,sequence:1,down:['KeyW'],actions:['takeoff']});
      const out=networkVehicleInput(clone,0,controls,{look:[[100,40]],flightSettings:{curve:null}});
      assert.equal(out.forward,1);assert.notEqual(clone.seats,source.seats);assert.equal(clone.body,source.body);
      assert.deepEqual(hullObservation(source),before);assert.deepEqual(source.body.matrix.toArray(),body);
      assert.deepEqual(source.seats.map(s=>[s.aimYaw,s.pitch,s.lookYaw,s.lookPitch,s.lease]),seats);
      assert.equal(clone.seats[0].weapons,source.seats[0].weapons);
    } finally { f.system.dispose(); }
  }
});

test('private native prediction stays exact with the live input port attached', () => {
  const native=createVehiclePredictionBridge(bridge);
  try {
    for(const kind of ['hover','springs','walker','flyer']) {
      const a=syntheticHull(kind),b=syntheticHull(kind);
      try {
        attachNetworkSeatsSim(a.system,bridge);
        const commands=Array.from({length:16},(_,i)=>hullCommand(i+1));
        const pa=predictVehicle(a.v,commands,0,3,[],{native}),pb=predictVehicle(b.v,commands,0,3);
        assert.deepEqual(hullObservation(pa),hullObservation(pb),kind);
        assert.deepEqual(hullObservation(a.v),hullObservation(b.v),`${kind} authority`);
      } finally {a.system.dispose();b.system.dispose();}
    }
  } finally {native.dispose();}
});
