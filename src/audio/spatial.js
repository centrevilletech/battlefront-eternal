import { prepareAudioPcm, scalePcm } from "./pcm.js";
import { VOICE } from "./commands.js";
import { Target } from "./automation.js";
import { ALSOFT, LAYOUTS, autoLayout } from "./layouts.js";
import { loadAudioDsp } from "./wasm-dsp.js";
import { decodeAudioAsset } from "./decode.js";

// The page chooses sounds and sends context-timed control commands. Rust
// owns every sample, source, bed, panner, decoder and master operation.
// One worklet stays attached across output changes; channel truncation at
// the device is discrete I/O, with mono and all downmixing already in Rust.
export const SLOTS = 64;
const STREAMS = 32;
const DECODER = "hrtf/sadie2_d1_3oa";
const KINDS = { plain: 0, voice: 1, music: 2, ambience: 0 };
const LAWS = { panpot: 0, vbap: 1, alsoft: 2, ambi3: 3 };
const CLARITY_EQ = [[2, 220, -2, 0.9], [2, 3200, 1.5, 0.8], [3, 8500, 3, 0.7]];
export const ORIGINAL_LEVEL = { stereo: 1.5, "5.1": 3.3, "7.1": 0.9, "7.1.4": 0.9, headphones: 0 };
export const DYNAMICS = {
  // Home theater: the full range, as mixed.
  home: { label: "Home theater (full range)", threshold: 0, ratio: 1, knee: 0, attack: 0.003, release: 0.25, makeup: -10.2 },
  // Normal: gentle compression of the loudest moments, slow enough to let
  // each attack through (25 ms) so shots keep their punch.
  normal: { label: "Normal", threshold: -18, ratio: 2, knee: 8, attack: 0.025, release: 0.2, makeup: -2.3 },
  // Night: a narrow range, so quiet sounds carry and blasts don't startle.
  night: { label: "Night (narrow range)", threshold: -30, ratio: 5, knee: 10, attack: 0.004, release: 0.35, makeup: 10.6 },
};
export const LIGHT_LEVELS = { home: -10.2, normal: -6.1, night: -5.7 };

export class Renderer {
  constructor(ctx) {
    this.ctx = ctx;
    this.config = { quality: "enhanced", output: "auto", dynamics: "normal", mono: false, clarity: true };
    this.voices = new Set(); this.beds = new Set();
    this.slots = []; this.streamSlots = []; this.commands = []; this.batch = [];
    this.freeStreamSamples = []; this.sampleIds = new WeakMap(); this.uploaded = new Set(); this.nextSample = 0;
    this.roomParams = null; this.masterVolume = 1; this.decoder = null;
    this.engineGen = 0; this.waits = new Map(); this.nextId = 0;
    this.onLimiter = null; this.onMeter = null; this.onError = null;
  }
  fail(error) {
    this.error = error instanceof Error ? error : new Error(String(error));
    this.onError?.(this.error);
    for (const wait of this.waits.values()) wait.reject(this.error);
    this.waits.clear();
    return this.error;
  }
  async worklets() {
    if (!this.modules) this.modules = Promise.all([
      this.ctx.audioWorklet.addModule(new URL("./worklets/output.js?no-inline", import.meta.url)),
      this.ctx.audioWorklet.addModule(new URL("./worklets/meter.js?no-inline", import.meta.url)),
      loadAudioDsp(), prepareAudioPcm(),
    ]).then(([, , dsp]) => { this.dsp = dsp; this.workletsReady = true; }).catch((error) => { throw this.fail(error); });
    return this.modules;
  }
  async load(root = null, config = null) {
    if (config) this.config = { ...this.config, ...config };
    if (!this.loaded) this.loaded = Promise.all([this.worklets(), root ? this.loadDecoder(root) : null]);
    await this.loaded;
    return true;
  }
  async loadDecoder(root) {
    const info = await fetch(`${root}/${DECODER}.json`).then((r) => r.ok ? r.json() : null);
    if (!info) return null;
    const ears = await Promise.all(["left", "right"].map(async (ear) => {
      const response = await fetch(`${root}/${DECODER}_${ear}.wav`);
      if (!response.ok) throw new Error(`audio: missing ${ear} headphone decoder`);
      return decodeAudioAsset(this.ctx, await response.arrayBuffer());
    }));
    if (ears.some((ear) => ear.numberOfChannels !== 16)) throw new Error("audio: headphone decoder must have sixteen channels");
    const scale = (info.rate || 48000) / this.ctx.sampleRate;
    await Promise.all(ears.map((buffer) => scalePcm(buffer, scale, false)));
    const filters = [];
    for (let c = 0; c < 16; c++) {
      const b = this.ctx.createBuffer(2, ears[0].length, this.ctx.sampleRate);
      for (let ear = 0; ear < 2; ear++) {
        const data = b.getChannelData(ear); data.set(ears[ear].getChannelData(c));
      }
      filters.push(b);
    }
    this.decoder = { info, filters };
    return this.decoder;
  }
  resolve(output) {
    const max = this.ctx.destination.maxChannelCount || 2;
    let mode = output === "auto" || !LAYOUTS[output] ? autoLayout(max) : output;
    if (LAYOUTS[mode].channels > max) mode = autoLayout(max);
    return mode;
  }
  async configure(config) {
    this.config = { ...this.config, ...config };
    await this.worklets();
    await this.build();
  }
  async build() {
    const config = this.config;
    this.mode = this.resolve(config.output); this.layout = LAYOUTS[this.mode]; this.channels = this.layout.channels;
    this.enhanced = config.quality !== "original";
    this.light = !this.enhanced && this.mode === "stereo";
    this.binaural = this.enhanced && this.mode === "headphones" && Boolean(this.decoder);
    this.hrtf = this.mode === "headphones" && !this.binaural;
    this.alsoft = !this.enhanced && this.mode !== "headphones" ? ALSOFT[this.mode] : null;
    this.engine = true; this.master = !this.light; this.lfe = this.enhanced && this.layout.lfe >= 0;
    const destination = this.ctx.destination;
    destination.channelCount = this.channels; destination.channelCountMode = "explicit"; destination.channelInterpretation = "discrete";
    const id = ++this.nextId;
    const options = this.options(id);
    const ready = new Promise((resolve, reject) => this.waits.set(id, { resolve, reject }));
    if (!this.mixer) {
      this.slots = Array.from({ length: SLOTS }, (_, i) => SLOTS - 1 - i);
      this.streamSlots = Array.from({ length: STREAMS }, (_, i) => STREAMS - 1 - i);
      this.engineGen++;
      this.mixer = new AudioWorkletNode(this.ctx, "audio-output", {
        numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [12], processorOptions: options,
      });
      this.mixer.port.onmessage = ({ data }) => {
        if (data.type === "ready") { this.waits.get(data.id)?.resolve(); this.waits.delete(data.id); }
        else if (data.type === "error") this.fail(new Error(data.message));
        else if (data.reduction !== undefined) this.onLimiter?.(data);
      };
      this.mixer.addEventListener("processorerror", () => this.fail(new Error("audio: the Rust render processor stopped; reload the game")));
      this.mixer.connect(destination);
      this.output = this.mixer;
    } else {
      this.mixer.port.postMessage({ type: "configure", options });
      for (const voice of this.voices) voice.rebuild();
      for (const bed of this.beds) bed.voice?.rebuild();
    }
    await ready;
    if (this.meterNode) { this.output.disconnect(this.meterNode); this.meterNode.disconnect(); this.meterNode = null; }
    if (this.meterOn) this.meter(true);
  }
  options(id) {
    const layout = this.layout;
    const clarity = this.enhanced && this.config.clarity !== false;
    return { id, dsp: this.dsp, channels: this.channels, enhanced: this.enhanced, headphones: this.mode === "headphones", binaural: this.binaural, mono: Boolean(this.config.mono),
      speakers: layout.speakers, decoder: this.alsoft,
      law: this.binaural ? LAWS.ambi3 : this.alsoft ? LAWS.alsoft : this.channels === 2 ? LAWS.panpot : LAWS.vbap,
      center: this.mode === "headphones" ? -1 : layout.center, lfe: layout.lfe,
      original: 10 ** ((ORIGINAL_LEVEL[this.mode] || 0) / 20), room: this.roomParams,
      dynamics: { ...(DYNAMICS[this.config.dynamics] || DYNAMICS.normal), skip: layout.lfe },
      eq: clarity ? CLARITY_EQ.map(([kind, frequency, gain, q]) => [kind, Math.min(frequency, this.ctx.sampleRate * 0.45), gain, q]) : [],
      clarity: clarity ? 10 ** (-0.5 / 20) : 0, lightLevel: LIGHT_LEVELS[this.config.dynamics] ?? LIGHT_LEVELS.normal,
      volume: this.masterVolume, time: this.ctx.currentTime,
      filters: this.binaural ? this.decoder.filters.flatMap((b) => [b.getChannelData(0), b.getChannelData(1)]) : null,
    };
  }
  setRoom(params) { this.roomParams = { ...this.roomParams, ...params }; this.mixer?.port.postMessage({ type: "room", params }); }
  setVolume(value) { this.masterVolume = value; this.mixer?.port.postMessage({ type: "volume", value, time: this.ctx.currentTime }); }
  voice(voice) { const out = new VoiceOut(this, voice); this.voices.add(out); return out; }
  // Stream lanes own a shared gain and three scheduled segment sources.
  stream(channels, kind = "plain", voice = null) {
    const out = new StreamOut(this, channels, kind, voice); this.beds.add(out); return out;
  }
  flush() {
    if (!this.mixer || this.error) return;
    if (this.batch.length) { const data = Float64Array.from(this.batch); this.mixer.port.postMessage({ type: "params", data }, [data.buffer]); this.batch.length = 0; }
    if (this.commands.length) { const data = Float64Array.from(this.commands); this.mixer.port.postMessage({ type: "voices", data }, [data.buffer]); this.commands.length = 0; }
  }
  command(...values) {
    this.commands.push(...values);
    if (!this.flushQueued) { this.flushQueued = true; queueMicrotask(() => { this.flushQueued = false; this.flush(); }); }
  }
  uploadSample(buffer, id) {
    const data = new Float32Array(buffer.length * buffer.numberOfChannels);
    for (let c = 0; c < buffer.numberOfChannels; c++) data.set(buffer.getChannelData(c), c * buffer.length);
    this.mixer.port.postMessage({ type: "sample", id, frames: buffer.length, channels: buffer.numberOfChannels, rate: buffer.sampleRate, data }, [data.buffer]);
  }
  engineSample(buffer) {
    let id = this.sampleIds.get(buffer);
    if (id === undefined) { id = this.nextSample++; this.sampleIds.set(buffer, id); }
    if (!this.uploaded.has(id)) {
      this.uploaded.add(id);
      this.uploadSample(buffer, id);
    }
    return id;
  }
  async testBuffer(tone, seconds) {
    try { this.signalDsp ||= new WebAssembly.Instance(this.dsp).exports; }
    catch (error) { throw this.fail(new Error(`audio: Rust test signals could not start: ${error.message}`)); }
    const x = this.signalDsp, frames = Math.round(seconds * this.ctx.sampleRate);
    const at = x.signal_pcm(this.ctx.sampleRate, frames, tone ? 1 : 0);
    const buffer = this.ctx.createBuffer(1, frames, this.ctx.sampleRate);
    buffer.copyToChannel(new Float32Array(x.memory.buffer, at, frames), 0);
    return buffer;
  }
  meter(on) {
    this.meterOn = on;
    if (on && !this.mixer) return;
    if (on && !this.meterNode) {
      const weights = this.layout.names.map((n) => n === "LFE" ? 0 : /^(SL|SR|BL|BR|TFL|TFR|TBL|TBR)$/.test(n) ? 1.41 : 1);
      this.meterNode = new AudioWorkletNode(this.ctx, "loudness-meter", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: this.channels, channelCountMode: "explicit", channelInterpretation: "discrete", processorOptions: { dsp: this.dsp, weights } });
      this.meterNode.addEventListener("processorerror", () => this.fail(new Error("audio: the required Rust meter processor stopped; reload the game")));
      this.meterNode.port.onmessage = ({ data }) => { if (data.reserve) this.meterNode.port.postMessage({ reserve: true }); this.onMeter?.(data); }; this.output.connect(this.meterNode);
    } else if (!on && this.meterNode) { this.output.disconnect(this.meterNode); this.meterNode.disconnect(); this.meterNode = null; }
  }
  teardown() { this.mixer?.disconnect(); this.meterNode?.disconnect(); this.mixer = this.meterNode = null; }
}

// AudioParam-shaped controls produce only commands, never browser nodes.
function parameter(renderer, send, initial = 0) {
  let value = initial;
  return { get value() { return value; }, set value(v) { value = v; send(v, renderer.ctx.currentTime, 0, true); },
    setValueAtTime(v, now) { value = v; send(v, now, 0, true); },
    setTargetAtTime(v, now, tau) { value = v; send(v, now, tau, false); },
    cancelScheduledValues(now) { send(value, now, -1, false); },
  };
}
class VoiceOut {
  constructor(r, voice, stream = null) {
    this.r = r; this.voice = voice; this.stream = stream; this.level = 1; this.cutoff = 20000;
    this.engine = true; this.lost = false; this.slot = stream ? SLOTS + stream.slot : r.slots.pop();
    if (this.slot === undefined) throw r.fail(new Error("audio: Rust voice capacity exhausted"));
    if (!stream) r.commands.push(VOICE.BIND, this.slot, voice.mode3D ? 1 : 0);
    this.rateParam = parameter(r, (v, at, tau) => r.commands.push(VOICE.RATE, this.slot, v, tau, at), 1);
    this.gainParam = parameter(r, (v, at, tau, set) => r.commands.push(set ? VOICE.GAIN_SET : VOICE.GAIN, this.slot, v, ...(set ? [at] : [tau, at])));
    this.sendParam = parameter(r, (v, at, tau) => r.commands.push(stream ? VOICE.STREAM_PARAM : VOICE.SEND, ...(stream ? [stream.slot, 1] : [this.slot]), v, tau, at));
    this.position = [0, 1, 2].map((axis) => new Target());
    this.positionParam = [0, 1, 2].map((axis) => parameter(r, (v, at, tau, set) => r.commands.push(VOICE.POSITION, this.slot, axis, v, tau, at, set ? 1 : 0)));
    for (const p of this.position) p.hold(0);
    this.pan = new Target(); this.pan.hold(0);
    this.panParam = parameter(r, (v, at, tau) => r.commands.push(VOICE.PAN, this.slot, v, tau, at));
    this.sendLevel = new Target(); this.sendLevel.hold(0); this.sent = new Float32Array(5).fill(NaN);
    this.rebuild();
  }
  rebuild() { this.sides = this.r.light ? [0, 1].map((c) => ({ gain: this.sideParam(c) })) : null; this.sent.fill(NaN); }
  update(local, distance, pan, extra = {}) {
    const r = this.r, now = r.ctx.currentTime;
    let x, y, z;
    if (local) { x = local.x; y = local.y; z = local.z; }
    else { const a = Math.max(-1, Math.min(1, pan || 0)) * Math.PI / 2; x = Math.sin(a); y = 0; z = -Math.cos(a); }
    if (r.light) { r.commands.push(this.stream ? VOICE.STREAM_PLACE : VOICE.PLACE, this.stream ? this.stream.slot : this.slot, x, y, z, this.stream ? 1 : this.level, now); return; }
    if (r.hrtf && (this.voice.mode3D || !r.enhanced)) {
      const d = Math.max(distance, 1); this.position[0].set(this.positionParam[0], x * d, now, 0.01); this.position[1].set(this.positionParam[1], y * d, now, 0.01); this.position[2].set(this.positionParam[2], z * d, now, 0.01);
    } else if (r.binaural && !this.voice.mode3D) this.pan.set(this.panParam, Math.max(-1, Math.min(1, x)), now, 0.015);
    else {
      let spread = extra.spread ?? 0;
      if (local && r.enhanced) spread = Math.max(spread, Math.min(Math.max(1 - distance / 1.5, 0), 1) * 0.6);
      const lfe = r.enhanced && r.lfe ? (extra.lfe ?? 0) * 0.5 : 0;
      if (this.sent[0] !== Math.fround(x) || this.sent[1] !== Math.fround(y) || this.sent[2] !== Math.fround(z) || this.sent[3] !== Math.fround(spread) || this.sent[4] !== Math.fround(lfe)) {
        this.sent.set([x, y, z, spread, lfe]);
        const slot = this.stream ? SLOTS + this.stream.slot * 12 : this.slot;
        r.batch.push(slot, x, y, z, spread, lfe);
      }
    }
    if (r.enhanced && extra.cutoff !== undefined && Math.abs(extra.cutoff - this.cutoff) > 10) {
      this.cutoff = extra.cutoff; r.commands.push(this.stream ? VOICE.STREAM_PARAM : VOICE.CUTOFF, ...(this.stream ? [this.stream.slot, 0] : [this.slot]), Math.min(this.cutoff, r.ctx.sampleRate / 2), 0.03, now);
    }
    if (extra.send !== undefined) this.sendLevel.set(this.sendParam, extra.send, now, 0.05);
  }
  engineStart(buffer, offset, loop, rate) {
    const r = this.r; r.commands.push(VOICE.START, this.slot, r.engineSample(buffer), offset, loop ? 1 : 0, rate, Math.min(this.cutoff, r.ctx.sampleRate / 2), r.ctx.currentTime);
    if (this.sides) for (const side of this.sides) side.gain.value = 0;
  }
  engineGain(value) { this.r.commands.push(VOICE.GAIN_SET, this.slot, value, this.r.ctx.currentTime); }
  engineRelease(tau, after) {
    const now = this.r.ctx.currentTime;
    if (this.sides) { for (const side of this.sides) { side.gain.cancelScheduledValues(now); side.gain.setTargetAtTime(0, now, tau); } this.r.command(VOICE.STOP_AT, this.slot, now + after); }
    else this.r.command(VOICE.RELEASE, this.slot, tau, after, now);
  }
  sideParam(c) { return parameter(this.r, (v, at, tau, set) => this.r.commands.push(tau < 0 ? VOICE.CANCEL : set ? VOICE.SIDE_SET : VOICE.SIDE, this.slot, c, ...(tau < 0 ? [at] : set ? [v, at] : [v, tau, at]))); }
  dispose() { if (this.lost) return; this.lost = true; if (!this.stream) { this.r.command(VOICE.STOP, this.slot); this.r.slots.push(this.slot); this.r.voices.delete(this); } }
}
class StreamOut {
  constructor(r, channels, kind, voice) {
    this.r = r; this.slot = r.streamSlots.pop(); this.parts = [null, null, null]; this.samples = Array.from({ length: 3 }, () => r.freeStreamSamples.pop() ?? r.nextSample++); this.channels = channels; this.kind = Number.isInteger(kind) ? kind : (KINDS[kind] ?? 0); this.spatial = voice?.mode3D ? 1 : 0;
    if (this.slot === undefined) throw r.fail(new Error("audio: Rust stream capacity exhausted"));
    r.commands.push(VOICE.STREAM_BIND, this.slot, channels, Number.isInteger(kind) ? kind : (KINDS[kind] ?? 0), voice?.mode3D ? 1 : 0);
    this.gain = parameter(r, (v, at, tau, set) => r.commands.push(VOICE.STREAM_GAIN, this.slot, v, tau, at, set ? 1 : 0), 1);
    this.voice = voice ? new VoiceOut(r, voice, this) : null;
  }
  start(buffer, when = this.r.ctx.currentTime, pitch = 1, loop = false, offset = 0) {
    const r = this.r;
    // The first decoded segment fixes the lane width, as the stream's
    // explicit native input did. Narrower later segments contribute zeros.
    if (!this.boundFormat) {
      this.boundFormat = true;
      if (this.channels !== buffer.numberOfChannels) { this.channels = buffer.numberOfChannels; r.commands.push(VOICE.STREAM_BIND, this.slot, this.channels, this.kind, this.spatial); }
    }
    let part = this.parts.findIndex((p) => !p || p.ends <= r.ctx.currentTime);
    if (part < 0) throw r.fail(new Error("audio: all Rust segment slots are already scheduled"));
    const playing = { part, ends: loop ? Infinity : when + (buffer.duration - offset) / pitch, stop: (at) => { playing.ends = Math.min(playing.ends, at); r.command(VOICE.STREAM_STOP, this.slot, part, at); } };
    this.parts[part] = playing;
    r.uploadSample(buffer, this.samples[part]);
    r.command(VOICE.STREAM_START, this.slot, part, this.samples[part], when, offset, pitch, loop ? 1 : 0);
    return playing;
  }
  dispose() { if (this.disposed) return; this.disposed = true; this.r.command(VOICE.STREAM_RELEASE, this.slot); this.r.flush(); this.r.mixer.port.postMessage({ type: "release-samples", ids: this.samples }); this.r.freeStreamSamples.push(...this.samples); this.r.streamSlots.push(this.slot); this.r.beds.delete(this); }
}
