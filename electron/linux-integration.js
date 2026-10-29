// Linux: register the AppImage with the desktop on every launch, so app
// launchers (Omarchy's included) list Battlefront Eternal and `battlefront`
// starts it from a terminal. Only what is missing or stale is rewritten, and
// a `battlefront` command this app did not create is left alone.
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";

export const DISPLAY_NAME = "Battlefront Eternal";
const ID = "battlefront-eternal";
const MARKER = "# Created by Battlefront Eternal";

export function desktopEntry(executable) {
  const exec = `"${executable.replace(/(["`$\\])/g, "\\$1")}"`;
  return [
    "[Desktop Entry]",
    "Type=Application",
    `Name=${DISPLAY_NAME}`,
    "GenericName=Star Wars Battlefront",
    "Comment=Star Wars: Battlefront (2004) on a new engine",
    `Exec=${exec} %U`,
    `Icon=${ID}`,
    "Terminal=false",
    "Categories=Game;ActionGame;",
    "Keywords=battlefront;star wars;swbf;bfe;",
    `StartupWMClass=${ID}`,
    "X-AppImage-Integrate=false",
    "",
  ].join("\n");
}

// The newest update the app installed in the user's folder starts directly
// (electron/update/state.js keeps `current` pointing at it), without the
// AppImage unpacking first; without user namespaces it runs as the
// AppImage's own launcher would, unsandboxed.
const CURRENT = 'current="${XDG_DATA_HOME:-$HOME/.local/share}/battlefront-eternal/current/battlefront-eternal"';
const RUN_CURRENT = ['if [ -x "$current" ]; then', '  unshare -Ur true 2>/dev/null || set -- --no-sandbox "$@"', '  exec "$current" "$@"', "fi"];

export function launcherScript(executable) {
  return [`#!/bin/sh`, `${MARKER}; it follows the AppImage when it moves.`, CURRENT, ...RUN_CURRENT, `exec '${executable.replace(/'/g, "'\\''")}' "$@"`, ""].join("\n");
}

// The launcher get.battlefronteternal.com/install.sh writes runs the
// AppImage unpacked (for FUSE 3-only systems such as Omarchy), which
// unpacks the whole app on every start; this one runs the installed update
// when there is one. `command` is the installer's own quoting of the
// AppImage path.
const INSTALLER_LAUNCHER = /^#!\/usr\/bin\/env bash\nexport BFE_NO_DESKTOP_INTEGRATION=1\nexec (.+) --appimage-extract-and-run "\$@"\n$/;
const UPDATING_MARKER = "# Battlefront Eternal: the newest installed update, else the AppImage.";
export function updatingLauncher(command) {
  return ["#!/usr/bin/env bash", UPDATING_MARKER, "export BFE_NO_DESKTOP_INTEGRATION=1", CURRENT, ...RUN_CURRENT, `exec ${command} --appimage-extract-and-run "$@"`, ""].join("\n");
}

export async function adoptInstallerLauncher({ home = os.homedir() } = {}) {
  const file = path.join(home, ".local", "bin", "battlefront-eternal");
  const existing = await fs.readFile(file, "utf8").catch(() => null);
  if (existing === null) return "missing";
  const command = INSTALLER_LAUNCHER.exec(existing)?.[1] ?? (existing.includes(UPDATING_MARKER) ? /\nexec (.+) --appimage-extract-and-run "\$@"\n$/.exec(existing)?.[1] : null);
  if (!command) return "foreign";
  return (await writeIfChanged(file, updatingLauncher(command), 0o755)) ? "updated" : "current";
}

async function writeIfChanged(file, content, mode) {
  const current = await fs.readFile(file).catch(() => null);
  const next = Buffer.isBuffer(content) ? content : Buffer.from(content);
  if (current && current.equals(next)) return false;
  await fs.mkdir(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  await fs.writeFile(temp, next, { mode });
  await fs.rename(temp, file);
  return true;
}

export async function integrateLinuxDesktop({ executable, icon, home = os.homedir(), env = process.env } = {}) {
  if (!executable || env.BFE_NO_DESKTOP_INTEGRATION) return { skipped: true };
  const data = env.XDG_DATA_HOME || path.join(home, ".local", "share");
  const applications = path.join(data, "applications");
  const result = {};

  result.icon = await writeIfChanged(path.join(data, "icons", "hicolor", "512x512", "apps", `${ID}.png`), await fs.readFile(icon));
  result.entry = await writeIfChanged(path.join(applications, `${ID}.desktop`), desktopEntry(executable), 0o755);

  const command = path.join(home, ".local", "bin", "battlefront");
  const existing = await fs.readFile(command, "utf8").catch(() => null);
  result.command = existing === null || existing.includes(MARKER) ? await writeIfChanged(command, launcherScript(executable), 0o755) : "foreign";

  if (result.entry) execFile("update-desktop-database", [applications], () => {});
  if (result.icon) execFile("gtk-update-icon-cache", ["-q", "-t", path.join(data, "icons", "hicolor")], () => {});
  return result;
}
