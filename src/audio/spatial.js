import { prepareAudioPcm } from "./pcm.js";
import { INFO as CONTROL_LAYOUT } from "./control-layout.js";
import { ALSOFT, LAYOUTS, autoLayout } from "./layouts.js";
import { loadAudioDsp } from "./wasm-dsp.js";
import { loadAudioPcm } from "./codec.js";

// The page chooses sounds and sends context-timed control commands. Rust
// owns every sample, source, bed, panner, decoder and master operation.
// One worklet stays attached across output changes; channel truncation at
// the device is discrete I/O, with mono and all downmixing already in Rust.
const DECODER = "hrtf/sadie2_d1_3oa";
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

// This object lives on the page thread. It chooses layouts, assigns slots,
// and expresses game decisions in context time; it never renders a sample.
// The worklet owns source cursors, filter histories and all output DSP.
export class Renderer {
  constructor(ctx) {
    this.ctx = ctx;
    this.config = { quality: "enhanced", output: "auto", dynamics: "normal", mono: false, clarity: true };
    this.commands = []; this.batch = [];
    this.streamWaits = new Map();
    // Sample IDs name Rust storage, not browser nodes. Weak identity avoids
    // uploading the same retained effect twice; stream segments instead recycle
    // three dedicated IDs whose decoder jobs are explicitly closed on reuse.
    this.sampleIds = new WeakMap(); this.uploaded = new Set(); this.nextSample = 0;
    this.roomParams = null; this.masterVolume = 1; this.decoder = null;
    this.configuration = 0; this.waits = new Map(); this.nextId = 0;
    this.onLimiter = null; this.onMeter = null; this.onError = null;
  }
  // Module, processor and producer failures are audible failures. Reject
  // pending readiness promises and notify the player instead of resolving a
  // partially built graph that appears healthy but produces missing audio.
  fail(error) {
    this.error = error instanceof Error ? error : new Error(String(error));
    this.onError?.(this.error);
    for (const wait of this.waits.values()) wait.reject(this.error);
    this.waits.clear();
    for (const wait of this.streamWaits.values()) wait.reject(this.error);
    this.streamWaits.clear();
    return this.error;
  }
  // Compile once on the page, then hand the required module to each thin
  // host. The PCM worker has its own instance and never shares mutable DSP
  // state with the audio thread. Asset preparation can allocate asynchronously.
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
  // SADIE is optional content, not an alternate implementation of the same
  // filter. When absent, headphones use the Rust IRCAM database. A present
  // but incomplete SADIE decoder is an error rather than a partly silent field.
  async loadDecoder(root) {
    const info = await fetch(`${root}/${DECODER}.json`).then((r) => r.ok ? r.json() : null);
    if (!info) return null;
    const ears = await Promise.all(["left", "right"].map(async (ear) => {
      const response = await fetch(`${root}/${DECODER}_${ear}.wav`);
      if (!response.ok) throw new Error(`audio: missing ${ear} headphone decoder`);
      // The codec worker performs explicit device-rate conversion and restores
      // the former impulse-area sourceRate/deviceRate scale. These WAV lanes are
      // harmonic filters; host codec channel interpretation must never reorder them.
      return loadAudioPcm(await response.arrayBuffer(), { rate: this.ctx.sampleRate, compensate: true });
    }));
    if (ears.some((ear) => ear.numberOfChannels !== 16)) throw new Error("audio: headphone decoder must have sixteen channels");
    // AudioBuffers here package two typed impulse lanes for upload only.
    // They are never connected to a browser ConvolverNode or BufferSourceNode;
    // Rust constructs sixteen independent two-ear convolvers from these samples.
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
  // The device limits the requested speaker width. Mono is a Rust fold of
  // the selected mix, so it does not ask the browser for a speaker downmix or
  // change the channel layout used by the game to place voices.
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
  // Reconfigure one persistent host so device/layout changes do not create
  // parallel graphs. Rust replaces layout-dependent DSP between callbacks,
  // while active source cursors and their timed gain commands survive.
  async build() {
    const config = this.config;
    this.mode = this.resolve(config.output); this.layout = LAYOUTS[this.mode]; this.channels = this.layout.channels;
    this.enhanced = config.quality !== "original";
    this.light = !this.enhanced && this.mode === "stereo";
    this.binaural = this.enhanced && this.mode === "headphones" && Boolean(this.decoder);
    this.hrtf = this.mode === "headphones" && !this.binaural;
    this.alsoft = !this.enhanced && this.mode !== "headphones" ? ALSOFT[this.mode] : null;
    this.lfe = this.enhanced && this.layout.lfe >= 0;
    const destination = this.ctx.destination;
    // The worklet reserves twelve output lanes. The destination truncates them
    // discretely to the selected device width; it must not fold rear, centre or
    // LFE samples into other channels after Rust has completed the mix.
    destination.channelCount = this.channels; destination.channelCountMode = "explicit"; destination.channelInterpretation = "discrete";
    this.configuration++;
    const id = ++this.nextId;
    const options = this.options(id);
    const ready = new Promise((resolve, reject) => this.waits.set(id, { resolve, reject }));
    if (!this.mixer) {
      this.mixer = new AudioWorkletNode(this.ctx, "audio-output", {
        numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [12], processorOptions: options,
      });
      this.mixer.port.onmessage = ({ data }) => {
        if (data.type === "ready" || data.type === "sync") { this.waits.get(data.id)?.resolve(); this.waits.delete(data.id); }
        else if (data.type === "primed") { this.streamWaits.get(data.token)?.resolve(); this.streamWaits.delete(data.token); }
        else if (data.type === "error") this.fail(new Error(data.message));
        else if (data.type === "control-buffer") this.onControlBuffer(data.data);
        else if (data.reduction !== undefined) this.onLimiter?.(data);
      };
      this.mixer.addEventListener("processorerror", () => this.fail(new Error("audio: the Rust render processor stopped; reload the game")));
      this.mixer.connect(destination);
      this.output = this.mixer;
    // The existing worklet owns its module and PCM storage. Only configuration
    // crosses this port again; passing another serialized module is unnecessary
    // and fails deserialization on the packaged app's repeated configuration path.
    } else {
      // The host retains its required module instance. Electron cannot
      // deserialize that module again through this MessagePort.
      delete options.dsp;
      this.mixer.port.postMessage({ type: "configure", options });
    }
    await ready;
    if (this.meterNode) { this.output.disconnect(this.meterNode); this.meterNode.disconnect(); this.meterNode = null; }
    if (this.meterOn) this.meter(true);
  }
  // These numbers are setup/control data, not a second implementation of
  // sample processing. Rust receives the selected law, speaker table, EQ and
  // impulse pairs and performs all interpolation, routing and filtering.
  options(id) {
    const layout = this.layout;
    const clarity = this.enhanced && this.config.clarity !== false;
    return { id, dsp: this.dsp, controlLayout: CONTROL_LAYOUT, channels: this.channels, enhanced: this.enhanced, headphones: this.mode === "headphones", binaural: this.binaural, mono: Boolean(this.config.mono),
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
  // Cold content upload barrier. The output port acknowledges this only
  // after earlier sample messages have finished preparing native storage.
  sync() {
    const id = ++this.nextId;
    const ready = new Promise((resolve,reject)=>this.waits.set(id,{resolve,reject}));
    this.mixer.port.postMessage({type:"sync",id}); return ready;
  }
  setVolume(value) { this.masterVolume = value; this.mixer?.port.postMessage({ type: "volume", value, time: this.ctx.currentTime }); }
  // Batch controls on the page and transfer their storage once. f64 preserves
  // context times and command values until Rust applies the original float
  // parameter boundaries. No per-render callback arrays are built on this thread.
  flush() {
    if (!this.mixer || this.error) return;
    if (this.batch.length) { const data = Float64Array.from(this.batch); this.mixer.port.postMessage({ type: "params", data }, [data.buffer]); this.batch.length = 0; }
    if (this.commands.length) { const data = Float64Array.from(this.commands); this.mixer.port.postMessage({ type: "voices", data }, [data.buffer]); this.commands.length = 0; }
  }
  // A microtask flush preserves command ordering within one page turn while
  // allowing a start, placement and gain update to cross as a single batch.
  // Scheduling remains in AudioContext time, independent of page timer jitter.
  command(...values) {
    this.commands.push(...values);
    if (!this.flushQueued) { this.flushQueued = true; queueMicrotask(() => { this.flushQueued = false; this.flush(); }); }
  }
  // Retained effects cross as channel-major PCM. Transfer a staging copy so
  // the page's reusable AudioBuffer stays intact; Rust owns the uploaded copy
  // until the matching sample release message arrives between render calls.
  uploadSample(buffer, id) {
    const data = new Float32Array(buffer.length * buffer.numberOfChannels);
    for (let c = 0; c < buffer.numberOfChannels; c++) data.set(buffer.getChannelData(c), c * buffer.length);
    this.mixer.port.postMessage({ type: "sample", id, frames: buffer.length, channels: buffer.numberOfChannels, rate: buffer.sampleRate, data }, [data.buffer]);
  }
  // An effect asset's stable identity is independent of its many voice slots.
  // Multiple sources can read the same Rust PCM without duplicating the asset
  // or sharing their playback-rate, filter or gain state.
  engineSample(buffer) {
    let id = this.sampleIds.get(buffer);
    if (id === undefined) { id = this.nextSample++; this.sampleIds.set(buffer, id); }
    if (!this.uploaded.has(id)) {
      this.uploaded.add(id);
      this.uploadSample(buffer, id);
    }
    return id;
  }
  // The speaker panel's waveform also comes from Rust. A separate page-side
  // instance can allocate a short signal without touching the worklet's memory
  // or asking its render callback to synthesize/upload a buffer.
  async testBuffer(tone, seconds) {
    try { this.signalDsp ||= new WebAssembly.Instance(this.dsp).exports; }
    catch (error) { throw this.fail(new Error(`audio: Rust test signals could not start: ${error.message}`)); }
    const x = this.signalDsp, frames = Math.round(seconds * this.ctx.sampleRate);
    const at = x.signal_pcm(this.ctx.sampleRate, frames, tone ? 1 : 0);
    const buffer = this.ctx.createBuffer(1, frames, this.ctx.sampleRate);
    buffer.copyToChannel(new Float32Array(x.memory.buffer, at, frames), 0);
    return buffer;
  }
  // Metering taps the completed output without returning samples to the mix.
  // LFE has zero loudness weight; surround lanes use BS.1770's increased weight.
  // Only the Rust meter filters and integrates the signal.
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
