import * as THREE from 'three';
import { seatControl, seatInput } from '../boarding.js';

// Private page replay of networkVehicleInput. It only changes the prediction
// clone; the authority applies the command in the native lease owner.
export function predictionVehicleInput(vehicle, index, controls, command) {
    const fs = command.flightSettings || {};
    const look = { scale: Math.min(2, command.mouseScale ?? 1), invert: Boolean(command.invertMouse),
      flightScale: Math.max(.05, Math.min(10, fs.sensitivity ?? 1)), flightInvert: Boolean(fs.invert) };
    for (const [dx, dy] of command.look) predictionSeatLook(vehicle, index, dx, dy, look);
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

export function predictionSeatLook(vehicle, index, dx, dy, { scale = 1, invert = false, flightScale = 1, flightInvert = false } = {}) {
  if (seatControl(vehicle, index) === "flight") {
    const stick = vehicle;
    stick.stickX = THREE.MathUtils.clamp(stick.stickX + (dx * flightScale) / 120, -1, 1);
    stick.stickY = THREE.MathUtils.clamp(stick.stickY + ((flightInvert ? -dy : dy) * flightScale) / 120, -1, 1);
    const radius = Math.max(1, Math.hypot(stick.stickX, stick.stickY));
    stick.stickX /= radius;
    stick.stickY /= radius;
    return;
  }
  const seat = vehicle.seats[index];
  seat.lookYaw = (seat.lookYaw ?? seat.aimYaw) - (dx * scale) / 2048;
  seat.lookPitch = (seat.lookPitch ?? seat.pitch) + ((invert ? dy : -dy) * scale) / 2048;
  vehicle.clampAim(index);
}
