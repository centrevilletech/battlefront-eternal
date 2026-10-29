import { ScoreboardModel, PEEK, END, TABLE, AWARDS } from "./model.js";
import { ScoreboardInput, bindDom } from "./input.js";
import { ClassicView } from "./classic/view.js";
import { PageView } from "./page/view.js";
import { emitSound } from "../audio/events.js";

// The scoreboard: both teams' Characters, bots included, with their KILLS,
// DEATHS and CP POINTS, as the 2004 game's end-of-match table lists them
// (docs/scoreboard.md).
//
// - Tab in play shows it until Tab is let go, or pins it up after a tap.
//   This is the port's own: the original honours Tab only in network
//   games, where it lists the human players (NAME, TEAM, KILLS, PING). The
//   table and the rules for holding and tapping are the original's. Not
//   while the spawn screen is up (Tab picks posts there), the pause menu
//   is, or once the game is over.
// - When the battle ends, after the end display, the end-of-match table:
//   until DONE or 30 s without input, which go on to the next battle
//   of the Instant Action playlist, as the original's ScriptCB_QuitFromStats
//   does (the port's restart(): src/launch.js nextUrl(), the same battle
//   again without a playlist).
//   Online, the server's next launch must be ready; the session follows it
//   automatically, and DONE cannot advance an unavailable server.
// - DETAILS (a row's record) from either, AWARDS from the end table; BACK
//   or Esc goes back to the table. Esc on the table puts the pause menu
//   over it (src/pause).
// While it is up the HUD hides, the game's own sound fades out, and the
// world keeps running.
//
// The parts:
// - model.js: the rows, their order, the selection and scroll, pure;
// - input.js: keys and mouse to the model, Tab's hold-or-tap rule;
// - a view: the board itself, with its layer of the battle HUD, picked by
//   name (the scoreboardView setting's without one): Classic, the 2004
//   table drawn on the HUD, by default; "page", the front end's look as
//   elements over the battle (page/view.js), which the battle names unless
//   its URL asks for the original's menus. Each behind the same model and
//   input;
// - port.js: the world's side, plain values made from the battle.

export const VIEWS = { classic: ClassicView, page: PageView };

const SELECT_SOUND = "shell_select_change";
const ENTER_SOUND = "shell_menu_enter";
const EXIT_SOUND = "shell_menu_exit";
// The bus with the game's own sound in it, and how long it takes to fade.
const INGAME = "ingame";
const FADE = 1; // s

// port: the scoreboard port (port.js); hud: the BattleHud; controls: the
// game's Controls (off while the board is up); menu: the pause menu;
// spawnScreen: the spawn screen, if any (no Tab while it's open); common:
// common.json (model records); clock: "auto" to step itself on every HUD
// frame, "manual" to be stepped by update(dt).
export function createScoreboard({ port, hud, controls = null, menu = null, settings = null, common = null, spawnScreen = null,
  element = hud.renderer?.domElement ?? null, clock = "auto", view: viewName = null } = {}) {
  const model = new ScoreboardModel({ port });
  let view = null;
  const input = new ScoreboardInput({ model, hit: (x, y) => view.hit(x, y), act: (kind, target) => act(kind, target), height: () => hud.height });
  const View = VIEWS[viewName || settings?.get?.("scoreboardView")] || ClassicView;
  view = new View({ hud, port, model, common, input });
  const sound = (name) => emitSound("interface", name);
  const layer = view.layer;
  let last = null;
  let ended = false; // the end table has opened
  let quitting = false;
  let disposed = false;
  const offs = [];

  // Every HUD frame, open or not: the battle's end can open it.
  offs.push(hud.watch?.(() => {
    if (board.clock === "auto") tick();
  }));

  function tick() {
    const now = performance.now();
    const dt = last === null ? 0 : Math.min(Math.max((now - last) / 1000, 0), 0.1);
    last = now;
    update(dt);
  }

  // The game's controls go off and the page's cursor hides over the canvas;
  // a captured mouse stays captured, and moves the board's pointer.
  function freeMouse() {
    if (controls) {
      controls.enabled = false;
      controls.clear?.();
    }
    if (element) element.style.cursor = "none";
  }

  function restoreMouse() {
    if (controls) controls.enabled = !menu?.isOpen && !spawnScreen?.isOpen;
    if (element) element.style.cursor = "";
  }

  model.on("open", ({ state }) => {
    quitting = false;
    view.open({ state });
    input.reset();
    hud.setLayer(layer);
    freeMouse();
    // The end display gives way to the table.
    if (state === END) hud.announce?.(null);
    emitSound("busFade", INGAME, 0, FADE);
  });
  model.on("close", ({ state }) => {
    view.close();
    if (hud.layer === layer) hud.setLayer(null);
    restoreMouse();
    sound(EXIT_SOUND);
    if (state === PEEK) emitSound("busFade", INGAME, 1, FADE);
  });
  for (const event of ["sort", "select", "scroll"]) model.on(event, () => sound(SELECT_SOUND));
  let passing = false; // through the table from DETAILS to AWARDS or back
  model.on("page", (event) => {
    view.pageChanged(event);
    if (!passing) sound(event.page === TABLE ? EXIT_SOUND : ENTER_SOUND);
  });
  model.on("quit", () => quit());

  // Whether Tab may show it now.
  function canPeek() {
    return !disposed && !model.isOpen && !port.state().gameOver && !spawnScreen?.isOpen && !menu?.isOpen;
  }

  // Tab: the table, held or pinned (input.js TabHold).
  function peek() {
    if (!canPeek()) return false;
    model.open(PEEK);
    input.tab.start(input.time);
    return true;
  }

  function close() {
    if (model.state === PEEK) model.close();
  }

  // DONE or 30 s idle at the end: the next battle (the port's
  // restart()). (The original's idle quit from AWARDS plays
  // shell_menu_enter.)
  function quit() {
    if (model.state !== END || quitting || port.canRestart?.() === false) return;
    if (port.restart?.() === false) return;
    quitting = true;
    sound(model.page === AWARDS ? ENTER_SOUND : EXIT_SOUND);
  }

  // BACK or Esc: from DETAILS or AWARDS to the table; from Tab's table, out.
  function back() {
    if (model.page !== TABLE) model.back();
    else close();
  }

  // A button, from inside the event that pressed it. DETAILS and AWARDS
  // also go straight from one to the other (the page view's tabs), by way
  // of the table.
  function act(kind) {
    if (!model.isOpen) return;
    model.touch();
    if (kind === "back") back();
    else if (kind === "done") quit();
    else if (kind === "details" || kind === "awards") {
      if (model.page !== TABLE && model.page !== kind) {
        passing = true;
        model.back();
        passing = false;
      }
      if (kind === "details") model.showDetails();
      else model.showAwards();
    }
  }

  // One frame of the board, dt in seconds.
  function update(dt) {
    if (disposed) return;
    const state = port.state();
    // The battle is over and the end display done: the end table, once.
    if (state.finished && !ended) {
      ended = true;
      if (model.state === PEEK) model.close();
      model.open(END);
    }
    if (!model.isOpen) return;
    if (model.state === PEEK && (state.gameOver || spawnScreen?.isOpen)) {
      model.close();
      return;
    }
    // The pause menu over it pauses it.
    view.hidePointer = Boolean(menu?.isOpen);
    if (menu?.isOpen) return;
    input.update(dt);
    if (model.state === PEEK) {
      if (input.tab.step(input.held.has("Tab"), input.time)) {
        model.close();
        return;
      }
      // During play the numbers are live.
      model.refresh();
    }
    if (model.update(dt)) return;
    view.update(dt);
  }

  // The pause menu (Esc) can open over the board; closing it, even with
  // RESUME, comes back to the board with the mouse still free.
  if (menu) {
    const onClose = menu.onClose;
    menu.onClose = (resume) => {
      onClose(resume && !model.isOpen);
      if (model.isOpen && controls) controls.enabled = false;
    };
  }
  const unbind = element ? bindDom({
    input,
    element,
    toInterface: (x, y) => hud.toInterface(x, y),
    scale: () => ({ x: 1024 / Math.max(element.clientWidth, 1), y: hud.height / Math.max(element.clientHeight, 1) }),
    isOpen: () => model.isOpen,
    menuOpen: () => Boolean(menu?.isOpen),
    tab: () => peek(),
    // Esc: back to the table from DETAILS and AWARDS; on the table, the
    // pause menu over the board (without one, DONE on the end table and
    // closing Tab's).
    escape: () => (model.page !== TABLE ? back() : menu?.open ? menu.open("key") : model.state === END ? quit() : back()),
  }) : () => {};

  const board = {
    model,
    input,
    view,
    clock,
    peek,
    close,
    update,
    get isOpen() {
      return model.isOpen;
    },
    get state() {
      return model.state;
    },
    dispose() {
      if (disposed) return;
      close();
      disposed = true;
      unbind();
      for (const off of offs) off?.();
      view.dispose?.();
    },
  };
  return board;
}
