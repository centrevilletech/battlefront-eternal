// Where the Classic spawn screen's pieces sit, in the HUD's interface space
// (1024 units across, H down; src/hud.js). The 2004 screen's Lua lays them
// out inside the safe area (90% of the screen, from 5% in) and the C++ adds
// the rest (checked against the retail screenshots):
//
// - Five class windows down the left, turned 35 degrees about their
//   vertical axis (right edge away), 0.35 x 0.13 of the safe area, a title
//   bar's height and 10 units apart.
// - The objectives and spawn point windows at the right, top and bottom,
//   turned -20 degrees, 0.3 x 0.15.
// - SPAWN at the bottom middle, 0.2 x 0.12, tipped 45 degrees about its
//   horizontal axis (top edge away); the word itself lies flat.
// - The team logos at the top middle, the selected team's on the left.
// - The class's model standing at (W/2, H/1.2), whatever the safe area.
//
// Every value scales with H (768 at 4:3, 624 at 16:9).

export const W = 1024;
export const SAFE = 0.05;
export const SAFE_SCALE = 0.9;
export const CLASS_TILT = (35 * Math.PI) / 180;
export const SIDE_TILT = (-20 * Math.PI) / 180;
export const SPAWN_TILT = (45 * Math.PI) / 180;
export const SLOTS = 5;

// fh: gamefont_tiny's line height (18; the title bars are that tall).
export function classicLayout(H = 768, fh = 18) {
  const sw = SAFE_SCALE * W;
  const sh = SAFE_SCALE * H;
  const ox = SAFE * W;
  const oy = SAFE * H;
  const slotW = 0.35 * sw;
  const slotH = 0.13 * sh;
  const slots = [];
  for (let i = 0; i < SLOTS; i++) {
    slots.push({
      x: ox + 0.175 * sw,
      y: oy + 0.125 * sh + i * (slotH + fh + 10),
      w: slotW,
      h: slotH,
      tilt: CLASS_TILT,
      // The C++ hit box: the full screen's 0.35 W wide, from 0.065 H + 32
      // above the centre (the title bar included) to 0.065 H below.
      hit: [-0.175 * W, -(0.065 * H + 32), 0.175 * W, 0.065 * H],
      textH: 0.08 * sh,
    });
  }
  const sideW = 0.3 * sw;
  const sideH = 0.15 * sh;
  return {
    H,
    fh,
    safe: { x: ox, y: oy, w: sw, h: sh },
    slots,
    // The C++ draws the weapon lists at 0.13 H / 135.
    infoScale: (0.13 * H) / 135,
    objectives: { x: ox + 0.85 * sw, y: oy + 0.125 * sh, w: sideW, h: sideH, tilt: SIDE_TILT },
    post: { x: ox + 0.85 * sw, y: oy + 0.925 * sh, w: sideW, h: sideH, tilt: SIDE_TILT },
    spawn: { x: ox + 0.5 * sw, y: oy + sh - 0.12 * sh + 50, w: 0.2 * sw, h: 0.12 * sh, tilt: SPAWN_TILT },
    // The logos' places (the selected team's left, the other right), the
    // size parameters they grow and shrink between, and the one hotspot
    // that switches team. The original's is a 38.4-unit square between the
    // logos, which only reaches their inner halves; this port's covers both
    // logos with a margin, clear of the windows and the preview.
    logos: {
      own: { x: ox + 0.45 * sw, y: oy + 0.13 * sh },
      other: { x: ox + 0.55 * sw, y: oy + 0.13 * sh },
      big: 0.5 * (sw / 800),
      small: 0.25 * (sw / 800),
      hit: { x: ox + 0.5 * sw, y: oy + 0.13 * sh, halfX: 0.05 * sw + 60, halfY: 55 },
    },
    preview: { x: W / 2, y: H / 1.2 },
  };
}
