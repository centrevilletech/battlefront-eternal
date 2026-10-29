// The player's settings (src/settings.js: the profile name, options,
// bindings), kept in the user data folder by the main process rather than
// in the page's localStorage alone.
//
// localStorage is Chromium's, and Chromium keeps one process's lock on the
// user data folder: a second process sharing it gets an empty, in-memory
// localStorage. An update installed while the game runs starts the new
// version beside the old one (electron/update/index.js: launch), which
// waits for its window, and under Steam stays until the game ends. That
// new version found no settings (the profile back to PLAYER 1) and lost
// any it saved (reported by a tester). A file the main process reads and writes
// has no such lock.
//
// The record: { complete, values }. complete: values are every setting,
// and the file is the settings. A session that cannot read localStorage
// before the file exists (shadowed: started by an update) writes only
// what the player changed (complete false), and the next session lays
// that over localStorage's settings and completes the file.
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import path from "node:path";

export const PLAYER_SETTINGS_FILE = "player-settings.json";
const LIMIT = 256 * 1024;

// A record as stored, or null for anything else.
export function cleanRecord(record) {
  if (!record || typeof record !== "object" || Array.isArray(record)) return null;
  const { complete, values } = record;
  if (typeof complete !== "boolean" || !values || typeof values !== "object" || Array.isArray(values)) return null;
  try {
    const text = JSON.stringify({ complete, values });
    return text.length <= LIMIT ? JSON.parse(text) : null;
  } catch {
    return null;
  }
}

export function readPlayerSettings(dir) {
  try {
    return cleanRecord(JSON.parse(readFileSync(path.join(dir, PLAYER_SETTINGS_FILE), "utf8")));
  } catch {
    return null;
  }
}

// Through a file of its own, then renamed over: a crash mid-write leaves
// the previous settings. False when the record is malformed or unwritable.
export function writePlayerSettings(dir, record) {
  const clean = cleanRecord(record);
  if (!clean) return false;
  const file = path.join(dir, PLAYER_SETTINGS_FILE), temp = `${file}.${process.pid}.tmp`;
  try {
    mkdirSync(dir, { recursive: true });
    writeFileSync(temp, JSON.stringify(clean));
    renameSync(temp, file);
    return true;
  } catch {
    return false;
  }
}

// Whether this process shares the user data folder with a running older
// version: one an update started (electron/update/index.js sets
// BFE_UPDATED_FROM). The boot hand-off's parent waits before Electron is
// ready (electron/boot.js), so it holds no lock.
export const shadowedStorage = (env = process.env) => !!env.BFE_UPDATED_FROM;
