// Shapes the interface's widgets are cut and laid out with: a window's
// 9-slice, text wrapped to a width, and a point-in-quad test for hotspots
// seen in perspective.

// A rectangle's 9-slice (16-unit corners; texture cut at 0.25 and 0.75):
// [x0, y0, x1, y1, u0, v0, u1, v1] per piece, centred on the origin. A side
// under 32 units loses its middle row or column.
export function nineSlice(w, h, corner = 16) {
  const xs = [-w / 2, -w / 2 + corner, w / 2 - corner, w / 2];
  const ys = [-h / 2, -h / 2 + corner, h / 2 - corner, h / 2];
  if (xs[2] < xs[1]) xs[1] = xs[2] = 0;
  if (ys[2] < ys[1]) ys[1] = ys[2] = 0;
  const us = [0, 0.25, 0.75, 1];
  const pieces = [];
  for (let j = 0; j < 3; j++) {
    for (let i = 0; i < 3; i++) {
      if (xs[i + 1] <= xs[i] || ys[j + 1] <= ys[j]) continue;
      pieces.push([xs[i], ys[j], xs[i + 1], ys[j + 1], us[i], us[j], us[i + 1], us[j + 1]]);
    }
  }
  return pieces;
}

// Text broken into lines that fit `width` (measure(text) gives a line's
// width); words longer than a line stand alone.
export function wrapLines(text, width, measure) {
  const lines = [];
  for (const paragraph of String(text ?? "").split(/\r?\n/)) {
    let line = "";
    for (const word of paragraph.split(/\s+/).filter(Boolean)) {
      const next = line ? `${line} ${word}` : word;
      if (line && measure(next) > width) {
        lines.push(line);
        line = word;
      } else {
        line = next;
      }
    }
    lines.push(line);
  }
  return lines;
}

// Inside the quad a, b, c, d (corners in order, either winding)?
export function insideQuad(x, y, corners) {
  let sign = 0;
  for (let i = 0; i < corners.length; i++) {
    const a = corners[i];
    const b = corners[(i + 1) % corners.length];
    const cross = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
    if (cross === 0) continue;
    if (!sign) sign = Math.sign(cross);
    else if (Math.sign(cross) !== sign) return false;
  }
  return true;
}
