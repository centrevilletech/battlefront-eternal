// The foliage pass (foliage.rs): the near scene's foliage (foliage.js,
// foliage-gpu.js), drawn in the near phase. Empty for now: three draws it.
export function foliagePass() {
  return { name: "foliage", phase: "near" };
}
