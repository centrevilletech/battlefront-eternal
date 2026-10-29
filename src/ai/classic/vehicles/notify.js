import * as THREE from "three";

// The vehicle notifier (docs/ai-vehicles.md).
//
// It runs at the controller cycle's wrap (the scheduler calls it). With G
// the AI cycles per second (2 on Easy and Medium, 10 on Hard, 30 before a
// difficulty is set) there are N = 3G buckets; a persistent counter, N at
// first, goes 0, 1, ..., N and round again, so one pass in N + 1 finds an
// empty bucket. A boardable thing (a vehicle or an armed building) whose id
// mod N is the counter is considered:
//
//   - not when every seat is taken;
//   - not when it moves faster than 3 m/s (speed^2 > 9);
//   - a vehicle (not a building) only where soldiers can reach it: its spot
//     clear of the planning barriers at a 2 m radius, and at its own
//     traversal radius (provisional reading of the two barrier tests);
//   - then the units within the notify radius (50 m, or the mission's
//     SetAIVehicleNotifyRadius), at most 30, friendly or neutral to it, are
//     each told about it when: driven by an AI controller, their class may
//     enter vehicles (not NoEnterVehicles), a vehicle that belongs to a
//     team is that team's (a vehicle a player drives is open to its allies
//     and neutrals regardless), and their own team is not low on
//     reinforcements (25 or fewer, teams 1 and 2).
//
// A told controller appends the vehicle to its candidates (at most 10 per
// service; the boarding interrupt, priority 6, picks one at random).

export class VehicleNotifier {
  constructor() {
    this.counter = null; // persistent, N before the first pass
    this.found = [];
  }

  // vehicles: the VehicleAI service (index.js).
  run(vehicles) {
    const G = vehicles.cyclesPerSecond();
    const N = 3 * G;
    if (this.counter === null) this.counter = N;
    this.counter = this.counter === N ? 0 : this.counter + 1;
    let told = 0;
    for (const entry of vehicles.boardables()) {
      if (N > 0 && entry.id % N !== this.counter) continue;
      told += this.consider(vehicles, entry);
    }
    return told;
  }

  consider(vehicles, entry) {
    if (!entry.alive) return 0;
    if (vehicles.seatCount(entry) <= vehicles.occupiedSeats(entry)) return 0;
    const v = entry.velocity;
    if (v && v.x * v.x + v.y * v.y + v.z * v.z > 9) return 0;
    if (entry.vehicle && !vehicles.reachable(entry)) return 0;
    const world = vehicles.world;
    const units = world.units;
    if (!units?.query) return 0;
    const radius = vehicles.notifyRadius();
    const team = entry.team;
    const found = units.query(entry.position, radius, this.found, (u) => u !== entry && relation(world, team, u.team) >= 0);
    if (found.length > 30) found.length = 30;
    const humanPilot = vehicles.humanAboard(entry);
    let told = 0;
    for (const unit of found) {
      const controller = unit.controller;
      if (!controller?.isUnitController || !unit.soldier || unit.seat) continue;
      if (Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0) !== 0) continue;
      if (team > 0 && !(relation(world, team, unit.team) > 0) && !humanPilot) continue;
      if (vehicles.lowTickets(unit.team)) continue;
      controller.addVehicleNote?.({ unit: entry, generation: entry.generation });
      told += 1;
    }
    return told;
  }
}

function relation(world, a, b) {
  if (world.relation) return world.relation(a, b);
  if (world.units?.relation) return world.units.relation(a, b);
  if (a === b) return a ? 1 : 0;
  return !a || !b ? 0 : -1;
}

export { THREE };
