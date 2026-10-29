// The widgets menu screens are built from: what each holds and does. How
// each looks is a theme's (themes/classic draws them as the 2004 interface
// does), so another theme can draw the same widgets differently.
//
// Every widget has a kind (the theme's name for it), an optional id, and:
// - items(): its focusable items (row indices; [0] for a single item);
// - enabled(item), and hidden;
// - ring: whether Up and Down reach its items (false: only the pointer);
// - activate(item, screen): Enter or a click; step(item, delta, screen):
//   Left or Right; key(code, item, screen): keys it takes while focused;
// - press(target, screen) and drag(target, x, y, screen): the pointer held
//   on it (a slider); wheel(delta, target, screen); hover(target, screen).
// A target is { widget, item } plus what the theme's hotspot adds (a
// list's row, a slider's track).

const resolve = (value, ...args) => (typeof value === "function" ? value(...args) : value);
const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

export class Widget {
  constructor(kind, { id = null, hidden = false, ring = true, disabled = false, ...rest } = {}) {
    Object.assign(this, rest);
    this.kind = kind;
    this.id = id;
    this.hidden = hidden;
    this.ring = ring;
    this.disabled = disabled;
  }

  items() {
    return [0];
  }

  enabled() {
    return !resolve(this.disabled);
  }
}

// The menu rows under an orange title bar, stacked into an arrow: rows
// [{ id, label, hidden, disabled, action(screen) }];
// layout: the original's AddVerticalButtons layout (font, titleFont,
// width, ySpacing, yHeight, yTop) and its container (at: the safe area's
// [a, b], or the screen's with safe: false; y, and x added to the shift
// the original gives it, or containerX in its place; rotX, rotY, rotZ in
// degrees, turned in `order`, "YXZ" by default). listCount: how many rows
// the original's list has, hidden ones included (they still widen its
// container's shift).
export class Rows extends Widget {
  constructor(options) {
    super("rows", options);
    this.rows = this.rows || [];
    this.layout = { font: "gamefont_medium", width: 250, ySpacing: 5, rotX: 0, rotY: 0, rotZ: 0, at: [0.5, 0.5], x: 0, y: 0, ...this.layout };
  }

  visibleRows() {
    return this.rows.map((row, index) => ({ row, index })).filter(({ row }) => !resolve(row.hidden));
  }

  items() {
    return this.visibleRows().map(({ index }) => index);
  }

  enabled(item) {
    return !resolve(this.disabled) && !resolve(this.rows[item]?.disabled);
  }

  label(item) {
    return String(resolve(this.rows[item]?.label) ?? "");
  }

  activate(item, screen) {
    const row = this.rows[item];
    if (!row) return false;
    screen.sound(row.sound ?? "accept");
    row.action?.(screen, row);
    return true;
  }
}

// One option row, of an Options widget:
// - kind "choice": choices [[value, label]], cycled by Left / Right, Enter
//   or a click (wrapping);
// - "toggle": on or off (labels: [off, on]);
// - "slider": min, max, step, its value shown by format(value);
// - "action": action(screen) on Enter or a click, its text in display;
// - "fixed": a value shown (display), not changed.
// get() and set(value) read and write it. Its label, display, hidden and
// disabled may be functions. note: a line shown while it is focused (how a
// change applies).
export class OptionRow {
  constructor(options) {
    Object.assign(this, { kind: "choice", choices: [], min: 0, max: 1, step: 0.05, ...options });
  }

  value() {
    return this.get?.();
  }

  // The value column's text.
  text() {
    const value = this.value();
    if (this.kind === "action" || this.kind === "fixed") return String(resolve(this.display, value) ?? "");
    if (this.kind === "toggle") return String((this.labels || ["OFF", "ON"])[value ? 1 : 0]);
    if (this.kind === "slider") return this.format ? this.format(value) : `${Math.round(this.fraction() * 100)}%`;
    const choice = this.choices.find(([v]) => v === value || String(v) === String(value));
    return String(choice ? choice[1] : value ?? "");
  }

  // A slider's place along its track, 0..1.
  fraction() {
    return clamp((Number(this.value()) - this.min) / Math.max(this.max - this.min, 1e-9), 0, 1);
  }

  setFraction(f) {
    const raw = this.min + clamp(f, 0, 1) * (this.max - this.min);
    return this.change(this.snap(raw));
  }

  snap(v) {
    const step = this.step || 0;
    const snapped = step ? this.min + Math.round((v - this.min) / step) * step : v;
    return Number(clamp(snapped, this.min, this.max).toFixed(6));
  }

  // Set it; whether the value changed.
  change(value) {
    if (this.value() === value) return false;
    this.set?.(value);
    return true;
  }

  // Left (-1) or Right (+1).
  stepBy(delta) {
    if (this.kind === "toggle") return this.change(!this.value());
    if (this.kind === "slider") return this.change(this.snap(Number(this.value()) + delta * (this.step || 0.05)));
    if (this.kind !== "choice" || !this.choices.length) return false;
    const at = this.choices.findIndex(([v]) => v === this.value() || String(v) === String(this.value()));
    const next = this.choices[((at < 0 ? 0 : at + delta) + this.choices.length) % this.choices.length];
    return this.change(next[0]);
  }
}

// Two columns, label | value, as the original's option pages: rows are
// OptionRows; layout names the page's look to the
// theme ("game", "video", "audio", "controls"). Rows that don't fit scroll,
// following the focus: first is the top row shown, and the theme sets
// shown to how many fit.
export class Options extends Widget {
  constructor(options) {
    super("options", options);
    this.rows = (this.rows || []).map((row) => (row instanceof OptionRow ? row : new OptionRow(row)));
    this.layout ??= "game";
    this.first = 0;
    this.shown = Infinity;
  }

  visible() {
    return this.rows.map((row, index) => ({ row, index })).filter(({ row }) => !resolve(row.hidden));
  }

  items() {
    return this.visible().map(({ index }) => index);
  }

  enabled(item) {
    const row = this.rows[item];
    return Boolean(row) && !resolve(this.disabled) && !resolve(row.disabled) && row.kind !== "fixed";
  }

  focus(item) {
    this.reveal(item);
  }

  // Scroll so the row is in view.
  reveal(item) {
    const order = this.items();
    const at = order.indexOf(item);
    if (at < 0 || !Number.isFinite(this.shown)) return;
    if (at < this.first) this.first = at;
    else if (at >= this.first + this.shown) this.first = at - this.shown + 1;
  }

  scrollBy(delta) {
    const count = this.items().length;
    const max = Number.isFinite(this.shown) ? Math.max(count - this.shown, 0) : 0;
    const next = clamp(this.first + delta, 0, max);
    if (next === this.first) return false;
    this.first = next;
    return true;
  }

  activate(item, screen) {
    const row = this.rows[item];
    if (!row) return false;
    if (row.kind === "action") {
      screen.sound(row.sound ?? "accept");
      row.action?.(screen, row);
      return true;
    }
    if (row.kind === "slider") return false;
    if (row.stepBy(1)) screen.sound("accept");
    return true;
  }

  step(item, delta, screen) {
    const row = this.rows[item];
    if (!row || row.kind === "action") return false;
    if (row.stepBy(delta)) screen.sound("accept");
    return true;
  }

  // The pointer down on a row: a slider's track sets it there (and drags);
  // anything else acts as Enter.
  press(target, screen) {
    const row = this.rows[target.item];
    if (row?.kind === "slider" && target.track) {
      this.drag(target, screen.stack?.pointer.x ?? 0, 0, screen);
      return;
    }
    screen.accept(target);
  }

  drag(target, x, y, screen) {
    const row = this.rows[target.item];
    const track = target.track;
    if (row?.kind !== "slider" || !track) return;
    if (row.setFraction((x - track.x0) / Math.max(track.x1 - track.x0, 1e-6))) screen.sound("accept");
  }

  wheel(delta) {
    return this.scrollBy(delta);
  }
}

// A free-standing text button: BACK at the bottom left, an accept-type
// (LAUNCH, OK) at the bottom right, a middle one (RESET TO DEFAULTS) at the
// bottom centre. BACK is off the Up / Down ring
// (Esc is its key); the others are on it, after the rows.
export class Button extends Widget {
  constructor(options) {
    super("button", { place: "back", ...options });
    if (options?.ring === undefined) this.ring = this.place !== "back";
  }

  label() {
    return String(resolve(this.text) ?? "");
  }

  activate(item, screen) {
    if (this.place === "back" && !this.action) return screen.back();
    screen.sound(this.sound ?? "accept");
    this.action?.(screen);
    return true;
  }
}

// A screen's title: big text at the top centre (an option page's), or
// anything else static: lines of text in a font at a place the theme
// knows (look).
export class Text extends Widget {
  constructor(options) {
    super("text", { ring: false, ...options });
  }

  items() {
    return [];
  }

  lines() {
    return [].concat(resolve(this.text) ?? []).flatMap((line) => String(line).split(/\r?\n/));
  }
}

// A window (border_3_pieces nine-sliced), with an optional title tab: box
// { x, y, w, h } (centre and size, interface units) in the frame the
// theme's look gives it.
export class Panel extends Widget {
  constructor(options) {
    super("panel", { ring: false, ...options });
  }

  items() {
    return [];
  }
}

// A list box: rows, each a list of column texts;
// the selected row's frame and the hovered row's highlight; a scroll thumb
// when it doesn't all fit. Up / Down move the selection (handing the focus
// on at either end), Page Up / Page Down by half a view, the wheel a row a
// notch; a click selects, a second within 0.4 s accepts (onAccept).
// shown: how many rows fit (the theme sets it).
export const DOUBLE_CLICK = 0.4; // s

export class List extends Widget {
  constructor(options) {
    super("list", options);
    this.rows ??= [];
    this.selected ??= 0;
    this.first = 0;
    this.shown = 10;
    this.hoverRow = null;
    this.lastClick = null;
  }

  rowsNow() {
    return resolve(this.rows) || [];
  }

  select(index, screen) {
    const count = this.rowsNow().length;
    if (!count) return false;
    const next = clamp(index, 0, count - 1);
    if (next === this.selected) return false;
    this.selected = next;
    this.reveal();
    this.onSelect?.(next, screen);
    screen?.sound("select");
    return true;
  }

  reveal() {
    if (this.selected < this.first) this.first = this.selected;
    else if (this.selected >= this.first + this.shown) this.first = this.selected - this.shown + 1;
    this.first = clamp(this.first, 0, Math.max(this.rowsNow().length - this.shown, 0));
  }

  key(code, item, screen) {
    const count = this.rowsNow().length;
    if (code === "ArrowUp") return this.selected > 0 ? (this.select(this.selected - 1, screen), true) : false;
    if (code === "ArrowDown") return this.selected < count - 1 ? (this.select(this.selected + 1, screen), true) : false;
    if (code === "PageUp" || code === "PageDown") {
      const step = Math.max(Math.floor(this.shown / 2), 1);
      this.select(this.selected + (code === "PageUp" ? -step : step), screen);
      return true;
    }
    return false;
  }

  activate(item, screen) {
    if (!this.onAccept) return false;
    screen.sound("accept");
    this.onAccept(this.selected, screen);
    return true;
  }

  hover(target) {
    this.hoverRow = target?.widget === this && Number.isInteger(target.row) ? target.row : null;
  }

  press(target, screen) {
    if (target.part === "track") {
      this.first = clamp(Math.round(target.fraction * this.rowsNow().length - this.shown / 2), 0, Math.max(this.rowsNow().length - this.shown, 0));
      return;
    }
    if (!Number.isInteger(target.row)) return;
    const now = screen.stack?.time ?? 0;
    const again = this.lastClick && this.lastClick.row === target.row && now - this.lastClick.time < DOUBLE_CLICK;
    this.lastClick = again ? null : { row: target.row, time: now };
    this.select(target.row, screen);
    if (again) this.activate(0, screen);
  }

  wheel(delta, target, screen) {
    this.select(this.selected + delta, screen);
    return true;
  }
}

// Header-bar tabs along the top of a window (the controls page's
// INFANTRY / VEHICLE / STARFIGHTER): tabs [{ id, label }], active its index;
// onChange(index, screen). Left / Right while one is focused step through
// them.
export class Tabs extends Widget {
  constructor(options) {
    super("tabs", { active: 0, ring: false, ...options });
  }

  items() {
    return this.tabs.map((_, index) => index);
  }

  choose(index, screen) {
    const next = (index + this.tabs.length) % this.tabs.length;
    if (next === this.active) return false;
    this.active = next;
    screen?.sound("accept");
    this.onChange?.(next, screen);
    return true;
  }

  activate(item, screen) {
    this.choose(item, screen);
    return true;
  }

  step(item, delta, screen) {
    this.choose(this.active + delta, screen);
    return true;
  }
}

// Buttons side by side (the original's AddHorizontalButtons): a popup's
// YES / NO. buttons [{ id, label, action(screen), sound }]; Left and Right
// move between them as Up and Down do.
export class HorizontalButtons extends Widget {
  constructor(options) {
    super("hbuttons", options);
    this.buttons ??= [];
  }

  items() {
    return this.buttons.map((_, index) => index);
  }

  label(item) {
    return String(resolve(this.buttons[item]?.label) ?? "");
  }

  key(code, item, screen) {
    if (code !== "ArrowLeft" && code !== "ArrowRight") return false;
    screen.move(code === "ArrowLeft" ? -1 : 1);
    return true;
  }

  activate(item, screen) {
    const button = this.buttons[item];
    if (!button) return false;
    screen.sound(button.sound ?? "accept");
    button.action?.(screen);
    return true;
  }
}
