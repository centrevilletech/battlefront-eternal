import test from "node:test";
import assert from "node:assert/strict";
import { ALSOFT, alsoftGains, alsoftStereo } from "../src/audio/layouts.js";
import { LIGHT_LEVELS, ORIGINAL_LEVEL, Renderer, softClip } from "../src/audio/spatial.js";
import { AudioEngine } from "../src/audio/engine.js";
import { voicesRust } from "../src/audio/wasm-dsp.js";
import { SoundData } from "../src/audio/data.js";

// The light mix (Classic on stereo speakers) against a stand-in for the
// browser's audio graph that records nodes and connections; the rendered
// sound is checked in Chromium (tests/audio-checks.js). These look at the
// browser's own nodes a voice, so the page is as with ?voices=native; the
// voice engine's graph has a test of its own (voicesRust set).
globalThis.location = { search: "?voices=native" };

let ids = 0;
class Param {
  constructor(value) {
    this.value = value;
    this.events = [];
    this.automationRate = "a-rate";
  }
  setValueAtTime(value, time) {
    this.events.push(["set", value, time]);
    this.value = value;
    return this;
  }
  setTargetAtTime(value, time, tau) {
    this.events.push(["target", value, time, tau]);
    return this;
  }
  cancelScheduledValues(time) {
    this.events.push(["cancel", time]);
    return this;
  }
  linearRampToValueAtTime(value, time) {
    this.events.push(["ramp", value, time]);
    return this;
  }
}
class Node {
  constructor(ctx, kind, options = {}) {
    this.ctx = ctx;
    this.kind = kind;
    this.options = options;
    this.id = ids++;
    this.channelCount = options.channelCount ?? 2;
    this.channelCountMode = options.channelCountMode ?? "max";
    this.channelInterpretation = options.channelInterpretation ?? "speakers";
    ctx.nodes.push(this);
  }
  connect(to, output = 0, input = 0) {
    this.ctx.edges.push({ from: this, output, to, input });
    return to;
  }
  disconnect() {
    this.ctx.edges = this.ctx.edges.filter((e) => e.from !== this);
  }
  // What tells two nodes apart in a graph's description.
  detail() {
    const params = Object.entries(this)
      .filter(([, v]) => v instanceof Param)
      .map(([k, v]) => `${k}=${+v.value.toFixed(6)}`);
    const own = this.kind === "AudioWorkletNode" ? [this.name, JSON.stringify(this.options)] : this.kind === "WaveShaperNode" ? [this.curve?.length] : [this.type ?? "", this.panningModel ?? ""];
    return [...own, ...params, this.channelCount, this.channelCountMode, this.channelInterpretation].join(",");
  }
}
class FakeContext {
  constructor({ channels = 2 } = {}) {
    this.nodes = [];
    this.edges = [];
    this.sampleRate = 48000;
    this.currentTime = 1;
    this.state = "running";
    this.destination = new Node(this, "destination");
    this.destination.maxChannelCount = channels;
    this.modules = [];
    this.audioWorklet = { addModule: (url) => (this.modules.push(String(url).split("/").pop()), Promise.resolve()) };
  }
  createGain() {
    const n = new Node(this, "GainNode");
    n.gain = new Param(1);
    return n;
  }
  createBiquadFilter() {
    const n = new Node(this, "BiquadFilterNode");
    n.type = "lowpass";
    n.frequency = new Param(350);
    n.Q = new Param(1);
    n.gain = new Param(0);
    return n;
  }
  createStereoPanner() {
    const n = new Node(this, "StereoPannerNode");
    n.pan = new Param(0);
    return n;
  }
  createDelay() {
    const n = new Node(this, "DelayNode");
    n.delayTime = new Param(0);
    return n;
  }
  createChannelMerger(inputs = 6) {
    return Object.assign(new Node(this, "ChannelMergerNode"), { inputs });
  }
  createChannelSplitter(outputs = 6) {
    return Object.assign(new Node(this, "ChannelSplitterNode"), { outputs });
  }
  createBuffer(channels, length, sampleRate) {
    const data = Array.from({ length: channels }, () => new Float32Array(length));
    return { numberOfChannels: channels, length, sampleRate, duration: length / sampleRate, getChannelData: (c) => data[c] };
  }
  createBufferSource() {
    const n = new Node(this, "AudioBufferSourceNode");
    n.playbackRate = new Param(1);
    n.start = () => {};
    n.stop = () => {};
    return n;
  }
}
globalThis.AudioWorkletNode = class extends Node {
  constructor(ctx, name, options = {}) {
    super(ctx, "AudioWorkletNode", options);
    this.name = name;
    this.port = { postMessage: () => {} };
  }
};
globalThis.ConvolverNode = class extends Node {
  constructor(ctx, options = {}) {
    super(ctx, "ConvolverNode", options);
  }
};
globalThis.PannerNode = class extends Node {
  constructor(ctx, options = {}) {
    super(ctx, "PannerNode", options);
    this.panningModel = options.panningModel;
    for (const k of ["positionX", "positionY", "positionZ"]) this[k] = new Param(0);
  }
};
globalThis.WaveShaperNode = class extends Node {
  constructor(ctx, options = {}) {
    super(ctx, "WaveShaperNode", options);
    this.curve = options.curve;
  }
};

// The live graph as lines, from the output back: each node by what it is,
// its inputs in order (so two graphs built alike read alike, whenever their
// nodes were made).
function describe(ctx) {
  const sig = (n) => `${n.kind}(${n.detail()})`;
  const label = new Map([[ctx.destination, 0]]);
  const queue = [ctx.destination];
  const lines = [];
  while (queue.length) {
    const node = queue.shift();
    const ins = ctx.edges
      .filter((e) => e.to === node)
      .sort((a, b) => a.input - b.input || a.output - b.output || sig(a.from).localeCompare(sig(b.from)) || a.from.id - b.from.id);
    for (const e of ins) {
      if (!label.has(e.from)) {
        label.set(e.from, label.size);
        queue.push(e.from);
      }
      lines.push(`${label.get(e.from)} ${sig(e.from)}[${e.output}] -> ${label.get(node)}[${e.input}]`);
    }
  }
  return { lines, nodes: [...label.keys()] };
}
const kinds = (ctx) => describe(ctx).nodes.map((n) => n.kind);
const into = (ctx, node) => ctx.edges.filter((e) => e.from === node);

async function renderer(config, options) {
  const ctx = new FakeContext(options);
  const r = new Renderer(ctx);
  await r.load();
  r.configure(config);
  return { ctx, r };
}

// A playing voice's own nodes (as the engine makes them) through r.voice.
function voice(ctx, r, mode3D = 1) {
  const source = ctx.createBufferSource();
  const gain = ctx.createGain();
  source.connect(gain);
  const out = r.voice({ mode3D });
  out.attach(gain);
  return { source, gain, out };
}

test("OpenAL Soft's stereo law without allocating, as alsoftGains gives it", () => {
  const out = [0, 0];
  let worst = 0;
  for (let az = -180; az <= 180; az += 7.5) {
    for (const el of [-60, 0, 25, 89]) {
      const a = (az * Math.PI) / 180;
      const e = (el * Math.PI) / 180;
      const dir = [-Math.sin(a) * Math.cos(e), Math.sin(e), -Math.cos(a) * Math.cos(e)];
      alsoftStereo(...dir, out);
      const want = alsoftGains("stereo", ...dir);
      worst = Math.max(worst, Math.abs(out[0] - want[0]), Math.abs(out[1] - want[1]));
      // Not unit length: the same.
      alsoftStereo(...dir.map((v) => v * 3), out);
      worst = Math.max(worst, Math.abs(out[0] - want[0]), Math.abs(out[1] - want[1]));
    }
  }
  assert.ok(worst < 1e-12, `worst ${worst}`);
  // Ahead 0.5957 a side; no direction at all is ahead; hard right.
  assert.deepEqual(alsoftStereo(0, 0, -1, out).map((g) => +g.toFixed(4) || 0), [0.5957, 0.5957]);
  assert.deepEqual(alsoftStereo(0, 0, 0, out).map((g) => +g.toFixed(4) || 0), [0.5957, 0.5957]);
  assert.deepEqual(alsoftStereo(1, 0, 0, out).map((g) => +g.toFixed(4) || 0), [0, 1]);
  assert.equal(ALSOFT.stereo.front, 1.5);
});

test("the soft clip leaves everything under -1 dBFS alone and never passes full scale", () => {
  for (const x of [0, 0.1, -0.5, 0.89, -0.891]) assert.equal(softClip(x), x);
  let last = -Infinity;
  for (let x = -2; x <= 2; x += 0.001) {
    const y = softClip(x);
    assert.ok(Math.abs(y) <= 1, `${x} -> ${y}`);
    assert.ok(y >= last, "rising");
    last = y;
  }
  assert.ok(softClip(1) > 0.97 && softClip(1) < 0.98, "full scale in: 0.97 out");
  assert.ok(softClip(2) > 0.9999, "6 dB over: full scale");
});

test("Classic on stereo is the light mix: the browser's own nodes, two speaker gains a voice", async () => {
  const { ctx, r } = await renderer({ quality: "original", output: "auto", dynamics: "normal", mono: false, clarity: true });
  assert.equal(r.mode, "stereo", "a stereo device");
  assert.equal(r.light, true);
  assert.equal(r.enhanced, false);
  for (const gone of ["mixer", "reverb", "reverbIn", "limiter", "lfe", "hrtfOut"]) assert.equal(r[gone], null, gone);
  const a = voice(ctx, r);
  const b = voice(ctx, r, 0);
  a.out.update({ x: 1, y: 0, z: 0 }, 12, 0, { send: 1, cutoff: 3000, lfe: 1 });
  // A 2D voice: the engine gives the original no pan (centred).
  b.out.update(null, 0, 0, { send: 1 });
  r.flush();
  const used = kinds(ctx);
  for (const heavy of ["AudioWorkletNode", "ConvolverNode", "PannerNode", "BiquadFilterNode", "DelayNode", "StereoPannerNode"]) {
    assert.ok(!used.includes(heavy), `no ${heavy}`);
  }
  // Each voice's gain feeds two gains, one into each side of the pair.
  for (const [v, want] of [[a, [0, 1]], [b, [0.5957, 0.5957]]]) {
    const sides = into(ctx, v.gain).map((e) => e.to);
    assert.equal(sides.length, 2);
    sides.forEach((side, c) => {
      assert.equal(side.kind, "GainNode");
      assert.deepEqual(into(ctx, side).map((e) => [e.to, e.input]), [[r.pair, c]]);
      assert.ok(Math.abs(side.gain.value - want[c]) < 1e-4, `side ${c}: ${side.gain.value}`);
    });
  }
  // The master: the pair into the mix at the original's stereo level, the
  // profile's level halved into the soft clip, the volume, the output.
  const chain = [];
  for (let n = ctx.destination; n !== r.pair; ) {
    const ins = ctx.edges.filter((e) => e.to === n);
    assert.equal(ins.length, 1, `${n.kind} has one input`);
    n = ins[0].from;
    chain.push(n.kind);
    if (n === r.mix) assert.ok(Math.abs(n.gain.value - 10 ** (ORIGINAL_LEVEL.stereo / 20)) < 1e-9);
    if (n.kind === "GainNode" && n !== r.mix && n !== r.volume) assert.ok(Math.abs(n.gain.value - 10 ** (LIGHT_LEVELS.normal / 20) / 2) < 1e-9);
  }
  assert.deepEqual(chain, ["GainNode", "WaveShaperNode", "GainNode", "GainNode", "ChannelMergerNode"]);
  assert.equal(r.preDynamics, r.mix);
  // The next frame eases toward the new direction; a change too small to
  // hear sends nothing.
  const [left] = into(ctx, a.gain).map((e) => e.to);
  a.out.update({ x: 0, y: 0, z: -1 }, 12, 0, {});
  assert.deepEqual(left.gain.events.at(-1).slice(0, 2), ["target", alsoftStereo(0, 0, -1, [0, 0])[0]]);
  const sent = left.gain.events.length;
  a.out.update({ x: 1e-4, y: 0, z: -1 }, 12, 0, {});
  assert.equal(left.gain.events.length, sent);
});

test("the light mix plays streams at OpenAL Soft's angles, in as few nodes as that takes", async () => {
  const { ctx, r } = await renderer({ quality: "original", output: "stereo", dynamics: "home", mono: false });
  const bed = (channels, kind) => {
    const out = ctx.createGain();
    r.bed(channels, out, kind);
    return out;
  };
  // Music: its pair lands hard left and right, so straight into the mix.
  assert.deepEqual(into(ctx, bed(2, "music")).map((e) => e.to), [r.mix]);
  // Voice-over: ahead, one gain into both speakers alike.
  const [both] = into(ctx, bed(1, "voice")).map((e) => e.to);
  assert.deepEqual([both.kind, +both.gain.value.toFixed(4), both.channelCount, both.channelCountMode, both.channelInterpretation], ["GainNode", 0.5957, 2, "explicit", "speakers"]);
  assert.deepEqual(into(ctx, both).map((e) => e.to), [r.mix]);
  // A quad bed: the front pair straight to its speakers, the rear pair
  // mostly on its own side.
  const [split] = into(ctx, bed(4, "ambience")).map((e) => e.to);
  assert.equal(split.kind, "ChannelSplitterNode");
  const routes = into(ctx, split).map((e) => (e.to.kind === "GainNode" ? [e.output, into(ctx, e.to)[0].input, +e.to.gain.value.toFixed(4)] : [e.output, e.input, 1])).sort();
  assert.deepEqual(routes, [[0, 0, 1], [1, 1, 1], [2, 0, 0.6672], [2, 1, 0.1672], [3, 0, 0.1672], [3, 1, 0.6672]]);
  assert.ok(!kinds(ctx).includes("AudioWorkletNode"));
});

test("Enhanced builds the same graph before and after a trip through the light mix", async () => {
  for (const output of ["stereo", "5.1", "headphones"]) {
    const config = { quality: "enhanced", output, dynamics: "normal", mono: false, clarity: true };
    const { ctx, r } = await renderer(config, { channels: 8 });
    const a = voice(ctx, r);
    const b = voice(ctx, r, 0);
    const frame = () => {
      a.out.update({ x: 0.6, y: 0, z: -0.8 }, 30, 0, { send: 0.4, cutoff: 9000, lfe: 1, spread: 0 });
      b.out.update(null, 0, 0, { send: 0 });
      r.flush();
    };
    const bed = ctx.createGain();
    r.bed(2, bed, "music");
    frame();
    // Rebuilt as any audio setting rebuilds it (a voice's air filter starts
    // where it was), and then after the light mix: the same.
    r.configure(config);
    frame();
    const before = describe(ctx).lines;
    r.configure({ quality: "original", output: "stereo" });
    frame();
    assert.equal(r.light, true, output);
    r.configure(config);
    frame();
    assert.equal(r.light, false);
    assert.deepEqual(describe(ctx).lines, before, `${output}: the same graph`);
    // And it is the enhanced stage: the mixer and its law, the room, the
    // clarity stage, the limiter; a 3D voice through its air filter.
    const worklets = describe(ctx).nodes.filter((n) => n.kind === "AudioWorkletNode").map((n) => n.name);
    for (const name of ["spatial-mixer", "fdn-reverb", "clarity", "true-peak-limiter"]) assert.ok(worklets.includes(name), `${output}: ${name}`);
    assert.equal(into(ctx, a.gain).map((e) => e.to.kind).sort().join(), "BiquadFilterNode,GainNode", `${output}: air filter and send`);
  }
});

test("a battle that starts in the light mix loads no worklet; another graph loads them and builds once they are in", async () => {
  const ctx = new FakeContext({ channels: 8 });
  const r = new Renderer(ctx);
  const config = { quality: "original", output: "stereo", dynamics: "normal" };
  await r.load(null, config);
  assert.equal(r.configure(config), null, "built at once");
  assert.deepEqual(ctx.modules, [], "no worklet: the device's own thread renders");
  const v = voice(ctx, r);
  v.out.update(null, 0, 0, {});
  // The meter (the audio panel) wants one.
  r.meter(true);
  await r.modules;
  assert.ok(ctx.modules.includes("meter.js"));
  // Enhanced asked for: the light mix plays until the graph can be built.
  const later = new Renderer(new FakeContext({ channels: 8 }));
  await later.load(null, config);
  later.configure(config);
  const pending = later.configure({ quality: "enhanced" });
  assert.ok(pending instanceof Promise);
  assert.equal(later.light, true, "still the light mix");
  await pending;
  assert.equal(later.light, false);
  assert.equal(later.enhanced, true);
  assert.ok(describe(later.ctx).nodes.some((n) => n.name === "spatial-mixer"));
  // Loaded from the start for any other graph, as before.
  const enhanced = new Renderer(new FakeContext());
  await enhanced.load(null, { quality: "enhanced" });
  assert.equal(enhanced.ctx.modules.length, 6);
  assert.equal(enhanced.configure({ quality: "enhanced" }), null);
});

test("Classic on headphones and on surround keeps OpenAL Soft's output", async () => {
  const { ctx, r } = await renderer({ quality: "original", output: "5.1", dynamics: "home" }, { channels: 8 });
  assert.equal(r.light, false);
  const nodes = describe(ctx).nodes;
  const mixer = nodes.find((n) => n.name === "spatial-mixer");
  assert.equal(mixer.options.processorOptions.law, "alsoft");
  assert.ok(nodes.some((n) => n.name === "front-stabilizer"));
  r.configure({ output: "headphones" });
  assert.equal(r.light, false);
  assert.equal(r.hrtf, true);
  const v = voice(ctx, r);
  assert.equal(into(ctx, v.gain).map((e) => e.to.kind).sort().join(), "GainNode,PannerNode");
  // Stereo again: the light mix, the voice moved over to it.
  r.configure({ output: "stereo" });
  assert.equal(r.light, true);
  assert.deepEqual(into(ctx, v.gain).map((e) => e.to.kind), ["GainNode", "GainNode"]);
  assert.deepEqual(into(ctx, v.gain).map((e) => into(ctx, e.to)[0].to), [r.pair, r.pair]);
});

// An engine with one looping 3D sound (made up), on a stand-in context.
async function engineWith(config) {
  const ctx = new FakeContext({ channels: 8 });
  const pcm = new Int16Array(22050).fill(1000);
  const data = new SoundData(
    {
      objects: [
        { type: "Bus", Name: "0x00000001", Gain: 1 },
        { type: "SoundProperties", Name: "0x0000c001", Bus: "0x00000001", Mode3D: 1, MinDistance: 5, MaxDistance: 100, MuteDistance: 100, Looping: 1, ModeDoppler: 0, SampleList: [["0x00000100", 1]] },
      ],
      samples: { "0x00000100": [0, pcm.length, 22050] },
    },
    pcm.buffer,
  );
  const engine = new AudioEngine({ context: ctx });
  await engine.renderer.load();
  engine.renderer.configure(config);
  engine.setData(data);
  engine.setListener({ x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, { x: 0, y: 1, z: 0 });
  return { ctx, engine };
}

test("the engine plays through the light mix: the voice's level and OpenAL Soft's panning on two gains", async () => {
  const { ctx, engine } = await engineWith({ quality: "original", output: "stereo", dynamics: "normal" });
  // To the listener's left, 20 m off (past the curve's MinDistance).
  const v = engine.play("0x0000c001", { position: { x: -20, y: 0, z: -2 }, category: "effects" });
  engine.update(1 / 60);
  assert.ok(v.real, "heard");
  assert.equal(v.nodes.gain, null, "no gain of its own");
  // Its source straight into the two speaker gains: three nodes a voice.
  const sides = into(ctx, v.nodes.source).map((e) => e.to);
  assert.deepEqual(sides.map((n) => into(ctx, n)[0].to), [engine.renderer.pair, engine.renderer.pair]);
  const pan = alsoftStereo(v.local.x, v.local.y, v.local.z, [0, 0]);
  assert.ok(pan[0] > 0.9 && pan[1] < 0.1, "mostly left");
  // The voice's level is the original mode's: the library's curve and the
  // Steam build's falloff past half of MaxDistance (none here).
  assert.ok(v.audible > 0 && v.audible < 1);
  sides.forEach((side, c) => assert.ok(Math.abs(side.gain.value - v.audible * pan[c]) < 1e-9, `side ${c}`));
  // Closer: the level and the panning move together, eased.
  engine.setListener({ x: -12, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, { x: 0, y: 1, z: 0 });
  engine.update(1 / 60);
  const now = alsoftStereo(v.local.x, v.local.y, v.local.z, [0, 0]);
  sides.forEach((side, c) => assert.deepEqual(side.gain.events.at(-1), ["target", v.audible * now[c], ctx.currentTime, 0.015]));
  // Stopped: both fade out, the source stops.
  v.stop(0.03);
  sides.forEach((side) => assert.deepEqual(side.gain.events.at(-1), ["target", 0, ctx.currentTime, 0.01]));
});

test("the voice engine is the default; ?voices=native keeps the browser's nodes", () => {
  const page = globalThis.location;
  try {
    for (const [search, on] of [["", true], ["?map=bes1", true], ["?voices=native", false], ["?voices=rust", true]]) {
      globalThis.location = { search };
      assert.equal(voicesRust(), on, search || "(none)");
    }
  } finally {
    globalThis.location = page;
  }
});

test("the voice engine plays the light mix's voices in one worklet, sent what the browser's gains would be set to", async () => {
  const ctx = new FakeContext({ channels: 8 });
  const pcm = new Int16Array(22050).fill(1000);
  const data = new SoundData(
    {
      objects: [
        { type: "Bus", Name: "0x00000001", Gain: 1 },
        { type: "SoundProperties", Name: "0x0000c001", Bus: "0x00000001", Mode3D: 1, MinDistance: 5, MaxDistance: 100, MuteDistance: 100, Looping: 1, ModeDoppler: 0, SampleList: [["0x00000100", 1]] },
      ],
      samples: { "0x00000100": [0, pcm.length, 22050] },
    },
    pcm.buffer,
  );
  const engine = new AudioEngine({ context: ctx });
  const r = engine.renderer;
  r.voicesRust = true;
  const config = { quality: "original", output: "stereo", dynamics: "normal" };
  await r.load(null, config);
  assert.ok(ctx.modules.includes("spatial-mixer.js"), "the worklets load for the engine");
  await r.configure(config);
  engine.setData(data);
  engine.setListener({ x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, { x: 0, y: 1, z: 0 });
  assert.equal(r.light, true);
  assert.equal(r.engine, true);
  assert.equal(r.mixer.name, "voice-engine");
  assert.deepEqual(into(ctx, r.mixer).map((e) => e.to), [r.mix], "into the mix, as the pair");
  const sent = [];
  r.mixer.port.postMessage = (m) => sent.push(m);
  // The commands the messages carried, each [code, ...its numbers].
  const ARITY = { 1: 7, 2: 3, 3: 4, 4: 4, 5: 4, 6: 4, 7: 4, 8: 1, 9: 4, 10: 5, 11: 3, 12: 2 };
  const commands = () => {
    const out = [];
    for (const m of sent.splice(0)) {
      if (m.type !== "voices") continue;
      for (let i = 0; i < m.data.length; i += 1 + ARITY[m.data[i]]) out.push([...m.data.subarray(i, i + 1 + ARITY[m.data[i]])]);
    }
    return out;
  };
  const near = (got, want) => got.length === want.length && got.every((x, i) => Math.abs(x - want[i]) < 1e-9);
  const sources = ctx.nodes.filter((n) => n.kind === "AudioBufferSourceNode").length;
  const v = engine.play("0x0000c001", { position: { x: -20, y: 0, z: -2 }, category: "effects" });
  engine.update(1 / 60);
  assert.ok(v.real, "heard");
  assert.equal(ctx.nodes.filter((n) => n.kind === "AudioBufferSourceNode").length, sources, "no buffer source");
  assert.equal(sent[0].type, "sample");
  const slot = v.nodes.out.slot;
  const t = ctx.currentTime;
  const pan = alsoftStereo(v.local.x, v.local.y, v.local.z, [0, 0]);
  // The start (sample 0 from 0 s, looping, at rate 1), its speaker gains
  // made at 0, then placed as the browser's would be (AudioParam.value),
  // all at the context's time.
  const first = commands();
  assert.deepEqual(first.slice(0, 3), [
    [1, slot, 0, 0, 1, 1, 20000, t],
    [9, slot, 0, 0, t],
    [9, slot, 1, 0, t],
  ]);
  const placed = first.slice(3).filter((c) => c[0] === 9);
  assert.equal(placed.length, 2);
  placed.forEach((c, side) => assert.ok(near(c, [9, slot, side, v.audible * pan[side], t]), `placed ${side}: ${c}`));
  // Closer: both speaker gains eased to the new level and panning.
  engine.setListener({ x: -12, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, { x: 0, y: 1, z: 0 });
  engine.update(1 / 60);
  const now = alsoftStereo(v.local.x, v.local.y, v.local.z, [0, 0]);
  const eased = commands().filter((c) => c[0] === 10);
  assert.equal(eased.length, 2);
  eased.forEach((c, side) => assert.ok(near(c, [10, slot, side, v.audible * now[side], 0.015, t]), `eased ${side}: ${c}`));
  // Stopped: both fade out from now, and the source stops 40 ms on (sent
  // once the task is done, not a frame later).
  v.stop(0.03);
  await Promise.resolve();
  assert.deepEqual(commands(), [
    [11, slot, 0, t],
    [10, slot, 0, 0, 0.01, t],
    [11, slot, 1, t],
    [10, slot, 1, 0, 0.01, t],
    [12, slot, t + 0.04],
  ]);
});

test("the master chain goes into the mixer's module, and the rest of its graph with it", async () => {
  const build = async (config, masterRust = true) => {
    const ctx = new FakeContext({ channels: 8 });
    const r = new Renderer(ctx);
    r.voicesRust = true;
    r.masterRust = masterRust;
    r.masterVolume = 0.5;
    await r.load(null, config);
    await r.configure(config);
    return { ctx, r };
  };
  const { ctx, r } = await build({ quality: "enhanced", output: "5.1", dynamics: "night", mono: false });
  assert.equal(r.master, true);
  // One worklet, its output the finished sound; no other in the graph.
  assert.deepEqual(ctx.nodes.filter((n) => n.kind === "AudioWorkletNode").map((n) => n.name), ["spatial-mixer"]);
  assert.deepEqual(ctx.nodes.filter((n) => n.kind === "BiquadFilterNode"), []);
  assert.deepEqual(into(ctx, r.mixer).map((e) => e.to), [ctx.destination]);
  assert.equal(r.output, r.mixer);
  // Its last two inputs bring what comes through the browser's nodes.
  const inputs = r.mixer.options.numberOfInputs;
  assert.equal(inputs, 10, "a few for voices that keep the browser's nodes");
  assert.equal(r.mixer.options.processorOptions.slots, 64);
  assert.deepEqual(into(ctx, r.reverbIn).map((e) => [e.to, e.input]), [[r.mixer, inputs - 2]]);
  assert.deepEqual(into(ctx, r.mix).map((e) => [e.to, e.input]), [[r.mixer, inputs - 1]]);
  assert.deepEqual(r.mixer.options.outputChannelCount, [6]);
  // What the worklet is told: the nodes' own parameters.
  const m = r.mixer.options.processorOptions.master;
  assert.equal(m.rooms, 0b110111, "the reverb reaches every speaker, the LFE left dry");
  assert.deepEqual(m.lfe, [3, 120, -3.0103]);
  assert.deepEqual(m.highpass, [180, -3.0103]);
  assert.equal(m.eq.length, 3);
  assert.deepEqual(m.eq[2], [3, 8500, 3, 0.7]);
  assert.equal(m.clarity.skip, 3);
  assert.equal(m.dynamics.skip, 3);
  assert.equal(m.dynamics.threshold, -30);
  assert.equal(m.volume, 0.5);
  // The meter reads the finished sound.
  r.meter(true);
  assert.deepEqual(into(ctx, r.mixer).map((e) => e.to), [ctx.destination, r.meterNode]);
  r.meter(false);
  // A voice in the slots asks for the LFE as ever (update's r.lfe).
  assert.ok(r.lfe);
  // A voice in the engine takes a slot with no input; one that keeps the
  // browser's nodes one with an input; and each goes back where it was.
  const own = r.voice({ mode3D: 1 }, true);
  own.attach(ctx.createGain());
  const engineVoice = r.voice({ mode3D: 1 });
  assert.ok(own.slot >= 0 && own.slot < inputs - 2, `input ${own.slot}`);
  assert.ok(engineVoice.engine && engineVoice.slot >= inputs - 2, `engine slot ${engineVoice.slot}`);
  const spare = [r.slots.length, r.nodeSlots.length];
  own.dispose();
  engineVoice.dispose();
  assert.deepEqual([r.slots.length, r.nodeSlots.length], [spare[0] + 1, spare[1] + 1]);
  assert.ok(r.slots.every((slot) => slot >= inputs - 2) && r.nodeSlots.every((slot) => slot < inputs - 2));
  // The room and the volume reach the worklet.
  const sent = [];
  r.mixer.port.postMessage = (message) => sent.push(message);
  r.setRoom({ decay: 2 });
  r.setVolume(0.25);
  assert.deepEqual(sent, [
    { type: "room", params: { decay: 2 } },
    { type: "volume", value: 0.25, time: ctx.currentTime },
  ]);
  // The limiter's reports, as its worklet's.
  const reports = [];
  r.onLimiter = (report) => reports.push(report);
  r.mixer.port.onmessage({ data: { type: "sync", id: 1 } });
  r.mixer.port.onmessage({ data: { reduction: -2 } });
  assert.deepEqual(reports, [{ reduction: -2 }]);
  // No clarity: no EQ or stage in the chain.
  const plain = await build({ quality: "enhanced", output: "stereo", dynamics: "normal", mono: false, clarity: false });
  const options = plain.r.mixer.options.processorOptions.master;
  assert.deepEqual(options.eq, []);
  assert.equal(options.clarity, null);
  assert.equal(options.rooms, -1);
  assert.equal(options.lfe, null);
  // The mono fold keeps its nodes, as the switch off does; the light mix
  // has its own graph.
  for (const [config, off] of [
    [{ quality: "enhanced", output: "stereo", dynamics: "normal", mono: true }, true],
    [{ quality: "enhanced", output: "stereo", dynamics: "normal", mono: false }, false],
    [{ quality: "original", output: "stereo", dynamics: "normal", mono: false }, true],
  ]) {
    const { r } = await build(config, off);
    assert.equal(r.master, false, JSON.stringify(config));
    assert.equal(r.engine, true);
    assert.ok(r.volume);
  }
});

test("the enhanced mode's beds on stereo take as few nodes as the light mix's", async () => {
  const { ctx, r } = await renderer({ quality: "enhanced", output: "stereo", dynamics: "normal" });
  const bed = (channels, kind) => {
    const gain = ctx.createGain();
    return { gain, out: r.bed(channels, gain, kind) };
  };
  // Stereo: straight into the mix, the upmix being for surround.
  const music = bed(2, "music");
  assert.deepEqual(into(ctx, music.gain).map((e) => e.to), [r.mix]);
  assert.equal(music.out.nodes.length, 0);
  // Mono: one gain, into both speakers alike ("speakers").
  const voice = bed(1, "voice");
  assert.equal(voice.out.nodes.length, 1);
  const both = voice.out.nodes[0];
  assert.deepEqual(into(ctx, voice.gain).map((e) => e.to), [both]);
  assert.deepEqual(into(ctx, both).map((e) => e.to), [r.mix]);
  assert.ok(Math.abs(both.gain.value - Math.SQRT1_2) < 1e-9);
  assert.deepEqual([both.channelCount, both.channelCountMode, both.channelInterpretation], [2, "explicit", "speakers"]);
  // Quad: the front pair straight to its speakers, the back pair folded
  // onto its side through a gain each.
  const ambience = bed(4, "ambience");
  const [split, route, ...amps] = ambience.out.nodes;
  assert.equal(split.kind, "ChannelSplitterNode");
  assert.equal(route.kind, "ChannelMergerNode");
  assert.deepEqual(amps.map((a) => a.gain.value), [0.7, 0.7]);
  const routes = ctx.edges.filter((e) => e.to === route).map((e) => [e.from === split ? e.output : e.from === amps[0] ? 2 : 3, e.input]);
  assert.deepEqual(routes.sort(), [
    [0, 0],
    [1, 1],
    [2, 0],
    [3, 1],
  ]);
});

test("a voice with a node of its own keeps the browser's nodes beside the voice engine", async () => {
  // A stream's line said somewhere, and the audio panel's test.
  for (const quality of ["original", "enhanced"]) {
    const ctx = new FakeContext();
    const r = new Renderer(ctx);
    r.voicesRust = true;
    const config = { quality, output: "stereo", dynamics: "normal" };
    await r.load(null, config);
    await r.configure(config);
    assert.equal(r.engine, true, quality);
    const line = ctx.createGain();
    const out = r.voice({ mode3D: 1 }, true);
    out.attach(line);
    assert.ok(!out.engine, `${quality}: not in the engine`);
    if (quality === "original") {
      assert.deepEqual(into(ctx, line).map((e) => into(ctx, e.to)[0].to), [r.pair, r.pair]);
    } else {
      const [filter, send] = into(ctx, line).map((e) => e.to);
      assert.equal(filter.kind, "BiquadFilterNode");
      assert.deepEqual(into(ctx, filter).map((e) => [e.to, e.input]), [[r.mixer, out.slot]]);
      assert.deepEqual(into(ctx, send).map((e) => e.to), [r.reverbIn]);
    }
    // A voice without one still plays in the engine, on another slot.
    const other = r.voice({ mode3D: 1 });
    assert.ok(other.engine, `${quality}: the rest play in the engine`);
    assert.notEqual(other.slot, out.slot);
    out.dispose();
    other.dispose();
  }
});

test("a voice playing when the light mix comes or goes carries on in the new graph", async () => {
  const { ctx, engine } = await engineWith({ quality: "original", output: "stereo", dynamics: "normal" });
  const r = engine.renderer;
  const v = engine.play("0x0000c001", { position: { x: 8, y: 0, z: -10 }, category: "effects" });
  engine.update(1 / 60);
  const lightSource = v.nodes.source;
  // To Enhanced: a voice with no gain of its own leaves the graph at once
  // (never heard at the light mix's level there) and starts again next
  // frame, with one.
  r.configure({ quality: "enhanced", output: "stereo" });
  assert.deepEqual(into(ctx, lightSource), [], "out of the new graph");
  engine.update(1 / 60);
  assert.equal(v.real, false);
  engine.update(1 / 60);
  assert.ok(v.real && v.nodes.gain && v.nodes.source !== lightSource, "started again");
  assert.equal(into(ctx, v.nodes.gain).map((e) => e.to.kind).sort().join(), "BiquadFilterNode,GainNode");
  // Back to the light mix: a voice with a gain of its own just moves over.
  const gain = v.nodes.gain;
  r.configure({ quality: "original", output: "stereo" });
  engine.update(1 / 60);
  assert.equal(v.nodes.gain, gain, "the same voice");
  const sides = into(ctx, gain).map((e) => e.to);
  assert.deepEqual(sides.map((n) => into(ctx, n)[0].to), [r.pair, r.pair]);
  const pan = alsoftStereo(v.local.x, v.local.y, v.local.z, [0, 0]);
  sides.forEach((side, c) => assert.ok(Math.abs(side.gain.value - pan[c]) < 1e-9, "panning alone: the level is its gain's"));
  v.stop(0);
});
