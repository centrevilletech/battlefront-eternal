// The front end's backgrounds: the original's Bink movies (Movies/shell.mvs,
// transcoded to WebM, docs/shell-assets.md), behind the interface, as the
// movie config (shell.lvl mcfg) plays them:
//
// - A screen's movie is its intro (the camera's move between the hologram
//   rooms), played once, then its loop, for ever. The intro fades out over
//   its last FadeOut seconds; the loop starts as it ends, fading in over its
//   FadeIn. The main menu's intro has no fades: it cuts into its loop.
// - A new movie starts at once, under the one playing, which fades out over
//   its FadeOut on top of it: a short cross-fade (0.5 s from a loop).
//   Going back plays no intro, only the loop.
// - Movies fill the screen, cropped to keep their shape (the original
//   stretches them; "fill" does that), or a rectangle of it: Instant
//   Action's flyover plays in its corner.
//
// Each movie is a muted <video> in a layer of its own; a missing or broken
// file leaves the screen black and lets the screen go on.

import { SharpVideo } from "./video-sharpen.js";

const READY_WAIT = 2.5; // s: how long a new movie may take to show its first frame
const SHOW_WAIT = 0.3; // s: how long a started movie may take to put it on screen
// The movies are drawn upscaled and sharpened (video-sharpen.js); strength
// 0-1, ?moviesharpen= overrides it and 0 shows the plain video.
export const SHARPEN = (() => {
  const value = typeof location === "undefined" ? null : new URLSearchParams(location.search).get("moviesharpen");
  return value === null || !Number.isFinite(Number(value)) ? 0.7 : Math.min(Math.max(Number(value), 0), 1);
})();
// Under the movies: the deep blue of the hologram rooms, which a fade
// passes through (and all there is when no movie was extracted).
const BACKDROP = "radial-gradient(ellipse at 40% 45%, #0c2858 0%, #061531 48%, #01040c 100%)";

export class MoviePlayer {
  constructor(parent = document.body) {
    this.root = document.createElement("div");
    this.root.className = "shell-movies";
    this.root.style.cssText = `position:fixed;inset:0;overflow:hidden;background:${BACKDROP};z-index:0;pointer-events:none`;
    parent.prepend(this.root);
    this.layers = [];
    this.token = 0;
    this.pending = null; // a delayed start: { timer, cancel }
    this.watching = false;
    this.onResize = () => this.layers.forEach((layer) => layer.place());
    window.addEventListener("resize", this.onResize);
  }

  // Play a screen's movie, { intro, loop } (clips { url, fadeIn, fadeOut },
  // either may be null). options:
  // - intro: play the intro first (false going back);
  // - rect: (width, height) => [x, y, w, h] in CSS pixels, or null for the
  //   whole screen;
  // - fit: "contain" (whole image), "cover" (crop) or "fill" (stretch);
  // - delay: seconds before it starts (Instant Action's preview waits 0.5 s).
  // Returns { started, ended }: when the intro (or else the loop) has
  // started, its first frame on screen (the whoosh goes with it), and when
  // the intro has ended (the original pushes the screen
  // then); true if it played, false if there was nothing to play, it failed,
  // or another movie took its place.
  play(movie, { intro = true, rect = null, fit = "contain", delay = 0 } = {}) {
    const token = ++this.token;
    this.cancelPending();
    let started;
    let ended;
    const result = {
      started: new Promise((resolve) => { started = resolve; }),
      ended: new Promise((resolve) => { ended = resolve; }),
    };
    const settle = (ok) => {
      started(ok);
      ended(ok);
    };
    const begin = () => {
      this.pending = null;
      const first = (intro && movie?.intro) || movie?.loop || null;
      if (!first) {
        this.fadeAll();
        return settle(false);
      }
      const isIntro = first === movie.intro;
      const layer = this.add(first, { loop: !isIntro, rect, fit });
      // The loop waits, loaded, under its intro.
      const next = isIntro && movie.loop ? this.add(movie.loop, { loop: true, rect, fit, below: layer, wait: true }) : null;
      layer.ready.then((ok) => {
        if (token !== this.token) {
          // Another movie came first.
          layer.fadeOut(0);
          next?.fadeOut(0);
          return settle(false);
        }
        this.fadeAll([layer, next]);
        layer.start();
        if (ok) layer.shown().then(() => started(true));
        else started(false);
        if (!isIntro) return ended(ok);
        // The intro fades out over its last FadeOut seconds; the loop starts
        // when it ends.
        const finish = () => {
          if (layer.finished) return;
          layer.finished = true;
          if (token === this.token) next?.start();
          else next?.fadeOut(0);
          layer.fadeOut(0);
          ended(ok);
        };
        if (!ok) return finish();
        layer.onEnd = finish;
        layer.onNear = () => layer.fadeOut(first.fadeOut, false);
        this.watch();
      });
    };
    if (delay > 0) this.pending = { timer: setTimeout(begin, delay * 1000), cancel: () => settle(false) };
    else begin();
    return result;
  }

  cancelPending() {
    if (!this.pending) return;
    clearTimeout(this.pending.timer);
    this.pending.cancel();
    this.pending = null;
  }

  // Fade out everything showing (the screen has no movie).
  stop() {
    this.token += 1;
    this.cancelPending();
    this.fadeAll();
  }

  // Every layer but those kept fades out over its own FadeOut.
  fadeAll(keep = []) {
    for (const layer of this.layers) if (!keep.includes(layer)) layer.fadeOut(layer.clip.fadeOut);
  }

  add(clip, options) {
    const layer = new MovieLayer(this, clip, options);
    this.layers.push(layer);
    return layer;
  }

  remove(layer) {
    this.layers = this.layers.filter((l) => l !== layer);
  }

  // Intros are watched every frame for the start of their fade-out.
  watch() {
    if (this.watching) return;
    this.watching = true;
    const step = () => {
      let any = false;
      for (const layer of this.layers) any = layer.check() || any;
      if (any) requestAnimationFrame(step);
      else this.watching = false;
    };
    requestAnimationFrame(step);
  }

  // What's playing, bottom first, for checks: [{ url, opacity, time, loop,
  // playing, failed }].
  state() {
    return this.layers.map((layer) => ({
      url: layer.clip.url,
      opacity: Number(getComputedStyle(layer.el).opacity),
      time: layer.video.currentTime,
      loop: layer.video.loop,
      playing: !layer.video.paused,
      failed: layer.failed,
    }));
  }

  dispose() {
    this.stop();
    window.removeEventListener("resize", this.onResize);
    for (const layer of [...this.layers]) layer.dispose();
    this.root.remove();
  }
}

class MovieLayer {
  constructor(player, clip, { loop, rect, fit, below = null, wait = false }) {
    this.player = player;
    this.clip = clip;
    this.rect = rect;
    this.finished = false;
    this.failed = false;
    this.leaving = false;
    this.onEnd = null;
    this.onNear = null;
    const video = (this.video = document.createElement("video"));
    video.muted = true;
    video.defaultMuted = true;
    video.playsInline = true;
    video.preload = "auto";
    video.loop = loop;
    video.disablePictureInPicture = true;
    this.fit = ["fill", "cover"].includes(fit) ? fit : "contain";
    // What shows: the sharpened canvas, or the video itself without WebGL2.
    this.sharp = SHARPEN > 0 ? SharpVideo.create(video, { fit: this.fit, strength: SHARPEN }) : null;
    this.el = this.sharp ? this.sharp.canvas : video;
    if (this.sharp) this.sharp.onLost = () => this.unsharpen();
    this.el.style.cssText = `position:absolute;object-fit:${this.fit};opacity:0;transition:none`;
    this.place();
    // A new layer goes under those already there, which fade out over it.
    const root = player.root;
    root.insertBefore(this.el, below ? below.el : root.firstChild);
    this.ready = new Promise((resolve) => {
      const done = (ok) => {
        clearTimeout(timer);
        if (!ok) this.failed = true;
        resolve(ok);
      };
      const timer = setTimeout(() => done(video.readyState >= 2), READY_WAIT * 1000);
      video.addEventListener("loadeddata", () => done(true), { once: true });
      video.addEventListener("error", () => done(false), { once: true });
    });
    video.addEventListener("ended", () => this.onEnd?.());
    video.src = clip.url;
    if (wait) video.load();
  }

  place() {
    const [x, y, w, h] = this.rect ? this.rect(window.innerWidth, window.innerHeight) : [0, 0, window.innerWidth, window.innerHeight];
    Object.assign(this.el.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px` });
    this.sharp?.resize(w, h);
  }

  // The GPU went away: show the plain video where the canvas was.
  unsharpen() {
    if (!this.sharp) return;
    const { canvas } = this.sharp;
    this.video.style.cssText = canvas.style.cssText;
    canvas.replaceWith(this.video);
    this.sharp.dispose();
    this.sharp = null;
    this.el = this.video;
  }

  // Show it, fading in over its FadeIn, and play it.
  start() {
    if (this.leaving) return;
    const el = this.el;
    this.video.play()?.catch?.(() => {});
    this.sharp?.start();
    const fade = this.clip.fadeIn;
    el.style.transition = fade > 0 ? `opacity ${fade}s linear` : "none";
    // Read back once so the transition starts from 0.
    void el.offsetWidth;
    el.style.opacity = "1";
  }

  // Resolves as its first frame comes to the screen once started: play()
  // can take a moment to get there.
  shown() {
    const video = this.video;
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, SHOW_WAIT * 1000);
      const done = () => {
        clearTimeout(timer);
        resolve();
      };
      if (typeof video.requestVideoFrameCallback === "function") video.requestVideoFrameCallback(done);
      else video.addEventListener("playing", done, { once: true });
    });
  }

  // Fade out over some seconds, then go (remove: false only fades).
  fadeOut(seconds, remove = true) {
    if (this.leaving) return;
    if (remove) this.leaving = true;
    const el = this.el;
    el.style.transition = seconds > 0 ? `opacity ${seconds}s linear` : "none";
    el.style.opacity = "0";
    if (remove) setTimeout(() => this.dispose(), seconds * 1000 + 50);
  }

  // Every frame while an intro plays: its fade-out begins FadeOut seconds
  // before its end. Returns whether it still needs watching.
  check() {
    if (!this.onNear || this.leaving) return false;
    const video = this.video;
    if (Number.isFinite(video.duration) && video.duration > 0 && video.duration - video.currentTime <= this.clip.fadeOut) {
      const near = this.onNear;
      this.onNear = null;
      near();
      return false;
    }
    return true;
  }

  dispose() {
    const video = this.video;
    this.sharp?.dispose();
    video.pause();
    video.removeAttribute("src");
    video.load();
    video.remove();
    this.player.remove(this);
  }
}
