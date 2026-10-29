import * as THREE from 'three';
import { JetpackVolume } from './jetpack-volume.js';
import { JetpackWake } from './jetpack-wake.js';

const EXHAUST_LIGHT = [1, 0.52, 0.18];

// Geometry anchors feed the attached volume; history bends its tail, while
// detached combustion wisps/smoke travel along their own world-space paths.
export class JetpackEffects {
  constructor({scene,settings,lights=null,definition=[],random=Math.random}) {
    this.settings=settings;this.lights=lights;this.time=0;this.throttle=0;this.ignition=10;this.wasActive=false;
    this.lightPosition=new THREE.Vector3();this.targetTail=new THREE.Vector3();
    const jets=definition.filter(e=>/^JetExhaust(?:_Left|_Right)?$/i.test(e.name));
    this.nozzles=(jets.length?jets.slice(0,4):[null,null]).map((jet,i)=>({
      local:new THREE.Vector3(...(jet?.spawner?.offset?.map(v=>(v[0]+v[1])*.5)||[-.24,-.21,i?-.15:.15])),
      localDirection:new THREE.Vector3(...(jet?.spawner?.box?.map(v=>(v[0]+v[1])*.5)||[-1,0,0])).normalize(),
      position:new THREE.Vector3(),direction:new THREE.Vector3(),radius:0.034,
      tail:new THREE.Vector3(),bendWorld:new THREE.Vector3(),ready:false,
    }));
    this.volume=new JetpackVolume({scene,count:this.nozzles.length});this.flame=this.volume.mesh;
    this.wake=new JetpackWake({scene,random});
    this.smoke=this.wake.smoke;this.sparks=this.wake.embers;
    this.lastPosition=new THREE.Vector3();this.hasPosition=false;
  }
  get emitted(){return this.wake.emitted;}
  get enhanced(){return (this.settings?.get('jetpackEffects')??'enhanced')==='enhanced';}
  clear(){
    this.volume.clear();this.wake.clear();this.throttle=0;this.wasActive=false;
    for(const nozzle of this.nozzles)nozzle.ready=false;
  }
  update(dt,{active,position,quaternion,velocity,nozzleFrames=null}) {
    dt=Math.max(0,Math.min(dt,.1));this.time+=dt;
    if(!this.enhanced){this.clear();this.hasPosition=false;return;}
    if(this.hasPosition&&this.lastPosition.distanceToSquared(position)>100)this.clear();
    this.lastPosition.copy(position);this.hasPosition=true;
    if(active&&!this.wasActive)this.ignition=0;
    this.wasActive=Boolean(active);this.ignition+=dt;
    this.throttle+=(Number(Boolean(active))-this.throttle)*(1-Math.exp(-dt/(active ? .045 : .07)));
    if(!active&&this.throttle<.015)this.throttle=0;
    const ignitionPulse=1+.14*Math.sin(Math.min(this.ignition/.16,1)*Math.PI)*Math.exp(-this.ignition*5);
    const power=this.throttle*ignitionPulse*(1+Math.min(velocity.length()/40,.12));
    this.lightPosition.set(0,0,0);
    for(let i=0;i<this.nozzles.length;i++) {
      const jet=this.nozzles[i],frame=nozzleFrames?.[i];
      if(frame){jet.position.copy(frame.position);jet.direction.copy(frame.direction).normalize();jet.radius=frame.radius??.034;}
      else{jet.position.copy(jet.local).applyQuaternion(quaternion).add(position);jet.direction.copy(jet.localDirection).applyQuaternion(quaternion).normalize();}
      this.targetTail.copy(jet.position).addScaledVector(jet.direction,.58);
      if(!jet.ready){jet.tail.copy(this.targetTail);jet.ready=true;}
      jet.tail.lerp(this.targetTail,1-Math.exp(-dt/.055));
      jet.bendWorld.subVectors(jet.tail,this.targetTail);
      jet.bendWorld.addScaledVector(jet.direction,-jet.bendWorld.dot(jet.direction)).clampLength(0,.14);
      this.lightPosition.add(jet.position).addScaledVector(jet.direction,.12);
    }
    this.volume.update(this.time,power,this.nozzles);
    this.wake.update(dt,{nozzles:this.nozzles,velocity,throttle:this.throttle,active:Boolean(active)});
    if(active)this.lights?.add(this.lightPosition.multiplyScalar(1/this.nozzles.length),EXHAUST_LIGHT,1.5,.30*this.throttle);
  }
  dispose(){this.volume.dispose();this.wake.dispose();}
}
