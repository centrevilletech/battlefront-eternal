import { Turret } from '../props.js';

// Turret.steer, used only on the page's private look-preview scratch. The
// native crew phase owns the actual turret and never calls this function.
export function stepTurretPreview(state, dt, turnInput, pitchInput, zoom = 1) {
  const k = Math.min(1, state.filter * dt), rate = state.turnRate / zoom;
  state.yawVel += (turnInput * rate - state.yawVel) * k;
  state.pitchVel += (pitchInput * rate - state.pitchVel) * k;
  const yaw = state.yaw + state.yawVel * dt;
  state.yaw = state.yawLimits ? Turret.prototype.holdYaw.call(state, yaw) : Math.atan2(Math.sin(yaw), Math.cos(yaw));
  // Quirk: the drum's velocity becomes zero when the arc holds its yaw.
  if (state.yawLimits && Math.abs(Math.atan2(Math.sin(yaw - state.yaw), Math.cos(yaw - state.yaw))) > 1e-9) state.yawVel = 0;
  state.pitch = Math.max(state.pitchLimits[0], Math.min(state.pitchLimits[1], state.pitch + state.pitchVel * dt));
}
