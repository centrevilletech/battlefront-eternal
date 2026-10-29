import { Target } from "./automation.js";
import { ALSOFT, ALSOFT_STREAM_ANGLES, LAYOUTS, autoLayout } from "./layouts.js";

// Where sound goes once it leaves a voice: the output layout, the panning,
// the reverb, the LFE, and the master chain.
//
// Two renderings, switched live:
// - "original": what the Steam build's OpenAL Soft 1.20.1 output gives.
//   Speakers: its own panning, 2D ambisonic decoders of first order on
//   stereo, second on 5.1, third on 7.1 (src/audio/layouts.js, ALSOFT), and
//   streams' channels at its fixed angles; on surround, its front
//   stabilizer (the Steam build's alsoft.ini turns it on), which alone
//   feeds the centre. No LFE. Headphones: OpenAL Soft turns on its HRTF
//   when Windows reports headphones, so every source and stream channel
//   goes through an HRTF (the browser's, standing in for OpenAL Soft's MIT
//   KEMAR set). No extra reverb; a limiter in place of OpenAL Soft's.
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
const LFE_CUTOFF = 120; // Hz
const LFE_SEND = 0.5; // an explosion's LFE send, linear
// A BiquadFilterNode's lowpass Q is in dB: -3.01 dB is a Butterworth section.
const BUTTERWORTH_Q = -3.0103;

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

export class Renderer {
  constructor(ctx) {
    this.ctx = ctx;
    this.loaded = null;
    this.slots = [];
    this.batch = [];
    this.voices = new Set();
    this.beds = new Set();
    this.config = { quality: "enhanced", output: "auto", dynamics: "normal", mono: false, clarity: true };
    this.decoder = null;
    this.onMeter = null;
    this.onLimiter = null;
  }

  // Load the worklets, and the headphone decoder from under `root` (the
  // audio folder) when given; until then voices fall back to stereo panners.
  load(root = null) {
    if (!this.loaded) {
      const add = (file) => this.ctx.audioWorklet.addModule(new URL(`./worklets/${file}`, import.meta.url));
      const worklets = Promise.all(["spatial-mixer.js", "limiter.js", "reverb.js", "meter.js", "stabilizer.js", "clarity.js"].map(add));
      this.loaded = Promise.all([worklets, root ? this.loadDecoder(root) : null]).then(() => true);
    }
    return this.loaded;
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

  configure(config) {
    this.config = { ...this.config, ...config };
    this.build();
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
    this.mixer = keep(
      new AudioWorkletNode(ctx, "spatial-mixer", {
        numberOfInputs: SLOTS,
        numberOfOutputs: 2,
        outputChannelCount: [feeds, 1],
        processorOptions: { channels: feeds, law, speakers: layout.speakers, lfe: layout.lfe, decoder: this.alsoft },
      }),
    );
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
    this.reverbIn = keep(ctx.createGain());
    this.reverbIn.channelCount = 1;
    this.reverbIn.channelCountMode = "explicit";
    const rooms = layout.speakers.map((s) => (s ? 1 : 0));
    this.reverb = keep(new AudioWorkletNode(ctx, "fdn-reverb", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [N], processorOptions: { channels: N } }));
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
          processorOptions: { channels: N, left: 0, right: 1, centre: layout.center },
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
          processorOptions: { channels: N, skip: layout.lfe },
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
        processorOptions: { ceiling: -1, release: enhanced ? 0.08 : 0.06, dynamics: { ...d, skip: layout.lfe } },
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
    this.volume?.gain.setTargetAtTime(v, this.ctx.currentTime, 0.05);
  }

  // A loudness meter on the output (for the overlay and the tests).
  meter(on) {
    this.meterOn = on;
    if (on && !this.meterNode) {
      const N = this.channels;
      const weights = this.layout.names.map((n) => (n === "LFE" ? 0 : /^(SL|SR|BL|BR|TFL|TFR|TBL|TBR)$/.test(n) ? 1.41 : 1));
      this.meterNode = new AudioWorkletNode(this.ctx, "loudness-meter", {
        numberOfInputs: 1,
        numberOfOutputs: 0,
        channelCount: N,
        channelCountMode: "explicit",
        channelInterpretation: "discrete",
        processorOptions: { weights },
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
    this.reverb?.port.postMessage(params);
  }

  // A voice's way out. Returns { attach(node), update(...), dispose() }.
  voice(voice) {
    const out = new VoiceOut(this, voice);
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

  // Send this frame's slot directions to the mixer.
  flush() {
    if (!this.batch.length || !this.mixer) return;
    const data = Float32Array.from(this.batch);
    this.mixer.port.postMessage({ type: "params", data }, [data.buffer]);
    this.batch.length = 0;
  }
}

// One voice's chain after its gain: an air-and-occlusion lowpass (enhanced),
// then a mixer slot, or an HRTF panner on headphones without the decoder,
// or a stereo panner (a 2D voice on headphones, or out of slots); and its
// reverb send.
class VoiceOut {
  constructor(renderer, voice) {
    this.r = renderer;
    this.voice = voice;
    this.slot = -1;
    this.nodes = [];
    this.source = null;
    this.cutoff = 20000;
  }

  rebuild() {
    const r = this.r;
    const ctx = r.ctx;
    this.release();
    this.nodes = [];
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
      this.slot = r.binaural && !this.voice.mode3D ? -1 : (r.slots.pop() ?? -1);
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

  // The voice's gain node feeds this.
  attach(node) {
    this.source = node;
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
    if (this.filter && extra.cutoff !== undefined && Math.abs(extra.cutoff - this.cutoff) > 10) {
      this.cutoff = extra.cutoff;
      this.filter.frequency.setTargetAtTime(Math.min(this.cutoff, r.ctx.sampleRate / 2), now, 0.03);
    }
    if (extra.send !== undefined) this.sendLevel.set(this.send.gain, extra.send, now, 0.05);
  }

  release() {
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
      this.r.slots.push(this.slot);
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
