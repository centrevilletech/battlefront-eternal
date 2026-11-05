// Authenticated seat cycling is a scalar command. Its touched lease mirror
// is published before this returns, including any bot displaced by the move.
export function cycleNetworkSeat(system, unit, direction = 1) {
  return networkSeat.cycle(system, unit, direction);
}

export const networkSeat = {
  cycle(system, unit, direction = 1) { return system.f2.seats.cycle(unit, direction); },
};
