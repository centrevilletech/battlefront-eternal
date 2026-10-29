import { seatControl, seatInput, seatLook } from '../boarding.js';

// Shared by authoritative network humans and local replay. Only input reaches
// the server: the resulting hull pose remains entirely simulation-owned. The
// seat's mouse and keys are the player's (src/boarding.js), with the mouse
// scales bounded here.
export function networkVehicleInput(vehicle, index, controls, command) {
  const fs = command.flightSettings || {};
  const look = { scale: Math.min(2, command.mouseScale ?? 1), invert: Boolean(command.invertMouse),
    flightScale: Math.max(.05, Math.min(10, fs.sensitivity ?? 1)), flightInvert: Boolean(fs.invert) };
  for (const [dx, dy] of command.look) seatLook(vehicle, index, dx, dy, look);
  if (seatControl(vehicle, index) !== 'flight') {
    // Mouse aim is direct, as in Vehicle.aimPlayerSeat; hull steering has its own limits.
    const seat = vehicle.seats[index];
    seat.aimYaw = seat.lookYaw ?? seat.aimYaw;
    seat.pitch = seat.lookPitch ?? seat.pitch;
    vehicle.clampAim(index);
  }
  // Remote targets arrive at different times on the server and client.
  // Their presence must not change the gain used to replay pilot controls.
  // Weapon assist and target locking remain separate authoritative systems.
  return seatInput(vehicle, index, controls, command, { assist: false });
}
