import { Target } from "./automation.js";
import { ALSOFT, ALSOFT_STREAM_ANGLES, LAYOUTS, alsoftStereo, autoLayout } from "./layouts.js";
import { loadAudioDsp, masterRust, voicesRust } from "./wasm-dsp.js";

// Where sound goes once it leaves a voice: the output layout, the panning,
// the reverb, the LFE, and the master chain.
//
// Two renderings, switched live:
// - "original": what the Steam build's OpenAL Soft 1.20.1 output gives.
//   Stereo speakers: the light mix (buildLight), the game's voices summed
//   as a PC of 2004 summed them, each panned by OpenAL Soft's stereo law;
//   no reverb and no limiter, so it costs the audio thread little more than
//   the voices themselves. Surround: its own panning, 2D ambisonic decoders
//   of second order on 5.1, third on 7.1 (src/audio/layouts.js, ALSOFT),
//   and streams' channels at its fixed angles; its front stabilizer (the
//   Steam build's alsoft.ini turns it on), which alone feeds the centre. No
//   LFE. Headphones: OpenAL Soft turns on its HRTF when Windows reports
//   headphones, so every source and stream channel goes through an HRTF
//   (the browser's, standing in for OpenAL Soft's MIT KEMAR set). The
//   sub-level's reverb space, and a limiter in place of OpenAL Soft's.
// - "enhanced": the 2026 sound stage. Speakers: equal-power panning on
//   stereo, pairwise amplitude panning (VBAP) over the layout on surround,
//   the height ring on 7.1.4, spread for close sources, explosions sent to
//   the LFE below 120 Hz, voice-over on the centre, music upmixed to the
//   surrounds, ambience beds over the surround field. Enhanced
//   headphones: every 3D voice and ambience bed is encoded into
//   third-order ambisonics and decoded to the ears through a measured dummy
//   head (SADIE II's KU 100, MagLS; tools/make_binaural.py writes the
//   filters), one convolver per ambisonic channel and ear however many
//   voices play; 2D sounds and music stay plain stereo. Without the filters,
//   the browser's HRTF panner per voice. Speakers and headphones alike: air
//   absorption and occlusion (filters set per voice by the engine), zone
//   reverb, a dynamics profile, and a true-peak limiter at -1 dBTP.
//
// Every 3D voice takes one input slot of the spatial mixer worklet; the
// engine sends each slot's direction once a frame (flush()).

// Mixer inputs: the game hears at most 32 voices, a voice that stops keeps
// its input for its fade (a tenth of a second or so), and the beds take up
// to seven more (the original mode's music, quad ambience, and voice-over at
// OpenAL Soft's angles). Each input costs the browser something every block
// even unused (96 cost about 10 ms a second); past these, a voice falls back
// to a stereo panner.
export const SLOTS = 64;
// With the master chain in the mixer's module, a voice in the engine takes
// no input of the mixer's, and an input costs the browser something every
// block; a voice that keeps the browser's nodes takes one of these few (a
// stream's line said somewhere: a line or two at a time, then a panner).
const NODE_INPUTS = 8;
// The headphone decoder's files under the audio root (tools/make_binaural.py).
const DECODER = "hrtf/sadie2_d1_3oa";
const AMBI_CHANNELS = 16;
// An ambience bed's channels on headphones: virtual speakers at these
// azimuths (front pair, rear pair), slightly spread.
const BED_SPEAKERS = [30, -30, 110, -110];
const BED_SPREAD = 0.2;
// The enhanced mode's clarity EQ, ahead of the dynamics: [type, Hz, dB, Q].
// Many sounds at once pile up around 200 Hz (the game's mix has its most
// energy at 125 to 250 Hz, and almost none above 11 kHz); this takes a
// little out there and adds presence and air. worklets/clarity.js follows.
const CLARITY_EQ = [
  ["peaking", 220, -2, 0.9],
  ["peaking", 3200, 1.5, 0.8],
  ["highshelf", 8500, 3, 0.7],
];
// The clarity stage's output level (dB): it makes the mix about half a LU
// louder (more presence and air, harder attacks); this keeps the A/B with
// the original level (tests/audio-loudness.mjs).
const CLARITY_LEVEL = -0.5;
// The enhanced mode's room keeps out of the low end: its send is high-passed.
const REVERB_LOW_CUT = 180; // Hz

// The browser's HRTF panner plays a source ahead 0.9 dB over unity in each
// ear (pink noise, Chromium); this brings it to -3 dB an ear, where the
// stereo laws and the headphone decoder put a source ahead.
const HRTF_LEVEL = 10 ** (-3.9 / 20);
// The original mode's output level against the enhanced (dB), so an A/B
// hears the sound and not a level change. OpenAL Soft's decoders give a
// source ahead (where most of the game's sound is) less power than the
// enhanced mode's unit-power panning: on stereo 0.71 (1.5 dB under, as
// tests/audio-loudness.mjs measures over a battle), on 5.1 with the
// stabilizer 0.47, on 7.1 0.81.
export const ORIGINAL_LEVEL = { stereo: 1.5, "5.1": 3.3, "7.1": 0.9, "7.1.4": 0.9, headphones: 0 };
// A stream's channels to the enhanced mode's stereo speakers, [left, right]
// each: mono on both at equal power; a quad bed's back pair folded onto its
// own side, a little lower.
const STEREO_BEDS = {
  1: [[Math.SQRT1_2, Math.SQRT1_2]],
  2: [
    [1, 0],
    [0, 1],
  ],
  4: [
    [1, 0],
    [0, 1],
    [0.7, 0],
    [0, 0.7],
  ],
};
const LFE_CUTOFF = 120; // Hz
const LFE_SEND = 0.5; // an explosion's LFE send, linear
// A BiquadFilterNode's lowpass Q is in dB: -3.01 dB is a Butterworth section.
const BUTTERWORTH_Q = -3.0103;
// The voice engine's commands (rust/audio/src/voices.rs), each code then its
// slot and arguments.
const VOICE = { START: 1, GAIN_SET: 2, GAIN: 3, RATE: 4, CUTOFF: 5, SEND: 6, RELEASE: 7, STOP: 8, SIDE_SET: 9, SIDE: 10, CANCEL: 11, STOP_AT: 12 };
// A filter's kind as the master chain takes it (rust/audio/src/master.rs).
const FILTER_KINDS = { lowpass: 0, highpass: 1, peaking: 2, highshelf: 3 };

// The dynamics profiles. The game's mix is authored to full scale (a gun at
// arm's length peaks near 0 dBFS; a battle on Theed plays at about -14 LUFS
// before the master chain); each profile compresses (or not) and then sets
// the output level so that play lands on its loudness target: Home theater
// -24 LUFS integrated (ASWG-R001's home figure), Normal -20, Night (and
// headphones or laptop speakers) -18 with the range narrowed. makeup is that
// output level in dB, set from tests/audio-loudness.mjs (2026-10-08, the
// same 60 s of Theed in each mode, enhanced and original: Home -23.4 and
// -23.8 LUFS, loudness range 13 and 12 LU; Normal -19.6 and -19.7, 10 and 8
// LU; Night -18.8 and -18.5, 5 and 4 LU; true peak at or under -1 dBTP).
export const DYNAMICS = {
  // Home theater: the full range, as mixed.
  home: { label: "Home theater (full range)", threshold: 0, ratio: 1, knee: 0, attack: 0.003, release: 0.25, makeup: -10.2 },
  // Normal: gentle compression of the loudest moments, slow enough to let
  // each attack through (25 ms) so shots keep their punch.
  normal: { label: "Normal", threshold: -18, ratio: 2, knee: 8, attack: 0.025, release: 0.2, makeup: -2.3 },
  // Night: a narrow range, so quiet sounds carry and blasts don't startle.
  night: { label: "Night (narrow range)", threshold: -30, ratio: 5, knee: 10, attack: 0.004, release: 0.35, makeup: 10.6 },
};
// The light mix has no compressor: each profile is a level there (dB,
// after what enters the dynamics), set from tests/audio-loudness.mjs as the
// makeups are (2026-11-02, 60 s of Theed: Home -22.9 LUFS, Normal -18.8,
// Night -18.4, against the enhanced mode's -21.8, -18.7 and -18.2 on the
// same battle; true peak at most -1.3 dBTP, so the soft clip only guards).
export const LIGHT_LEVELS = { home: -10.2, normal: -6.1, night: -5.7 };

// The light mix's speaker gains carry a voice's level and its panning, eased
// as the engine eases a voice's level (its SMOOTH); a change of PAN_STEP
// of the gain or less (0.009 dB) isn't sent at all.
const LIGHT_TAU = 0.015; // s
const PAN_STEP = 1e-3;
// The light mix's overs: what passes full scale is clipped softly rather
// than by the output. Unity up to -1 dBFS, then a knee that reaches full
// scale only 6 dB over it (the shaper's input is halved, CLIP_RANGE, so
// its curve covers +-2).
const CLIP_KNEE = 10 ** (-1 / 20);
const CLIP_RANGE = 2;
let clipCurve = null;
export function softClip(x) {
  const a = Math.abs(x);
  return a <= CLIP_KNEE ? x : Math.sign(x) * (CLIP_KNEE + (1 - CLIP_KNEE) * Math.tanh((a - CLIP_KNEE) / (1 - CLIP_KNEE)));
}
function clipShape() {
  // An odd length puts a point at 0, so silence stays silence.
  if (!clipCurve) clipCurve = Float32Array.from({ length: 4097 }, (_, i) => softClip(CLIP_RANGE * ((2 * i) / 4096 - 1)));
  return clipCurve;
}

// The light mix's dynamics for a profile: its level, halved into the soft
// clip. { input, output, nodes }; tests/audio-loudness.mjs measures through
// the same.
export function lightDynamics(ctx, profile) {
  const level = ctx.createGain();
  level.gain.value = 10 ** ((LIGHT_LEVELS[profile] ?? LIGHT_LEVELS.normal) / 20) / CLIP_RANGE;
  const clip = new WaveShaperNode(ctx, { curve: clipShape() });
  level.connect(clip);
  return { input: level, output: clip, nodes: [level, clip] };
}

export class Renderer {
  constructor(ctx) {
    this.ctx = ctx;
    this.loaded = null;
    this.slots = [];
    this.batch = [];
    // voicesRust (unless ?voices=native; tests set it): the enhanced voices
    // render in the mixer's own voice engine (rust/audio/src/voices.rs)
    // instead of through a buffer source, a gain, a lowpass and a send each,
    // and the light mix's in a worklet of their own instead of a source and
    // two speaker gains each, to the same samples (tests/audio-checks.js);
    // `commands` is the frame's for it, `engineGen` counts the mixers built
    // (a voice of an older one is lost with it), and samples go to each
    // mixer once (`uploaded`).
    this.voicesRust = voicesRust();
    this.engine = false;
    // masterRust (unless ?master=native; tests set it): with the voice engine,
    // what follows the mixer renders in its module too: `master`, as built.
    this.masterRust = masterRust();
    this.master = false;
    this.engineGen = 0;
    this.commands = [];
    this.flushQueued = false;
    this.sampleIds = new WeakMap();
    this.nextSample = 0;
    this.uploaded = new Set();
    this.voices = new Set();
    this.beds = new Set();
    // The reverb's room as last set: a reverb built again starts in it.
    this.roomParams = null;
    this.config = { quality: "enhanced", output: "auto", dynamics: "normal", mono: false, clarity: true };
    this.decoder = null;
    this.onMeter = null;
    this.onLimiter = null;
  }

  // Load the worklets, and the headphone decoder from under `root` (the
  // audio folder) when given; until then voices fall back to stereo panners.
  // `config`, the settings about to be built: the light mix needs no
  // worklet, and a context that never loads one renders on the device's own
  // audio thread rather than handing every block across from a worklet
  // thread. A later graph that needs them loads them then (configure).
  load(root = null, config = null) {
    if (!this.loaded) {
      if (config) this.config = { ...this.config, ...config };
      const worklets = this.lightFor(this.config) && !this.voicesRust ? null : this.worklets();
      this.loaded = Promise.all([worklets, root ? this.loadDecoder(root) : null]).then(() => true);
    }
    return this.loaded;
  }

  worklets() {
    if (!this.modules) {
      const add = (file) => this.ctx.audioWorklet.addModule(new URL(`./worklets/${file}`, import.meta.url));
      // The worklets' DSP module (rust/audio) is compiled here and handed
      // to each processor, whose scope cannot fetch it; with ?audio=js
      // this.dsp stays null and the worklets run their JavaScript.
      this.modules = Promise.all([Promise.all(["spatial-mixer.js", "limiter.js", "reverb.js", "meter.js", "stabilizer.js", "clarity.js"].map(add)), loadAudioDsp()]).then(([, dsp]) => {
        this.dsp = dsp || null;
        this.workletsReady = true;
      });
    }
    return this.modules;
  }

  // The headphone decoder: per ambisonic channel, a stereo filter (left
  // ear, right ear). Missing files leave this.decoder null.
  async loadDecoder(root) {
    const ctx = this.ctx;
    try {
      const info = await fetch(`${root}/${DECODER}.json`).then((r) => (r.ok ? r.json() : null));
      if (!info) return null;
      const ears = await Promise.all(
        ["left", "right"].map((ear) =>
          fetch(`${root}/${DECODER}_${ear}.wav`)
            .then((r) => {
              if (!r.ok) throw new Error(`${ear} ear: ${r.status}`);
              return r.arrayBuffer();
            })
            .then((data) => ctx.decodeAudioData(data)),
        ),
      );
      if (ears.some((b) => b.numberOfChannels !== AMBI_CHANNELS)) throw new Error("not 16 channels");
      // decodeAudioData resamples to the context's rate, which scales a
      // filter's gain by the ratio of the rates; this undoes it.
      const k = (info.rate || 48000) / ctx.sampleRate;
      const filters = [];
      for (let c = 0; c < AMBI_CHANNELS; c++) {
        const buffer = ctx.createBuffer(2, ears[0].length, ctx.sampleRate);
        ears.forEach((ear, e) => {
          const d = buffer.getChannelData(e);
          d.set(ear.getChannelData(c));
          for (let i = 0; i < d.length; i++) d[i] *= k;
        });
        filters.push(buffer);
      }
      this.decoder = { info, filters };
    } catch (error) {
      console.warn(`audio: no headphone decoder (${error.message}); using the browser's HRTF`);
      this.decoder = null;
    }
    return this.decoder;
  }

  // The layout the output mode resolves to on this device.
  resolve(output) {
    const max = this.ctx.destination.maxChannelCount || 2;
    let mode = output === "auto" || !LAYOUTS[output] ? autoLayout(max) : output;
    if (LAYOUTS[mode].channels > max) mode = autoLayout(max);
    return mode;
  }

  // Settings to build: a graph that runs worklets the light mix never
  // loaded is built once they are (the light mix playing meanwhile), and
  // the promise of that is returned; null when built at once. With the
  // voice engine the light mix runs one too (unless ?voices=native).
  configure(config) {
    this.config = { ...this.config, ...config };
    if ((!this.lightFor(this.config) || this.voicesRust) && !this.workletsReady) return this.worklets().then(() => this.build());
    this.build();
    return null;
  }

  // Whether these settings are the light mix: the original, on stereo.
  lightFor(config) {
    return config.quality === "original" && this.resolve(config.output) === "stereo";
  }

  build() {
    const ctx = this.ctx;
    const config = this.config;
    this.teardown();
    const mode = this.resolve(config.output);
    const layout = LAYOUTS[mode];
    const N = layout.channels;
    const enhanced = config.quality !== "original";
    this.mode = mode;
    this.layout = layout;
    this.channels = N;
    this.enhanced = enhanced;
    // The original on stereo speakers is the light mix.
    this.light = !enhanced && mode === "stereo";
    this.pair = null;
    // (No voice engine without a mixer; set again below when built.)
    this.engine = false;
    this.master = false;
    this.nodeSlots = null;
    if (this.light) return this.buildLight();
    // Headphones: the ambisonic decoder when enhanced and loaded, else an
    // HRTF panner per voice.
    this.binaural = enhanced && mode === "headphones" && Boolean(this.decoder);
    this.hrtf = mode === "headphones" && !this.binaural;
    // The original on speakers pans as OpenAL Soft does.
    this.alsoft = !enhanced && mode !== "headphones" ? ALSOFT[mode] || null : null;
    const dest = ctx.destination;
    dest.channelCount = N;
    dest.channelCountMode = "explicit";
    dest.channelInterpretation = N > 2 ? "discrete" : "speakers";
    const discrete = (node, channels = N) => {
      node.channelCount = channels;
      node.channelCountMode = "explicit";
      node.channelInterpretation = channels > 2 ? "discrete" : "speakers";
      return node;
    };
    this.discrete = discrete;
    const nodes = [];
    const keep = (node) => {
      nodes.push(node);
      return node;
    };
    this.nodes = nodes;

    // Everything sums into mix: the mixer's speaker feeds, the beds, the
    // reverb, and the LFE; HRTF panners through their level.
    this.mix = keep(discrete(ctx.createGain()));
    this.hrtfOut = keep(ctx.createGain());
    this.hrtfOut.gain.value = HRTF_LEVEL;
    this.hrtfOut.connect(this.mix);
    const law = this.binaural ? "ambi3" : this.alsoft ? "alsoft" : mode === "stereo" || mode === "headphones" ? "panpot" : "vbap";
    const feeds = this.binaural ? AMBI_CHANNELS : N;
    // The voice engine needs the module, and takes the enhanced voices that
    // go to a mixer slot (not those of the browser's HRTF panners).
    this.engine = Boolean(this.dsp) && enhanced && !this.hrtf && this.voicesRust;
    // The master chain follows the engine into the module, on speakers; the
    // binaural decoder's convolvers, and the mono fold, keep their nodes.
    this.master = this.engine && this.masterRust && !this.binaural && !config.mono;
    this.engineGen += 1;
    this.commands.length = 0;
    this.uploaded = new Set();
    const master = this.master;
    this.mixer = keep(
      new AudioWorkletNode(ctx, "spatial-mixer", {
        numberOfInputs: master ? NODE_INPUTS + 2 : SLOTS,
        numberOfOutputs: master ? 1 : this.engine ? 3 : 2,
        outputChannelCount: master ? [feeds] : this.engine ? [feeds, 1, 1] : [feeds, 1],
        processorOptions: {
          slots: SLOTS,
          channels: feeds,
          law,
          speakers: layout.speakers,
          lfe: layout.lfe,
          decoder: this.alsoft,
          ...(this.dsp ? { dsp: this.dsp } : {}),
          ...(this.engine ? { engine: { q: BUTTERWORTH_Q } } : {}),
          ...(master ? { master: this.masterOptions(layout) } : {}),
        },
      }),
    );
    // The reverb's send, summed: one mono channel.
    this.reverbIn = keep(ctx.createGain());
    this.reverbIn.channelCount = 1;
    this.reverbIn.channelCountMode = "explicit";
    if (master) {
      // Everything after the mixer is in its module. What still comes
      // through the browser's nodes (a voice's own send, the beds, a voice
      // with no slot) joins there, through its last two inputs; its output
      // is the finished sound.
      this.reverbIn.connect(this.mixer, 0, NODE_INPUTS);
      this.mix.connect(this.mixer, 0, NODE_INPUTS + 1);
      this.mixer.connect(dest);
      this.mixer.port.onmessage = (e) => e.data?.reduction !== undefined && this.onLimiter?.(e.data);
      for (const v of this.voices) v.slot = -1;
      for (const b of this.beds) b.slots = [];
      this.slots = Array.from({ length: SLOTS }, (_, i) => SLOTS - 1 - i);
      // The slots with an input are the first (the last here).
      this.nodeSlots = this.slots.splice(SLOTS - NODE_INPUTS);
      this.lfe = layout.lfe >= 0;
      this.reverb = null;
      this.reverbHP = null;
      this.limiter = null;
      this.volume = null;
      this.preDynamics = null;
      this.output = this.mixer;
      if (this.meterOn) this.meter(true);
      for (const v of this.voices) v.rebuild();
      for (const b of this.beds) b.rebuild();
      return;
    }
    if (this.binaural) {
      // Each ambisonic channel through its pair of ear filters, summed.
      const split = keep(ctx.createChannelSplitter(AMBI_CHANNELS));
      this.mixer.connect(split, 0);
      this.decoder.filters.forEach((buffer, c) => {
        const ears = keep(new ConvolverNode(ctx, { buffer, disableNormalization: true }));
        split.connect(ears, c);
        ears.connect(this.mix);
      });
    } else {
      this.mixer.connect(this.mix, 0);
    }
    // The old graph's slots are gone with it.
    for (const v of this.voices) v.slot = -1;
    for (const b of this.beds) b.slots = [];
    this.slots = Array.from({ length: SLOTS }, (_, i) => SLOTS - 1 - i);

    // The LFE: what the mixer sends, low-passed (24 dB/octave), into the LFE
    // channel.
    this.lfe = null;
    if (enhanced && layout.lfe >= 0) {
      const a = keep(ctx.createBiquadFilter());
      const b = keep(ctx.createBiquadFilter());
      for (const f of [a, b]) {
        f.type = "lowpass";
        f.frequency.value = LFE_CUTOFF;
        f.Q.value = BUTTERWORTH_Q;
      }
      const route = keep(ctx.createChannelMerger(N));
      this.mixer.connect(a, 1);
      a.connect(b);
      b.connect(route, 0, layout.lfe);
      route.connect(this.mix);
      this.lfe = a;
    }

    // The reverb: one mono send in, every speaker out (the LFE left dry).
    // The engine's voices' sends, summed, are the mixer's third output.
    if (this.engine) this.mixer.connect(this.reverbIn, 2);
    const rooms = layout.speakers.map((s) => (s ? 1 : 0));
    this.reverb = keep(
      new AudioWorkletNode(ctx, "fdn-reverb", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [N],
        processorOptions: { channels: N, ...(this.roomParams ? { params: this.roomParams } : {}), ...(this.dsp ? { dsp: this.dsp } : {}) },
      }),
    );
    if (enhanced) {
      this.reverbHP = keep(ctx.createBiquadFilter());
      this.reverbHP.type = "highpass";
      this.reverbHP.frequency.value = REVERB_LOW_CUT;
      this.reverbHP.Q.value = BUTTERWORTH_Q;
      this.reverbHP.channelCount = 1;
      this.reverbHP.channelCountMode = "explicit";
      this.reverbIn.connect(this.reverbHP);
      this.reverbHP.connect(this.reverb);
    } else {
      this.reverbHP = null;
      this.reverbIn.connect(this.reverb);
    }
    if (layout.lfe >= 0) {
      const split = keep(ctx.createChannelSplitter(N));
      const merge = keep(ctx.createChannelMerger(N));
      this.reverb.connect(split);
      rooms.forEach((on, c) => on && split.connect(merge, c, c));
      merge.connect(this.mix);
    } else {
      this.reverb.connect(this.mix);
    }

    // The master chain: the original's front stabilizer on surround, the
    // enhanced mode's clarity, mono fold (an option), dynamics, the limiter,
    // and the master volume.
    let tail = this.mix;
    if (this.alsoft && layout.center >= 0) {
      const stabilizer = keep(
        new AudioWorkletNode(ctx, "front-stabilizer", {
          numberOfInputs: 1,
          numberOfOutputs: 1,
          outputChannelCount: [N],
          channelCount: N,
          channelCountMode: "explicit",
          channelInterpretation: "discrete",
          processorOptions: { channels: N, left: 0, right: 1, centre: layout.center, ...(this.dsp ? { dsp: this.dsp } : {}) },
        }),
      );
      tail.connect(stabilizer);
      tail = stabilizer;
    }
    if (!enhanced && ORIGINAL_LEVEL[mode]) {
      const level = keep(discrete(ctx.createGain()));
      level.gain.value = 10 ** (ORIGINAL_LEVEL[mode] / 20);
      tail.connect(level);
      tail = level;
    }
    if (enhanced && config.clarity !== false) {
      for (const [type, frequency, gain, Q] of CLARITY_EQ) {
        const eq = keep(discrete(ctx.createBiquadFilter()));
        eq.type = type;
        eq.frequency.value = Math.min(frequency, ctx.sampleRate * 0.45);
        eq.gain.value = gain;
        eq.Q.value = Q;
        tail.connect(eq);
        tail = eq;
      }
      const clarity = keep(
        new AudioWorkletNode(ctx, "clarity", {
          numberOfInputs: 1,
          numberOfOutputs: 1,
          outputChannelCount: [N],
          channelCount: N,
          channelCountMode: "explicit",
          channelInterpretation: "discrete",
          processorOptions: { channels: N, skip: layout.lfe, ...(this.dsp ? { dsp: this.dsp } : {}) },
        }),
      );
      tail.connect(clarity);
      const level = keep(discrete(ctx.createGain()));
      level.gain.value = 10 ** (CLARITY_LEVEL / 20);
      clarity.connect(level);
      tail = level;
    }
    if (config.mono) {
      const fold = keep(ctx.createGain());
      fold.channelCount = 1;
      fold.channelCountMode = "explicit";
      fold.channelInterpretation = "speakers";
      tail.connect(fold);
      // The mono sum plays from the left and right speakers alike.
      const spread = keep(ctx.createChannelMerger(N));
      fold.connect(spread, 0, 0);
      fold.connect(spread, 0, 1);
      tail = spread;
    }
    // What enters the dynamics (for tests/audio-loudness.mjs).
    this.preDynamics = tail;
    // The profile is a listening choice, so both modes take it alike (an A/B
    // hears the sound, not the processing); Home theater leaves the original
    // as it was mixed.
    const d = DYNAMICS[config.dynamics] || DYNAMICS.normal;
    this.limiter = keep(
      new AudioWorkletNode(ctx, "true-peak-limiter", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [N],
        channelCount: N,
        channelCountMode: "explicit",
        channelInterpretation: "discrete",
        processorOptions: { ceiling: -1, release: enhanced ? 0.08 : 0.06, dynamics: { ...d, skip: layout.lfe }, ...(this.dsp ? { dsp: this.dsp } : {}) },
      }),
    );
    tail.connect(this.limiter);
    this.limiter.port.onmessage = (e) => this.onLimiter?.(e.data);
    this.volume = keep(discrete(ctx.createGain()));
    this.volume.gain.value = this.masterVolume ?? 1;
    this.limiter.connect(this.volume);
    this.volume.connect(dest);
    this.output = this.volume;
    if (this.meterOn) this.meter(true);

    // Voices and beds already playing move to the new graph.
    for (const v of this.voices) v.rebuild();
    for (const b of this.beds) b.rebuild();
  }

  // The master chain for the mixer's module, the same parameters as the
  // nodes and worklets of build() take below (the enhanced mode's).
  masterOptions(layout) {
    const ctx = this.ctx;
    const config = this.config;
    const clarity = config.clarity !== false;
    const dynamics = DYNAMICS[config.dynamics] || DYNAMICS.normal;
    return {
      ceiling: -1,
      release: 0.08,
      dynamics: { ...dynamics, skip: layout.lfe },
      room: this.roomParams,
      highpass: [REVERB_LOW_CUT, BUTTERWORTH_Q],
      // The channels the reverb reaches, a bit each.
      rooms: layout.lfe >= 0 ? layout.speakers.reduce((mask, s, c) => (s ? mask | (1 << c) : mask), 0) : -1,
      lfe: layout.lfe >= 0 ? [layout.lfe, LFE_CUTOFF, BUTTERWORTH_Q] : null,
      eq: clarity ? CLARITY_EQ.map(([type, frequency, gain, Q]) => [FILTER_KINDS[type], Math.min(frequency, ctx.sampleRate * 0.45), gain, Q]) : [],
      clarity: clarity ? { skip: layout.lfe, level: 10 ** (CLARITY_LEVEL / 20) } : null,
      volume: this.masterVolume ?? 1,
      time: ctx.currentTime,
    };
  }

  // The original on stereo speakers, light: the game's voices summed as a
  // PC of 2004 summed them, each a gain to each speaker carrying its level
  // and OpenAL Soft's stereo law for its direction, so the image is the
  // Steam build's; streams' channels at its fixed angles; the level the
  // dynamics profile plays at, and overs clipped softly. Rust mixes the
  // voices in a worklet by default; browser sources and gains are the
  // fallback. Neither path needs reverb, a decoder or limiter lookahead.
  buildLight() {
    const ctx = this.ctx;
    const config = this.config;
    const N = 2;
    this.binaural = false;
    this.hrtf = false;
    this.alsoft = null;
    this.mixer = null;
    this.hrtfOut = null;
    this.lfe = null;
    this.reverb = null;
    this.reverbIn = null;
    this.reverbHP = null;
    this.limiter = null;
    this.batch.length = 0;
    const dest = ctx.destination;
    dest.channelCount = N;
    dest.channelCountMode = "explicit";
    dest.channelInterpretation = "speakers";
    const discrete = (node, channels = N) => {
      node.channelCount = channels;
      node.channelCountMode = "explicit";
      node.channelInterpretation = channels > 2 ? "discrete" : "speakers";
      return node;
    };
    this.discrete = discrete;
    const nodes = [];
    const keep = (node) => {
      nodes.push(node);
      return node;
    };
    this.nodes = nodes;

    // Every voice's two speaker gains sum into the pair, which goes into
    // the mix with the beds, at the original's level on stereo.
    this.mix = keep(discrete(ctx.createGain()));
    this.mix.gain.value = 10 ** (ORIGINAL_LEVEL.stereo / 20);
    this.pair = keep(ctx.createChannelMerger(N));
    this.pair.connect(this.mix);
    // No mixer slots here.
    for (const v of this.voices) v.slot = -1;
    for (const b of this.beds) b.slots = [];
    this.slots = [];
    // The voice engine (unless ?voices=native): the voices play in the module
    // (rust/audio/src/voices.rs, render_light), each source and its two
    // speaker gains in one worklet whose output joins the mix as the pair
    // does; a voice with no slot left keeps the browser's nodes.
    this.engine = Boolean(this.dsp) && this.voicesRust;
    this.engineGen += 1;
    this.commands.length = 0;
    this.uploaded = new Set();
    if (this.engine) {
      this.mixer = keep(
        new AudioWorkletNode(ctx, "voice-engine", {
          numberOfInputs: 0,
          numberOfOutputs: 1,
          outputChannelCount: [N],
          processorOptions: { dsp: this.dsp, slots: SLOTS },
        }),
      );
      this.mixer.connect(this.mix);
      this.slots = Array.from({ length: SLOTS }, (_, i) => SLOTS - 1 - i);
    }

    let tail = this.mix;
    if (config.mono) {
      const fold = keep(ctx.createGain());
      fold.channelCount = 1;
      fold.channelCountMode = "explicit";
      fold.channelInterpretation = "speakers";
      tail.connect(fold);
      const spread = keep(ctx.createChannelMerger(N));
      fold.connect(spread, 0, 0);
      fold.connect(spread, 0, 1);
      tail = spread;
    }
    this.preDynamics = tail;
    // The profile as a level, and the soft clip.
    const dynamics = lightDynamics(ctx, config.dynamics);
    dynamics.nodes.forEach(keep);
    tail.connect(discrete(dynamics.input));
    this.volume = keep(discrete(ctx.createGain()));
    this.volume.gain.value = this.masterVolume ?? 1;
    dynamics.output.connect(this.volume);
    this.volume.connect(dest);
    this.output = this.volume;
    if (this.meterOn) this.meter(true);

    for (const v of this.voices) v.rebuild();
    for (const b of this.beds) b.rebuild();
  }

  teardown() {
    for (const node of this.nodes || []) {
      try {
        node.disconnect();
      } catch {
        // already gone
      }
    }
    this.nodes = [];
    this.meterNode = null;
  }

  setVolume(v) {
    this.masterVolume = v;
    if (this.master) this.mixer.port.postMessage({ type: "volume", value: v, time: this.ctx.currentTime });
    else this.volume?.gain.setTargetAtTime(v, this.ctx.currentTime, 0.05);
  }

  // A loudness meter on the output (for the overlay and the tests).
  meter(on) {
    this.meterOn = on;
    if (on && !this.workletsReady) {
      this.worklets().then(() => this.meter(this.meterOn), () => {});
      return;
    }
    if (on && !this.meterNode) {
      const N = this.channels;
      const weights = this.layout.names.map((n) => (n === "LFE" ? 0 : /^(SL|SR|BL|BR|TFL|TFR|TBL|TBR)$/.test(n) ? 1.41 : 1));
      this.meterNode = new AudioWorkletNode(this.ctx, "loudness-meter", {
        numberOfInputs: 1,
        numberOfOutputs: 0,
        channelCount: N,
        channelCountMode: "explicit",
        channelInterpretation: "discrete",
        processorOptions: { weights, ...(this.dsp ? { dsp: this.dsp } : {}) },
      });
      this.meterNode.port.onmessage = (e) => this.onMeter?.(e.data);
      this.output.connect(this.meterNode);
      this.nodes.push(this.meterNode);
    } else if (!on && this.meterNode) {
      this.meterNode.disconnect();
      this.meterNode = null;
    }
  }

  setRoom(params) {
    this.roomParams = { ...this.roomParams, ...params };
    if (this.master) this.mixer.port.postMessage({ type: "room", params });
    else this.reverb?.port.postMessage(params);
  }

  // A voice's way out. Returns { attach(node), update(...), dispose() }.
  // `own`: the caller plays it through a node of its own (attach), a
  // stream's line said somewhere or the audio panel's test, so it takes
  // the browser's nodes where the voice engine plays the others.
  voice(voice, own = false) {
    const out = new VoiceOut(this, voice, own);
    this.voices.add(out);
    out.rebuild();
    return out;
  }

  // A stream's channels in: kind "music" (stereo, upmixed to the surrounds
  // when enhanced), "voice" (the centre when enhanced), "ambience" (a quad
  // bed: front pair and rear pair), or "plain".
  bed(channels, gain, kind = "plain") {
    const out = new BedOut(this, channels, gain, kind);
    this.beds.add(out);
    out.rebuild();
    return out;
  }

  // Send this frame's slot directions to the mixer, and the voice
  // engine's commands.
  flush() {
    if (!this.mixer) return;
    if (this.batch.length) {
      const data = Float32Array.from(this.batch);
      this.mixer.port.postMessage({ type: "params", data }, [data.buffer]);
      this.batch.length = 0;
    }
    if (this.commands.length) {
      const data = Float64Array.from(this.commands);
      this.mixer.port.postMessage({ type: "voices", data }, [data.buffer]);
      this.commands.length = 0;
    }
  }

  // A voice engine command from outside the frame's update (a stop, a
  // release): sent once the current task is done, not a frame later.
  command(...values) {
    this.commands.push(...values);
    if (!this.flushQueued) {
      this.flushQueued = true;
      queueMicrotask(() => {
        this.flushQueued = false;
        this.flush();
      });
    }
  }

  // The engine's id for a sample, sent to this mixer the first time.
  engineSample(buffer) {
    let id = this.sampleIds.get(buffer);
    if (id === undefined) {
      id = this.nextSample++;
      this.sampleIds.set(buffer, id);
    }
    if (!this.uploaded.has(id)) {
      this.uploaded.add(id);
      const data = buffer.getChannelData(0).slice();
      this.mixer.port.postMessage({ type: "sample", id, rate: buffer.sampleRate, data }, [data.buffer]);
    }
    return id;
  }
}

// One voice's chain after its gain: an air-and-occlusion lowpass (enhanced),
// then a mixer slot, or an HRTF panner on headphones without the decoder,
// or a stereo panner (a 2D voice on headphones, or out of slots); and its
// reverb send. In the light mix, a gain to each speaker and nothing else:
// an engine voice has no gain of its own there (attach(source, false)), and
// its level (this.level) goes on these two with its panning.
class VoiceOut {
  constructor(renderer, voice, own = false) {
    this.r = renderer;
    this.voice = voice;
    this.own = own;
    this.slot = -1;
    this.nodes = [];
    this.source = null;
    this.leveled = true;
    this.level = 1;
    this.cutoff = 20000;
    this.sides = null;
  }

  rebuild() {
    const r = this.r;
    const ctx = r.ctx;
    // An engine voice's sound was in the mixer just replaced: its voice
    // starts it again (Voice.apply).
    if (this.engine) {
      this.lost = true;
      this.slot = -1;
      return;
    }
    this.release();
    this.nodes = [];
    // A new voice with a slot to go to renders in the voice engine (the
    // enhanced mode's or the light mix's): no nodes, its sample and
    // parameters sent as commands.
    if (r.engine && !this.own && !this.source && (this.voice.mode3D || !r.binaural) && r.slots.length) {
      this.engine = true;
      this.lost = false;
      this.filter = null;
      this.send = null;
      this.panner = null;
      this.pan = null;
      this.position = null;
      this.entry = null;
      this.head = null;
      this.sides = null;
      this.slot = r.slots.pop();
      // Targets (automation.js) reach the engine as commands.
      const param = (op) => ({ setTargetAtTime: (value, now, tau) => !this.lost && r.commands.push(op, this.slot, value, tau, now) });
      this.rateParam = param(VOICE.RATE);
      if (r.light) {
        // The light mix: its two speaker gains in the engine, which place()
        // and fade() set as they would the browser's (their first value,
        // 0, goes with the start: engineStart).
        this.sides = [0, 1].map((c) => ({ gain: this.sideParam(c) }));
        this.gains = [0, 0];
        this.levels = [0, 0];
        this.placed = false;
        this.sent = null;
        return;
      }
      r.mixer.port.postMessage({ type: "start", slot: this.slot });
      this.sent = new Float32Array(5).fill(NaN);
      this.sendLevel = new Target();
      this.sendLevel.hold(0);
      this.gainParam = param(VOICE.GAIN);
      this.sendParam = param(VOICE.SEND);
      return;
    }
    if (r.light) {
      this.filter = null;
      this.send = null;
      this.panner = null;
      this.pan = null;
      this.position = null;
      this.sent = null;
      this.entry = null;
      this.head = null;
      // Left and right, into the renderer's pair; the first gains a
      // direction gives apply at once (placed).
      this.sides = [0, 1].map((c) => {
        const side = ctx.createGain();
        side.gain.value = 0;
        side.connect(r.pair, 0, c);
        this.nodes.push(side);
        return side;
      });
      this.gains = [0, 0];
      this.levels = [0, 0];
      this.placed = false;
      if (this.source) this.attach(this.source, this.leveled);
      return;
    }
    this.sides = null;
    // A voice with no gain of its own (it started in the light mix) stays
    // out of the other graphs: its engine starts it again (Voice.apply).
    if (this.source && !this.leveled) {
      this.source = null;
      this.leveled = true;
    }
    const enhanced = r.enhanced;
    let head = null;
    if (enhanced) {
      this.filter = ctx.createBiquadFilter();
      this.filter.type = "lowpass";
      // The corner moves once a block (2.7 ms), not every sample: the browser
      // works an a-rate corner out sample by sample, several times the cost
      // while it glides, for a difference under -49 dB.
      this.filter.frequency.automationRate = "k-rate";
      this.filter.frequency.value = Math.min(this.cutoff, ctx.sampleRate / 2);
      this.filter.Q.value = BUTTERWORTH_Q;
      this.nodes.push(this.filter);
      head = this.filter;
    } else {
      this.filter = null;
    }
    this.send = ctx.createGain();
    this.send.gain.value = 0;
    this.nodes.push(this.send);
    this.send.connect(r.reverbIn);
    // The params set every frame, and the direction last sent to the mixer
    // (a slot's gains only change when it does).
    this.sendLevel = new Target();
    this.sendLevel.hold(0);
    this.pan = null;
    this.position = null;
    this.sent = null;
    // On headphones without the decoder, an HRTF panner: for 3D voices, and
    // in the original for 2D ones too (OpenAL Soft's HRTF takes them,
    // ahead).
    if (r.hrtf && (this.voice.mode3D || !r.enhanced)) {
      this.panner = new PannerNode(ctx, {
        panningModel: "HRTF",
        distanceModel: "linear",
        refDistance: 1,
        maxDistance: 1e5,
        rolloffFactor: 0,
      });
      this.nodes.push(this.panner);
      this.panner.connect(r.hrtfOut);
      this.entry = this.panner;
      this.position = [new Target(), new Target(), new Target()];
      for (const t of this.position) t.hold(0);
    } else {
      this.panner = null;
      // On binaural headphones a 2D voice stays plain stereo.
      this.slot = r.binaural && !this.voice.mode3D ? -1 : ((r.nodeSlots ?? r.slots).pop() ?? -1);
      if (this.slot < 0) {
        // Out of slots: a plain stereo panner.
        this.entry = ctx.createStereoPanner();
        this.nodes.push(this.entry);
        this.entry.connect(r.mix);
        this.pan = new Target();
        this.pan.hold(0);
      } else {
        this.entry = null;
        r.mixer.port.postMessage({ type: "start", slot: this.slot });
        this.sent = new Float32Array(5).fill(NaN);
      }
    }
    this.head = head;
    if (head) this.connectEntry(head);
    if (this.source) this.attach(this.source);
  }

  connectEntry(node) {
    const r = this.r;
    if (this.entry) node.connect(this.entry);
    else if (this.slot >= 0) node.connect(r.mixer, 0, this.slot);
  }

  // The voice's gain node feeds this; in the light mix the voice's source
  // itself can (leveled false: this.level is its level).
  attach(node, leveled = true) {
    this.source = node;
    if (this.sides) {
      this.leveled = leveled;
      for (const side of this.sides) node.connect(side);
      return;
    }
    if (this.head) node.connect(this.head);
    else this.connectEntry(node);
    node.connect(this.send);
  }

  // local: direction in the listener's frame (x right, y up, z back), or
  // null for a 2D voice placed by pan (-1 left .. 1 right).
  update(local, distance, pan, extra = {}) {
    const r = this.r;
    const now = r.ctx.currentTime;
    let x;
    let y;
    let z;
    if (local) {
      x = local.x;
      y = local.y;
      z = local.z;
    } else {
      // 2D: on the front arc by its pan.
      const a = (Math.max(-1, Math.min(1, pan || 0)) * Math.PI) / 2;
      x = Math.sin(a);
      y = 0;
      z = -Math.cos(a);
    }
    if (this.sides) {
      this.place(x, y, z, now);
      return;
    }
    // Close in, a point source spreads out (and its direction means little).
    // OpenAL Soft has no spread (its sources have no radius).
    let spread = extra.spread ?? 0;
    if (local && r.enhanced) spread = Math.max(spread, Math.min(Math.max(1 - distance / 1.5, 0), 1) * 0.6);
    const lfe = r.enhanced && r.lfe ? (extra.lfe ?? 0) * LFE_SEND : 0;
    if (this.panner) {
      const d = Math.max(distance, 1);
      this.position[0].set(this.panner.positionX, x * d, now, 0.01);
      this.position[1].set(this.panner.positionY, y * d, now, 0.01);
      this.position[2].set(this.panner.positionZ, z * d, now, 0.01);
    } else if (this.slot >= 0) {
      // The mixer works a slot's gains out again only from new values (as
      // it receives them, in single precision).
      const sent = this.sent;
      if (sent[0] !== Math.fround(x) || sent[1] !== Math.fround(y) || sent[2] !== Math.fround(z) || sent[3] !== Math.fround(spread) || sent[4] !== Math.fround(lfe)) {
        sent[0] = x;
        sent[1] = y;
        sent[2] = z;
        sent[3] = spread;
        sent[4] = lfe;
        r.batch.push(this.slot, x, y, z, spread, lfe);
      }
    } else if (this.entry?.pan) {
      this.pan.set(this.entry.pan, Math.max(-1, Math.min(1, x)), now, 0.015);
    }
    if ((this.filter || this.engine) && extra.cutoff !== undefined && Math.abs(extra.cutoff - this.cutoff) > 10) {
      this.cutoff = extra.cutoff;
      if (this.engine) r.commands.push(VOICE.CUTOFF, this.slot, Math.min(this.cutoff, r.ctx.sampleRate / 2), 0.03, now);
      else this.filter.frequency.setTargetAtTime(Math.min(this.cutoff, r.ctx.sampleRate / 2), now, 0.03);
    }
    if (extra.send !== undefined) this.sendLevel.set(this.engine ? this.sendParam : this.send.gain, extra.send, now, 0.05);
  }

  // The light mix: the level times OpenAL Soft's stereo gains for the
  // direction, the first at once (so an attack isn't softened), then eased
  // as the engine eases a voice's level; a change of a thousandth or less
  // isn't sent (PAN_STEP).
  place(x, y, z, now) {
    const gains = alsoftStereo(x, y, z, this.gains);
    for (let c = 0; c < 2; c++) {
      const g = gains[c] * this.level;
      if (!Number.isFinite(g)) continue;
      const param = this.sides[c].gain;
      const last = this.levels[c];
      if (!this.placed) param.value = g;
      else if (Math.abs(g - last) > PAN_STEP * Math.max(Math.abs(g), Math.abs(last)) || (g === 0) !== (last === 0)) param.setTargetAtTime(g, now, LIGHT_TAU);
      else continue;
      this.levels[c] = g;
    }
    this.placed = true;
  }

  // The light mix: a voice without a gain of its own fades out here (its
  // release).
  fade(now, tau) {
    for (const side of this.sides || []) {
      side.gain.cancelScheduledValues(now);
      side.gain.setTargetAtTime(0, now, tau);
    }
  }

  // The engine voice's sample from `offset` (s), at `rate`, its lowpass
  // open as the browser's starts. Each command carries the context time its
  // AudioParam call would have had (it decides the block it lands in).
  engineStart(buffer, offset, loop, rate) {
    const r = this.r;
    r.commands.push(VOICE.START, this.slot, r.engineSample(buffer), offset, loop ? 1 : 0, rate, Math.min(this.cutoff, r.ctx.sampleRate / 2), r.ctx.currentTime);
    // The light mix's speaker gains start at 0, as rebuild sets the
    // browser's.
    if (this.sides) for (const side of this.sides) side.gain.value = 0;
  }

  // A light mix speaker gain in the engine, as an AudioParam to place()
  // and fade(): its value set (and scheduled at the context's time, as the
  // browser's setter does), a target, a cancel.
  sideParam(c) {
    const out = this;
    const r = this.r;
    return {
      set value(v) {
        if (!out.lost) r.commands.push(VOICE.SIDE_SET, out.slot, c, v, r.ctx.currentTime);
      },
      setTargetAtTime(v, now, tau) {
        if (!out.lost) r.commands.push(VOICE.SIDE, out.slot, c, v, tau, now);
      },
      cancelScheduledValues(now) {
        if (!out.lost) r.commands.push(VOICE.CANCEL, out.slot, c, now);
      },
    };
  }

  // Its gain set outright (a voice's first).
  engineGain(level) {
    if (!this.lost) this.r.commands.push(VOICE.GAIN_SET, this.slot, level, this.r.ctx.currentTime);
  }

  // Its gain to 0 with time constant `tau`, the voice stopped `after` s on.
  engineRelease(tau, after) {
    if (this.lost || this.slot < 0) return;
    const now = this.r.ctx.currentTime;
    if (this.sides) {
      // As the light mix releases a voice: both speaker gains fade, then
      // the source stops.
      this.fade(now, tau);
      this.r.command(VOICE.STOP_AT, this.slot, now + after);
      return;
    }
    this.r.command(VOICE.RELEASE, this.slot, tau, after, now);
  }

  release() {
    if (this.engine && !this.lost && this.slot >= 0) this.r.command(VOICE.STOP, this.slot);
    for (const node of this.nodes) {
      try {
        node.disconnect();
      } catch {
        // gone
      }
    }
    if (this.source) {
      try {
        this.source.disconnect();
      } catch {
        // gone
      }
    }
    if (this.slot >= 0) {
      (this.engine ? this.r.slots : (this.r.nodeSlots ?? this.r.slots)).push(this.slot);
      this.slot = -1;
    }
  }

  dispose() {
    this.release();
    this.source = null;
    this.r.voices.delete(this);
  }
}

// A stream's channels into the output, by kind.
class BedOut {
  constructor(renderer, channels, gain, kind) {
    this.r = renderer;
    this.channels = channels;
    this.gain = gain;
    this.kind = kind;
    this.nodes = [];
    this.slots = [];
    this.input = renderer.ctx.createGain();
    this.input.channelCount = channels;
    this.input.channelCountMode = "explicit";
    this.input.channelInterpretation = "discrete";
    this.input.connect(gain);
  }

  rebuild() {
    const r = this.r;
    const ctx = r.ctx;
    for (const n of this.nodes) n.disconnect();
    this.nodes = [];
    this.freeSlots();
    try {
      this.gain.disconnect();
    } catch {
      // first build
    }
    if (r.light && ALSOFT_STREAM_ANGLES[this.channels]) {
      this.light();
      return;
    }
    if (r.enhanced && r.mode === "stereo" && STEREO_BEDS[this.channels]) {
      this.stereo(STEREO_BEDS[this.channels]);
      return;
    }
    const layout = r.layout;
    const N = r.channels;
    const keep = (n) => {
      this.nodes.push(n);
      return n;
    };
    const route = keep(ctx.createChannelMerger(N));
    route.connect(r.mix);
    const split = keep(ctx.createChannelSplitter(Math.max(this.channels, 1)));
    this.gain.connect(split);
    const index = (name) => layout.names.indexOf(name);
    const to = (from, channel, g = 1) => {
      if (channel < 0) return;
      const amp = keep(ctx.createGain());
      amp.gain.value = g;
      split.connect(amp, from);
      amp.connect(route, 0, channel);
    };
    const L = 0;
    const R = 1;
    const C = layout.center;
    const surround = N > 2;
    const rearL = index("BL") >= 0 ? index("BL") : index("SL");
    const rearR = index("BR") >= 0 ? index("BR") : index("SR");
    const sideL = index("SL");
    const sideR = index("SR");
    // A channel as a virtual speaker in the spatial mixer, at an azimuth
    // (degrees, positive to the left).
    const speaker = (from, az, spread = 0) => {
      const slot = r.slots.pop();
      if (slot === undefined) return;
      this.slots.push(slot);
      const a = (az * Math.PI) / 180;
      split.connect(r.mixer, from, slot);
      r.mixer.port.postMessage({ type: "params", data: Float32Array.of(slot, -Math.sin(a), 0, -Math.cos(a), spread, 0) });
      r.mixer.port.postMessage({ type: "start", slot });
    };
    // A channel through an HRTF panner at an azimuth.
    const place = (from, az) => {
      const p = keep(new PannerNode(ctx, { panningModel: "HRTF", rolloffFactor: 0, distanceModel: "linear" }));
      const a = (az * Math.PI) / 180;
      p.positionX.value = -Math.sin(a);
      p.positionZ.value = -Math.cos(a);
      split.connect(p, from);
      p.connect(r.hrtfOut);
    };
    const angles = ALSOFT_STREAM_ANGLES[this.channels];
    if (!r.enhanced && angles) {
      if (r.alsoft) {
        // The original on speakers: each channel at OpenAL Soft's angle for
        // it, through its panning; on stereo, angles ahead taken 3 times
        // wider (the front pair hard left and right).
        const wide = r.alsoft.streamFront || 0;
        angles.forEach((az, from) => speaker(from, wide > 0 && Math.abs(az) < 90 ? Math.sign(az) * Math.min(Math.abs(az) * wide, 90) : az));
        return;
      }
      if (r.hrtf) {
        // The original on headphones: OpenAL Soft's HRTF takes every
        // channel at its angle, mono ahead.
        angles.forEach((az, from) => place(from, az));
        return;
      }
    }
    if (r.binaural && this.channels === 4) {
      // A quad bed on headphones: four virtual speakers in the ambisonic
      // mix.
      BED_SPEAKERS.forEach((az, from) => speaker(from, az, BED_SPREAD));
      return;
    }
    if (r.hrtf && this.channels === 4) {
      // A quad bed on headphones: four virtual speakers.
      BED_SPEAKERS.forEach((az, from) => place(from, az));
      return;
    }
    if (this.channels === N && N > 4) {
      // A bed already in this layout (an upmixed music file): as it is.
      for (let c = 0; c < N; c++) to(c, c);
      return;
    }
    if (this.channels === 6 && N === 8) {
      // A 5.1 bed on 7.1: the surrounds (at about 110 degrees) between the
      // sides and the backs.
      for (const c of [0, 1, 2, 3]) to(c, c);
      to(4, sideL, 0.87);
      to(4, rearL, 0.5);
      to(5, sideR, 0.87);
      to(5, rearR, 0.5);
      return;
    }
    if (this.channels === 1) {
      if (this.kind === "voice" && r.enhanced && C >= 0) to(0, C, 1);
      else {
        to(0, L, Math.SQRT1_2);
        to(0, R, Math.SQRT1_2);
      }
      return;
    }
    if (this.channels === 2) {
      to(0, L);
      to(1, R);
      if (this.kind === "music" && r.enhanced && surround) this.upmix(split, route, rearL, rearR, sideL, sideR);
      return;
    }
    // Quad: front pair to the fronts, rear pair to the surrounds.
    to(0, L);
    to(1, R);
    if (!surround) {
      // On two speakers the back pair folds onto its own side: in full in
      // the original (OpenAL Soft pans back speakers at 110 degrees almost
      // wholly to their side), a little lower in the enhanced mode.
      const fold = r.enhanced ? 0.7 : 1;
      to(2, L, fold);
      to(3, R, fold);
      return;
    }
    if (sideL >= 0 && rearL !== sideL) {
      // 7.1: the rear pair sits between the sides and the backs (about 110
      // degrees).
      to(2, sideL, 0.87);
      to(2, rearL, 0.5);
      to(3, sideR, 0.87);
      to(3, rearR, 0.5);
    } else {
      to(2, rearL);
      to(3, rearR);
    }
  }

  // The light mix: each channel to both speakers by OpenAL Soft's stereo
  // law at its angle for it, the angles ahead taken 3 times wider.
  light() {
    const wide = ALSOFT.stereo.streamFront;
    this.stereo(
      ALSOFT_STREAM_ANGLES[this.channels].map((az) => {
        const a = ((Math.abs(az) < 90 ? Math.sign(az) * Math.min(Math.abs(az) * wide, 90) : az) * Math.PI) / 180;
        return alsoftStereo(-Math.sin(a), 0, -Math.cos(a), [0, 0]).map((g) => (Math.abs(g) < 1e-6 ? 0 : Math.abs(g - 1) < 1e-6 ? 1 : g));
      }),
    );
  }

  // Each channel to the two speakers by `gains`, [left, right] each, in as
  // few nodes as that takes (each one costs the audio thread every block): a
  // stereo stream straight into the mix, a mono one through one gain into
  // both speakers alike, a quad bed's front pair straight to its own
  // speakers and its rear pair through a gain to each.
  stereo(gains) {
    const r = this.r;
    const ctx = r.ctx;
    const keep = (n) => {
      this.nodes.push(n);
      return n;
    };
    if (gains.every(([left, right], c) => left === (c === 0 ? 1 : 0) && right === (c === 1 ? 1 : 0))) {
      this.gain.connect(r.mix);
      return;
    }
    if (this.channels === 1) {
      // Mono into two channels plays on both alike ("speakers").
      const both = keep(r.discrete(ctx.createGain(), 2));
      both.gain.value = gains[0][0];
      this.gain.connect(both);
      both.connect(r.mix);
      return;
    }
    const split = keep(ctx.createChannelSplitter(this.channels));
    const route = keep(ctx.createChannelMerger(2));
    this.gain.connect(split);
    route.connect(r.mix);
    gains.forEach((pair, from) =>
      pair.forEach((g, c) => {
        if (g === 1) split.connect(route, from, c);
        else if (g) {
          const amp = keep(ctx.createGain());
          amp.gain.value = g;
          split.connect(amp, from);
          amp.connect(route, 0, c);
        }
      }),
    );
  }

  // A light upmix for stereo music: the side (L - R) through decorrelating
  // delays into the surrounds, 6 dB down; the fronts keep the original.
  upmix(split, route, rearL, rearR, sideL, sideR) {
    const ctx = this.r.ctx;
    const keep = (n) => {
      this.nodes.push(n);
      return n;
    };
    const side = keep(ctx.createGain());
    const invert = keep(ctx.createGain());
    invert.gain.value = -1;
    split.connect(side, 0);
    split.connect(invert, 1);
    invert.connect(side);
    const tone = keep(ctx.createBiquadFilter());
    tone.type = "lowpass";
    tone.frequency.value = 7000;
    side.connect(tone);
    const feed = (channel, delay, sign) => {
      if (channel < 0) return;
      const d = keep(ctx.createDelay(0.05));
      d.delayTime.value = delay;
      const g = keep(ctx.createGain());
      g.gain.value = 0.5 * sign * (sideL >= 0 && channel !== sideL && channel !== sideR ? 0.7 : 1);
      tone.connect(d);
      d.connect(g);
      g.connect(route, 0, channel);
    };
    feed(rearL, 0.012, 1);
    feed(rearR, 0.017, -1);
    if (sideL >= 0 && sideL !== rearL) {
      feed(sideL, 0.008, 1);
      feed(sideR, 0.011, -1);
    }
  }

  freeSlots() {
    for (const slot of this.slots) this.r.slots.push(slot);
    this.slots = [];
  }

  dispose() {
    for (const n of this.nodes) n.disconnect();
    this.nodes = [];
    this.freeSlots();
    this.input.disconnect();
    this.r.beds.delete(this);
  }
}
