// The far pass (far.rs): the far scene, drawn into the far target before
// the near scene: the sky dome and its layers, the map's low-detail
// buckets, the far terrain and the far water, read with the far camera's
// frame block (VIEW_FAR). Empty for now: three draws all of it.
export function farPass() {
  return { name: "far", phase: "far" };
}
