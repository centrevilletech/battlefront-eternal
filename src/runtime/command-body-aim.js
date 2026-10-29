// Droideka mouse counts are consumed by shared tick(), including its fold/
// roll rules. Fire samples that predicted pose, just as mounted fire samples
// its current servo pose; it must not mutate counts or invent an unticked aim.
export function commandBodyAim(soldier, walker, preview, command, zoom, scale, invert) {
  if (walker) return { aimYaw: soldier.aimYaw, aimPitch: soldier.aimPitch };
  preview.aimYaw = soldier.aimYaw;
  preview.aimPitch = soldier.aimPitch;
  for (const [dx, dy] of command.look || []) preview.lookRaw(dx / zoom, dy / zoom, command.mouseScale ?? scale, command.invertMouse ?? invert);
  return { aimYaw: preview.aimYaw, aimPitch: preview.aimPitch };
}
