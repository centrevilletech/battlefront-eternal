import { stepPrediction } from './helpers/hull-prediction.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {Soldier} from './helpers/body.js';
import {fixture} from './flight-fixture.js';
// Review tripwire: classify additions before deciding whether the correction
// must carry them. These lists deliberately include excluded state so a new
// physics field cannot silently hide behind a codec-only roundtrip test.
const words = text => text.trim().split(/\s+/);
function classified(groups) {
 const result=Object.values(groups).flatMap(words);
 assert.equal(new Set(result).size,result.length,'a replay field must have one classification');
 return result.sort();
}
test('every Soldier field has an explicit replay ownership classification',()=>{
 const groups={
  exact:'fuel position settled velocity normal aimYaw aimPitch posture air dive diveFrom stateTime lock grounded',
  prerequisites:'world maxSpeed maxStrafe maxJump maxTurn maxPitch accel pitchLimits jet collisionRule maxFlyHeight getupTime diveTime',
  overwritten:'lookX lookY tickLength collisionDamage contactCount contactMean forwardAxis rightAxis left facing pull motion center closest point contactNormal surfaceNormal pushDir clipDir push before origin hit groundHit stepBlocked stepStart stepTarget stepVelocity stepResolved',
  presentation:'previous groundAdjustment',
  aiOnly:'avoidance',
  deadBody:'passThrough dead',
  machineLocal:'gatherCaches mover',
 };
 assert.deepEqual(Object.keys(new Soldier({},{})).sort(),classified(groups),'Classify new fields in docs/network-replay-contract.md and add a shared-simulation continuation case');
});
test('vehicle scalar additions require replay or presentation ownership review',()=>{
 const v=fixture().v;v.resetPresentationHistory();v.flightState='Flying';v.position.y=100;stepPrediction(v.system,1/60,v,{});
 const groups={
  exact:'health team yaw tilt roll rollRate support yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickCooldown trickPending gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld flightState trick flightLockState flightWarning landingMessage landingReason',
  conditional:'trickTime trickDirection',
  prerequisites:'acceleration base boostSpeed boundRadius creature deceleration flightClass flying gearFoot healthType hover label legTop maxHealth midSpeed minSpeed name probeRadius radius repulsor reverseSpeed speed strafeSpeed turnRate walking waterDepth waterRule wheeled',
  presentation:'age aimCamera clips hoverPhase pose previousRoll previousTilt previousYaw rotation tickFromRoll tickFromTilt tickFromYaw viewFirstPerson walkBlend walkPhase',
  externalLifecycle:'dying lastAttacker seatIndex',
  separateTowing:'towAngle towLeg towTarget towWinding',
  hostDamage:'waterClock',
  nativeIdentity:'simSlot simGen f2Synced driver',
 };
 const fields=Object.keys(v).filter(k=>v[k]===null||['number','boolean','string'].includes(typeof v[k])).sort();
 assert.deepEqual(fields,classified(groups),'Classify new vehicle scalar state and add a shared-simulation continuation case');
});
