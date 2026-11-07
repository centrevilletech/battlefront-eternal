import * as THREE from "three";
import {SoldierMirror} from "./sim/gen/units.js";
import { makeGround, makeHit } from "./collision.js";
import { constructSoldier, spawnSoldier } from "./presentation/body.js";

// Soldier is a page resource/correction mirror. Class values and spawn resets
// are constructed by the native unit owner, shared with authoritative bodies.
// Authored clip scaling and constants still read by isolated movement checks.
export const ENGINE = { diveLength:0.75, steepNormal:0.75, landLock:0.3, thrownGravity:13.5 };

export class Soldier extends SoldierMirror {
  constructor(world, def, native=world.rays.x) {
    // Generated numeric fields start as doubles, so completed native rows
    // never allocate boxed values when a previously integral angle changes.
    super();
    this.world = world;
    constructSoldier(this, world, def, native);
    // Legacy exact-correction payloads still carry collision scratch. These
    // are resource fields; the native movement owner never reads them back.
    this.contactCount = 0;
    this.contactMean = new THREE.Vector3();
    this.forwardAxis = new THREE.Vector3();
    this.rightAxis = new THREE.Vector3();
    this.left = new THREE.Vector3();
    this.facing = new THREE.Vector3();
    this.pull = new THREE.Vector3();
    this.motion = new THREE.Vector3();
    this.center = new THREE.Vector3();
    this.closest = new THREE.Vector3();
    this.point = new THREE.Vector3();
    this.contactNormal = new THREE.Vector3();
    this.surfaceNormal = new THREE.Vector3();
    this.pushDir = new THREE.Vector3();
    this.clipDir = new THREE.Vector3();
    this.push = new THREE.Vector3();
    this.before = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    // The sum of near-horizontal building and prop contact normals since a
    // bot's path executor last read it: it steers round what it bumps into.
    this.avoidance = new THREE.Vector3();
    this.stepBlocked = false;
    this.stepStart = new THREE.Vector3();
    this.stepTarget = new THREE.Vector3();
    this.stepVelocity = new THREE.Vector3();
    this.stepResolved = new THREE.Vector3();
    // A body this one passes through (a rider's mount, while his body falls
    // from its back: src/seats.js dropBody), until it spawns again.
    this.passThrough = null;
    this.hit=makeHit();
    this.groundHit=makeGround();
    this.gatherCaches = undefined;

  }

  // Cold placement only; adopted world bodies replace this with lifecycle
  // commands and displayed frames read the resulting native publication.
  spawn(x, y, z, yaw) { spawnSoldier(this,x,y,z,yaw); }

}
