import * as THREE from "three";
import { insideQuad } from "../../../interface/shapes.js";
import { BLACK, DIMMED, GREY, WHITE, bar, corners, flashyText, frame, quad, rgba, scaled, selectedColor, slider, tint, translate } from "./draw.js";

// The Classic theme: the menus drawn as the 2004 PC game draws them, in
// the interface space (src/interface), checked against the retail captures.
// Each widget kind has a painter that draws it and records its hotspots;
// another theme (an Enhanced one) can draw the same widgets its own way.
//
// What it draws:
// - rows: the angled menu-row stack under its orange title bar, the
//   container turned as the screen's layout says;
// - options: an option page's label | value rows, sliders, the focused
//   value pulsing yellow;
// - button: BACK, an accept-type button, a middle one, in the bottom
//   corners and centre;
// - text: a page's title, a popup's question, a heading;
// - panel: a window, nine-sliced;
// - list: a list box with its selection frame, hover row and scroll bar;
//   tabs: header-bar tabs; hbuttons: a popup's YES / NO.
// - Over a screen marked dim, the world (or movie) behind is halved, as
//   the option pages do; and over everything the pointer (mousecursor).

const ROW_INTRO = 0.1; // s between the menu rows' intros
const POINTER = 32; // units
const DIM = rgba(0, 0, 0, 128); // an option page's backdrop: the world at half
const HEADER = ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"];
const BUTTON = ["buttonleft", "buttonmid", "buttonright"];

// Calibrated on the retail pause menu: a row's bar starts 4.7
// units left of the row's place and its text 0.8 right; the text's line
// sits 2 units under the row's centre line. A row's bar is the row pitch
// tall.
const BAR_X = -4.7;
const TEXT_X = 0.8;
const TEXT_DY = 2;

// The container a stack of rows sits in is moved left by half the widest
// bar its whole list would have (the original's AddVerticalButtons, which
// counts the rows hidden on PC too): its width grows a pitch a row down to
// the middle, starting a pitch wider for an odd count.
export function rowsShift(width, pitch, count) {
  let w = width + (count % 2 === 1 ? pitch : 0);
  let widest = 0;
  const half = Math.floor(count / 2);
  for (let i = 1; i <= count; i++) {
    if (i < half) w += pitch;
    else if (i > half) w -= pitch;
    widest = Math.max(widest, w);
  }
  return -widest / 2;
}

// Each visible row's bar width (as the original's ShowHideVerticalButtons
// sets them on every entry) and whether its right end is flipped.
export function rowBars(width, pitch, n) {
  const half = Math.floor(n / 2);
  const out = [];
  let w = width;
  for (let i = 1; i <= n; i++) {
    out.push({ width: w, flipped: i > half });
    if (i < half) w += pitch;
    else if (i > half) w -= pitch;
  }
  return out;
}

function rotation(layout) {
  const r = THREE.MathUtils.degToRad;
  return new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(r(layout.rotX || 0), r(layout.rotY || 0), r(layout.rotZ || 0), layout.order || "YXZ"));
}

// --- the page looks -------------------------------------------------------
// Option pages, by layout name (the original's ifs_opt_general, _sound and
// _pcvideo; the controls page's top rows): the font, where the labels end
// and the values start ([safe-area x, units]), the first row's centre
// (units below the safe area's top) and the pitch, sliders' tracks, where a
// slider's number goes, a scroll bar's place and the room kept under the
// rows (bottom) when they scroll.
export const OPTION_LOOKS = {
  game: { font: "gamefont_medium", labels: [0.5, -15], values: [0.5, 15], top: 150, pitch: 40, rowHeight: 35, valueWidth: 350, textDy: 2 },
  audio: { font: "gamefont_medium", labels: [0.4, 0], values: [0.4, 35], top: 55, pitch: 35, rowHeight: 35,
    slider: { from: [0.4, 20], to: [1, -176.8], height: 24, thumb: 0.1 }, number: [1, 0], valueWidth: 500 },
  // The live captures show the VIDEO page in gamefont_medium (the original
  // takes it when the safe area is over 700 tall) with 110-unit sliders.
  video: { font: "gamefont_medium", labels: [0.5, -15], values: [0.5, 15], top: 32, pitch: 30, rowHeight: 28, bottom: 60,
    slider: { from: [0.5, 15], width: 110, height: 22, thumb: 0.1 }, number: [0.5, 140], valueWidth: 400, scroll: [0.5, 380] },
  controls: { font: "gamefont_tiny", labels: [0.15, 0], values: null, top: 121, pitch: 24.5, rowHeight: 25,
    slider: { from: [0.4, 0], width: 0.25, height: 24, thumb: 0.1, relative: true }, number: [1, -2], valueWidth: 800, align: "left" },
};

// --- painters --------------------------------------------------------------

function paintRows(w, ctx) {
  const { surface } = ctx;
  const L = w.layout;
  const fonts = surface.fonts;
  const yh = L.yHeight ?? fonts.lineHeight(L.font);
  const fh = fonts.lineHeight(L.font);
  const p = yh + L.ySpacing;
  const visible = w.visibleRows();
  const n = visible.length;
  const shift = L.containerX ?? rowsShift(L.width, p, w.listCount ?? w.rows.length);
  const m = surface.anchor(L.at[0], L.at[1], L.safe !== false).multiply(translate(shift + (L.x || 0), L.y || 0)).multiply(rotation(L));
  const y0 = L.yTop ?? -(n * p) / 2;
  const shown = ctx.alpha;
  if (w.title) {
    // The title bar: its text 1.75 pitches above the first row and half a
    // pitch in; the bar from there less half a pitch, from a quarter pitch
    // over the text to a quarter pitch under its glyphs' full depth (the
    // box the original measures text by, deeper than the font's line: the
    // retail captures' bars are that tall).
    const intro = ctx.screen.intro(`${w.id}:title`, 0);
    const font = L.titleFont || L.font;
    const ty = y0 - 1.75 * p;
    const width = L.width - 1.25 * (yh - L.ySpacing) - 0.65 * p;
    const by = ty - 0.25 * p;
    bar(ctx, m, HEADER, BAR_X, by, width, ty + fonts.glyphHeight(font) + 0.25 * p, { grow: intro.frame.bar, color: tint(WHITE, shown) });
    flashyText(ctx, m, p / 2 + TEXT_X, (by + ty + fonts.glyphHeight(font) + 0.25 * p) / 2 - fonts.capsMiddle(font), String(w.title), font, BLACK, { intro, alpha: shown });
  }
  const bars = rowBars(L.width, p, n);
  visible.forEach(({ index }, k) => {
    const cy = y0 + k * p;
    const focused = ctx.focused(w, index);
    const a = (focused ? ctx.pulse : 1) * shown;
    const intro = ctx.screen.intro(`${w.id}:${index}`, (k + 1) * ROW_INTRO);
    const color = focused ? tint(WHITE, a) : tint(scaled(WHITE, DIMMED), a);
    bar(ctx, m, BUTTON, BAR_X, cy - p / 2, bars[k].width + 5, cy + p / 2, { flipped: bars[k].flipped, grow: intro.frame.bar, color });
    const enabled = w.enabled(index);
    flashyText(ctx, m, TEXT_X, cy - fonts.capsMiddle(L.font), w.label(index), L.font, enabled ? WHITE : GREY, { intro, alpha: a });
    ctx.hotspot({ widget: w, item: index }, corners(ctx, m, 0, cy - yh / 2, L.width, cy + yh / 2));
  });
}

// A page's title, an option page's (the original's gamefont_large text at
// the top centre).
function paintText(w, ctx) {
  const look = w.look || "title";
  const { surface } = ctx;
  const fonts = surface.fonts;
  const lines = w.lines();
  if (look === "title" || look === "heading") {
    const font = w.font || "gamefont_large";
    const m = surface.anchor(0.5, 0, true);
    const intro = w.flashy === false ? null : ctx.screen.intro(`${w.id}:text`, 0);
    lines.forEach((line, i) => flashyText(ctx, m, 0, (w.y ?? 10) + i * fonts.lineHeight(font), line, font, WHITE, { align: 2, intro, alpha: ctx.alpha }));
    return;
  }
  if (look === "popup") {
    // The question: gamefont_medium, centred lines from 74 units over the
    // popup's centre, in a box 260 wide.
    const font = "gamefont_medium";
    const m = surface.anchor(0.5, 0.5, false);
    const wrapped = lines.flatMap((line) => wrap(fonts, font, line, w.width || 260));
    wrapped.forEach((line, i) => fonts.draw(ctx.batch, m, 0, -74 + i * fonts.lineHeight(font), line, font, tint(WHITE, ctx.alpha), 2));
    return;
  }
  // Anywhere else: at (x, y) of a safe-area anchor.
  const font = w.font || "gamefont_small";
  const m = surface.anchor(...(w.at || [0, 0]), w.safe !== false);
  lines.forEach((line, i) => fonts.draw(ctx.batch, m, w.x || 0, (w.y || 0) + i * fonts.lineHeight(font), line, font, tint(w.color || WHITE, ctx.alpha * (w.alpha ?? 1)), w.align ?? 0));
}

function wrap(fonts, font, text, width) {
  const lines = [];
  let line = "";
  for (const word of String(text).split(/\s+/).filter(Boolean)) {
    const next = line ? `${line} ${word}` : word;
    if (line && fonts.width(font, next) > width) {
      lines.push(line);
      line = word;
    } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

// BACK, an accept button and a middle one (the original's
// CreateNegativeButton / CreatePositiveButton / CreateMiddleButton):
// gamefont_medium at the safe area's bottom, 15 units up, white, the
// focused one pulsing yellow.
const BUTTON_PLACES = { back: [0, 1, 0], accept: [1, 1, 1], middle: [0.5, 1, 2] };
const BUTTON_INSET = 21; // units from the safe edge to the text (measured)

function paintButton(w, ctx) {
  const { surface } = ctx;
  const fonts = surface.fonts;
  const font = "gamefont_medium";
  const [ax, ay, align] = BUTTON_PLACES[w.place] || BUTTON_PLACES.back;
  const m = surface.anchor(ax, ay, true);
  const text = w.label();
  const width = fonts.width(font, text);
  const x = align === 0 ? BUTTON_INSET : align === 1 ? -BUTTON_INSET : 0;
  // Its capitals' middle 12 units over the safe area's bottom (measured).
  const y = -12 - fonts.capsMiddle(font);
  const focused = ctx.focused(w, 0);
  const enabled = w.enabled(0);
  const color = !enabled ? GREY : focused ? selectedColor(ctx.pulse) : WHITE;
  const intro = ctx.screen.intro(`${w.id || w.place}:button`, 0);
  flashyText(ctx, m, x, y, text, font, color, { align, intro, alpha: ctx.alpha });
  const left = align === 0 ? x : align === 1 ? x - width : x - width / 2;
  ctx.hotspot({ widget: w, item: 0 }, corners(ctx, m, left - 4, -15 - 12.5, left + width + 4, -15 + 12.5));
}

// A window: the popup's (300 x 180 at the screen's centre), or a box.
function paintPanel(w, ctx) {
  const { surface } = ctx;
  if (w.look === "popup") {
    const [pw, ph] = w.size || [300, 180];
    frame(ctx, surface.anchor(0.5, 0.5, false), 0, 0, pw, ph, "border_3_pieces", tint(WHITE, ctx.alpha));
    return;
  }
  const box = typeof w.box === "function" ? w.box(surface) : w.box;
  if (!box) return;
  const m = surface.anchor(...(w.at || [0, 0]), w.safe !== false);
  frame(ctx, m, box.x, box.y, box.w, box.h, w.skin || "border_3_pieces", tint(WHITE, ctx.alpha * (w.alpha ?? 1)));
}

// A popup's buttons: gamefont_medium, YES and NO centred 85 units apart
// around the popup's middle (at -40 and 45), 63 under it; a lone OK in the
// middle; the focused one pulsing yellow. y: another line for them.
function paintHorizontal(w, ctx) {
  const { surface } = ctx;
  const fonts = surface.fonts;
  const font = "gamefont_medium";
  const m = surface.anchor(0.5, 0.5, false);
  const fh = fonts.lineHeight(font);
  const count = w.buttons.length;
  const y = w.y ?? 63;
  w.buttons.forEach((button, i) => {
    const x = count === 1 ? 0 : -40 + i * 85 - (count - 2) * 42.5;
    const focused = ctx.focused(w, i);
    const color = focused ? selectedColor(ctx.pulse) : WHITE;
    const intro = ctx.screen.intro(`${w.id}:${i}`, 0);
    flashyText(ctx, m, x, y - fonts.capsMiddle(font), w.label(i), font, color, { align: 2, intro, alpha: ctx.alpha });
    ctx.hotspot({ widget: w, item: i }, corners(ctx, m, x - 40, y - 22.5, x + 40, y + 22.5));
  });
}

// Option rows: labels | values.
function paintOptions(w, ctx) {
  const { surface } = ctx;
  const look = { ...(OPTION_LOOKS[w.layout] || OPTION_LOOKS.game), ...(w.look || {}) };
  const fonts = surface.fonts;
  const font = look.font;
  const fh = fonts.lineHeight(font);
  const H = surface.height;
  const safeH = 0.9 * H;
  const at = (x) => surface.anchor(x[0], 0, true);
  const rows = w.visible();
  // How many fit, above the bottom buttons.
  if (look.bottom !== undefined) {
    const room = safeH - look.top - look.bottom;
    w.shown = Math.max(Math.floor(room / look.pitch), 1);
  } else w.shown = Infinity;
  const first = Number.isFinite(w.shown) ? Math.min(w.first, Math.max(rows.length - w.shown, 0)) : 0;
  w.first = first;
  const end = Number.isFinite(w.shown) ? first + w.shown : rows.length;
  const labelsM = at(look.labels);
  const valuesM = look.values ? at(look.values) : null;
  const caps = fonts.capsMiddle(font);
  rows.slice(first, end).forEach(({ row, index }, k) => {
    const cy = look.top + k * look.pitch;
    const y = cy - caps + (look.textDy || 0);
    const focused = ctx.focused(w, index);
    const enabled = w.enabled(index) || row.kind === "fixed";
    const greyed = !enabled || (row.kind === "fixed" && row.dim);
    const label = String(typeof row.label === "function" ? row.label() : row.label ?? "");
    const align = look.align === "left" ? 0 : 1;
    if (label) fonts.draw(ctx.batch, labelsM, look.labels[1], y, label, font, tint(WHITE, ctx.alpha), align);
    const pulse = focused ? ctx.pulse : 1;
    const valueColor = greyed ? GREY : focused ? selectedColor(pulse) : WHITE;
    let track = null;
    if (row.kind === "slider" && look.slider) {
      const s = look.slider;
      const fromM = at(s.from);
      const x0 = s.from[1];
      let x1;
      if (s.to) {
        // To another anchor: the track's right end in the same frame.
        const toX = (0.05 + 0.9 * s.to[0]) * 1024 + s.to[1];
        const fromX = (0.05 + 0.9 * s.from[0]) * 1024 + s.from[1];
        x1 = x0 + (toX - fromX);
      } else x1 = x0 + (s.relative ? s.width * 0.9 * 1024 : s.width);
      slider(ctx, fromM, x0, cy - s.height / 2, x1, cy + s.height / 2, row.fraction(), { thumb: s.thumb, focused, alpha: ctx.alpha });
      track = corners(ctx, fromM, x0, cy - s.height / 2, x1, cy + s.height / 2);
      if (look.number) {
        const numberM = at(look.number);
        fonts.draw(ctx.batch, numberM, look.number[1], y, row.text(), font, tint(valueColor, ctx.alpha), look.number[0] >= 1 ? 1 : 0);
      }
    } else if (look.values) {
      fonts.draw(ctx.batch, valuesM, look.values[1], y, row.text(), font, tint(valueColor, ctx.alpha), 0);
    } else if (look.number) {
      fonts.draw(ctx.batch, at(look.number), look.number[1], y, row.text(), font, tint(valueColor, ctx.alpha), 1);
    }
    // The whole row is its hotspot, from the label's left to the value
    // column's end; a slider's track also carries where it runs, for a
    // click or a drag along it.
    const labelLeft = look.align === "left" ? look.labels[1] : look.labels[1] - Math.max(fonts.width(font, label), 120);
    const from = labelsM.clone().multiply(translate(labelLeft - 10, 0));
    const span = rowSpan(look, labelLeft - 10);
    const hot = corners(ctx, from, 0, cy - look.rowHeight / 2, span, cy + look.rowHeight / 2);
    const target = { widget: w, item: index };
    if (track) target.track = { x0: track[0].x, x1: track[1].x };
    ctx.hotspot(target, hot);
  });
  // The focused row's note (how its change applies), under the rows.
  const focused = ctx.screen.focused?.widget === w ? w.rows[ctx.screen.focused.item] : null;
  const note = focused ? (typeof focused.note === "function" ? focused.note() : focused.note) : null;
  if (note) {
    const y = look.bottom !== undefined ? safeH - look.bottom + 8 : look.top + Math.min(rows.length, w.shown) * look.pitch + 6;
    fonts.draw(ctx.batch, surface.anchor(0.5, 0, true), 0, y, String(note), "gamefont_tiny", tint(rgba(223, 223, 32), ctx.alpha), 2);
  }
  // The scroll bar, when they don't all fit.
  if (look.scroll && rows.length > w.shown) {
    const m = at(look.scroll);
    const x = look.scroll[1];
    const top = look.top - look.pitch / 2;
    const height = w.shown * look.pitch;
    scrollBar(ctx, m, x, top, height, first, w.shown, rows.length);
  }
}

// How far a row's hotspot runs from its left (units): to the values'
// column end, or across the page for the left-aligned look.
function rowSpan(look, left) {
  const unit = (a) => (0.05 + 0.9 * a[0]) * 1024 + a[1];
  const start = unit([look.labels[0], left]);
  const end = look.align === "left" ? unit([1, 0]) : look.values ? unit(look.values) + (look.valueWidth || 300) : unit([1, 0]);
  return Math.max(end - start, 10);
}

// A list's scroll bar: slider_bg the list's height, slider_fg the part in
// view (at least 6 units); both see-through, as the retail captures
// measure (the track at half alpha, the thumb at a third).
const TRACK_ALPHA = 0.5;
const THUMB_ALPHA = 0.33;

function scrollBar(ctx, m, x, top, height, first, shown, count) {
  quad(ctx, m, "slider_bg", x, top, x + 16, top + height, [0, 0, 1, 1], tint(WHITE, TRACK_ALPHA * ctx.alpha));
  const t0 = top + (first / count) * height;
  const t1 = Math.max(top + ((first + shown) / count) * height, t0 + 6);
  quad(ctx, m, "slider_fg", x, t0, x + 16, Math.min(t1, top + height), [0, 0, 1, 1], tint(WHITE, THUMB_ALPHA * ctx.alpha));
  return { x, top, height };
}

// A list box: rows of columns, the selection's frame (listbox_cursor,
// pulsing 1 -> 0.5 -> 1 a second while the list is focused), the hovered
// row lit (listbox_hilight), and its scroll bar. box: { x, y, w, h } at
// the anchor `at`; columns: [{ x: 0..1 of the inner width, align }].
function paintList(w, ctx) {
  const { surface } = ctx;
  const fonts = surface.fonts;
  const look = typeof w.look === "function" ? w.look(surface) : w.look;
  const m = surface.anchor(...(look.at || [0, 0]), look.safe !== false);
  m.multiply(translate(look.x || 0, look.y || 0)).multiply(rotation(look));
  const { x, y, w: width, h: height } = look.box;
  const font = look.font || "gamefont_tiny";
  const pitch = look.pitch || 23;
  const fh = fonts.lineHeight(font);
  if (look.window !== false) frame(ctx, m, x, y, width, height, "border_3_pieces", tint(WHITE, ctx.alpha));
  const rows = w.rowsNow();
  const top = y - height / 2 + (look.inset ?? 15);
  w.shown = Math.max(Math.floor((height - 2 * (look.inset ?? 15)) / pitch), 1);
  w.first = Math.min(w.first, Math.max(rows.length - w.shown, 0));
  const scroll = rows.length > w.shown;
  const left = x - width / 2 + (look.padLeft ?? 15);
  const inner = width - (look.padLeft ?? 15) - (look.padRight ?? 15) - (scroll ? 26 : 0);
  const focused = ctx.screen.focused?.widget === w;
  for (let k = 0; k < w.shown; k++) {
    const index = w.first + k;
    const row = rows[index];
    if (!row) break;
    const cy = top + k * pitch + pitch / 2;
    if (index === w.selected) {
      const a = focused ? cursorPulse(ctx.stack.time) : 1;
      frame(ctx, m, left + inner / 2, cy, inner + 6, pitch + 10, "listbox_cursor", tint(WHITE, a * ctx.alpha));
    } else if (index === w.hoverRow) {
      frame(ctx, m, left + inner / 2, cy, inner + 6, pitch + 10, "listbox_hilight", tint(WHITE, ctx.alpha));
    }
    (look.columns || [{ x: 0 }]).forEach((column, c) => {
      const text = String(row[c] ?? "");
      if (!text) return;
      fonts.draw(ctx.batch, m, left + 8 + column.x * inner, cy - fonts.capsMiddle(font), text, font, tint(row.disabled ? GREY : column.color || WHITE, ctx.alpha), column.align ?? 0);
    });
    for (let icon = 0; icon < (row.icons?.length || 0); icon++) {
      const size = 16.9;
      const right = left + inner - (row.icons.length - icon - 1) * (size + 3);
      quad(ctx, m, row.icons[icon], right - size, cy - size / 2, right, cy + size / 2, [0, 0, 1, 1], tint(WHITE, ctx.alpha));
    }
    ctx.hotspot({ widget: w, item: 0, row: index }, corners(ctx, m, left, cy - pitch / 2, left + inner, cy + pitch / 2));
  }
  if (scroll) {
    const bar = scrollBar(ctx, m, x + width / 2 - 34, top, w.shown * pitch, w.first, w.shown, rows.length);
    const hot = corners(ctx, m, bar.x, bar.top, bar.x + 16, bar.top + bar.height);
    ctx.hotspot({ widget: w, item: 0, part: "track", get fraction() { return Math.min(Math.max((ctx.stack.pointer.y - hot[0].y) / Math.max(hot[3].y - hot[0].y, 1), 0), 1); } }, hot);
  }
}

// The selection frame's strength: 1 -> 0.5 -> 1, one cycle a second.
function cursorPulse(time) {
  const u = (time % 1) * 2;
  return u < 1 ? 1 - 0.5 * u : 0.5 + 0.5 * (u - 1);
}

// Header-bar tabs: each a quarter of the safe width, 0.3 of it apart,
// the active one solid, the others at 0.4.
function paintTabs(w, ctx) {
  const { surface } = ctx;
  const fonts = surface.fonts;
  const look = typeof w.look === "function" ? w.look(surface) : w.look;
  const m = surface.anchor(...(look.at || [0, 0]), true);
  const font = look.font || "gamefont_tiny";
  const caps = fonts.capsMiddle(font);
  const sw = 0.9 * 1024;
  const y0 = look.y - 13;
  const y1 = look.y + 11;
  w.tabs.forEach((tab, i) => {
    const x = look.x + i * look.spacing * sw;
    const width = look.width * sw;
    const active = i === w.active;
    const focused = ctx.focused(w, i);
    const a = (active ? 1 : 0.4) * ctx.alpha;
    bar(ctx, m, HEADER, x - 8, y0, width - (y1 - y0), y1, { color: tint(WHITE, a) });
    const color = focused ? selectedColor(ctx.pulse) : active ? WHITE : rgba(200, 200, 200);
    fonts.draw(ctx.batch, m, x + 4, (y0 + y1) / 2 - caps, String(tab.label), font, tint(color, focused ? 1 : a), 0);
    ctx.hotspot({ widget: w, item: i }, corners(ctx, m, x - 8, y0, x + width, y1));
  });
}

export const PAINTERS = {
  rows: paintRows,
  options: paintOptions,
  button: paintButton,
  text: paintText,
  panel: paintPanel,
  list: paintList,
  tabs: paintTabs,
  hbuttons: paintHorizontal,
};

// Kinds a popup hides on the screen under it (its buttons stay).
const UNDER_POPUP = new Set(["rows", "options", "list", "tabs"]);

export class ClassicTheme {
  // painters: more kinds, or replacements, by kind; pointer: draw the
  // mousecursor pointer while the pointer has moved.
  constructor({ painters = {}, pointer = true } = {}) {
    this.painters = { ...PAINTERS, ...painters };
    this.pointer = pointer;
    this.name = "classic";
  }

  register(kind, painter) {
    this.painters[kind] = painter;
  }

  // The stack's visible screens into the batch, their hotspots recorded.
  draw(stack, surface, batch) {
    stack.hotspots.length = 0;
    const visible = stack.visible;
    visible.forEach((screen, i) => {
      const top = i === visible.length - 1;
      const ctx = {
        surface,
        batch,
        screen,
        stack,
        theme: this,
        top,
        alpha: screen.shown,
        pulse: stack.pulse,
        focused: (widget, item) => top && screen.isFocused(widget, item),
        hotspot: (target, quadCorners) => {
          if (top) stack.hotspots.push({ target, corners: quadCorners });
        },
      };
      if (screen.dim) batch.quad(surface.anchor(0, 0, false), 0, 0, 1024, surface.height, surface.solid, [0, 0, 1, 1], DIM);
      screen.draw?.(ctx, "under");
      for (const widget of screen.widgets) {
        if (widget.hidden || (!top && UNDER_POPUP.has(widget.kind))) continue;
        this.painters[widget.kind]?.(widget, ctx);
      }
      screen.draw?.(ctx, "over");
    });
    if (this.pointer && stack.pointer.visible && stack.showPointer !== false) this.drawPointer(stack, surface, batch);
  }

  drawPointer(stack, surface, batch) {
    const { x, y } = stack.pointer;
    const m = surface.anchor(0, 0, false);
    const texture = surface.texture("mousecursor");
    if (texture) batch.quad(m, x, y, x + POINTER, y + POINTER, texture, [0, 0, 1, 1], WHITE);
    else batch.quad(m, x, y, x + 8, y + 8, surface.solid, [0, 0, 1, 1], WHITE);
  }

  // The hotspot under an interface point: the last drawn first.
  hit(stack, x, y) {
    for (let i = stack.hotspots.length - 1; i >= 0; i--) {
      const hotspot = stack.hotspots[i];
      if (insideQuad(x, y, hotspot.corners)) return hotspot.target;
    }
    return null;
  }
}
