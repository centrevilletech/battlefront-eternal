import { listenSounds } from "../audio/events.js";
import { SoundData } from "../audio/data.js";

// The front end's sound, as the original's shell plays it (Sound/shell.lvl,
// core.lvl's buses; docs/shell.md):
//
// - The music: "shell_soundtrack" plays its tracks in turn, one at a time,
//   with 10 s of silence between them (SetShellMusicInterval). A screen
//   names the music it wants; one that names none keeps what plays, and
//   "STOP" stops it.
// - The buses: the music on shellmusic (0.6 of the music volume), the
//   whooshes and menu sounds on shellfx (0.3 of the interface volume).
//   A forward move into a screen whose music isn't already playing dips
//   shellmusic to 0 over a second and brings it back over a second when the
//   move is done.
// - A whoosh, shell_transition1 to 10 in turn, as each move's camera flight
//   starts.
// - The menus' own sounds (shell_select_change, shell_menu_accept...),
//   asked for through src/audio/events.js as in a battle.
//
// The battle's engine (src/audio) plays a battle's sound sub-level; the
// front end's music and whooshes are plain files (shell.json), so this is a
// small player of its own on the same AudioContext rules: nothing sounds
// until the page has been clicked or a key pressed (the click gate).

export const SOUNDTRACK = "shell_soundtrack";
export const STOP = "STOP";
export const INTERVAL = 10; // s between tracks
const BUSES = { shellmusic: 0.6, shellfx: 0.3 };

// The music's timeline, pure: which track plays, and the silences between.
// start(track) begins a track and returns something stop(handle) ends;
// call ended() when it finishes. As the original's player, the silence is
// measured against the interval as it is now, so shortening it (the AUDIO
// OPTIONS page sets 0) cuts a silence short.
export class ShellMusic {
  constructor({ tracks = [], interval = INTERVAL, start = () => null, stop = () => {} } = {}) {
    this.tracks = tracks;
    this.interval = interval;
    this.startTrack = start;
    this.stopTrack = stop;
    this.current = null; // the music asked for
    this.phase = "stopped"; // playing, gap, stopped
    this.handle = null;
    this.cycle = -1; // the soundtrack's last track, in turn
    this.silent = 0; // s since the last track ended
  }

  // The tracks a music name plays, in order: those whose properties list it
  // (shell.json); without properties, the soundtrack is every track not
  // marked out of it, and another name is the track of that name.
  list(name) {
    return this.tracks.filter((track) => (Array.isArray(track.properties) ? track.properties.includes(name)
      : name === SOUNDTRACK ? track.soundtrack !== false : track.name === name));
  }

  // A screen's music: a name starts it unless it's already the music; STOP
  // stops it; null or undefined keeps what plays.
  set(name) {
    if (name === null || name === undefined || name === this.current) return;
    this.halt();
    if (name === STOP || name === false) return;
    this.current = name;
    this.next();
  }

  // Whether this music is the one set (playing, or between its tracks).
  playing(name) {
    return Boolean(name) && name === this.current;
  }

  halt() {
    if (this.handle) this.stopTrack(this.handle);
    this.handle = null;
    this.current = null;
    this.phase = "stopped";
  }

  next() {
    const list = this.list(this.current);
    if (!list.length) {
      this.phase = "gap";
      this.silent = 0;
      return;
    }
    this.cycle = (this.cycle + 1) % list.length;
    this.phase = "playing";
    this.handle = this.startTrack(list[this.cycle]);
  }

  // The playing track finished (or failed): the silence, then the next.
  ended(handle = this.handle) {
    if (handle !== this.handle || this.phase !== "playing") return;
    this.handle = null;
    this.phase = "gap";
    this.silent = 0;
  }

  update(dt) {
    if (this.phase !== "gap" || !this.current) return;
    this.silent += dt;
    if (this.silent >= this.interval) this.next();
  }
}

// The interface sounds the menus play (core.lvl, on shellfx), made ready
// as soon as there is sound, so the first of each isn't late.
export const INTERFACE_SOUNDS = ["shell_select_change", "shell_menu_accept", "shell_menu_enter", "shell_menu_cancel", "shell_menu_error",
  "shell_menu_exit", "shell_keyboard_key", "shell_keyboard_backspace"];
// A menu sound still loading when it was asked for plays if it arrives
// within this long, else not at all (a late click is worse than none).
const LATE = 0.15; // s

export class ShellAudio {
  // data: the shell's data (src/shell/data.js); settings: the player's
  // (volumes).
  constructor({ data, settings = null } = {}) {
    this.data = data;
    this.settings = settings;
    this.ctx = null;
    this.nodes = null;
    this.whooshes = (data?.whooshes || []).map((url) => ({ url, bytes: null, buffer: null }));
    this.whooshIndex = 0;
    this.sounds = new Map(); // name -> { buffer|Promise, gain, pitch, interval, last }
    this.dip = 1; // the shellmusic bus's own level while there's no context yet
    this.music = new ShellMusic({
      tracks: data?.music || [],
      start: (track) => this.startTrack(track),
      stop: (handle) => this.stopTrack(handle),
    });
    this.unlisten = listenSounds((type, ...args) => {
      if (type === "interface") this.play(args[0]);
      else if (type === "busFade") this.fadeBus(...args);
    });
    settings?.onChange?.((key) => {
      if (/Volume$/.test(key)) this.applyVolumes();
    });
    // Fetch the whooshes now; they decode once there's a context.
    for (const whoosh of this.whooshes) whoosh.bytes = fetchBytes(whoosh.url);
  }

  get running() {
    return this.ctx?.state === "running";
  }

  // Inside a click or key press: make or wake the AudioContext. Returns
  // whether it runs.
  unlock() {
    if (!this.ctx) {
      const Context = window.AudioContext || window.webkitAudioContext;
      if (!Context) return false;
      this.ctx = new Context({ latencyHint: "interactive" });
      this.build();
      for (const whoosh of this.whooshes) this.decode(whoosh);
      for (const name of INTERFACE_SOUNDS) this.sound(name);
    }
    if (this.ctx.state === "suspended") this.ctx.resume().then(() => this.retry(), () => {});
    this.retry();
    return this.running;
  }

  build() {
    const ctx = this.ctx;
    const master = ctx.createGain();
    const music = ctx.createGain();
    const dip = ctx.createGain();
    const fx = ctx.createGain();
    dip.gain.value = this.dip;
    dip.connect(music).connect(master);
    fx.connect(master);
    master.connect(ctx.destination);
    this.nodes = { master, music, dip, fx };
    this.applyVolumes();
    // Hidden tabs go quiet.
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) ctx.suspend();
      else if (navigator.userActivation?.hasBeenActive ?? true) ctx.resume();
    });
  }

  volume(key) {
    const v = this.settings?.get?.(key);
    return v === undefined || v === null ? 1 : Number(v);
  }

  applyVolumes() {
    if (!this.nodes) return;
    this.nodes.master.gain.value = this.volume("masterVolume");
    this.nodes.music.gain.value = BUSES.shellmusic * this.volume("musicVolume");
    this.nodes.fx.gain.value = BUSES.shellfx * this.volume("interfaceVolume");
  }

  // A track asked for before the context was there starts once it is.
  retry() {
    const handle = this.music.handle;
    if (handle && !handle.stopped && this.route(handle) && handle.element.paused) handle.element.play().catch(() => {});
  }

  // A track's element feeds the shellmusic bus; it never plays straight to
  // the speakers.
  route(handle) {
    if (!handle.source && this.ctx) {
      handle.source = this.ctx.createMediaElementSource(handle.element);
      handle.source.connect(this.nodes.dip);
    }
    return Boolean(handle.source);
  }

  startTrack(track) {
    const element = new Audio();
    element.preload = "auto";
    element.src = track.url;
    const handle = { track, element, stopped: false, source: null };
    const end = () => {
      if (!handle.stopped) this.music.ended(handle);
    };
    element.addEventListener("ended", end);
    element.addEventListener("error", end);
    if (this.route(handle)) element.play().catch(() => {});
    return handle;
  }

  stopTrack(handle) {
    handle.stopped = true;
    handle.element.pause();
    handle.element.removeAttribute("src");
    handle.element.load();
    handle.source?.disconnect();
  }

  // ScriptCB_SndBusFade for the front end's buses: shellmusic's level ramps
  // to `level` over `seconds`.
  fadeBus(bus, level, seconds = 0) {
    if (bus !== "shellmusic") return;
    this.dip = level;
    if (!this.nodes) return;
    const gain = this.nodes.dip.gain;
    const now = this.ctx.currentTime;
    gain.cancelScheduledValues(now);
    gain.setValueAtTime(gain.value, now);
    if (seconds > 0) gain.linearRampToValueAtTime(level, now + seconds);
    else gain.setValueAtTime(level, now);
  }

  // The next whoosh in turn (shell_transition1..10).
  whoosh() {
    if (!this.whooshes.length) return;
    const whoosh = this.whooshes[this.whooshIndex];
    this.whooshIndex = (this.whooshIndex + 1) % this.whooshes.length;
    this.decode(whoosh).then((buffer) => buffer && this.playBuffer(buffer));
  }

  decode(entry) {
    if (entry.buffer) return Promise.resolve(entry.buffer);
    if (!this.ctx) return Promise.resolve(null);
    entry.buffer = Promise.resolve(entry.bytes || fetchBytes(entry.url))
      .then((bytes) => (bytes ? this.ctx.decodeAudioData(bytes.slice(0)) : null))
      .catch(() => null);
    return entry.buffer;
  }

  playBuffer(buffer, { gain = 1, pitch = 1 } = {}) {
    if (!this.running || !buffer) return;
    const source = this.ctx.createBufferSource();
    source.buffer = buffer;
    source.playbackRate.value = pitch;
    if (gain === 1) source.connect(this.nodes.fx);
    else {
      const level = this.ctx.createGain();
      level.gain.value = gain;
      source.connect(level).connect(this.nodes.fx);
    }
    source.start();
  }

  // A menu sound by name, at most once per its PlayInterval.
  play(name) {
    if (!this.running || !name) return;
    const sound = this.sound(name);
    const now = this.ctx.currentTime;
    if (now - sound.last < sound.interval) return;
    sound.last = now;
    if (sound.buffer && !(sound.buffer instanceof Promise)) return this.playBuffer(sound.buffer, sound);
    Promise.resolve(sound.buffer).then((buffer) => {
      if (this.ctx.currentTime - now <= LATE) this.playBuffer(buffer, sound);
    });
  }

  sound(name) {
    let sound = this.sounds.get(name);
    if (sound) return sound;
    const own = this.data?.sounds?.[name];
    sound = { buffer: null, gain: own?.gain ?? 1, pitch: own?.pitch ?? 1, interval: 0, last: -Infinity };
    this.sounds.set(name, sound);
    // From shell.json, or else a battle's sound sub-level: the sound's own
    // pitch and gain, and its sample fetched by itself.
    const loading = own ? this.decode({ url: own.url }) : coreSounds().then((core) => core?.load(name, this.ctx, sound));
    sound.buffer = loading.catch(() => null).then((buffer) => (sound.buffer = buffer));
    return sound;
  }

  update(dt) {
    this.music.update(dt);
  }

  dispose() {
    this.music.halt();
    this.unlisten();
    this.ctx?.close();
  }
}

function fetchBytes(url) {
  return fetch(url)
    .then((r) => (r.ok && !/text\/html/.test(r.headers.get("content-type") || "") ? r.arrayBuffer() : null))
    .catch(() => null);
}

// core.lvl's sounds without loading a whole bank: the front end's own sound
// sub-level (public/audio/shell, tools/zero_shell.py), else the first
// battle's the extractor wrote (public/audio/index.json), has every core.lvl
// sound in its sound.json, and each sample is fetched alone from its
// bank.bin by byte range. load(name, ctx, into) gives the sample as an
// AudioBuffer and sets the sound's gain, pitch, PlayInterval and whether it
// loops on `into`. The loading screen's sounds come this way too
// (loading-audio.js).
let core = null;
export function coreSounds() {
  core ||= (async () => {
    const manifestOf = (sublevel) => fetch(`/audio/${sublevel}/sound.json`)
      .then((r) => (r.ok && /json/.test(r.headers.get("content-type") || "") ? r.json() : null)).catch(() => null);
    let sublevel = "shell";
    let manifest = await manifestOf(sublevel);
    if (!manifest) {
      const index = await fetch("/audio/index.json").then((r) => (r.ok ? r.json() : null)).catch(() => null);
      sublevel = Object.values(index?.maps || index?.missions || {})[0];
      manifest = sublevel ? await manifestOf(sublevel) : null;
    }
    if (!manifest) return null;
    const data = new SoundData(manifest, null);
    return {
      async load(name, ctx, into) {
        const sound = data.sound(name);
        const id = sound?.SampleList?.[0]?.[0];
        const entry = id && data.samples[id];
        if (!entry || !ctx) return null;
        into.gain = sound.Gain ?? 1;
        into.pitch = sound.Pitch ?? 1;
        into.interval = sound.PlayInterval ?? 0;
        into.loop = Boolean(sound.Looping);
        const [offset, frames, rate] = entry;
        const response = await fetch(`/audio/${sublevel}/bank.bin`, { headers: { Range: `bytes=${offset}-${offset + frames * 2 - 1}` } });
        if (!response.ok) return null;
        let bytes = await response.arrayBuffer();
        if (response.status !== 206) bytes = bytes.slice(offset, offset + frames * 2);
        const pcm = new Int16Array(bytes);
        const buffer = ctx.createBuffer(1, Math.max(pcm.length, 1), Math.max(rate, 8000));
        const out = buffer.getChannelData(0);
        for (let i = 0; i < pcm.length; i++) out[i] = pcm[i] / 32768;
        // Under 8 kHz the buffer runs faster; the pitch makes it up.
        into.pitch *= rate / Math.max(rate, 8000);
        return buffer;
      },
    };
  })();
  return core;
}
