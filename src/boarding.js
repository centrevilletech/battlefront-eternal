import * as THREE from "three";
import { flightBindings } from "./flight/controls.js";

// Browser identity transport for native seat eligibility and control records.
import {clientExports as x} from './runtime/fixed-clock.js';
import {PageSeatFrameLayout as F,PageSeatRowLayout as R} from './sim/gen/page-targeting.js';
const handle=x.page_seat_create(),framePtr=x.page_seat_frame(handle),rowsPtr=x.page_seat_rows(handle);
let frame,rows;
const eye=new THREE.Vector3(),aim=new THREE.Vector3();
function refresh(){if(!frame||frame.length===0){frame=new Float64Array(x.memory.buffer,framePtr,F.words);rows=new Float64Array(x.memory.buffer,rowsPtr,128*R.words);}}
function run(op){return x.page_seat_run(handle,op);}
function leaseFacts(lease){refresh();frame[F.human]=+!!lease?.human;frame[F.networkHuman]=+!!lease?.unit?.networkHuman;frame[F.lease]=+!!lease;frame[F.released]=+!!lease?.released;}
export function personUnit(unit){refresh();frame[F.human]=+!!unit?.human;frame[F.networkHuman]=+!!unit?.networkHuman;return !!run(0);}
export function personLease(lease){leaseFacts(lease);return !!run(0);}
function vehicleFacts(v){refresh();frame[F.alive]=+!!v?.alive;frame[F.flying]=+!!v?.flying;frame[F.inFlight]=+(v?.flightState!=='Landed');}
export function inFlight(v){vehicleFacts(v);return !!run(1);}
export const teamRelation=(a,b)=>x.page_relation(a,b);
export function canBoard(unit,person=personUnit(unit)){
  refresh();frame[F.alive]=+!!unit?.alive;frame[F.dead]=+!!unit?.dead;frame[F.seated]=+!!unit?.seat;frame[F.walker]=+!!unit?.walker;frame[F.soldier]=+!!unit?.soldier;
  frame[F.noEnter]=Number.parseFloat(unit?.classDef?.NoEnterVehicles??0);frame[F.person]=+!!person;frame[F.air]=+!!unit?.soldier?.air;return !!run(2);
}
export function boardingSeat(vehicle,{team=0,person=false,relation=teamRelation,preferred=-1}={}){
  refresh();const seats=vehicle?.seats;let count=0;
  if(seats)for(let i=0;i<seats.length&&i<128;i++){
    const lease=seats[i].lease,at=count++*R.words;
    rows[at+R.lease]=+!!lease;rows[at+R.released]=+!!lease?.released;rows[at+R.relation]=lease?relation(team,lease.team):0;
    rows[at+R.free]=+!!vehicle.seatFree(i);rows[at+R.person]=+personLease(lease);
  }
  frame[F.alive]=+!!vehicle?.alive;frame[F.team]=team;frame[F.person]=+!!person;frame[F.own]=vehicle?.controllingTeam??NaN;frame[F.preferred]=preferred;frame[F.count]=count;
  return run(3);
}

// Makes seat `index` his: a bot holding it moves on to the next free seat,
// or gets out (Vehicles.displace). Whether the seat is free now.
// What a soldier's use key (E) gets him into: the nearest vehicle in reach
// ahead of him that he may board (Vehicles.enterable), else the turret he
// faces (LiveObjects.enterable). Null for nothing, or when he may not board.
// world: { vehicles, live }.
export function mountInReach(world, unit, person = personUnit(unit)) {
  return boardingReach.mount(world, unit, person);
}

// The same, on an object the simulation's capture can watch
// (tools/sim-capture/seats.mjs): a module's own functions cannot be.
export const boardingReach = { mount };

function mount(world, unit, person) {
  if (!canBoard(unit, person)) return null;
  const s=unit.soldier;refresh();frame[F.position]=s.position.x;frame[F.position+1]=s.position.y;frame[F.position+2]=s.position.z;frame[F.yaw]=s.aimYaw;frame[F.pitch]=s.aimPitch;run(7);
  eye.set(frame[F.eye],frame[F.eye+1],frame[F.eye+2]);aim.set(frame[F.aim],frame[F.aim+1],frame[F.aim+2]);
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
  leaseFacts(turret?.lease);frame[F.gunner]=+!!turret?.gunner;return !!run(4);
}

// Whether the one holding `lease` may get out now. A person may at any time:
// in flight he bails out beside the hull and falls (the 2004 game's seat
// exit has no airborne test). A bot stays aboard a craft in flight (the
// Classic AI never bails out of one).
export function mayLeave(vehicle, lease) {
  leaseFacts(lease);vehicleFacts(vehicle);return !!run(5);
}

// A rider killed on his seat leaves it at once, his body falling from where
// it rode. The 2004 game's death in a seat runs the seat exit without its
// search for a spot beside the hull: the body stays at the seat, at rest,
// among soldiers again (a corpse rounds still meet),
// and while the hull lives the last bot aboard takes the seat over, as after
// any exit. The hull goes on without him: a hover slows to a stop at its
// Deceleration, a craft in flight holds its course (the native flight solver),
// and anyone may take it. He isn't flung off: the 2004 game throws a dead
// rider (9 m/s along the round, lifted) only from a seat whose class sets
// hash 81bce0f9 to 0, which no retail class does. A turret's gunner leaves
// his gun the same way (the armed building seats its pilot through the same
// code), and the turret waits for whoever comes next. For a bot or a network
// human (Vehicles.vacate, TurretSeats.vacate) and the player
// (page view release) alike. Returns whether he held a
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

// What a seat's keys and mouse do to the hull: the pilot of a craft flies it
// ("flight"), the driver of anything else drives it ("drive"), and every
// other seat ("turret") only aims and fires its own guns. Read for the seat
// held when the keys are read, never kept from before a seat change.
export function seatControl(vehicle, index) {
  vehicleFacts(vehicle);frame[F.preferred]=index;frame[F.count]=vehicle?.seats?.length??0;return ['flight','drive','turret'][run(6)]??null;
}

// The key map a seat reads (which keys queue which actions, and which are
// held for fire): the flight bindings in every seat of a craft (its seat,
// use and fire keys among them), else the soldier's (null).
export const seatScheme = (vehicle, bindings) => (vehicle?.flying ? flightBindings(bindings) : null);

// Mouse counts on a seat: a craft's pilot moves its virtual stick (the
// flight sensitivity and invert); any other seat turns its aim, held inside
// the seat's limits (the mouse scale and invert).
export function seatLook(vehicle, index, dx, dy, settings) {
  vehicle.system.f2.seats.look(vehicle, index, dx, dy, settings);
}
