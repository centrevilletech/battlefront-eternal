import { switchSeat } from '../boarding.js';

// Cycle an authenticated occupant through available seats, by the same seat
// switch the player's keys make (src/boarding.js switchSeat). A bot may swap
// into the vacated seat; another human keeps their seat and controls.
export function cycleNetworkSeat(system, unit, direction = 1) {
  const lease = unit?.seat, vehicle = lease?.vehicle;
  if (!unit?.networkHuman || !vehicle?.alive || lease.released) return false;
  return switchSeat(system, vehicle, lease, lease.index, direction) >= 0;
}
