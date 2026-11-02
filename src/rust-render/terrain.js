// The terrain pass (terrain.rs): the near scene's terrain patches and their
// painted layers, drawn in the near phase. Empty for now: three draws them.
export function terrainPass() {
  return { name: "terrain", phase: "near" };
}
