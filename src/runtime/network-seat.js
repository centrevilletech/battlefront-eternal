import { switchSeat } from '../boarding.js';

// Cycle an authenticated occupant through available seats, by the same seat
// switch the player's keys make (src/boarding.js switchSeat). A bot may swap
// into the vacated seat; another human keeps their seat and controls.
export function cycleNetworkSeat(system, unit, direction = 1) {
  return networkSeat.cycle(system, unit, direction);
}

// The same, on an object the simulation's capture can watch
// (tools/sim-capture/seats.mjs): a module's own functions cannot be.
export const networkSeat = {
  cycle(system, unit, direction = 1) {
    if (system?.f2?.seats) return system.f2.seats.cycle(unit, direction);
    if (system?.networkSeatsSim) return system.networkSeatsSim.cycle(system, unit, direction);
    const lease = unit?.seat, vehicle = lease?.vehicle;
    if (!unit?.networkHuman || !vehicle?.alive || lease.released) return false;
    return switchSeat(system, vehicle, lease, lease.index, direction) >= 0;
  },
};
