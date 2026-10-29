import * as THREE from "three";
import { nineSlice } from "../../../interface/shapes.js";

// How the 2004 interface draws its pieces (the geometry of the original's
// FlashyText drawing):
//
// - A bar behind a text is three pieces: a left end as wide as the bar is
//   tall, a middle stretched to the bar's width, and the right end (another
//   square, past that width), mirrored top to bottom when the bar is
//   flipped. Menu rows use buttonleft / buttonmid / buttonright (blue, the
//   right end a triangle), titles headerbutton* (orange). A row's bar that
//   isn't the focused one is drawn at 150/255 of its colour.
// - The intro: the bar grows left to right, a white block (cursor) sweeps
//   across the text, the text grows from its centre line, then fades in
//   (flashy.js).
// - Text without a bar that has the focus is yellow, its green following
//   its alpha: (255, 1.2 A + 50 clamped to 100..255, 100).
// - Windows are border_3_pieces nine-sliced, with 16-unit corners.
// Missing textures (not yet extracted) fall back to flat colours close to
// theirs, so a screen still reads.

export const rgba = (r, g, b, a = 255) => [r / 255, g / 255, b / 255, a / 255];
export const WHITE = rgba(255, 255, 255);
export const BLACK = rgba(0, 0, 0);
export const GREY = rgba(128, 128, 128);
export const DIMMED = 150 / 255; // an unfocused row's bar
export const tint = (color, a) => [color[0], color[1], color[2], (color[3] ?? 1) * a];
export const scaled = (color, k) => [color[0] * k, color[1] * k, color[2] * k, color[3] ?? 1];

// Stand-ins for the menu art until it is extracted (public/common's
// buttonleft, buttonmid, buttonright, slider_bg and slider_fg): drawn here
// to the same shapes, 64 x 64 (the sliders' flat), so a screen reads the
// same. A bar: a soft 3-texel edge top and bottom; the left end soft at its
// left too; the middle a gradient from light to dark steel blue; the right
// end the triangle below its diagonal.
const STAND_INS = new Map();

function standIn(name) {
  if (STAND_INS.has(name)) return STAND_INS.get(name);
  const size = 64;
  const data = new Uint8Array(size * size * 4);
  const light = [57, 125, 180];
  const dark = [24, 72, 131];
  const rim = [8, 56, 106];
  // Texels from an edge: three fading in, then a dark rim easing into the
  // fill over three more.
  const alphaAt = (t) => (t <= 0 ? 0 : t === 1 ? 0.2 : t === 2 ? 0.8 : 1);
  const fillAt = (t) => (t <= 3 ? 0 : t === 4 ? 0.55 : t === 5 ? 0.85 : 1);
  const shade = (base, t) => base.map((c, i) => rim[i] + (c - rim[i]) * fillAt(t));
  let make = null;
  if (name === "buttonleft") {
    make = (x, y) => {
      const t = Math.min(x, y, size - 1 - y);
      return [shade(light, t), alphaAt(t)];
    };
  } else if (name === "buttonmid") {
    make = (x, y) => {
      const t = Math.min(y, size - 1 - y);
      return [shade(light.map((c, i) => c + (dark[i] - c) * (x / (size - 1))), t), alphaAt(t)];
    };
  } else if (name === "buttonright") {
    make = (x, y) => {
      const t = Math.min(y, size - 1 - y);
      return [shade(dark, t), Math.min(Math.max(Math.min((y - x) / 1.5, 1), 0), alphaAt(t))];
    };
  }
  else if (name === "slider_bg") make = () => [[8, 40, 66], 1];
  else if (name === "slider_fg") make = () => [[255, 101, 24], 1];
  else if (name === "white") make = () => [[255, 255, 255], 1];
  if (!make) {
    STAND_INS.set(name, null);
    return null;
  }
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const [rgb, a] = make(x, y);
      data.set([rgb[0], rgb[1], rgb[2], Math.round(255 * a)], (y * size + x) * 4);
    }
  }
  const texture = new THREE.DataTexture(data, size, size);
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  STAND_INS.set(name, texture);
  return texture;
}

// Flat stand-ins for the rest: [r, g, b, a] per piece.
const FALLBACK = {
  buttonleft: rgba(57, 125, 180),
  buttonmid: rgba(40, 95, 150),
  buttonright: rgba(24, 72, 131),
  headerbuttonleft: rgba(255, 165, 24),
  headerbuttonmid: rgba(255, 133, 24),
  headerbuttonright: rgba(255, 101, 24),
  backbutton_left: rgba(16, 40, 80, 220),
  backbutton_mid: rgba(16, 40, 80, 220),
  backbutton_right: rgba(16, 40, 80, 220),
  slider_bg: rgba(8, 40, 65),
  slider_fg: rgba(255, 101, 24),
  border_3_pieces: rgba(0, 44, 98, 180),
  listbox_cursor: rgba(255, 165, 24, 255),
  listbox_hilight: rgba(57, 125, 180, 255),
  white: rgba(255, 255, 255),
  cursor: rgba(255, 255, 255),
};

export const translate = (x, y, z = 0) => new THREE.Matrix4().makeTranslation(x, y, z);

// A piece of art, its stand-in, or a flat colour.
export function piece(ctx, name) {
  const texture = ctx.surface.texture(name);
  const loaded = texture && (texture.image || texture.userData?.loaded);
  return { texture: loaded ? texture : standIn(name), fallback: FALLBACK[name] || WHITE };
}

export function quad(ctx, m, name, x0, y0, x1, y1, uv, color) {
  const { texture, fallback } = piece(ctx, name);
  if (texture) ctx.batch.quad(m, x0, y0, x1, y1, texture, uv, color);
  else if (name === "buttonright" || name === "headerbuttonright") triangle(ctx, m, x0, y0, x1, y1, uv, [fallback[0] * color[0], fallback[1] * color[1], fallback[2] * color[2], fallback[3] * color[3]]);
  else ctx.batch.quad(m, x0, y0, x1, y1, ctx.surface.solid, [0, 0, 1, 1], [fallback[0] * color[0], fallback[1] * color[1], fallback[2] * color[2], fallback[3] * color[3]]);
}

// The right end's stand-in: its lower left half (upper left when the uv
// runs bottom to top), as slivers.
function triangle(ctx, m, x0, y0, x1, y1, uv, color) {
  const steps = 6;
  const flipped = uv[1] > uv[3];
  for (let i = 0; i < steps; i++) {
    const a = y0 + ((y1 - y0) * i) / steps;
    const b = y0 + ((y1 - y0) * (i + 1)) / steps;
    const k = flipped ? 1 - (i + 0.5) / steps : (i + 0.5) / steps;
    ctx.batch.quad(m, x0, a, x0 + (x1 - x0) * k, b, ctx.surface.solid, [0, 0, 1, 1], color);
  }
}

// A bar: left end from x0, the middle to x0 + width, the right end past it,
// y0..y1, grown `grow` (0..1) of its whole length from the left. names:
// [left, middle, right].
export function bar(ctx, m, names, x0, y0, width, y1, { flipped = false, grow = 1, color = WHITE } = {}) {
  const h = y1 - y0;
  const end = x0 + Math.max(width, h) + h;
  const visible = x0 + (end - x0) * grow;
  const pieces = [[x0, x0 + h, names[0]], [x0 + h, x0 + Math.max(width, h), names[1]], [x0 + Math.max(width, h), end, names[2]]];
  for (const [a, b, name] of pieces) {
    if (visible <= a || b <= a) continue;
    const to = Math.min(b, visible);
    const u1 = (to - a) / (b - a);
    const uv = name === names[2] && flipped ? [0, 1, u1, 0] : [0, 0, u1, 1];
    quad(ctx, m, name, a, y0, to, y1, uv, color);
  }
}

// A window: the 9-slice of `name` centred on (x, y).
export function frame(ctx, m, x, y, w, h, name = "border_3_pieces", color = WHITE) {
  const { texture, fallback } = piece(ctx, name);
  if (!texture) {
    ctx.batch.quad(m, x - w / 2 + 4, y - h / 2 + 4, x + w / 2 - 4, y + h / 2 - 4, ctx.surface.solid, [0, 0, 1, 1], [fallback[0], fallback[1], fallback[2], fallback[3] * color[3]]);
    return;
  }
  for (const [x0, y0, x1, y1, u0, v0, u1, v1] of nineSlice(w, h)) ctx.batch.quad(m, x + x0, y + y0, x + x1, y + y1, texture, [u0, v0, u1, v1], color);
}

// Focused text with no bar: yellow, its green from its alpha.
export function selectedColor(a) {
  const g = Math.min(Math.max(1.2 * a * 255 + 50, 100), 255);
  return [1, g / 255, 100 / 255, a];
}

// A text with its line's top at y, playing an intro (TitleIntro or null):
// the white block sweeping across, then the text growing from its middle
// and fading in. align: 0 from x, 1 ending at x, 2 centred on x.
export function flashyText(ctx, m, x, y, text, font, color, { align = 0, intro = null, alpha = 1 } = {}) {
  if (!text) return;
  const fonts = ctx.surface.fonts;
  const fh = fonts.lineHeight(font);
  const width = fonts.width(font, text);
  const state = intro?.frame ?? { block: null, text: 1, alpha: 1 };
  const left = align === 1 ? x - width : align === 2 ? x - width / 2 : x;
  if (state.block) {
    const bx = left + state.block.x * Math.max(width - fh, 0);
    const { texture } = piece(ctx, "cursor");
    ctx.batch.quad(m, bx, y, bx + fh, y + fh, texture || ctx.surface.solid, [0, 0, 1, 1], tint(WHITE, state.block.alpha * alpha));
  }
  if (state.text <= 0.01 || state.alpha <= 0) return;
  const mid = y + fh / 2;
  const grown = m.clone().multiply(translate(0, mid)).multiply(new THREE.Matrix4().makeScale(1, state.text, 1)).multiply(translate(0, -mid));
  fonts.draw(ctx.batch, grown, x, y, text, font, tint(color, state.alpha * alpha), align);
}

// A slider (the original's NewHSlider): the navy track x0..x1, y0..y1,
// with the orange thumb `thumb` of its width wide at fraction f. The
// focused one is solid, the others at half alpha (as the retail captures
// measure).
export const SLIDER_DIM = 0.5;

export function slider(ctx, m, x0, y0, x1, y1, f, { thumb = 0.1, focused = false, alpha = 1 } = {}) {
  const a = (focused ? 1 : SLIDER_DIM) * alpha;
  const w = x1 - x0;
  const tw = Math.max(thumb * w, 6);
  const tx = x0 + (w - tw) * Math.min(Math.max(f, 0), 1);
  quad(ctx, m, "slider_bg", x0, y0, x1, y1, [0, 0, 1, 1], tint(WHITE, a));
  quad(ctx, m, "slider_fg", tx, y0, tx + tw, y1, [0, 0, 1, 1], tint(WHITE, a));
}

// The four corners of a rectangle under m, in interface units: a hotspot.
export function corners(ctx, m, x0, y0, x1, y1) {
  const s = ctx.surface;
  return [s.toPlane(m, x0, y0), s.toPlane(m, x1, y0), s.toPlane(m, x1, y1), s.toPlane(m, x0, y1)];
}
