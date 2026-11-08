import {sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from 'three';
import { JetpackVolume } from './jetpack-volume.js';
import { JetpackWake } from './jetpack-wake.js';

import {EffectPhase} from './effects-gpu.js';
import {JetFrame as J,JetNozzle as N} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['local',Float64Array],['nozzles',Float64Array],['wake_input',Float64Array],['wake_particles',Float32Array],['volume_input',Float64Array],['volume_arrays',Float32Array]];
const EXHAUST_LIGHT = [1, 0.52, 0.18];

// Geometry anchors feed the attached volume; history bends its tail, while
// detached combustion wisps/smoke travel along their own world-space paths.
export class JetpackEffects {
  constructor({scene,settings,lights=null,definition=[],random=Math.random}) {
    this.settings=settings;this.lights=lights;this.time=0;this.throttle=0;this.ignition=10;
    this.lightPosition=new THREE.Vector3();
    this.phase=new EffectPhase('PageJetpack',BANKS,JSON.stringify(definition));
    this.nozzles=Array.from({length:this.phase.input[J.NOZZLES]},()=>({position:new THREE.Vector3(),direction:new THREE.Vector3(),radius:.034,bendWorld:new THREE.Vector3()}));
    this.volume=new JetpackVolume({scene,count:this.nozzles.length});this.flame=this.volume.mesh;
    this.wake=new JetpackWake({scene,random});
    this.smoke=this.wake.smoke;this.sparks=this.wake.embers;
    this.random=random;
  }
  get emitted(){return this.wake.emitted;}
  get enhanced(){return (this.settings?.get('jetpackEffects')??'enhanced')==='enhanced';}
  clear(){this.phase.refresh();this.phase.input[J.CLEAR]=1;this.phase.frame(this.random);this.publish();}
  publish(){
    const p=this.phase,x=p.input;this.time=x[J.TIME];this.throttle=x[J.THROTTLE];this.ignition=x[J.IGNITION];
    for(let i=0;i<this.nozzles.length;i++){const n=this.nozzles[i],at=i*N.STRIDE;sceneTransform(n.position.fromArray(p.nozzles,at+N.POSITION));n.direction.fromArray(p.nozzles,at+N.DIRECTION);n.bendWorld.fromArray(p.nozzles,at+N.BEND);n.radius=p.nozzles[at+N.RADIUS];}
    this.volume.publish(p.volume_input,p.volume_arrays);this.wake.publish(p.wake_input,p.wake_particles);
  }
  update(dt,facts) {
    const p=this.phase;p.refresh();const x=p.input;
    x[J.DT]=dt;x[J.ACTIVE]=facts.active?1:0;x[J.ENHANCED]=this.enhanced?1:0;x[J.NOZZLES]=this.nozzles.length;
    facts.position.toArray(x,J.POSITION);facts.quaternion.toArray(x,J.QUATERNION);facts.velocity.toArray(x,J.VELOCITY);
    for(let i=0;i<this.nozzles.length;i++){
      const frame=facts.nozzleFrames?.[i],at=i*N.STRIDE;p.nozzles[at+N.HAS_PREVIOUS]=frame?1:0;
      if(frame){frame.position.toArray(p.nozzles,at+N.POSITION);frame.direction.toArray(p.nozzles,at+N.DIRECTION);}
      p.nozzles[at+N.RADIUS]=frame?.radius??0.034;
    }
    p.frame(this.random);this.publish();
    if(x[J.LIGHT_STRENGTH]>0){this.lightPosition.fromArray(x,J.LIGHT_POSITION);this.lights?.add(this.lightPosition,EXHAUST_LIGHT,1.5,x[J.LIGHT_STRENGTH]);}
  }
  dispose(){this.phase.dispose();this.volume.dispose();this.wake.dispose();}
}
