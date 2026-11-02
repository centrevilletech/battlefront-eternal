// The transparent pass (transparent.rs): what the near scene blends (the
// map's blended buckets, holograms, sprites, basic materials), in three's
// back-to-front order, which it must own to keep that order. Its phase runs
// from a point in three's transparent list (core.anchor). Empty for now:
// three draws all of it.
export function transparentPass() {
  return { name: "transparent", phase: "transparent" };
}
