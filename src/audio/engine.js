import { Target } from "./automation.js";
import { distanceGain, gate, openAlFalloff, pickSample, prop, reach, rollGain, rollPitch } from "./props.js";
import { Renderer } from "./spatial.js";

// The sound engine: one AudioContext, the game's buses, and its voices,
// following the 2004 game's sound library (docs/formats.md, "Sound"):
//
// - Voices: a pool of 64 virtual voices, of which 32 are heard. Every frame
//   each voice scores (1 - Bias) x its level here + Bias x Priority, and the
//   top 32 play; the rest keep time silently and come back where they would
//   be. A sound asked for with the pool full gets in only by stealing the
//   lowest-scoring voice, and only if (1 - Bias) x its bus gain + Bias x
//   Priority beats that score.
// - Out of earshot: a one-shot whose level where it is asked for is 0 (3D,
//   at or past its MuteDistance) doesn't start at all (the game's entity
//   sounds all ask it to), so a battle far off takes no voices and can't
//   steal the ones near.
// - Level: clamp(bus gain x (Gain +- GainDev), 0, 1) x the distance gain. A
//   bus's gain is its Gain times its parents' (core.lvl: root -> ingame ->
//   main -> soundfx and music; ambience and voiceover under ingame).
// - Ducking: while anything plays on a bus with AttenuateBus (voiceover), the
//   bus it names (main) ramps to 1 - Attenuation over AttenuationLeadIn, and
//   back over AttenuationLeadOut once the last voice is done.
// - Doppler (on unless ModeDoppler 0): the pitch times 1 + (the speed toward
//   the listener) / 344.
// - The listener: the camera, 2 m out in front of it for distance.
//
// The original mode adds what the Steam build's OpenAL output does: an extra
// falloff past half of MaxDistance, pitch changes gliding 1/256 of the way
// twice a frame, and 2D sounds always centred (its Pan never arrives).

const SMOOTH = 0.015; // s, gain changes
const STOP_FADE = 0.03; // s, a stopped voice fades out over this
export const VIRTUAL_VOICES = 64;
export const REAL_VOICES = 32;
const SOUND_SPEED = 344; // m/s, the library's
const DOPPLER_LIMIT = 172; // m/s: faster than this toward the listener, no shift
const PITCH_GLIDE = 1 - (255 / 256) ** 2; // the OpenAL output, per frame
const HZ_RANGE = [100, 200000]; // what the OpenAL output lets a voice play at
export const LISTENER_AHEAD = 2; // m in front of the camera

// What the volume sliders call each kind of sound.
export const CATEGORIES = {
  weapons: "effectsVolume",
  explosions: "effectsVolume",
  foley: "effectsVolume",
  effects: "effectsVolume",
  ambience: "ambienceVolume",
  music: "musicVolume",
  voice: "voiceVolume",
  interface: "interfaceVolume",
};

export class AudioEngine {
  constructor({ settings = null, maxVoices = REAL_VOICES, virtualVoices = VIRTUAL_VOICES, context = null } = {}) {
    this.settings = settings;
    this.maxVoices = maxVoices;
    this.virtualVoices = virtualVoices;
    this.gates = new Map(); // sound key -> its play gate state
    this.ctx = context;
    this.data = null;
    this.voices = [];
    this.listener = {
      position: { x: 0, y: 0, z: 0 },
      ear: { x: 0, y: 0, z: 0 }, // where distance is measured from
      forward: { x: 0, y: 0, z: -1 },
      up: { x: 0, y: 1, z: 0 },
      right: { x: 1, y: 0, z: 0 },
      velocity: { x: 0, y: 0, z: 0 },
    };
    this.buffers = new Map(); // sample id -> AudioBuffer
    this.buses = new Map(); // bus key -> { gain, parent }
    this.busGains = new Map(); // bus key -> its gain with its parents', this frame
    this.ducks = [];
    this.live = []; // scratch: the voices ranked each frame
    this.time = 0;
    this.stats = { real: 0, virtual: 0, started: 0, stolen: 0 };
    // Per-voice extras the enhanced renderer wants (air, occlusion, rooms,
    // LFE): (voice) -> { cutoff, send, lfe, spread, gain }.
    this.shape = null;
    if (context) this.renderer = new Renderer(context);
  }

  // The context starts suspended until the page has been clicked; the first
  // click (the one that captures the mouse) resumes it.
  ensureContext() {
    if (this.ctx) return this.ctx;
    const Context = window.AudioContext || window.webkitAudioContext;
    this.ctx = new Context({ latencyHint: "interactive" });
    this.renderer = new Renderer(this.ctx);
    const resume = () => {
      if (this.ctx.state === "suspended" && !document.hidden) this.ctx.resume();
    };
    for (const type of ["pointerdown", "keydown", "touchstart"]) window.addEventListener(type, resume, { capture: true });
    // Hidden tabs go quiet and stop spending time on audio.
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) this.ctx.suspend();
      else if (navigator.userActivation?.hasBeenActive ?? true) this.ctx.resume();
    });
    return this.ctx;
  }

  setData(data) {
    this.data = data;
    this.buses.clear();
    this.busGains.clear();
    this.ducks = [];
    for (const [key, bus] of data.buses()) {
      this.buses.set(key, { gain: bus.Gain ?? 1, parent: bus.Parent && bus.Parent !== key ? bus.Parent : null, duck: 1 });
      if (bus.AttenuateBus) {
        this.ducks.push({
          bus: key,
          target: bus.AttenuateBus,
          amount: bus.Attenuation ?? 0.5,
          leadIn: bus.AttenuationLeadIn ?? 0.5,
          leadOut: bus.AttenuationLeadOut ?? 0.5,
          level: 1,
        });
      }
    }
  }

  // A bus's gain with its parents', and any ducking on them. Ducks and
  // fades only move in updateDucks(), so each bus's is worked out once a
  // frame.
  busGain(key) {
    const known = this.busGains.get(key);
    if (known !== undefined) return known;
    const first = key;
    let gain = 1;
    let guard = 0;
    while (key && guard++ < 16) {
      const bus = this.buses.get(key);
      if (!bus) break;
      gain *= bus.gain * bus.duck * (bus.fadeLevel ?? 1);
      key = bus.parent;
    }
    this.busGains.set(first, gain);
    return gain;
  }

  categoryVolume(category) {
    const key = CATEGORIES[category] || "effectsVolume";
    const v = this.settings?.get(key);
    return v === undefined || v === null ? 1 : Number(v);
  }

  applyVolumes() {
    const master = this.settings?.get("masterVolume");
    this.renderer?.setVolume(master === undefined || master === null ? 1 : Number(master));
  }

  // A sample's buffer: the upscaled one in the enhanced mode once it has
  // loaded, else the original.
  buffer(id) {
    if (this.renderer?.enhanced) {
      const better = this.enhanced?.sample(id);
      if (better) return better;
    }
    if (this.buffers.has(id)) return this.buffers.get(id);
    const sample = this.data?.sample(id);
    if (!sample || !sample.frames) {
      this.buffers.set(id, null);
      return null;
    }
    // AudioBuffers can't be slower than 3 kHz; a few of the game's samples
    // are (320 Hz), and play stretched by their rate.
    const rate = Math.max(sample.rate, 8000);
    const buffer = this.ctx.createBuffer(1, sample.frames, rate);
    const out = buffer.getChannelData(0);
    for (let i = 0; i < sample.frames; i++) out[i] = sample.pcm[i] / 32768;
    buffer.rateScale = sample.rate / rate;
    this.buffers.set(id, buffer);
    return buffer;
  }

  setListener(position, forward, up, velocity = null) {
    const l = this.listener;
    copy(l.position, position);
    copy(l.forward, forward);
    copy(l.up, up);
    normalize(l.forward);
    // right = forward x up
    l.right.x = l.forward.y * l.up.z - l.forward.z * l.up.y;
    l.right.y = l.forward.z * l.up.x - l.forward.x * l.up.z;
    l.right.z = l.forward.x * l.up.y - l.forward.y * l.up.x;
    normalize(l.right);
    // up again, square to the others
    l.up.x = l.right.y * l.forward.z - l.right.z * l.forward.y;
    l.up.y = l.right.z * l.forward.x - l.right.x * l.forward.z;
    l.up.z = l.right.x * l.forward.y - l.right.y * l.forward.x;
    l.ear.x = l.position.x + l.forward.x * LISTENER_AHEAD;
    l.ear.y = l.position.y + l.forward.y * LISTENER_AHEAD;
    l.ear.z = l.position.z + l.forward.z * LISTENER_AHEAD;
    if (velocity) copy(l.velocity, velocity);
  }

  // Play a sound by name (or key). options:
  // - position: where it is ({x, y, z}); follow: () => position, for a source
  //   that moves (read every frame); none plays it at the listener (2D).
  // - category: which volume slider it answers to (CATEGORIES).
  // - gain, pitch: multipliers on top of the sound's own.
  // - loop: force looping on (or off) whatever the sound says.
  // - overrides: properties to use over the sound's (a placed emitter's
  //   MinDistance and MaxDistance).
  // Returns a handle with stop(), or null when the name plays nothing.
  play(name, options = {}) {
    if (!this.ctx || !this.data) return null;
    const sound = this.data.sound(name);
    if (!sound) {
      this.missing?.(name);
      return null;
    }
    // Before the first click the context is suspended: a one-shot asked for
    // then would only burst out later, so it doesn't play. Loops start.
    const loops = options.loop ?? prop(sound, "Looping") > 0;
    if (this.ctx.state !== "running" && !loops && !options.offline) return null;
    if (sound.type === "SoundLayered") return this.playLayered(sound, options);
    if (sound.type !== "SoundProperties") return null;
    // The play gate: PlayInterval since the last start, then PlayProbability.
    const state = this.gateOf(sound);
    if (!gate(sound, state, this.time)) return null;
    const entry = pickSample(sound.SampleList, (id) => Boolean(this.data.samples[id]), state, this.time);
    if (!entry) return null;
    // Out of earshot where it is asked for: a one-shot doesn't start.
    if (!loops && this.outOfEarshot(sound, options)) {
      this.stats.culled = (this.stats.culled || 0) + 1;
      return null;
    }
    // The pool is full: steal the lowest-scoring voice, if this beats it.
    let pool = 0;
    let lowest = null;
    for (const v of this.voices) {
      if (!(v instanceof Voice) || v.done) continue;
      pool++;
      if (!lowest || v.score < lowest.score) lowest = v;
    }
    if (pool >= this.virtualVoices) {
      const bias = prop(sound, "Bias");
      const estimate = (1 - bias) * this.busGain(sound.Bus) + bias * prop(sound, "Priority");
      if (!lowest || !(estimate > lowest.score)) {
        this.stats.dropped = (this.stats.dropped || 0) + 1;
        return null;
      }
      lowest.stop(STOP_FADE);
      this.stats.stolen += 1;
    }
    const voice = new Voice(this, sound, entry, options);
    this.voices.push(voice);
    this.stats.started += 1;
    return voice;
  }

  // Whether a 3D sound asked for at a place (options.position, or where
  // options.follow says it is now) has no level there: at or past its
  // MuteDistance (a placed emitter's own distances over the sound's).
  outOfEarshot(sound, options) {
    if (!(prop(sound, "Mode3D") > 0)) return false;
    const p = options.position || options.follow?.();
    if (!p) return false;
    const ear = this.listener.ear;
    const dx = p.x - ear.x;
    const dy = p.y - ear.y;
    const dz = p.z - ear.z;
    const curve = options.overrides ? { ...sound, ...options.overrides } : sound;
    return !(distanceGain(curve, Math.sqrt(dx * dx + dy * dy + dz * dz)) > 0);
  }

  // Each sound's play gate and sample cycle, kept across plays.
  gateOf(sound) {
    let state = this.gates.get(sound.Name);
    if (!state) {
      state = { next: -Infinity };
      if (prop(sound, "CyclePlayback") > 0) state.cycle = -1;
      this.gates.set(sound.Name, state);
    }
    return state;
  }

  // Each Layer(name, sound, delay, delayDev) plays its sound after its delay.
  playLayered(layered, options) {
    const layers = [];
    for (const layer of layered.Layer || []) {
      const [, soundKey, delay = 0, dev = 0] = layer;
      const wait = Math.max(delay + dev * (2 * Math.random() - 1), 0);
      layers.push({ wait, soundKey });
    }
    const group = new LayeredHandle(this, layers, options);
    this.voices.push(group);
    return group;
  }

  // A SoundParameterized (engines): its SoundLayered's layers play, and each
  // layer's Gain or Pitch follows a graph of an input the game feeds it
  // (Speed, Acceleration, Health), straight lines between the graph's points
  // and flat past its ends. Without one, the name plays as a plain sound.
  playParameterized(name, options = {}) {
    if (!this.ctx || !this.data) return null;
    const p = this.data.object("SoundParameterized", name);
    const layered = p && this.data.object("SoundLayered", p.SoundLayered);
    if (!layered) return this.play(name, options);
    const bindings = new Map();
    for (const layer of p["Layer.children"] || []) {
      const layerName = [].concat(layer.Name)[0];
      const [input, graphKey, output] = layer.ParameterGraph || [];
      const points = this.data.object("ParameterGraph", graphKey)?.ControlPoint || [];
      if (!bindings.has(layerName)) bindings.set(layerName, []);
      bindings.get(layerName).push({ input, output, points });
    }
    const layers = (layered.Layer || []).map(([layerName, soundKey]) => ({
      bindings: bindings.get(layerName) || [],
      handle: this.play(soundKey, { ...options, loop: true }),
    }));
    return new ParameterizedHandle(layers);
  }

  update(dt) {
    if (!this.ctx || !this.data) return;
    this.time += dt;
    const l = this.listener;
    this.updateDucks(dt);
    // Each voice's place and loudness at the listener.
    for (let i = 0; i < this.voices.length; i++) this.voices[i].measure(l, dt);
    // Rank them: priority first, loudness among equals; what can't be heard
    // drops out.
    const live = this.live;
    live.length = 0;
    for (const v of this.voices) if (!v.done && v.audible !== undefined) live.push(v);
    live.sort(byScore);
    let real = 0;
    for (const voice of live) {
      const want = voice.score > 0 && real < this.maxVoices;
      if (want) real++;
      voice.setReal(want);
    }
    for (const voice of this.voices) voice.apply(l);
    // Finished voices leave the list (in place, in order).
    const voices = this.voices;
    let kept = 0;
    for (let i = 0; i < voices.length; i++) if (!voices[i].done) voices[kept++] = voices[i];
    voices.length = kept;
    this.stats.real = real;
    this.stats.virtual = live.length - real;
    live.length = 0;
    this.renderer?.flush();
  }

  // Voice-over and the like pull other buses down while they play: a linear
  // ramp from where the bus is to 1 - Attenuation over the lead-in when the
  // first voice starts, and back to 1 over the lead-out when the last ends.
  updateDucks(dt) {
    this.busGains.clear();
    for (const bus of this.buses.values()) {
      const f = bus.fade;
      if (!f) continue;
      f.elapsed += dt;
      bus.fadeLevel = f.from + (f.to - f.from) * Math.min(f.elapsed / f.length, 1);
      if (f.elapsed >= f.length) bus.fade = null;
    }
    for (const duck of this.ducks) {
      let busy = false;
      for (const v of this.voices) {
        if (v.bus === duck.bus && !v.done) {
          busy = true;
          break;
        }
      }
      busy ||= Boolean(this.streamBusy?.(duck.bus));
      if (busy !== duck.busy) {
        duck.busy = busy;
        duck.from = duck.level;
        duck.to = busy ? 1 - duck.amount : 1;
        duck.length = Math.max(busy ? duck.leadIn : duck.leadOut, 1e-3);
        duck.elapsed = 0;
      }
      if (duck.to !== undefined && duck.level !== duck.to) {
        duck.elapsed += dt;
        const f = Math.min(duck.elapsed / duck.length, 1);
        duck.level = duck.from + (duck.to - duck.from) * f;
      }
      const target = this.buses.get(duck.target);
      if (target) target.duck = duck.level;
    }
  }

  stopAll() {
    for (const voice of this.voices) voice.stop(0);
  }

  // A bus's gain ramped to a level over some seconds (ScriptCB_SndBusFade).
  fadeBus(key, level, seconds) {
    const bus = this.buses.get(key);
    if (!bus) return;
    bus.fade = { from: bus.fadeLevel ?? 1, to: level, length: Math.max(seconds, 1e-3), elapsed: 0 };
  }
}

// One playing sound. It keeps its own clock so it can drop out (virtual) and
// come back where it would have been.
class Voice {
  constructor(engine, sound, entry, options) {
    this.engine = engine;
    // A placed emitter can set its own distances over the sound's.
    this.sound = options.overrides ? { ...sound, ...options.overrides } : sound;
    this.sampleId = entry[0];
    this.buffer = engine.buffer(this.sampleId);
    this.options = options;
    this.category = options.category || "effects";
    this.bus = this.sound.Bus || "0xeceb308a";
    this.position = options.position ? { x: options.position.x, y: options.position.y, z: options.position.z } : null;
    this.follow = options.follow || null;
    this.mode3D = this.position || this.follow ? prop(this.sound, "Mode3D") : 0;
    this.loop = options.loop ?? prop(this.sound, "Looping") > 0;
    this.gain = rollGain(this.sound) * (options.gain ?? 1);
    this.pitch = rollPitch(this.sound) * (options.pitch ?? 1);
    this.priority = prop(this.sound, "Priority");
    this.bias = prop(this.sound, "Bias");
    this.doppler = prop(this.sound, "ModeDoppler") > 0;
    this.reverbGain = prop(this.sound, "ReverbGain");
    this.reach = reach(this.sound);
    // The distance curve's properties and the pan, resolved once (prop()
    // looks up defaults on every call).
    this.curve = {
      RollInDistance: prop(this.sound, "RollInDistance"),
      MinDistance: prop(this.sound, "MinDistance"),
      MuteDistance: prop(this.sound, "MuteDistance"),
      MaxDistance: prop(this.sound, "MaxDistance"),
      RollOff: prop(this.sound, "RollOff"),
    };
    this.pan = prop(this.sound, "Pan");
    this.elapsed = 0; // s of the sample played (at its own rate)
    // The sample's own length: its buffer may run faster (rateScale < 1).
    this.duration = this.buffer ? this.buffer.duration / (this.buffer.rateScale || 1) : 0;
    // RandomPlayPos starts it anywhere in the sample.
    if (prop(this.sound, "RandomPlayPos") === 1) this.elapsed = Math.random() * this.duration;
    this.glide = null; // the original's gliding pitch, once started
    this.done = !this.buffer;
    this.real = false;
    this.nodes = null;
    this.distance = 0;
    this.audible = 0;
    this.score = 0;
    this.local = { x: 0, y: 0, z: -1 };
    this.last = null;
    this.radial = 0;
    this.extra = {};
    this.started = engine.time;
  }

  get playing() {
    return !this.done;
  }

  // Non-finite multipliers are ignored: they would reach an AudioParam,
  // which throws, and a NaN pitch would also poison the glide for good.
  setPitch(multiplier) {
    if (Number.isFinite(multiplier)) this.pitchScale = multiplier;
  }

  // The original game's pitch setter: an absolute pitch in place of the rolled
  // one (the charge and overheat sounds).
  setAbsolutePitch(pitch) {
    if (!Number.isFinite(pitch)) return;
    this.pitch = pitch;
    this.pitchScale = 1;
  }

  setGain(multiplier) {
    if (Number.isFinite(multiplier)) this.gainScale = multiplier;
  }

  moveTo(position) {
    if (!this.position) this.position = { x: 0, y: 0, z: 0 };
    copy(this.position, position);
  }

  measure(l, dt) {
    if (this.done) return;
    if (this.follow) {
      const p = this.follow();
      if (p) this.moveTo(p);
    }
    const e = this.engine;
    let gain = Math.min(this.gain * e.busGain(this.bus), 1) * (this.gainScale ?? 1);
    if (this.mode3D && this.position) {
      const d = placeVoice(this, l);
      // Doppler wants the speed toward the listener.
      if (this.last !== null && dt > 0) this.radial = (this.last - d) / dt;
      this.last = d;
      gain *= distanceGain(this.curve, d);
      // The Steam build's OpenAL output: its own falloff past Max / 2.
      if (!e.renderer?.enhanced) gain *= openAlFalloff(this.curve, d);
    } else {
      this.distance = 0;
      this.span = 0;
    }
    this.audible = gain;
    // The library's score: (1 - Bias) x the level + Bias x Priority. With
    // the game's Bias of 0.0001 that is the level; nothing at all scores 0.
    this.score = gain > 1e-6 ? (1 - this.bias) * gain + this.bias * this.priority : 0;
    // The voice's own clock runs whether it is heard or not, from the frame
    // after it was asked for (it starts at its first sample).
    if (this.begun) this.elapsed += dt * this.pitch * (this.pitchScale ?? 1);
    this.begun = true;
    if (!this.loop && this.elapsed >= this.duration) this.finish();
  }

  setReal(want) {
    if (this.done) return;
    if (want && !this.real) this.start();
    else if (!want && this.real) this.drop();
  }

  start() {
    const e = this.engine;
    const ctx = e.ctx;
    const scale = this.buffer.rateScale || 1;
    // Where the sample is now, in the buffer's own time.
    const offset = (this.loop ? this.elapsed % this.duration : this.elapsed) * scale;
    if (!this.loop && offset >= this.buffer.duration) {
      this.finish();
      return;
    }
    const source = ctx.createBufferSource();
    source.buffer = this.buffer;
    source.loop = this.loop;
    const gain = ctx.createGain();
    gain.gain.value = 0;
    source.connect(gain);
    const out = e.renderer.voice(this);
    out.attach(gain);
    const rate = this.rate();
    source.playbackRate.value = rate;
    source.start(ctx.currentTime, Math.max(offset, 0));
    source.onended = () => {
      if (!this.loop && this.nodes?.source === source) this.finish();
    };
    this.nodes = { source, gain, out, fresh: true, level: new Target(), rate: new Target() };
    this.nodes.rate.hold(rate);
    this.real = true;
  }

  rate() {
    let rate = this.pitch * (this.pitchScale ?? 1);
    if (this.doppler && this.mode3D) rate *= dopplerRate(this.radial);
    // As a frequency in Hz the OpenAL output keeps it within 100 Hz-200 kHz.
    const hz = this.buffer.sampleRate * (this.buffer.rateScale || 1);
    rate = Math.min(Math.max(rate * hz, HZ_RANGE[0]), HZ_RANGE[1]) / hz;
    // The original: changes glide 1/256 of the way twice a frame.
    if (!this.engine.renderer?.enhanced) {
      if (this.glide === null) this.glide = rate;
      else this.glide += (rate - this.glide) * PITCH_GLIDE;
      rate = this.glide;
    }
    rate *= this.buffer.rateScale || 1;
    return Number.isFinite(rate) && rate > 0 ? rate : 1;
  }

  apply() {
    if (!this.real || !this.nodes || this.done) return;
    const e = this.engine;
    const now = e.ctx.currentTime;
    const extra = e.shape ? e.shape(this) : this.extra;
    const raw = this.audible * e.categoryVolume(this.category) * (extra.gain ?? 1);
    const level = Number.isFinite(raw) ? raw : 0;
    const nodes = this.nodes;
    const g = nodes.gain.gain;
    // A voice's first gain applies at once, so its attack isn't softened.
    if (nodes.fresh) {
      g.setValueAtTime(level, now);
      nodes.level.hold(level);
      nodes.fresh = false;
    } else {
      nodes.level.set(g, level, now, SMOOTH);
    }
    nodes.rate.set(nodes.source.playbackRate, this.rate(), now, SMOOTH);
    extra.send = (extra.send ?? 0) * (e.renderer?.enhanced ? this.reverbGain : 1);
    // The Steam build's 2D voices never get their Pan: they play centred.
    const pan = e.renderer?.enhanced ? this.pan : 0;
    this.nodes.out.update(this.mode3D ? this.local : null, this.span, pan, extra);
  }

  drop() {
    this.release(STOP_FADE);
    this.real = false;
    this.engine.stats.stolen += 1;
  }

  release(fade) {
    const nodes = this.nodes;
    if (!nodes) return;
    this.nodes = null;
    const now = this.engine.ctx.currentTime;
    nodes.gain.gain.cancelScheduledValues(now);
    nodes.gain.gain.setTargetAtTime(0, now, Math.max(fade, 1e-3) / 3);
    try {
      nodes.source.stop(now + fade + 0.01);
    } catch {
      // already stopped
    }
    setTimeout(() => {
      nodes.source.disconnect();
      nodes.gain.disconnect();
      nodes.out.dispose();
    }, (fade + 0.1) * 1000);
  }

  stop(fade = STOP_FADE) {
    if (this.done) return;
    this.release(fade);
    this.real = false;
    this.done = true;
  }

  finish() {
    this.stop(0.005);
  }
}

// A SoundLayered's layers: each starts after its own delay.
class LayeredHandle {
  constructor(engine, layers, options) {
    this.engine = engine;
    this.layers = layers;
    this.options = options;
    this.handles = [];
    this.clock = 0;
    this.done = false;
    this.position = options.position ? { ...options.position } : null;
  }

  get playing() {
    return !this.done;
  }

  measure(l, dt) {
    if (this.done) return;
    for (const layer of this.layers) {
      if (layer.started || this.clock < layer.wait) continue;
      layer.started = true;
      const handle = this.engine.play(layer.soundKey, { ...this.options, position: this.position || this.options.position });
      if (handle) {
        if (this.pitchScale) handle.setPitch?.(this.pitchScale);
        if (this.gainScale) handle.setGain?.(this.gainScale);
        this.handles.push(handle);
      }
    }
    this.clock += dt;
    if (this.layers.every((layer) => layer.started) && this.handles.every((h) => h.done)) this.done = true;
  }

  moveTo(position) {
    if (!this.position) this.position = { x: 0, y: 0, z: 0 };
    copy(this.position, position);
    for (const h of this.handles) h.moveTo?.(position);
  }

  setReal() {}

  apply() {}

  setPitch(m) {
    this.pitchScale = m;
    for (const h of this.handles) h.setPitch?.(m);
  }

  setAbsolutePitch(p) {
    for (const h of this.handles) h.setAbsolutePitch?.(p);
  }

  setGain(m) {
    this.gainScale = m;
    for (const h of this.handles) h.setGain?.(m);
  }

  stop(fade) {
    this.done = true;
    for (const h of this.handles) h.stop(fade);
  }
}

const GAIN = "0x1b5426fe"; // hash of "Gain"
const PITCH = "0xbd324ab1"; // hash of "Pitch"

class ParameterizedHandle {
  constructor(layers) {
    this.layers = layers;
  }

  get done() {
    return this.layers.every((l) => !l.handle || l.handle.done);
  }

  // Feed an input (its name's key, e.g. hashKey("Speed")).
  set(inputKey, value) {
    for (const layer of this.layers) {
      if (!layer.handle) continue;
      for (const b of layer.bindings) {
        if (b.input !== inputKey) continue;
        const y = graph(b.points, value);
        // Graph outputs multiply the layer's own: Gain within 0..1, Pitch no
        // lower than 0.1.
        if (b.output === GAIN) layer.handle.setGain(Math.min(Math.max(y, 0), 1));
        else if (b.output === PITCH) layer.handle.setPitch(Math.max(y, 0.1));
      }
    }
  }

  // Every input at once, as the original game updates a vehicle's engine: each
  // layer's Gain and Pitch start at 1 and every binding fed this update
  // multiplies its own in, through its graph, or with `raw` the value
  // itself (the original game passes 1 raw for an input that is idle, which
  // leaves the layer as it is). inputs: key -> { value, raw }.
  apply(inputs) {
    for (const layer of this.layers) {
      if (!layer.handle) continue;
      let gain = 1;
      let pitch = 1;
      for (const b of layer.bindings) {
        const input = inputs[b.input];
        if (!input) continue;
        const y = input.raw ? input.value : graph(b.points, input.value);
        if (b.output === GAIN) gain *= y;
        else if (b.output === PITCH) pitch *= y;
      }
      layer.handle.setGain(Math.min(Math.max(gain, 0), 1));
      layer.handle.setPitch(Math.max(pitch, 0.1));
    }
  }

  moveTo(position) {
    for (const l of this.layers) l.handle?.moveTo?.(position);
  }

  stop(fade) {
    for (const l of this.layers) l.handle?.stop(fade);
  }
}

// Highest score first (Array.prototype.sort is stable, so ties keep their
// order in the voice list).
function byScore(a, b) {
  return b.score - a.score;
}

// A 3D voice's place against the listener l: its distance from the listener
// point ahead of the camera (voice.distance, returned) and its direction
// from the camera itself (voice.local, x right, y up, z back; voice.span,
// the distance to the camera). voice.position must be set.
export function placeVoice(voice, l) {
  const p = voice.position;
  const ex = p.x - l.ear.x;
  const ey = p.y - l.ear.y;
  const ez = p.z - l.ear.z;
  // (Math.hypot is several times slower than this.)
  const d = Math.sqrt(ex * ex + ey * ey + ez * ez);
  voice.distance = d;
  const dx = p.x - l.position.x;
  const dy = p.y - l.position.y;
  const dz = p.z - l.position.z;
  const span = Math.sqrt(dx * dx + dy * dy + dz * dz);
  const inv = span > 1e-4 ? 1 / span : 0;
  voice.span = span;
  voice.local.x = (dx * l.right.x + dy * l.right.y + dz * l.right.z) * inv;
  voice.local.y = (dx * l.up.x + dy * l.up.y + dz * l.up.z) * inv;
  voice.local.z = -(dx * l.forward.x + dy * l.forward.y + dz * l.forward.z) * inv;
  return d;
}

// The Doppler factor for a source closing on the listener at `radial` m/s
// (negative going away): 1 + radial / 344, none at 172 m/s or faster.
export function dopplerRate(radial) {
  return Math.abs(radial) < DOPPLER_LIMIT ? 1 + radial / SOUND_SPEED : 1;
}

export function graph(points, x) {
  if (!points.length) return 1;
  if (x <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    const [x1, y1] = points[i];
    if (x <= x1) {
      const [x0, y0] = points[i - 1];
      return x1 > x0 ? y0 + ((y1 - y0) * (x - x0)) / (x1 - x0) : y1;
    }
  }
  return points[points.length - 1][1];
}

function copy(out, v) {
  out.x = v.x;
  out.y = v.y;
  out.z = v.z;
  return out;
}

function normalize(v) {
  const n = Math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z) || 1;
  v.x /= n;
  v.y /= n;
  v.z /= n;
  return v;
}
