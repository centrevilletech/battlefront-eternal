// The Steam overlay (Shift+Tab: friends, chat, the web browser) over this
// game, where it can work (docs/presence.md, Overlays).
//
// Steam loads its overlay into the process it starts, and the overlay draws
// when that process presents a frame. Electron presents frames from a GPU
// process of its own, so the overlay never sees one; with Chromium's GPU
// work in the main process (in-process-gpu) and DirectComposition off, it
// does. Windows only, and only when the player turns it on: Electron 25 and
// later have shown the overlay as a white rectangle, and the GPU sandbox and
// GPU crash recovery are given up for it.
//
// Linux has no overlay here. On Wayland, Steam's overlay cannot hook a
// native Wayland window at all; on X11 it would draw, but it reads the
// keyboard only through Xlib, and Chromium reads it through XInput2 over
// XCB, so Shift+Tab never reaches it (checked against Valve's own
// gameoverlayrenderer.so). The library is removed at start instead
// (launcher.js: withoutSteamOverlay).
import { startedBySteam } from "./launcher.js";

export const OVERLAY_SWITCHES = [["in-process-gpu"], ["disable-direct-composition"]];

// Whether the overlay is supported here, why not where not.
export function overlaySupport({ platform = process.platform, env = process.env } = {}) {
  if (platform === "win32") return { supported: true };
  if (platform !== "linux") return { supported: false, reason: "platform" };
  return { supported: false, reason: env.WAYLAND_DISPLAY ? "wayland" : "x11" };
}

// The Chromium switches this run takes for the overlay: when Steam started
// it on Windows and the player turned the overlay on (the steam record's
// `overlay`). [] otherwise.
export function overlaySwitches({ steam, platform = process.platform, env = process.env, argv = process.argv } = {}) {
  if (!overlaySupport({ platform, env }).supported || steam?.overlay !== true) return [];
  return startedBySteam({ argv, env }) ? OVERLAY_SWITCHES : [];
}

// Whether Steam's overlay library is loaded in this process (Node's
// diagnostic report lists the loaded libraries): what the options page shows
// as the overlay's state when Steam started the game.
export function overlayLoaded(report = () => process.report?.getReport?.()) {
  try {
    const objects = report()?.sharedObjects;
    return Array.isArray(objects) && objects.some((file) => /gameoverlayrenderer(64)?\.(dll|so)$/i.test(String(file)));
  } catch {
    return false;
  }
}
