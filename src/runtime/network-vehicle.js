import { seatInput } from '../boarding.js';
const NO_ASSIST = { assist: false };

// Network humans send input into the native lease owner before latching.
// Returning the decoded keys preserves the command API used by replay tools.
export function networkVehicleInput(vehicle, index, controls, command) {
  return networkVehicle.input(vehicle, index, controls, command);
}

export const networkVehicle = {
  input(vehicle, index, controls, command) {
    vehicle.system.f2.seats.network(vehicle, index, controls, command);
    return seatInput(vehicle, index, controls, command, NO_ASSIST);
  },
};
