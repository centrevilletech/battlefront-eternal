import {sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from "three";

import {EffectPhase} from './effects-gpu.js';
import {SoldierFrame as F} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array]];
const NETWORK_POSE = ['posture', 'air', 'dive', 'dead'];
const WALKER_POSE = ["roll", "lean", "legs", "time"];

// Presentation inherits live source fields without making the live soldier
// itself a prototype. V8 tags prototype numbers; refreshing a seated body's
// yaw and pitch would then allocate a box for each native output value.
const SOURCE=Symbol('presentation source'),STATE_PROTOTYPES=new WeakMap();
function displayState(soldier,position,velocity) {
  const base=Object.getPrototypeOf(soldier)||Object.prototype;
  let prototype=STATE_PROTOTYPES.get(base);
  if(!prototype) {prototype=Object.create(base);STATE_PROTOTYPES.set(base,prototype);}
  // Field descriptors are cold resources shared by every body of this type.
  // Writes shadow a forwarded field exactly as ordinary inheritance did;
  // reset can delete the shadow and expose the current source value again.
  const keys=Object.keys(soldier).concat(NETWORK_POSE,WALKER_POSE,'mode','reverse');
  for(const key of keys)if(!Object.hasOwn(prototype,key))Object.defineProperty(prototype,key,{
    get(){return this[SOURCE][key];},
    set(value){Object.defineProperty(this,key,{value,writable:true,enumerable:true,configurable:true});},
  });
  const state={position,velocity,aimYaw:NaN,aimPitch:NaN};
  Object.setPrototypeOf(state,prototype);
  Object.defineProperty(state,SOURCE,{value:soldier});
  return state;
}

// Dedicated servers never interpolate a rendered frame. Keep the interface
// used by spawn/seat transitions, with live authoritative coordinates, without
// copying every soldier's pose before and after each physics step.
export class HeadlessSoldierPresentation {
  constructor(soldier) {
    this.soldier = this.state = soldier;
    this.position = soldier.position;
    this.velocity = soldier.velocity;
    this.previousYaw = soldier.aimYaw;
    this.tickYaw = 0;
  }
  reset() { this.previousYaw = this.soldier.aimYaw; }
  beforeStep() { this.previousYaw = this.soldier.aimYaw; }
  afterStep() {}
  update() { return this.state; }
}

// Rendering samples the last simulation interval without changing collision,
// weapon aim, or input. Mouse look remains immediate; only turns produced by
// the fixed physics tick are interpolated. Small stair corrections get a
// separate visual offset shared by the body, held weapon, and camera.
export class SoldierPresentation {
  constructor(soldier) {
    this.soldier = soldier;
    this.position = new THREE.Vector3();
    this.velocity = new THREE.Vector3();
    this.previousPosition = new THREE.Vector3();
    this.previousVelocity = new THREE.Vector3();
    this.previousPose = {};
    // Native publication refreshes fractional yaw directly. Reserve a
    // double field before reset installs an integer spawn angle.
    this.previousYaw = NaN;
    this.state = displayState(soldier,this.position,this.velocity);
    this.phase=new EffectPhase('PageSoldierView',BANKS);
    this.reset();
  }

  clearNetworkState() {for(const key of NETWORK_POSE)delete this.state[key];}
  networkReset() {this.networkFire=null;this.networkFired=false;this.deathPending=false;this.clearNetworkState();}
  // The public fields are transport mirrors consumed by the existing body
  // phase and prediction correction. No interpolation runs in these mirrors.
  facts(command) {
    this.phase.refresh();const x=this.phase.input,s=this.soldier;
    x[F.COMMAND]=command;s.position.toArray(x,F.POSITION);s.velocity.toArray(x,F.VELOCITY);
    x[F.YAW]=s.aimYaw;x[F.PITCH]=s.aimPitch;x[F.GROUND_STEP]=s.groundAdjustment||0;
    x[F.HAS_MODE]=s.mode===undefined?0:1;x[F.MODE]=s.mode??NaN;x[F.REVERSE]=s.reverse?1:0;
    for(let k=0;k<4;k++)x[F.POSE+k]=s[WALKER_POSE[k]]??NaN;
    this.previousPosition.toArray(x,F.PREVIOUS_POSITION);this.previousVelocity.toArray(x,F.PREVIOUS_VELOCITY);
    x[F.PREVIOUS_YAW]=this.previousYaw;x[F.TICK_YAW]=this.tickYaw??0;x[F.PREVIOUS_STEP]=this.previousStep??0;
    x[F.LAST_STEP]=this.lastStep??0;x[F.STEP_OFFSET]=this.stepOffset??0;x[F.PREVIOUS_MODE]=this.previousMode??NaN;
    for(let k=0;k<4;k++)x[F.PREVIOUS_POSE+k]=this.previousPose[WALKER_POSE[k]]??NaN;
  }
  publish(interval=false,shown=false) {
    const x=this.phase.input;
    this.previousPosition.fromArray(x,F.PREVIOUS_POSITION);this.previousVelocity.fromArray(x,F.PREVIOUS_VELOCITY);
    this.previousYaw=x[F.PREVIOUS_YAW];this.tickYaw=x[F.TICK_YAW];this.previousStep=x[F.PREVIOUS_STEP];this.lastStep=x[F.LAST_STEP];this.stepOffset=x[F.STEP_OFFSET];
    if(interval){this.previousMode=this.soldier.mode;if(this.soldier.mode!==undefined)for(let k=0;k<4;k++)this.previousPose[WALKER_POSE[k]]=x[F.PREVIOUS_POSE+k];}
    if(shown){sceneTransform(this.position.fromArray(x,F.SHOWN_POSITION));this.velocity.fromArray(x,F.SHOWN_VELOCITY);this.state.aimYaw=x[F.SHOWN_YAW];this.state.aimPitch=x[F.SHOWN_PITCH];
      if(this.soldier.mode!==undefined)for(let k=0;k<4;k++)this.state[WALKER_POSE[k]]=x[F.SHOWN_POSE+k];}
  }
  reset(){this.networkReset();this.facts(1);this.phase.frame();this.publish(true,true);}
  beforeStep(){this.facts(2);this.phase.frame();this.publish(true);}
  afterStep(){this.facts(3);this.phase.frame();const reset=Boolean(this.phase.input[F.TELEPORTED]);if(reset)this.networkReset();this.publish(reset,reset);}
  update(dt,alpha=1){this.networkFired=false;this.facts(0);this.phase.input[F.DT]=dt;this.phase.input[F.ALPHA]=alpha;this.phase.frame();this.publish(false,true);return this.state;}
}
