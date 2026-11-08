import {sceneProperty,sceneLayer} from "./rust-render/scene-changes.js";
import * as THREE from "three";

import { unseatBody } from "./seats.js";

import {EffectPhase} from './effects-gpu.js';
import {GunnerFrame as F} from './presentation/visual-effects-layout.js';
import {makeUnitHit} from './registry.js';
const BANKS=[['input',Float64Array]];

// Page view of the player at a turret. EntitySeats installs the world's lease;
// The native network frame supplies unacknowledged look to prop and body phases;
// the native camera phase follows their published turret and rider.
// First-person eyes inside the hull hide its rig just as a cockpit does.

export class Gunner {
  constructor({ camera, collision, ordnance, combat, animator, hiddenLayer = 1 }) {
    this.camera = camera;
    this.collision = collision;
    this.ordnance = ordnance;
    this.combat = combat;
    this.animator = animator;
    this.hiddenLayer = hiddenLayer;
    this._turret = null;
    this.hiddenRig = null; // the turret whose drum and gun are hidden (first person)
    // What the page shows over the world's aim: the looks it has not taken
    // yet and the way through the tick, published by the native frame.
    this.previewYaw = 0;
    this.previewPitch = 0;
    this.previewState = { yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0 };
    this.shownYaw = 0;
    this.shownPitch = 0;
    this.eye = new THREE.Vector3();
    this.firstPerson = false; // the world's view: where its aim starts (the camera phase)
    this.viewFirstPerson = false; // the page's view: where its reticle's aim starts
    this.reticlePoint = new THREE.Vector3();
    this.phase=new EffectPhase('PageGunnerView',BANKS);this.unitHit=makeUnitHit();this.origin=new THREE.Vector3();this.layerMeshes=new WeakMap();
    this.dir = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
  }

  get active() {
    return Boolean(this.turret);
  }

  // The turret manned, supplied by the seat packet (SeatReader.setGunner). Another turret, or
  // none: what the page showed of the last one is let go.
  get turret() {
    return this._turret;
  }
  set turret(turret) {
    if (turret === this._turret) return;
    const previous = this._turret;
    if (previous?.humanGunner === this) { previous.humanGunner = null; previous.gunner = null; }
    this._turret = turret;
    if (turret) { turret.humanGunner = this; turret.gunner = this; }
    this.hideRig(null);
    this.previewYaw = this.previewPitch = 0;
  }

  // SeatReader.clearSeat and page respawn only release presentation. The
  // native crew owner has already decided the exit and the body's placement.
  exit(soldier) {
    const turret = this.turret;
    if (!turret) return;
    const lease = turret.lease;
    if (lease?.human) {
      lease.released = true;
      if (lease.unit?.seat === lease) { lease.unit.seat = null; unseatBody(lease.unit); }
      turret.lease = null;
    }
    turret.gunner = null; turret.humanGunner = null;
    this.turret = null;
    if (this.combat) this.combat.mounted = false;
    this.animator?.gunner?.(null);
    const root = this.animator?.actor?.root;
    if (turret.hidesGunner && root && !this.combat?.blowApart) sceneProperty(root,"visible",true);
    this.hideRig(null);
  }

  // Native network preview consumes mouse counts from the command history.
  // This legacy input hook no longer retains a second, unread accumulator.
  look() {}

  facts(yaw=this.shownYaw,pitch=this.shownPitch){
    this.phase.refresh();const x=this.phase.input,t=this.turret;
    x[F.ACTIVE]=t?1:0;x[F.YAW]=yaw;x[F.PITCH]=pitch;this.camera.position.toArray(x,F.CAMERA);
    if(t){x[F.FACING]=t.facing;t.rig.root.updateWorldMatrix(true,false);x.set(t.rig.root.matrixWorld.elements,F.ROOT);
      t.pivot.toArray(x,F.PIVOT);t.up.toArray(x,F.UP);t.camera.eye.toArray(x,F.EYE_OFFSET);t.camera.center.toArray(x,F.CENTER_OFFSET);t.camera.offset.toArray(x,F.TRACK_OFFSET);x[F.TILT]=t.camera.tilt;
      x[F.FIRST]=this.viewFirstPerson?1:0;x[F.CANONICAL]=t.f2Seats?1:0;
      if(t.f2Seats){t.f2Pose.eye.toArray(x,F.CANONICAL_EYE);t.f2Pose.chase.toArray(x,F.CANONICAL_CHASE);}
    }
    this.phase.frame();
  }
  reticleDirection(out){
    if(!this.turret?.aimOrigin)return this.viewDirection(out);
    this.facts();const x=this.phase.input,t=this.turret;
    this.origin.fromArray(x,F.ORIGIN);this.dir.fromArray(x,F.DIRECTION);
    const wall=this.collision?.cast(this.origin,this.dir,x[F.REACH],this.hit,t.bodies?.built||null);
    x[F.WALL]=wall?1:0;x[F.WALL_DISTANCE]=wall?.distance??0;this.phase.kernel.wall();
    const unit=this.ordnance?.units?.raycast?.(this.origin,this.dir,x[F.REACH],t.lease?.unit??null,this.unitHit);
    x[F.UNIT]=unit?1:0;x[F.UNIT_DISTANCE]=unit?.distance??0;this.phase.kernel.resolve();this.reticlePoint.fromArray(x,F.POINT);
    return out.fromArray(x,F.RETICLE);
  }
  viewDirection(out){this.facts();return out.fromArray(this.phase.input,F.DIRECTION);}
  eyePoint(out,yaw=this.shownYaw,pitch=this.shownPitch){this.facts(yaw,pitch);return out.fromArray(this.phase.input,F.EYE);}

  // The drum and gun out of the view (first person), as his own body is:
  // on the hidden layer, where they still cast their shadows.
  hideRig(turret) {
    if (this.hiddenRig === turret) return;
    this.layer(this.hiddenRig,0);this.layer(turret,this.hiddenLayer);
    this.hiddenRig = turret;
  }
  layer(turret,value){
    if(!turret?.rig?.root)return;let meshes=this.layerMeshes.get(turret);
    if(!meshes){meshes=[];const visit=object=>{if(object.isMesh)meshes.push(object);};if(turret.drum||turret.gun){turret.drum?.traverse(visit);turret.gun?.traverse(visit);}else turret.rig.root.traverse(visit);this.layerMeshes.set(turret,meshes);}
    for(const mesh of meshes)sceneLayer(mesh.layers,"set",value);
  }
}
