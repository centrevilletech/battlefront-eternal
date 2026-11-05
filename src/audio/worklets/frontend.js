// Self-contained host for the Rust menu/loading render phase. PCM upload
// and commands arrive between blocks; process only renders and copies.
class FrontendOutput extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const module = options.processorOptions.dsp;
    if (!(module instanceof WebAssembly.Module)) throw new Error("audio: required Rust front-end module is missing");
    this.x = new WebAssembly.Instance(module).exports;
    this.h = this.x.front_new(sampleRate);
    this.refresh();
    this.report = { type: "ended", count: 0, voices: this.ended };
    this.port.onmessage = ({ data: m }) => {
      try {
        const x = this.x, h = this.h;
        if (m.type === "sample") {
          const at = x.front_sample(h, m.slot, m.frames, m.channels, m.rate);
          new Float32Array(x.memory.buffer, at, m.data.length).set(m.data);
        } else if (m.type === "release") x.front_release(h, m.slot);
        else if (m.type === "commands") {
          const at = x.front_commands_ptr(h, m.data.length);
          new Float64Array(x.memory.buffer, at, m.data.length).set(m.data);
          x.front_commands(h);
        } else if (m.type === "sync") this.port.postMessage({ type: "sync", id: m.id });
        if (!this.left.byteLength) this.refresh();
      } catch (error) { this.port.postMessage({ type: "error", message: error.message }); throw error; }
    };
    this.port.postMessage({ type: "ready" });
  }
  refresh() {
    const x = this.x, h = this.h, at = x.front_out(h);
    this.left = new Float32Array(x.memory.buffer, at, 128);
    this.right = new Float32Array(x.memory.buffer, at + 512, 128);
    this.ended = new Uint32Array(x.memory.buffer, x.front_ended(h), x.front_ended_len());
    if (this.report) this.report.voices = this.ended;
  }
  process(inputs, outputs) {
    const count = this.x.front_process(this.h, currentFrame);
    outputs[0][0].set(this.left); outputs[0][1].set(this.right);
    if (count) { this.report.count = count; this.port.postMessage(this.report); }
    return true;
  }
}
registerProcessor("audio-frontend", FrontendOutput);
