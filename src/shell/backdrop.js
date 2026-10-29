import { SharpVideo } from "./video-sharpen.js";
import { SHARPEN } from "./movies.js";
import { resolveUrl } from "./data.js";
import { PREVIEW_DELAY } from "./media.js";

// The front end's backdrop under the page theme (src/menus/themes/page): a
// movie fading in down each side of the screen, the Clone Wars on the left
// and the Galactic Civil War on the right, playing on under every screen.
// And Instant Action's map preview, the highlighted map's flyover in a
// frame of the page's own.
//
// The movies are the original's, from the player's own prepared data
// (common/shell/movies), upscaled and sharpened as the other backgrounds
// are (video-sharpen.js).

// Each side's movies, played in turn.
export const SIDES = {
  left: ["republic_win", "republic_base", "cis_win"],
  right: ["imperial_win", "rebel_base", "rebel_win"],
};
const CROSS = 1.2; // s: one movie fades into the next

export class MovieBackdrop {
  constructor({ sides = SIDES } = {}) {
    this.el = document.createElement("div");
    this.el.className = "bfe-backdrop";
    this.sides = ["left", "right"].map((name) => {
      const side = document.createElement("div");
      side.className = `bfe-backdrop-side ${name}`;
      const movie = new SideMovie(sides[name] || []);
      side.append(movie.el);
      this.el.append(side);
      return movie;
    });
    // Each side's sharpened movie is drawn at the size it shows.
    this.observer = new ResizeObserver((entries) => {
      for (const entry of entries) this.sides.find((side) => side.el.parentNode === entry.target)?.resize(entry.contentRect.width, entry.contentRect.height);
    });
    for (const side of this.sides) this.observer.observe(side.el.parentNode);
  }

  play() {
    for (const side of this.sides) side.play();
  }

  pause() {
    for (const side of this.sides) side.pause();
  }

  dispose() {
    this.observer.disconnect();
    for (const side of this.sides) side.dispose();
    this.el.remove();
  }
}

// Instant Action's preview: show(map) puts the map's flyover (shell.json's
// fly) in the frame, PREVIEW_DELAY after the highlight stops moving, its
// name under it.
export class MapPreview {
  constructor() {
    this.el = document.createElement("figure");
    this.el.className = "bfe-preview";
    const frame = document.createElement("div");
    frame.className = "bfe-preview-frame";
    this.slot = new MovieSlot(frame);
    this.slot.video.loop = true;
    this.slot.video.addEventListener("loadeddata", () => this.slot.start());
    this.caption = document.createElement("figcaption");
    this.el.append(frame, this.caption);
    this.timer = null;
    this.map = null;
    this.observer = new ResizeObserver(([entry]) => this.slot.resize(entry.contentRect.width, entry.contentRect.height));
    this.observer.observe(frame);
  }

  // null: none (the screen left), the movie stopped.
  show(map) {
    if (map === this.map) return;
    this.map = map;
    this.caption.textContent = map?.name || "";
    clearTimeout(this.timer);
    this.slot.el.classList.remove("on");
    if (!map?.fly?.url) {
      this.slot.video.pause();
      this.slot.video.removeAttribute("src");
      this.slot.video.load();
      return;
    }
    this.timer = setTimeout(() => { this.slot.video.src = map.fly.url; }, PREVIEW_DELAY * 1000);
  }

  dispose() {
    clearTimeout(this.timer);
    this.observer.disconnect();
    this.slot.dispose();
    this.el.remove();
  }
}

// One side's movies, in turn: the next one waits, loaded, under the one
// playing and fades in over its last CROSS seconds. A missing or broken one
// is passed over (all of them: the side stays dark); one not loaded in time
// leaves the one playing to go round again.
class SideMovie {
  constructor(names) {
    this.names = names;
    this.playing = false;
    this.crossing = null;
    this.el = document.createElement("div");
    this.el.className = "bfe-backdrop-movies";
    this.slots = [new MovieSlot(this.el), new MovieSlot(this.el)];
    this.front = this.slots[0];
    for (const slot of this.slots) {
      const video = slot.video;
      video.addEventListener("loadeddata", () => {
        slot.failures = 0;
        if (slot === this.front && this.playing) slot.start();
      });
      video.addEventListener("timeupdate", () => {
        if (slot === this.front && video.duration - video.currentTime <= CROSS) this.cross();
      });
      video.addEventListener("ended", () => {
        if (slot !== this.front || this.crossing) return;
        video.currentTime = 0;
        if (this.playing) video.play()?.catch?.(() => {});
      });
      video.addEventListener("error", () => {
        if (++slot.failures < this.names.length) slot.load(this.names, slot.at + 1);
      });
    }
  }

  get back() {
    return this.slots[this.slots[0] === this.front ? 1 : 0];
  }

  play() {
    if (!this.names.length) return;
    this.playing = true;
    if (this.front.at === null) {
      this.front.video.loop = this.names.length === 1;
      this.front.load(this.names, 0);
      if (this.names.length > 1) this.back.load(this.names, 1);
    } else if (this.front.video.readyState >= 2) this.front.start();
  }

  pause() {
    this.playing = false;
    for (const slot of this.slots) slot.video.pause();
  }

  // The next movie over this one; then the one after loads under it.
  cross() {
    const from = this.front, to = this.back;
    if (this.names.length < 2 || this.crossing || !this.playing || to.video.readyState < 2) return;
    this.front = to;
    to.el.style.zIndex = "2";
    from.el.style.zIndex = "1";
    to.video.currentTime = 0;
    to.start();
    this.crossing = setTimeout(() => {
      this.crossing = null;
      from.stop();
      from.load(this.names, to.at + 1);
    }, CROSS * 1000);
  }

  resize(width, height) {
    for (const slot of this.slots) slot.resize(width, height);
  }

  dispose() {
    clearTimeout(this.crossing);
    this.playing = false;
    for (const slot of this.slots) slot.dispose();
  }
}

// A muted <video>, drawn sharpened where WebGL2 runs.
class MovieSlot {
  constructor(parent) {
    this.at = null; // which of the side's movies it has
    this.failures = 0;
    const video = (this.video = document.createElement("video"));
    video.muted = true;
    video.defaultMuted = true;
    video.playsInline = true;
    video.preload = "auto";
    video.disablePictureInPicture = true;
    this.sharp = SHARPEN > 0 ? SharpVideo.create(video, { fit: "cover", strength: SHARPEN }) : null;
    if (this.sharp) this.sharp.onLost = () => this.unsharpen();
    this.el = document.createElement("div");
    this.el.className = "bfe-movie";
    this.el.style.setProperty("--cross", `${CROSS}s`);
    this.el.append(this.sharp ? this.sharp.canvas : video);
    parent.append(this.el);
  }

  load(names, at) {
    this.at = at % names.length;
    this.video.src = resolveUrl(`movies/${names[this.at]}.webm`);
  }

  // Play it, fading in.
  start() {
    this.video.play()?.catch?.(() => {});
    this.sharp?.start();
    this.el.classList.add("on");
  }

  stop() {
    this.el.classList.remove("on");
    this.el.style.zIndex = "";
    this.video.pause();
  }

  resize(width, height) {
    this.sharp?.resize(width, height);
  }

  // The GPU went away: the plain video where the canvas was.
  unsharpen() {
    if (!this.sharp) return;
    this.sharp.canvas.replaceWith(this.video);
    this.sharp.dispose();
    this.sharp = null;
  }

  dispose() {
    this.sharp?.dispose();
    this.video.removeAttribute("src");
    this.video.load();
  }
}
