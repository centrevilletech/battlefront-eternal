import { CommandControls } from './commands.js';
import { replayTicks } from './reconcile.js';
import { networkVehicleInput } from './network-vehicle.js';

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
  clone.system.collision = predictionCollision(clone.predictionSource.system.collision);
  clone.damage = () => {};
  // Drawn in its source's place (Vehicle.present): the seats its source's
  // crew holds, on the page's clock for what is only drawn.
  clone.held = i => clone.predictionSource.held(i);
  clone.age = clone.predictionSource.age;
  return clone;
}

export function predictVehicle(source, pending, from, generation, held = []) {
  if (!source?.alive) return null;
  const vehicle = predictionVehicle(source), controls = new CommandControls();
  vehicle.predictionTick = from;
  vehicle.predictionPath = new Map();
  const enabled = source.body.enabled;
  try {
    replayTicks(pending, from, generation, held, (_tick, command) => {
      controls.accept({ ...command, epoch: 1, sequence: command.tick });
      const input = networkVehicleInput(vehicle, 0, controls, command);
      vehicle.captureTick();
      // The world advances wheeled/repulsor collision at 120 Hz; flyers at 60 Hz.
      if (vehicle.flying) vehicle.tick(1 / 60, input);
      else { vehicle.tick(1 / 120, input); vehicle.tick(1 / 120, input); }
      // A walker's legs keep up with its predicted hull, as the world's
      // aimTick advances them after its ticks; the cycle before this tick
      // draws them between the two (Vehicle.poseCrew).
      vehicle.walkFromPhase = vehicle.walkPhase; vehicle.walkFromBlend = vehicle.walkBlend;
      vehicle.walk(1 / 60);
      vehicle.predictionPath.set(command.tick, vehicle.position.clone());
      vehicle.predictionTick = command.tick;
    });
  } finally { source.body.enabled = enabled; }
  return vehicle;
}
