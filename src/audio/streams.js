import { Target } from "./automation.js";
import { placeVoice } from "./engine.js";
import { hashKey, keyOf } from "./hash.js";
import { distanceGain, gate, openAlFalloff, pickSample, prop, rollGain, rollPitch } from "./props.js";
import { scale } from "./enhanced.js";

// Streamed sound: music, voice-over, and ambience. Each OpenAudioStream call
// in the mission opens one slot on a stream file (Theed opens nab2 twice),
// and a slot plays one segment at a time. A SoundStreamProperties (or
// SoundStream) object names its Stream and a SegmentList to pick from: by
// weight, or in turn with CyclePlayback, each segment gated by its own
// interval (Segment(id, weight, interval, intervalDev)). A looping one picks
// again when a segment ends and carries straight on.
//
// Voice-over and the soldiers' chatter are queued, as the 2004 game queues
// them: each stream's slot keeps up to four lines, best score first (the
// voice score, (1 - Bias) x level + Bias x Priority, the level with its bus
// and, for a line said somewhere, its distance; ties behind earlier ones),
// worked out again for every line queued whenever one is added, and a slot
// that falls idle plays the first.
// So lines on one stream never overlap or cut each other off. A fifth line
// drops the last; one said somewhere that would itself be last isn't
// queued, while a 2D one drops the line ahead of it instead. A line is
// checked as it is asked for: its segment is picked (with each segment's
// own interval), then its PlayInterval and PlayProbability. A line said out
// of earshot still scores Bias x Priority: it is queued, and holds the
// slot unheard.
//
// A soldier's line (a mono Mode3D stream, at the place he said it, which it
// keeps) plays from there through one of the output's 3D voices, as his
// other sounds do; every other stream is a bed (music, ambience, the
// commander's voice-over).
//
// Segments are fetched on demand (by byte range when the file is large) and
// decoded off the main thread; only recently used ones stay decoded.

const WHOLE_FILE = 8 << 20; // bytes: smaller stream files are fetched whole
const CACHE_SECONDS = 600; // decoded audio kept, in seconds of sound
const SMOOTH = 0.02;

export class Streams {
  constructor(engine) {
    this.engine = engine;
    this.files = new Map(); // stream name -> Promise<ArrayBuffer> for whole files
    this.decoded = new Map(); // "stream|segment" -> Promise<AudioBuffer>
    this.order = [];
    this.byKey = new Map();
    this.voices = new Set();
    this.worker = null;
    this.jobs = new Map();
    this.nextJob = 1;
    this.slots = new Map(); // stream name -> slots opened
    this.queues = new Map(); // stream name -> queued requests
    this.gates = new Map(); // properties key -> gate state
  }

  setData(data) {
    this.data = data;
    this.byKey.clear();
    for (const [name, index] of data.streams) this.byKey.set(hashKey(name), index);
    for (const voice of this.voices) voice.stop(0);
    this.voices.clear();
    this.slots.clear();
    for (const [, name] of data.mission.streams || []) this.slots.set(name, (this.slots.get(name) || 0) + 1);
    this.queues.clear();
  }

  // Slots of a stream in use.
  busyOn(name) {
    let n = 0;
    for (const v of this.voices) if (v.index.name === name && !v.done) n++;
    return n;
  }

  free(name) {
    return this.busyOn(name) < (this.slots.get(name) || 1);
  }

  gateOf(props) {
    let state = this.gates.get(props.Name);
    if (!state) {
      state = { next: -Infinity };
      if (prop(props, "CyclePlayback") > 0) state.cycle = -1;
      this.gates.set(props.Name, state);
    }
    return state;
  }

  // Ask for a line (voice-over, chatter): it plays when its stream has a
  // slot free. options.position: where it is said (a 3D line). Returns
  // false when it won't play at all.
  queue(nameOrKey, options = {}) {
    const asked = this.ask(nameOrKey, options);
    if (!asked) return false;
    const { props, index, segment } = asked;
    const queue = this.queues.get(index.name) || [];
    const position = this.placed(props, options) ? options.position : null;
    const request = { name: nameOrKey, props, position, options: { ...options, segment }, score: 0 };
    // Every line waiting is scored again (where the listener is now); one
    // scoring 0 drops.
    for (let i = queue.length - 1; i >= 0; i--) {
      queue[i].score = this.score(queue[i].props, queue[i].position);
      if (!(queue[i].score > 0)) queue.splice(i, 1);
    }
    request.score = asked.score;
    // Behind any of an equal score; at most four.
    let at = queue.findIndex((r) => r.score < request.score);
    if (at < 0) at = queue.length;
    queue.splice(at, 0, request);
    this.queues.set(index.name, queue);
    if (queue.length > 4) {
      // The last goes; a line said somewhere that would be last isn't
      // queued, a 2D one drops the line ahead of it instead.
      if (queue[queue.length - 1] !== request) queue.pop();
      else if (position) {
        queue.pop();
        return false;
      } else queue.splice(queue.length - 2, 1);
    }
    return true;
  }

  // Play a line at once, 2D, on its stream (the local player's own command,
  // which the original game plays unqueued with its 3D turned off): it takes
  // the stream's slot from whatever plays there.
  say(nameOrKey, options = {}) {
    const asked = this.ask(nameOrKey, options);
    if (!asked) return null;
    if (!this.free(asked.index.name)) {
      for (const voice of [...this.voices]) if (voice.index.name === asked.index.name) voice.stop(0.05);
    }
    return this.play(nameOrKey, { ...options, position: null, segment: asked.segment });
  }

  // A line's checks as it is asked for: its stream; a segment
  // (each gated by its own interval); its PlayInterval and PlayProbability;
  // a score where it is said. { props, index, segment, score } or null.
  ask(nameOrKey, options) {
    const props = this.data?.sound(nameOrKey);
    if (!props || !props.Stream) return null;
    const index = this.stream(props.Stream);
    if (!index) return null;
    const now = this.engine.time;
    const state = this.gateOf(props);
    const entry = pickSample(props.SegmentList, (key) => Boolean(index.segments[key]), state, now);
    if (!entry) return null;
    if (!gate(props, state, now)) return null;
    const score = this.score(props, this.placed(props, options) ? options.position : null);
    if (!(score > 0)) return null;
    return { props, index, segment: entry[0], score };
  }

  // Whether a line plays where it is said: a Mode3D stream with a place.
  placed(props, options) {
    return Boolean(options?.position) && prop(props, "Mode3D") > 0;
  }

  // The voice score of a line, said at `position` (or 2D): (1 - Bias) x
  // level + Bias x Priority, the level being Gain with its bus, times the
  // distance gain from the listener for a 3D line.
  // Past its MuteDistance a line still scores Bias x Priority, so a line
  // said far off is queued and holds the slot unheard, as in the original.
  score(props, position) {
    let level = Math.min(prop(props, "Gain") * this.engine.busGain(props.Bus || "0x9f9c4fd4"), 1);
    if (position) {
      const ear = this.engine.listener.ear;
      const dx = position.x - ear.x;
      const dy = position.y - ear.y;
      const dz = position.z - ear.z;
      level *= distanceGain(props, Math.sqrt(dx * dx + dy * dy + dz * dz));
    }
    const bias = prop(props, "Bias");
    return (1 - bias) * level + bias * prop(props, "Priority");
  }

  // The stream index a name or key stands for.
  stream(nameOrKey) {
    return this.byKey.get(keyOf(nameOrKey)) || null;
  }

  decode(bytes, index) {
    if (!this.worker) {
      this.worker = new Worker(new URL("./stream-worker.js", import.meta.url), { type: "module" });
      this.worker.onmessage = (event) => {
        const job = this.jobs.get(event.data.id);
        this.jobs.delete(event.data.id);
        if (!job) return;
        if (event.data.error) job.reject(new Error(event.data.error));
        else job.resolve(event.data.channels);
      };
    }
    const id = this.nextJob++;
    return new Promise((resolve, reject) => {
      this.jobs.set(id, { resolve, reject });
      this.worker.postMessage(
        { id, bytes, channels: index.channels, substreams: index.substreams, interleave: index.substreamInterleave },
        [bytes],
      );
    });
  }

  async bytes(index, offset, length) {
    const url = `${this.data.base}/streams/${index.name}.bin`;
    const total = Object.values(index.segments).reduce((end, [o, s]) => Math.max(end, o + s), 0);
    if (total <= WHOLE_FILE) {
      if (!this.files.has(index.name)) this.files.set(index.name, fetch(url).then((r) => r.arrayBuffer()));
      const whole = await this.files.get(index.name);
      return whole.slice(offset, offset + length);
    }
    const response = await fetch(url, { headers: { Range: `bytes=${offset}-${offset + length - 1}` } });
    const body = await response.arrayBuffer();
    return response.status === 206 ? body : body.slice(offset, offset + length);
  }

  // A segment as an AudioBuffer: 1 or 2 channels, or 4 for a stream of two
  // stereo substreams (front pair, then rear pair).
  segment(index, segmentKey) {
    // The enhanced mode plays the upscaled segment (the layout's upmix for
    // music) when there is one.
    const r = this.engine.renderer;
    const better = r?.enhanced ? this.engine.enhanced?.segment(index.name, segmentKey, r.channels) : null;
    if (better) return this.enhancedSegment(index, segmentKey, better);
    const id = `${index.name}|${segmentKey}`;
    if (this.decoded.has(id)) {
      this.order = this.order.filter((k) => k !== id).concat(id);
      return this.decoded.get(id);
    }
    const entry = index.segments[segmentKey];
    if (!entry) return Promise.resolve(null);
    const [offset, size, , rate] = entry;
    const count = index.substreams || 1;
    const chunk = (index.substreamInterleave || 0) * count;
    const length = count > 1 && chunk ? Math.ceil(size / chunk) * chunk : size;
    const promise = this.bytes(index, offset, length)
      .then((bytes) => this.decode(bytes, index))
      .then((channels) => {
        const ctx = this.engine.ctx;
        const frames = Math.min(...channels.map((c) => c.length));
        const buffer = ctx.createBuffer(channels.length, Math.max(frames, 1), rate);
        channels.forEach((c, i) => buffer.copyToChannel(c.subarray(0, frames), i));
        return buffer;
      });
    this.decoded.set(id, promise);
    this.order.push(id);
    this.trim();
    return promise;
  }

  enhancedSegment(index, segmentKey, file) {
    const id = `${index.name}|${segmentKey}|${file.url}`;
    if (this.decoded.has(id)) {
      this.order = this.order.filter((k) => k !== id).concat(id);
      return this.decoded.get(id);
    }
    const promise = fetch(file.url)
      .then((r) => r.arrayBuffer())
      .then((data) => this.engine.ctx.decodeAudioData(data))
      .then((buffer) => scale(buffer, file.playback))
      .catch(() => null);
    this.decoded.set(id, promise);
    this.order.push(id);
    this.trim();
    return promise;
  }

  trim() {
    let seconds = 0;
    for (let i = this.order.length - 1; i >= 0; i--) {
      const id = this.order[i];
      const [name, seg] = id.split("|", 2);
      const index = this.data.streams.get(name);
      const entry = index?.segments[seg];
      seconds += entry ? entry[2] / (index.substreams || 1) / entry[3] : 0;
      if (seconds > CACHE_SECONDS && i < this.order.length - 2) {
        this.decoded.delete(id);
        this.order.splice(i, 1);
      }
    }
  }

  // Play a stream's properties by name. options: position (a 3D stream),
  // category, gain, loop, fadeIn (s), onEnd, segment (a key, to force one).
  play(nameOrKey, options = {}) {
    const props = this.data?.sound(nameOrKey);
    if (!props || !props.Stream) return null;
    const index = this.stream(props.Stream);
    if (!index) return null;
    // One segment per slot: none free, nothing plays.
    if (!this.free(index.name)) return null;
    if (!options.segment) {
      const state = this.gateOf(props);
      const entry = pickSample(props.SegmentList, (key) => Boolean(index.segments[key]), state, this.engine.time);
      if (!entry) return null;
      options = { ...options, segment: entry[0] };
    }
    const voice = new StreamVoice(this, props, index, options);
    this.voices.add(voice);
    voice.next();
    return voice;
  }

  update(dt) {
    const l = this.engine.listener;
    for (const voice of this.voices) voice.update(dt, l);
    // Queued lines start as their streams' slots fall idle.
    for (const [name, queue] of this.queues) {
      while (queue.length && this.free(name)) {
        const request = queue.shift();
        this.play(request.name, request.options);
      }
    }
    // A line said somewhere steers its output now, not next frame.
    this.engine.renderer?.flush();
  }

  stopAll(fade = 0.05) {
    for (const voice of [...this.voices]) voice.stop(fade);
    for (const queue of this.queues.values()) queue.length = 0;
  }

  busy(bus) {
    for (const voice of this.voices) if (voice.bus === bus && voice.sounding) return true;
    return false;
  }
}

class StreamVoice {
  constructor(streams, props, index, options) {
    this.streams = streams;
    this.engine = streams.engine;
    // A placed emitter's distances over the properties' own.
    this.props = options.overrides ? { ...props, ...options.overrides } : props;
    this.index = index;
    this.options = options;
    this.category = options.category || "music";
    this.bus = props.Bus || "0x9f9c4fd4";
    this.loop = options.loop ?? prop(props, "Looping") > 0;
    this.gain = rollGain(props) * (options.gain ?? 1);
    this.pitch = rollPitch(props);
    this.position = options.position || null;
    // A soldier's line said somewhere (options.spatial; a mono, Mode3D
    // stream) plays from there through one of the output's 3D voices, as
    // his own sounds do; anything else placed is a bed set by its distance.
    this.mode3D = options.spatial && this.position && prop(this.props, "Mode3D") > 0 && (index.channels || 1) === 1 ? 1 : 0;
    this.voiceOut = null;
    this.local = { x: 0, y: 0, z: -1 };
    this.distance = 0;
    this.span = 0;
    this.extra = {};
    this.reverbGain = prop(this.props, "ReverbGain");
    // How the renderer lays it out: voice-over, a music bed, or ambience.
    this.kind = options.kind || { voice: "voice", ambience: "ambience", music: "music" }[this.category] || "plain";
    this.level = 1; // fades
    this.fadeTau = SMOOTH;
    this.fadeUntil = 0;
    this.done = false;
    this.sounding = false;
    this.current = null;
    this.queued = null;
    const ctx = this.engine.ctx;
    this.out = ctx.createGain();
    this.out.gain.value = 0;
    this.outLevel = new Target();
    this.bed = null;
    this.fadeTo(1, options.fadeIn ?? 0);
  }

  // The stream's gain with its bus and volume slider, as the engine sets a
  // voice's.
  level0() {
    const e = this.engine;
    return this.gain * e.busGain(this.bus) * e.categoryVolume(this.category);
  }

  pick() {
    if (this.options.segment && !this.segmentKey) return this.options.segment;
    const state = this.streams.gateOf(this.props);
    const entry = pickSample(this.props.SegmentList, (key) => Boolean(this.index.segments[key]), state, this.engine.time);
    return entry ? entry[0] : null;
  }

  // Queue the next segment to start when the current one ends.
  async next() {
    const key = this.pick();
    if (!key) {
      this.finish();
      return;
    }
    this.segmentKey = key;
    const buffer = await this.streams.segment(this.index, key).catch(() => null);
    if (this.done) return;
    if (!buffer) {
      this.finish();
      return;
    }
    const ctx = this.engine.ctx;
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    source.playbackRate.value = this.pitch;
    if (this.mode3D) {
      if (!this.voiceOut) {
        this.voiceOut = this.engine.renderer.voice(this);
        this.voiceOut.attach(this.out);
      }
      source.connect(this.out);
    } else {
      if (!this.bed) this.bed = this.engine.renderer.bed(buffer.numberOfChannels, this.out, this.kind);
      source.connect(this.bed.input);
    }
    const when = this.current ? Math.max(this.current.ends, ctx.currentTime) : ctx.currentTime;
    source.start(when);
    const playing = { source, ends: when + buffer.duration / this.pitch, key };
    source.onended = () => {
      source.disconnect();
      if (this.current === playing) {
        this.current = null;
        if (!this.queued) this.finish();
      }
    };
    if (this.current) this.queued = playing;
    else this.current = playing;
    this.sounding = true;
  }

  update(dt, l) {
    if (this.done) return;
    const ctx = this.engine.ctx;
    // A queued segment takes over when the current one is over.
    if (this.queued && ctx.currentTime >= this.current?.ends - 0.005) {
      this.current = this.queued;
      this.queued = null;
    }
    if (this.loop && this.current && !this.queued && !this.pending && this.current.ends - ctx.currentTime < 8) {
      this.pending = true;
      this.next().finally(() => {
        this.pending = false;
      });
    }
    let gain = this.level0();
    if (this.mode3D) gain *= this.place(l);
    else if (this.position && prop(this.props, "Mode3D") > 0) {
      const ear = l.ear || l.position;
      const dx = this.position.x - ear.x;
      const dy = this.position.y - ear.y;
      const dz = this.position.z - ear.z;
      const d = Math.sqrt(dx * dx + dy * dy + dz * dz);
      gain *= distanceGain(this.props, d);
    }
    this.target = gain;
    // A fade keeps its own pace until it is through.
    const now = ctx.currentTime;
    this.outLevel.set(this.out.gain, gain * this.level, now, now < this.fadeUntil ? this.fadeTau : SMOOTH);
    this.onUpdate?.(this);
  }

  // A line said somewhere, placed as the engine places its 3D voices: its
  // direction steers its output, and its distance gain comes back (with
  // the OpenAL output's own falloff in the original mode, and the enhanced
  // mode's air and room).
  place(l) {
    const e = this.engine;
    const d = placeVoice(this, l);
    let gain = distanceGain(this.props, d);
    if (!e.renderer?.enhanced) gain *= openAlFalloff(this.props, d);
    const extra = e.shape ? e.shape(this) : this.extra;
    gain *= extra.gain ?? 1;
    extra.send = (extra.send ?? 0) * (e.renderer?.enhanced ? this.reverbGain : 1);
    this.voiceOut?.update(this.local, this.span, 0, extra);
    return gain;
  }

  // Fade to `level` over `seconds` (95% of the way by then); update() keeps
  // to the fade's pace until it is through.
  fadeTo(level, seconds) {
    this.level = level;
    const ctx = this.engine.ctx;
    const g = this.out.gain;
    this.outLevel?.forget();
    g.cancelScheduledValues(ctx.currentTime);
    const base = this.target ?? this.level0();
    this.fadeTau = seconds > 0 ? seconds / 3 : SMOOTH;
    this.fadeUntil = ctx.currentTime + Math.max(seconds, 0);
    if (seconds > 0) g.setTargetAtTime(base * level, ctx.currentTime, this.fadeTau);
    else g.value = base * level;
  }

  stop(fade = 0.05) {
    if (this.done) return;
    this.fadeTo(0, fade);
    const ctx = this.engine.ctx;
    for (const part of [this.current, this.queued]) {
      if (!part) continue;
      try {
        // Twice the fade: under 0.3% of the level is left to cut.
        part.source.stop(ctx.currentTime + 2 * fade + 0.05);
      } catch {
        // not started yet
      }
    }
    this.finish();
  }

  finish() {
    if (this.done) return;
    this.done = true;
    this.sounding = false;
    this.streams.voices.delete(this);
    const bed = this.bed;
    const voiceOut = this.voiceOut;
    setTimeout(() => {
      this.out.disconnect();
      bed?.dispose();
      voiceOut?.dispose();
    }, 3000);
    this.options.onEnd?.(this);
  }
}
