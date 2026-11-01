import * as THREE from "three";
import { InterfaceSurface } from "../interface/index.js";
import { ScreenStack, ClassicTheme, bindMenuInput, SOUNDS } from "../menus/index.js";
import { PageTheme } from "../menus/themes/page/index.js";
import { createScreens } from "./screens.js";
import { Settings } from "../settings.js";
import { loadShellData } from "./data.js";
import { ShellMedia, SCREEN_MEDIA } from "./media.js";
import { MovieBackdrop, MapPreview } from "./backdrop.js";
import { serversView } from "./server-browser-view.js";
import { SOUNDTRACK } from "./audio.js";
import { CanvasFonts } from "./text.js";
import { showGate } from "./gate.js";
import { reportPresence } from "../presence.js";
import { pageGpu } from "../gpu-info.js";

// The front end: the page when the URL names no battle (src/launch.js;
// docs/shell.md). Its menus are drawn by the page theme
// (src/menus/themes/page) over a movie down each side (backdrop.js), in the
// player's look: it opens on the logo and the click gate; the press starts
// the sound and brings in the main menu. ?menus=classic draws them as the
// original does instead, on the canvas over each screen's movie: the gate
// over the first screen movie, then the main menu's flythrough and the
// first whoosh. ?screen=main (the pause menu's QUIT) goes straight to the
// main menu, the sound starting with the first click or key if the browser
// wants one.
//
// The parts:
// - data.js: shell.json and common.json, read with fallbacks;
// - movies.js: the background movies, intros, loops and their fades;
// - audio.js: the music's cycle, the bus dips, whooshes and menu sounds;
// - media.js: each screen's movie and music, and the moves between screens;
// - gate.js: the click gate; text.js: the game's fonts on a 2D canvas;
// - backdrop.js: the page theme's movies and Instant Action's preview;
// - loading.js: the battle page's loading screen.

// Under the page theme: each screen's music and no movie of its own (the
// backdrop plays under them all).
const PAGE_MEDIA = Object.fromEntries(Object.entries(SCREEN_MEDIA).map(([id, entry]) => [id, { ...entry, movie: null }]));
// Where the page theme puts screens' widgets: the main menu under the
// logo; Instant Action's lists either side of the map's preview, the
// available maps under the game they're from; the server browser across
// the page. Both of those darken the backdrop, as the option pages do.
const PAGE_LAYOUTS = {
  main: { home: true },
  ia: { dim: true, hide: ["available-title"], columns: [["#games", "available"], ["#preview", "edit"], ["selected-title", "selected"]] },
  mp: { kind: "wide", dim: true },
};

// Instant Action's game, over its available maps: the 2004 game's now;
// the 2005 game's are planned (listed, not yet to be picked).
function gamePicker() {
  const head = document.createElement("div");
  head.className = "bfe-column-head";
  const label = document.createElement("span");
  label.className = "bfe-text bfe-text-label";
  label.textContent = "AVAILABLE MAPS:";
  const field = document.createElement("label");
  field.className = "bfe-field";
  const select = document.createElement("select");
  select.title = "The game whose maps are listed";
  select.add(new Option("BATTLEFRONT (2004)", "swbf1", true, true));
  const next = new Option("BATTLEFRONT II (2005), PLANNED", "swbf2");
  next.disabled = true;
  select.add(next);
  // Its keys are its own, not the menus'.
  select.addEventListener("keydown", (event) => event.stopPropagation());
  select.addEventListener("change", () => select.blur());
  field.append(select);
  head.append(label, field);
  return head;
}
// Instant Action's era marks, by the original's icon names.
const PAGE_ICONS = { rep_icon: "REP", cis_icon: "CIS", imp_icon: "IMP", all_icon: "REB" };

export async function createShell({ query = location.search, parent = document.body } = {}) {
  const params = new URLSearchParams(query);
  // The battle page's help line and stats aren't the front end's; #hud
  // stays, empty, for errors.
  const hud = document.querySelector("#hud");
  if (hud) hud.textContent = "";
  document.querySelector("#stats")?.setAttribute("hidden", "");

  const data = await loadShellData();
  const settings = new Settings();
  settings.adoptGpuPreset(pageGpu().renderer);
  const page = params.get("menus") !== "classic";
  const media = new ShellMedia({ data, settings, parent, screens: page ? PAGE_MEDIA : SCREEN_MEDIA });
  const fonts = new CanvasFonts({ fonts: data.fonts, pages: data.pages });
  fonts.load(["gamefont_medium"]);

  const shell = {
    data,
    media,
    settings,
    fonts,
    gate: null,
    get screen() {
      return media.screen;
    },
    // Into a screen (back: returning to it); see media.js.
    enter: (id, options) => media.enter(id, options),
  };
  window.__shell = shell;

  // The page theme, its movies playing from the first screen.
  let theme = null;
  let backdrop = null;
  let preview = null;
  if (page) {
    backdrop = new MovieBackdrop();
    preview = new MapPreview();
    // A sponsor's logo comes through the desktop (the page loads no remote
    // images); without it, the browser shows the sponsor's name.
    const network = globalThis.bfeDesktop?.network;
    const logo = network ? (url) => network.request("logo", url).catch(() => null) : null;
    theme = new PageTheme({ parent, backdrop: backdrop.el, settings, icons: PAGE_ICONS,
      layouts: { ...PAGE_LAYOUTS, ia: { ...PAGE_LAYOUTS.ia, slots: { preview: preview.el, games: gamePicker() } } },
      views: { servers: serversView({ maps: data.maps, logo }) },
      // Each move in comes with the original's whoosh.
      onMove: ({ forward }) => { if (forward) media.audio.whoosh(); } });
    backdrop.play();
    // Instant Action's flyover plays in the page's own frame.
    media.preview = (mapId) => preview.show(data.maps.find((map) => map.id === mapId) || null);
    Object.assign(shell, { theme, backdrop, preview });
  }

  // The sound's clock.
  let last = performance.now();
  const tick = (now) => {
    media.update(Math.min(Math.max((now - last) / 1000, 0), 0.25));
    last = now;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  const first = params.get("screen") || "start";
  if (first === "start") {
    media.enter("start");
    theme?.gate(true);
    shell.gate = showGate({ parent, fonts, drawn: !theme, onPress: () => {
      media.audio.unlock();
      media.audio.music.set(SOUNDTRACK);
    } });
    await shell.gate.pressed;
    shell.gate = null;
    await media.enter("main");
  } else {
    // Straight to a screen: try the sound now (the browser may allow it
    // after the battle page's clicks), else at the first press.
    media.audio.unlock();
    const wake = () => {
      media.audio.unlock();
      if (media.audio.running) for (const type of ["pointerdown", "keydown"]) window.removeEventListener(type, wake, { capture: true });
    };
    for (const type of ["pointerdown", "keydown"]) window.addEventListener(type, wake, { capture: true });
    await media.enter(first);
  }
  // The original's: the canvas, the interface's textures and fonts.
  let renderer = null;
  let surface = null;
  const resize = () => {
    renderer.setPixelRatio(Math.min(devicePixelRatio || 1, 2));
    renderer.setSize(innerWidth, innerHeight);
  };
  if (!theme) {
    renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true });
    renderer.setClearColor(0, 0);
    renderer.domElement.style.cssText = "position:fixed;inset:0;z-index:1;width:100%;height:100%";
    parent.appendChild(renderer.domElement);
    resize();
    window.addEventListener("resize", resize);
    surface = new InterfaceSurface(renderer, { url: data.texture, fonts: data.fonts, strings: data.strings });
  }
  const stack = new ScreenStack({ theme: theme || new ClassicTheme(), sound: (kind) => media.audio.play(SOUNDS[kind] || kind) });
  const screens = createScreens({ data, settings, media, query, browser: Boolean(theme) });
  const initial = first === "start" ? "main" : first;
  stack.push(screens.main());
  if (initial !== "main" && screens[initial]) stack.push(screens[initial]());
  Object.assign(shell, { renderer, surface, stack, screens });
  const mediaId = () => {
    const id = stack.screens.findLast((screen) => !screen.popup)?.id || "main";
    return id.startsWith("options") ? "options" : id.startsWith("mp") ? "mp" : id.startsWith("ia") ? "ia" : id;
  };
  let current = mediaId(), depth = stack.screens.length, busy = false, revision = 0;
  reportPresence({ screen: current });
  // The page's elements take the pointer themselves.
  const unbind = theme
    ? bindMenuInput({ stack, element: theme.root, toInterface: () => ({ x: 0, y: 0 }), isActive: () => !busy, pointer: false })
    : bindMenuInput({ stack, element: renderer.domElement, toInterface: (x, y) => surface.toInterface(x, y), isActive: () => !busy });
  let drawn = performance.now(), frame;
  const draw = (now) => {
    const next = mediaId();
    if (next !== current) {
      const back = stack.screens.length < depth;
      current = next;
      reportPresence({ screen: next });
      const token = ++revision;
      busy = true;
      if (next !== "ia") preview?.show(null);
      media.enter(next, { back }).then(() => {
        if (token !== revision) return;
        busy = false;
        if (next === "ia") media.preview(data.maps[stack.top?.widget("available")?.selected || 0]?.id);
      });
    }
    depth = stack.screens.length;
    stack.update(Math.min((now - drawn) / 1000, 0.25));
    drawn = now;
    if (theme) theme.render(stack);
    else {
      renderer.clear();
      if (!busy) surface.drawFrame((batch) => stack.draw(surface, batch));
    }
    frame = requestAnimationFrame(draw);
  };
  frame = requestAnimationFrame(draw);
  shell.dispose = () => {
    cancelAnimationFrame(frame);
    unbind();
    theme?.dispose();
    backdrop?.dispose();
    preview?.dispose();
    if (renderer) {
      window.removeEventListener("resize", resize);
      renderer.dispose();
      renderer.domElement.remove();
    }
  };
  return shell;
}
