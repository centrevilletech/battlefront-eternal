// A delayed trigger names the direction seen when it was pressed. Apply it to
// that shot before weapon spread/kick, never rewind the soldier's current view
// or replay mouse motion already included in the absolute aim sample.
export function networkShotFor(owner, channel = 0) {
  return owner?.networkShots ? owner.networkShots.find(edge=>edge.button===channel) ?? null : owner?.networkShot ?? null;
}
// The same press-time aim as an eye-ray target, for a gun that converges
// on the aim point (a droideka's offset barrels): move `target` onto the
// ray from `eye` the press saw, keeping its reach, and let the caller
// converge. Overwriting the converged direction with the body aim (as
// infantry takes it, their edge already converged) would fire parallel
// to the eye ray from an offset barrel, off the reticle at every range.
export function networkShotTarget(owner, eye, target, channel = 0) {
  const shot=networkShotFor(owner,channel);
  if(!owner?.networkHuman || owner.seat || !Number.isFinite(shot?.aimYaw) || !Number.isFinite(shot?.aimPitch))return target;
  const reach=target.distanceTo(eye);
  if(!Number.isFinite(reach)||reach<=0)return target;
  const pitch=shot.aimPitch,yaw=shot.aimYaw,cos=Math.cos(pitch);
  return target.set(eye.x+Math.sin(yaw)*cos*reach,eye.y+Math.sin(pitch)*reach,eye.z+Math.cos(yaw)*cos*reach);
}
export function networkShotDirection(owner, direction, channel = 0) {
  const shot=networkShotFor(owner,channel);
  if(!owner?.networkHuman || owner.seat || !Number.isFinite(shot?.aimYaw) || !Number.isFinite(shot?.aimPitch))return direction;
  const pitch=shot.aimPitch,yaw=shot.aimYaw,cos=Math.cos(pitch);
  return direction.set(Math.sin(yaw)*cos,Math.sin(pitch),Math.cos(yaw)*cos);
}
