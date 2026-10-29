import { coreSounds } from "./audio.js";

// The loading screen's sound, as Load/load.lvl names it, core.lvl defines it
// and LoadDisplay plays it (docs/shell.md). The config names five sounds;
// three can play:
//
// - load_ytracking and load_xtracking: the pulse. Both loop one 0.35 s ping
//   sample (a tone that dies away in 0.2 s), at pitch 1.8 and 2.1 and gain
//   0.7, so looped they pulse five or six times a second. One plays while
//   the horizontal lines move, the other while the vertical pair does; each
//   stops as its lines come to rest.
// - load_zoom: one 0.43 s sample at pitch 1.5, once, as a zoom starts.
// - load_transition plays as each zoom ends, but names a sample the install
//   doesn't have; load_bar is the pulse's sample at gain 0, played only
//   without pictures. Neither is heard.
//
// All are on core.lvl's shellfx bus (0.3 of the interface volume) under
// the master volume, as the front end's menu sounds. The samples come from
// the front end's sound sub-level (public/audio/shell, or a battle's), each
// fetched alone by byte range, as ShellAudio fetches a menu sound.
//
// The sounds come as the sequence's plan (LoadingTimeline.sounds()) and the
// moment it starts, and are all put on the AudioContext's clock at once, so
// each is heard when its frame shows however busy the page is afterwards.
// The battle page has its own AudioContext here, made as the screen opens:
// browsers that remember a click on the site (the gate's, LAUNCH's) let it
// run at once; otherwise it starts at the page's first click or key, and
// what is left of the plan plays from there. It is closed with the screen;
// the battle's own audio engine comes after.

export const SOUNDS = { ytrack: "load_ytracking", xtrack: "load_xtracking", zoom: "load_zoom", transition: "load_transition" };
// core.lvl's shellfx bus gain.
export const BUS = 0.3;
// A sound is heard this long after its moment: the first frame drawn at or
// after the moment reaches the screen one or two refreshes later.
export const AV = 1 / 30; // s
// A one-shot further behind than this when it could be played is dropped.
export const LATE = 0.05; // s
const STOP = 0.01; // s: a pulse's end
const FADE = 0.08; // s: everything's end as the screen goes

// The page's clock (loading.js's), in ms.
const clock = () => performance.timeOrigin + performance.now();

export class LoadingSound {
  // volumes(): { master, interface } as the player has them set;
  // context: an AudioContext to use (tests), else one is made.
  constructor({ volumes = () => ({ master: 1, interface: 1 }), context = null, sounds = coreSounds } = {}) {
    this.volumes = volumes;
    this.ctx = context;
    this.core = sounds;
    this.nodes = null;
    this.sounds = new Map(); // name -> { buffer, gain, pitch, loop } once loaded
    this.settled = false; // every sound has loaded or failed
    this.plan = null; // { start, events: [{ cue, at, until, handled }] }
    this.voices = new Set();
    this.closed = false;
    this.unlisten = () => {};
    // How it went, for checks: pulses and one-shots put on the clock, those
    // too late to, and when each is heard ({ cue, at, heard } in clock() ms,
    // as the audio clock reads when it was scheduled).
    this.stats = { loops: 0, once: 0, missed: 0 };
    this.log = [];
  }

  get running() {
    return Boolean(this.ctx) && this.ctx.state === "running";
  }

  // Make the context and fetch the sounds. Nothing here throws: a page
  // without sound data, or without Web Audio, just has a silent screen.
  start() {
    if (this.closed) return this;
    try {
      const Context = globalThis.AudioContext || globalThis.webkitAudioContext;
      if (!this.ctx && Context) this.ctx = new Context({ latencyHint: "interactive" });
    } catch {
      this.ctx = null;
    }
    if (!this.ctx) return this;
    this.build();
    if (this.ctx.state !== "running") this.wake();
    Promise.resolve(this.core())
      .then((core) => core && Promise.all(Object.values(SOUNDS).map(async (name) => {
        const sound = { buffer: null, gain: 1, pitch: 1, loop: false, interval: 0 };
        sound.buffer = await Promise.resolve(core.load(name, this.ctx, sound)).catch(() => null);
        if (sound.buffer && !this.closed) this.sounds.set(name, sound);
      })))
      .catch(() => {})
      .then(() => {
        this.settled = true;
        this.apply();
      });
    return this;
  }

  build() {
    const ctx = this.ctx;
    const master = ctx.createGain();
    const bus = ctx.createGain();
    bus.connect(master).connect(ctx.destination);
    this.nodes = { master, bus };
    this.applyVolumes();
  }

  applyVolumes() {
    if (!this.nodes) return;
    const { master = 1, interface: ui = 1 } = this.volumes() || {};
    this.nodes.master.gain.value = Number.isFinite(Number(master)) ? Number(master) : 1;
    this.nodes.bus.gain.value = BUS * (Number.isFinite(Number(ui)) ? Number(ui) : 1);
  }

  // A context the browser keeps suspended runs at the page's first click or
  // key; what is left of the plan plays from then.
  wake() {
    const resume = () => {
      if (this.closed || !this.ctx) return;
      this.ctx.resume().then(() => this.apply(), () => {});
    };
    const types = ["pointerdown", "keydown", "touchstart"];
    for (const type of types) globalThis.addEventListener?.(type, resume, { capture: true });
    this.unlisten = () => { for (const type of types) globalThis.removeEventListener?.(type, resume, { capture: true }); };
    this.ctx.resume?.().then(() => this.apply(), () => {});
  }

  // The sequence's sounds ([{ cue, at, until }], s from its start) and its
  // start (clock() ms).
  schedule(sounds, start) {
    if (this.closed || this.plan) return;
    this.plan = { start, events: sounds.map((sound) => ({ ...sound, handled: null })) };
    this.apply();
  }

  // The clock() ms that an audio clock time is heard at, and back: from the
  // context's output timestamp where it has one, else from its current time
  // and the latencies it reports.
  clockMap() {
    const ctx = this.ctx;
    const stamp = ctx.getOutputTimestamp?.();
    const latency = (Number(ctx.baseLatency) || 0) + (Number(ctx.outputLatency) || 0);
    const base = stamp && stamp.contextTime > 0 && stamp.performanceTime > 0
      ? performance.timeOrigin + stamp.performanceTime - stamp.contextTime * 1000
      : clock() + latency * 1000 - ctx.currentTime * 1000;
    return { when: (ms) => (ms - base) / 1000, heard: (time) => base + time * 1000 };
  }

  // Put every sound of the plan not yet handled on the audio clock (once
  // the context runs and its sound has loaded): a pulse still to end from
  // where its loop would be by now, a one-shot unless it is past.
  apply() {
    if (this.closed || !this.running || !this.plan) return;
    const map = this.clockMap();
    const now = clock();
    for (const event of this.plan.events) {
      if (event.handled) continue;
      const sound = this.sounds.get(SOUNDS[event.cue]);
      if (!sound) {
        if (this.settled) event.handled = "none";
        continue;
      }
      const at = this.plan.start + (event.at + AV) * 1000;
      const until = event.until === null || event.until === undefined ? null : this.plan.start + (event.until + AV) * 1000;
      event.handled = this.play(sound, event.cue, at, until, now, map) ? "played" : "missed";
      if (event.handled === "missed") this.stats.missed += 1;
    }
  }

  play(sound, cue, at, until, now, map) {
    const loop = until !== null;
    if (loop ? until <= now : now - at > LATE * 1000) return false;
    try {
      const ctx = this.ctx;
      const begin = Math.max(at, now);
      const when = Math.max(map.when(begin), 0);
      const source = ctx.createBufferSource();
      source.buffer = sound.buffer;
      source.loop = loop;
      source.playbackRate.value = sound.pitch;
      const level = ctx.createGain();
      level.gain.value = sound.gain;
      source.connect(level).connect(this.nodes.bus);
      // Joining a pulse late: where its loop would be by now.
      const duration = sound.buffer.duration || 1;
      source.start(when, loop ? (((begin - at) / 1000) * sound.pitch) % duration : 0);
      if (loop) {
        const end = map.when(until);
        level.gain.setValueAtTime(sound.gain, Math.max(end - STOP, when));
        level.gain.linearRampToValueAtTime(0, end);
        source.stop(end);
      }
      const voice = { source, level };
      this.voices.add(voice);
      source.onended = () => this.voices.delete(voice);
      this.log.push({ cue, at: Math.round(at * 10) / 10, heard: Math.round(map.heard(when) * 10) / 10 });
      this.stats[loop ? "loops" : "once"] += 1;
      return true;
    } catch {
      return false;
    }
  }

  // What the checks read.
  state() {
    const events = this.plan?.events || [];
    return { context: this.ctx ? this.ctx.state : null, loaded: [...this.sounds.keys()], planned: events.length,
      pending: events.filter((e) => !e.handled).length, ...this.stats, log: this.log };
  }

  // The screen is gone: whatever still sounds ends, the context closes.
  dispose() {
    if (this.closed) return;
    this.closed = true;
    this.unlisten();
    const ctx = this.ctx;
    if (!ctx) return;
    for (const { source, level } of this.voices) {
      try {
        const now = ctx.currentTime;
        level.gain.cancelScheduledValues?.(now);
        level.gain.setValueAtTime(level.gain.value, now);
        level.gain.linearRampToValueAtTime(0, now + FADE);
        source.stop(now + FADE);
      } catch {
        // Already stopped.
      }
    }
    this.voices.clear();
    if (ctx.state !== "closed") setTimeout(() => ctx.close().catch(() => {}), FADE * 1000 + 50);
  }
}
