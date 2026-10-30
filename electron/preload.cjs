// The renderer's only bridge to the desktop app. Sandboxed preloads must be
// CommonJS. Keep this narrow: no filesystem, shell or socket access.
const { contextBridge, ipcRenderer } = require("electron");

const ACTIONS = new Set(["play", "retry", "search", "choose", "steam", "gog", "import", "export", "cancel-transfer", "update-skip", "update-retry",
  "prepare", "pick-copy", "recheck", "enhanced-sound", "add-sound"]);
// An action's value: a copy's index (pick-copy) or on and off (enhanced-sound).
const actionValue = (value) => (typeof value === "number" || typeof value === "boolean" ? value : undefined);

// Delivered to this page's own origin only.
const pageOrigin = () => (window.location.origin && window.location.origin !== "null" ? window.location.origin : "*");
ipcRenderer.on("bfe:net-port", (event, message) => window.postMessage({ type: "bfe:net-port", id: String(message?.id ?? "") }, pageOrigin(), event.ports));
ipcRenderer.on("bfe:net-exit", (_event, message) => window.postMessage({ type: "bfe:net-exit", code: Number(message?.code) || 0 }, pageOrigin()));

contextBridge.exposeInMainWorld("bfeDesktop", {
  host: { request: (action, value) => ipcRenderer.invoke("bfe:host", action, value) },
  network: {
    request: (action, value) => ipcRenderer.invoke("bfe:network", action, value),
    onSignal: (callback) => {
      const listener = (_event, message) => callback(message);
      ipcRenderer.on("bfe:network-signal", listener);
      return () => ipcRenderer.removeListener("bfe:network-signal", listener);
    },
    // A battle's direct port to the network process. It arrives as a window
    // message ({type: "bfe:net-port", id}) carrying the MessagePort: ports
    // cannot cross the context bridge itself.
    openSession: (id) => ipcRenderer.send("bfe:net-port", String(id).slice(0, 64)),
  },
  // What friends see the player doing (electron/presence): the page's
  // screen, side and players; the main process checks every report.
  presence: { update: (report) => ipcRenderer.send("bfe:presence", report) },
  // The player's settings, kept by the main process (electron/player-settings.js).
  settingsStore: {
    read: () => ipcRenderer.sendSync("bfe:player-settings-read"),
    write: (record) => ipcRenderer.send("bfe:player-settings-write", record),
  },
  // The options page's STEAM & DISCORD: what is set up, and the player's
  // choices (add to or remove from the Steam library, Discord's detail).
  integrations: { request: (action, value) => ipcRenderer.invoke("bfe:integrations", action, value) },
  platform: process.platform,
  versions: { electron: process.versions.electron, chrome: process.versions.chrome },
  contentTransfer: action => ["import", "export"].includes(action) ? ipcRenderer.invoke("bfe:content-transfer", action) : Promise.resolve(false),
  contentDir: () => ipcRenderer.invoke("bfe:content-dir"),
  // This build's version and its game data's generations (the main menu).
  about: () => ipcRenderer.invoke("bfe:about"),
  chooseContentDir: () => ipcRenderer.invoke("bfe:choose-content-dir"),
  // The first-run loader (electron/ui/prepare.js).
  prepare: {
    state: () => ipcRenderer.invoke("bfe:prepare-state"),
    action: (name, value) => (ACTIONS.has(name) ? ipcRenderer.invoke("bfe:prepare-action", name, actionValue(value)) : Promise.resolve(false)),
    onState: (callback) => {
      const listener = (_event, state) => callback(state);
      ipcRenderer.on("bfe:prepare-state", listener);
      return () => ipcRenderer.removeListener("bfe:prepare-state", listener);
    },
  },
});
