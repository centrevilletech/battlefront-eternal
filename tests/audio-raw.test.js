import test from "node:test";
import assert from "node:assert/strict";
import { AUDIO_DSP_WASM } from "../src/audio-dsp-wasm.js";

// Exercise the real worklet boundary: samples arrive after live rendering
// has begun, so an upload can detach every saved view of the module heap.
test("HRTF voice outputs survive an upload after their first block", async () => {
  const previous = { processor: globalThis.AudioWorkletProcessor, register: globalThis.registerProcessor, rate: globalThis.sampleRate, frame: globalThis.currentFrame };
  let Mixer;
  globalThis.AudioWorkletProcessor = class { constructor() { this.port = { postMessage() {} }; } };
  globalThis.registerProcessor = (name, proc) => { if (name === "spatial-mixer") Mixer = proc; };
  globalThis.sampleRate = 48000;
  globalThis.currentFrame = 0;
  try {
    await import("../src/audio/worklets/spatial-mixer.js");
    const dsp = new WebAssembly.Module(Buffer.from(AUDIO_DSP_WASM, "base64"));
    const mixer = new Mixer({ numberOfInputs: 0, processorOptions: { slots: 4, channels: 2, raw: true, dsp, engine: { q: -3.0103, filtered: false } } });
    const outputs = Array.from({ length: 5 }, () => [new Float32Array(128)]);
    assert.equal(mixer.process([], outputs), true);
    const old = mixer.dsp.x.memory.buffer;
    mixer.port.onmessage({ data: { type: "sample", id: 0, rate: 48000, data: new Float32Array(1000000).fill(0.25) } });
    assert.notEqual(mixer.dsp.x.memory.buffer, old);
    mixer.port.onmessage({ data: { type: "voices", data: Float64Array.of(1, 0, 0, 0, 1, 1, 24000, 128 / 48000, 2, 0, 0.5, 128 / 48000) } });
    globalThis.currentFrame = 128;
    assert.equal(mixer.process([], outputs), true);
    assert.ok(outputs[0][0].every((x) => x === 0.125));
    assert.ok(outputs[1][0].every((x) => x === 0));
    mixer.port.onmessage({ data: { type: "voices", data: Float64Array.of(8, 0) } });
    globalThis.currentFrame = 256;
    mixer.process([], outputs);
    assert.ok(outputs[0][0].every((x) => x === 0));
  } finally {
    globalThis.AudioWorkletProcessor = previous.processor;
    globalThis.registerProcessor = previous.register;
    globalThis.sampleRate = previous.rate;
    globalThis.currentFrame = previous.frame;
  }
});
