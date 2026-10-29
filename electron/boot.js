// The desktop client's entry point. Before anything else loads, a start
// from outside Steam goes through the player's Steam library when the
// player added the game there (electron/steam/relaunch.js), and a newer
// version installed by the updater starts in this one's place
// (electron/update/index.js: handOff); otherwise the app itself (main.js).
// The await keeps Electron's ready event until main.js has run. Nothing
// about Steam or updates may stop this executable from starting itself.
// Slow links first: every connection this process makes waits long enough
// on each address (slow-links.js).
import "./slow-links.js";
import { app } from "electron";
import path from "node:path";
import { handOff, startupFailed, startupGuard, updateLog } from "./update/index.js";
import { updateRoot } from "./update/state.js";

let handedOff = false, throughSteam = false, stay = false;
try {
  const { canRestartSelf, startedBySteam, steamStays, withoutSteamOverlay } = await import("./steam/launcher.js");
  // Started by Steam on Linux: again in place, without Steam's overlay.
  const clean = withoutSteamOverlay();
  if (clean && typeof process.execve === "function" && canRestartSelf()) process.execve(process.execPath, process.argv, clean);
  const { arrived, readSteamSettings, relaunchThroughSteam } = await import("./steam/relaunch.js");
  const bySteam = startedBySteam();
  stay = steamStays();
  const root = updateRoot(), dir = root && path.join(root, "launches");
  const steam = bySteam || !app.isPackaged ? null : readSteamSettings(app.getPath("userData"));
  if (bySteam) arrived({ dir });
  // Already running: that copy comes forward (main.js: second-instance)
  // without a round trip through Steam.
  else if (steam?.appid && steam.launch !== false && app.requestSingleInstanceLock()) {
    app.releaseSingleInstanceLock();
    const log = updateLog(root);
    throughSteam = await relaunchThroughSteam({ dir, steam, log: (line) => log(`steam: ${line}`) });
  }
} catch (error) {
  console.warn("start through Steam failed:", error?.message ?? error);
}
try {
  // Steam watches the process it started: where it cannot follow the
  // version handed off to, that one stays until the game ends.
  if (!throughSteam) handedOff = await handOff({ app, stay });
} catch (error) {
  console.warn("update hand-off failed:", error?.message ?? error);
}
if (throughSteam) app.exit(0);
else if (handedOff?.exited) app.exit(await handedOff.exited);
else if (handedOff) app.exit(0);
else {
  // Until its window is up, a version that fails exits instead of showing
  // Electron's error dialog, so whatever started it carries on instead.
  startupGuard({ app });
  try {
    await import("./main.js");
  } catch (error) {
    if (!startupFailed(error)) throw error;
  }
}
