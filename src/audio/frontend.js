import { loadAudioDsp } from "./wasm-dsp.js";
import { prepareAudioPcm } from "./pcm.js";
import { VOICE, FRONT } from "./commands.js";

// The front end supplies only assets and timed commands. The Rust phase
// owns playback, resampling, summing and fades, including track endings.
export class FrontendAudio {
  constructor(context) {
    this.ctx = context;
    this.voices = new Map();
    this.serial = 0;
    this.waits = new Map();
    this.isReady = false;
    this.ready = this.build();
  }
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
        else if (m.type === "ended") {
          for (let i = 0; i < m.count; i++) {
            const slot = m.voices[FRONT.END_STRIDE * i + FRONT.END_SLOT], voice = this.voices.get(slot);
            if (voice?.generation !== m.voices[FRONT.END_STRIDE * i + FRONT.END_GENERATION]) continue;
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
  fail(error) {
    this.isReady = false;
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
  sync() { return new Promise((resolve) => { const id = ++this.serial; this.waits.set(id, resolve); this.node.port.postMessage({ type: "sync", id }); }); }
}
