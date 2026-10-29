import * as THREE from "three";
import { flightBindings, flightInput } from "./flight/controls.js";

// Getting into a vehicle, out of it and from seat to seat, and what a seat's
// keys do, as one set of rules for everyone aboard: the player (a page's own
// battle or the world worker's, src/vehicles.js VehicleDriver), a network
// human on a dedicated server or a desktop host (src/runtime/
// multiplayer-world.js), and the bots (src/ai/classic/vehicles). Every path
// that seats or unseats someone, or reads a seated person's keys, asks here
// (docs/vehicles.md "Boarding, leaving and seat controls").

const UP_EYE = 1.7; // m: the soldier's eye above his feet, where his E looks from
const eye = new THREE.Vector3();
const aim = new THREE.Vector3();

// A person (the player, a network human) rather than a bot, by unit or lease.
export const personUnit = (unit) => Boolean(unit && (unit.human || unit.networkHuman));
export const personLease = (lease) => Boolean(lease && (lease.human || lease.unit?.networkHuman));

// A craft off the ground: taking off, flying or landing.
export const inFlight = (v) => Boolean(v?.flying && v.alive && v.flightState !== "Landed");

// The registry's relation (1 friend, 0 neutral, -1 enemy) without alliances.
export const teamRelation = (a, b) => (a === b ? (a ? 1 : 0) : !a || !b ? 0 : -1);

// Whether a unit may get into anything now: alive, a soldier on foot in no
// seat (not a droideka), of a class that may enter vehicles (NoEnterVehicles);
// a person not in a jump, fall or jet (the player's E has always waited for
// his feet).
export function canBoard(unit, person = personUnit(unit)) {
  if (!unit || !unit.alive || unit.dead || unit.seat || unit.walker || !unit.soldier) return false;
  if (Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0) !== 0) return false;
  return !person || !unit.soldier.air;
}

// The seat a soldier of `team` boarding `vehicle` takes, or -1 when he may
// not. As the 2004 game's boarding eligibility has it (docs/
// ai-vehicles.md "Soldier boarding eligibility"):
// - the hull is alive;
// - a hull anyone holds, in any seat (pilot, gunner or passenger), takes
//   only that crew's friends (relation above 0). An empty hull of any team
//   takes a person;
// - a bot boards only its own team's hulls (a hull of no team, anyone's),
//   and only a free seat;
// - a person takes `preferred` when it is free (the player's last seat),
//   else the first seat that is free or a bot holds (Vehicles.displace moves
//   the bot on). He never takes another person's seat.
export function boardingSeat(vehicle, { team = 0, person = false, relation = teamRelation, preferred = -1 } = {}) {
  if (!vehicle?.alive || !vehicle.seats?.length) return -1;
  for (const seat of vehicle.seats) {
    const lease = seat.lease;
    if (lease && !lease.released && !(relation(team, lease.team) > 0)) return -1;
  }
  if (!person) {
    const own = vehicle.controllingTeam;
    if (own > 0 && own !== team) return -1;
    if (preferred >= 0) return vehicle.seatFree(preferred) ? preferred : -1;
    return vehicle.firstFreeSeat();
  }
  if (preferred >= 0 && vehicle.seatFree(preferred)) return preferred;
  for (let i = 0; i < vehicle.seats.length; i++) {
    const lease = vehicle.seats[i].lease;
    if (vehicle.seatFree(i) || lease && !personLease(lease)) return i;
  }
  return -1;
}

// Makes seat `index` his: a bot holding it moves on to the next free seat,
// or gets out (Vehicles.displace). Whether the seat is free now.
export function claimSeat(vehicle, index) {
  const lease = vehicle.seats[index]?.lease;
  if (lease && !lease.released && !personLease(lease)) vehicle.system?.displace?.(lease);
  return vehicle.seatFree(index);
}

// What a soldier's use key (E) gets him into: the nearest vehicle in reach
// ahead of him that he may board (Vehicles.enterable), else the turret he
// faces (LiveObjects.enterable). Null for nothing, or when he may not board.
// world: { vehicles, live }.
export function mountInReach(world, unit, person = personUnit(unit)) {
  if (!canBoard(unit, person)) return null;
  const s = unit.soldier;
  eye.set(s.position.x, s.position.y + UP_EYE, s.position.z);
  aim.set(Math.sin(s.aimYaw) * Math.cos(s.aimPitch), Math.sin(s.aimPitch), Math.cos(s.aimYaw) * Math.cos(s.aimPitch));
  const vehicles = world.vehicles;
  const relation = vehicles?.relation ?? teamRelation;
  const vehicle = vehicles?.enterable(eye, aim, (v) => boardingSeat(v, { team: unit.team, person, relation }) >= 0) ?? null;
  if (vehicle) return { vehicle };
  // As with a vehicle's seats, a person takes a bot's turret over (the bot
  // is set down beside it); he never takes another person's.
  const turret = world.live?.enterable(eye, aim, person ? (t) => turretTakeover(t) : null) ?? null;
  return turret ? { turret } : null;
}

// Whether a person may take this turret from whoever mans it: unmanned,
// or manned by a bot lease (TurretSeats.man / the local crew vacates it).
export function turretTakeover(turret) {
  if (!turret?.gunner) return true;
  const lease = turret.lease;
  return Boolean(lease && !lease.released && !personLease(lease));
}

// Whether the one holding `lease` may get out now. A person may at any time:
// in flight he bails out beside the hull and falls (the 2004 game's seat
// exit has no airborne test). A bot stays aboard a craft in flight (the
// Classic AI never bails out of one).
export function mayLeave(vehicle, lease) {
  return personLease(lease) || !inFlight(vehicle);
}

// A rider killed on his seat leaves it at once, his body falling from where
// it rode. The 2004 game's death in a seat runs the seat exit without its
// search for a spot beside the hull: the body stays at the seat, at rest,
// among soldiers again (a corpse rounds still meet),
// and while the hull lives the last bot aboard takes the seat over, as after
// any exit. The hull goes on without him: a hover slows to a stop at its
// Deceleration, a craft in flight holds its course (src/flight/pilotless.js),
// and anyone may take it. He isn't flung off: the 2004 game throws a dead
// rider (9 m/s along the round, lifted) only from a seat whose class sets
// hash 81bce0f9 to 0, which no retail class does. A turret's gunner leaves
// his gun the same way (the armed building seats its pilot through the same
// code), and the turret waits for whoever comes next. For a bot or a network
// human (Vehicles.vacate, TurretSeats.vacate) and the player
// (VehicleDriver.release, Gunner.exit) alike. Returns whether he held a
// seat.
export function leaveKilled(unit) {
  const lease = unit?.seat;
  if (!lease || lease.released) return false;
  const vehicle = lease.vehicle, turret = lease.turret;
  if (vehicle) {
    if (lease.human) vehicle.humanDriver?.release(false, true);
    else vehicle.system?.vacate(lease, { placement: false, reason: "killed" });
  } else if (turret) {
    if (lease.human) turret.humanGunner?.exit(unit.soldier, { killed: true });
    else turret.crew?.vacate(lease, { placement: false, reason: "killed" });
  } else return false;
  return true;
}

// A person's seat switch (the seat keys: 1 and 2 in a craft, F and the
// wheel elsewhere): on to the next seat that way that no other person
// holds. A bot there trades places with him. Into a gunner's seat his view
// goes with him, inside its arc; back at the controls he aims along the
// hull. Leaving the pilot's seat lets go of the stick (a craft with nobody
// at the controls holds its course: src/flight/pilotless.js). from: the seat
// he holds (the player's may have no lease in a fixture). Returns the new
// seat, or -1 with none to take.
export function switchSeat(system, vehicle, lease, from = lease?.index ?? 0, direction = 1) {
  const n = vehicle?.seats?.length ?? 0;
  if (!vehicle?.alive || n < 2) return -1;
  for (let k = 1; k < n; k++) {
    const to = (((from + direction * k) % n) + n) % n;
    const occupant = vehicle.seats[to].lease;
    if (occupant && occupant !== lease && personLease(occupant)) continue;
    // The player's seat, seen from a network human's switch.
    if (vehicle.driver && vehicle.seatIndex === to && !lease?.human) continue;
    const a = vehicle.seats[from], b = vehicle.seats[to];
    if (to > 0) { b.aimYaw = a.aimYaw; b.pitch = a.pitch; b.lookYaw = a.lookYaw; b.lookPitch = a.lookPitch; }
    if (lease) system.move(lease, to);
    if (occupant && occupant !== lease) system.move(occupant, from);
    if (to === 0) vehicle.restAim(0);
    else vehicle.clampAim(to);
    if (from === 0) vehicle.stickX = vehicle.stickY = 0;
    return to;
  }
  return -1;
}

// What a seat's keys and mouse do to the hull: the pilot of a craft flies it
// ("flight"), the driver of anything else drives it ("drive"), and every
// other seat ("turret") only aims and fires its own guns. Read for the seat
// held when the keys are read, never kept from before a seat change.
export function seatControl(vehicle, index) {
  if (!vehicle?.seats?.[index]) return null;
  if (index > 0) return "turret";
  return vehicle.flying ? "flight" : "drive";
}

// The key map a seat reads (which keys queue which actions, and which are
// held for fire): the flight bindings in every seat of a craft (its seat,
// use and fire keys among them), else the soldier's (null).
export const seatScheme = (vehicle, bindings) => (vehicle?.flying ? flightBindings(bindings) : null);

// Mouse counts on a seat: a craft's pilot moves its virtual stick (the
// flight sensitivity and invert); any other seat turns its aim, held inside
// the seat's limits (the mouse scale and invert).
export function seatLook(vehicle, index, dx, dy, { scale = 1, invert = false, flightScale = 1, flightInvert = false } = {}) {
  if (seatControl(vehicle, index) === "flight") {
    vehicle.stickX = THREE.MathUtils.clamp(vehicle.stickX + (dx * flightScale) / 120, -1, 1);
    vehicle.stickY = THREE.MathUtils.clamp(vehicle.stickY + ((flightInvert ? -dy : dy) * flightScale) / 120, -1, 1);
    const radius = Math.max(1, Math.hypot(vehicle.stickX, vehicle.stickY));
    vehicle.stickX /= radius;
    vehicle.stickY /= radius;
    return;
  }
  const seat = vehicle.seats[index];
  seat.lookYaw = (seat.lookYaw ?? seat.aimYaw) - (dx * scale) / 2048;
  seat.lookPitch = (seat.lookPitch ?? seat.pitch) + ((invert ? dy : -dy) * scale) / 2048;
  vehicle.clampAim(index);
}

// A seat's keys this tick, as the hull takes them (seatControl): the flight
// keys with the bindings the command carries, the driving keys (Space
// brakes), or nothing for a gunner's seat. assist false keeps steering
// assistance out (a network pilot's, whose replay must not depend on remote
// targets).
export function seatInput(vehicle, index, controls, command = {}, { assist = true } = {}) {
  const control = seatControl(vehicle, index);
  if (control === "flight") {
    const fs = command.flightSettings || {};
    const input = flightInput(controls, flightBindings(fs.bindings), command.gamepad);
    input.responseCurve = fs.curve;
    if (!assist) input.steeringAssist = false;
    return input;
  }
  if (control === "drive") return { forward: controls.axis("forward"), right: controls.axis("right"), turn: controls.axis("turn"), brake: controls.down.has("Space") };
  return { forward: 0, right: 0, turn: 0, brake: false };
}
