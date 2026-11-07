// Cold replica construction only. F2 and ClientPrediction own flight ticks.
// Keep the same defaults so a new facade can receive its first correction.
import * as THREE from 'three';
import { flightClass, flightConfig } from './config.js';

export function initFlight(v) {
  v.flightClass = flightClass(v.name); v.flightConfig = flightConfig(v.def, v.flightClass);
  const c = v.flightConfig;
  v.turnRate = c.turnRate; v.minSpeed = c.minSpeed; v.midSpeed = c.cruiseSpeed; v.speed = c.maxSpeed; v.boostSpeed = c.boostSpeed;
  v.flightState = 'Landed'; v.power = 100; v.stickX = v.stickY = 0;
  v.flightRollRate = v.bank = 0; v.rollLock = false; v.boosting = false;
  v.trick = ''; v.trickTime = v.trickCooldown = 0; v.trickPending = -1; v.trickDirection = 1;
  v.gear = 1; v.landingMessage = ''; v.landingMessageTime = 0; v.flightTrickRight = new THREE.Vector3();
  v.flightClock = 0; v.boostExhausted = false; v.landingReason = ''; v.flightWarning = ''; v.terrainWarning = false;
  v.flightAssistId = -1; v.flightLockId = -1; v.flightLockGeneration = -1; v.flightLockTimer = v.flightLostLOS = 0; v.flightLockState = '';
  v.flightStart = new THREE.Vector3(); v.flightEnd = new THREE.Vector3(); v.flightStartQ = new THREE.Quaternion();
  v.flightTrickLoopQ = new THREE.Quaternion(); v.flightTrickEndQ = new THREE.Quaternion();
  v.flightPreviousQ = v.quaternion.clone(); v.flightTickQ = v.quaternion.clone();
}

// A new replica stands on its loaded gear before its first native pose.
export function standHeight(v) {
  return v.gearFoot != null ? Math.max(v.gearFoot, -v.bounds.min.y) : null;
}
