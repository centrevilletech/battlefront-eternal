import { ScreenStack } from "../menus/stack.js";
import { MenuScreen } from "../menus/screen.js";
import { Button, Rows } from "../menus/widgets.js";
import { confirmPopup } from "../menus/popup.js";
import { optionsMenu } from "../menus/option-pages.js";
import { bindMenuInput } from "../menus/input.js";
import { ClassicTheme } from "../menus/themes/classic/index.js";
import { mainMenuUrl, restartUrl } from "../launch.js";
import { withSettings } from "./overrides.js";

// The pause menu, as the 2004 game's (ifs_pausemenu; docs/menus.md):
// - Esc opens it, and so does the mouse being let go by the browser (Esc
//   while it is captured, or the page losing focus): GAME PAUSED over
//   RESUME GAME, OPTIONS, RESPAWN, RESTART MISSION, QUIT and EXIT TO
//   WINDOWS, a stack of rows turned 25 degrees, with BACK at the bottom
//   left. Nothing is dimmed: the frozen world shows behind it.
// - The world stops and the HUD hides (the HUD's overlay slot). Over the
//   spawn screen or the scoreboard it takes that screen's place until it
//   closes.
// - Every choice but RESUME and OPTIONS asks first, YES / NO with NO
//   focused: RESPAWN kills the soldier and resumes; RESTART MISSION loads
//   the battle again; QUIT goes to the front end's main menu; EXIT TO
//   WINDOWS closes the tab if the browser lets it, else goes there too.
// - OPTIONS: the same option pages as the front end's (src/menus).
// - RESUME (BACK, Esc) gives the mouse back to the game: onClose(true), and
//   the game captures it again from inside the click or key that chose it.
//
// The object it returns is what the spawn screen and scoreboard already
// call: isOpen, open(), close(resume), the onClose they wrap, and
// enableRespawn().

// The original's layout: its list has 11 rows, 5 of them never shown in
// single player on PC (free camera, player list, the console, friends,
// recent players), which still set its container's place.
export const LAYOUT = { at: [0.6, 0.5], y: 20, rotY: 25, font: "gamefont_medium", width: 260, ySpacing: 5 };
const LIST_COUNT = 11;
const LOCK_GRACE = 250; // ms: an Esc this soon after the mouse was let go is the same press
const PROMPT_DELAY = 0.15; // s before "CLICK TO RESUME" shows while the mouse is awaited

const say = (strings, key, fallback) => String((key && strings[key]) || fallback).replace(/\r?\n/g, " ").trim();

// hud: the BattleHud (its overlay slot); settings: the Settings; controls:
// the game's Controls (release, the lock); strings: common.json's.
// onOpen() and onClose(resume): the game pauses and resumes the world.
// viewpoint: { get, set } for GAME OPTIONS' VIEWPOINT; audioPanel(): the
// audio panel. navigate(url) and closeWindow(): leaving the page (tests
// replace them). clock: "auto" steps itself on each HUD frame, "manual"
// is stepped by update(dt).
// restart: the RESTART MISSION row. Undefined: the battle's URL again (a
// local battle). { label, prompt, run }: that action instead (the machine
// hosting a match asks its server). False: no row (a client of someone
// else's server).
export function createPauseMenu({ hud, settings, controls = null, strings = {}, element = hud?.renderer?.domElement ?? null,
  onOpen = () => {}, onClose = () => {}, viewpoint = null, audioPanel = null, navigate = defaultNavigate,
  closeWindow = () => window.close(), theme = new ClassicTheme(), clock = "auto", target = typeof window !== "undefined" ? window : null, restart = undefined } = {}) {
  let open = false;
  let openedAt = -Infinity;
  let openedBy = null;
  let resumeOnRelease = false;
  let respawn = null;
  let owners = [];
  let waiting = null; // { time } while RESUME waits for the mouse
  let last = null;
  const stack = new ScreenStack({ theme });
  // A page theme (src/menus/themes/page) draws the menus as a page of
  // elements over the battle, with the system's pointer on them; the
  // Classic one draws them on the HUD, with the original's pointer.
  const page = typeof theme?.render === "function";

  const rows = new Rows({
    id: "pause",
    title: say(strings, "game.pause.title", "GAME PAUSED"),
    layout: LAYOUT,
    listCount: LIST_COUNT,
    rows: [
      { id: "resume", label: say(strings, "common.resume", "RESUME GAME"), action: () => menu.close(true) },
      { id: "opts", label: say(strings, "ifs.main.options", "OPTIONS"), action: () => stack.push(options()) },
      { id: "suicide", label: () => respawn?.label || say(strings, "game.pause.suicide", "RESPAWN"), action: () => ask("suicide") },
      ...(restart === false ? [] : [{ id: "restart", label: restart?.label || say(strings, "common.restart", "RESTART MISSION"), action: () => ask("restart") }]),
      { id: "quit", label: say(strings, "common.quit", "QUIT"), action: () => ask("quit") },
      { id: "exit", label: say(strings, "common.quit2windows", "EXIT TO WINDOWS"), action: () => ask("exit") },
    ],
  });
  const screen = new MenuScreen({
    id: "pause",
    widgets: [rows, new Button({ id: "back", place: "back", text: say(strings, "common.back", "BACK"), action: () => menu.close(true) })],
    back: () => menu.close(true),
    focus: "keep",
  });

  // The questions, and what YES does.
  const QUESTIONS = {
    suicide: { prompt: () => respawn?.prompt || say(strings, "game.pause.suicide_prompt", "DO YOU REALLY WISH TO KILL YOUR CURRENT PLAYER?"),
      yes: () => {
        if (respawn && (respawn.available?.() ?? true)) respawn.respawn();
        menu.close(true);
      } },
    restart: { prompt: () => restart?.prompt || say(strings, "ifs.pause.warn_restart", "DO YOU REALLY WANT TO RESTART THIS BATTLE?"),
      yes: () => {
        if (!restart?.run) return leave(withSettings(restartUrl(), settings), "restart");
        restart.run();
        menu.close(true);
      } },
    quit: { prompt: () => say(strings, "ifs.pause.warn_host_quit", "LEAVING THE GAME NOW WILL END THIS SESSION. ARE YOU SURE YOU WANT TO LEAVE?"),
      yes: () => leave(mainMenuUrl(), "quit") },
    exit: { prompt: () => say(strings, "ifs.main.askquit", "REALLY EXIT TO WINDOWS?"),
      yes: () => {
        menu.closeWindow();
        // Still here: the browser keeps tabs it didn't open by script.
        setTimeout(() => leave(mainMenuUrl(), "exit"), 300);
      } },
  };

  function ask(kind) {
    const question = QUESTIONS[kind];
    stack.push(confirmPopup({ id: `confirm-${kind}`, message: question.prompt(), strings, onYes: question.yes }));
  }

  function leave(url, why) {
    menu.leaving = { url, why };
    menu.navigate(url);
  }

  function options() {
    return optionsMenu({ settings, strings, inBattle: true, viewpoint, audioPanel });
  }

  // The HUD's overlay: the menus, or the prompt while RESUME waits for the
  // mouse (which leaves the HUD showing).
  const overlay = {
    hidesHud: true,
    prepare: () => {
      if (menu.clock === "auto") tick();
    },
    draw: (surface, batch) => {
      if (open) stack.draw(surface, batch);
      else if (waiting && !page) drawPrompt(surface, batch);
    },
  };

  function tick() {
    const now = performance.now();
    const dt = last === null ? 0 : Math.min(Math.max((now - last) / 1000, 0), 0.1);
    last = now;
    update(dt);
  }

  function update(dt) {
    if (open || waiting) stack.update(dt);
    if (waiting) waiting.time += dt;
  }

  function drawPrompt(surface, batch) {
    if (waiting.time < PROMPT_DELAY) return;
    const font = "gamefont_medium";
    const text = say(strings, null, "CLICK TO RESUME");
    surface.fonts.draw(batch, surface.anchor(0.5, 0.5, false), 0, -surface.fonts.lineHeight(font) / 2, text, font, [1, 1, 1, 0.5 + 0.5 * stack.pulse], 2);
  }

  function audioPanelOpen() {
    const panel = typeof document !== "undefined" ? document.getElementById("audio-panel") : null;
    return Boolean(panel && !panel.hidden);
  }

  const unbind = element && target ? bindMenuInput({
    stack,
    element,
    toInterface: (x, y) => hud.toInterface(x, y),
    scale: () => ({ x: 1024 / Math.max(element.clientWidth, 1), y: hud.height / Math.max(element.clientHeight, 1) }),
    isActive: () => open,
    exclusive: true,
    pointer: !page,
    // Esc closes the audio panel first; and the Esc that made the browser
    // let go of the mouse (and so opened the menu) isn't a second press.
    passKey: (event) => event.code === "Escape" && (audioPanelOpen() || stack.top === screen || (openedBy === "pointer" && performance.now() - openedAt < LOCK_GRACE)),
    target,
  }) : () => {};

  // Esc with nothing open that takes it itself (the spawn screen and the
  // scoreboard open the menu from their own Esc).
  const onKey = (event) => {
    if (event.code !== "Escape" || event.repeat) return;
    if (open) {
      if (stack.top !== screen || audioPanelOpen()) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (openedBy === "pointer" && performance.now() - openedAt < LOCK_GRACE) return;
      // Escape can release a pointer lock as its browser default action.
      // Reacquire only once that key has been released, or it immediately
      // drops the new lock and opens the pause menu again.
      resumeOnRelease = true;
      return;
    }
    if (owners.some((owner) => owner?.isOpen) || audioPanelOpen()) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    menu.open("key");
  };
  const onKeyUp = (event) => {
    if (event.code !== "Escape") return;
    openedBy = null;
    if (!resumeOnRelease) return;
    resumeOnRelease = false;
    if (open && stack.top === screen) menu.close(true);
  };
  target?.addEventListener("keydown", onKey, { capture: true });
  target?.addEventListener("keyup", onKeyUp, { capture: true });

  const menu = {
    stack,
    screen,
    overlay,
    clock,
    leaving: null,
    onOpen,
    onClose,
    // Leaving the page (a check can replace them).
    navigate,
    closeWindow,
    get isOpen() {
      return open;
    },
    // Whether RESUME is waiting for the mouse.
    get waiting() {
      return Boolean(waiting);
    },
    // why: "key", "pointer" (the browser let the mouse go), or a caller's.
    open(why = "key") {
      if (open) return;
      open = true;
      resumeOnRelease = false;
      openedAt = performance.now();
      openedBy = why;
      waiting = null;
      last = null;
      overlay.hidesHud = true;
      stack.clear();
      // The pointer starts in the middle, as the original warps it there;
      // it goes where the mouse is once the mouse moves.
      stack.pointer.x = 512;
      stack.pointer.y = hud.height / 2;
      stack.pointer.visible = true;
      stack.push(screen);
      // Each time, from RESUME GAME (the pointer takes it from there).
      screen.focused = { widget: rows, item: 0 };
      hud.setOverlay(overlay);
      controls?.release?.();
      if (element && !page) element.style.cursor = "none";
      menu.onOpen();
    },
    // resume: the player asked to play on (RESUME, BACK, Esc).
    close(resume = false) {
      if (!open) return;
      open = false;
      stack.clear();
      // The HUD stops drawing the overlay: the page goes now.
      if (page) theme.render(stack);
      hud.setOverlay(null);
      if (element && !page) element.style.cursor = "";
      menu.onClose(resume);
    },
    // RESUME's mouse: while the browser hasn't captured it again (Chrome
    // refuses a capture right after Esc let it go), the world stays paused
    // with CLICK TO RESUME showing, and a click captures it.
    awaitPointer(on) {
      waiting = on ? { time: 0 } : null;
      if (page) theme.prompt?.(on ? say(strings, null, "CLICK TO RESUME") : null);
      overlay.hidesHud = false;
      if (on) hud.setOverlay(overlay);
      else if (!open && hud.overlay === overlay) hud.setOverlay(null);
    },
    // The screens that open the menu from their own Esc.
    setEscapeOwners(...screens) {
      owners = screens.filter(Boolean);
    },
    // RESPAWN's text and action (the spawn screen's pauseRespawn): label,
    // prompt, available() (the soldier alive), respawn().
    enableRespawn(options) {
      respawn = options;
    },
    // The old settings dialog's hook; the menu draws its own pointer.
    setPointerLocked() {},
    update,
    dispose() {
      menu.close(false);
      unbind();
      target?.removeEventListener("keydown", onKey, { capture: true });
      target?.removeEventListener("keyup", onKeyUp, { capture: true });
    },
  };
  return menu;
}

// Leaving the battle's page in its place (as the end table's next battle
// does): the browser's Back doesn't go back into a battle.
function defaultNavigate(url) {
  const to = new URL(url, location.href).href;
  if (to === location.href) location.reload();
  else location.replace(to);
}
