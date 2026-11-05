// Turret construction facts for the native crew owner. Resolving node indices
// here once keeps scene-node traversal off the world phase path.
export function turretSettings(t) {
  const bones = t.rig.bones, at = node => node ? bones.indexOf(node) : null;
  return { facing: t.facing, position: t.position, matrix: t.matrix, pitchLimits: t.pitchLimits, yawLimits: t.yawLimits, wrapYaw: t.wrapYaw,
    gunPitchLimits: t.gunPitchLimits, gunYawLimits: t.gunYawLimits, turnRate: t.turnRate, filter: t.filter, gunPitchRate: t.gunPitchRate,
    gunYawRate: t.gunYawRate, yawFactor: t.yawFactor, pitchFactor: t.pitchFactor, cameraEye: t.camera.eye, cameraCenter: t.camera.center,
    cameraOffset: t.camera.offset, cameraTilt: t.camera.tilt, pivot: t.pivot, up: t.up, drumParentTurn: t.drumParentTurn, gunBind: t.gunBind,
    root: t.rig.root.matrix, parents: bones.map(b => bones.indexOf(b.parent)), drum: at(t.drum), gun: at(t.gun), seat: at(t.seat), firePoints: t.firePoints.map(at) };
}
export function turretState(t, index) {
  return { index, yaw: t.yaw, pitch: t.pitch, yawVel: t.yawVel, pitchVel: t.pitchVel, gunYaw: t.gunYaw, gunPitch: t.gunPitch, barrel: t.barrel,
    rootWorld: t.rig.root.matrixWorld, bones: t.rig.bones.map(b => ({ p: b.position, q: b.quaternion, s: b.scale, world: b.matrixWorld })) };
}
