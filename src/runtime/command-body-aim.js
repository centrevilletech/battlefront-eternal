// Droideka mouse counts are consumed by shared tick(), including its fold/
// roll rules. Fire samples that predicted pose, just as mounted fire samples
// its current servo pose; it must not mutate counts or invent an unticked aim.
export function commandBodyAim(soldier, walker, preview, command, zoom, scale, invert, combat=null) {
  preview.begin(soldier,combat);
  if(!walker&&command.look)preview.append(command.look,zoom,command.mouseScale??scale,command.invertMouse??invert);
  return preview.run();
}
