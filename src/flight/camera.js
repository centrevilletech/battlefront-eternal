import * as THREE from 'three';
import { setting } from '../seats.js';
const X=new THREE.Vector3(1,0,0), Y=new THREE.Vector3(0,1,0), Z=new THREE.Vector3(0,0,1), ONE=new THREE.Vector3(1,1,1);

// How fast a section's chase camera eases (1/s), as the 2004 game's camera
// section parser reads it: its turn (AimTension) and each of its
// axes in its own frame, right, left, up, down, back, forward (MoveTension
// sets all six; MoveTensionX, Y and Z one axis, one value for both ways or a
// pair). `rate` where the section sets none. Both are kept hashed by the
// import; most flyer pilot sections set them, the snowspeeder's rising at
// 2.5/s and sinking at 8.5.
export function cameraTension(s, rate) {
  const number = (name, fallback) => { const n = parseFloat(setting(s, name)); return Number.isFinite(n) ? n : fallback; };
  const all = number('MoveTension', rate), moveTension = [all, all, all, all, all, all];
  ['MoveTensionX', 'MoveTensionY', 'MoveTensionZ'].forEach((name, axis) => {
    const value = setting(s, name), pair = value === undefined ? null : String(value).trim().split(/\s+/).map(Number);
    if (pair && Number.isFinite(pair[0])) { moveTension[axis * 2] = pair[0]; moveTension[axis * 2 + 1] = Number.isFinite(pair[1]) ? pair[1] : pair[0]; }
  });
  return { aimTension: number('AimTension', rate), moveTension };
}
const flip=new THREE.Quaternion().setFromAxisAngle(Y,Math.PI);
// The camera section's walls and snap (as a seat's, src/vehicles.js): rays
// from 1.5 m up in the frame; more than 80 m from where it should be, it
// jumps; pulled in by a wall at once, it eases back out at 6/s.
const RAY_ORIGIN=new THREE.Vector3(0,1.5,0), SNAP=80, RETURN_RATE=6;
const frame=new THREE.Matrix4(), basis=new THREE.Matrix4(), frameQ=new THREE.Quaternion(), drawnQ=new THREE.Quaternion(), want=new THREE.Quaternion(), turn=new THREE.Quaternion(), inverse=new THREE.Quaternion();
const back=new THREE.Vector3(), up=new THREE.Vector3(), side=new THREE.Vector3(), above=new THREE.Vector3(), local=new THREE.Vector3();

// Each axis of a camera eases toward where it should be on its own, in the
// camera's frame, at its section's rate for the way it has to go
// (MoveTension: right, left, up, down, back, forward), as the 2004 game's
// camera section moves it.
export function easeAxes(cam, target, rates, dt) {
  inverse.copy(cam.quaternion).invert();
  local.subVectors(target, cam.position).applyQuaternion(inverse);
  local.x *= 1-Math.exp(-(local.x>0?rates[0]:rates[1])*dt);
  local.y *= 1-Math.exp(-(local.y>0?rates[2]:rates[3])*dt);
  local.z *= 1-Math.exp(-(local.z>0?rates[4]:rates[5])*dt);
  cam.position.add(local.applyQuaternion(cam.quaternion));
}

// A camera turned to look along `ahead` with `upward` as its up.
function lookAlong(ahead, upward, out) {
  back.copy(ahead).negate().normalize();
  side.crossVectors(upward, back);
  if (side.lengthSq() < 1e-10) side.set(1,0,0);
  side.normalize(); above.crossVectors(back, side);
  return out.setFromRotationMatrix(basis.makeBasis(side, above, back));
}

// The pilot's views of his craft, as the 2004 game shows them (docs/
// vehicles.md "Camera"). Behind it, the chase camera of his section: it looks
// along the craft's nose with the craft's up, pitched down by TiltValue,
// turning toward that at AimTension; it rests at TrackCenter in the craft's
// frame and TrackOffset (right, up, back) in its own, walls pulling it in,
// and each of its axes eases there at its MoveTension. The frame is the one
// the craft flies by: the camera does not lean with the drawn hull. In the
// cockpit, the eye (EyePointOffset) on the drawn hull looks along the nose.
// The field of view across is ThirdPersonFOV behind and FirstPersonFOV in
// the cockpit (48 degrees unless the section sets one); it does not change
// with speed, and nothing blurs. The port blends a switch between the two
// views over cameraBlendTime.
export function updateFlightCamera(view,v,dt,firstPerson) {
  const cam=view.camera,c=v.flightConfig,seat=v.seats[0]?.view;if(!c||!seat)return;
  const p=view.simulation?v.position:v.root.position;
  frameQ.copy(view.simulation?v.quaternion:v.flightFrameQ||v.root.quaternion);
  if(view.simulation)drawnQ.copy(v.quaternion).multiply(turn.setFromAxisAngle(Z,v.bank||0));else drawnQ.copy(v.root.quaternion);
  const fresh=!view.cameraSet||view.lastVehicle!==v;
  if(!fresh&&view.firstPerson!==firstPerson){view.blendPosition=cam.position.clone();view.blendQuaternion=cam.quaternion.clone();view.blendFov=cam.fov;view.blendTime=0;}
  view.lastVehicle=v;view.firstPerson=firstPerson;v.viewFirstPerson=firstPerson;
  if(view.simulation)v.aimCamera=cam;
  if(view.lastHealth!==undefined&&v.health<view.lastHealth)view.hitShake=.3;view.lastHealth=v.health;view.hitShake=Math.max(0,(view.hitShake||0)-dt);
  view.speedEffect=0;
  if(firstPerson){
    up.copy(Y).applyQuaternion(drawnQ);cam.up.copy(up);
    cam.position.copy(seat.eye).applyQuaternion(drawnQ).add(p);
    cam.quaternion.copy(drawnQ).multiply(flip);view.enclosed=true;cam.near=.03;
    if(view.hitShake)cam.position.addScaledVector(up,Math.sin(v.age*73)*view.hitShake*.25);
    if(v.boosting)cam.position.addScaledVector(up,Math.sin(v.age*51)*.025);
  }else{
    view.enclosed=false;cam.near=.1;
    frame.compose(p,frameQ,ONE);
    lookAlong(local.copy(Z).applyQuaternion(frameQ),up.copy(Y).applyQuaternion(frameQ),want).multiply(turn.setFromAxisAngle(X,-seat.tilt));
    view.pivot.copy(seat.center).applyMatrix4(frame);
    const snap=fresh||cam.position.distanceTo(view.pivot)>SNAP;
    if(snap)cam.quaternion.copy(want);else cam.quaternion.slerp(want,1-Math.exp(-seat.aimTension*dt));
    cam.up.copy(Y).applyQuaternion(cam.quaternion);
    view.origin.copy(RAY_ORIGIN).applyMatrix4(frame);
    view.target.copy(seat.offset).applyQuaternion(cam.quaternion).add(view.pivot);
    view.boom.subVectors(view.target,view.origin);
    const reach=view.clearance(v,cam.quaternion);
    if(snap||reach<view.reach)view.reach=reach;else view.reach+=(reach-view.reach)*(1-Math.exp(-RETURN_RATE*dt));
    view.target.copy(view.origin).addScaledVector(view.boom,view.reach);
    if(snap)cam.position.copy(view.target);
    else{
      easeAxes(cam,view.target,seat.moveTension,dt);
      // The eased spot can lag behind a new wall: check where it will draw.
      view.boom.subVectors(cam.position,view.origin);
      const clear=view.clearance(v,cam.quaternion);
      if(clear<1)cam.position.copy(view.origin).addScaledVector(view.boom,clear);
    }
    view.lookAt.copy(view.pivot);
  }
  const across=firstPerson?seat.firstFov:seat.thirdFov;
  let fov=THREE.MathUtils.radToDeg(2*Math.atan(Math.tan(across/2)/(cam.aspect||1)));
  if(view.blendTime<c.cameraBlendTime){
    view.blendTime+=dt;const t=Math.min(1,view.blendTime/c.cameraBlendTime),a=t*t*(3-2*t);
    cam.position.lerpVectors(view.blendPosition,cam.position.clone(),a);cam.quaternion.slerpQuaternions(view.blendQuaternion,cam.quaternion.clone(),a);fov=view.blendFov+(fov-view.blendFov)*a;
    // The way between the two views stays out of walls.
    view.origin.copy(p);view.boom.subVectors(cam.position,p);cam.position.copy(p).addScaledVector(view.boom,view.clearance(v,cam.quaternion));
  }
  cam.fov=fov;cam.updateProjectionMatrix();cam.updateMatrixWorld();view.cameraSet=true;
}
