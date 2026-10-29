// The spawn screen's keys and mouse, bound as the 2004 PC game binds them:
//
// - Up / wheel up: the previous class (wrapping); Down / wheel down: the next.
// - Left or Right, or a click on the team logos: switch team, if allowed.
// - A click on a class window (its title bar included): that class.
// - Tab: the next eligible post; held, another every 0.2 s. A click on one of
//   the team's own posts on the map: that post.
// - Space held (down, not just pressed: holding it as the screen opens
//   spawns at once), the right mouse button, or a click on SPAWN: spawn.
// - Enter: a left click wherever the cursor is (it starts at the centre, so
//   on its own it does nothing).
// - The frame after any frame that pressed a class or team key ignores
//   those keys (as in the original).
// - Esc: the pause menu, over the screen.
//
// SpawnInput turns raw presses into model actions once per frame; bindDom
// feeds it from the page and frees the mouse while the screen is open.

const CLASS_KEYS = { ArrowUp: -1, ArrowDown: 1 };
const TEAM_KEYS = new Set(["ArrowLeft", "ArrowRight"]);
const ENTER_KEYS = new Set(["Enter", "NumpadEnter"]);
// Keys the page would otherwise scroll or move focus with.
const SWALLOW = new Set(["Space", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Tab", "Enter", "NumpadEnter"]);
export const TAB_REPEAT = 0.2; // s between posts while Tab is held

export class SpawnInput {
  // model: the SpawnModel; hit(x, y): what the interface point holds
  // ({ kind: "class", index } | { kind: "team" } | { kind: "spawn" } |
  // { kind: "post", name } | null); spawn(): commit the choice (from inside
  // the event that asked, so the pointer can be captured again).
  constructor({ model, hit = () => null, spawn = () => model.commit(), width = 1024, height = () => 768 } = {}) {
    this.model = model;
    this.hit = hit;
    this.spawn = spawn;
    this.width = width;
    this.height = height;
    this.held = new Set();
    this.pending = [];
    this.navigated = false; // the last frame pressed a class or team key
    this.tabAt = 0;
    this.time = 0;
    this.cursor = { x: width / 2, y: height() / 2 };
  }

  // The screen opened: the cursor goes to the centre (the original warps
  // it there) and presses from before are dropped, held keys kept.
  reset() {
    this.pending.length = 0;
    this.navigated = false;
    this.tabAt = this.time;
    this.cursor.x = this.width / 2;
    this.cursor.y = this.height() / 2;
  }

  get active() {
    return this.model.state === "choosing";
  }

  // A key went down (repeat: the browser's auto-repeat) or up. Keys are
  // tracked even while the screen can't act on them (act false), so one
  // held as it opens counts. Returns whether the screen used it.
  key(code, down, repeat = false, act = true) {
    if (!down) {
      this.held.delete(code);
      return false;
    }
    this.held.add(code);
    if (!act || !this.active) return false;
    if (code in CLASS_KEYS || TEAM_KEYS.has(code)) this.pending.push({ code });
    else if (ENTER_KEYS.has(code) && !repeat) this.click(this.cursor.x, this.cursor.y);
    else if (code === "Space") this.trySpawn();
    return SWALLOW.has(code);
  }

  // The wheel: up is the previous class, down the next.
  wheel(deltaY) {
    if (!this.active || !deltaY) return;
    this.pending.push({ code: deltaY < 0 ? "ArrowUp" : "ArrowDown" });
  }

  // The cursor, in interface units.
  move(x, y) {
    this.cursor.x = x;
    this.cursor.y = y;
  }

  // Mouse buttons: 0 clicks at the cursor, 2 spawns.
  button(index, down) {
    if (!down || !this.active) return false;
    if (index === 2) this.trySpawn();
    else if (index === 0) this.click(this.cursor.x, this.cursor.y);
    return true;
  }

  // A left click, tested against the hotspots. Spawning happens at once;
  // the rest waits for the frame.
  click(x, y) {
    const target = this.hit(x, y);
    if (!target) return null;
    if (target.kind === "spawn") this.trySpawn();
    else this.pending.push({ target });
    return target;
  }

  trySpawn() {
    if (this.active) this.spawn();
  }

  // Once per frame (dt in seconds): the presses since the last frame, in
  // order, then Space held and Tab held.
  update(dt) {
    this.time += dt;
    const model = this.model;
    let navigated = false;
    let classStep = 0;
    let teamSwitch = false;
    for (const { code, target } of this.pending.splice(0)) {
      if (!this.active) break;
      if (target?.kind === "class") model.selectClass(target.index);
      else if (target?.kind === "team") model.switchTeam("mouse");
      else if (target?.kind === "post") model.selectPost(target.name);
      else if (code in CLASS_KEYS) {
        classStep = CLASS_KEYS[code];
        navigated = true;
      } else if (TEAM_KEYS.has(code)) {
        teamSwitch = true;
        navigated = true;
      }
    }
    // Keys change the team, then the class, unless the last frame did.
    if (this.active && !this.navigated) {
      if (teamSwitch) model.switchTeam("key");
      if (classStep < 0) model.previousClass();
      else if (classStep > 0) model.nextClass();
    }
    this.navigated = navigated;
    if (this.held.has("Space")) this.trySpawn();
    if (this.active && this.held.has("Tab") && this.time >= this.tabAt) {
      model.cyclePost();
      this.tabAt = this.time + TAB_REPEAT;
    }
  }
}

// The page's events into a SpawnInput: keys and the wheel on the window,
// buttons and the cursor over the canvas. While the screen is open the mouse
// is free (pointer lock released, the game's own controls off) and the
// page's cursor hidden over the canvas, where the screen draws its own.
//
// toInterface(x, y): canvas pixels to interface units (BattleHud);
// isOpen(): whether the screen wants input; menuOpen(): whether the
// pause menu covers it; openMenu(): Esc's menu.
export function bindDom({ input, element, toInterface, isOpen, menuOpen = () => false, openMenu = () => {}, target = window }) {
  let escapeWithMenu = false;
  const listeners = [];
  const listen = (on, type, handler, options) => {
    on.addEventListener(type, handler, options);
    listeners.push(() => on.removeEventListener(type, handler, options));
  };
  const local = (event) => {
    const rect = element.getBoundingClientRect();
    return toInterface(event.clientX - rect.left, event.clientY - rect.top);
  };
  // The menu closes itself on Esc; noted first, so the same press doesn't
  // open it again here.
  listen(target, "keydown", (event) => {
    if (event.code === "Escape") escapeWithMenu = menuOpen();
  }, true);
  listen(target, "keydown", (event) => {
    const acting = isOpen() && !menuOpen();
    if (event.code === "Escape") {
      if (acting && !escapeWithMenu) openMenu();
      return;
    }
    if (input.key(event.code, true, event.repeat, acting)) event.preventDefault();
  });
  listen(target, "keyup", (event) => input.key(event.code, false));
  listen(target, "blur", () => input.held.clear());
  listen(target, "wheel", (event) => {
    if (isOpen() && !menuOpen()) input.wheel(event.deltaY);
  }, { passive: true });
  // A captured mouse has no place of its own (the soldier's look).
  listen(element, "mousemove", (event) => {
    if (typeof document !== "undefined" && document.pointerLockElement) return;
    // The browser sends a move that didn't move when it lets go of a
    // captured mouse (at the window's corner): not the player's.
    if (event.movementX === 0 && event.movementY === 0) return;
    const point = local(event);
    input.move(point.x, point.y);
  });
  listen(element, "mousedown", (event) => {
    if (!isOpen() || menuOpen()) return;
    const point = local(event);
    input.move(point.x, point.y);
    if (input.button(event.button, true)) event.preventDefault();
  });
  return () => listeners.forEach((remove) => remove());
}
