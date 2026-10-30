import { MenuScreen, Options, OptionRow, Button, Text, confirmPopup, okPopup } from "../menus/index.js";

// OPTIONS > STEAM & DISCORD (desktop client only; electron/steam and
// electron/presence, docs/presence.md): how much Discord friends see, and
// the game in the player's Steam library, which is what makes Steam show
// friends "playing Battlefront Eternal". The desktop app owns these
// settings (desktop-settings.json), since the boot reads the Steam one
// before any page loads; the page keeps the last state it was told.
//
// The first time the main menu shows, offerDiscord asks once whether
// Discord friends may see what the player plays (nothing goes to Discord
// before that), and, when Steam is installed without the game in its
// library, offerSteam asks once.

const DETAIL = [["full", "MAP AND SERVER"], ["game", "GAME ONLY"], ["off", "OFF"]];

export function discordText(discord) {
  if (!discord || discord.state === "unavailable") return "NOT IN THIS BUILD";
  if (discord.detail === "off") return "OFF";
  if (discord.state === "connected") return discord.user?.name ? `CONNECTED: ${discord.user.name}`.toUpperCase() : "CONNECTED";
  return "DISCORD NOT RUNNING";
}

export function steamText(steam) {
  if (!steam) return "CHECKING...";
  if (!steam.available) return steam.problem === "unsupported" ? "NOT ON THIS SYSTEM" : "STEAM NOT FOUND";
  if (steam.added) return "IN LIBRARY: REMOVE";
  if (steam.problem === "snap") return "SNAP STEAM CANNOT START IT";
  if (steam.problem) return "CANNOT START FROM STEAM";
  return "ADD TO STEAM";
}

export function integrationsPage({ bridge, strings = {}, every = 3000 }) {
  let state = null, working = false, timer = null;
  const refresh = async () => {
    try { state = await bridge.request("state"); } catch { state = null; }
  };
  // Steam closing and starting again takes a while.
  const work = async (job) => {
    working = true;
    try { await job(); } finally { working = false; await refresh(); }
  };
  const notice = (screen, message) => screen.stack.push(okPopup({ message, strings }));
  const rows = [
    new OptionRow({
      id: "discord", kind: "choice", label: "DISCORD ACTIVITY", choices: DETAIL,
      get: () => state?.discord?.detail ?? "off",
      set: async (value) => {
        if (state) state.discord.detail = value;
        await bridge.request("discord", value).catch(() => {});
        await refresh();
      },
      note: "WHAT DISCORD FRIENDS SEE WHILE YOU PLAY",
    }),
    new OptionRow({ id: "discord-status", kind: "fixed", label: "DISCORD", display: () => discordText(state?.discord) }),
    new OptionRow({
      id: "steam", kind: "action", label: "STEAM LIBRARY", display: () => (working ? "CHANGING STEAM..." : steamText(state?.steam)),
      disabled: () => working || !state?.steam?.available || (!state.steam.added && !!state.steam.problem),
      note: "STEAM SHOWS FRIENDS WHAT YOU PLAY WHEN THE GAME STARTS THROUGH IT",
      action: async (screen) => {
        const steam = state?.steam;
        if (!steam?.available) return;
        if (steam.added) {
          const remove = (restart) => work(async () => {
            try {
              const result = await bridge.request("steam-remove", { restart });
              if (result?.needsRestart) screen.stack.push(confirmPopup({ message: "STEAM WILL RESTART. CONTINUE?", strings, onYes: () => remove(true) }));
            } catch (error) { notice(screen, String(error.message || error).replace(/^Error invoking remote method '[^']+': (Error: )?/, "").toUpperCase()); }
          });
          screen.stack.push(confirmPopup({ message: "REMOVE FROM YOUR STEAM LIBRARY?", strings, onYes: () => remove(false) }));
          return;
        }
        await steamDialog(bridge, steam);
        await refresh();
      },
    }),
    // Started by Steam through an installed launcher from before it could
    // stay for Steam (electron/steam/launcher.js: steamLost).
    new OptionRow({
      id: "steam-lost", kind: "action", label: "STEAM", display: "REINSTALL ONCE",
      hidden: () => !state?.steam?.lost,
      note: "AN OLDER INSTALL STARTED THIS COPY: STEAM SOON STOPS SHOWING IT",
      action: () => window.open("https://battlefronteternal.com/", "_blank"),
    }),
    // Windows: Steam's overlay over the game, by the player's choice
    // (electron/steam/overlay.js). Linux: Steam's overlay cannot reach it.
    new OptionRow({
      id: "steam-overlay", kind: "toggle", label: "STEAM OVERLAY",
      hidden: () => !state?.steam?.overlay?.supported,
      get: () => state?.steam?.overlay?.on === true,
      set: async (value) => {
        if (state?.steam?.overlay) state.steam.overlay.on = value;
        await bridge.request("steam-overlay", value).catch(() => {});
      },
      note: "EXPERIMENTAL. SHIFT+TAB FROM THE NEXT START THROUGH STEAM",
    }),
    new OptionRow({
      id: "steam-overlay-now", kind: "fixed", label: "OVERLAY THIS START",
      display: () => (state?.steam?.overlay?.active ? "LOADED" : "NOT LOADED"),
      hidden: () => !(state?.steam?.overlay?.supported && state?.steam?.overlay?.switches),
    }),
    new OptionRow({
      id: "steam-overlay-linux", kind: "fixed", label: "STEAM OVERLAY", display: "NOT ON LINUX",
      hidden: () => !["wayland", "x11"].includes(state?.steam?.overlay?.reason) || !state?.steam?.available,
    }),
    new OptionRow({
      id: "steam-launch", kind: "toggle", label: "START THROUGH STEAM",
      hidden: () => !state?.steam?.added,
      get: () => state?.steam?.launch !== false,
      set: async (value) => {
        if (state?.steam) state.steam.launch = value;
        await bridge.request("steam-launch", value).catch(() => {});
      },
      note: "WITH STEAM OPEN, EVERY START GOES THROUGH YOUR STEAM LIBRARY",
    }),
  ];
  return new MenuScreen({
    id: "options-integrations",
    dim: true,
    // Discord may start or stop while the page is open.
    onEnter: () => {
      refresh();
      clearInterval(timer);
      timer = setInterval(() => { if (!working) refresh(); }, every);
    },
    onExit: () => clearInterval(timer),
    widgets: [
      new Text({ id: "title", look: "title", text: "STEAM & DISCORD", y: 10 }),
      new Options({ id: "rows", layout: "game", rows }),
      new Button({ id: "back", place: "back", text: strings["common.back"] || "BACK" }),
    ],
  });
}

// The one-time offers' dialogs (Steam's, Discord's) without a page theme;
// the page theme restyles every dialog in its look (themes/page/style.js).
function offerStyle(className) {
  const style = document.createElement("style");
  style.textContent = `
    .${className} { box-sizing:border-box; width:min(660px,calc(100vw - 32px)); max-height:calc(100dvh - 32px); overflow:auto; margin:auto; padding:24px; color:#f3f1dc; background:#101e32; border:1px solid #80c9ed; border-radius:8px; font:17px/1.5 system-ui,sans-serif; text-align:left; }
    .${className}::backdrop { background:rgba(3,10,22,.8); }
    .${className} h2 { font:600 28px/1.2 system-ui,sans-serif; margin:0 0 20px; }
    .${className} p { margin:0 0 16px; }
    .${className} .label { color:var(--bfe-note, #94cce9); font-size:13px; letter-spacing:.1em; text-transform:uppercase; margin-bottom:8px; }
    .${className} .error { color:#ffb7a8; }
    .${className} footer { display:flex; flex-wrap:wrap; gap:12px; padding-top:8px; }
    .${className} button { flex:1 1 140px; white-space:nowrap; min-height:48px; padding:10px 16px; border:1px solid #8eb4ce; border-radius:4px; background:#20334c; color:#fff; font:600 16px/1.3 system-ui,sans-serif; cursor:pointer; }
    .${className} button[data-accept] { background:#f1e5a6; color:#142136; border-color:#f1e5a6; }
    .${className} button:focus-visible { outline:3px solid var(--bfe-value-focus, #7bdbff); outline-offset:3px; }
    .${className} button:disabled { opacity:.6; cursor:wait; }
  `;
  return style;
}

const paragraph = (text, className) => {
  const p = document.createElement("p");
  p.textContent = text;
  if (className) p.className = className;
  return p;
};

// Adding the game to the Steam library: what happens, said plainly, then
// the player's yes. Resolves true when it was added.
export function steamDialog(bridge, steam) {
  return new Promise((resolve) => {
    const previousFocus = document.activeElement;
    const dialog = document.createElement("dialog");
    dialog.className = "bfe-steam-dialog";
    dialog.setAttribute("aria-labelledby", "bfe-steam-title");
    const style = offerStyle("bfe-steam-dialog");
    const title = document.createElement("h2");
    title.id = "bfe-steam-title";
    title.textContent = "Show Steam friends when you play";
    const parts = [
      paragraph("Steam", "label"), title,
      paragraph("Battlefront Eternal is added to your Steam library as a non-Steam game. With Steam open, the game then starts through Steam, so your friends see “Playing Battlefront Eternal” and Steam counts your play time."),
    ];
    if (steam.name) parts.push(paragraph(`Steam account: ${steam.name}.`));
    if (steam.running) parts.push(paragraph("Steam closes and opens again to add it. Downloads in Steam pause for a moment."));
    if (steam.needsAccess) parts.push(paragraph("You use the Flatpak version of Steam. It can start Battlefront Eternal only with permission to start programs outside its sandbox, which applies to everything Steam starts. The game asks you for it next."));
    parts.push(paragraph("You can remove it again in Options, Steam & Discord."));
    const status = paragraph("");
    status.setAttribute("role", "status");
    status.hidden = true;
    const error = paragraph("", "error");
    error.setAttribute("role", "alert");
    error.hidden = true;
    const footer = document.createElement("footer");
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Not now";
    const accept = document.createElement("button");
    accept.type = "button";
    accept.dataset.accept = "";
    accept.textContent = steam.running ? "Restart Steam and add" : "Add to Steam";
    let busy = false, finished = false;
    const finish = (value) => {
      if (finished) return;
      finished = true;
      dialog.close();
      dialog.remove();
      style.remove();
      previousFocus?.focus?.();
      resolve(value);
    };
    const sync = () => { accept.disabled = busy; cancel.disabled = busy; };
    cancel.onclick = () => { if (!busy) finish(false); };
    accept.onclick = async () => {
      if (busy || finished) return;
      busy = true;
      sync();
      error.hidden = true;
      status.textContent = steam.running ? "Restarting Steam… this can take a little while." : "Adding to Steam…";
      status.hidden = false;
      try {
        const result = await bridge.request("steam-add", { restart: true });
        if (result?.added) return finish(true);
        throw new Error(result?.needsAccess ? "Flatpak Steam needs that permission to start the game." : "Steam could not be changed.");
      } catch (cause) {
        error.textContent = String(cause?.message || cause).replace(/^Error invoking remote method '[^']+': (Error: )?/, "");
        error.hidden = false;
      } finally {
        busy = false;
        status.hidden = true;
        if (!finished) sync();
      }
    };
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); if (!busy) finish(false); });
    // Menu input listens on window: keys and the wheel stay in the dialog.
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.addEventListener("wheel", (event) => event.stopPropagation());
    footer.append(cancel, accept);
    dialog.append(...parts, status, error, footer);
    document.head.append(style);
    document.body.append(dialog);
    sync();
    dialog.showModal();
    cancel.focus();
  });
}

// Discord: whether friends see the game, and how much (DETAIL), asked
// plainly. Resolves the player's answer, "off" when the dialog is closed.
export function discordDialog() {
  return new Promise((resolve) => {
    const previousFocus = document.activeElement;
    const dialog = document.createElement("dialog");
    dialog.className = "bfe-discord-dialog";
    dialog.setAttribute("aria-labelledby", "bfe-discord-title");
    const style = offerStyle("bfe-discord-dialog");
    const title = document.createElement("h2");
    title.id = "bfe-discord-title";
    title.textContent = "Show Discord friends what you play?";
    const footer = document.createElement("footer");
    const finish = (value) => {
      dialog.close();
      dialog.remove();
      style.remove();
      previousFocus?.focus?.();
      resolve(value);
    };
    const choice = (text, value, accept = false) => {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = text;
      button.dataset.choice = value;
      if (accept) button.dataset.accept = "";
      button.onclick = () => finish(value);
      return button;
    };
    const no = choice("No thanks", "off");
    footer.append(no, choice("Game only", "game"), choice("Map and server", "full", true));
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish("off"); });
    // Menu input listens on window: keys and the wheel stay in the dialog.
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.addEventListener("wheel", (event) => event.stopPropagation());
    dialog.append(
      paragraph("Discord", "label"), title,
      paragraph("Battlefront Eternal can show “Playing Battlefront Eternal” on your Discord profile while you play. With map and server, friends also see the battle you are in and the server's name."),
      paragraph("Nothing is sent to Discord unless you say yes."),
      paragraph("You can change this any time in Options, Steam & Discord."),
      footer,
    );
    document.head.append(style);
    document.body.append(dialog);
    dialog.showModal();
    no.focus();
  });
}

// Once, on the main menu, before anything goes to Discord: the build can
// reach Discord and the player has not answered yet. Any answer is kept.
let discordOffered = false;
export async function offerDiscord(bridge) {
  if (discordOffered || !bridge) return null;
  discordOffered = true;
  let state;
  try { state = await bridge.request("state"); } catch { return null; }
  const discord = state?.discord;
  if (!discord || discord.state === "unavailable" || discord.asked !== false) return null;
  const detail = await discordDialog();
  await bridge.request("discord", detail).catch(() => {});
  return detail;
}

// Once, on the main menu: the game is not in an installed Steam's library
// yet and the player has not said no. "Not now" is remembered.
let offered = false;
export async function offerSteam(bridge) {
  if (offered || !bridge) return false;
  offered = true;
  let state;
  try { state = await bridge.request("state"); } catch { return false; }
  const steam = state?.steam;
  if (!steam?.available || steam.added || steam.declined || steam.problem || steam.startedBySteam) return false;
  const added = await steamDialog(bridge, steam);
  if (!added) await bridge.request("steam-decline").catch(() => {});
  return added;
}
