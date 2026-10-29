import { SpawnModel, COMMITTED } from "./model.js";
import { SpawnInput, bindDom } from "./input.js";
import { ClassicView } from "./classic/view.js";
import { PageView } from "./page/view.js";
import { emitSound } from "../audio/events.js";

// The spawn screen: unit, team and command post before every life, as the
// 2004 game asks for them (docs/spawn-screen.md). It opens when the map
// loads and four seconds after each death (the world's "awaiting" phase),
// and closes when the world spawns the player.
//
// The parts:
// - model.js: the choice and the screen's state, pure;
// - input.js: keys and mouse to the model, and the mouse freed while open;
// - a view: the screen itself, over the battle HUD as a layer of it.
//   Classic (the 2004 screen, drawn on the HUD) unless named: "page" is the
//   front end's page look (page/view.js, elements over the battle), which
//   the battle names unless its URL asks for the original's menus; the
//   spawnScreenView setting can name one too. All of them behind the same
//   model and input.
// Everything about the world goes through the port (the world's spawn
// port, docs/spawn-screen.md): the player's state, rosters, posts, the
// request, the preview body, the camera shots.

export const VIEWS = { classic: ClassicView, page: PageView };

// The mission's SetSoundEffect names (public/audio/<mission>/sound.json).
const CHANGE_SOUND = "SpawnDisplaySpawnPointChange";
const ACCEPT_SOUND = "SpawnDisplaySpawnPointAccept";
// A request the world hasn't acted on by then gives the choice back.
const COMMIT_WAIT = 1; // s

// The pause menu's RESPAWN (game.pause.suicide), with or without the
// screen: it kills the soldier when he is alive.
export function pauseRespawn({ menu, port, strings = {} }) {
  menu?.enableRespawn?.({
    label: strings["game.pause.suicide"] || "RESPAWN",
    prompt: strings["game.pause.suicide_prompt"] || "DO YOU REALLY WISH TO KILL YOUR CURRENT PLAYER?",
    available: () => port.humanState()?.phase === "alive",
    respawn: () => port.suicide(),
  });
}

// port: the world's spawn port; hud: the BattleHud; controls: the game's
// Controls (switched off while open); menu: the pause menu (Esc opens it
// over the screen); common: common.json (model records); clock: "auto" to
// step itself on every HUD frame, "manual" to be stepped by update(dt).
export function createSpawnScreen({ port, hud, controls = null, menu = null, settings = null, common = null,
  element = hud.renderer?.domElement ?? null, random = Math.random, clock = "auto", view: viewName = null } = {}) {
  const model = new SpawnModel({ port, random });
  let view = null;
  const input = new SpawnInput({ model, hit: (x, y) => view.hit(x, y), spawn: () => spawn(), height: () => hud.height });
  const View = VIEWS[viewName || settings?.get?.("spawnScreenView")] || ClassicView;
  view = new View({ hud, port, model, common, input });
  const sound = (effect) => emitSound("interface", effect);
  let time = 0;
  let committedAt = 0;
  let last = null;
  let disposed = false;
  const offs = [];

  // The layer steps the screen once per HUD frame when it runs itself.
  const layer = view.layer;
  layer.prepare = () => {
    if (screen.clock === "auto") tick();
  };

  function tick() {
    const now = performance.now();
    const dt = last === null ? 0 : Math.min(Math.max((now - last) / 1000, 0), 0.1);
    last = now;
    update(dt);
  }

  function freeMouse() {
    if (controls) {
      controls.enabled = false;
      controls.clear?.();
    }
    // The game's own release (so it doesn't pause the game).
    if (controls?.release) controls.release();
    else if (typeof document !== "undefined" && document.pointerLockElement) document.exitPointerLock();
    // The page's cursor hides over the canvas where the view draws its own
    // pointer, and shows where it uses the page's (systemPointer).
    if (element && !view.systemPointer) element.style.cursor = "none";
  }

  function restoreMouse() {
    if (controls) controls.enabled = !menu?.isOpen;
    if (element) element.style.cursor = "";
  }

  model.on("open", ({ first }) => {
    view.open({ first });
    input.reset();
    hud.setLayer(layer);
    freeMouse();
    last = null;
    // The world as a camera shot shows it: on the battle's first opening,
    // the mission's first shot (load.png's view), after that a random one;
    // unless the view moves through the shots itself (directsShots).
    if (!view.directsShots) {
      const shots = port.cameraShots?.() || [];
      const shot = first ? shots[0] : shots[Math.min(Math.floor(random() * shots.length), shots.length - 1)];
      port.setViewShot?.(shot ?? null);
    }
  });
  model.on("close", () => {
    view.close();
    hud.setLayer(null);
    restoreMouse();
    port.setViewShot?.(null);
    sound(ACCEPT_SOUND);
  });
  model.on("class", () => sound(CHANGE_SOUND));
  model.on("team", (event) => {
    view.teamChanged(event);
    sound(CHANGE_SOUND);
  });
  model.on("post", () => {
    view.postChanged();
    sound(CHANGE_SOUND);
  });

  function open(reason = "death") {
    if (disposed || model.isOpen || gameOver()) return;
    model.open(reason);
  }

  // reason: "spawned" when he has a unit again; anything else (the battle
  // over, the screen thrown away) also takes back a choice already sent.
  function close(reason = "spawned") {
    if (model.state === COMMITTED && reason !== "spawned") port.cancel?.();
    model.close();
  }

  // SPAWN, from inside the event that asked: the choice goes to the world,
  // and the mouse is captured again while the browser still allows it.
  function spawn() {
    const request = model.commit();
    if (!request) return null;
    committedAt = time;
    port.request(request);
    controls?.lock?.();
    return request;
  }

  // The battle is over: the screen closes and doesn't come back.
  function gameOver() {
    return Boolean(port.humanState()?.over ?? hud.battle?.over);
  }

  // One frame of the screen, dt in seconds.
  function update(dt) {
    if (disposed || !model.isOpen) return;
    const human = port.humanState();
    if (human?.spectator) { close("spectating"); return; }
    const phase = human?.phase;
    if (phase === "alive" || phase === "dying" || gameOver()) {
      close(phase === "alive" || phase === "dying" ? "spawned" : "over");
      return;
    }
    // The pause menu over it pauses it.
    view.hidePointer = Boolean(menu?.isOpen);
    if (menu?.isOpen) return;
    time += dt;
    input.update(dt);
    model.check();
    if (model.state === COMMITTED && time - committedAt > COMMIT_WAIT) {
      // The world didn't take it: back to choosing, and the choice withdrawn.
      model.uncommit();
      port.cancel?.();
    }
    view.update(dt);
  }

  // Esc opens the menu over the screen; closing it comes back to the
  // screen with the mouse still free.
  if (menu) {
    const onClose = menu.onClose;
    menu.onClose = (resume) => {
      onClose(resume && !model.isOpen);
      if (model.isOpen && controls) controls.enabled = false;
    };
    pauseRespawn({ menu, port, strings: hud.strings });
  }
  const unbind = element ? bindDom({
    input,
    element,
    toInterface: (x, y) => hud.toInterface(x, y),
    isOpen: () => model.isOpen,
    menuOpen: () => Boolean(menu?.isOpen),
    openMenu: () => menu?.open(),
  }) : () => {};

  if (port.on) {
    offs.push(port.on("awaitingSpawn", () => open("death")));
    offs.push(port.on("spawned", () => close()));
    offs.push(port.on("teamChanged", (event) => model.followTeam(event?.team)));
  }

  const screen = {
    model,
    input,
    view,
    clock,
    open,
    close,
    spawn,
    update,
    get isOpen() {
      return model.isOpen;
    },
    dispose() {
      if (disposed) return;
      close("disposed");
      disposed = true;
      unbind();
      for (const off of offs) off?.();
      view.dispose?.();
    },
  };
  // The map loads with the player waiting for his first choice.
  const phase = port.humanState()?.phase;
  if (phase === "unspawned" || phase === "awaiting") open("start");
  return screen;
}
