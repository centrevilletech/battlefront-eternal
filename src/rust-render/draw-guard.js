// All scenes draw through Rust. Keep the browser entry point guarded even in
// the front end, so an unported screen cannot silently compile and draw itself.
const interfaces = new WeakMap();
const guarded = new WeakSet();
const screens = new Set(["interface", "classic-spawn", "spawn-room", "spawn-holomap", "spawn-stage", "spawn-logo", "classic-scoreboard"]);

export function requireRustDraws(renderer) {
  if (guarded.has(renderer)) return;
  guarded.add(renderer);
  renderer.battleThreeCalls = 0;
  renderer.battleCompileCalls = 0;
  // Source ownership is complete even for helpers created during play. Both
  // compiler entry points must fail before Three can reflect a battle program.
  const compile = () => {
    renderer.battleCompileCalls++;
    throw new Error("Battle attempted a three.js compile; an owned program is required");
  };
  renderer.compile = renderer.compileAsync = compile;
  renderer.render = () => {
    renderer.battleThreeCalls++;
    throw new Error("Battle attempted a three.js draw; a Rust pass is required");
  };
}

export function bindInterfaceRenderer(renderer, pass) {
  interfaces.set(renderer, pass);
}

// Named surfaces retain their layer API while submitting only Rust draw records.
export function renderInterface(renderer, scene, camera, screen) {
  if (!screens.has(screen)) throw new Error(`Unknown interface screen: ${screen}`);
  const pass = interfaces.get(renderer);
  if (!pass) throw new Error("Rust interface renderer is not initialized");
  return pass.render(scene, camera);
}
