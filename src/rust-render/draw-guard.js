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

export function renderInterface(renderer, scene, camera, screen) {
  if (!screens.has(screen)) throw new Error(`Unknown interface screen: ${screen}`);
  return (draws.get(renderer) || renderer.render.bind(renderer))(scene, camera);
}
