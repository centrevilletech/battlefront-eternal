// Synthetic camera fixtures drive the same coarse phase and generated
// records as the page. Camera policy is never reimplemented by a test host.
import * as T from 'three';
import {shownGunner} from './network-preview.js';
import {PageCameras} from '../../src/sim/wasm/presentation/cameras.js';
import {CameraOutput as O} from '../../src/presentation/layout.js';
import {presentTurret} from './page-presentation.js';
import {CollisionWorld} from '../../tools/collision-fixture.mjs';
const owners=new WeakMap(),EMPTY_DEF={};
export function openCameraWorld(){
  const floor=new T.BoxGeometry(1000,1,1000).translate(0,-100.5,0);
  return new CollisionWorld(floor.attributes.position.array,floor.index.array,new Uint32Array(floor.index.count/3));
}
export class CameraFrame {
  constructor(collision=openCameraWorld(),camera=new T.PerspectiveCamera(60,16/9,.1,1000),def=EMPTY_DEF,authority=false,shots=[]){
    let owner=owners.get(collision);
    if(!owner){owner=new PageCameras(collision.fixtureInstance||collision.rays.instance,collision,camera,def,def.cameraMotions,authority,shots);owners.set(collision,owner);}
    else{owner.x.camera_reset(owner.handle);owner.camera=camera;owner.authority=authority;owner.view.setDef(def);}
    owner.automaticEye=false;this.owner=owner;this.camera=camera;this.collision=collision;
    this.state={posture:'stand',air:null,dive:0,velocity:new T.Vector3(),position:new T.Vector3(),aimYaw:0,aimPitch:0};
    this.feet=this.state.position;this.combat={zoom:1};this.remotes={active:false,piloted:null};this.driver={active:false};this.gunner={active:false,turret:null};
    this.death={active:false,deaths:1,hfov:1.1};
    this.killer={id:7,generation:1,alive:true,removed:false,team:2,soldier:{posture:'stand'},actor:{root:{position:new T.Vector3()}}};
    this.shot={trace:{show(){},hide(){}},refs:{units:{byId:()=>this.killer},relation:()=>-1}};
    this.chest=new T.Object3D();this.eye=new T.Vector3();this.first=false;this.ownEyes=false;this.style='enhanced';this.spawn=null;
  }
  frame(dt,frozen=false){
    if(this.death.active)this.chest.updateWorldMatrix(true,false);
    const cue=this.owner.frame(dt,frozen,this.feet,this.state,this.state,this.combat,this.ownEyes,this.first,this.ownEyes?this.eye:null,this.death,this.shot,this.remotes,this.driver,this.gunner,this.death.active?this.chest:null,this.style,this.spawn);
    return cue;
  }
  finish(){this.owner.finish();}
}
export class VehicleCamera {
  constructor(camera,collision,simulation=false){
    this.host=new CameraFrame(collision,camera,EMPTY_DEF,simulation);this.camera=camera;this.cameraSet=false;this.speedEffect=0;this.enclosed=false;this.lookAt=new T.Vector3();
    this.host.driver={active:true,vehicle:null,cameraSet:false,aimView:{cameraSet:false},hideHull(){}};
  }
  update(vehicle,dt,first){
    const h=this.host,d=h.driver;d.vehicle=vehicle;h.first=first;d.cameraSet=this.cameraSet;d.aimView.cameraSet=this.cameraSet;
    h.frame(dt);h.finish();const o=h.owner.output;
    this.cameraSet=!!o[O.VEHICLE_SET];this.speedEffect=o[O.SPEED_EFFECT];this.enclosed=!!o[O.ENCLOSED];this.lookAt.fromArray(o,O.LOOK_AT);
  }
}
export class TurretCamera {
  constructor(camera,collision,turret){
    this.host=new CameraFrame(collision,camera);this.camera=camera;
    this.host.gunner={active:true,turret,previewYaw:0,shownPitch:0,shownAim(){},hideRig(){},eyePoint:(out)=>turret.eyePoint(out,this.yaw,this.pitch)};
  }
  update(dt,turret,yaw,pitch,first=false){
    const h=this.host;this.yaw=yaw;this.pitch=pitch;h.gunner.turret=turret;h.gunner.previewYaw=yaw-turret.yaw;h.gunner.shownPitch=pitch;h.first=first;h.frame(dt);h.finish();
  }
}

// Test callers publish one complete camera phase, just as game.js does.
export function presentGunner(gunner,dt,zoom,first){
  const h=gunner.cameraPhase ||= new CameraFrame(gunner.collision,gunner.camera);
  shownGunner(gunner);
  const turret=gunner.turret,own=turret.humanGunner;
  turret.humanGunner=gunner;presentTurret(turret);turret.humanGunner=own;
  h.gunner=gunner;h.first=first;h.frame(dt);h.finish();
}
export function presentDriver(driver,dt,first){
  const h=driver.cameraPhase ||= new CameraFrame(driver.collision,driver.camera);
  h.driver=driver;h.first=first;h.frame(dt);h.finish();
}
export class SoldierCamera {
  constructor(camera,collision,def,{responsive=false}={}){
    this.host=new CameraFrame(collision.fixtureInstance?collision:openCameraWorld(),camera,def);this.host.owner.view.responsive=responsive;
  }
  sampleEye(feet,pose,zoom=1){
    const h=this.host;h.owner.automaticEye=true;h.feet=feet;h.state.position.copy(pose.position);h.state.aimYaw=pose.aimYaw;h.state.aimPitch=pose.aimPitch;h.state.posture=pose.posture;h.state.air=pose.air;h.combat.zoom=zoom;h.ownEyes=true;h.frame(0);h.finish();return h.camera.position.clone();
  }
  update(dt,feet,yaw,pitch,posture='stand',zoom=1,first=false,eye=null){
    const h=this.host;h.state.position.copy(feet);h.state.aimYaw=yaw;h.state.aimPitch=pitch;h.state.posture=posture;h.combat.zoom=zoom;h.ownEyes=first;h.first=first;if(eye)h.eye.copy(eye);h.frame(dt);h.finish();
  }
}

export function presentRemote(remote,dt,first){
  const h=remote.cameraHost;h.owner.camera=h.camera=remote.camera;h.owner.bindCamera(remote.camera);
  h.remotes=remote;h.first=first;h.frame(dt);h.finish();
}
