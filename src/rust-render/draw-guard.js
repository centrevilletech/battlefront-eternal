// Three still draws interface scenes that have no Rust replacement. Battle
// code must use a Rust pass; accidental three draws fail at their call site.
const draws = new WeakMap();
const screens = new Set(["interface", "classic-spawn", "spawn-room", "spawn-holomap", "spawn-stage", "spawn-logo", "classic-scoreboard"]);

export function requireRustDraws(renderer) {
  if (draws.has(renderer)) return;
  draws.set(renderer, renderer.render.bind(renderer));
  renderer.battleThreeCalls = 0;
  renderer.render = () => {
    renderer.battleThreeCalls++;
    throw new Error("Battle attempted a three.js draw; a Rust pass is required");
  };
}

// Only these call sites own separate UI scenes. Settling the target first
// also settles cached GL state left by a deferred Rust battle/HUD phase.
export function renderInterface(renderer, scene, camera, screen) {
  if (!screens.has(screen)) throw new Error(`Unknown interface screen: ${screen}`);
  // Enter the resource owner through its state-settling wrapper before a
  // permitted interface draw, including one immediately after the Rust HUD.
  renderer.setRenderTarget(renderer.getRenderTarget());
  return (draws.get(renderer) || renderer.render.bind(renderer))(scene, camera);
}
