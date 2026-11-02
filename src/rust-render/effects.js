// The effects pass (effects.rs): bolts, muzzle flashes, the effects-gpu
// particles, debris, lightning, the sniper's trace, jetpack flames and the
// droideka's shield, drawn in the transparent phase after the transparent
// pass. Their simulation stays here in the page. Empty for now: three draws
// them.
export function effectsPass() {
  return { name: "effects", phase: "transparent" };
}
