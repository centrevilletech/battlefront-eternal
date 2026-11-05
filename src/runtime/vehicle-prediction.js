import { CommandControls } from './commands.js';
import { replayTicks } from './reconcile.js';
import { predictionVehicleInput } from './vehicle-prediction-input.js';
import { captureGroundCorrection } from './network-entity-ground.js';
import { VehiclesWorld } from '../vehicles-world.js';

const copy = value => value?.isVector3 || value?.isQuaternion || value?.isEuler || value?.isMatrix4 ? value.clone() : value;
const collisionViews = new WeakMap();
function predictionCollision(source) {
  let view = collisionViews.get(source);
  if (!view) {
    view = Object.create(source);
    // CollisionWorld keeps synchronous query callbacks closed over itself.
    // Execute queries on that same owner so scalar scratch and callback state
    // agree; only collider publication is suppressed for predicted hulls.
    for (const key of ['gather', 'gatherCache', 'cast', 'castMoving', 'ground', 'overlapsSphere']) {
      if (typeof source[key] === 'function') view[key] = source[key].bind(source);
    }
    view.moveBody = () => {};
    collisionViews.set(source, view);
  }
  return view;
}

// Isolate all simulation scratch vectors from the authoritative vehicle. The
// original collider remains the cast exclusion, but predicted poses never move it.
export function predictionVehicle(source) {
  const clone = Object.create(Object.getPrototypeOf(source));
  for (const [key, value] of Object.entries(source)) clone[key] = copy(value);
  for (const key of ['hit', 'aimHit', 'unitHit', 'frameScratch']) if (source[key]) clone[key] = Object.fromEntries(Object.entries(source[key]).map(([k,v])=>[k,copy(v)]));
  clone.wheels = source.wheels.map(wheel => Object.fromEntries(Object.entries(wheel).map(([k,v])=>[k,copy(v)])));
  clone.seats = source.seats.map(seat => ({ ...seat, lease: null }));
  clone.driver = {}; clone.seatIndex = 0;
  clone.itemTarget = { ...source.itemTarget, center: source.itemTarget.center.clone() };
  clone.predictionSource = source.predictionSource || source;
  clone.system = Object.create(clone.predictionSource.system);
  // Page input replay changes only this private hull.
  clone.system.f2 = null;
  clone.system.collision = predictionCollision(clone.predictionSource.system.collision);
  // Its queries go to that view of the collision too, never moving the real collider.
  clone.system.world = new VehiclesWorld(clone.system);
  clone.damage = () => {};
  // Drawn in its source's place (Vehicle.present): the seats its source's
  // crew holds, on the page's clock for what is only drawn.
  clone.held = i => clone.predictionSource.held(i);
  clone.age = clone.predictionSource.age;
  return clone;
}

// A ground hull's state as the world sends it to its pilot
// (network-entity-player.js), less how fast his aim turns and turned from
// (aimSeats steps them for his rider's pose; no replay does), or null for
// a flyer (its compact correction is rounded: a replay from it is not
// the prediction going on).
export function hullState(vehicle) {
  const seat = vehicle.seats?.[0];
  if (vehicle.flying || !seat) return null;
  const { yawRate, lastYaw } = seat;
  seat.yawRate = 0; seat.lastYaw = null;
  try { return captureGroundCorrection(vehicle); } catch { return null; } finally { seat.yawRate = yawRate; seat.lastYaw = lastYaw; }
}

// What the world steps on a hull and no replay does: the other seats' aim
// (their gunners') and how fast his own turns (his rider's pose). A
// prediction going on takes them from its source as a replay would.
function refreshCrew(vehicle, source) {
  for (let i = 1; i < source.seats.length && i < vehicle.seats.length; i++) Object.assign(vehicle.seats[i], source.seats[i], { lease: null });
  vehicle.seats[0].yawRate = source.seats[0].yawRate; vehicle.seats[0].lastYaw = source.seats[0].lastYaw;
}

// Whether the world's state for tick `tick`, restored onto `source`, is the
// state `prediction` reached for that tick: then replaying the pending
// commands from it would come out exactly as the prediction did (the same
// rules on the same input), and the prediction goes on as it is.
export function predictedExactly(prediction, source, tick) {
  const predicted = prediction?.predictionStates?.get(tick);
  const authority = predicted && hullState(source);
  if (!authority || authority.length !== predicted.length) return false;
  for (let i = 0; i < authority.length; i++) if (authority[i] !== predicted[i]) return false;
  return true;
}

// Ticks of a prediction's history a continuation keeps: a correction can be
// for any tick the page predicted since the last one.
const HISTORY = 60;

export function predictVehicle(source, pending, from, generation, held = []) {
  if (!source?.alive) return null;
  const vehicle = predictionVehicle(source), controls = new CommandControls();
  vehicle.predictionTick = from;
  vehicle.predictionPath = new Map();
  vehicle.predictionStates = new Map();
  // Continuing an earlier prediction: what it reached for each tick, so the
  // world's state for one of them is compared against it.
  for (const name of ['predictionPath', 'predictionStates']) {
    if (source[name]) for (const [tick, value] of source[name]) if (tick > from - HISTORY) vehicle[name].set(tick, value);
  }
  if (vehicle.predictionSource !== source) refreshCrew(vehicle, vehicle.predictionSource);
  const enabled = source.body.enabled;
  try {
    replayTicks(pending, from, generation, held, (_tick, command) => {
      controls.accept({ ...command, epoch: 1, sequence: command.tick });
      const input = predictionVehicleInput(vehicle, 0, controls, command);
      {
        vehicle.captureTick();
        // The world advances wheeled/repulsor collision at 120 Hz; flyers at 60 Hz.
        if (vehicle.flying) vehicle.tick(1 / 60, input);
        else { vehicle.tick(1 / 120, input); vehicle.tick(1 / 120, input); }
        // A walker's legs keep up with its predicted hull, as the world's
        // aimTick advances them after its ticks; the cycle before this tick
        // draws them between the two (Vehicle.poseCrew).
        vehicle.walkFromPhase = vehicle.walkPhase; vehicle.walkFromBlend = vehicle.walkBlend;
        vehicle.walk(1 / 60);
      }
      vehicle.predictionPath.set(command.tick, vehicle.position.clone());
      vehicle.predictionStates.set(command.tick, hullState(vehicle));
      vehicle.predictionTick = command.tick;
    });
  } finally {
    source.body.enabled = enabled;
  }
  return vehicle;
}
