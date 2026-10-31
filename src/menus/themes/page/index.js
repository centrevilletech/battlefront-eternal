import { STYLE } from "./style.js";
import { THEMES, DEFAULT_THEME, themeOf } from "./looks.js";

// The Page theme: the menus as a page of their own over the front end's
// backdrop, in one of three looks that share one layout and change only
// colours and surfaces (THEMES): bf1, the 2004 front end's blue hologram
// with orange for the choice; bf2, the 2005 front end's grey metal on a
// blueprint with red for the choice; eternal, this port's own black glass
// and amber. The player's menuTheme setting picks one (for now a dropdown
// at the top right sets it).
//
// It shows the same screens and widgets the Classic theme draws
// (widgets.js) and leaves what they do to them: the menus' keys still go to
// the stack (bindMenuInput with pointer: false), and the pointer works on
// the elements here through the screens' own setFocus, accept and step,
// with their sounds. render(stack), every frame, keeps the page in step:
// the focus, values, labels, rows that came or went, the screen up and a
// popup over it.
//
// Where a screen's widgets go: its layout (layouts, by screen id) says
// home (the logo over its rows: the main menu), columns (widget ids, or
// "#name" for an element of the layout's slots, side by side: Instant
// Action), kind (the page's arrangement: "rows", "options", "wide"), hide
// (widget ids the page leaves out) and dim (the backdrop darkened behind
// it, as for a screen marked dim); otherwise by kind. The title goes at the top (a rows widget's own when
// the screen has no title text), the rows, option rows, lists and tabs
// down the middle, BACK at the bottom left, a middle button in the middle,
// an accept button on the right, and a popup in a window over the screen
// under it. A kind the page doesn't know is drawn by views[kind]
// ((widget, screenView) => { el, update(live), dispose() }).
//
// Moving between screens (render): the screen left cascades out (its
// chosen button flashes last) while the next cascades in from the side it
// is reached from, backwards going back; bottom buttons both screens have
// stay put; the logo flies between the main menu's middle and the corner;
// the backdrop drifts deeper the further in the player goes. onMove({
// forward, from, to }) hears each move (the shell's whoosh).
//
// Over a battle (overlay: the pause menu, src/pause) there is no backdrop:
// the frozen world shows through, darkened and blurred, and the page is
// gone (it lets the pointer through) while no screen is up, but for a
// line it may show (prompt(): CLICK TO RESUME).

export { THEMES, DEFAULT_THEME, themeOf };
export const GATE_TEXT = "PRESS ANY KEY OR CLICK TO BEGIN";

const LEAVE = 0.45; // s: about how long a screen takes to go
const ENTER = 0.95; // s: about how long a screen's entrance takes
const FLIGHT = 0.62; // s: the logo's flight to or from the corner
// A focus the pointer moved is never scrolled to (the pointer is on it).
const POINTER_FOCUS = 0.15; // s

const resolve = (value, ...args) => (typeof value === "function" ? value(...args) : value);
// A widget's look may be worked out from the interface surface; a list's
// columns are all the page takes from it.
const SURFACE = { width: 1024, height: 768 };

function make(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// A button that never takes the keyboard focus (the menus' Enter would
// press it again). Its label is kept in lower case for the looks that set
// it in title case (bf2); the others show capitals.
function button(text, className) {
  const node = make("button", className);
  node.type = "button";
  node.tabIndex = -1;
  node.append(make("span", "label", String(text).toLowerCase()));
  node.addEventListener("mousedown", (event) => event.preventDefault());
  return node;
}

function setLabel(node, text) {
  const label = node.querySelector(".label");
  const value = String(text).toLowerCase();
  if (label.textContent !== value) label.textContent = value;
}

// BATTLEFRONT over the ETERNAL bar, in the look's colours.
export function logo() {
  const node = make("div", "bfe-logo");
  node.setAttribute("role", "img");
  node.setAttribute("aria-label", "Battlefront Eternal");
  const word = make("span", "word", "Battlefront");
  word.dataset.text = "Battlefront";
  node.append(word, make("span", "tag", "Eternal"));
  return node;
}

// Scroll the nearest scrolling box just enough to show the node.
function reveal(node) {
  for (let box = node.parentElement; box && box !== document.body; box = box.parentElement) {
    if (!/(auto|scroll)/.test(getComputedStyle(box).overflowY)) continue;
    const r = node.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    if (r.top < b.top) box.scrollTop -= b.top - r.top + 6;
    else if (r.bottom > b.bottom) box.scrollTop += r.bottom - b.bottom + 6;
    return;
  }
}

// When a node's own animations are done (a slow frame can start them
// late, so not after a set time), or `latest` seconds on at the latest.
function settled(node, latest, done) {
  let called = false;
  const once = () => {
    if (called) return;
    called = true;
    done();
  };
  requestAnimationFrame(() => {
    const running = node.getAnimations({ subtree: true }).filter((animation) => animation.effect?.getComputedTiming?.().iterations !== Infinity);
    Promise.all(running.map((animation) => animation.finished.catch(() => {}))).then(once);
  });
  setTimeout(once, latest * 1000);
}

function leave(node, after = LEAVE) {
  node.classList.add("leaving");
  node.classList.remove("entering");
  settled(node, after + 1, () => node.remove());
}

// The labels in a bottom slot, to know one both screens have.
const slotText = (slot) => [...(slot?.querySelectorAll(".bfe-btn") || [])].map((node) => node.textContent).join("\u0001");

export class PageTheme {
  // parent: where the page goes; backdrop: an element behind the screens
  // (the front end's movies); settings: the player's (menuTheme);
  // layouts: by screen id (above); icons: list icons' names to their short
  // labels.
  constructor({ parent = document.body, backdrop = null, settings = null, layouts = {}, icons = {}, views = {}, onMove = null, gateText = GATE_TEXT, overlay = false } = {}) {
    addStyle();
    this.name = "page";
    this.overlay = overlay;
    this.settings = settings;
    this.layouts = layouts;
    this.icons = icons;
    this.views = views;
    this.onMove = onMove;
    this.gateText = gateText;
    this.depth = -1;
    this.base = null; // the screen up: a ScreenView
    this.popup = null; // a popup over it
    this.gateView = null;
    this.pointerAt = -Infinity;
    const root = (this.root = make("div", "bfe-page"));
    const back = make("div", "bfe-page-back");
    if (backdrop) back.append(backdrop);
    this.stage = make("div", "bfe-page-stage");
    const brand = (this.brand = make("div", "bfe-page-brand"));
    brand.append(logo());
    const picker = make("label", "bfe-page-theme");
    const select = (this.select = make("select"));
    for (const theme of THEMES) select.add(new Option(theme.label, theme.id));
    select.addEventListener("change", () => {
      this.setTheme(select.value);
      select.blur();
    });
    // Its own keys, not the menus'.
    select.addEventListener("keydown", (event) => event.stopPropagation());
    picker.append(make("span", "", "Theme"), select);
    this.line = make("p", "bfe-page-prompt");
    root.classList.toggle("overlay", overlay);
    root.classList.toggle("empty", overlay);
    root.append(back, make("div", "bfe-page-scrim"), make("div", "bfe-page-dim"), make("div", "bfe-page-pattern"), brand, this.stage, this.line, picker);
    this.setTheme(themeOf(settings?.get?.("menuTheme")), { save: false });
    parent.append(root);
  }

  // The ScreenStack's theme calls: drawing is rendering the page (the
  // pause menu's HUD overlay draws through it), and the page takes the
  // pointer on its own elements.
  draw(stack) {
    this.render(stack);
  }

  hit() {
    return null;
  }

  setTheme(id, { save = true } = {}) {
    const theme = themeOf(id);
    // A new look comes in at once, not through the buttons' transitions.
    this.root.classList.add("instant");
    document.documentElement.dataset.bfeTheme = theme;
    this.select.value = theme;
    requestAnimationFrame(() => requestAnimationFrame(() => this.root.classList.remove("instant")));
    if (save) this.settings?.set?.("menuTheme", theme);
  }

  // The first screen: the logo and the line to press, before the menus.
  gate(on = true) {
    if (on && !this.gateView && !this.base) {
      const view = make("div", "bfe-screen bfe-gate");
      view.dataset.layout = "home";
      const head = make("header", "bfe-screen-head");
      head.append(logo());
      view.append(head, make("p", "bfe-gate-line", this.gateText));
      this.stage.append(view);
      this.gateView = view;
      this.root.classList.add("home");
    } else if (!on && this.gateView) {
      this.gateView.dataset.dir = "forward";
      leave(this.gateView);
      this.gateView = null;
    }
  }

  layoutOf(screen) {
    return this.layouts[screen.id] || {};
  }

  // A line in the middle of the page, pulsing, with nothing else up (the
  // pause menu's CLICK TO RESUME while it waits for the mouse); null: none.
  prompt(text) {
    if (this.line.textContent !== (text || "")) this.line.textContent = text || "";
    this.root.classList.toggle("prompting", Boolean(text));
  }

  // The stack's screens onto the page, every frame.
  render(stack) {
    const visible = stack.visible;
    const top = visible.at(-1) || null;
    const popup = top?.popup ? top : null;
    const base = popup ? (visible.length > 1 ? visible[0] : null) : top;
    if (base !== (this.base?.screen ?? null)) this.move(stack, base);
    if (popup !== (this.popup?.screen ?? null)) {
      if (this.popup) {
        this.popup.dispose();
        leave(this.popup.el, 0.24);
      }
      this.popup = popup ? new ScreenView(this, popup) : null;
      if (this.popup) this.stage.append(this.popup.el);
    }
    const kind = this.base?.kind ?? (this.gateView ? "home" : null);
    this.root.classList.toggle("empty", !this.base && !this.popup && !this.gateView);
    this.root.classList.toggle("home", kind === "home");
    this.root.classList.toggle("wide", kind === "wide" || kind === "options");
    this.root.classList.toggle("dim", Boolean(base?.dim || this.base?.layout.dim));
    // How deep the player is: the backdrop drifts in a step a screen.
    const depth = Math.max(stack.screens.filter((screen) => !screen.popup).length - 1, 0);
    if (depth !== this.depth) {
      this.depth = depth;
      this.root.style.setProperty("--depth", String(Math.min(depth, 4)));
    }
    this.base?.update(!popup);
    this.popup?.update(true);
  }

  // Another screen up: the one left goes, the next comes in.
  move(stack, screen) {
    const old = this.base;
    const gate = this.gateView;
    // Forward when the screen left is still on the stack, under this one.
    const forward = !old || stack.screens.includes(old.screen);
    const dir = forward ? "forward" : "back";
    this.gateView = null;
    const next = (this.base = screen ? new ScreenView(this, screen) : null);
    if (next) {
      next.el.dataset.dir = dir;
      next.el.classList.add("entering");
      this.stage.append(next.el);
      settled(next.el, ENTER + 1, () => next.el.classList.remove("entering"));
    }
    if (gate) {
      gate.dataset.dir = dir;
      leave(gate);
    }
    let flies = false;
    if (old) {
      old.el.dataset.dir = dir;
      this.steady(old, next);
      old.dispose();
    }
    flies = this.fly(old, next);
    if (old) leave(old.el, flies ? Math.max(FLIGHT, LEAVE) : LEAVE);
    if (old || gate) this.onMove?.({ forward, from: old?.screen ?? null, to: screen });
  }

  // Bottom buttons both screens have stay where they are.
  steady(old, next) {
    if (!next) return;
    for (const slot of ["left", "middle", "right"]) {
      const a = old.el.querySelector(`.bfe-screen-foot > .${slot}`);
      const b = next.el.querySelector(`.bfe-screen-foot > .${slot}`);
      const text = slotText(a);
      if (!text || text !== slotText(b)) continue;
      a.style.visibility = "hidden";
      b.classList.add("steady");
    }
  }

  // The logo's flight: from the main menu's middle to the corner going in,
  // back again coming out. The big logo is the one that flies (drawn at its
  // own size, it stays sharp); the corner's shows when it lands. Returns
  // whether the screen left has a logo in flight (it stays a moment longer).
  fly(old, next) {
    const brand = this.brand.querySelector(".bfe-logo");
    const ease = `transform ${FLIGHT}s cubic-bezier(.7, 0, .25, 1)`;
    const map = (a, b) => `translate(${b.left - a.left}px, ${b.top - a.top}px) scale(${b.width / Math.max(a.width, 1)})`;
    if (old?.kind === "home" && next && next.kind !== "home") {
      const logo = old.el.querySelector(".bfe-screen-head .bfe-logo");
      if (!logo) return false;
      const from = logo.getBoundingClientRect();
      const to = brand.getBoundingClientRect();
      logo.style.transformOrigin = "0 0";
      logo.style.transition = ease;
      requestAnimationFrame(() => { logo.style.transform = map(from, to); });
      // The corner's logo shows as the big one lands.
      clearTimeout(this.landing);
      const land = () => {
        clearTimeout(this.landing);
        if (this.base === next) this.brand.classList.add("on");
      };
      logo.addEventListener("transitionend", land, { once: true });
      this.landing = setTimeout(land, (FLIGHT + 0.6) * 1000);
      return true;
    }
    if (old && old.kind !== "home" && next?.kind === "home") {
      const logo = next.el.querySelector(".bfe-screen-head .bfe-logo");
      if (logo) {
        const from = brand.getBoundingClientRect();
        const to = logo.getBoundingClientRect();
        logo.style.transformOrigin = "0 0";
        logo.style.transition = "none";
        logo.style.transform = map(to, from);
        void logo.offsetWidth;
        logo.style.transition = ease;
        logo.style.transform = "";
      }
      clearTimeout(this.landing);
      this.brand.classList.remove("on");
      return false;
    }
    if (next) {
      clearTimeout(this.landing);
      this.brand.classList.toggle("on", next.kind !== "home");
    }
    return false;
  }

  dispose() {
    clearTimeout(this.landing);
    this.base?.dispose();
    this.popup?.dispose();
    this.root.remove();
    delete document.documentElement.dataset.bfeTheme;
  }
}

// One screen on the page: its widgets' views, placed.
class ScreenView {
  constructor(theme, screen) {
    this.theme = theme;
    this.screen = screen;
    this.layout = theme.layoutOf(screen);
    this.parts = [];
    if (screen.popup) this.buildPopup();
    else this.buildPage();
  }

  // Whether it takes the pointer: it is the top screen.
  get live() {
    return this.screen.stack?.top === this.screen;
  }

  buildPage() {
    const { screen, layout } = this;
    const kinds = new Set(screen.widgets.map((widget) => widget.kind));
    const el = (this.el = make("div", "bfe-screen"));
    el.dataset.screen = screen.id;
    this.kind = layout.kind || (layout.home ? "home" : layout.columns ? "wide" : kinds.has("options") || kinds.has("tabs") ? "options" : kinds.has("list") ? "wide" : "rows");
    el.dataset.layout = this.kind;
    const head = make("header", "bfe-screen-head");
    const body = make("div", "bfe-screen-body");
    const foot = make("footer", "bfe-screen-foot");
    const corner = make("div", "bfe-screen-corner");
    const slots = { back: make("div", "left"), middle: make("div", "middle"), accept: make("div", "right") };
    foot.append(slots.back, slots.middle, slots.accept);
    el.append(head, body, foot, corner);
    if (layout.home) head.append(logo());
    // The layout's columns: a place for each widget it names, in its order.
    const places = new Map();
    if (layout.columns) {
      const grid = make("div", "bfe-columns");
      layout.columns.forEach((names, i) => {
        const column = make("div", "bfe-column");
        column.dataset.column = String(i);
        for (const name of names) {
          if (name.startsWith("#")) {
            const slot = layout.slots?.[name.slice(1)];
            if (slot) column.append(slot);
            continue;
          }
          const place = make("div");
          places.set(name, place);
          column.append(place);
        }
        grid.append(column);
      });
      body.append(grid);
    }
    let titled = Boolean(layout.home);
    for (const widget of screen.widgets) {
      if (layout.hide?.includes(widget.id)) continue;
      const view = viewOf(widget, this);
      if (!view) continue;
      this.parts.push(view);
      const place = places.get(widget.id);
      if (place) {
        place.replaceWith(view.el);
        continue;
      }
      if (widget.kind === "button") {
        (slots[widget.place] || slots.back).append(view.el);
        continue;
      }
      if (widget.kind === "text") {
        const look = widget.look || "title";
        if (look === "title") {
          if (!layout.home) head.append(view.el);
          titled = true;
          continue;
        }
        if (look === "label" && widget.at) {
          view.corner();
          corner.append(view.el);
          continue;
        }
      }
      body.append(view.el);
    }
    // A screen of rows: its rows' own title.
    const rows = screen.widgets.find((widget) => widget.kind === "rows" && widget.title);
    if (!titled && rows) head.append(titleOf(resolve(rows.title)));
    for (const place of places.values()) place.remove();
    // The focused option's note under the panel, not after its last row: a
    // long page (VIDEO OPTIONS) scrolls, and would hide it there.
    for (const part of this.parts) if (part instanceof OptionsView) body.after(part.note);
  }

  buildPopup() {
    this.kind = "popup";
    const el = (this.el = make("div", "bfe-popup"));
    el.dataset.screen = this.screen.id;
    const box = make("div", "bfe-popup-box");
    el.append(box);
    for (const widget of this.screen.widgets) {
      const view = viewOf(widget, this);
      if (!view) continue;
      this.parts.push(view);
      box.append(view.el);
    }
  }

  update(live) {
    this.el.classList.toggle("inert", !live);
    for (const part of this.parts) part.update(live);
  }

  dispose() {
    for (const part of this.parts) part.dispose?.();
  }

  // --- the pointer, through the screen ------------------------------------

  pointer(target) {
    if (!this.live) return;
    this.theme.pointerAt = performance.now();
    this.screen.setFocus(target, "pointer");
  }

  click(target) {
    if (!this.live) return;
    this.pointer(target);
    this.screen.accept(target);
  }

  step(target, delta) {
    if (!this.live) return;
    this.pointer(target);
    this.screen.step(target, delta);
  }

  bind(node, target) {
    node.addEventListener("pointerenter", () => this.pointer(target));
    node.addEventListener("click", () => this.click(target));
  }

  // A part's focused element changed: scroll to it, unless the pointer
  // moved the focus there.
  focus(part, node) {
    if (node === part.focusNode) return;
    part.focusNode = node;
    if (node && performance.now() - this.theme.pointerAt > POINTER_FOCUS * 1000) reveal(node);
  }
}

function titleOf(text) {
  const node = make("h1", "bfe-text bfe-text-title");
  node.append(make("span", "", String(text ?? "")));
  return node;
}

function viewOf(widget, view) {
  const own = view.theme.views[widget.kind];
  if (own) return own(widget, view);
  switch (widget.kind) {
    case "rows": return new RowsView(widget, view);
    case "options": return new OptionsView(widget, view);
    case "button": return new ButtonView(widget, view);
    case "text": return new TextView(widget, view);
    case "list": return new ListView(widget, view);
    case "tabs": return new TabsView(widget, view);
    case "hbuttons": return new ButtonsView(widget, view);
    default: return null; // a window: the page draws its own
  }
}

// Menu rows: the stacked buttons.
class RowsView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = make("nav", "bfe-rows");
    this.signature = null;
    this.nodes = new Map();
    this.focusNode = null;
    // A scroll bar only where the rows don't fit (a long server list), never
    // for the moment they slide in.
    this.observer = new ResizeObserver(() => this.fit());
    this.observer.observe(this.el);
  }

  fit() {
    const last = this.el.lastElementChild;
    this.el.classList.toggle("scroll", Boolean(last) && last.offsetTop + last.offsetHeight > this.el.clientHeight + 1);
  }

  dispose() {
    this.observer.disconnect();
  }

  update(live) {
    const w = this.widget;
    this.el.hidden = Boolean(w.hidden);
    const rows = w.visibleRows();
    const signature = rows.map(({ index }) => `${index}\u0001${w.label(index)}\u0001${w.enabled(index) ? 1 : 0}`).join("\u0002");
    if (signature !== this.signature) {
      this.signature = signature;
      this.build(rows);
      this.fit();
    }
    let focused = null;
    for (const [index, node] of this.nodes) {
      const on = live && this.view.screen.isFocused(w, index);
      node.classList.toggle("active", on);
      if (on) focused = node;
    }
    this.view.focus(this, focused);
  }

  build(rows) {
    const w = this.widget;
    this.nodes = new Map();
    this.el.replaceChildren(...rows.map(({ row, index }, k) => {
      const node = button(w.label(index), "bfe-btn");
      node.dataset.id = String(row.id ?? index);
      node.style.setProperty("--i", String(k));
      node.classList.toggle("disabled", !w.enabled(index));
      this.view.bind(node, { widget: w, item: index });
      this.nodes.set(index, node);
      return node;
    }));
  }
}

// Option rows: label | value, a value changed with its arrows, a click
// (the next one), Left and Right, or a slider's track.
class OptionsView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = make("div", "bfe-options");
    this.list = make("div", "bfe-options-rows");
    this.note = make("p", "bfe-options-note");
    this.el.append(this.list, this.note);
    this.signature = null;
    this.rows = new Map();
    this.focusNode = null;
  }

  update(live) {
    const w = this.widget;
    const screen = this.view.screen;
    this.el.hidden = Boolean(w.hidden);
    const rows = w.visible();
    const signature = rows.map(({ row, index }) => `${index}\u0001${resolve(row.label) ?? ""}\u0001${row.kind}`).join("\u0002");
    if (signature !== this.signature) {
      this.signature = signature;
      this.build(rows);
    }
    let focused = null;
    for (const [index, parts] of this.rows) {
      const row = w.rows[index];
      const on = live && screen.isFocused(w, index);
      parts.node.classList.toggle("active", on);
      parts.node.classList.toggle("disabled", row.kind !== "fixed" && !w.enabled(index));
      parts.node.classList.toggle("dim", row.kind === "fixed" && Boolean(resolve(row.dim)));
      const text = row.text();
      if (parts.text.textContent !== text) {
        // A new value pops in (not the first one shown).
        if (parts.shown) {
          parts.text.classList.remove("changed");
          void parts.text.offsetWidth;
          parts.text.classList.add("changed");
        }
        parts.text.textContent = text;
      }
      parts.shown = true;
      if (parts.fill) {
        const at = `${(row.fraction() * 100).toFixed(2)}%`;
        parts.fill.style.width = at;
        parts.thumb.style.left = at;
      }
      if (on) focused = parts.node;
    }
    // The focused row's note: how its change applies.
    const row = screen.focused?.widget === w ? w.rows[screen.focused.item] : null;
    const note = row ? resolve(row.note) : null;
    const text = note ? String(note) : "";
    if (this.note.textContent !== text) this.note.textContent = text;
    this.view.focus(this, focused);
  }

  build(rows) {
    this.rows = new Map();
    this.list.replaceChildren(...rows.map(({ row, index }, k) => {
      const target = { widget: this.widget, item: index };
      const node = make("div", "bfe-opt");
      node.style.setProperty("--i", String(k));
      node.dataset.kind = row.kind;
      node.dataset.id = String(row.id ?? row.key ?? index);
      const label = String(resolve(row.label) ?? "");
      if (label) node.append(make("span", "bfe-opt-label", label));
      else node.classList.add("bare");
      const value = make("span", "bfe-opt-value");
      const parts = { node, text: null, fill: null, thumb: null };
      if (row.kind === "choice" || row.kind === "toggle") {
        parts.text = make("span", "bfe-opt-text");
        const arrows = [["prev", "‹", -1], ["next", "›", 1]].map(([name, glyph, delta]) => {
          const arrow = make("button", `bfe-opt-arrow ${name}`, glyph);
          arrow.type = "button";
          arrow.tabIndex = -1;
          arrow.addEventListener("mousedown", (event) => event.preventDefault());
          arrow.addEventListener("click", (event) => {
            event.stopPropagation();
            this.view.step(target, delta);
          });
          return arrow;
        });
        value.append(arrows[0], parts.text, arrows[1]);
      } else if (row.kind === "slider") {
        const track = make("span", "bfe-slider");
        parts.fill = make("span", "bfe-slider-fill");
        parts.thumb = make("span", "bfe-slider-thumb");
        track.append(parts.fill, parts.thumb);
        parts.text = make("span", "bfe-opt-text");
        this.slide(track, target, row);
        value.append(track, parts.text);
      } else {
        parts.text = make("span", row.kind === "action" ? "bfe-opt-action" : "bfe-opt-text");
        value.append(parts.text);
      }
      node.append(value);
      this.view.bind(node, target);
      this.rows.set(index, parts);
      return node;
    }));
  }

  // A slider's track: pressed, it sets the value there, and follows the
  // pointer until it lets go.
  slide(track, target, row) {
    const set = (event) => {
      const rect = track.getBoundingClientRect();
      if (row.setFraction((event.clientX - rect.left) / Math.max(rect.width, 1))) this.view.screen.sound("accept");
    };
    track.addEventListener("pointerdown", (event) => {
      if (!this.view.live || event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      this.view.pointer(target);
      track.setPointerCapture(event.pointerId);
      set(event);
      const move = (e) => set(e);
      const up = () => {
        track.removeEventListener("pointermove", move);
        track.removeEventListener("pointerup", up);
        track.removeEventListener("pointercancel", up);
      };
      track.addEventListener("pointermove", move);
      track.addEventListener("pointerup", up);
      track.addEventListener("pointercancel", up);
    });
    track.addEventListener("click", (event) => event.stopPropagation());
  }
}

// BACK, a middle button, an accept button: along the bottom.
class ButtonView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = button(widget.label(), `bfe-btn small bfe-foot ${widget.place || "back"}`);
    view.bind(this.el, { widget, item: 0 });
  }

  update(live) {
    const w = this.widget;
    this.el.hidden = Boolean(w.hidden);
    setLabel(this.el, w.label());
    this.el.classList.toggle("disabled", !w.enabled(0));
    this.el.classList.toggle("active", live && this.view.screen.isFocused(w, 0));
  }
}

// Text: a screen's title, a heading, a label (in a corner when it has a
// place of its own), a popup's message.
class TextView {
  constructor(widget) {
    this.widget = widget;
    const look = widget.look || "title";
    const tag = { title: "h1", heading: "h2", popup: "p" }[look] || "div";
    this.el = make(tag, `bfe-text bfe-text-${look}`);
    this.inner = look === "title" ? this.el.appendChild(make("span")) : this.el;
    this.text = null;
  }

  // Placed by its anchor, at the screen's edge or across from it (the main
  // menu's version line).
  corner() {
    const [x, y] = this.widget.at;
    const style = this.el.style;
    this.el.classList.add("bfe-text-corner");
    if (x >= 1) style.right = "var(--bfe-edge)";
    else if (x <= 0) style.left = "var(--bfe-edge)";
    else Object.assign(style, { left: `${x * 100}%`, transform: "translateX(-50%)" });
    if (y >= 1) style.bottom = "14px";
    else if (y <= 0) style.top = "14px";
    else style.top = `${y * 100}%`;
  }

  update() {
    this.el.hidden = Boolean(this.widget.hidden);
    const text = this.widget.lines().join("\n");
    if (text !== this.text) {
      this.text = text;
      this.inner.textContent = text;
    }
  }
}

// A list: rows of columns, the selected one marked; a click selects, a
// double click takes it (onAccept), the wheel scrolls.
class ListView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = make("div", "bfe-list");
    const columns = resolve(widget.look, SURFACE)?.columns || [{ x: 0 }];
    // Each column as wide as the room to the next one's start.
    const widths = columns.map((column, i) => Math.max((columns[i + 1]?.x ?? 1) - (column.x || 0), 0.05));
    this.el.style.setProperty("--columns", widths.map((width) => `${width.toFixed(3)}fr`).join(" "));
    this.aligns = columns.map((column) => column.align ?? 0);
    this.signature = null;
    this.nodes = [];
    this.selectedNode = null;
    this.el.addEventListener("pointerenter", () => view.pointer({ widget, item: 0 }));
  }

  update(live) {
    const w = this.widget;
    this.el.hidden = Boolean(w.hidden);
    const rows = w.rowsNow();
    const signature = rows.map((row) => `${row.join("\u0001")}\u0001${row.disabled ? 1 : 0}\u0001${(row.icons || []).join(",")}`).join("\u0002");
    if (signature !== this.signature) {
      this.signature = signature;
      this.build(rows);
    }
    this.el.classList.toggle("focused", live && this.view.screen.isFocused(w, 0));
    this.el.classList.toggle("empty", !rows.length);
    const selected = this.nodes[w.selected] || null;
    for (const node of this.nodes) node.classList.toggle("selected", node === selected);
    if (selected !== this.selectedNode) {
      this.selectedNode = selected;
      if (selected && performance.now() - this.view.theme.pointerAt > POINTER_FOCUS * 1000) reveal(selected);
    }
  }

  build(rows) {
    const w = this.widget;
    const icons = this.view.theme.icons;
    this.nodes = rows.map((row, index) => {
      const node = make("div", "bfe-list-row");
      node.classList.toggle("disabled", Boolean(row.disabled));
      this.aligns.forEach((align, c) => {
        const cell = make("span", "bfe-list-cell", String(row[c] ?? ""));
        if (align === 1) cell.classList.add("end");
        node.append(cell);
      });
      const marks = make("span", "bfe-list-icons");
      for (const icon of row.icons || []) marks.append(make("span", `bfe-icon ${icon}`, icons[icon] ?? icon.replace(/_icon$/, "").toUpperCase()));
      node.append(marks);
      node.addEventListener("mousedown", (event) => event.preventDefault());
      node.addEventListener("click", () => {
        if (!this.view.live) return;
        this.view.pointer({ widget: w, item: 0 });
        w.select(index, this.view.screen);
      });
      node.addEventListener("dblclick", () => {
        if (this.view.live) this.view.screen.accept({ widget: w, item: 0 });
      });
      return node;
    });
    this.el.replaceChildren(...this.nodes);
    this.selectedNode = null;
  }
}

// Tabs along the top of a page: the active one lit.
class TabsView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = make("div", "bfe-tabs");
    this.nodes = widget.tabs.map((tab, i) => {
      const node = button(tab.label, "bfe-tab");
      if (tab.badge) node.append(make("span", "bfe-tab-badge", tab.badge));
      node.classList.toggle("disabled", Boolean(tab.disabled));
      node.addEventListener("click", () => {
        if (tab.disabled) view.screen.sound("error");
        else if (view.live) widget.choose(i, view.screen);
      });
      return node;
    });
    this.el.append(...this.nodes);
  }

  update(live) {
    this.nodes.forEach((node, i) => {
      node.classList.toggle("active", i === this.widget.active);
      node.classList.toggle("focused", live && this.view.screen.isFocused(this.widget, i));
    });
  }
}

// Buttons side by side: a popup's YES and NO, Instant Action's ADD and
// REMOVE.
class ButtonsView {
  constructor(widget, view) {
    this.widget = widget;
    this.view = view;
    this.el = make("div", "bfe-hbuttons");
    this.signature = null;
    this.nodes = [];
  }

  update(live) {
    const w = this.widget;
    this.el.hidden = Boolean(w.hidden);
    const signature = w.buttons.map((_, i) => w.label(i)).join("\u0001");
    if (signature !== this.signature) {
      this.signature = signature;
      this.nodes = w.buttons.map((_, i) => {
        const node = button(w.label(i), "bfe-btn small");
        this.view.bind(node, { widget: w, item: i });
        return node;
      });
      this.el.replaceChildren(...this.nodes);
    }
    this.nodes.forEach((node, i) => {
      node.classList.toggle("active", live && this.view.screen.isFocused(w, i));
      node.classList.toggle("disabled", !w.enabled(i));
    });
  }
}

let styled = false;
function addStyle() {
  if (styled) return;
  styled = true;
  const style = document.createElement("style");
  style.dataset.bfe = "page-theme";
  style.textContent = STYLE;
  document.head.append(style);
}
