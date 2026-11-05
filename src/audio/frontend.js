import { loadAudioDsp } from "./wasm-dsp.js";
import { prepareAudioPcm } from "./pcm.js";
import { attachAudioStream } from "./stream-pcm.js";
import { VOICE, FRONT } from "./commands.js";

// The front end supplies only assets and timed commands. The Rust phase
// owns playback, resampling, summing and fades, including track endings.
// The shell calls this page-thread controller; the worklet owns all PCM
// cursors, resampling, fades and summation. Handles below represent playback
// requests and completion callbacks, never browser DSP nodes.
export class FrontendAudio {
  constructor(context) {
    this.ctx = context;
    this.voices = new Map();
    this.serial = 0;
    this.waits = new Map();
    this.streamWaits = new Map();
    this.isReady = false;
    this.ready = this.build();
  }
  // Wait for the Rust processor's ready message before permitting playback.
  // Loading/instantiation failures reject readiness and become a visible player
  // error instead of silently replacing the required output implementation.
  async build() {
    const [dsp] = await Promise.all([loadAudioDsp(), prepareAudioPcm(), this.ctx.audioWorklet.addModule(new URL("./worklets/frontend.js?no-inline", import.meta.url))]);
    this.node = new AudioWorkletNode(this.ctx, "audio-frontend", {
      numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2], channelCount: 2, channelCountMode: "explicit", channelInterpretation: "discrete", processorOptions: { dsp },
    });
    await new Promise((resolve, reject) => {
      this.node.addEventListener("processorerror", () => { const error = new Error("audio: required Rust front-end processor failed; reload the game"); reject(error); this.fail(error); });
      this.node.port.onmessage = ({ data: m }) => {
        if (m.type === "ready") resolve();
        else if (m.type === "error") { const error = new Error(`audio: ${m.message}`); reject(error); this.fail(error); }
        else if (m.type === "sync") { this.waits.get(m.id)?.(); this.waits.delete(m.id); }
        else if (m.type === "primed") { this.streamWaits.get(m.token)?.resolve(); this.streamWaits.delete(m.token); }
        else if (m.type === "ended") {
          for (let i = 0; i < m.count; i++) {
            const slot = m.voices[FRONT.END_STRIDE * i + FRONT.END_SLOT], voice = this.voices.get(slot);
            // A slot can be reused before its previous ended message reaches the page.
            // Compare generations before closing the producer or releasing Rust storage,
            // so a delayed event cannot stop the next track occupying the same slot.
            if (voice?.generation !== m.voices[FRONT.END_STRIDE * i + FRONT.END_GENERATION]) continue;
            voice.asset?.close();
            this.voices.delete(slot);
            this.node.port.postMessage({ type: "release", slot });
            voice.onended?.();
          }
        }
      };
    });
    this.node.connect(this.ctx.destination);
    this.isReady = true;
    return this;
  }
  // Suspend the context and surface an actionable error. Reject pending
  // stream priming promises too; a shell waiting for music must not remain
  // stuck on a processor or decoder that can no longer deliver samples.
  fail(error) {
    if (!error.message.startsWith("audio:")) error = new Error(`audio: the required Rust front-end output failed (${error.message}); reload the game`);
    this.isReady = false;
    for (const wait of this.streamWaits.values()) wait.reject(error);
    this.streamWaits.clear();
    this.error = error;
    this.ctx.suspend?.();
    if (typeof document === "undefined" || document.querySelector("#audio-error")) return;
    const message = document.createElement("div"); message.id = "audio-error"; message.setAttribute("role", "alert");
    message.textContent = error.message;
    Object.assign(message.style, { position: "fixed", inset: "0 0 auto", zIndex: "2147483647", padding: "1em", background: "#400", color: "white" });
    document.body.append(message);
  }
  command(...data) { this.node.port.postMessage({ type: "commands", data }); }
  volumes(master = 1, music = 0.6, fx = 0.3) { this.command(VOICE.FRONT_VOLUME, master, music, fx); }
  dip(value, seconds = 0, at = this.ctx.currentTime) { this.command(VOICE.FRONT_DIP, value, seconds, at); }
  // Short retained sounds upload one planar copy before their start command.
  // Transfer staging ownership without detaching the caller's reusable PCM;
  // Rust keeps the copy until completion is acknowledged on the page.
  play(buffer, { gain = 1, pitch = 1, when = this.ctx.currentTime, offset = 0, loop = false, bus = 0 } = {}) {
    if (!this.isReady) throw new Error("audio: Rust front-end processor is not ready");
    let slot = 0; while (this.voices.has(slot) && slot < FRONT.SLOTS) slot++;
    if (slot === FRONT.SLOTS) throw new Error("audio: front-end voice pool is full");
    const channels = buffer.numberOfChannels, frames = buffer.length;
    if (!frames || channels < 1 || channels > 12) throw new Error("audio: invalid front-end PCM asset");
    const data = new Float32Array(frames * channels);
    for (let c = 0; c < channels; c++) data.set(buffer.getChannelData(c), c * frames);
    this.node.port.postMessage({ type: "sample", slot, frames, channels, rate: buffer.sampleRate, data }, [data.buffer]);
    const generation = ++this.serial;
    const voice = {
      generation, onended: null,
      stop: (at = this.ctx.currentTime) => { if (this.voices.get(slot) === voice) this.command(VOICE.FRONT_STOP, slot, at); },
      fade: (start, end) => { if (this.voices.get(slot) === voice) this.command(VOICE.FRONT_FADE, slot, start, end); },
    };
    this.voices.set(slot, voice);
    this.command(VOICE.FRONT_START, slot, generation, when, offset, pitch, Number(loop), gain, bus);
    return voice;
  }
  // Long music attaches its decoder directly to the worklet. Reserve a page
  // slot before awaiting preroll, then schedule in the current context clock.
  // Stopping or completion closes the producer instead of retaining a track.
  async playStream(asset, { gain = 1, pitch = 1, bus = 1 } = {}) {
    if (!this.isReady) throw Error("audio: Rust front-end processor is not ready");
    let slot = 0; while (this.voices.has(slot) && slot < FRONT.SLOTS) slot++;
    if (slot === FRONT.SLOTS) { asset.close(); throw Error("audio: front-end voice pool is full"); }
    const generation = ++this.serial;
    const voice = { generation, asset, onended: null,
      stop: (at = this.ctx.currentTime) => { if (this.voices.get(slot) === voice) this.command(VOICE.FRONT_STOP, slot, at); },
      fade: (start, end) => { if (this.voices.get(slot) === voice) this.command(VOICE.FRONT_FADE, slot, start, end); },
    };
    this.voices.set(slot, voice);
    try {
      await attachAudioStream(asset, this.node, slot, this.streamWaits, (error) => this.fail(error));
      this.command(VOICE.FRONT_START, slot, generation, this.ctx.currentTime, 0, pitch, 0, gain, bus);
      return voice;
    } catch (error) { asset.close(); this.voices.delete(slot); this.node.port.postMessage({ type: "release", slot }); throw error; }
  }
  // This is a message-order fence for page commands, not a rendered-frame
  // or audio-device fence. Rust has applied earlier messages when it replies.
  sync() { return new Promise((resolve) => { const id = ++this.serial; this.waits.set(id, resolve); this.node.port.postMessage({ type: "sync", id }); }); }
}
