// Shared by authoritative combat and immediate, cosmetic firing feedback.
export function infantryFireBlocked(soldier, gunUnderwater, stick = 0) {
  return Boolean(soldier && ((soldier.air && soldier.air !== 'jet') ||
    soldier.dive || soldier.lock > 0 || soldier.rolled ||
    (soldier.posture === 'prone' && stick > 0.1)) || gunUnderwater);
}

// Direction toward a damage source in the same view convention as the HUD.
export function damageDirection(x, z, yaw) {
  return Math.atan2(-x * Math.cos(yaw) + z * Math.sin(yaw),
    x * Math.sin(yaw) + z * Math.cos(yaw));
}
