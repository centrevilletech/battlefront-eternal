// The Rust audio DSP (rust/audio, ?audio=wasm) against the JavaScript
// worklets, sample for sample: each processor is fed the same blocks
// through its own process() here in Node (the worklet files loaded with
// their globals stubbed, the WebAssembly from src/audio-dsp-wasm.js with
// this process's Math as its host), and every output Float32 must be
// bit-identical, for the parameter sets battles use (the panning laws and
// layouts, the rooms, the dynamics profiles) and directed signals
// (impulses, noise, full-scale, silence, denormal-tiny, a NaN).
//
//   node tests/audio-dsp-parity.mjs [name...]
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { AUDIO_DSP_WASM } from "../src/audio-dsp-wasm.js";
import { LAYOUTS, ALSOFT } from "../src/audio/layouts.js";

const RATE = 48000;
const BLOCK = 128;
const only = process.argv.slice(2);
const wanted = (name) => !only.length || only.some((o) => name.includes(o));

// A worklet file's processor class, with the worklet globals stubbed.
function loadProcessor(file, rate = RATE) {
  const code = fs.readFileSync(new URL(`../src/audio/worklets/${file}`, import.meta.url), "utf8");
  let Processor = null;
  const sandbox = {
    sampleRate: rate,
    console,
    AudioWorkletProcessor: class {
      constructor() {
        this.port = { onmessage: null, messages: [], postMessage(m) { this.messages.push(m); } };
      }
    },
    registerProcessor: (name, cls) => { Processor = cls; },
  };
  vm.runInNewContext(code, sandbox, { filename: file });
  assert(Processor, `${file} registered a processor`);
  return Processor;
}

const math = {
  js_sin: Math.sin, js_cos: Math.cos, js_tan: Math.tan, js_asin: Math.asin,
  js_atan2: Math.atan2, js_exp: Math.exp, js_pow: Math.pow, js_log10: Math.log10,
};
const dsp = await WebAssembly.instantiate(Buffer.from(AUDIO_DSP_WASM, "base64"), { env: math });
const x = dsp.instance.exports;
const f32 = (ptr, len) => new Float32Array(x.memory.buffer, ptr, len);
const f64 = (ptr, len) => new Float64Array(x.memory.buffer, ptr, len);
const u8 = (ptr, len) => new Uint8Array(x.memory.buffer, ptr, len);

// Seeded noise and directed blocks.
function makeRand(seed) {
  let s = seed >>> 0;
  return () => ((s = (Math.imul(s, 1103515245) + 12345) >>> 0) / 4294967296) * 2 - 1;
}
function signal(kind, channels, blocks, seed = 7) {
  const rand = makeRand(seed);
  const out = [];
  for (let b = 0; b < blocks; b++) {
    out.push(Array.from({ length: channels }, (_, c) => {
      const a = new Float32Array(BLOCK);
      for (let i = 0; i < BLOCK; i++) {
        if (kind === "noise") a[i] = rand() * 0.6;
        else if (kind === "hot") a[i] = rand() * 1.4;
        else if (kind === "sine") a[i] = Math.fround(0.8 * Math.sin((b * BLOCK + i) * 0.11 + c));
        else if (kind === "impulse") a[i] = b === 0 && i === 0 ? 1 : 0;
        else if (kind === "tiny") a[i] = rand() * 1e-38;
        else if (kind === "square") a[i] = ((b * BLOCK + i) >> 4) % 2 ? 0.9 : -0.9;
      }
      if (kind === "nan" && b === 1 && c === 0) a[3] = NaN;
      if (kind === "nan") for (let i = 0; i < BLOCK; i++) if (!a[i]) a[i] = rand() * 0.3;
      return a;
    }));
  }
  return out;
}

const bits = new DataView(new ArrayBuffer(8));
const sameF32 = (a, b) => { bits.setFloat32(0, a); const u = bits.getUint32(0); bits.setFloat32(0, b); return u === bits.getUint32(0); };
const sameF64 = (a, b) => { bits.setFloat64(0, a); const u = bits.getBigUint64(0); bits.setFloat64(0, b); return u === bits.getBigUint64(0); };

let checked = 0;
function compare(label, jsOut, rsOut) {
  let diffs = 0, first = "";
  const walk = (a, b, path) => {
    if (Array.isArray(a)) { a.forEach((v, i) => walk(v, b[i], `${path}[${i}]`)); return; }
    for (let i = 0; i < a.length; i++) {
      checked++;
      if (!sameF32(a[i], b[i])) { if (!first) first = `${path}[${i}] js=${a[i]} rust=${b[i]}`; diffs++; }
    }
  };
  walk(jsOut, rsOut, "");
  assert.equal(diffs, 0, `${label}: ${diffs} samples differ, first ${first}`);
  console.log(`ok ${label}`);
}

// Runs a single-input processor (reverb, stabilizer, limiter, clarity)
// through both, `drive` being called per block index before the block on
// both sides (port messages etc).
function runPair({ label, js, rust: { input, output, process: proc }, inputs, inChannels, outChannels, drive }) {
  const jsAll = [], rsAll = [];
  inputs.forEach((blockIn, b) => {
    drive?.(b, "js");
    drive?.(b, "rust");
    const o = Array.from({ length: outChannels }, () => new Float32Array(BLOCK));
    js.process([blockIn.map((c) => Float32Array.from(c))], [o], {});
    jsAll.push(o.map((c) => Float32Array.from(c)));
    const inp = f32(input(), inChannels * BLOCK);
    blockIn.forEach((c, i) => inp.set(c, i * BLOCK));
    proc(inChannels, outChannels);
    const outp = f32(output(), outChannels * BLOCK);
    rsAll.push(Array.from({ length: outChannels }, (_, c) => Float32Array.from(outp.subarray(c * BLOCK, (c + 1) * BLOCK))));
  });
  compare(label, jsAll, rsAll);
}

// --- The spatial mixer, per law and layout -------------------------------
if (wanted("mixer")) {
  const Mixer = loadProcessor("spatial-mixer.js");
  const LAWS = { panpot: 0, vbap: 1, alsoft: 2, ambi3: 3 };
  const setLayout = (h, law, speakers, decoder, dirty) => {
    const cfg = f64(x.mixer_cfg(h, Math.max(speakers.length * 3, 1)), Math.max(speakers.length * 3, 1));
    speakers.forEach((s, i) => cfg.set(s ? [s[0], s[1], 1] : [0, 0, 0], i * 3));
    x.mixer_speakers(h, speakers.length, dirty);
    if (decoder) {
      const flat = [decoder.rows.length, ...decoder.rows.map((r) => (r ? r.length : -1)), ...decoder.rows.flatMap((r) => r || [])];
      f64(x.mixer_cfg(h, flat.length), flat.length).set(flat);
      f64(x.mixer_io(h), 1)[0] = decoder.front || 0;
      x.mixer_decoder(h, 1);
    } else x.mixer_decoder(h, 0);
    x.mixer_law(h, LAWS[law]);
  };
  for (const [label, law, layout, decoder] of [
    ["panpot stereo", "panpot", LAYOUTS.stereo, null],
    ["alsoft stereo", "alsoft", LAYOUTS.stereo, ALSOFT.stereo],
    ["alsoft 5.1", "alsoft", LAYOUTS["5.1"], ALSOFT["5.1"]],
    ["alsoft 7.1", "alsoft", LAYOUTS["7.1"], ALSOFT["7.1"]],
    ["vbap 5.1", "vbap", LAYOUTS["5.1"], null],
    ["vbap 7.1.4", "vbap", LAYOUTS["7.1.4"], null],
    ["ambi3", "ambi3", null, null],
  ]) {
    const channels = law === "ambi3" ? 16 : layout.channels;
    const speakers = layout ? layout.speakers : [];
    const slots = 10, active = 7, blocks = 40;
    const js = new Mixer({ numberOfInputs: slots, processorOptions: { channels, law, speakers, lfe: layout?.lfe ?? -1, decoder } });
    const h = x.mixer_new(slots, channels);
    setLayout(h, law, speakers, decoder, 0);
    const params = (b) => {
      const p = new Float32Array(active * 6);
      for (let v = 0; v < active; v++) {
        const a = v * 2.4 + b * 0.05;
        p.set([v, Math.sin(a), 0.3 * Math.sin(v + b * 0.01), -Math.cos(a), (v % 4) * 0.25, v % 3 === 0 ? 0.5 : 0], v * 6);
      }
      return p;
    };
    const sendParams = (b) => {
      const p = params(b);
      js.port.onmessage({ data: { type: "params", data: p } });
      f32(x.mixer_params_ptr(h), p.length).set(p);
      x.mixer_params(h, p.length, 0);
    };
    sendParams(0);
    for (let v = 0; v < active; v++) { js.port.onmessage({ data: { type: "start", slot: v } }); x.mixer_start(h, v); }
    const inputs = signal("noise", active, blocks);
    const jsAll = [], rsAll = [];
    for (let b = 0; b < blocks; b++) {
      if (b === 13 || b === 27) sendParams(b);
      // Inputs: slot of voices, the rest unconnected; block 20 on, slot 1 silent.
      const jsIn = Array.from({ length: slots }, (_, s) => (s < active ? [b >= 20 && s === 1 ? new Float32Array(BLOCK) : Float32Array.from(inputs[b][s])] : undefined));
      const jsOut = [Array.from({ length: channels }, () => new Float32Array(BLOCK)), [new Float32Array(BLOCK)]];
      js.process(jsIn, jsOut, {});
      jsAll.push([...jsOut[0], jsOut[1][0]].map((c) => Float32Array.from(c)));
      const inp = f32(x.mixer_in(h), slots * BLOCK);
      const present = u8(x.mixer_present(h), slots);
      for (let s = 0; s < slots; s++) { present[s] = s < active ? 1 : 0; if (s < active) inp.set(b >= 20 && s === 1 ? new Float32Array(BLOCK) : inputs[b][s], s * BLOCK); }
      x.mixer_process(h, channels, 1);
      const outp = f32(x.mixer_out(h), (channels + 1) * BLOCK);
      rsAll.push(Array.from({ length: channels + 1 }, (_, c) => Float32Array.from(outp.subarray(c * BLOCK, (c + 1) * BLOCK))));
    }
    compare(`mixer ${label}`, jsAll, rsAll);
    // A layout change mid-stream keeps matching.
    js.port.onmessage({ data: { type: "layout", law: "panpot", speakers: LAYOUTS.stereo.speakers, lfe: -1, decoder: null } });
    setLayout(h, "panpot", LAYOUTS.stereo.speakers, null, 1);
    // (channels differ after the change only in the rows the gains use;
    // run a few blocks at the constructed channel count)
    const more = signal("sine", active, 4, 3);
    const j2 = [], r2 = [];
    for (let b = 0; b < 4; b++) {
      const jsIn = Array.from({ length: slots }, (_, s) => (s < active ? [Float32Array.from(more[b][s])] : undefined));
      const jsOut = [Array.from({ length: channels }, () => new Float32Array(BLOCK)), [new Float32Array(BLOCK)]];
      js.process(jsIn, jsOut, {});
      j2.push([...jsOut[0], jsOut[1][0]].map((c) => Float32Array.from(c)));
      const inp = f32(x.mixer_in(h), slots * BLOCK);
      for (let s = 0; s < active; s++) inp.set(more[b][s], s * BLOCK);
      x.mixer_process(h, channels, 1);
      const outp = f32(x.mixer_out(h), (channels + 1) * BLOCK);
      r2.push(Array.from({ length: channels + 1 }, (_, c) => Float32Array.from(outp.subarray(c * BLOCK, (c + 1) * BLOCK))));
    }
    compare(`mixer ${label} after layout change`, j2, r2);
  }
}

// --- The reverb -----------------------------------------------------------
if (wanted("reverb")) {
  const Reverb = loadProcessor("reverb.js");
  const KEYS = ["room", "roomHF", "decay", "hfRatio", "reflections", "reflDelay", "reverb", "reverbDelay", "diffusion", "density", "hfReference"];
  const send = (h, obj, initial) => {
    const io = f64(x.reverb_io(h), 12);
    let mask = 0;
    KEYS.forEach((k, i) => { if (obj[k] !== undefined) { io[i] = obj[k]; mask |= 1 << i; } });
    io[11] = mask;
    x.reverb_params(h, initial ? 1 : 0);
  };
  for (const [label, params, channels, kind, blocks] of [
    ["the enhanced room", { room: 0.08, roomHF: 0.8, decay: 0.8, hfRatio: 0.6, reflections: 0.2, reflDelay: 0.02, reverb: 0.7, reverbDelay: 0.03, diffusion: 1, density: 1, hfReference: 5000 }, 2, "noise", 300],
    ["a wet hall, 6ch", { room: 0.35, roomHF: 0.4, decay: 3.5, hfRatio: 0.3, reflections: 0.6, reflDelay: 0.05, reverb: 1.2, reverbDelay: 0.1, diffusion: 0.7, density: 0.6, hfReference: 3000 }, 6, "impulse", 600],
    ["a dry small room", { room: 0.05, roomHF: 1, decay: 0.3, hfRatio: 1.5, reflections: 0.1, reflDelay: 0, reverb: 0.5, reverbDelay: 0, diffusion: 0.2, density: 0.2, hfReference: 8000 }, 2, "sine", 200],
  ]) {
    const js = new Reverb({ processorOptions: { channels, params } });
    const h = x.reverb_new(RATE, channels);
    send(h, params, true);
    runPair({
      label: `reverb ${label}`, js,
      rust: { input: () => x.reverb_in(h), output: () => x.reverb_out(h), process: (i, o) => x.reverb_process(h, i, o) },
      inputs: signal(kind, 1, blocks).map((b) => b), inChannels: 1, outChannels: channels,
      drive: (b, side) => {
        if (b === 50) {
          const next = { decay: 1.8, room: 0.2, density: 0.9 };
          if (side === "js") js.port.onmessage({ data: next }); else send(h, next, false);
        }
      },
    });
  }
  // Silence in for longer than every delay: the quiet shortcut on both.
  const js = new Reverb({ processorOptions: { channels: 2, params: { room: 0.2, decay: 0.5 } } });
  const h = x.reverb_new(RATE, 2);
  send(h, { room: 0.2, decay: 0.5 }, true);
  runPair({
    label: "reverb silence out the tail", js,
    rust: { input: () => x.reverb_in(h), output: () => x.reverb_out(h), process: (i, o) => x.reverb_process(h, i, o) },
    inputs: signal("impulse", 1, 400), inChannels: 1, outChannels: 2,
  });
}

// --- The stabilizer -------------------------------------------------------
if (wanted("stabilizer")) {
  const Stab = loadProcessor("stabilizer.js");
  // 5.1 and 7.1 (the front pair's filters run side by side), fewer
  // inputs than channels, no centre, a layout whose centre is its left
  // (one channel after the other, as the worklet), and signals down to
  // subnormals.
  for (const [label, channels, centre, kind, inChannels = channels] of [
    ["6ch", 6, 2, "noise"], ["8ch", 8, 2, "noise"], ["6ch, 4 inputs", 6, 2, "noise", 4], ["4ch, no centre", 4, 5, "noise"],
    ["6ch, centre on left", 6, 0, "noise"], ["6ch tiny", 6, 2, "tiny"], ["6ch impulse", 6, 2, "impulse"],
  ]) {
    const js = new Stab({ processorOptions: { channels, left: 0, right: 1, centre } });
    const h = x.stabilizer_new(RATE, channels, 0, 1, centre);
    runPair({
      label: `stabilizer ${label}`, js,
      rust: { input: () => x.stabilizer_in(h), output: () => x.stabilizer_out(h), process: (i, o) => x.stabilizer_process(h, i, o) },
      inputs: signal(kind, inChannels, 120), inChannels, outChannels: channels,
    });
  }
}

// --- The limiter ----------------------------------------------------------
if (wanted("limiter")) {
  const Lim = loadProcessor("limiter.js");
  const DYN = {
    home: { threshold: 0, ratio: 1, knee: 0, attack: 0.003, release: 0.25, makeup: -10.2 },
    normal: { threshold: -18, ratio: 2, knee: 8, attack: 0.025, release: 0.2, makeup: -2.3 },
    night: { threshold: -30, ratio: 5, knee: 10, attack: 0.004, release: 0.35, makeup: 10.6 },
  };
  const FIELDS = ["threshold", "ratio", "knee", "attack", "release", "makeup", "skip"];
  const setDyn = (h, d) => {
    const io = f64(x.limiter_io(h), 8);
    let mask = 0;
    FIELDS.forEach((k, i) => { if (d[k] !== undefined) { io[i] = d[k]; mask |= 1 << i; } });
    io[7] = mask;
    x.limiter_dynamics(h);
  };
  for (const [label, dyn, channels, kind, blocks] of [
    ["home hot", DYN.home, 2, "hot", 400],
    ["normal noise", DYN.normal, 2, "noise", 400],
    ["night square 6ch", DYN.night, 6, "square", 400],
    ["normal quiet then loud", DYN.normal, 2, "tiny", 80],
  ]) {
    const dynamics = { ...dyn, skip: channels > 2 ? 3 : -1 };
    const js = new Lim({ processorOptions: { ceiling: -1, release: 0.08, dynamics } });
    const h = x.limiter_new(RATE, -1, 0.08, 0.0015);
    setDyn(h, dynamics);
    const jsReports = [], rsReports = [];
    runPair({
      label: `limiter ${label}`, js,
      rust: {
        input: () => x.limiter_in(h), output: () => x.limiter_out(h),
        process: (i, o) => { if (x.limiter_process(h, i, o)) rsReports.push(f64(x.limiter_io(h), 1)[0]); },
      },
      inputs: signal(kind, channels, blocks), inChannels: channels, outChannels: channels,
      drive: (b, side) => {
        if (side === "js") jsReports.push(...js.port.messages.splice(0).map((m) => m.reduction));
      },
    });
    jsReports.push(...js.port.messages.splice(0).map((m) => m.reduction));
    assert.equal(rsReports.length, jsReports.length, `limiter ${label}: report count`);
    rsReports.forEach((v, i) => assert(sameF64(v, jsReports[i]), `limiter ${label}: reduction ${i} js=${jsReports[i]} rust=${v}`));
    console.log(`ok limiter ${label} reports`);
  }
  // A NaN through the limiter (denormals and NaN handled as JS does).
  const js = new Lim({ processorOptions: { ceiling: -1, release: 0.08, dynamics: DYN.normal } });
  const h = x.limiter_new(RATE, -1, 0.08, 0.0015);
  setDyn(h, { ...DYN.normal, skip: -1 });
  runPair({
    label: "limiter NaN", js,
    rust: { input: () => x.limiter_in(h), output: () => x.limiter_out(h), process: (i, o) => x.limiter_process(h, i, o) },
    inputs: signal("nan", 2, 30), inChannels: 2, outChannels: 2,
  });
}

// --- The clarity stage ----------------------------------------------------
if (wanted("clarity")) {
  const Clarity = loadProcessor("clarity.js");
  // Channels run their filters two at a time, an odd one alone: odd and
  // even counts, the LFE skipped, fewer inputs than channels, and signals
  // that take the filters' state down to subnormals and zeros.
  for (const [label, channels, kind, skip = channels > 2 ? 3 : -1, inChannels = channels, blocks = 200] of [
    ["2ch noise", 2, "noise"], ["6ch square", 6, "square"], ["2ch sine", 2, "sine"],
    ["3ch noise", 3, "noise", -1], ["12ch noise", 12, "noise"], ["6ch, 3 inputs", 6, "noise", 3, 3],
    ["2ch tiny", 2, "tiny"], ["2ch impulse", 2, "impulse", -1, 2, 400], ["2ch nan", 2, "nan"],
  ]) {
    const js = new Clarity({ processorOptions: { channels, skip } });
    const h = x.clarity_new(RATE, channels, skip);
    runPair({
      label: `clarity ${label}`, js,
      rust: { input: () => x.clarity_in(h), output: () => x.clarity_out(h), process: (i, o) => x.clarity_process(h, i, o) },
      inputs: signal(kind, inChannels, blocks), inChannels, outChannels: channels,
    });
  }
}

// --- The meter -------------------------------------------------------------
if (wanted("meter")) {
  const Meter = loadProcessor("meter.js");
  for (const [label, channels, weights] of [
    ["2ch", 2, null],
    ["6ch weighted", 6, [1, 1, 1, 0, 1.41, 1.41]],
  ]) {
    const js = new Meter({ processorOptions: weights ? { weights } : {} });
    const h = x.meter_new(RATE);
    if (weights) { f64(x.meter_cfg(h, weights.length), weights.length).set(weights); x.meter_weights(h, weights.length); }
    const inputs = signal("noise", channels, 800); // a little over 2 s
    const rs = [];
    for (const blockIn of inputs) {
      js.process([blockIn.map((c) => Float32Array.from(c))], [], {});
      const inp = f32(x.meter_in(h), channels * BLOCK);
      blockIn.forEach((c, i) => inp.set(c, i * BLOCK));
      if (x.meter_process(h, channels)) rs.push(...readMeterCfg(h));
    }
    function readMeterCfg(handle) {
      // The readings the last process laid out in cfg, its length in io.
      const ptr = x.meter_readings(handle);
      const total = f64(x.meter_io(handle), 1)[0];
      const all = f64(ptr, total);
      const out = [];
      let at = 0;
      while (at < total) {
        const [momentary, shortTerm, integrated, peak, truePeak, n] = [...all.subarray(at, at + 6)];
        out.push({ momentary, shortTerm, integrated, peak, truePeak, channels: [...all.subarray(at + 6, at + 6 + n)] });
        at += 6 + n;
      }
      return out;
    }
    const jsm = js.port.messages;
    assert.equal(rs.length, jsm.length, `meter ${label}: reading count js=${jsm.length} rust=${rs.length}`);
    jsm.forEach((m, i) => {
      for (const k of ["momentary", "shortTerm", "integrated", "peak", "truePeak"]) {
        assert(sameF64(rs[i][k], m[k]), `meter ${label} reading ${i} ${k}: js=${m[k]} rust=${rs[i][k]}`);
      }
      m.channels.forEach((v, c) => assert(sameF64(rs[i].channels[c], v), `meter ${label} reading ${i} ch${c}`));
      checked += 5 + m.channels.length;
    });
    console.log(`ok meter ${label} (${jsm.length} readings)`);
  }
}

// --- Other rates ------------------------------------------------------------
if (wanted("rates")) {
  // 44.1 kHz: the limiter and clarity coefficients at another rate.
  const Lim = loadProcessor("limiter.js", 44100);
  const jsLim = new Lim({ processorOptions: { ceiling: -1, release: 0.08, dynamics: { threshold: -18, ratio: 2, knee: 8, attack: 0.025, release: 0.2, makeup: -2.3, skip: -1 } } });
  const hLim = x.limiter_new(44100, -1, 0.08, 0.0015);
  {
    const io = f64(x.limiter_io(hLim), 8);
    io.set([-18, 2, 8, 0.025, 0.2, -2.3, -1, 0b1111111]);
    x.limiter_dynamics(hLim);
  }
  runPair({
    label: "limiter at 44.1 kHz", js: jsLim,
    rust: { input: () => x.limiter_in(hLim), output: () => x.limiter_out(hLim), process: (i, o) => x.limiter_process(hLim, i, o) },
    inputs: signal("hot", 2, 200), inChannels: 2, outChannels: 2,
  });
  // 4 kHz: reverb lines shorter than a block take the sample-at-a-time path.
  const Reverb = loadProcessor("reverb.js", 4000);
  const jsRev = new Reverb({ processorOptions: { channels: 2, params: { room: 0.3, decay: 0.8 } } });
  const hRev = x.reverb_new(4000, 2);
  {
    const io = f64(x.reverb_io(hRev), 12);
    io[0] = 0.3; io[2] = 0.8; io[11] = 0b101;
    x.reverb_params(hRev, 1);
  }
  runPair({
    label: "reverb at 4 kHz (short lines)", js: jsRev,
    rust: { input: () => x.reverb_in(hRev), output: () => x.reverb_out(hRev), process: (i, o) => x.reverb_process(hRev, i, o) },
    inputs: signal("noise", 1, 120), inChannels: 1, outChannels: 2,
  });
}

console.log(`\n${checked} samples and readings compared, all bit-identical`);
