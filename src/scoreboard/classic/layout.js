// Where the Classic end-of-match table's pieces sit, in the HUD's interface
// space (1024 units across, H down; src/hud.js). The 2004 game's
// ifs_teamstats Lua lays them out inside the safe area (90% of the screen,
// from 5% in); the numbers are checked against the retail capture of
// Geonosis' end screen.
//
// - The window: border_3_pieces nine-sliced, the safe area's width and 0.82
//   of its height, centred at 0.48 of it down.
// - Two lists, team 1's in the left half and team 2's in the right, centred
//   at 0.53 down: rows 27 apart, as many as 0.85 of the window's height
//   holds, less one (16 at 4:3, 13 at 16:9).
// - Each row: the name from 2 units in, 0.65 of the row wide, and KILLS,
//   DEATHS and CP POINTS right-aligned in columns 0.133 of the row wide at
//   its right end.
// - Over each list, its title at the left and the three columns' icons, 30
//   units square; the scroll arrows at the list's middle, above and below.
// - The team logos at a quarter and three quarters across, H / 2 down.
// - DETAILS under the window's bottom edge, DONE at the bottom left and
//   AWARDS at the bottom right, on the safe area's bottom line less 15.
//
// Every value follows H (768 at 4:3, 624 at 16:9).

export const W = 1024;
export const SAFE = 0.05;
export const SAFE_SCALE = 0.9;
export const PITCH = 27; // the rows' height and spacing
export const ICON = 30; // a header icon's or arrow's side
// The columns' share of the row (val1str .. val3str), and where the
// header icons' centres sit across a team's half.
const COLUMN = 0.133;
const HEADER = [0.2, 0.32, 0.44];
// The original measures a label wider than its glyphs run: by 5 units for
// AWARDS, which ends that far short of its place, and by 8 for DETAILS,
// centred by its display box (measured on the retail captures).
export const STRING_PAD = 5;
export const DISPLAY_PAD = 8;

// fontHeight(name): a font's line height (gamefont_medium 23, small 21).
export function teamStatsLayout(H = 768) {
  const sw = SAFE_SCALE * W;
  const sh = SAFE_SCALE * H;
  const ox = SAFE * W;
  const oy = SAFE * H;
  const at = (rx, ry) => ({ x: ox + rx * sw, y: oy + ry * sh });
  const height = 0.82 * sh; // the window's
  const half = 0.5 * sw; // a team's half
  const window = { ...at(0.5, 0.48), w: sw, h: height };
  const lists = at(0.5, 0.53);
  const buttons = at(0.5, 0.5);
  const width = half - 20; // a row's
  const inner = width - 10;
  const column = COLUMN * inner;
  const shown = Math.floor((0.85 * height) / PITCH) - 1;
  const top = lists.y - (shown * PITCH) / 2 + PITCH / 2; // the first row's centre
  const sides = {};
  for (const [side, sign] of [[1, -1], [2, 1]]) {
    const x = lists.x + (sign * half) / 2; // the list's centre
    const left = x - inner / 2; // the rows' left edge
    sides[side] = {
      x,
      left,
      // A row's hotspot runs the row's width from its left edge.
      right: left + width,
      title: { x: x - half / 2 + 15, y: lists.y - height / 2, w: half / 2, h: 30 },
      name: { x: left + 2, w: 0.65 * inner },
      // Each column's right edge.
      columns: [3, 2, 1].map((k) => left + inner - column * (k - 1)),
      headers: HEADER.map((f) => ({ x: buttons.x + half * f - ICON / 2 + (sign * half) / 2, y: buttons.y - height / 2 + 0.03 * H })),
      up: { x: buttons.x + (sign * half) / 2, y: buttons.y - height / 2 + ICON * 2.5 - 10 },
      down: { x: buttons.x + (sign * half) / 2, y: buttons.y + height / 2 - ICON * 1.5 + 15 },
      logo: at(side === 1 ? 0.25 : 0.75, 0.5),
    };
  }
  return {
    H,
    safe: { x: ox, y: oy, w: sw, h: sh },
    window,
    sides,
    shown,
    top,
    width,
    // The selection's frame: the row's width and 10 more than its height,
    // 4 units below its centre (ListManager's cursor).
    cursor: { w: width, h: PITCH + 10, dy: 4 },
    // A text's line top, 10 units above its row's centre.
    textTop: -10,
    // The team logos' size parameter (W / 800).
    logoSize: W / 800,
    details: { x: buttons.x, y: buttons.y + height * 0.485 + 4 },
    done: { x: ox + 20, y: oy + sh - 15 },
    awards: { right: ox + sw - 15, y: oy + sh - 15 },
  };
}

// The row index (into the rows shown) under an interface point on a side,
// or -1.
export function rowAt(layout, side, x, y) {
  const list = layout.sides[side];
  if (x < list.left || x > list.right) return -1;
  const k = Math.floor((y - (layout.top - PITCH / 2)) / PITCH);
  return k >= 0 && k < layout.shown ? k : -1;
}

// DETAILS (ifs_personalstats) and AWARDS (ifs_awardstats): the table's
// window and dimming, a title centred at a tenth of the safe area down
// (DETAILS' second line, the row's name, 30 units under it), and a list of
// rows centred in the window: labels right-aligned ending `gap` before the
// middle, values left-aligned from `gap` after it. The rows sit `pitch`
// apart (their height and spacing), as many as the window holds less one;
// the first rows are blank under the title. AWARDS' icons sit in the gap,
// one per award row (their own formula, as the Lua has it, 10 units lower
// on a 768-unit screen only).
export const DETAILS_ROWS = { height: 26, pitch: 31, gap: 15, blank: 3, font: "gamefont_small" };
export const AWARDS_ROWS = { height: 28, pitch: 34, gap: 25, blank: 2, font: "gamefont_medium" };

export function statsPageLayout(H = 768, rows = DETAILS_ROWS) {
  const table = teamStatsLayout(H);
  const { x, y, h } = table.window;
  const shown = Math.floor(h / rows.pitch) - 1;
  const top = y - (shown * rows.pitch) / 2 + rows.height / 2; // the first row's centre
  const sh = table.safe.h;
  const title = { x: W / 2, y: table.safe.y + 0.1 * sh, w: 460 };
  // The award icons, k = 1..8 (rows 3..10).
  const icons = Array.from({ length: 8 }, (_, i) => ({
    x: W / 2,
    y: H / 2 - h / 2 + (1.8 + i + 1) * rows.pitch - 0.35 * rows.pitch * (H / 600 - 1) + (H === 768 ? 10 : 0),
  }));
  return {
    ...table,
    shown,
    rowTop: top,
    pitch: rows.pitch,
    labelRight: x - rows.gap,
    valueLeft: x + rows.gap,
    title,
    name: { x: W / 2, y: title.y + 30, w: 460 },
    icons,
    // Where DETAILS' logo flies: from its side of the table to the
    // window's top corner on that side, at half the size.
    corner: (side) => ({ x: W / 2 + (side === 1 ? -1 : 1) * 0.4 * table.safe.w, y: H / 2 - 0.3 * sh }),
  };
}
