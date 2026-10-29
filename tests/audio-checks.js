import { ALSOFT, LAYOUTS, alsoftGains, vbap } from "../src/audio/layouts.js";
import { DYNAMICS, ORIGINAL_LEVEL, Renderer } from "../src/audio/spatial.js";
import { AudioEngine } from "../src/audio/engine.js";
import { SoundData } from "../src/audio/data.js";

// Offline audio checks (OfflineAudioContext), with generated signals and
// made-up sound data; no retail assets. Run by tests/audio.mjs.

const RATE = 48000;
const db = (x) => 20 * Math.log10(Math.max(x, 1e-12));
const addModule = (ctx, name) => ctx.audioWorklet.addModule(`/src/audio/worklets/${name}.js`);

function rms(data, from = 0, to = data.length) {
  let s = 0;
  for (let i = from; i < to; i++) s += data[i] * data[i];
  return Math.sqrt(s / Math.max(to - from, 1));
}

// True peak by 4x oversampling (BS.1770-4 Annex 2 style), independent of
// the worklets' own code.
function truePeak(channels) {
  const P = 4;
  const T = 12;
  const n = P * T;
  const h = [];
  for (let i = 0; i < n; i++) {
    const t = (i - (n - 1) / 2) / P;
    const sinc = t === 0 ? 1 : Math.sin(Math.PI * t) / (Math.PI * t);
    h.push(sinc * (0.5 - 0.5 * Math.cos((2 * Math.PI * (i + 0.5)) / n)));
  }
  let peak = 0;
  for (const x of channels) {
    for (let i = T; i < x.length; i++) {
      peak = Math.max(peak, Math.abs(x[i]));
      for (let p = 1; p < P; p++) {
        let acc = 0;
        for (let k = 0; k < T; k++) acc += h[k * P + p] * x[i - k];
        peak = Math.max(peak, Math.abs(acc));
      }
    }
  }
  return peak;
}

function sine(ctx, freq, amp, phase = 0) {
  const len = ctx.length;
  const buffer = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = buffer.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = amp * Math.sin((2 * Math.PI * freq * i) / ctx.sampleRate + phase);
  const src = ctx.createBufferSource();
  src.buffer = buffer;
  return src;
}

export async function checkAudio() {
  const results = [];
  const assert = (name, ok, detail) => {
    results.push({ name, ok: Boolean(ok), detail });
    if (!ok) throw new Error(`${name}: ${JSON.stringify(detail)}`);
  };

  // 1. The spatial mixer's routing for every layout: a tone from a set of
  // directions lands on the channels pairwise panning says, at unit power.
  for (const [name, layout] of Object.entries(LAYOUTS)) {
    const law = layout.channels === 2 ? "panpot" : "vbap";
    for (const az of [0, 30, -30, 90, -110, 150, 180]) {
      const N = layout.channels;
      const ctx = new OfflineAudioContext(N, RATE / 4, RATE);
      await addModule(ctx, "spatial-mixer");
      const a = (az * Math.PI) / 180;
      const node = new AudioWorkletNode(ctx, "spatial-mixer", {
        numberOfInputs: 4,
        numberOfOutputs: 2,
        outputChannelCount: [N, 1],
        processorOptions: { channels: N, law, speakers: layout.speakers, lfe: layout.lfe, params: [0, -Math.sin(a), 0, -Math.cos(a), 0, 0] },
      });
      node.connect(ctx.destination, 0);
      const src = sine(ctx, 1000, 0.5);
      src.connect(node, 0, 0);
      src.start();
      const out = await ctx.startRendering();
      const level = [];
      for (let c = 0; c < N; c++) level.push(rms(out.getChannelData(c), RATE / 8) / (0.5 / Math.SQRT2));
      const power = level.reduce((s, x) => s + x * x, 0);
      let expect;
      if (law === "panpot") {
        const pan = Math.sin(a) * -1; // positive azimuth is left: pan -1
        const t = ((-Math.sin(a) + 1) * Math.PI) / 4;
        expect = [Math.cos(t), Math.sin(t)];
        void pan;
      } else {
        expect = vbap(layout, az);
      }
      const err = Math.max(...level.map((x, c) => Math.abs(x - expect[c])));
      assert(`${name} at ${az} deg lands where it should`, err < 0.02 && Math.abs(power - 1) < 0.03, { level: level.map((x) => +x.toFixed(3)), expect: expect.map((x) => +x.toFixed(3)), power: +power.toFixed(3) });
    }
  }

  // 2. The whole enhanced chain on 5.1: an explosion's LFE send reaches only
  // the LFE channel, low-passed (a 2 kHz tone there is 30+ dB under the 50
  // Hz one), and voice-over beds land on the centre.
  {
    const ctx = new OfflineAudioContext(6, RATE, RATE);
    const r = new Renderer(ctx);
    await r.load();
    r.configure({ quality: "enhanced", output: "5.1", dynamics: "home", clarity: false });
    const out = r.voice({ mode3D: 1 });
    const mixLow = sine(ctx, 50, 0.25);
    const mixHigh = sine(ctx, 2000, 0.25);
    const sum = ctx.createGain();
    mixLow.connect(sum);
    mixHigh.connect(sum);
    out.attach(sum);
    ctx.suspend(0.05).then(() => {
      out.update({ x: 0, y: 0, z: -1 }, 20, 0, { lfe: 1, cutoff: 20000, send: 0 });
      r.flush();
      ctx.resume();
    });
    mixLow.start();
    mixHigh.start();
    const rendered = await ctx.startRendering();
    const lfe = rendered.getChannelData(3).slice(RATE / 4);
    // Split the LFE channel's 50 Hz and 2 kHz content by correlation.
    const tone = (f) => {
      let s = 0;
      let c = 0;
      for (let i = 0; i < lfe.length; i++) {
        const t = (2 * Math.PI * f * (i + RATE / 4)) / RATE;
        s += lfe[i] * Math.sin(t);
        c += lfe[i] * Math.cos(t);
      }
      return (2 * Math.hypot(s, c)) / lfe.length;
    };
    // Undo the home profile's output level to compare with the input.
    const trim = 10 ** (-DYNAMICS.home.makeup / 20);
    const low = tone(50) * trim;
    const high = tone(2000) * trim;
    const centre = rms(rendered.getChannelData(2), RATE / 4) * trim;
    const left = rms(rendered.getChannelData(0), RATE / 4) * trim;
    // The send is 0.5 of the voice; two Butterworth sections at 120 Hz pass
    // 50 Hz at about -0.1 dB.
    assert("5.1: the LFE carries the low tone and sheds the high one", Math.abs(db(low) - db(0.25 * 0.5)) < 1 && db(low) - db(high) > 30, { low: db(low).toFixed(1), high: db(high).toFixed(1) });
    assert("5.1: a source straight ahead plays from the centre", Math.abs(centre - 0.25) < 0.01 && left < centre * 0.01, { centre, left });
  }
  {
    const ctx = new OfflineAudioContext(6, RATE / 2, RATE);
    const r = new Renderer(ctx);
    await r.load();
    r.configure({ quality: "enhanced", output: "5.1", dynamics: "home", clarity: false });
    const gain = ctx.createGain();
    const bed = r.bed(1, gain, "voice");
    const src = sine(ctx, 400, 0.3);
    src.connect(bed.input);
    src.start();
    const rendered = await ctx.startRendering();
    const trim = 10 ** (-DYNAMICS.home.makeup / 20);
    const levels = [0, 1, 2, 3, 4, 5].map((c) => rms(rendered.getChannelData(c), RATE / 8) * trim);
    assert("5.1: voice-over sits on the centre channel", Math.abs(levels[2] - 0.3 / Math.SQRT2) < 0.01 && Math.max(levels[0], levels[1], levels[4], levels[5]) < 1e-4, levels);
  }

  // 3. The limiter: a sine 6 dB over full scale comes out at or under
  // -1 dBTP; a quiet one passes untouched.
  for (const [label, amp, freq] of [["loud", 2, 997], ["loud, near Nyquist/4", 2, 11025], ["quiet", 0.1, 997]]) {
    const ctx = new OfflineAudioContext(2, RATE, RATE);
    await addModule(ctx, "limiter");
    const lim = new AudioWorkletNode(ctx, "true-peak-limiter", { outputChannelCount: [2], processorOptions: { ceiling: -1 } });
    const src = sine(ctx, freq, amp, Math.PI / 4);
    src.connect(lim);
    lim.connect(ctx.destination);
    src.start();
    const out = await ctx.startRendering();
    const chans = [out.getChannelData(0).slice(RATE / 10), out.getChannelData(1).slice(RATE / 10)];
    const tp = db(truePeak(chans));
    if (amp > 1) assert(`limiter (${label}): true peak at or under -1 dBTP`, tp <= -0.9, { truePeakDb: +tp.toFixed(2) });
    else assert(`limiter (${label}): passes untouched`, Math.abs(rms(chans[0]) - amp / Math.SQRT2) < 1e-3, { rms: rms(chans[0]) });
  }

  // 4. The loudness meter on EBU Tech 3341's sine cases, and its true peak.
  const meterRun = async (seconds, make) => {
    const ctx = new OfflineAudioContext(2, Math.round(RATE * seconds), RATE);
    await addModule(ctx, "meter");
    const meter = new AudioWorkletNode(ctx, "loudness-meter", { numberOfOutputs: 0, channelCount: 2, channelCountMode: "explicit" });
    const readings = [];
    meter.port.onmessage = (e) => readings.push(e.data);
    const merge = ctx.createChannelMerger(2);
    make(ctx, merge);
    merge.connect(meter);
    await ctx.startRendering();
    await new Promise((r) => setTimeout(r, 300));
    return readings;
  };
  for (const level of [-23, -33]) {
    const readings = await meterRun(20, (ctx, merge) => {
      const a = sine(ctx, 1000, 10 ** (level / 20));
      const b = sine(ctx, 1000, 10 ** (level / 20));
      a.connect(merge, 0, 0);
      b.connect(merge, 0, 1);
      a.start();
      b.start();
    });
    const last = readings[readings.length - 1];
    assert(`meter: stereo 1 kHz at ${level} dBFS reads ${level} LUFS (EBU 3341)`, last && Math.abs(last.integrated - level) < 0.2 && Math.abs(last.momentary - level) < 0.2, last);
  }
  {
    const readings = await meterRun(2, (ctx, merge) => {
      const a = sine(ctx, RATE / 4, 1, Math.PI / 4);
      a.connect(merge, 0, 0);
      a.start();
    });
    // Its samples peak at -3 dBFS; the wave between them reaches 0 dBTP.
    const tp = Math.max(...readings.map((r) => r.truePeak));
    const sp = Math.max(...readings.map((r) => r.channels[0]));
    assert("meter: true peak of a quarter-rate sine between samples", Math.abs(tp) < 0.5 && Math.abs(sp + 3.01) < 0.05, { truePeak: tp, samplePeak: sp });
  }

  // 5. The reverb: an impulse's tail decays at the decay time set (T60 from
  // the Schroeder integral's -5 to -25 dB slope), and the channels differ.
  {
    const decay = 1.6;
    const ctx = new OfflineAudioContext(4, RATE * 3, RATE);
    await addModule(ctx, "reverb");
    const verb = new AudioWorkletNode(ctx, "fdn-reverb", {
      outputChannelCount: [4],
      processorOptions: { channels: 4, params: { room: 1, roomHF: 1, decay, hfRatio: 1, reflections: 0, reverb: 1, reverbDelay: 0.01, diffusion: 1, density: 1 } },
    });
    const imp = ctx.createBuffer(1, RATE, RATE);
    imp.getChannelData(0)[0] = 1;
    const src = ctx.createBufferSource();
    src.buffer = imp;
    src.connect(verb);
    verb.connect(ctx.destination);
    src.start();
    const out = await ctx.startRendering();
    const x = out.getChannelData(0);
    const energy = new Float64Array(x.length);
    let acc = 0;
    for (let i = x.length - 1; i >= 0; i--) {
      acc += x[i] * x[i];
      energy[i] = acc;
    }
    const at = (dB) => energy.findIndex((e) => 10 * Math.log10(e / energy[0]) < dB);
    const t60 = ((at(-25) - at(-5)) / RATE) * 3;
    const y = out.getChannelData(1);
    let xy = 0;
    let xx = 0;
    let yy = 0;
    for (let i = 0; i < RATE; i++) {
      xy += x[i] * y[i];
      xx += x[i] * x[i];
      yy += y[i] * y[i];
    }
    const corr = xy / Math.sqrt(xx * yy);
    assert("reverb: tail decays at the set decay time", Math.abs(t60 - decay) / decay < 0.3, { t60: +t60.toFixed(2), decay });
    assert("reverb: channels are decorrelated", Math.abs(corr) < 0.5, { corr: +corr.toFixed(3) });
  }

  // 6. The engine: voices ranked by priority, capped at the real-voice
  // limit, and bus gains (with the voice-over duck) applied.
  {
    const ctx = new OfflineAudioContext(2, RATE, RATE);
    const pcm = new Int16Array(RATE / 2);
    for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(8000 * Math.sin((2 * Math.PI * 300 * i) / 24000));
    const manifest = {
      objects: [
        { type: "Bus", Name: "0x00000001", Gain: 1 },
        { type: "Bus", Name: "0x00000002", Gain: 0.5, Parent: "0x00000001" },
        { type: "Bus", Name: "0x00000003", Gain: 0.6, Parent: "0x00000001", AttenuateBus: "0x00000002", Attenuation: 0.5, AttenuationLeadIn: 0.5, AttenuationLeadOut: 0.5 },
        ...Array.from({ length: 20 }, (_, i) => ({ type: "SoundProperties", Name: `0x0000a${String(i).padStart(3, "0")}`, Priority: i / 20, Gain: 1, Bus: "0x00000002", Mode3D: 0, SampleList: [["0x00000100", 1]] })),
        { type: "SoundProperties", Name: "0x0000b000", Priority: 1, Gain: 1, Bus: "0x00000003", Mode3D: 0, SampleList: [["0x00000100", 1]] },
      ],
      samples: { "0x00000100": [0, pcm.length, 24000] },
    };
    const data = new SoundData(manifest, pcm.buffer);
    const engine = new AudioEngine({ context: ctx, maxVoices: 8 });
    await engine.renderer.load();
    engine.renderer.configure({ quality: "original", output: "stereo", dynamics: "home" });
    engine.setData(data);
    for (let i = 0; i < 20; i++) engine.play(`0x0000a${String(i).padStart(3, "0")}`, { offline: true });
    engine.update(0.016);
    const real = engine.voices.filter((v) => v.real);
    const lowestReal = Math.min(...real.map((v) => v.priority));
    assert("engine: no more real voices than the limit", engine.stats.real === 8 && real.length === 8, engine.stats);
    assert("engine: the highest priorities are the ones heard", lowestReal >= 12 / 20 - 1e-9, { lowestReal });
    assert("engine: bus gain is the product up the tree", Math.abs(engine.busGain("0x00000002") - 0.5) < 1e-9);
    engine.play("0x0000b000", { offline: true });
    for (let i = 0; i < 40; i++) engine.update(1 / 60);
    assert("engine: voice-over ducks its target bus to Attenuation over its lead-in", Math.abs(engine.busGain("0x00000002") - 0.25) < 1e-6, { gain: engine.busGain("0x00000002") });
  }

  // 8. The original mode's panning, OpenAL Soft 1.20.1's: the worklet's
  // law against the decoder tables, and a few gains worked out by hand from
  // OpenAL Soft's source (stereo ahead 0.5957 a side; 5.1 ahead 0.5174 on
  // each front and -0.0357 on each side; 7.1 hard left 0.9481 on the left
  // side speaker).
  {
    const level = async (layout, x, y, z) => {
      const N = LAYOUTS[layout].channels;
      const ctx = new OfflineAudioContext(N, 512, RATE);
      ctx.destination.channelInterpretation = "discrete";
      await addModule(ctx, "spatial-mixer");
      const node = new AudioWorkletNode(ctx, "spatial-mixer", {
        numberOfInputs: 1,
        numberOfOutputs: 2,
        outputChannelCount: [N, 1],
        processorOptions: { channels: N, law: "alsoft", decoder: ALSOFT[layout], speakers: LAYOUTS[layout].speakers, lfe: LAYOUTS[layout].lfe, params: [0, x, y, z, 0, 0] },
      });
      node.connect(ctx.destination, 0);
      const one = new ConstantSourceNode(ctx, { offset: 1 });
      one.connect(node, 0, 0);
      one.start();
      const rendered = await ctx.startRendering();
      return Array.from({ length: N }, (_, c) => rendered.getChannelData(c)[300]);
    };
    let worst = 0;
    for (const layout of ["stereo", "5.1", "7.1", "7.1.4"]) {
      for (const az of [0, 25, -40, 90, -120, 180]) {
        for (const el of [0, 30]) {
          const a = (az * Math.PI) / 180;
          const e = (el * Math.PI) / 180;
          const dir = [-Math.sin(a) * Math.cos(e), Math.sin(e), -Math.cos(a) * Math.cos(e)];
          const got = await level(layout, ...dir);
          const want = alsoftGains(layout, ...dir);
          for (let c = 0; c < got.length; c++) worst = Math.max(worst, Math.abs(got[c] - want[c]));
        }
      }
    }
    assert("original: the mixer pans as OpenAL Soft's decoders", worst < 1e-5, { worst });
    const near = (v, x) => Math.abs(v - x) < 2e-4;
    const s = alsoftGains("stereo", 0, 0, -1);
    const f51 = alsoftGains("5.1", 0, 0, -1);
    const l71 = alsoftGains("7.1", -1, 0, 0);
    assert("original: OpenAL Soft's gains by hand", near(s[0], 0.5957) && near(s[1], 0.5957) && near(f51[0], 0.5174) && near(f51[4], -0.0357) && near(l71[6], 0.9481) && f51[2] === 0, { s, f51, l71 });
  }
  {
    // The front stabilizer: 256 samples late on every channel; a side signal
    // (L = -R) left alone; a low mid signal (L = R) a third of the way to the
    // centre (centre 0.5, fronts 0.866 of it).
    const stabilize = async (feed) => {
      const ctx = new OfflineAudioContext(6, RATE / 4, RATE);
      ctx.destination.channelInterpretation = "discrete";
      await addModule(ctx, "stabilizer");
      const node = new AudioWorkletNode(ctx, "front-stabilizer", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [6], channelCount: 6, channelCountMode: "explicit", channelInterpretation: "discrete", processorOptions: { channels: 6, left: 0, right: 1, centre: 2 } });
      const merge = new ChannelMergerNode(ctx, { numberOfInputs: 6 });
      merge.connect(node);
      node.connect(ctx.destination);
      feed(ctx, merge);
      return ctx.startRendering();
    };
    const click = await stabilize((ctx, merge) => {
      const buffer = ctx.createBuffer(1, ctx.length, RATE);
      buffer.getChannelData(0)[1000] = 1;
      const src = new AudioBufferSourceNode(ctx, { buffer });
      src.connect(merge, 0, 4);
      src.start();
    });
    const at = click.getChannelData(4).findIndex((v) => Math.abs(v) > 0.5);
    assert("original: the stabilizer delays the other channels 256 samples", at === 1256, { at });
    const side = await stabilize((ctx, merge) => {
      const src = sine(ctx, 200, 0.5);
      const invert = new GainNode(ctx, { gain: -1 });
      src.connect(merge, 0, 0);
      src.connect(invert);
      invert.connect(merge, 0, 1);
      src.start();
    });
    const a = 0.5 / Math.SQRT2;
    const sideLevels = [0, 1, 2].map((ch) => rms(side.getChannelData(ch), RATE / 8) / a);
    assert("original: the stabilizer leaves a side signal alone", Math.abs(sideLevels[0] - 1) < 0.01 && Math.abs(sideLevels[1] - 1) < 0.01 && sideLevels[2] < 1e-3, sideLevels);
    const mid = await stabilize((ctx, merge) => {
      const src = sine(ctx, 200, 0.5);
      src.connect(merge, 0, 0);
      src.connect(merge, 0, 1);
      src.start();
    });
    const midLevels = [0, 1, 2].map((ch) => rms(mid.getChannelData(ch), RATE / 8) / a);
    assert("original: the stabilizer moves a third of a low centre sound to the centre", Math.abs(midLevels[0] - 0.866) < 0.02 && Math.abs(midLevels[1] - 0.866) < 0.02 && Math.abs(midLevels[2] - 0.5) < 0.02, midLevels);
  }
  {
    // The whole original chain on 5.1: a 2D sound (ahead) lands on the front
    // pair at 0.5174 each and, through the stabilizer, the centre gets 0.2587
    // and the pair 0.4481 (all of a tone well under 5 kHz).
    const ctx = new OfflineAudioContext(6, RATE / 2, RATE);
    const r = new Renderer(ctx);
    await r.load();
    r.configure({ quality: "original", output: "5.1", dynamics: "home" });
    const out = r.voice({ mode3D: 0 });
    const src = sine(ctx, 200, 0.25);
    out.attach(src);
    out.update(null, 0, 0, { send: 0 });
    r.flush();
    src.start();
    const rendered = await ctx.startRendering();
    const trim = 10 ** (-(DYNAMICS.home.makeup + ORIGINAL_LEVEL["5.1"]) / 20) / (0.25 / Math.SQRT2);
    const levels = [0, 1, 2, 3, 4, 5].map((c) => rms(rendered.getChannelData(c), RATE / 4) * trim);
    assert("original: a 2D sound on 5.1 as OpenAL Soft plays it", Math.abs(levels[0] - 0.4481) < 0.01 && Math.abs(levels[1] - 0.4481) < 0.01 && Math.abs(levels[2] - 0.2587) < 0.01 && levels[3] < 1e-4 && Math.abs(levels[4] - 0.0357) < 0.005, levels.map((x) => +x.toFixed(4)));
  }

  // 9. The enhanced mode's clarity stage (worklets/clarity.js): a top
  // octave from the 2.5-7 kHz band, in proportion; nothing added from below
  // it; attacks lifted a few dB while a steady sound is left alone; the LFE
  // untouched.
  {
    const clarity = async (feed, seconds = 1) => {
      const ctx = new OfflineAudioContext(6, RATE * seconds, RATE);
      ctx.destination.channelInterpretation = "discrete";
      await addModule(ctx, "clarity");
      const node = new AudioWorkletNode(ctx, "clarity", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [6], channelCount: 6, channelCountMode: "explicit", channelInterpretation: "discrete", processorOptions: { channels: 6, skip: 3 } });
      const merge = new ChannelMergerNode(ctx, { numberOfInputs: 6 });
      merge.connect(node);
      node.connect(ctx.destination);
      feed(ctx, merge);
      return ctx.startRendering();
    };
    // A tone's amplitude at a frequency, by correlation, over [from, to).
    const at = (x, f, from, to) => {
      let s = 0;
      let c = 0;
      for (let i = from; i < to; i++) {
        s += x[i] * Math.sin((2 * Math.PI * f * i) / RATE);
        c += x[i] * Math.cos((2 * Math.PI * f * i) / RATE);
      }
      return (2 * Math.hypot(s, c)) / (to - from);
    };
    const tone = (f) => (ctx, merge) => {
      const src = sine(ctx, f, 0.25);
      src.connect(merge, 0, 0);
      src.connect(merge, 0, 3);
      src.start();
    };
    const high = await clarity(tone(4000));
    const h = high.getChannelData(0);
    const fundamental = at(h, 4000, RATE / 2, RATE);
    const octave = at(h, 8000, RATE / 2, RATE);
    assert("clarity: a 4 kHz tone gains an octave above it, well under it", db(octave / fundamental) > -32 && db(octave / fundamental) < -16 && Math.abs(db(fundamental / 0.25)) < 0.3, { octave: db(octave / fundamental).toFixed(1), fundamental: db(fundamental / 0.25).toFixed(2) });
    const low = await clarity(tone(1000));
    const l = low.getChannelData(0);
    let above = 0;
    for (const f of [2000, 3000, 4000, 6000, 8000]) above = Math.max(above, at(l, f, RATE / 2, RATE));
    assert("clarity: nothing added to a tone under the band", db(above / 0.25) < -50 && Math.abs(db(at(l, 1000, RATE / 2, RATE) / 0.25)) < 0.3, { above: db(above / 0.25).toFixed(1) });
    const lfe = high.getChannelData(3);
    let worst = 0;
    const input = sine(new OfflineAudioContext(1, RATE, RATE), 4000, 0.25).buffer.getChannelData(0);
    for (let i = 0; i < RATE; i++) worst = Math.max(worst, Math.abs(lfe[i] - input[i]));
    assert("clarity: the LFE passes untouched", worst < 1e-6, { worst });
    // Noise bursts of 60 ms every half second, over a steady bed 20 dB down.
    const bursts = await clarity((ctx, merge) => {
      const buffer = ctx.createBuffer(1, ctx.length, RATE);
      const d = buffer.getChannelData(0);
      let seed = 7;
      const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647) * 2 - 1;
      for (let i = 0; i < d.length; i++) d[i] = 0.02 * rand() + ((i % (RATE / 2)) < RATE * 0.06 ? 0.2 * rand() : 0);
      const src = new AudioBufferSourceNode(ctx, { buffer });
      src.connect(merge, 0, 0);
      src.connect(merge, 0, 1);
      src.start();
    }, 2);
    const out = bursts.getChannelData(0);
    // The same input, to compare against.
    const ref = new Float32Array(out.length);
    let seed = 7;
    const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647) * 2 - 1;
    for (let i = 0; i < ref.length; i++) ref[i] = 0.02 * rand() + ((i % (RATE / 2)) < RATE * 0.06 ? 0.2 * rand() : 0);
    const lift = (from, to) => db(rms(out, from, to) / rms(ref, from, to));
    const attack = lift(RATE * 1.5, RATE * 1.5 + RATE * 0.015);
    const bed = lift(RATE * 1.5 + RATE * 0.3, RATE * 1.5 + RATE * 0.45);
    assert("clarity: an attack lands harder; the bed between is left alone", attack > 1.5 && attack < 4.5 && Math.abs(bed) < 0.6, { attack: attack.toFixed(2), bed: bed.toFixed(2) });
  }

  // 7. The headphone decoder. The encoder's harmonics against the values
  // tools/make_binaural.py checked against spaudiopy (written with the
  // filters), spread keeping the energy, and the decoded ears: a source on
  // the left louder in the left ear, one ahead even at about -3 dB, a 2D
  // sound left as plain stereo. Skipped without the decoder's files.
  const decoderInfo = await fetch("/audio/hrtf/sadie2_d1_3oa.json")
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null);
  if (!decoderInfo) {
    results.push({ name: "headphone decoder (skipped: run tools/make_binaural.py)", ok: true });
    return results;
  }
  {
    const encode = async (front, left, up, spread) => {
      const ctx = new OfflineAudioContext(16, 256, RATE);
      ctx.destination.channelInterpretation = "discrete";
      await addModule(ctx, "spatial-mixer");
      const node = new AudioWorkletNode(ctx, "spatial-mixer", {
        numberOfInputs: 1,
        numberOfOutputs: 2,
        outputChannelCount: [16, 1],
        processorOptions: { channels: 16, law: "ambi3", speakers: [], lfe: -1, params: [0, -left, up, -front, spread, 0] },
      });
      node.connect(ctx.destination, 0);
      const one = new ConstantSourceNode(ctx, { offset: 1 });
      one.connect(node, 0, 0);
      one.start();
      const rendered = await ctx.startRendering();
      return Array.from({ length: 16 }, (_, c) => rendered.getChannelData(c)[200]);
    };
    let worst = 0;
    for (const t of decoderInfo.test) {
      const got = await encode(...t.direction, 0);
      for (let c = 0; c < 16; c++) worst = Math.max(worst, Math.abs(got[c] - t.harmonics[c]));
    }
    assert("headphones: the encoder's harmonics match spaudiopy's", worst < 1e-5, { worst });
    const energy = (v) => v.reduce((s, x) => s + x * x, 0);
    const point = energy(await encode(0.6, 0.48, 0.64, 0));
    const spread = energy(await encode(0.6, 0.48, 0.64, 0.5));
    assert("headphones: spread keeps a source's energy", Math.abs(point - 16 / (4 * Math.PI)) < 1e-4 && Math.abs(spread - point) < 1e-4, { point, spread });
  }
  {
    // Each ear's level (dB) of a tone from a direction, through the whole
    // enhanced headphone chain.
    const ears = async (local, mode3D = 1, freq = 1000) => {
      const ctx = new OfflineAudioContext(2, RATE / 2, RATE);
      const r = new Renderer(ctx);
      await r.load("/audio");
      r.configure({ quality: "enhanced", output: "headphones", dynamics: "home", clarity: false });
      const out = r.voice({ mode3D });
      const src = sine(ctx, freq, 0.25);
      out.attach(src);
      out.update(local, 20, 0, { cutoff: 20000, send: 0 });
      r.flush();
      src.start();
      const rendered = await ctx.startRendering();
      const trim = 10 ** (-DYNAMICS.home.makeup / 20) / (0.25 / Math.SQRT2);
      const level = (c) => db(rms(rendered.getChannelData(c), RATE / 8) * trim);
      return { binaural: r.binaural, left: level(0), right: level(1) };
    };
    const first = await ears({ x: 0, y: 0, z: -1 });
    assert("headphones: the ambisonic decoder is in use", first.binaural, first);
    // Against the decoder's own levels (the browser's chain adds nothing);
    // the head's measured levels are in the file for comparison.
    let worst = 0;
    const seen = [];
    for (const t of decoderInfo.levels.filter((l) => l.freq === 1000 || l.freq === 4000)) {
      const [front, left, up] = t.direction;
      const got = await ears({ x: -left, y: up, z: -front }, 1, t.freq);
      worst = Math.max(worst, Math.abs(got.left - t.decoded[0]), Math.abs(got.right - t.decoded[1]));
      seen.push(`${t.azimuth}/${t.elevation} ${t.freq} Hz: ${got.left.toFixed(1)} ${got.right.toFixed(1)} (head ${t.measured[0]} ${t.measured[1]})`);
    }
    assert("headphones: each ear's level per direction is the decoder's", worst < 0.3, { worst, seen });
    const side = await ears({ x: -1, y: 0, z: 0 }, 1, 4000);
    assert("headphones: a source on the left is 10+ dB louder in the left ear at 4 kHz", side.left - side.right > 10, side);
    const flat = await ears(null, 0);
    assert("headphones: a 2D sound stays plain stereo", Math.abs(flat.left + 3.01) < 0.05 && Math.abs(flat.right + 3.01) < 0.05, flat);
  }

  return results;
}
