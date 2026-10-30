// Battlefront Eternal desktop client: the Electron main process.
//
// The renderer is the same page the browser development build serves, loaded
// from app://bfe/ (electron/content.js) with the sandbox on, context isolation
// and no Node. The only bridge is electron/preload.cjs. Game data is prepared
// from the player's own copy into a folder outside the app (docs/desktop.md).
import { app, BrowserWindow, dialog, ipcMain, Menu, net, protocol, session, shell } from "electron";
import { existsSync, promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createHandler, DESKTOP_PREFIX, HOST, ORIGIN, SCHEME } from "./content.js";
import { BF1_GOG_PAGE, BF1_STEAM_APP, findContent, findGameInstall, inspectInstall, installedAddOns } from "./discover.js";
import { exportContent, importContent, relocatePreparation } from "./content-archive.js";
import { adoptInstallerLauncher, integrateLinuxDesktop } from "./linux-integration.js";
import { createNetworkBridge, loadNetworkIdentity } from "./network.js";
import { createNetHost } from "./net-host.js";
import { createHostController } from "./host.js";
import { CONTENT_VERSION, preparedContentCurrent, Preparer, readPrepareState, toolchainPaths } from "./prepare.js";
import { startupContent } from "./startup-content.js";
import { createUpdater } from "./update/index.js";
import { createPresence } from "./presence/index.js";
import { DiscordPresence } from "./presence/discord.js";
import { DISCORD_APPLICATION_ID, DISCORD_LARGE_IMAGE } from "./presence/application.js";
import { startedBySteam, steamLost, steamStays } from "./steam/launcher.js";
import { createSteamIntegration } from "./steam/index.js";
import { overlayLoaded, overlaySupport, overlaySwitches } from "./steam/overlay.js";
import { readSteamSettings } from "./steam/relaunch.js";
import { readPlayerSettings, shadowedStorage, writePlayerSettings } from "./player-settings.js";

function argument(name) {
  const prefix = `--${name}=`;
  const found = process.argv.find((arg) => arg.startsWith(prefix));
  return found ? found.slice(prefix.length) : null;
}

const here = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(here, "..");
// The Vite build (npm run build:desktop), packaged alongside this file.
const appRoot = path.join(projectRoot, "dist-desktop");
const uiRoot = path.join(here, "ui");
// --dev-url=http://127.0.0.1:18449 (npm run desktop:dev) or BFE_DEV_URL loads
// the Vite dev server instead.
const devUrl = argument("dev-url") || process.env.BFE_DEV_URL || null;
// The main menu's query (--query=...): dev flags that launch URLs keep
// (src/launch.js), so every battle the menu starts uses them.
const MENU_QUERY = argument("query") || "";

protocol.registerSchemesAsPrivileged([
  { scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true } },
]);

const primaryInstance = app.requestSingleInstanceLock();
if (!primaryInstance) app.quit();

// The Steam overlay where it can draw (electron/steam/overlay.js): Chromium's
// GPU work in this process, when Steam started the game on Windows and the
// player turned the overlay on. Read before anything starts the GPU.
const overlay = overlaySwitches({ steam: readSteamSettings(app.getPath("userData")) });
for (const [name, value] of overlay) app.commandLine.appendSwitch(name, value);

// ---- Settings ----

const settingsFile = () => path.join(app.getPath("userData"), "desktop-settings.json");
let settings = {};
let hosting = null, quitApproved = false, netHost = null;

async function loadSettings() {
  try {
    settings = JSON.parse(await fs.readFile(settingsFile(), "utf8")) || {};
  } catch {
    settings = {};
  }
}

// One save at a time, each through a file of its own: the loader, the
// options page and the Steam library's checks may save together.
let settingsSaved = Promise.resolve(), settingsSaves = 0;
function saveSettings() {
  const write = async () => {
    await fs.mkdir(path.dirname(settingsFile()), { recursive: true });
    const temp = `${settingsFile()}.${process.pid}.${++settingsSaves}.tmp`;
    await fs.writeFile(temp, JSON.stringify(settings, null, 2));
    await fs.rename(temp, settingsFile());
  };
  settingsSaved = settingsSaved.then(write, write);
  return settingsSaved;
}

// ---- Game data ----
//
// Where the renderer's /maps, /sides, /common and /audio come from, in order:
// --content-dir= or BFE_CONTENT_DIR; a folder the player chose; the app's own
// prepared folder, filled from the Steam or GOG copy by tools/prepare.py; or, with
// no copy to prepare from, extracted data found elsewhere (a checkout's
// public/, say).

const forcedContentDir = argument("content-dir") || process.env.BFE_CONTENT_DIR || null;
const managedContentDir = () => path.join(app.getPath("userData"), "game-data");
let activeContentDir = null;
let contentReady = false, startupTask = null;
let transferController = null, transferTask = null;

const contentRoot = () => path.resolve(forcedContentDir || settings.chosenContentDir || activeContentDir || managedContentDir());
const hasGameData = (root) => !!root && existsSync(path.join(root, "common", "common.json"));

// The AppImage this run came from, also when it handed off to an update
// installed in the user's folder (update/launch.js: BFE_APPIMAGE).
const appImage = process.env.APPIMAGE || process.env.BFE_APPIMAGE || null;

function likelyContentDirs() {
  const home = app.getPath("home");
  const checkouts = ["", "src", "code", "Code", "dev", "git", "projects", "Projects", "repos", "Documents", "Desktop", "Downloads"];
  return [
    settings.foundContentDir,
    app.isPackaged ? null : path.join(projectRoot, "public"),
    path.join(path.dirname(app.getPath("exe")), "game-data"),
    appImage ? path.join(path.dirname(appImage), "game-data") : null,
    ...checkouts.map((dir) => path.join(home, dir, "battlefront-eternal", "public")),
  ].filter(Boolean);
}

async function findInstall() {
  const extra = [settings.gameDir, app.isPackaged ? null : path.join(projectRoot, "data", "game", "install")].filter(Boolean);
  return findGameInstall({ extra });
}

// ---- Preparation ----

// Measured step times, kept across runs for the loader's estimate
// (prepare.js: estimate).
const timingsFile = () => path.join(app.getPath("userData"), "prepare-timings.json");
let timingsSave = null;

const preparer = new Preparer({
  paths: toolchainPaths({ packaged: app.isPackaged, resourcesPath: process.resourcesPath, projectRoot }),
  logFile: path.join(app.getPath("userData"), "logs", "prepare.log"),
  onTiming: (timings) => {
    clearTimeout(timingsSave);
    timingsSave = setTimeout(() => fs.writeFile(timingsFile(), JSON.stringify(timings)).catch(() => {}), 1000);
  },
  onEvent: (event) => {
    // The log streams to the page in batches; everything else at once.
    if (event.event === "log") scheduleState();
    else sendState();
    if (event.event === "finished") finishPreparation();
    // Another run holds this folder (tools/prepare.py's lock): wait for it,
    // then resume from what it recorded instead of doing the work twice.
    if (event.event === "busy") setTimeout(() => preparer.last && preparer.start(preparer.last), 15000);
  },
});

async function loadTimings() {
  try {
    const timings = JSON.parse(await fs.readFile(timingsFile(), "utf8"));
    if (timings && typeof timings === "object") preparer.timings = timings;
  } catch {
    // none yet
  }
}

// What the loader shows: searching, missing (no copy found), preparing, error.
let loader = { mode: "searching", install: null, message: null };

function loaderState() {
  const preparing = loader.mode === "preparing";
  return { ...loader, hasContent: hasGameData(contentRoot()), content: contentRoot(), snapshot: preparing ? preparer.snapshot : null, estimate: preparing ? preparer.estimate() : null };
}

let stateTimer = null;
function sendState() {
  clearTimeout(stateTimer);
  stateTimer = null;
  if (mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send("bfe:prepare-state", loaderState());
}
function scheduleState() {
  stateTimer ??= setTimeout(sendState, 250);
}

function setLoader(next) {
  loader = { install: null, message: null, ...next };
  // Checking for an update is still starting up; downloading one is not.
  presence.loader(loader.mode === "update" && loader.phase === "checking" ? "starting" : loader.mode);
  sendState();
}

async function startPreparing(install, content = managedContentDir()) {
  contentReady = false;
  activeContentDir = content;
  delete settings.chosenContentDir;
  settings.preparingContentDir = content;
  settings.gameDir = install.dir;
  await saveSettings();
  setLoader({ mode: "preparing", install: install.dir });
  openLoader();
  // Every core: nothing else runs while the loader is up. The tool sizes
  // what runs side by side to the memory available.
  await preparer.start({ game: install.dir, content: activeContentDir, jobs: os.cpus().length });
  sendState();
}

// Only successful, current preparation can leave the loader. A failed map
// step must not be mistaken for ready content just because common.json exists.
async function finishPreparation() {
  contentReady = preparedContentCurrent(await readPrepareState(contentRoot()));
  if (!contentReady) {
    setLoader({ mode: "error", message: "Game data could not be fully updated. Retry preparation before playing. See the preparation log for details." });
    return;
  }
  if (loaderOpen()) setTimeout(() => loaderOpen() && openMenu(), 2500);
}

// Decides what the window opens on. Serialized so menu and loader actions
// cannot start two repairs or open the game during source verification.
function chooseStart() {
  if (startupTask) return startupTask;
  startupTask = chooseContent().catch(error => {
    setLoader({ mode: "error", message: error.message });
    return "loader";
  }).finally(() => { startupTask = null; });
  return startupTask;
}

async function chooseContent() {
  if (forcedContentDir || devUrl) { contentReady = true; return "menu"; }
  if (preparer.running) return "loader";
  contentReady = false;
  const result = await startupContent({
    chosen: settings.chosenContentDir,
    managed: settings.preparingContentDir || managedContentDir(),
    hasGameData, findInstall, installedAddOns,
    findContent: () => findContent({ likely: likelyContentDirs(), home: app.getPath("home") }),
  });
  if (result.mode === "menu") {
    activeContentDir = result.content;
    if (settings.chosenContentDir && !hasGameData(settings.chosenContentDir)) delete settings.chosenContentDir;
    settings.foundContentDir = result.content;
    await saveSettings();
    contentReady = true;
    return "menu";
  }
  if (result.mode === "prepare") {
    if ((await hosting?.("status"))?.active) throw Error("Stop your hosted server before updating game data, then retry.");
    if (result.selected) {
      await beginTransfer(async options => {
        setLoader({ mode: "transfer", operation: "source" });
        // Only verified source archives can reuse imported checkpoints.
        const content = await preparationTarget(result.content, result.install.dir, options);
        options.signal.throwIfAborted();
        await startPreparing(result.install, content);
        return true;
      });
    } else await startPreparing(result.install, result.content);
    return "loader";
  }
  setLoader({ mode: result.mode });
  return "loader";
}

async function preparationTarget(content, install, options) {
  if (await relocatePreparation(content, install, options)) return content;
  const managed = managedContentDir();
  return path.resolve(managed) === path.resolve(content)
    ? fs.mkdtemp(path.join(app.getPath("userData"), "game-data-repair-")) : managed;
}

// ---- Activity for friends ----
//
// Discord Rich Presence (electron/presence; docs/presence.md). The window's
// navigation says which battle the page is; the pages report the rest.

const presenceLog = (line) => console.log(`presence: ${line}`);
const discord = new DiscordPresence({ clientId: process.env.BFE_DISCORD_APP_ID || DISCORD_APPLICATION_ID, log: presenceLog });
const presence = createPresence({ settings: () => settings.presence, contentRoot, discord, image: DISCORD_LARGE_IMAGE, log: presenceLog });

// The game in the player's Steam library, when the player asks for it
// (electron/steam): Steam shows friends "playing" while it runs.
const steamLog = (line) => console.log(`steam: ${line}`);
const steam = createSteamIntegration({
  settings: () => settings.steam,
  save: async (record) => { settings.steam = record; await saveSettings(); },
  artwork: path.join(here, "assets", "steam"),
  packaged: app.isPackaged,
  bySteam: () => startedBySteam(),
  log: steamLog,
});

// A Flatpak Steam can start the game only with a permission that reaches
// past its sandbox: asked here, in a dialog of the app's own, never on a
// page's word.
async function allowFlatpakSteam() {
  const options = {
    type: "warning",
    title: "Flatpak Steam",
    message: "Allow Flatpak Steam to start programs outside its sandbox?",
    detail: "Your Steam is the Flatpak version, which can start Battlefront Eternal only with permission to start programs outside its sandbox. The permission applies to everything Steam starts, not just this game. Removing the game from Steam in Options takes it back.",
    buttons: ["Allow", "Cancel"],
    defaultId: 1,
    cancelId: 1,
  };
  const choice = mainWindow && !mainWindow.isDestroyed() ? await dialog.showMessageBox(mainWindow, options) : await dialog.showMessageBox(options);
  return choice.response === 0;
}

// Started through a process that stays for Steam (electron/boot.js): when
// Steam ends that one (its STOP), the game ends with it.
const steamParent = Number(process.env.BFE_STEAM_PARENT);
if (Number.isInteger(steamParent) && steamParent > 0) {
  setInterval(() => {
    try { process.kill(steamParent, 0); } catch (error) { if (error.code === "ESRCH") app.quit(); }
  }, 2000).unref();
}

// The options page's STEAM & DISCORD (src/shell/integrations.js).
async function integrations(action, value) {
  if (action === "state") {
    // Off until the player answers the main menu's one-time question.
    const chosen = settings.presence?.discord;
    const detail = ["full", "game", "off"].includes(chosen) ? chosen : "off";
    const status = discord.status();
    const bySteam = startedBySteam();
    return { discord: { detail, asked: typeof chosen === "string", ...status }, steam: { ...(await steam.state()), declined: !!settings.steam?.declined, startedBySteam: bySteam, lost: steamLost(),
      overlay: { ...overlaySupport(), on: settings.steam?.overlay === true, switches: overlay.length > 0, active: bySteam && overlayLoaded() } } };
  }
  if (action === "discord") {
    if (!["full", "game", "off"].includes(value)) throw new Error("Unknown activity setting");
    settings.presence = { ...settings.presence, discord: value };
    await saveSettings();
    presence.refresh();
    return true;
  }
  if (action === "steam-add") {
    const restart = value?.restart === true;
    const result = await steam.add({ restart });
    if (!result?.needsAccess) return result;
    if (!(await allowFlatpakSteam())) return { needsAccess: true, refused: true };
    return steam.add({ restart, grant: true });
  }
  if (action === "steam-remove") return steam.remove({ restart: value?.restart === true });
  if (action === "steam-launch") {
    settings.steam = { ...settings.steam, launch: value === true };
    await saveSettings();
    return true;
  }
  if (action === "steam-overlay") {
    settings.steam = { ...settings.steam, overlay: value === true };
    await saveSettings();
    return true;
  }
  if (action === "steam-decline") {
    settings.steam = { ...settings.steam, declined: true };
    await saveSettings();
    return true;
  }
  throw new Error("Unknown integration action");
}

// ---- Updates ----
//
// On launch, under the loader and before anything else: a newer release on
// this build's channel is installed beside this version and started in its
// place (electron/update/index.js). Offline or failing, the game starts as
// it is; prepared game data is never touched, and a release with newer
// content generations prepares only the steps those change.

const updater = (() => {
  try {
    return createUpdater({ app, fetch: (url, options) => net.fetch(url, options) });
  } catch (error) {
    console.warn("updater unavailable:", error.message);
    return { enabled: false, log() {}, ready() {}, settle() {} };
  }
})();
let updateAbort = null, updateChoice = null, stayingForSteam = false;

// Whether a release's prepared-content generations differ from this
// version's, so its first start refreshes game data.
const refreshesContent = (release) => !!release.content && ["state", "maps", "sides", "common"].some((key) => release.content[key] !== CONTENT_VERSION[key]);

// Resolves true when the new version took over (this one is exiting).
async function bootUpdate() {
  if (!updater.enabled || devUrl) return false;
  setLoader({ mode: "update", phase: "checking" });
  let found;
  try {
    found = await updater.check();
  } catch (error) {
    updater.log(`check failed: ${error.message}`);
    return false;
  }
  if (!found) return false;
  const version = found.release.version, refresh = refreshesContent(found.release);
  for (;;) {
    const abort = (updateAbort = new AbortController());
    try {
      setLoader({ mode: "update", phase: "downloading", version, refresh, progress: null });
      const installed = await updater.install(found, { signal: abort.signal, onProgress: (progress) => { loader.progress = progress; scheduleState(); } });
      setLoader({ mode: "update", phase: "restarting", version, refresh });
      // The new version takes the single-instance lock; it comes back if
      // that version does not start.
      app.releaseSingleInstanceLock();
      // Started by Steam, which may watch only this process: it stays,
      // windowless, until the new version exits (electron/boot.js).
      const result = await updater.launch(installed, { stay: steamStays() && !steamLost() });
      if (result.started) {
        if (result.exited) {
          stayingForSteam = true;
          presence.stop();
          mainWindow?.destroy();
          app.exit(await result.exited);
        } else app.exit(0);
        return true;
      }
      // Nothing the player could do about it: this version carries on, and
      // the update is retried once at the next launch (update/state.js).
      app.requestSingleInstanceLock();
      return false;
    } catch (error) {
      if (abort.signal.aborted) {
        updater.log(`update to ${version} skipped`);
        return false;
      }
      updater.log(`update to ${version} failed: ${error.message}`);
      setLoader({ mode: "update-error", version, message: error.message });
      const choice = await new Promise((resolve) => { updateChoice = resolve; });
      if (choice !== "update-retry") return false;
    } finally {
      updateAbort = null;
      updateChoice = null;
    }
  }
}

// Paths an older version still runs: a hosted server keeps going from the
// version that started it, through updates, until it stops.
async function inUse() {
  try {
    if (!(await hosting?.("status"))?.active) return [];
    const config = JSON.parse(await fs.readFile(path.join(app.getPath("userData"), "hosted-server", "config.json"), "utf8"));
    return [config.engine].filter((value) => typeof value === "string");
  } catch {
    return [];
  }
}

// The loader's buttons.
async function loaderAction(name) {
  if (name === "update-skip") { updateAbort?.abort(); updateChoice?.(name); return; }
  if (name === "update-retry") { updateChoice?.(name); return; }
  if (name === "cancel-transfer") { transferController?.abort(); return; }
  if (transferTask) return;
  if (name === "import" || name === "export") return runContentTransfer(name);
  if (name === "play") { contentReady = false; return openMenu(); }
  if (name === "steam") return shell.openExternal(`https://store.steampowered.com/app/${BF1_STEAM_APP}/`);
  if (name === "gog") return shell.openExternal(BF1_GOG_PAGE);
  if (name === "choose") return chooseFolder();
  if (name === "search" || name === "retry") {
    setLoader({ mode: "searching" });
    if ((await chooseStart()) === "menu") openMenu();
  }
}

// A folder chosen by hand: a game installation (prepared from) or a folder of
// prepared data (used as it is).
async function chooseFolder() {
  if (transferTask || preparer.running) return;
  const result = await dialog.showOpenDialog(mainWindow, {
    title: "Choose Star Wars Battlefront or prepared game data",
    message: "Choose the Star Wars Battlefront (2004) installation, or a folder of prepared game data.",
    properties: ["openDirectory"],
  });
  const dir = result.canceled ? null : result.filePaths[0];
  if (!dir) return;
  const install = (await inspectInstall(dir)) || (await inspectInstall(path.dirname(dir)));
  if (install) {
    if ((await hosting?.("status"))?.active) {
      setLoader({ mode: "error", message: "Stop your hosted server before updating game data, then retry." });
      openLoader();
      return;
    }
    const imported = settings.chosenContentDir;
    if (imported && existsSync(path.join(imported, ".bfe-import.json"))) {
      return beginTransfer(async options => {
        setLoader({ mode: "transfer", operation: "source" });
        openLoader();
        const target = await preparationTarget(imported, install.dir, options);
        options.signal.throwIfAborted();
        delete settings.chosenContentDir;
        await startPreparing(install, target);
        return true;
      });
    }
    delete settings.chosenContentDir;
    return startPreparing(install);
  }
  if (hasGameData(dir)) {
    delete settings.preparingContentDir;
    settings.chosenContentDir = dir;
    contentReady = false;
    await saveSettings();
    openLoader();
    return openMenu();
  }
  dialog.showMessageBox(mainWindow, { type: "warning", message: "That folder holds neither Star Wars Battlefront (2004) nor prepared game data.", detail: dir });
}

// One transfer at a time. Renderer supplies only the action; native dialogs
// own paths. Imports become active only after complete checksum verification.
function beginTransfer(work) {
  if (transferTask) return transferTask;
  transferController = new AbortController();
  const options = { signal: transferController.signal, onProgress: progress => {
    loader.progress = progress;
    scheduleState();
  } };
  transferTask = work(options).catch(error => {
    setLoader({ mode: "transfer-error", message: options.signal.aborted ? "Transfer cancelled. Your current content is unchanged." : error.message });
    openLoader();
    return false;
  }).finally(() => { transferController = null; transferTask = null; });
  return transferTask;
}

async function runContentTransfer(action) {
  if (!["import", "export"].includes(action) || transferTask) return false;
  if (preparer.running) {
    await dialog.showMessageBox(mainWindow, { message: "Wait for game preparation to finish before transferring content." });
    return false;
  }
  return beginTransfer(async options => {
    if (action === "import" && (await hosting?.("status"))?.active) throw Error("Stop your hosted server before importing content.");
    const result = action === "export"
      ? await dialog.showSaveDialog(mainWindow, { title: "Export prepared game content", defaultPath: path.join(app.getPath("downloads"), "Battlefront-Eternal-content.bfepack"), filters: [{ name: "Battlefront Eternal content", extensions: ["bfepack"] }] })
      : await dialog.showOpenDialog(mainWindow, { title: "Import prepared game content", properties: ["openFile"], filters: [{ name: "Battlefront Eternal content export", extensions: ["bfepack", "zip"] }] });
    if (result.canceled) return false;
    options.signal.throwIfAborted();
    setLoader({ mode: "transfer", operation: action });
    openLoader();
    let completed;
    if (action === "export") completed = await exportContent(contentRoot(), result.filePath, options);
    else {
      if (forcedContentDir) throw Error("Restart without --content-dir or BFE_CONTENT_DIR before importing a different content pack.");
      completed = await importContent(result.filePaths[0], path.join(app.getPath("userData"), "content-imports"), options);
      const previous = { ...settings };
      settings.chosenContentDir = completed.path;
      settings.foundContentDir = completed.path;
      delete settings.preparingContentDir;
      try { await saveSettings(); }
      catch (error) { settings = previous; throw error; }
      activeContentDir = completed.path;
      contentReady = false;
    }
    setLoader({ mode: "transfer-done", operation: action, message: action === "export"
      ? "Your prepared maps, enhanced media and preparation records are ready to copy to another computer."
      : "Content verified and installed. You can play without installing or preparing the original game on this computer.", file: completed.path });
    return true;
  });
}

// ---- Security ----

const trustedOrigin = (url) => {
  try {
    const { origin, protocol: scheme, host } = new URL(url);
    if (scheme === `${SCHEME}:` && host === HOST) return true;
    return !!devUrl && origin === new URL(devUrl).origin;
  } catch {
    return false;
  }
};

function lockDown(contents) {
  contents.setWindowOpenHandler(({ url }) => {
    if (/^https:\/\//i.test(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  contents.on("will-navigate", (event, url) => {
    if (!trustedOrigin(url)) event.preventDefault();
  });
  contents.on("will-attach-webview", (event) => event.preventDefault());
}

const ALLOWED_PERMISSIONS = new Set(["pointerLock", "fullscreen", "keyboardLock", "clipboard-sanitized-write"]);

function configureSession() {
  const ses = session.defaultSession;
  ses.setPermissionRequestHandler((contents, permission, callback) => {
    callback(trustedOrigin(contents.getURL()) && ALLOWED_PERMISSIONS.has(permission));
  });
  ses.setPermissionCheckHandler((contents, permission) => !!contents && trustedOrigin(contents.getURL()) && ALLOWED_PERMISSIONS.has(permission));
  protocol.handle(SCHEME, createHandler(() => ({ appRoot, uiRoot, contentRoot: contentRoot() })));
}

const fromTrustedFrame = (event) => !!event.senderFrame && trustedOrigin(event.senderFrame.url);

function registerIpc() {
  let network;
  const hostPayload = app.isPackaged ? path.join(process.resourcesPath, 'host-runtime') : path.join(projectRoot, '.host-runtime');
  const host = createHostController({ executable: process.execPath,
    serverRoot: path.join(hostPayload, 'server'), engineRoot: path.join(hostPayload, 'engine'),
    contentRoot, stateRoot: path.join(app.getPath('userData'), 'hosted-server'),
    // Outside the game's processes where Steam's reaper follows them all.
    outside: () => process.platform === 'linux' && startedBySteam() && !steamStays(),
    consent: async () => { network ||= createNetworkBridge(app.getPath('userData'), message => mainWindow?.webContents.send('bfe:network-signal', message)); return (await network('notice')).accepted; },
  });
  ipcMain.handle('bfe:host', async (event, action, value) => {
    if (!fromTrustedFrame(event)) throw Error('Untrusted hosting caller');
    const result = await host(action, value);
    if (action === 'start' || action === 'join') presence.hosted(result);
    return result;
  });
  hosting = host;
  // Game traffic: a direct MessagePort between the page and the network
  // utility process (native UDP), one per battle.
  netHost = createNetHost({ identity: () => loadNetworkIdentity(app.getPath("userData")) });
  ipcMain.on("bfe:net-port", (event, id) => {
    if (!fromTrustedFrame(event)) return;
    netHost.open(event.sender, typeof id === "string" ? id.slice(0, 64) : "");
  });
  ipcMain.handle("bfe:network", async (event, action, value) => {
    if (!fromTrustedFrame(event)) throw new Error("Untrusted network caller");
    network ||= createNetworkBridge(app.getPath("userData"), message => {
      if (mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send("bfe:network-signal", message);
    });
    const result = await network(action, value);
    // The servers the player may join, for the activity's server name.
    if (action === "servers") presence.servers(result?.servers);
    else if (action === "discover") presence.servers(result);
    else if (action === "directInfo") presence.servers([result]);
    return result;
  });
  // The player's settings (electron/player-settings.js). Read synchronously:
  // the page's Settings are made before anything else runs.
  ipcMain.on("bfe:player-settings-read", (event) => {
    event.returnValue = fromTrustedFrame(event) ? { record: readPlayerSettings(app.getPath("userData")), shadowed: shadowedStorage() } : null;
  });
  ipcMain.on("bfe:player-settings-write", (event, record) => {
    if (fromTrustedFrame(event)) writePlayerSettings(app.getPath("userData"), record);
  });
  ipcMain.on("bfe:presence", (event, report) => {
    if (fromTrustedFrame(event)) presence.report(report);
  });
  ipcMain.handle("bfe:integrations", (event, action, value) => {
    if (!fromTrustedFrame(event) || typeof action !== "string") throw new Error("Untrusted integration caller");
    return integrations(action, value);
  });
  ipcMain.handle("bfe:content-transfer", (event, action) => {
    if (!fromTrustedFrame(event)) return false;
    return runContentTransfer(action);
  });
  ipcMain.handle("bfe:content-dir", (event) => (fromTrustedFrame(event) ? contentRoot() : null));
  ipcMain.handle("bfe:choose-content-dir", async (event) => {
    if (!fromTrustedFrame(event)) return null;
    await chooseFolder();
    return contentRoot();
  });
  ipcMain.handle("bfe:prepare-state", (event) => (fromTrustedFrame(event) ? loaderState() : null));
  // The main menu's version line: this build, the content generations it
  // wants, and the ones the game data in use was prepared at.
  ipcMain.handle("bfe:about", async (event) => {
    if (!fromTrustedFrame(event)) return null;
    const state = await readPrepareState(contentRoot()).catch(() => null);
    return {
      version: app.getVersion(),
      wanted: { maps: CONTENT_VERSION.maps, sides: CONTENT_VERSION.sides, common: CONTENT_VERSION.common ?? null },
      prepared: state ? { maps: state.mapImportVersion ?? null, sides: state.muzzleVersion ?? null, common: state.commonVersion ?? null, current: preparedContentCurrent(state) } : null,
    };
  });
  ipcMain.handle("bfe:prepare-action", async (event, name) => {
    if (!fromTrustedFrame(event) || typeof name !== "string") return false;
    await loaderAction(name);
    return true;
  });
}

// ---- Window ----

let mainWindow = null;
const loaderUrl = `${ORIGIN}/${DESKTOP_PREFIX}/prepare.html`;
const loaderOpen = () => !!mainWindow && !mainWindow.isDestroyed() && mainWindow.webContents.getURL().startsWith(loaderUrl);

async function openMenu() {
  if (transferTask) return;
  if (!contentReady && (await chooseStart()) !== "menu") { openLoader(); return; }
  if (!preparer.running && hasGameData(contentRoot())) setLoader({ mode: "ready" });
  mainWindow?.loadURL(`${devUrl || ORIGIN}/${!MENU_QUERY || MENU_QUERY.startsWith("?") ? MENU_QUERY : "?" + MENU_QUERY}`);
}

function openLoader() {
  if (!loaderOpen()) mainWindow?.loadURL(loaderUrl);
}

// Ctrl is crouch (src/input.js), so the stock Ctrl shortcuts are game chords:
// Ctrl+W (close) while moving forward, Ctrl+M (minimize) opening the map,
// Ctrl+R or Ctrl+Shift+R (reload) reloading and Linux's Ctrl+Q (quit)
// switching view would end the battle. Outside macOS, whose shortcuts take
// Command, those items have none: Alt+F4 and the title bar still close the
// window.
function buildMenu() {
  const isMac = process.platform === "darwin";
  const template = [
    ...(isMac ? [{ role: "appMenu" }] : []),
    {
      label: "Game",
      submenu: [
        { label: "Main Menu", click: openMenu },
        { label: "Game Data Preparation", click: openLoader },
        { label: "Choose Game or Data Folder…", click: chooseFolder },
        { label: "Export Prepared Content…", click: () => runContentTransfer("export") },
        { label: "Import Prepared Content…", click: () => runContentTransfer("import") },
        { label: "Open Game Data Folder", click: async () => shell.openPath(contentRoot()) },
        { label: "Open Preparation Log", click: () => shell.openPath(preparer.logFile) },
        { type: "separator" },
        isMac ? { role: "close" } : { label: process.platform === "win32" ? "Exit" : "Quit", click: () => app.quit() },
      ],
    },
    {
      label: "View",
      submenu: [
        isMac ? { role: "reload" } : { label: "Reload", click: () => mainWindow?.webContents.reload() },
        isMac ? { role: "forceReload" } : { label: "Force Reload", click: () => mainWindow?.webContents.reloadIgnoringCache() },
        { role: "toggleDevTools" }, { type: "separator" }, { role: "togglefullscreen" },
      ],
    },
    // Minimize and Close, which the title bar has too.
    ...(isMac ? [{ role: "windowMenu" }] : []),
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 900,
    minWidth: 800,
    minHeight: 600,
    title: "Battlefront Eternal",
    icon: path.join(here, "assets", "icon-512.png"),
    backgroundColor: "#01040c",
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: path.join(here, "preload.cjs"),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      webSecurity: true,
      spellcheck: false,
      // The world runs on timers (the simulation worker's pump): never let
      // the renderer's timers be throttled behind the compositor.
      backgroundThrottling: false,
    },
  });
  lockDown(mainWindow.webContents);
  mainWindow.webContents.on("did-navigate", (_event, url) => presence.navigated(url));
  mainWindow.once("ready-to-show", () => {
    mainWindow.show();
    // The version that started this one (a hand-off or an update) may
    // exit now; still running a little later, this version is healthy.
    updater.ready();
    setTimeout(async () => updater.settle({ protect: await inUse() }), 15000).unref?.();
  });
  mainWindow.on("closed", () => (mainWindow = null));
  // F11 toggles fullscreen without the menu bar showing.
  mainWindow.webContents.on("before-input-event", (event, input) => {
    if (input.type === "keyDown" && input.key === "F11") {
      mainWindow.setFullScreen(!mainWindow.isFullScreen());
      event.preventDefault();
    }
  });
}

app.on("second-instance", () => {
  if (!mainWindow) return;
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.focus();
});

app.on("web-contents-created", (_event, contents) => lockDown(contents));

app.whenReady().then(async () => {
  if (!primaryInstance) return;
  if (process.platform === "win32") app.setAppUserModelId("com.battlefronteternal.client");
  // The AppImage registers itself: launcher entry, icon and `battlefront`;
  // the installer's launcher learns to start installed updates directly.
  if (process.platform === "linux" && appImage) {
    integrateLinuxDesktop({ executable: appImage, icon: path.join(here, "assets", "icon-512.png") })
      .catch((error) => console.warn("desktop integration failed:", error.message));
    if (updater.enabled) adoptInstallerLauncher().catch((error) => console.warn("launcher update failed:", error.message));
  }
  await loadSettings();
  await loadTimings();
  // An entry in the Steam library follows a moved launcher (Steam closed).
  steam.maintain().catch((error) => steamLog(`library entry check failed: ${error.message}`));
  configureSession();
  registerIpc();
  buildMenu();
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
      openMenu();
    }
  });
  // The loader shows while updates are checked and the search runs; the
  // menu replaces it when the game data is ready.
  if (devUrl) return openMenu();
  openLoader();
  if (await bootUpdate()) return;
  if ((await chooseStart()) === "menu") openMenu();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin" && !stayingForSteam) app.quit();
});

// An interrupted preparation resumes next launch.
app.on("before-quit", event => {
  if (transferTask) {
    event.preventDefault();
    transferController?.abort();
    transferTask.finally(() => app.quit());
    return;
  }
  if (quitApproved || !hosting) return;
  event.preventDefault();
  hosting('status').then(async status => {
    if (!status.active) { quitApproved = true; app.quit(); return; }
    const options = { type: 'question', title: 'Hosted server is running', message: 'Other players can keep playing after you close the app.', buttons: ['Keep server running', 'Stop server and quit', 'Cancel'], defaultId: 0, cancelId: 2 };
    const result = mainWindow && !mainWindow.isDestroyed() ? await dialog.showMessageBox(mainWindow, options) : await dialog.showMessageBox(options);
    if (result.response === 2) return;
    if (result.response === 1) await hosting('stop');
    quitApproved = true; app.quit();
  }).catch(error => dialog.showErrorBox('Cannot close hosted server', error.message));
});
app.on("will-quit", () => { preparer.stop(); netHost?.stop(); presence.stop(); });
