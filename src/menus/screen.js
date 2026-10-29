import { TitleIntro } from "../interface/flashy.js";

// A menu screen: its widgets (widgets.js) in drawing order, which one has
// the focus, and what its keys do. A ScreenStack (stack.js) holds screens
// and gives the top one the input; a theme draws them.
//
// Focus, as the 2004 interface keeps it:
// - one focused item at a time, drawn lit and pulsing; hover and keyboard
//   focus are the same state;
// - Up and Down walk the screen's ring (the focusable items of its widgets,
//   in order), wrapping; with nothing focused either key goes to the first;
// - every pointer move gives the focus to whatever is under the pointer, or
//   to nothing over empty space;
// - Enter (or a click) accepts the focused item, Left and Right change its
//   value, Esc goes back.
// The focused widget sees a key first (a list moves its own selection).

export const FADE_IN = 0.4; // s: a screen's items fade in (gShellAnimTime)

const same = (a, b) => Boolean(a && b && a.widget === b.widget && a.item === b.item);

export class MenuScreen {
  // id: a name for checks and the stack; widgets: drawn in order; back:
  // what Esc and BACK do ("pop", null for nothing, or a function);
  // focus: where it is on entry ("first", or "keep" to come back to the
  // item left from); dim: the world (or movie) dimmed behind it, as the
  // option pages do; popup: a modal screen drawn over the one under it,
  // whose own lists and rows then hide. onEnter({ forward }), onExit() and
  // onKey(code) are hooks; anything else given is kept on the screen.
  constructor({ id = "screen", widgets = [], back = "pop", focus = "first", dim = false, popup = false, onEnter = null, onExit = null, onKey = null, ...rest } = {}) {
    Object.assign(this, rest);
    this.id = id;
    this.widgets = [];
    this.backAction = back;
    this.focusMode = focus;
    this.dim = dim;
    this.popup = popup;
    this.onEnter = onEnter;
    this.onExit = onExit;
    this.onKey = onKey;
    this.focused = null; // { widget, item }
    this.time = 0; // s since it was last entered
    this.intros = new Map();
    this.stack = null;
    for (const widget of widgets) this.add(widget);
  }

  add(widget) {
    widget.screen = this;
    this.widgets.push(widget);
    return widget;
  }

  widget(id) {
    return this.widgets.find((widget) => widget.id === id) || null;
  }

  // The focusable items, in ring order.
  ring() {
    const targets = [];
    for (const widget of this.widgets) {
      if (widget.hidden || widget.ring === false) continue;
      for (const item of widget.items()) if (widget.enabled(item)) targets.push({ widget, item });
    }
    return targets;
  }

  // The screen comes up: forward (pushed) or back (the screen over it
  // popped). Its texts play their intro again and its items fade in.
  enter({ forward = true } = {}) {
    this.time = 0;
    this.intros.clear();
    if (this.focusMode !== "keep" || !this.focused || !this.ring().some((t) => same(t, this.focused))) this.focused = this.ring()[0] ?? null;
    for (const widget of this.widgets) widget.enter?.({ forward });
    this.onEnter?.({ forward });
  }

  exit() {
    for (const widget of this.widgets) widget.exit?.();
    this.onExit?.();
  }

  update(dt) {
    this.time += dt;
    for (const intro of this.intros.values()) intro.update(dt);
    for (const widget of this.widgets) widget.update?.(dt);
  }

  // A text's intro, made the first time it is drawn after the screen came
  // up, starting `delay` seconds after that.
  intro(key, delay = 0) {
    let intro = this.intros.get(key);
    if (!intro) {
      intro = new TitleIntro();
      intro.start(delay - this.time);
      intro.update(0);
      this.intros.set(key, intro);
    }
    return intro;
  }

  // How far the items have faded in, 0..1.
  get shown() {
    return Math.min(this.time / FADE_IN, 1);
  }

  isFocused(widget, item) {
    return same(this.focused, { widget, item });
  }

  // source: "key" or "pointer" (a sound on a change either way, but only to
  // something).
  setFocus(target, source = "key") {
    const next = target && !target.widget.hidden && target.widget.enabled(target.item) ? { widget: target.widget, item: target.item } : null;
    if (same(next, this.focused) || (!next && !this.focused)) return false;
    this.focused?.widget.blur?.(this.focused.item);
    this.focused = next;
    if (next) {
      next.widget.focus?.(next.item, source);
      this.sound("select");
    }
    return true;
  }

  // Up / Down through the ring.
  move(delta) {
    const ring = this.ring();
    if (!ring.length) return false;
    const at = this.focused ? ring.findIndex((t) => same(t, this.focused)) : -1;
    const next = at < 0 ? ring[0] : ring[(at + delta + ring.length) % ring.length];
    if (same(next, this.focused)) return false;
    this.focused?.widget.blur?.(this.focused.item);
    this.focused = next;
    next.widget.focus?.(next.item, "key", delta);
    this.sound("select");
    return true;
  }

  // A key, from the stack. Returns whether it was used.
  key(code) {
    const target = this.focused;
    if (target && target.widget.key?.(code, target.item, this)) return true;
    if (this.onKey?.(code)) return true;
    switch (code) {
      case "ArrowUp": return this.move(-1) || true;
      case "ArrowDown": return this.move(1) || true;
      case "ArrowLeft":
      case "ArrowRight":
        if (target) this.step(target, code === "ArrowLeft" ? -1 : 1);
        return true;
      case "Enter":
      case "NumpadEnter":
        if (target) this.accept(target);
        return true;
      case "Escape":
        this.back();
        return true;
    }
    return false;
  }

  // Enter or a click on an item.
  accept(target) {
    if (!target || !target.widget.enabled(target.item)) return false;
    return target.widget.activate?.(target.item, this) ?? false;
  }

  // Left or Right on an item.
  step(target, delta) {
    if (!target || !target.widget.enabled(target.item)) return false;
    return target.widget.step?.(target.item, delta, this) ?? false;
  }

  // Esc or BACK.
  back() {
    const action = this.backAction;
    if (!action) return false;
    if (typeof action === "function") action(this);
    else this.stack?.pop();
    return true;
  }

  sound(kind) {
    this.stack?.sound(kind);
  }
}
