import test from "node:test";
import assert from "node:assert/strict";
import { installAudioHost } from "./audio-offline-host.js";
import { Renderer } from "../src/audio/spatial.js";
import { prepareAudioPcm, normalizePcm16 } from "../src/audio/pcm.js";

installAudioHost();
function context(channels = 2) { return new OfflineAudioContext(channels, 48000, 48000); }
const config = { quality: "original", output: "stereo", dynamics: "normal", mono: false };

test("a missing Rust module is an error in the output host", async () => {
  const r = new Renderer(context()); await r.worklets();
  assert.throws(() => new AudioWorkletNode(r.ctx, "audio-output", { processorOptions: { dsp: null } }), /Rust DSP module is missing/);
});
test("a module load failure reaches the player error callback", async () => {
  const ctx = context(); ctx.audioWorklet.addModule = async () => { throw new Error("test module unavailable"); };
  const r = new Renderer(ctx); let reported; r.onError = (error) => { reported = error; };
  await assert.rejects(r.configure(config), /test module unavailable/);
  assert.equal(r.error, reported); assert.equal(r.mixer, undefined);
});
test("Classic stereo has one required Rust source-to-output host", async () => {
  const r = new Renderer(context()); await r.configure(config);
  assert.deepEqual(WebAssembly.Module.imports(r.dsp), [], "no host DSP or math callbacks");
  assert.equal(r.ctx.nodes.length, 1); assert.equal(r.mixer.options.numberOfInputs, 0);
  assert.equal(r.mixer.options.outputChannelCount[0], 12); assert.equal(r.light, true);
  const b = r.ctx.createBuffer(1, 1000, 48000); b.getChannelData(0).fill(0.25);
  const out = r.voice({ mode3D: 1 }); out.engineStart(b, 0, true, 1); out.level = 0.5;
  out.update({ x: -1, y: 0, z: 0 }, 1, 0); r.flush(); await Promise.resolve(); await Promise.resolve();
  const rendered = await r.ctx.startRendering();
  assert.ok(rendered.getChannelData(0).some((v) => v > 0.05));
  assert.ok(rendered.getChannelData(1).every((v) => v === 0));
});
test("sample uploads after rendering starts refresh detached heap views", async () => {
  const r = new Renderer(context()); await r.configure(config);
  const node = r.mixer, x = node.processor.x;
  globalThis.currentFrame = 0; node.processor.process([], [node.output]); const before = x.memory.buffer;
  const b = r.ctx.createBuffer(1, 1000000, 48000); b.getChannelData(0).fill(0.25);
  const out = r.voice({ mode3D: 1 }); out.engineStart(b, 0, true, 1); out.level = 0.5; out.update({ x: -1, y: 0, z: 0 }, 1, 0); r.flush();
  await Promise.resolve(); await Promise.resolve();
  assert.notEqual(x.memory.buffer, before);
  globalThis.currentFrame = 128; node.processor.process([], [node.output]);
  assert.ok(node.output[0].some((v) => v > 0.05)); assert.ok(node.output[1].every((v) => v === 0));
});
test("layout changes retain the host, decoded samples and scheduled sources", async () => {
  const r = new Renderer(context(8)); await r.configure(config);
  const first = r.mixer, b = r.ctx.createBuffer(2, 48000, 48000); b.getChannelData(0).fill(0.1); b.getChannelData(1).fill(0.1);
  const lane = r.stream(2); lane.start(b, 0, 1, true); r.flush(); await Promise.resolve();
  for (const output of ["5.1", "7.1", "stereo"]) {
    await r.configure({ ...config, output });
    assert.equal(r.mixer, first);
    // The rebuilt stabilizer and limiter have their own startup delay.
    for (let block = 0; block < 5; block++) { globalThis.currentFrame = 1024 + block * 128; first.processor.process([], [first.output]); }
    assert.ok(first.output.slice(0, r.channels).some((c) => c.some((v) => v !== 0)));
  }
});
test("stream segment PCM uses bounded reusable slots", async () => {
  const r = new Renderer(context()); await r.configure(config);
  const buffer = r.ctx.createBuffer(1, 128, 48000); buffer.getChannelData(0).fill(0.1);
  for (let i = 0; i < 20; i++) { const lane = r.stream(1); lane.start(buffer); r.flush(); await Promise.resolve(); lane.dispose(); await Promise.resolve(); }
  assert.equal(r.nextSample, 3); assert.equal(r.freeStreamSamples.length, 3);
});
test("Rust bank preparation preserves signed PCM", async () => {
  await prepareAudioPcm();
  const source = Int16Array.of(-32768, -16384, -1, 0, 1, 16384, 32767), output = new Float32Array(source.length);
  normalizePcm16(source, output);
  assert.deepEqual(output, Float32Array.from(source, (v) => v / 32768));

});

test("speaker test reaches its named channel after the muted mono game mix", async () => {
  const r = new Renderer(context(6)); r.masterVolume = 0;
  await r.configure({ ...config, quality: "enhanced", output: "5.1", mono: true });
  const b = r.ctx.createBuffer(1, 128, 48000); b.getChannelData(0).fill(0.25);
  const lane = r.stream(1, 3 + 3); lane.start(b, 0); r.flush();
  await Promise.resolve(); await Promise.resolve();
  const out = await r.ctx.startRendering();
  for (let c = 0; c < 6; c++) {
    if (c === 3) assert.ok(out.getChannelData(c).subarray(0, 128).every((v) => v === 0.25));
    else assert.ok(out.getChannelData(c).every((v) => v === 0));
  }
});

test("a narrower queued segment preserves the first segment's surround lanes", async () => {
  const r = new Renderer(context(6));
  await r.configure({ ...config, quality: "enhanced", output: "5.1", dynamics: "home", clarity: false });
  const first = r.ctx.createBuffer(6, 48000, 48000); first.getChannelData(3).fill(0.25);
  const next = r.ctx.createBuffer(2, 48000, 48000); next.getChannelData(0).fill(0.1);
  const lane = r.stream(2, "music"); lane.start(first, 0, 1, true); lane.start(next, 0.03, 1, true);
  assert.equal(lane.channels, 6); r.flush(); await Promise.resolve(); await Promise.resolve();
  const out = await r.ctx.startRendering();
  assert.ok(out.getChannelData(3).subarray(512, 8192).every((v) => v > 0.05));
});
