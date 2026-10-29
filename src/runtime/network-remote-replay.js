import {PerspectiveCamera} from 'three';
// Mutable droid/controller fields for a replay checkpoint. The caller must also
// retain its independent RNG and camera state; this is not the complete wire
// envelope. Immutable definitions remain references until the adapter assigns IDs.
import {WEAPON_STATE_NUMBERS as REMOTE_WEAPON_NUMBERS,WEAPON_STATE_FLAGS as REMOTE_WEAPON_FLAGS} from './network-weapon-state.js';
export {REMOTE_WEAPON_NUMBERS,REMOTE_WEAPON_FLAGS};
const numbers=REMOTE_WEAPON_NUMBERS,flags=REMOTE_WEAPON_FLAGS;
const vectors=['position','velocity','eye','aimDir','target'];
export function captureRemoteReplay(context,droid){
 const state={randomSeed:context.random?.seed,yaw:droid.yaw,pitch:droid.pitch,health:droid.health,dead:droid.dead,countdown:droid.countdown,piloted:context.piloted===droid,lookX:context.lookX,lookY:context.lookY,zoomStep:context.zoomStep,firstPerson:context.firstPerson,selected:[...droid.arsenal.selected],weapons:droid.arsenal.weapons.map(w=>({level:w.levels.indexOf(w.def),...Object.fromEntries([...numbers,...flags].map(key=>[key,w[key]]))}))};
 if(context.simulationView&&context.aimCamera)state.camera={position:context.aimCamera.position.toArray(),quaternion:context.aimCamera.quaternion.toArray(),aspect:context.aimCamera.aspect,fov:context.simulationView.fov,reach:context.simulationView.reach,fresh:context.simulationView.fresh};
 for(const key of vectors)state[key]=droid[key].toArray();
 state.salvo=droid.salvo?{...droid.salvo,...Object.fromEntries(['at','x','y','z'].map(key=>[key,droid.salvo[key].clone()]))}:null;
 return state;
}
export function restoreRemoteReplay(context,droid,state){
 if(state.weapons.length!==droid.arsenal.weapons.length||state.weapons.some((w,i)=>!droid.arsenal.weapons[i].levels[w.level]))throw Error('recon replay content mismatch');
 for(const key of vectors)if(state[key])droid[key].fromArray(state[key]);
 for(const key of ['yaw','pitch','health','dead','countdown'])droid[key]=state[key];
 droid.arsenal.selected=[...state.selected];for(let i=0;i<state.weapons.length;i++){const target=droid.arsenal.weapons[i],saved=state.weapons[i];target.def=target.levels[saved.level];for(const key of [...numbers,...flags])target[key]=saved[key];}
 droid.salvo=state.salvo?{...state.salvo,...Object.fromEntries(['at','x','y','z'].map(key=>[key,state.salvo[key].clone()]))}:null;
 context.strikes=context.strikes.filter(item=>item!==droid);if(droid.salvo)context.strikes.push(droid);
 if(state.camera&&!context.aimCamera)context.aimCamera=new PerspectiveCamera();
 if(state.piloted&&!droid.dead){if(context.piloted!==droid||state.camera&&!context.simulationView)context.enter(droid);}else if(context.piloted===droid)context.exit();
 if(state.camera&&context.simulationView){context.aimCamera.position.fromArray(state.camera.position);context.aimCamera.quaternion.fromArray(state.camera.quaternion);context.aimCamera.aspect=state.camera.aspect;for(const key of ['fov','reach','fresh'])context.simulationView[key]=state.camera[key];context.simulationView.applyFov(state.camera.aspect);context.aimCamera.updateMatrixWorld();}
 for(const key of ['lookX','lookY','zoomStep','firstPerson'])context[key]=state[key];
 if(state.randomSeed!==undefined){if(!context.random||!('seed' in context.random))throw Error('recon replay requires independent seeded stream');context.random.seed=state.randomSeed;}
 if(!state.eye)context.aim(droid,state.piloted);
 return droid;
}
