import { createSpawnScreen } from "./index.js";

// The spawn screen on demand over the running game, for checks and
// screenshots, whatever the player is doing:
//   const { livePort } = await import("/tests/spawn-screen-fake-port.js");
//   const { spawnScreenDebug } = await import("/src/spawn-screen/debug.js");
//   const screen = await spawnScreenDebug(__bf, await livePort(__bf));
// options go to createSpawnScreen (clock: "manual" to step it by hand).
export async function spawnScreenDebug(bf, port, options = {}) {
  const common = await fetch("/common/common.json").then((response) => response.json());
  const screen = createSpawnScreen({ port, hud: bf.hud, controls: bf.controls, menu: bf.menu, settings: bf.settings, common, ...options });
  if (!screen.isOpen) screen.open("debug");
  return screen;
}
