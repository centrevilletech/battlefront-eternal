import * as THREE from 'three';
import { flightClass, flightConfig, throttleSpeed } from './config.js';
const X = new THREE.Vector3(1,0,0), Y = new THREE.Vector3(0,1,0), Z = new THREE.Vector3(0,0,1), DOWN = new THREE.Vector3(0,-1,0);
const clamp = THREE.MathUtils.clamp;
const alpha = (rate, dt) => 1-Math.exp(-rate*dt);
const forward = new THREE.Vector3(), up = new THREE.Vector3(), right = new THREE.Vector3(), desired = new THREE.Vector3(), axis = new THREE.Vector3();
const left = new THREE.Vector3(), local = new THREE.Vector3();
const dq = new THREE.Quaternion(), level = new THREE.Quaternion(), inverse = new THREE.Quaternion(), matrix = new THREE.Matrix4();
const hit = { point: new THREE.Vector3(), normal: new THREE.Vector3() };
const groundHit = { point: new THREE.Vector3(), normal: new THREE.Vector3(), body: null, height: 0 };
const landingAt = new THREE.Vector3(), landingAlong = new THREE.Vector3();
// The ground a landing settles on: flatter than this (the normal's height;
// the 2004 game's 0.9375, about 20 degrees).
const SETTLE = .9375;
// sweep()'s columns: the whole hull's, or the middle one, along the fuselage.
const FUSELAGE = true;
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
export function levelQuaternion(q, out) {
  forward.copy(Z).applyQuaternion(q); forward.y = 0;
  if (forward.lengthSq() < 1e-8) forward.copy(X).applyQuaternion(q).cross(Y);
  forward.normalize(); right.crossVectors(Y, forward).normalize();
  return out.setFromRotationMatrix(matrix.makeBasis(right, Y, forward));
}
// Sweep a grid across the full local bounding box. Rays are continuous in
// translation; interior samples also cover the gear and centre of the hull.
// below: surfaces no higher than this are looked past (a takeoff's own pad).
// rest: { position, quaternion }, the hull at rest: surfaces inside the box it
// fills there are looked past too. A pad's structure the parked hull already
// overlaps (on Bespin's gunship pad _16 it reaches into a corner of the box at
// mid-height) does not hold it down; the original lifts it out.
// fuselage: the middle column only, leaving the wings out.
export function sweep(v, from, to, q = v.quaternion, below = -Infinity, rest = null, fuselage = false) {
  const travel = to.clone().sub(from), distance = travel.length(); if (distance < 1e-8) return null;
  travel.divideScalar(distance); let nearest = null;
  const b = v.bounds;
  if (rest) restInverse.copy(rest.quaternion).invert();
  const xs = fuselage ? [(b.min.x+b.max.x)/2] : [b.min.x, (b.min.x+b.max.x)/2, b.max.x];
  for (const x of xs) for (const y of [b.min.y+.12, (b.min.y+b.max.y)/2, b.max.y]) for (const z of [b.min.z, (b.min.z+b.max.z)/2, b.max.z]) {
    const origin = new THREE.Vector3(x,y,z).applyQuaternion(q).add(from);
    let contact = null, skipped = 0;
    for (let pass = 0; pass < 4 && !contact && skipped < distance+.15; pass++) {
      contact = v.system.collision.cast(origin, travel, distance+.15-skipped, hit, v.body);
      if (contact && (contact.point.y <= below || rest && insideAtRest(b, rest, contact.point))) { skipped += contact.distance+.01; origin.addScaledVector(travel, contact.distance+.01); contact = null; }
      else break;
    }
    if (contact && (!nearest || skipped+contact.distance < nearest.distance)) nearest = { distance: skipped+contact.distance, point: contact.point.clone(), normal: contact.normal.clone() };
  }
  return nearest;
}
// A point inside box b (the hull's local bounds) placed at rest.
const restInverse = new THREE.Quaternion(), restLocal = new THREE.Vector3();
function insideAtRest(b, rest, point) {
  restLocal.copy(point).sub(rest.position).applyQuaternion(restInverse);
  return restLocal.x > b.min.x-.01 && restLocal.x < b.max.x+.01 && restLocal.y > b.min.y-.01 && restLocal.y < b.max.y+.01 && restLocal.z > b.min.z-.01 && restLocal.z < b.max.z+.01;
}
// The top of what a hull resting at `at` sits in: on a sloping pad or ground
// its lowest sweep rays start under that surface and would meet it on the way
// up. A takeoff leaves out contacts no higher (-Infinity: it sits clear). A
// craft standing on its gear looks down to just under its feet: its collision
// can end on the ground there (the Y-wing's, the MAF's), and its way down
// must not count the ground it lands on.
function restingTop(v, q, at) {
  const b = v.bounds; let top = -Infinity;
  const stand = standHeight(v), reach = stand !== null ? Math.max(2, b.min.y + stand + 2.22) : 2;
  for (const x of [b.min.x, (b.min.x+b.max.x)/2, b.max.x]) for (const z of [b.min.z, (b.min.z+b.max.z)/2, b.max.z]) {
    const under = v.system.collision.cast(new THREE.Vector3(x,b.min.y+2.12,z).applyQuaternion(q).add(at), DOWN, reach, hit, v.body);
    if (under) top = Math.max(top, under.point.y+.05);
  }
  return top;
}
export function landingSite(v) {
  const env = v.system.flightEnvironment || {}, c = v.flightConfig;
  const zone = env.landingZones?.find(z => new THREE.Box3(new THREE.Vector3().fromArray(z.min),new THREE.Vector3().fromArray(z.max)).containsPoint(v.position));
  const origin = v.position.clone(); origin.y += v.bounds.min.y + .3;
  const ground = v.system.collision.cast(origin, DOWN, c.landingHeight+.3, hit, v.body);
  if (zone) return { y: zone.floorY ?? zone.min[1], normal: Y };
  if (!env.space && ground && ground.normal.y >= c.maxSlope) return { y: ground.point.y, normal: ground.normal.clone() };
  return null;
}
function transition(v, state) { v.flightState = state; v.stateTime = 0; v.flightStart.copy(v.position); v.flightStartQ.copy(v.quaternion); }
// A lift nobody flies any more, still over its pad (a roof holding it, its
// crew gone): the craft goes back down onto it (the port's).
export function abandonLift(v) {
  if (v.flightState === 'TakingOff' && v.gear >= 1) transition(v, 'Landing');
}
function startTrick(v, kind, direction = 1) {
  const c = v.flightConfig;
  if (v.trick || v.power < c.trickCost || v.trickCooldown > 0 || v.flightState !== 'Flying') return;
  v.flightTrickRight.copy(X).applyQuaternion(v.quaternion);
  if(kind==='immelmann') {
    v.flightTrickLoopQ.copy(v.quaternion).multiply(new THREE.Quaternion().setFromAxisAngle(X,-Math.PI));
    levelQuaternion(v.flightTrickLoopQ,v.flightTrickEndQ);
  }
  v.power -= c.trickCost; v.trick = kind; v.trickTime = 0; v.trickDirection = direction; v.trickPending = -1;
}
// How high the craft's origin is over what lies below it, the hull left out:
// the 2004 game's 1024 m ray down (also what a page drawing a craft from its
// records stows the gear by, src/flight/gear.js). A craft whose gear stands
// less than 0.1 m below its origin (the MAF's lowest point is 0.29 m above
// it) rests with its origin in its pad: its ray starts 0.1 m over its gear's
// lowest point, so it still finds the ground it stands on (and the height
// is below zero there).
const rayFrom = new THREE.Vector3();
export function heightAbove(v) {
  return groundBelow(v, hit) ? hit.height : 1024;
}
// That ray's hit, written to out { point, normal, body, height }, or null:
// what a landing sets the craft down on (out.body null for the world's own
// collision, else the moving body it struck).
function groundBelow(v, out) {
  const lift = v.gearFoot != null && v.gearFoot < .1 ? .1 - v.gearFoot : 0;
  rayFrom.copy(v.position); rayFrom.y += lift;
  const below = v.system.collision.cast(rayFrom, DOWN, 1024 + lift, out, v.body);
  if (!below) return null;
  out.height = below.distance - lift;
  return out;
}
// How high a landed craft's origin stands over the ground under it: on its
// gear's lowest point (gearFoot, src/flight/gear.js: the craft with a takeoff
// clip), its collision never in the ground. Null for a craft without gear,
// which rests on its collision a clearance up (Vehicle.spawn, the landing).
export function standHeight(v) {
  return v.gearFoot != null ? Math.max(v.gearFoot, -v.bounds.min.y) : null;
}
// How high a craft's origin stands over the ground it has landed on: on its
// gear (standHeight), else its collision 0.15 m clear of it.
function settledHeight(v) {
  return standHeight(v) ?? -v.bounds.min.y + .15;
}
// The speed along the nose, as the 2004 game sets it each update: the stick's
// target (MidSpeed released, MaxSpeed forward, MinSpeed back; boost holds
// MaxSpeed), reached at the craft's Acceleraton from the speed it is flying
// at along its nose, so a bump or a takeoff's climb holds it back.
function noseSpeed(v, c, dt, throttle) {
  forward.copy(Z).applyQuaternion(v.quaternion);
  const rate = c.acceleration*(v.boosting?c.boostAcceleration:1)*dt, current = v.velocity.dot(forward);
  return clamp(v.boosting?c.maxSpeed:throttleSpeed(c,throttle), current-rate, current+rate);
}
// The drawn hull's lean into a turn (v.bank, about the nose; the frame the
// craft flies by, which the chase camera follows, stays level): BankAngle
// times the sideways part of the turn's pull, (rates x velocity) in the
// frame, over `per` (MaxSpeed; TurnRate x MaxSpeed while it takes off),
// eased at BankFilter. Turning right it leans right.
function lean(v, c, dt, per) {
  local.copy(v.velocity).applyQuaternion(inverse.copy(v.quaternion).invert());
  const pull = (v.yawRate*local.z - v.flightRollRate*local.y)/per;
  v.bank += (-c.bankAngle*pull - v.bank)*c.bankFilter*dt;
}
// The wings level at LevelFilter: the roll rate that brings the frame's
// side axis back to the horizon.
const levelRoll = (v, c) => -c.levelFilter*left.copy(X).applyQuaternion(v.quaternion).y;
// The frame turns by its rates for dt about their common axis in its own
// frame, as the 2004 game turns it.
function turnFrame(v, pitchStep, yawStep, rollStep) {
  axis.set(pitchStep, yawStep, rollStep); const angle = axis.length();
  if (angle > 1e-12) v.quaternion.multiply(dq.setFromAxisAngle(axis.divideScalar(angle), angle)).normalize();
}
// The lift off a pad and the landing, as the 2004 game moves a craft in
// both: its progress p (1 - gear) is how far it is
// over to flight. It goes along its nose at its speed as much as it flies
// and climbs at `rise` (TakeoffSpeed up, LandingSpeed down) as much as it
// does not, an eighth of that at the ground and all of it TakeoffHeight up.
// The stick (stickX, stickY, as the hull takes it) turns it as much as it
// is up and pitches it as much as it flies; its nose and wings level
// meanwhile.
function lift(v, c, dt, height, rise, throttle, stickX, stickY, pitchRate) {
  const p = 1 - v.gear, lifted = clamp(height/c.takeoffHeight, 0, 1);
  v.airspeed = noseSpeed(v, c, dt, throttle);
  v.tiltRate *= Math.max(0, 1-(1-p)*c.levelFilter*dt);
  v.tiltRate -= .5*c.levelFilter*(1-p)*forward.copy(Z).applyQuaternion(v.quaternion).y;
  v.tiltRate += (stickY*pitchRate*p - v.tiltRate)*Math.min(1, c.pitchFilter*dt);
  v.yawRate += (-stickX*c.turnRate*lifted - v.yawRate)*Math.min(1, c.turnFilter*dt);
  v.flightRollRate = levelRoll(v, c);
  v.velocity.copy(Z).applyQuaternion(v.quaternion).multiplyScalar(v.airspeed*p).addScaledVector(Y, (.875*lifted + .125)*rise*(1-p));
  lean(v, c, dt, c.turnRate*Math.max(Math.abs(c.maxSpeed),1e-6));
  turnFrame(v, -v.tiltRate*dt, v.yawRate*dt, v.flightRollRate*dt);
}
export function stepFlight(v, dt, input) {
  if (!v.flightConfig) initFlight(v);
  const c = v.flightConfig, env = v.system.flightEnvironment || {};
  v.flightPreviousQ.copy(v.quaternion); v.flightClock += dt; v.stateTime += dt;
  v.trickCooldown = Math.max(0,v.trickCooldown-dt); v.landingMessageTime=Math.max(0,v.landingMessageTime-dt); v.flightWarning = v.landingMessageTime>0?v.landingMessage:'';
  const site = landingSite(v);
  v.floorY = site?.y ?? NaN; v.altitude = site ? Math.max(0,v.position.y+v.bounds.min.y-site.y) : 1000;
  // Where a landing soon settles: near ground under it, flat enough (the
  // TO LAND prompt, and where a bot presses the key). The key lands the
  // craft wherever it is, at any speed.
  v.landingReason = !site ? 'No landing zone' : 'Press SPACE to land';
  // The stick, as the hull takes it. A person's mouse turns and pitches the
  // craft at TurnRate (the 2004 game gives a player on the mouse TurnRate
  // for both); a bot's stick pitches it at PitchRate. Steering assistance
  // softens it near an assist target.
  const response=x=>Math.sign(x)*Math.pow(Math.abs(x),input.responseCurve||c.responseCurve)*(input.steeringAssist!==false && v.system.flightAssists && v.flightAssistId>=0?c.stickyScale:1);
  const sx=clamp(input.stickX ?? v.stickX,-1,1), sy=clamp(input.stickY ?? v.stickY,-1,1), throttle=clamp(input.forward||0,-1,1);
  const pitchRate=input.bot?c.pitchRate:c.turnRate;
  const action = Boolean(input.takeoff || input.up || input.down);
  if (action && !v.flightActionHeld) {
    // The key, as the 2004 game's flyer update takes it, refuses
    // nothing: landed, the craft lifts off; flying, it starts down wherever
    // it is; late in a takeoff (its progress, 1 - gear, past 0.75) it goes
    // back down, and late in a landing (under 0.25) back up. What it meets
    // on the way is the lift's and the descent's own business. A space map
    // lands only in its zones (the port's), and a trick is flown out first.
    const progress = 1 - v.gear;
    if (v.flightState === 'Landed') { transition(v,'TakingOff'); v.gear=1; }
    else if (v.flightState === 'Flying' && !v.trick) {
      if (env.space && !site) { v.flightWarning = v.landingMessage = v.landingReason; v.landingMessageTime=2; }
      else transition(v,'Landing');
    } else if (v.flightState === 'TakingOff' && progress > .75) transition(v,'Landing');
    else if (v.flightState === 'Landing' && progress < .25) transition(v,'TakingOff');
  }
  v.flightActionHeld = action;
  v.boosting = false;
  // A takeoff goes over to flight once the craft is TakeoffHeight above
  // where it rests, its progress (1 - gear) reaching 1 in TakeoffTime; a
  // landing's runs out in LandingTime.
  let height = 0, ground = null;
  if (v.flightState === 'TakingOff') {
    height = heightAbove(v);
    if (height > c.takeoffHeight + c.landedHeight) v.gear = Math.max(0, v.gear - dt/c.takeoffTime);
    if (v.gear <= 0) { v.gear = 0; transition(v,'Flying'); }
  } else if (v.flightState === 'Landing') {
    ground = groundBelow(v, groundHit); height = ground ? ground.height : 1024;
    v.gear = Math.min(1, v.gear + dt/c.landingTime);
  }
  if (v.flightState === 'Landed') { v.velocity.set(0,0,0); v.airspeed=0; v.yawRate=v.tiltRate=v.flightRollRate=0; v.bank=0; }
  else if (v.flightState === 'TakingOff') {
    lift(v, c, dt, height, c.takeoffSpeed, throttle, response(sx), response(sy), pitchRate);
    // The pad it lifts from and what its parked hull overlapped are looked
    // past. Under a roof it slides along it, going on over to flight;
    // anything else in the way holds it where it is.
    const next = v.position.clone().addScaledVector(v.velocity, dt);
    const pad = restingTop(v, v.flightStartQ, v.flightStart), parked = { position: v.flightStart, quaternion: v.flightStartQ };
    let contact = sweep(v, v.position, next, v.quaternion, pad, parked);
    if (contact) v.flightWarning = 'Takeoff path blocked';
    if (contact && next.y > v.position.y) { next.y = v.position.y; contact = sweep(v, v.position, next, v.quaternion, pad, parked); }
    if (!contact) v.position.copy(next);
  } else if (v.flightState === 'Landing') {
    // The lift run back: its speed along the nose fades as its
    // progress runs out while it sinks at LandingSpeed, an eighth of it at
    // the ground and all of it TakeoffHeight up.
    lift(v, c, dt, height, -c.landingSpeed, throttle, response(sx), response(sy), pitchRate);
    // It comes to rest on the ground under its middle (or on the water, which
    // holds up a flyer's origin), its gear on it. Going along, its whole
    // hull meets what it runs into; going down, what lies under its wings
    // holds nothing up (the 2004 game's craft meet the ground with the
    // spheres their ODF names, docs/formats.md), nor does what it will rest
    // in, but its fuselage stops on what it comes down on top of.
    const next = v.position.clone().addScaledVector(v.velocity, dt), rest = settledHeight(v);
    const water = v.system.water, wet = water?.over?.(next.x, next.z) ? water.level : -Infinity;
    const floor = Math.max(ground ? ground.point.y + rest : -Infinity, wet);
    const down = next.y <= floor;
    if (down) next.y = floor;
    levelQuaternion(v.quaternion, level);
    const below = Number.isFinite(floor) ? restingTop(v, level, landingAt.set(next.x, floor, next.z)) : -Infinity;
    const along = landingAlong.set(next.x, v.position.y, next.z);
    const wall = sweep(v, v.position, along, v.quaternion, below);
    if (wall) {
      // Run into on its way, it stops there, hurt as it would be in flight.
      const impact = Math.max(0, -(along.x - v.position.x)*wall.normal.x - (along.z - v.position.z)*wall.normal.z)/dt;
      if (impact > c.lethalImpactSpeed) v.damage(Infinity);
      else v.damage(v.maxHealth*Math.min(c.scrapeDamageLimit, Math.pow(impact/c.impactDamageSpeed, 2)));
      along.copy(v.position);
    }
    const blocked = sweep(v, along, landingAt.set(along.x, next.y, along.z), v.quaternion, below, null, FUSELAGE);
    if (blocked) v.flightWarning = 'Landing path blocked';
    // What it was held back from slows it, as a bump does in flight.
    const to = landingAt.set(along.x, blocked ? v.position.y : next.y, along.z);
    v.velocity.subVectors(to, v.position).divideScalar(dt); v.position.copy(to);
    // On the ground it is down where the ground is flat enough (the 2004
    // game's 0.9375); a hull it comes down on sends it back up; on water or
    // a steeper slope it stays on its way down until he lifts off again.
    if (v.alive && down && !blocked && ground && floor === ground.point.y + rest) {
      if (ground.body?.owner?.system === v.system) transition(v,'TakingOff');
      else if (ground.normal.y > SETTLE) { transition(v,'Landed'); v.gear=1; v.airspeed=0; v.bank=0; v.velocity.set(0,0,0); v.quaternion.copy(level); }
    }
  } else if (v.flightState === 'Flying') {
    const roll=clamp(input.right||0,-1,1);
    if (!input.boost) v.boostExhausted=false;
    v.boosting=Boolean(input.boost && throttle>0 && v.power>0 && !v.boostExhausted);
    if (v.boosting) { v.power=Math.max(0,v.power-c.powerDrain*dt); if (!v.power) v.boostExhausted=true; }
    v.airspeed=noseSpeed(v, c, dt, throttle);
    if (input.trick && !v.trickHeld) {
      if (v.trickPending>=0 && v.flightClock-v.trickPending<=c.trickWindow) startTrick(v,'loop');
      else v.trickPending=v.flightClock;
    }
    v.trickHeld=Boolean(input.trick);
    if (input.immelmann && !v.immelmannHeld) startTrick(v,'immelmann'); v.immelmannHeld=Boolean(input.immelmann);
    if (v.trickPending>=0 && v.flightClock-v.trickPending>c.trickWindow) v.trickPending=-1;
    // A short wait allows a double tap to take precedence over mouse noise.
    if (v.trickPending>=0 && (roll || Math.abs(sx)>.3) && v.flightClock-v.trickPending>.06) startTrick(v,'barrel',Math.sign(roll||sx));
    // As in the 2004 game, the stick asks for a turn at TurnRate and a pitch
    // (see pitchRate above), and the rates follow it at TurnFilter and
    // PitchFilter. The turn is about the frame's own up, which stays level,
    // so the craft turns flat. Boost costs a share of both (the port's).
    const authority=v.boosting?c.boostTurn:1;
    v.yawRate+=(-response(sx)*c.turnRate*authority-v.yawRate)*Math.min(1,c.turnFilter*dt);
    v.tiltRate+=(response(sy)*pitchRate*authority-v.tiltRate)*Math.min(1,c.pitchFilter*dt);
    v.rollLock=Boolean(input.rollLock);
    // A/D roll the frame (the port's); let go, its wings level at
    // LevelFilter as the 2004 game levels them, but not in space, and the
    // roll lock holds the roll as it is.
    const rollRate=v.rollLock?0:roll?roll*c.rollRate:env.space||v.trick?0:levelRoll(v,c);
    let immelmannRoll=-1, lateralStep=0;
    let pitchStep=-v.tiltRate*dt, rollStep=rollRate*dt, yawStep=v.yawRate*dt;
    if (v.trick) {
      const duration=v.trick==='barrel'?c.barrelTime:v.trick==='loop'?c.loopTime:c.immelmannTime;
      const a=v.trickTime/duration, b=Math.min(1,(v.trickTime+dt)/duration);
      yawStep=0;
      if (v.trick==='barrel') { rollStep=(b-a)*Math.PI*2*v.trickDirection; lateralStep=c.barrelDisplacement*(Math.cos(a*Math.PI)-Math.cos(b*Math.PI))*.5*v.trickDirection; }
      else if(v.trick==='loop') { pitchStep=-(b-a)*Math.PI*2; rollStep=0; }
      else { pitchStep=-(Math.min(b/.7,1)-Math.min(a/.7,1))*Math.PI; rollStep=(Math.max(0,(b-.7)/.3)-Math.max(0,(a-.7)/.3))*Math.PI; }
      if(v.trick==='immelmann'&&b>=.7)immelmannRoll=(b-.7)/.3;
      v.trickTime+=dt;
      if (b>=1) { v.trick=''; v.trickCooldown=c.trickCooldown; }
    }
    turnFrame(v, pitchStep, yawStep, rollStep);
    if(immelmannRoll>=0)v.quaternion.slerpQuaternions(v.flightTrickLoopQ,v.flightTrickEndQ,Math.min(1,immelmannRoll));
    v.flightRollRate=rollStep/dt;
    // Boundary recovery changes attitude smoothly; velocity always follows it.
    forward.copy(Z).applyQuaternion(v.quaternion);
    let recovery=false; desired.copy(forward);
    if (!env.space && v.position.y>(env.ceiling??v.system.maxFlyHeight)) { desired.y=-.5; recovery=true; v.flightWarning='Flight ceiling'; }
    if (env.bounds) {
      const min=new THREE.Vector3().fromArray(env.bounds.min), max=new THREE.Vector3().fromArray(env.bounds.max);
      if (v.position.x<min.x || v.position.x>max.x || v.position.z<min.z || v.position.z>max.z || env.space&&(v.position.y<min.y||v.position.y>max.y)) {
        desired.addVectors(min,max).multiplyScalar(.5).sub(v.position).normalize(); recovery=true; v.flightWarning='Leaving battlefield';
      }
    }
    if(recovery) { dq.setFromUnitVectors(forward,desired.normalize()); dq.slerpQuaternions(new THREE.Quaternion(),dq.clone(),alpha(c.recoverySharpness,dt)); v.quaternion.premultiply(dq).normalize(); }
    v.velocity.copy(Z).applyQuaternion(v.quaternion).multiplyScalar(v.airspeed);
    lean(v, c, dt, Math.max(Math.abs(c.maxSpeed),1e-6));
    const next=v.position.clone().addScaledVector(v.velocity,dt).addScaledVector(v.flightTrickRight,lateralStep), contact=sweep(v,v.position,next);
    if(contact) {
      const speed=v.velocity.length(), impact=Math.max(0,-v.velocity.dot(contact.normal));
      v.trick=''; v.trickCooldown=c.trickCooldown;
      v.position.addScaledVector(v.velocity,Math.max(0,contact.distance-.2)/Math.max(speed,1));
      if(impact>c.lethalImpactSpeed && impact/speed>c.lethalImpactCosine) v.damage(Infinity);
      else {
        v.damage(v.maxHealth*Math.min(c.scrapeDamageLimit,Math.pow(impact/c.impactDamageSpeed,2)));
        v.velocity.addScaledVector(contact.normal,impact+2);
        if(v.velocity.lengthSq()>1) { forward.copy(Z).applyQuaternion(v.quaternion); dq.setFromUnitVectors(forward,desired.copy(v.velocity).normalize()); v.quaternion.premultiply(dq); }
        v.airspeed=Math.max(c.minSpeed,v.velocity.length());
      }
    } else v.position.copy(next);
    // The way it is going: a craft that flies backward looks behind.
    forward.copy(Z).applyQuaternion(v.quaternion).multiplyScalar(v.airspeed<0?-1:1);
    const ahead=Math.abs(v.airspeed)*c.terrainLookahead;
    v.terrainWarning=ahead>0&&Boolean(v.system.collision.cast(v.position,forward,ahead,hit,v.body));
  }
  if (!v.boosting) v.power=Math.min(100,v.power+c.powerRecharge*dt);
  v.stickX*=Math.exp(-c.stickReturn*dt); v.stickY*=Math.exp(-c.stickReturn*dt);
  v.grounded=v.flightState==='Landed'; v.sleeping=!v.occupied&&v.grounded;
  v.throttle=v.airspeed;
  // Heading projections are read-only telemetry for shared AI/seat services;
  // the roll is the drawn lean.
  forward.copy(Z).applyQuaternion(v.quaternion); v.yaw=Math.atan2(forward.x,forward.z); v.tilt=Math.asin(clamp(forward.y,-1,1)); v.roll=v.bank;
  if(v.seats?.[0]) { v.seats[0].aimYaw=v.yaw; v.seats[0].pitch=v.tilt; }
  if(v.landingMessageTime>0)v.flightWarning=v.landingMessage;
  v.syncBody();
}
