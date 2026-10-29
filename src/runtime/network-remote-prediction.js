import {CommandControls} from './commands.js';
import {replayTicks} from './reconcile.js';
import {reconRandom} from './network-remote-world.js';
import {captureRemoteReplay,restoreRemoteReplay} from './network-remote-replay.js';

// The most ticks a replay runs past the authoritative checkpoint.
export const REMOTE_REPLAY_TICKS=120;
// Replay the shared controller in an isolated context. Gameplay callbacks and
// mesh/light creation stay authoritative; weapon, countdown and RNG state still
// advance through the exact shared code, including death during a salvo.
// Commands stamped beyond the window (a downstream blackout keeps the input
// clock running while the checkpoint stands still) wait for a newer
// checkpoint: the droid is shown at the window's end, and every frame still
// draws, rather than each one failing until the packets come back.
export function predictRemote(source,droid,pending,from,generation,held=[],view={},cache=null) {
 pending=pending.filter(command=>command.tick<=from+REMOTE_REPLAY_TICKS);
 const {context,predicted,initialCamera}=cache?cache.acquire(source,droid):createPredictionContext(source,droid);
 context.droids.length=0;context.droids.push(predicted);context.strikes.length=0;
 const state=captureRemoteReplay(source,droid);
 // Legacy/headless callers can lack a computational camera checkpoint. Match
 // a fresh context in that case; never reuse camera smoothing from another replay.
 if(!state.camera){context.exit();context.aimCamera.copy(initialCamera);}
 restoreRemoteReplay(context,predicted,state);
 const controls=new CommandControls();
 const tick=replayTicks(pending,from,generation,held,(_tick,command)=>{
  controls.accept({...command,epoch:1,sequence:command.tick});
  if(typeof command.firstPerson==='boolean')context.firstPerson=command.firstPerson;
  if(Number.isFinite(command.aspect)&&command.aspect>0)context.aimCamera.aspect=command.aspect;
  const scale=Math.min(command.mouseScale??view.mouseScale??1,2);
  for(const [dx,dy] of command.look)if(context.active)context.look(Math.max(-10000,Math.min(10000,dx)),Math.max(-10000,Math.min(10000,dy)),scale,command.invertMouse||false);
  if(context.active)context.updateCamera(1/60,Boolean(context.firstPerson),true);
  context.update(1/60,{forward:controls.axis('forward'),right:controls.axis('right'),turn:controls.axis('turn')},controls);
 },view);
 return {context,droid:predicted,tick,state:captureRemoteReplay(context,predicted)};
}

function createPredictionContext(source,droid) {
 const context=source.createContext(droid.owner,{random:reconRandom(0,'prediction')});
 context.model=()=>null;context.fire=()=>{};context.explode=()=>{};context.lights={flash(){}};
 const predicted=context.createDroid(droid.def,null,droid.position,droid.velocity,droid.yaw,droid.team);
 predicted.owner=droid.owner;
 return {context,predicted,initialCamera:context.aimCamera.clone()};
}

// One isolated controller/Arsenal per current droid. This retains no command
// history: each replay restores the exact authoritative checkpoint first.
export class RemotePredictionContext {
 constructor(){this.reset();}
 reset(){this.source=null;this.original=null;this.owner=null;this.definition=null;this.generation=null;this.value=null;}
 acquire(source,droid){
  if(this.source!==source||this.original!==droid||this.owner!==droid.owner||this.definition!==droid.def||this.generation!==droid.networkGeneration){
   this.reset();this.source=source;this.original=droid;this.owner=droid.owner;this.definition=droid.def;this.generation=droid.networkGeneration;this.value=createPredictionContext(source,droid);
  }
  this.value.predicted.team=droid.team;
  return this.value;
 }
}
