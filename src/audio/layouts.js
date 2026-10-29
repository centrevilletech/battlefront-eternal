// Output layouts. Channel order is what a browser's discrete multichannel
// output maps to the device: on Windows the WAVEFORMATEXTENSIBLE order (FL
// FR FC LFE BL BR SL SR, then the four tops), which Chromium passes through.
// Speakers are [azimuth, elevation] in degrees, azimuth positive to the
// left; the LFE has none. The speaker test (src/audio/panel.js) checks the
// order on the user's own setup.

export const LAYOUTS = {
  headphones: {
    label: "Headphones (binaural)",
    channels: 2,
    names: ["L", "R"],
    speakers: [[30, 0], [-30, 0]],
    lfe: -1,
    center: -1,
  },
  stereo: {
    label: "Stereo speakers",
    channels: 2,
    names: ["L", "R"],
    speakers: [[30, 0], [-30, 0]],
    lfe: -1,
    center: -1,
  },
  "5.1": {
    label: "5.1 surround",
    channels: 6,
    names: ["L", "R", "C", "LFE", "SL", "SR"],
    speakers: [[30, 0], [-30, 0], [0, 0], null, [110, 0], [-110, 0]],
    lfe: 3,
    center: 2,
  },
  "7.1": {
    label: "7.1 surround",
    channels: 8,
    names: ["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR"],
    speakers: [[30, 0], [-30, 0], [0, 0], null, [150, 0], [-150, 0], [90, 0], [-90, 0]],
    lfe: 3,
    center: 2,
  },
  "7.1.4": {
    label: "7.1.4 (with height)",
    channels: 12,
    names: ["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR", "TFL", "TFR", "TBL", "TBR"],
    speakers: [[30, 0], [-30, 0], [0, 0], null, [150, 0], [-150, 0], [90, 0], [-90, 0], [45, 45], [-45, 45], [135, 45], [-135, 45]],
    lfe: 3,
    center: 2,
  },
};

// The original mode's panning: OpenAL Soft 1.20.1's built-in speaker
// decoders (alc/panning.cpp, LGPL-2.0-or-later; see THIRD-PARTY-NOTICES.md),
// which the Steam build's OpenAL output
// pans every source with. A source's direction becomes 2D ambisonic
// channels (N3D) W, Y, X, V, U, Q, P (ACN 0, 1, 3, 4, 8, 9, 15), and each
// speaker takes its row's sum of them. Stereo is first order, 5.1 second, 7.1
// third. The centre and LFE have no row: the decoders never feed them; the
// front stabilizer (alsoft.ini's front-stablizer, on in the Steam build)
// gives the centre its share. On stereo a positioned source in front has its
// azimuth widened 1.5 times (to the side at 60 degrees), and a stream's
// channel angle 3 times (its +-30 degree pair hard left and right). 7.1.4 is
// beyond OpenAL Soft 1.20.1: the original plays it as 7.1, the heights
// silent.
const X51_FL = [1.8854286e-1, 1.27709292e-1, 1.66295695e-1, 7.30571517e-2, 2.10901184e-2];
const X51_SL = [3.33000782e-1, 1.89084803e-1, -2.00042375e-1, -2.12307769e-2, -1.14579885e-2];
const X71_BL = [2.04124145e-1, 1.08880247e-1, -1.8858612e-1, -1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0];
const X71_SL = [2.04124145e-1, 2.17760495e-1, 0, 0, -1.49071198e-1, -3.73460789e-2, 0];
const X71_FL = [2.04124145e-1, 1.08880247e-1, 1.8858612e-1, 1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0];
// The right speaker of a left one: Y, V, and Q change sign.
const mirror = (row) => row.map((g, k) => (k === 1 || k === 3 || k === 5 ? -g : g));
const X71 = [X71_FL, mirror(X71_FL), null, null, X71_BL, mirror(X71_BL), X71_SL, mirror(X71_SL)];
export const ALSOFT = {
  stereo: { front: 1.5, streamFront: 3, rows: [[0.5, 2.88675135e-1, 5.52305643e-2], [0.5, -2.88675135e-1, 5.52305643e-2]] },
  "5.1": { front: 0, streamFront: 0, rows: [X51_FL, mirror(X51_FL), null, null, X51_SL, mirror(X51_SL)] },
  "7.1": { front: 0, streamFront: 0, rows: X71 },
  "7.1.4": { front: 0, streamFront: 0, rows: [...X71, null, null, null, null] },
};
// Where OpenAL Soft puts a stream's channels (alc/alu.cpp): azimuth in
// degrees, positive to the left as in LAYOUTS. A mono stream plays ahead; a
// quad stream is a stereo pair and a rear pair (AL_FORMAT_REAR16).
export const ALSOFT_STREAM_ANGLES = { 1: [0], 2: [30, -30], 4: [30, -30, 150, -150] };

// The gains OpenAL Soft 1.20.1 gives a direction (x right, y up, z back,
// unit length) on a layout: the original mode's law, as the spatial mixer
// runs it; for tests and the speaker panel. `front` widens azimuths ahead.
export function alsoftGains(layout, x, y, z, front = ALSOFT[layout].front) {
  const { rows } = ALSOFT[layout];
  let az = Math.atan2(x, -z); // OpenAL Soft's: positive to the right
  const ev = Math.asin(Math.max(-1, Math.min(1, y)));
  if (front > 0 && Math.abs(az) < Math.PI / 2) az = Math.sign(az) * Math.min(Math.abs(az) * front, Math.PI / 2);
  const X = Math.cos(az) * Math.cos(ev);
  const Y = -Math.sin(az) * Math.cos(ev);
  const h = [1, Math.sqrt(3) * Y, Math.sqrt(3) * X, Math.sqrt(15) * X * Y, (Math.sqrt(15) / 2) * (X * X - Y * Y), Math.sqrt(35 / 8) * Y * (3 * X * X - Y * Y), Math.sqrt(35 / 8) * X * (X * X - 3 * Y * Y)];
  return rows.map((row) => (row ? row.reduce((s, g, k) => s + g * h[k], 0) : 0));
}

// The layout for a device's channel count, when the output is "auto".
export function autoLayout(maxChannels) {
  if (maxChannels >= 12) return "7.1.4";
  if (maxChannels >= 8) return "7.1";
  if (maxChannels >= 6) return "5.1";
  return "stereo";
}

// Gains of a direction over a layout's ear-level ring by pairwise panning
// (the same law the spatial mixer runs); for tests and the speaker panel.
export function vbap(layout, azimuthDeg) {
  const ring = layout.speakers
    .map((s, c) => (s && s[1] < 20 ? [c, (s[0] * Math.PI) / 180] : null))
    .filter(Boolean)
    .sort((a, b) => a[1] - b[1]);
  const out = new Array(layout.channels).fill(0);
  const az = (azimuthDeg * Math.PI) / 180;
  for (let i = 0; i < ring.length; i++) {
    let [c1, a1] = ring[i];
    let [c2, a2] = ring[(i + 1) % ring.length];
    if (i === ring.length - 1) a2 += 2 * Math.PI;
    let a = az;
    if (a < a1) a += 2 * Math.PI;
    if (a < a1 || a > a2) continue;
    if (a2 - a1 > Math.PI) {
      // A pair more than half a turn apart (stereo, seen from behind): fold
      // the source onto their front arc, front and back alike.
      const front = (a1 + a2) / 2 - Math.PI;
      a = 2 * (front + Math.PI / 2) - a;
      [c1, a1, c2, a2] = [c2, a2 - 2 * Math.PI, c1, a1];
      while (a > a2) a -= 2 * Math.PI;
      while (a < a1) a += 2 * Math.PI;
    }
    const det = Math.cos(a1) * Math.sin(a2) - Math.cos(a2) * Math.sin(a1);
    let g1 = (Math.cos(a) * Math.sin(a2) - Math.cos(a2) * Math.sin(a)) / det;
    let g2 = (Math.cos(a1) * Math.sin(a) - Math.cos(a) * Math.sin(a1)) / det;
    g1 = Math.max(g1, 0);
    g2 = Math.max(g2, 0);
    const n = Math.hypot(g1, g2) || 1;
    out[c1] += g1 / n;
    out[c2] += g2 / n;
    break;
  }
  return out;
}
