import { loadShellData } from "./data.js";
import { CanvasFonts, tint } from "./text.js";
import { LoadingSound } from "./loading-audio.js";
import { Settings } from "../settings.js";
import { themeOf } from "../menus/themes/page/looks.js";

// The loading screen, shown over the battle page while the map loads, as the
// original's LoadDisplay draws it (Load/load.lvl and Load/<map>.lvl;
// docs/shell.md):
//
// - Four pictures, each filling the screen: the galaxy, the planet in space
//   (locale), its surface (landsat), and the level under the blue hologram
//   grid. On each of the first three a pair of cyan lines tracks onto the
//   next picture's rectangle (first the horizontal pair, then the vertical),
//   then the view zooms into it while the next picture grows out of it. The
//   lines ride the rectangle's edges out to the screen's.
// - The planet's name top left (gamefont_tiny), except over the level.
// - The LOADING disk bottom right: segments turning about a ring of lights
//   whose lit arc chases round it, each light behind the head a little
//   darker than the last (lightLevel), turned in perspective, LOADING
//   (gamefont_medium) across it.
// - The disk and the name in the front end's look (LOOKS, the menuTheme
//   setting): the 2004 look's are the original's; Eternal's and the 2005
//   look's take the page theme's colours and its Saira.
//
// The timeline is the original game's, on the clock alone (LoadingTimeline):
// each picture's lines track for TargetTime's 3.5 s (the horizontal pair in
// the first half, the vertical in the second), the zoom takes its 1 s, and
// the sequence ends as the last zoom does. The load doesn't pace it; the
// screen runs its whole course whatever the load's speed and goes two
// frames after both the sequence has ended and the battle is ready
// (handover()), at once, as the original cuts to the spawn screen. The same
// screen, length and sound serve a network battle (?network=1): the page's
// path is the same. ?loading=0 leaves it out.
//
// Its sound (loading-audio.js) comes from the same timeline: the
// load_ytracking pulse while the horizontal lines move, load_xtracking's
// while the vertical pair does, load_zoom as each zoom starts. Once the
// pictures are in, the page picks the moment the sequence starts and puts
// every sound of it on the AudioContext's own clock at once, then tells the
// screen to start: the sound can't drift from the pictures, however busy
// the page's thread gets while the battle loads.
//
// The battle's load holds the page's main thread for a second or two at a
// time, so the screen draws in a worker (loading-worker.js) on a canvas
// handed to it, where the browser has OffscreenCanvas; else on the page.
// Measured in Chromium, a blocked main thread leaves the worker's frames
// alone; what does hold them up is the GPU process, which serves the
// battle's shader links and texture uploads and the screen's canvas in
// turn (docs/shell.md). runView counts the hitches for checks.
//
// What makes it: shell.json's maps[].load (the four pictures and the
// rectangles) and load (the disk, its lights, the lines); without them it
// is the planet's name and the disk on black, the disk drawn from shapes.

// What each step of the battle's load counts for (createGame's onProgress;
// textures count from 0.5 to 0.8 as they arrive).
export const STEPS = { data: 0.3, world: 0.5, shaders: 0.85, ready: 1 };
// The original redraws the screen at most every 50 ms; its lines move, and
// its pulse starts and stops, on that step. A line is still once it moves
// less than a thousandth of the screen in one.
export const STEP = 0.05; // s
export const STILL = 0.001;
// How long the page leaves itself, between picking the sequence's start
// and the start, to put the sound on the audio clock and tell the screen.
export const LEAD = 0.1; // s

const IW = 1024;
// The disk's centre and the planet's name, in interface units, measured on
// the retail captures (4:3): the disk 252 units from the right and 223 up
// from the bottom, turned to 0.866 of its width, its 28 lights on a ring of
// 52 and LOADING tilted 14 degrees; the name 5 units into the safe area's
// top left corner. On other screens the disk keeps to the lower right
// corner (diskPlace).
const DISK = { right: 252, bottom: 223, turn: 0.866, ring: 52, leds: 28, text: -14,
  // [size, turn in radians a second, alpha] of each copy of the texture:
  // its thin ring (a quarter of its size out) lands at 60 and 44 units,
  // outside and inside the lights, as on the captures.
  layers: [[240, 0.25, 0.85], [176, -0.4, 0.75]] };
const NAME = { x: 51.2, top: 0.05, y: 5 };
const LINE = "rgba(40, 190, 255, 0.95)";
// The lights' lit arc, in turns of the ring: its head goes round 0.6 times
// a second; a light comes up over the light's spacing before the head
// reaches it (ramp: a 28th of a turn) and darkens all along the tail
// behind it, three quarters of the ring, faster at first (ease, a power).
export const LIGHTS = { speed: 0.6, ramp: 1 / 28, tail: 0.75, ease: 2 };
// How long the screen may outlast a ready battle waiting for its own
// sequence (pictures that never arrive), and the battle at most.
export const SEQUENCE_CAP = 30000; // ms
export const LOAD_CAP = 60000; // ms

// The disk and the name in each of the front end's looks
// (src/menus/themes/page/style.js):
// - lights: the lights' colours from off to the head, evenly spaced along
//   how lit they are; edge: a rim round each light, or none.
// - disk: the colours of the texture's segments and of its thin ring (one,
//   or one for each copy) at a share of its alpha, or null for its own
//   cyan; segments: the drawn disk's rings and its two layers, without the
//   texture.
// - text: LOADING's colour in the game's font. type: null for the game's
//   bitmap fonts (the original's), else the page's Saira: its weight,
//   LOADING's and the name's sizes (interface units), its letter spacing
//   (of the size), colour, outline ([colour, width]) and shadow ([colour,
//   blur]).
export const LOOKS = {
  // The original's: grey lights on the texture's cyan, the game's fonts.
  bf1: {
    lights: ["#0b0d0f", "#5c6166", "#b9bcbe"], edge: null,
    disk: null,
    segments: ["rgba(30, 170, 240, 0.9)", "rgba(40, 160, 220, 0.34)", "rgba(30, 140, 200, 0.28)"],
    text: "#dcdcff", type: null,
  },
  // Eternal: amber cooling to embers on black glass with pale rings,
  // off-white Saira spaced wide.
  eternal: {
    lights: ["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"], edge: null,
    disk: { segments: "#05070b", ring: "#c4ccd8", alpha: 0.85 },
    segments: ["rgba(196, 204, 216, 0.75)", "rgba(5, 7, 11, 0.3)", "rgba(5, 7, 11, 0.25)"],
    text: "#e8ecf2",
    type: { weight: 700, size: 13, name: 15, spacing: 0.24, color: "#e8ecf2", outline: null, shadow: ["rgba(0, 0, 0, 0.85)", 3] },
  },
  // Battlefront II: red on navy glass with silver rings, white Saira
  // outlined in black, as its buttons.
  bf2: {
    lights: ["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"], edge: "#06080c",
    disk: { segments: "#1d3f73", ring: "#c9d4e2", alpha: 0.9 },
    segments: ["rgba(201, 212, 226, 0.85)", "rgba(29, 63, 115, 0.34)", "rgba(29, 63, 115, 0.28)"],
    text: "#ffffff",
    type: { weight: 800, size: 15, name: 16, spacing: 0.04, color: "#ffffff", outline: ["#111111", 2], shadow: ["rgba(0, 0, 0, 0.6)", 3] },
  },
};
// The page theme's typeface (electron/ui/fonts, which the desktop loader
// shares).
const SAIRA = "BFE Saira";

// The planet each map is on, for its name when shell.json doesn't give it.
const PLANETS = {
  bes: ["bespin", "BESPIN"], end: ["endor", "ENDOR"], geo: ["geonosis", "GEONOSIS"], hot: ["hoth", "HOTH"],
  kam: ["kamino", "KAMINO"], kas: ["kashyyyk", "KASHYYYK"], nab: ["naboo", "NABOO"], rhn: ["rhenvar", "RHEN VAR"],
  tat: ["tatooine", "TATOOINE"], yav: ["yavin", "YAVIN 4"],
};

export function planetName(map, strings = {}, load = null) {
  if (load?.planet) return load.planet;
  const planet = PLANETS[String(map).slice(0, 3)];
  if (!planet) return String(map).toUpperCase();
  return strings[`planets.${planet[0]}.name`] || planet[1];
}

const clamp01 = (t) => Math.min(Math.max(t, 0), 1);
// The page's clock, the same in the page and its workers (ms).
export const clock = () => performance.timeOrigin + performance.now();

// How much of a line's way is left t s into a half of h s. Every step the
// original moves each line t/h of what is left (the share grows from almost
// nothing to all of it), so the way left after k steps is the product of
// (1 - j step / h) for j up to k. This is that product's logarithm summed as
// an integral: equal to it on the steps, smooth between them.
export function approach(t, h, step = STEP) {
  if (t <= 0) return 1;
  if (t + step / 2 >= h) return 0;
  const F = (s) => -(h - s) * Math.log(1 - s / h) - s;
  return Math.exp((F(t + step / 2) - F(step / 2)) / step);
}

// When the pulse sounds in a half of h s whose leading line travels
// `travel` (of the screen): the original plays it from the step in which
// the line moved more than STILL and stops it at the first that moved
// less, so it sounds while the line can be seen moving. [from, to] s into
// the half, or null for a line too close to move.
export function pulse(travel, h, step = STEP, still = STILL) {
  const steps = Math.ceil(h / step - 1e-9);
  let from = null;
  for (let k = 1; k <= steps; k++) {
    const moved = Math.abs(travel) * (approach((k - 1) * step, h, step) - approach(Math.min(k * step, h), h, step));
    if (moved > still && from === null) from = (k - 1) * step;
    else if (moved <= still && from !== null) return [from, (k - 1) * step];
  }
  return from === null ? null : [from, h];
}

// The pictures' timeline, pure, on the clock alone, as LoadDisplay runs it:
// on each picture but the last the horizontal lines track for the first
// half of `track` s, the vertical for the second, then the view zooms for
// `zoom` s and the next picture takes over; the sequence ends `hold` s
// after the last zoom (the original: at once). targets: for each picture,
// its rectangle of the next as fractions of the picture [x0, y0, x1, y1]
// (null for the last).
export class LoadingTimeline {
  constructor({ track = 3.5, zoom = 1, hold = 0, targets = [] } = {}) {
    this.track = Math.max(Number(track) || 0, STEP * 2);
    this.zoom = Math.max(Number(zoom) || 0, 0.05);
    this.hold = Math.max(Number(hold) || 0, 0);
    this.targets = targets;
  }

  get stages() {
    return this.targets.length;
  }

  // Seconds from the start to the end of the sequence.
  get duration() {
    return Math.max(this.stages - 1, 0) * (this.track + this.zoom) + this.hold;
  }

  // Where it is t s in: { stage, phase (track, zoom, hold), time into the
  // phase, lines: { y, x } each 0 (at the screen's edges) to 1 (on the
  // rectangle), x null before the vertical pair moves, or null; zoom 0-1
  // or null; done }.
  at(t) {
    t = Math.max(Number(t) || 0, 0);
    const cycle = this.track + this.zoom;
    const last = Math.max(this.stages - 1, 0);
    if (t >= last * cycle) return { stage: last, phase: "hold", time: t - last * cycle, lines: null, zoom: null, done: t >= this.duration };
    const stage = Math.floor(t / cycle);
    const time = t - stage * cycle;
    if (time >= this.track) return { stage, phase: "zoom", time: time - this.track, lines: { y: 1, x: 1 }, zoom: clamp01((time - this.track) / this.zoom), done: false };
    const half = this.track / 2;
    return { stage, phase: "track", time, lines: { y: 1 - approach(time, half), x: time < half ? null : 1 - approach(time - half, half) }, zoom: null, done: false };
  }

  // Its sounds, as LoadDisplay plays them: [{ cue, at, until }] in s from
  // the start (until null for a one-shot): each pulse while its leading
  // line (the top one, then the left) moves, the zoom as it starts, the
  // transition as it ends.
  sounds() {
    const out = [];
    const cycle = this.track + this.zoom;
    const half = this.track / 2;
    for (let n = 0; n < this.stages - 1; n++) {
      const base = n * cycle;
      const [x0, y0] = this.targets[n] || [0, 0];
      const y = pulse(y0, half);
      if (y) out.push({ cue: "ytrack", at: base + y[0], until: base + y[1] });
      const x = pulse(x0, half);
      if (x) out.push({ cue: "xtrack", at: base + half + x[0], until: base + half + x[1] });
      out.push({ cue: "zoom", at: base + this.track, until: null });
      out.push({ cue: "transition", at: base + cycle, until: null });
    }
    return out;
  }

  // Which pulse sounds t s in (ytrack, xtrack), zoom during a zoom, else
  // still: for checks.
  cue(t, sounds = this.sounds()) {
    const loop = sounds.find((s) => s.until !== null && t >= s.at && t < s.until);
    if (loop) return loop.cue;
    return this.at(t).phase === "zoom" ? "zoom" : "still";
  }
}

// Before the sequence starts: the first picture (once it is in), still.
const WAIT = { stage: 0, phase: "wait", time: 0, lines: null, zoom: null, done: false };

// The part of a picture (iw x ih) in view u of the way through a zoom into
// target [x, y, w, h]: its size shrinks at an even rate of scale, and its
// centre moves in step with the size.
export function zoomView(u, iw, ih, target) {
  const [tx, ty, tw, th] = target;
  const w = Math.exp(Math.log(iw) + (Math.log(Math.max(tw, 1)) - Math.log(iw)) * u);
  const h = Math.exp(Math.log(ih) + (Math.log(Math.max(th, 1)) - Math.log(ih)) * u);
  const k = iw === tw ? u : (iw - w) / (iw - tw);
  const cx = iw / 2 + (tx + tw / 2 - iw / 2) * k;
  const cy = ih / 2 + (ty + th / 2 - ih / 2) * k;
  return [cx - w / 2, cy - h / 2, w, h];
}

// Where a rectangle of the picture lands on a w x h screen when the view
// [vx, vy, vw, vh] of it fills the screen.
export function onScreen(rect, view, w, h) {
  const [x, y, rw, rh] = rect;
  const [vx, vy, vw, vh] = view;
  return [((x - vx) / vw) * w, ((y - vy) / vh) * h, (rw / vw) * w, (rh / vh) * h];
}

// Where the disk goes on a w x h screen: its centre (pixels) and the
// pixels to one of its units. It keeps to the lower right corner, as on the
// 4:3 captures, at any shape of screen: its offsets from the corner and its
// size scale with the screen's height (with its width on a screen narrower
// than 4:3), never stretched, so a wide screen doesn't carry it in towards
// the middle.
export function diskPlace(w, h) {
  const unit = Math.min(w / IW, h / 768);
  return { x: w - DISK.right * unit, y: h - DISK.bottom * unit, unit };
}

// How lit a light is, from 0 (off) to 1 (the head), `behind` turns behind
// the lit arc's head (any number: a whole turn is the ring). It comes up in
// a smooth step over the ramp just ahead of the head, then darkens all
// along the tail, every light behind the head a little darker than the one
// before it, to off. It is continuous all round, so as the head moves each
// light fades in and out instead of jumping between shades.
export function lightLevel(behind, { ramp = LIGHTS.ramp, tail = LIGHTS.tail, ease = LIGHTS.ease } = {}) {
  const b = behind - Math.floor(behind);
  if (b > 1 - ramp) {
    const u = (b - (1 - ramp)) / ramp;
    return u * u * (3 - 2 * u);
  }
  return b < tail ? (1 - b / tail) ** ease : 0;
}

// The colour `level` (0 to 1) of the way along evenly spaced colour stops
// ("#rrggbb"), mixed between the two it falls between.
export function mixColor(stops, level) {
  const at = Math.min(Math.max(level, 0), 1) * (stops.length - 1);
  const i = Math.min(Math.floor(at), stops.length - 2);
  const [a, b] = [stops[i], stops[i + 1]].map((hex) => [1, 3, 5].map((k) => parseInt(hex.slice(k, k + 2), 16)));
  return `rgb(${a.map((v, k) => Math.round(v + (b[k] - v) * (at - i))).join(", ")})`;
}

// Which of the front end's looks (LOOKS) the screen takes: the player's
// (the menuTheme setting, `theme`), or the original's (bf1) under the
// original's menus (?menus=classic).
export function loadingLook(params, theme) {
  return params.get("menus") === "classic" ? "bf1" : themeOf(theme);
}

// Saira, for the looks set in it, added to this thread's fonts (a worker
// has its own). Resolves with whether it loaded.
function loadSaira() {
  const fonts = globalThis.document?.fonts ?? globalThis.fonts;
  if (typeof FontFace !== "function" || !fonts) return Promise.resolve(false);
  const url = new URL("../../electron/ui/fonts/saira-latin.woff2", import.meta.url);
  const face = new FontFace(SAIRA, `url(${url.href})`, { weight: "100 900", stretch: "50% 125%" });
  return face.load().then(() => {
    fonts.add(face);
    return true;
  }, () => false);
}

// A copy of the disk's texture (one colour, its shapes in its alpha) in a
// look's colours for layer i: its segments in one, its thin ring (a
// quarter of its size out, two of its 256 pixels wide) in another. Each
// is a colour, or one per layer.
function recolor(image, { segments, ring }, i) {
  const pick = (value) => (Array.isArray(value) ? value[i] : value);
  const copy = tint(image, pick(segments));
  const c = copy.getContext("2d");
  const r = copy.width / 2;
  c.globalCompositeOperation = "source-atop";
  c.fillStyle = pick(ring);
  c.beginPath();
  c.arc(r, r, copy.width * 0.258, 0, Math.PI * 2);
  c.arc(r, r, copy.width * 0.243, 0, Math.PI * 2, true);
  c.fill();
  return copy;
}

// A picture as an ImageBitmap, or null (a missing file is Vite's index page).
function bitmap(url) {
  if (!url) return Promise.resolve(null);
  return fetch(url)
    .then((r) => (r.ok && !/text\/html/.test(r.headers.get("content-type") || "") ? r.blob() : Promise.reject(new Error(url))))
    .then((blob) => createImageBitmap(blob))
    .catch(() => null);
}

// The screen itself: its pictures, art and timeline, drawn onto a 2D
// context (the page's or a worker's), the disk and the name in `look`
// (LOOKS; loadingLook()).
export class LoadingView {
  constructor(map, { look = null } = {}) {
    this.map = String(map || "nab2").toLowerCase();
    this.timeline = new LoadingTimeline();
    this.stages = []; // [{ image, target }]
    this.art = { disk: null, horz: null, vert: null, backdrop: null };
    this.name = planetName(this.map);
    this.text = "LOADING";
    this.fonts = null;
    this.look = LOOKS[themeOf(look)];
    // The disk's texture in the look's colours, a copy for each layer.
    this.disks = null;
    // The look's Saira: null for the game's fonts, else "loading", "ready"
    // or "failed" (the game's fonts stand in).
    this.typeface = this.look.type ? "loading" : null;
    this.spin = 0;
    // When the sequence started (clock(), ms), once the page says (go()).
    this.start = null;
    this.now = WAIT;
    this.sounds = [];
    // Pictures to draw once before the sequence starts (prime()).
    this.priming = null;
    // For checks: each sound of the plan with the first frame drawn at or
    // after it ([cue, at, frame], clock() ms).
    this.shown = [];
  }

  // The map's pictures and the screen's art (the look's typeface comes in
  // on its own). Resolves with the sequence's plan once the pictures (and
  // the lines' art) are in: the run of pictures that loaded, from the
  // first.
  async load(shell = null) {
    if (this.typeface) loadSaira().then((ok) => { this.typeface = ok ? "ready" : "failed"; });
    shell ||= await loadShellData();
    const entry = shell.maps.find((m) => m.id === this.map);
    this.name = planetName(this.map, shell.strings, entry?.load);
    this.text = shell.strings[shell.load.text] || "LOADING";
    const { load } = shell;
    this.fonts = new CanvasFonts({ fonts: shell.fonts, pages: shell.pages });
    this.fonts.load(["gamefont_tiny", "gamefont_medium"]);
    const stages = entry?.load?.stages || [];
    const art = Promise.all([load.disk, load.edges.horz, load.edges.vert, load.backdrop].map(bitmap));
    const pictures = await Promise.all(stages.map((stage) => bitmap(stage.url)));
    const [disk, horz, vert, backdrop] = await art;
    this.art = { disk, horz, vert, backdrop };
    // The disk's copies in the look's colours.
    this.disks = disk && this.look.disk ? DISK.layers.map((_, i) => recolor(disk, this.look.disk, i)) : null;
    let count = 0;
    while (count < pictures.length && pictures[count]) count += 1;
    this.stages = stages.slice(0, count).map((stage, i) => ({ image: pictures[i], target: i < count - 1 ? stage.target : null }));
    // The rectangles as fractions of their pictures.
    const targets = this.stages.map(({ image, target }) => (target ? [target[0] / image.width, target[1] / image.height, (target[0] + target[2]) / image.width, (target[1] + target[3]) / image.height] : null));
    this.timeline = new LoadingTimeline({ track: load.track, zoom: load.zoom, hold: load.hold, targets });
    this.sounds = this.timeline.sounds();
    await this.prime([...this.stages.map((stage) => stage.image), ...(this.disks || [disk]), horz, vert].filter(Boolean));
    return this.plan();
  }

  // Draws each image once, a pixel big, one a frame, and resolves a frame
  // after the last: their uploads to the GPU are done before the sequence
  // starts, a few megabytes at a time, not on the frames that first show
  // them (or after a second without frames).
  prime(images) {
    return new Promise((resolve) => {
      const timer = setTimeout(() => { this.priming = null; resolve(); }, 1000);
      this.priming = { images, next: 0, resolve: () => { clearTimeout(timer); resolve(); } };
    });
  }

  // What the page needs to run the sequence: its length and its sounds.
  plan() {
    return { stages: this.timeline.stages, duration: this.timeline.duration, sounds: this.sounds };
  }

  // The sequence starts at `start` (clock() ms).
  go(start) {
    this.start = start;
    this.shown = this.sounds.map((sound) => [sound.cue, start + sound.at * 1000, null]);
  }

  // The frame at `now` (clock() ms), dt s after the last.
  update(dt, now = clock()) {
    this.spin += dt;
    this.now = this.start === null ? WAIT : this.timeline.at((now - this.start) / 1000);
    for (const event of this.shown) if (event[2] === null && now >= event[1]) event[2] = now;
  }

  draw(ctx, w, h) {
    const now = this.now;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    const priming = this.priming;
    if (priming && priming.next < priming.images.length) {
      ctx.drawImage(priming.images[priming.next++], 0, 0, 1, 1);
    } else if (priming) {
      this.priming = null;
      priming.resolve();
    }
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, w, h);
    const shown = this.stages[now.stage];
    if (shown?.image) this.drawPictures(ctx, shown, this.stages[now.stage + 1], w, h);
    else if (this.art.backdrop) ctx.drawImage(this.art.backdrop, 0, 0, w, h);
    // Interface units from here: 1024 across, H down (as the HUD's).
    const H = 768 + (h / w - 0.75) * 768;
    ctx.setTransform(w / IW, 0, 0, h / H, 0, 0);
    // Pixels to the unit, for the type's shadows (the transform leaves
    // them alone).
    this.scale = h / H;
    if (now.phase !== "hold" || !shown?.image) {
      // gamefont_tiny's capitals stand from 5 to 19 units under the line's
      // top.
      const top = NAME.top * H + NAME.y;
      if (this.typeface === "ready") this.write(ctx, NAME.x, top + 12, this.name, this.look.type.name, "left");
      else if (this.typeface !== "loading" && this.fonts?.has("gamefont_tiny")) this.fonts.draw(ctx, NAME.x, top, this.name, "gamefont_tiny", { color: this.look.type?.color });
    }
    const disk = diskPlace(w, h);
    ctx.setTransform(disk.unit, 0, 0, disk.unit, disk.x, disk.y);
    this.scale = disk.unit;
    this.drawDisk(ctx, 0, 0);
  }

  // The stage's picture (zooming, if it is), the next growing out of its
  // rectangle, and the tracking lines.
  drawPictures(ctx, shown, next, w, h) {
    const { image: picture, target } = shown;
    const now = this.now;
    const zoom = now.zoom;
    const view = zoom !== null && target ? zoomView(zoom, picture.width, picture.height, target) : [0, 0, picture.width, picture.height];
    ctx.drawImage(picture, view[0], view[1], view[2], view[3], 0, 0, w, h);
    if (!target || !now.lines) return;
    const [x, y, rw, rh] = onScreen(target, view, w, h);
    if (zoom !== null && next?.image) {
      ctx.globalAlpha = clamp01(zoom / 0.2);
      ctx.drawImage(next.image, x, y, rw, rh);
      ctx.globalAlpha = 1;
    }
    const lines = now.lines;
    // The lines are 2 interface units thick.
    const width = Math.max(1.5, (2 * w) / IW);
    const across = (at) => this.line(ctx, 0, at, w, at, width);
    const down = (at) => this.line(ctx, at, 0, at, h, width);
    across(y * lines.y);
    across(h - (h - (y + rh)) * lines.y);
    if (lines.x !== null) {
      down(x * lines.x);
      down(w - (w - (x + rw)) * lines.x);
    }
  }

  line(ctx, x0, y0, x1, y1, width) {
    const across = y0 === y1;
    const edge = across ? this.art.horz : this.art.vert;
    if (edge) {
      // load_edge_horz / _vert: a line an eighth of the texture thick
      // along its middle, stretched along the line.
      if (across) ctx.drawImage(edge, 0, y0 - width * 4, x1 - x0, width * 8);
      else ctx.drawImage(edge, x0 - width * 4, 0, width * 8, y1 - y0);
      return;
    }
    ctx.strokeStyle = LINE;
    ctx.lineWidth = width;
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.stroke();
  }

  // The LOADING disk, in interface units about its centre, turned about its
  // vertical axis.
  drawDisk(ctx, cx, cy) {
    const spin = this.spin;
    const look = this.look;
    ctx.save();
    ctx.translate(cx, cy);
    ctx.scale(DISK.turn, 1);
    // load_rotating_disk twice, turning both ways, in the look's colours.
    if (this.art.disk) {
      DISK.layers.forEach(([size, speed, alpha], i) => {
        ctx.save();
        ctx.rotate(spin * speed);
        ctx.globalAlpha = alpha * (look.disk?.alpha ?? 1);
        ctx.drawImage(this.disks?.[i] || this.art.disk, -size / 2, -size / 2, size, size);
        ctx.restore();
      });
    } else {
      this.drawSegments(ctx);
    }
    ctx.globalAlpha = 1;
    // The lights (load_led_0 is a plain square, tinted): a lit arc chasing
    // round the ring, each light's colour mixed from how lit it is.
    const head = spin * LIGHTS.speed;
    for (let i = 0; i < DISK.leds; i++) {
      ctx.save();
      ctx.rotate((i / DISK.leds) * Math.PI * 2);
      ctx.translate(DISK.ring, 0);
      if (look.edge) {
        ctx.fillStyle = look.edge;
        ctx.fillRect(-4.5, -4.5, 9, 9);
      }
      ctx.fillStyle = mixColor(look.lights, lightLevel(head - i / DISK.leds));
      ctx.fillRect(-3.5, -3.5, 7, 7);
      ctx.restore();
    }
    // LOADING across the middle, tilted.
    ctx.rotate((DISK.text * Math.PI) / 180);
    if (this.typeface === "ready") this.write(ctx, 0, 0, this.text, look.type.size, "center");
    else if (this.typeface !== "loading" && this.fonts?.has("gamefont_medium")) this.fonts.draw(ctx, 0, -12, this.text, "gamefont_medium", { align: 2, color: look.text });
    ctx.restore();
  }

  // Text in the look's Saira, its capitals' middle at y, from x (align
  // "left") or about it ("center"), with the look's outline and shadow.
  write(ctx, x, y, text, size, align) {
    const type = this.look.type;
    const spacing = type.spacing * size;
    ctx.save();
    ctx.font = `${type.weight} ${size}px "${SAIRA}"`;
    ctx.letterSpacing = `${spacing}px`;
    ctx.textAlign = align;
    ctx.textBaseline = "alphabetic";
    const box = ctx.measureText(text);
    // Centred, the spacing after the last letter left out.
    const from = align === "center" ? x + spacing / 2 : x;
    const base = y + (box.actualBoundingBoxAscent - box.actualBoundingBoxDescent) / 2;
    if (type.shadow) {
      ctx.shadowColor = type.shadow[0];
      ctx.shadowBlur = type.shadow[1] * (this.scale || 1);
      ctx.shadowOffsetY = this.scale || 1;
    }
    if (type.outline) {
      ctx.lineJoin = "round";
      ctx.strokeStyle = type.outline[0];
      ctx.lineWidth = type.outline[1];
      ctx.strokeText(text, from, base);
      ctx.shadowColor = "transparent";
    }
    ctx.fillStyle = type.color;
    ctx.fillText(text, from, base);
    ctx.restore();
  }

  // Without the disk's texture: its translucent segments and thin rings,
  // drawn in the look's colours.
  drawSegments(ctx) {
    const [ring, ...fills] = this.look.segments;
    ctx.strokeStyle = ring;
    ctx.lineWidth = 2;
    for (const r of [60, 44]) {
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.stroke();
    }
    const pieces = [[0.1, 1.0, 74, 118], [1.5, 0.7, 80, 104], [2.6, 1.1, 66, 96], [3.9, 0.5, 78, 122], [4.8, 0.9, 70, 100], [5.7, 0.4, 84, 110]];
    for (const [speed, layer] of [[0.35, 0], [-0.55, 1]]) {
      ctx.save();
      ctx.rotate(this.spin * speed + layer);
      ctx.fillStyle = fills[layer];
      const scale = layer ? 0.75 : 1;
      for (const [start, length, inner, outer] of pieces) {
        ctx.beginPath();
        ctx.arc(0, 0, outer * scale, start, start + length);
        ctx.arc(0, 0, inner * scale, start + length, start, true);
        ctx.closePath();
        ctx.fill();
      }
      ctx.restore();
    }
  }

  // What the checks read: where the pictures are, whether the sequence has
  // started and run its course, which sound goes with it now, the frames
  // that showed each sound's moment, and how smoothly the screen has drawn
  // (runView's count).
  state() {
    const now = this.now;
    const t = this.start === null ? 0 : (clock() - this.start) / 1000;
    return { stage: now.stage, stages: this.timeline.stages, phase: now.phase, done: this.start !== null && now.done, started: this.start !== null,
      cue: this.start === null ? "still" : this.timeline.cue(t, this.sounds), shown: this.shown, frames: this.frames || null };
  }
}

// Draws a view on each of its own animation frames (the drawing is a few
// images and squares, cheap at any refresh rate); returns a function that
// stops it. size() gives the canvas's pixel size.
export function runView(view, canvas, size, frame = (fn) => requestAnimationFrame(fn)) {
  const ctx = canvas.getContext("2d");
  let last = null;
  let stopped = false;
  // The screen's own smoothness, for checks: animation frames it was given,
  // the longest wait between two (ms), how many waits passed 50 ms (a
  // visible hitch at any refresh rate), and when those were ([ms, gap]).
  const frames = (view.frames = { count: 0, worst: 0, late: 0, hitches: [] });
  const step = (time) => {
    if (stopped) return;
    frame(step);
    // The frame's own time where the browser gives one.
    const now = Number.isFinite(time) ? time : performance.now();
    const gap = last === null ? 0 : now - last;
    if (frames.count > 0) {
      if (gap > frames.worst) frames.worst = gap;
      if (gap > 50) {
        frames.late += 1;
        if (frames.hitches.length < 64) frames.hitches.push([Math.round(now), Math.round(gap)]);
      }
    }
    frames.count += 1;
    view.update(Math.min(Math.max(gap / 1000, 0), 0.25), performance.timeOrigin + now);
    last = now;
    const [w, h] = size();
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    view.draw(ctx, w, h);
  };
  frame(step);
  return () => { stopped = true; };
}

// When the screen hands over, pure: once the battle is loaded (or `load` ms
// pass) and the screen's sequence is done (or `cap` ms more pass). isDone is
// polled each `tick` (a promise of the next moment to look); onReady runs
// as soon as the battle is loaded (the progress report). Resolves with
// whether the battle was loaded in time.
export async function handover({ loaded, isDone, tick, onReady = () => {}, load = LOAD_CAP, cap = SEQUENCE_CAP, now = () => performance.now() }) {
  let timer = null;
  const expiry = new Promise((resolve) => { timer = setTimeout(() => resolve(false), load); });
  const settled = await Promise.race([Promise.resolve(loaded).then(() => true, () => false), expiry]);
  clearTimeout(timer);
  onReady();
  const until = now() + cap;
  while (!isDone() && now() < until) await tick();
  return settled;
}

// Without the screen (?loading=0) the battle starts as soon as it is built.
const nothing = { progress() {}, done(game) { game?.begin?.(); return Promise.resolve(); }, close() {}, closed: true, value: 1, sequence: null, sound: null };

// The loading screen for this page's battle. Returns { progress(step,
// loaded, total), done(game), close(), value, sequence, sound }: progress is
// createGame's onProgress; done closes it once the game is ready, has drawn
// and the sequence has run its course; sequence is the last { stage, phase,
// done, cue } seen, for checks; sound the screen's LoadingSound.
export function createLoadingScreen({ query = location.search, parent = document.body, worker = true, sound = true } = {}) {
  const params = new URLSearchParams(query);
  if (params.get("loading") === "0") return nothing;
  const map = (params.get("map") || "nab2").toLowerCase();
  const canvas = document.createElement("canvas");
  canvas.id = "loading";
  canvas.style.cssText = "position:fixed;inset:0;width:100%;height:100%;z-index:20;pointer-events:none;background:#000";
  parent.append(canvas);
  const size = () => {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    return [Math.max(1, Math.round(window.innerWidth * ratio)), Math.max(1, Math.round(window.innerHeight * ratio))];
  };
  // The pulse and the zooms, at the player's volumes (?sound=0 for none).
  let settings = null;
  try { settings = new Settings(); } catch { settings = null; }
  // The disk in the player's look of the front end (the page is a fresh
  // one, without the front end's look on it, and a worker can't read the
  // settings).
  const look = loadingLook(params, settings?.get("menuTheme"));
  const audio = sound && params.get("sound") !== "0"
    ? new LoadingSound({ volumes: () => ({ master: settings?.get("masterVolume") ?? 1, interface: settings?.get("interfaceVolume") ?? 1 }) }).start()
    : null;

  let progress = 0;
  let closed = false;
  const screen = {
    canvas,
    // The look the disk is drawn in (LOOKS).
    look,
    // The last { stage, phase, done, cue, started, shown } the view
    // reported.
    sequence: { stage: 0, phase: "wait", done: false, cue: "still", started: false },
    // The sequence's plan ({ stages, duration, sounds }), when it started
    // and when it ends (clock() ms), once the pictures are in.
    plan: null,
    start: null,
    endsAt: null,
    // How smoothly it has drawn: { count, worst, late, hitches } (runView).
    frames: null,
    sound: audio,
    // The load's steps as they came: [step, ms since the page started].
    steps: [],
    progress: report,
    get value() {
      return progress;
    },
    get closed() {
      return closed;
    },
    // The battle is built: close once everything it will draw is loaded and
    // on the GPU (game.loaded(); or after 60 s whatever happens) and the
    // screen's own sequence has run its course (or 30 s more), then start it
    // (game.begin()): nothing moves while this screen shows, and the battle
    // isn't drawn under it until its last two frames (game.unveil()).
    done(game) {
      report("shaders");
      const loaded = game?.loaded ? game.loaded() : Promise.resolve(game?.ready);
      return handover({
        loaded,
        onReady: () => report("ready"),
        isDone: () => closed || (screen.endsAt !== null && clock() >= screen.endsAt),
        tick: () => new Promise((resolve) => setTimeout(resolve, 50)),
      }).then(() => new Promise((resolve) => {
        game?.unveil?.();
        requestAnimationFrame(() => requestAnimationFrame(() => {
          close();
          game?.begin?.();
          resolve();
        }));
      }));
    },
    close,
  };
  // The pictures are in: the sequence starts LEAD from now, its sounds put
  // on the audio clock first.
  function go(plan) {
    if (closed || screen.start !== null) return;
    const start = clock() + LEAD * 1000;
    screen.plan = plan;
    screen.start = start;
    screen.endsAt = start + plan.duration * 1000;
    audio?.schedule(plan.sounds, start);
    send({ type: "go", start });
  }
  // Draws on the page, or tells the worker.
  let send = () => {};
  let stop = () => {};
  const offscreen = worker && typeof canvas.transferControlToOffscreen === "function" && typeof Worker === "function";
  if (offscreen) {
    const thread = new Worker(new URL("./loading-worker.js", import.meta.url), { type: "module" });
    const surface = canvas.transferControlToOffscreen();
    [surface.width, surface.height] = size();
    thread.postMessage({ type: "start", canvas: surface, map, look }, [surface]);
    thread.onmessage = (event) => {
      const message = event.data || {};
      if (message.type === "ready") {
        go(message.plan);
      } else if (message.type === "state") {
        screen.sequence = message.state;
        screen.frames = message.state.frames;
      }
    };
    send = (message) => thread.postMessage(message);
    stop = () => thread.terminate();
  } else {
    const view = new LoadingView(map, { look });
    view.load().then(go, (error) => {
      console.warn(`loading screen: ${error.message}`);
      go(view.plan());
    });
    const halt = runView(view, canvas, size);
    const poll = setInterval(() => { screen.sequence = view.state(); screen.frames = view.frames; }, 100);
    send = (message) => {
      if (message.type === "go") view.go(message.start);
    };
    stop = () => {
      clearInterval(poll);
      halt();
    };
  }
  const onResize = () => send({ type: "size", size: size() });
  window.addEventListener("resize", onResize);

  // createGame's onProgress: a step of the load done, or textures loaded
  // of total.
  function report(step, loaded, total) {
    if (step !== "textures") screen.steps.push([step, Math.round(performance.now())]);
    if (step === "textures") {
      if (total > 0) progress = Math.max(progress, Math.min(0.8, 0.5 + (0.3 * loaded) / total));
    } else if (STEPS[step] !== undefined) progress = Math.max(progress, STEPS[step]);
  }

  function close() {
    if (closed) return;
    closed = true;
    window.removeEventListener("resize", onResize);
    audio?.dispose();
    stop();
    canvas.remove();
  }

  return screen;
}
