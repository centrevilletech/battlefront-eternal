import {sceneProperty} from "../rust-render/scene-changes.js";
import { ShellMusicRow as M, ShellMusicOp as OP } from "../audio/shell-music-layout.js";
import { listenSounds } from "../audio/events.js";
import { SoundData } from "../audio/data.js";
import { FrontendAudio } from "../audio/frontend.js";
import { loadAudioPcm, openAudio } from "../audio/codec.js";
import { prepareAudioPcm, normalizePcm16 } from "../audio/pcm.js";

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
  constructor({ tracks = [], interval = INTERVAL, start = () => null, stop = () => {}, loaded } = {}) {
    this.tracks = tracks; this.interval = interval;
    this.startTrack = start; this.stopTrack = stop;
    this.handle = null; this.current = null;
    // Resolve authored string membership only while loading. Runtime frames
    // use stable group and asset indices, with no filtering or track lists.
    sceneProperty(this,"groups",new Map());
    const members = [];
    const add = (name, index) => {
      let id = this.groups.get(name);
      if (id === undefined) { id = members.length; this.groups.set(name, id); members.push([]); }
      members[id].push(index);
    };
    for (let i = 0; i < tracks.length; i++) {
      const t = tracks[i];
      if (Array.isArray(t.properties)) {
        for (const name of new Set(t.properties)) add(name, i);
      } else {
        if (t.soundtrack !== false) add(SOUNDTRACK, i);
        if (t.name !== SOUNDTRACK) add(t.name, i);
      }
    }
    this.missing = members.length;
    this.kernel = new loaded.module.PageShellMusic(JSON.stringify(members));
    this.memory = loaded.memory; this.address = this.kernel.row_ptr(); this.refresh();
  }
  refresh() {
    if (this.buffer === this.memory.buffer) return;
    this.buffer = this.memory.buffer;
    this.row = new Float64Array(this.buffer, this.address, M.STRIDE);
  }
  get phase() { this.refresh(); return PHASES[this.row[M.PHASE]]; }
  get cycle() { this.refresh(); return this.row[M.CYCLE]; }
  get silent() { this.refresh(); return this.row[M.SILENT]; }
  // Every command returns complete policy decisions. Applying playback
  // handles is browser I/O, including qualification of delayed endings.
  command(op, dt = 0, group = 0, name = this.current) {
    this.refresh();
    this.row[M.ENABLED] = name ? 1 : 0;
    this.row[M.OP] = op; this.row[M.DT] = dt;
    this.row[M.INTERVAL] = this.interval; this.row[M.GROUP] = group;
    this.kernel.frame();
    const stop = this.row[M.STOP], track = this.row[M.START];
    if (stop) {
      const old = this.handle; this.current = null; this.handle = null;
      if (old) this.stopTrack(old);
    }
    this.current = name;
    if (track) this.handle = this.startTrack(this.tracks[track - 1]);
  }
  set(name) {
    if (name === null || name === undefined || name === this.current) return;
    if (name === STOP || name === false) { this.halt(); return; }
    this.command(OP.SET, 0, this.groups.get(name) ?? this.missing, name);
  }
  playing(name) { return Boolean(name) && name === this.current; }
  halt() { this.current = null; this.command(OP.HALT); }
  next() { this.command(OP.NEXT); }
  ended(handle = this.handle) {
    if (handle !== this.handle) return;
    this.command(OP.END);
    if (this.phase === "gap") this.handle = null;
  }
  update(dt) { this.command(OP.TICK, dt); }
}
const PHASES = ["stopped", "playing", "gap"];

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
  constructor({ data, settings = null, loaded } = {}) {
    this.data = data;
    this.settings = settings;
    this.ctx = null;
    this.output = null;
    this.whooshes = (data?.whooshes || []).map((url) => ({ url, bytes: null, buffer: null }));
    this.whooshIndex = 0;
    this.sounds = new Map(); // name -> { buffer|Promise, gain, pitch, interval, last }
    this.dip = 1; // the shellmusic bus's own level while there's no context yet
    this.music = new ShellMusic({
      tracks: data?.music || [], loaded,
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
    return this.ctx?.state === "running" && this.output?.isReady;
  }

  // Inside a click or key press: make or wake the AudioContext. Returns
  // whether it runs.
  unlock() {
    if (!this.ctx) {
      const Context = window.AudioContext || window.webkitAudioContext;
      if (!Context) return false;
      this.ctx = new Context({ latencyHint: "interactive" });
      this.build();
    }
    if (this.ctx.state === "suspended") this.ctx.resume().then(() => this.retry(), () => {});
    this.retry();
    return this.running;
  }

  build() {
    const ctx = this.ctx;
    this.output = new FrontendAudio(ctx);
    this.output.ready.then(() => {
      this.applyVolumes();
      this.output.dip(this.dip);
      for (const whoosh of this.whooshes) this.decode(whoosh);
      for (const name of INTERFACE_SOUNDS) this.sound(name);
      this.retry();
    }).catch((error) => this.output.fail(error));
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
    if (!this.output?.isReady) return;
    this.output.volumes(this.volume("masterVolume"), BUSES.shellmusic * this.volume("musicVolume"), BUSES.shellfx * this.volume("interfaceVolume"));
  }

  // A track asked for before the context was there starts once it is.
  retry() {
    const handle = this.music.handle;
    if (!this.running || !handle || handle.stopped || handle.loading || handle.source) return;
    handle.loading = true;
    openAudio(handle.url, { rate: this.ctx.sampleRate }).then(async (asset) => {
      if (handle.stopped) { asset.close(); return; }
      handle.asset = asset;
      handle.source = await this.output.playStream(asset, { bus: 1 });
      if (handle.stopped) { handle.source.stop(); return; }
      handle.source.onended = () => { if (!handle.stopped) this.music.ended(handle); };
    }).catch((error) => { if (!handle.stopped) this.output.fail(error); });
  }

  startTrack(track) {
    const handle = { track, url: track.url, stopped: false, source: null, loading: false };
    // ShellMusic installs its handle after startTrack returns.
    queueMicrotask(() => this.retry());
    return handle;
  }

  stopTrack(handle) {
    handle.stopped = true;
    if (handle.source) handle.source.stop();
    else handle.asset?.close();
    handle.buffer = null;
  }

  // ScriptCB_SndBusFade for the front end's buses: shellmusic's level ramps
  // to `level` over `seconds`.
  fadeBus(bus, level, seconds = 0) {
    if (bus !== "shellmusic") return;
    this.dip = level;
    if (this.output?.isReady) this.output.dip(level, seconds);
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
      .then((bytes) => (bytes ? loadAudioPcm(bytes.slice(0), { rate: this.ctx.sampleRate }) : null))
      .catch((error) => { this.output.fail(error); return null; });
    return entry.buffer;
  }

  playBuffer(buffer, { gain = 1, pitch = 1 } = {}) {
    if (!this.running || !buffer) return;
    return this.output.play(buffer, { gain, pitch });
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
        await prepareAudioPcm();
        normalizePcm16(pcm, out);
        // Under 8 kHz the buffer runs faster; the pitch makes it up.
        into.pitch *= rate / Math.max(rate, 8000);
        return buffer;
      },
    };
  })();
  return core;
}
