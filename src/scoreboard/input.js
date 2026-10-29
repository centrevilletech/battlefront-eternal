// The scoreboard's keys and mouse, bound as the 2004 PC game binds its
// player list and end-of-match table:
//
// - Tab in play: held half a second or more, the table shows until it is
//   let go; let go sooner (a tap), it stays up until Tab is held again, and
//   closes when that is let go. Esc, or a click on BACK, closes it.
// - Up / Down: the selection within its list. Left / Right: the same row
//   on the other team's list. Page Up / Page Down: a page.
// - The mouse: a click on a column's icon sorts by it (again: the other
//   way round); on an arrow, the lists scroll a row; on a row, it is
//   selected, and a second click on it within 0.4 s opens DETAILS. The row
//   under the pointer is lit.
// - Enter: a left click wherever the pointer is.
// - The end table: Esc or DONE ends it; DETAILS and AWARDS.
//
// ScoreboardInput turns raw presses into model actions once per frame;
// TabHold is the hold-or-tap rule; bindDom feeds them from the page.

export const HOLD = 0.5; // s Tab must be held for the table to close on its release
export const DOUBLE_CLICK = 0.4; // s
const HELD = 0;
const TAPPED = 1;

const NAV = { ArrowUp: ["move", -1], ArrowDown: ["move", 1], PageUp: ["turnPage", -1], PageDown: ["turnPage", 1], ArrowLeft: ["flip"], ArrowRight: ["flip"] };
const ENTER = new Set(["Enter", "NumpadEnter"]);
// Keys the page would otherwise scroll or move focus with.
const SWALLOW = new Set(["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Enter", "NumpadEnter"]);

// The original game's check, made every frame the list is up with whether Tab
// is held (ScriptCB_CheckPlayerListDone): for the first half second a
// frame without it marks a tap; after that a held list closes on the first
// frame without it, and a tapped one becomes held again once Tab is down.
export class TabHold {
  constructor() {
    this.openedAt = 0;
    this.mode = HELD;
  }

  start(now) {
    this.openedAt = now;
    this.mode = HELD;
  }

  get tapped() {
    return this.mode === TAPPED;
  }

  // Whether the list closes this frame.
  step(held, now) {
    if (now < this.openedAt + HOLD) {
      if (!held) this.mode = TAPPED;
      return false;
    }
    if (this.mode === HELD) return !held;
    if (held) this.mode = HELD;
    return false;
  }
}

export class ScoreboardInput {
  // model: the ScoreboardModel; hit(x, y): what the interface point holds
  // ({ kind: "sort", mode } | { kind: "scroll", step } | { kind: "row",
  // side, index } | { kind: "done" | "back" | "details" | "awards" } | null);
  // act(kind, detail): a button's action, from inside the event that asked.
  constructor({ model, hit = () => null, act = () => {}, width = 1024, height = () => 768 } = {}) {
    this.model = model;
    this.hit = hit;
    this.act = act;
    this.width = width;
    this.height = height;
    this.held = new Set();
    this.pending = [];
    this.time = 0;
    this.tab = new TabHold();
    this.cursor = { x: width / 2, y: height() / 2 };
    this.lastClick = null; // { side, index, time }: a row's first click
  }

  // The board opened: the pointer goes to the middle (the original warps
  // it there), and presses from before are dropped, held keys kept.
  reset() {
    this.pending.length = 0;
    this.lastClick = null;
    this.cursor.x = this.width / 2;
    this.cursor.y = this.height() / 2;
  }

  get active() {
    return this.model.isOpen;
  }

  // A key went down (repeat: the browser's auto-repeat) or up. Keys are
  // tracked even while the board can't act on them (act false), so Tab held
  // as it opens counts. Returns whether the board used it.
  key(code, down, repeat = false, act = true) {
    if (!down) {
      this.held.delete(code);
      return false;
    }
    this.held.add(code);
    if (!act || !this.active) return false;
    if (code in NAV) this.pending.push({ nav: NAV[code] });
    else if (ENTER.has(code) && !repeat) this.click(this.cursor.x, this.cursor.y);
    return SWALLOW.has(code);
  }

  // The pointer, in interface units: where it is, or how far it moved (a
  // captured mouse has no place of its own).
  move(x, y) {
    this.cursor.x = Math.min(Math.max(x, 0), this.width);
    this.cursor.y = Math.min(Math.max(y, 0), this.height());
  }

  nudge(dx, dy) {
    this.move(this.cursor.x + dx, this.cursor.y + dy);
  }

  // Mouse buttons: the left one clicks at the pointer.
  button(index, down) {
    if (!down || !this.active || index !== 0) return false;
    this.click(this.cursor.x, this.cursor.y);
    return true;
  }

  // A left click, tested against the hotspots. Buttons act at once, from
  // inside the event (a restart, a sound); the rest waits for the frame.
  click(x, y) {
    const target = this.hit(x, y);
    if (!target) return null;
    if (target.kind === "row") {
      const last = this.lastClick;
      const again = last && last.side === target.side && last.index === target.index && this.time - last.time < DOUBLE_CLICK;
      this.lastClick = again ? null : { side: target.side, index: target.index, time: this.time };
      this.pending.push({ target });
      if (again) this.act("details", target);
    } else if (target.kind === "sort" || target.kind === "scroll") {
      this.pending.push({ target });
    } else {
      this.act(target.kind, target);
    }
    return target;
  }

  // Once per frame (dt in seconds): the presses since the last, in order,
  // then the row under the pointer.
  update(dt) {
    this.time += dt;
    const model = this.model;
    for (const { nav, target } of this.pending.splice(0)) {
      if (!this.active) break;
      model.touch();
      if (nav) model[nav[0]](nav[1]);
      else if (target.kind === "sort") model.sortBy(target.mode);
      else if (target.kind === "scroll") model.scroll(target.step);
      else if (target.kind === "row") model.select(target.side, target.index);
    }
    const under = this.active ? this.hit(this.cursor.x, this.cursor.y) : null;
    model.hover = under?.kind === "row" ? { side: under.side, index: under.index } : null;
  }
}

// The page's events into a ScoreboardInput: keys on the window, buttons and
// the pointer over the canvas. While the board is up the game's own
// controls are off and the page's cursor is hidden over the canvas, where
// the board draws its own; a captured mouse stays captured and moves that
// pointer by how far it moves.
//
// toInterface(x, y): canvas pixels to interface units (BattleHud); scale():
// interface units per canvas pixel, { x, y }; isOpen(): whether the board
// wants input; menuOpen(): whether the pause menu covers it;
// tab(down, event): Tab pressed or let go while the board is closed;
// escape(): Esc on the board.
export function bindDom({ input, element, toInterface, scale, isOpen, menuOpen = () => false, tab = () => {}, escape = () => {}, target = window }) {
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
  const locked = () => typeof document !== "undefined" && document.pointerLockElement === element;
  // The menu closes itself on Esc; noted first, so the same press doesn't
  // also act here.
  listen(target, "keydown", (event) => {
    if (event.code === "Escape") escapeWithMenu = menuOpen();
  }, true);
  listen(target, "keydown", (event) => {
    const acting = isOpen() && !menuOpen();
    if (event.code === "Escape") {
      if (acting && !escapeWithMenu) escape();
      return;
    }
    if (event.code === "Tab" && !isOpen() && !event.repeat) tab(true, event);
    if (input.key(event.code, true, event.repeat, acting)) event.preventDefault();
  });
  listen(target, "keyup", (event) => input.key(event.code, false));
  listen(target, "blur", () => input.held.clear());
  const pointer = typeof document !== "undefined" ? document : element;
  listen(pointer, "mousemove", (event) => {
    if (!isOpen()) return;
    // The browser sends a move that didn't move when it lets go of a
    // captured mouse (at the window's corner): not the player's.
    if (event.movementX === 0 && event.movementY === 0) return;
    if (locked()) {
      const k = scale();
      input.nudge(event.movementX * k.x, event.movementY * k.y);
    } else if (event.target === element || element.contains?.(event.target)) {
      const point = local(event);
      input.move(point.x, point.y);
    }
  });
  listen(pointer, "mousedown", (event) => {
    if (!isOpen() || menuOpen()) return;
    if (!locked()) {
      if (event.target !== element && !element.contains?.(event.target)) return;
      const point = local(event);
      input.move(point.x, point.y);
    }
    if (input.button(event.button, true)) event.preventDefault();
  });
  return () => listeners.forEach((remove) => remove());
}
