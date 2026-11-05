import { AudioEngine } from "../src/audio/engine.js";
import { SoundData } from "../src/audio/data.js";

// Stable synthetic inputs for the recorded render gate. No game assets.
export const AUDIO_SCENES = [];
for (const quality of ["original", "enhanced"]) {
  for (const output of ["stereo", "5.1", "7.1", "7.1.4", "headphones"]) {
    for (const mono of [false, true]) {
      AUDIO_SCENES.push({ name: `${quality}-${output}${mono ? "-mono" : ""}`, quality, output, mono, beds: true, voices: 12, seconds: 1 });
    }
  }
}
for (const mono of [false, true]) AUDIO_SCENES.push({ name: `enhanced-binaural${mono ? "-mono" : ""}`, quality: "enhanced", output: "headphones", binaural: true, mono, beds: true, voices: 12, seconds: 1 });
for (const quality of ["original", "enhanced"]) AUDIO_SCENES.push({ name: `${quality}-stereo-strict`, quality, output: "stereo", mono: false, beds: false, voices: 2, seconds: 1, strict: true });

export async function renderAudioFixture(scene) {
  const rate = scene.rate || 48000;
  const channels = ({ "5.1": 6, "7.1": 8, "7.1.4": 12 })[scene.output] || 2;
  const ctx = new OfflineAudioContext(channels, rate * scene.seconds, rate);
  let seed = 7;
  const random = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 4294967296) * 2 - 1;
  const pcm = new Int16Array(22050);
  for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(6000 * Math.sin(i / 7) + 3000 * random());
  const name = (i) => `0x0000e${String(i).padStart(3, "0")}`;
  const objects = [{ type: "Bus", Name: "0x00000001", Gain: 0.8 }];
  for (let i = 0; i < scene.voices; i++) objects.push({ type: "SoundProperties", Name: name(i), Bus: "0x00000001", Mode3D: i === scene.voices - 1 && scene.beds ? 0 : 1, Pan: 0.3, MinDistance: 2, MaxDistance: 400, MuteDistance: 400, RollOff: 1, Looping: 1, Bias: 0.0001, ReverbGain: 0.5, PitchVariation: 0, ModeDoppler: 1, SampleList: [["0x00000100", 1]] });
  const engine = new AudioEngine({ context: ctx });
  const r = engine.renderer;
  if (scene.binaural) r.decoder = { filters: Array.from({ length: 16 }, (_, c) => {
    const b = ctx.createBuffer(2, 256, rate);
    for (let e = 0; e < 2; e++) {
      const d = b.getChannelData(e);
      d[c + e + 1] = 0.1 / (c + 1);
      d[100 + c * 3 + e] = -0.03 / (c + 1);
    }
    return b;
  }) };
  r.setRoom({ room: 0.5, decay: 1.2, reflections: 0.3 });
  r.masterVolume = 0.8;
  const config = { quality: scene.quality, output: scene.output, mono: scene.mono, dynamics: "normal", clarity: true };
  await r.load(null, config);
  await r.configure(config);
  engine.setData(new SoundData({ objects, samples: { "0x00000100": [0, pcm.length, 22050] } }, pcm.buffer));
  engine.shape = (voice) => {
    voice.extra.cutoff = 20000 / (1 + voice.distance / 35);
    voice.extra.gain = 1;
    voice.extra.send = 0.6;
    voice.extra.lfe = voice.mode3D ? 0.5 : 0;
    return voice.extra;
  };
  const ports = [r.mixer.port];
  let id = 0;
  const waits = new Map();
  for (const port of ports) {
    const before = port.onmessage;
    port.onmessage = (e) => { if (e.data?.type === "sync") { waits.get(e.data.id)?.(); waits.delete(e.data.id); } else before?.(e); };
  }
  const sync = () => Promise.all(ports.map((p) => new Promise((resolve) => { const key = ++id; waits.set(key, resolve); p.postMessage({ type: "sync", id: key }); })));
  const handles = [];
  const start = () => {
    for (let i = 0; i < scene.voices; i++) {
      const angle = i * 2.4;
      const distance = 5 + ((i * 13) % 70);
      handles.push(engine.play(name(i), { position: { x: distance * Math.sin(angle), y: 1 + (i % 3), z: distance * Math.cos(angle) }, loop: true, category: "ambience" }));
    }
    if (scene.beds) for (const [count, kind] of [[4, "ambience"], [2, "music"], [1, "voice"]]) {
      const b = ctx.createBuffer(count, rate, rate);
      for (let c = 0; c < count; c++) for (let i = 0; i < b.length; i++) b.getChannelData(c)[i] = 0.08 * Math.sin(i * (0.013 + c * 0.007)) + 0.01 * random();
      const lane = r.stream(count, kind); lane.gain.value = 0.6; lane.start(b, ctx.currentTime);
    }
  };
  // Preserve the recorded headphone source scheduling before the clock starts.
  if (scene.output === "headphones") start();
  const step = (k) => {
    const t = k / 60;
    engine.setListener({ x: 2 * Math.sin(t), y: 1.6, z: Math.sin(t * 0.3) }, { x: -Math.sin(t * 0.8), y: 0, z: -Math.cos(t * 0.8) }, { x: 0, y: 1, z: 0 });
    if (k === 3 && scene.output !== "headphones") start();
    if (k === 20) r.setRoom({ room: 0.2, decay: 0.6, reverbDelay: 0.05, density: 0.7 });
    if (k === 30) { r.setVolume(0.4); handles[handles.length - 1]?.stop(0.05); }
    engine.update(1 / 60);
  };
  step(0);
  for (let k = 1; k / 60 < scene.seconds; k++) ctx.suspend(Math.round(k / 60 * rate / 128) * 128 / rate).then(async () => { step(k); await sync(); await ctx.resume(); });
  await sync();
  const out = await ctx.startRendering();
  return encodeAudio(out);
}

function encodeAudio(out) {
  const encoded = [];
  let peak = 0;
  for (let c = 0; c < out.numberOfChannels; c++) {
    const data = out.getChannelData(c);
    for (const v of data) peak = Math.max(peak, Math.abs(v));
    const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    let text = "";
    for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
    encoded.push(btoa(text));
  }
  return { encoded, peak, channels: out.numberOfChannels, rate: out.sampleRate };
}

export async function renderVoiceLife(scene) {
      const { quality, output, channels, binaural } = scene;
      const seconds = 2.5;
      const ctx = new OfflineAudioContext(channels, 48000 * seconds, 48000);
      // A second of 22.05 kHz sound and 0.4 s of 44.1 kHz sound.
      const pcm = new Int16Array(22050 + 17640);
      let seed = 3;
      const rand = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 4294967296) * 2 - 1;
      for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(7000 * Math.sin(i / (i < 22050 ? 9 : 5)) + 2500 * rand());
      const sound = (name, sample, looping) => ({
        type: "SoundProperties", Name: name, Bus: "0x00000001", Mode3D: 1, MinDistance: 2, MaxDistance: 60, MuteDistance: 60, RollOff: 1,
        Looping: looping, Bias: 0.0001, ReverbGain: 0.5, PitchVariation: 0, ModeDoppler: 1, SampleList: [[sample, 1]],
      });
      const manifest = {
        objects: [
          { type: "Bus", Name: "0x00000001", Gain: 1 },
          sound("0x0000d001", "0x00000100", 1),
          sound("0x0000d002", "0x00000101", 0),
          sound("0x0000d003", "0x00000100", 1),
        ],
        samples: { "0x00000100": [0, 22050, 22050], "0x00000101": [22050, 17640, 44100] },
      };
      const engine = new AudioEngine({ context: ctx });
      if (binaural) engine.renderer.decoder = { filters: Array.from({ length: 16 }, (_, c) => {
        const b = ctx.createBuffer(2, 256, 48000);
        for (let e = 0; e < 2; e++) b.getChannelData(e)[c + e + 1] = 0.1 / (c + 1);
        return b;
      }) };
      if (quality === "enhanced") engine.renderer.setRoom({ room: 0.5, decay: 1.2, reflections: 0.3 });
      const config = { quality, output, dynamics: "normal", mono: false };
      await engine.renderer.load(null, config);
      await engine.renderer.configure(config);
      engine.setData(new SoundData(manifest, pcm.buffer));
      engine.shape = (voice) => {
        const e = voice.extra;
        e.cutoff = voice.distance > 6 ? Math.max(20000 / (1 + (voice.distance - 6) / 10), 1800) : 20000;
        e.gain = 1;
        e.send = 1;
        e.lfe = 0;
        return e;
      };
      const ports = [engine.renderer.mixer.port];
      const waits = new Map();
      let ids = 0;
      for (const p of ports) p.onmessage = (e) => { if (e.data?.type === "sync") waits.get(e.data.id)?.(); };
      const sync = () => Promise.all(ports.map((p) => new Promise((r) => waits.set(++ids, r) && p.postMessage({ type: "sync", id: ids }))));
      const handles = {};
      const seen = { ended: false, stopped: false, offset: -1 };
      const step = (k) => {
        const t = k / 60;
        engine.setListener({ x: 2 * Math.sin(t * 1.3), y: 1.6, z: Math.sin(t * 0.7) }, { x: -Math.sin(t), y: 0, z: -Math.cos(t) }, { x: 0, y: 1, z: 0 });
        if (k === 0) handles.a = engine.play("0x0000d001", { position: { x: -4, y: 0, z: -3 }, loop: true, category: "ambience" });
        handles.a?.moveTo({ x: -4 + 3 * Math.sin(t * 2), y: 0, z: -3 + 2 * Math.cos(t * 1.5) });
        // (A one-shot plays in a suspended context only when asked to.)
        if (k === 15) handles.b = engine.play("0x0000d002", { position: { x: 5, y: 0, z: 2 }, category: "effects", offline: true });
        if (k === 48) handles.c = engine.play("0x0000d001", { position: { x: 1, y: 0, z: -9 }, loop: true, category: "ambience" });
        if (k === 78) handles.c.stop(0.05);
        // Out of earshot (past MuteDistance) at first, then within it.
        if (k === 84) handles.d = engine.play("0x0000d003", { position: { x: 0, y: 0, z: 90 }, loop: true, category: "ambience" });
        if (k === 108) handles.d.moveTo({ x: 6, y: 0, z: 8 });
        engine.update(1 / 60);
        if (handles.b && !handles.b.playing) seen.ended = true;
        if (handles.c && !handles.c.playing) seen.stopped = true;
        if (k === 108 && handles.d.real) seen.offset = handles.d.elapsed;
      };
      step(0);
      for (let k = 1; k / 60 < seconds; k++) {
        ctx.suspend((Math.round(((k / 60) * 48000) / 128) * 128) / 48000).then(async () => {
          step(k);
          await sync();
          ctx.resume();
        });
      }
      await sync();
      const out = await ctx.startRendering();
      return { ...encodeAudio(out), seen };
    }

export const VOICE_LIVES = [];
for (const quality of ["original", "enhanced"]) VOICE_LIVES.push({ name: `${quality}-stereo-life`, quality, output: "stereo", channels: 2, strict: true, life: true });
for (const [output, channels] of [["5.1",6],["7.1",8],["7.1.4",12]]) VOICE_LIVES.push({name:`original-${output}-life`,quality:"original",output,channels,strict:true,life:true});

// Stream-only cases isolate the loader's segment handoff: scheduled
// overlaps, offset/pitch resampling, gain targets and placed voice-over.
export async function renderStreamFixture(scene) {
  const rate = 48000, channels = ({ "5.1": 6, "7.1": 8, "7.1.4": 12 })[scene.output] || 2;
  const ctx = new OfflineAudioContext(channels, Math.round(rate * scene.seconds), rate);
  const engine = new AudioEngine({ context: ctx }); const r = engine.renderer;
  const config = { quality: scene.quality, output: scene.output, dynamics: "normal", mono: !!scene.mono, clarity: true };
  await r.load(null, config); await r.configure(config);
  const lane = r.stream(scene.inputs, scene.kind || "voice", scene.spatial ? { mode3D: 1 } : null);
  const buffers = [22050, 44100, 48000].map((hz, part) => {
    const b = ctx.createBuffer(scene.inputs, Math.round(hz * 0.16), hz);
    for (let c = 0; c < scene.inputs; c++) for (let i = 0; i < b.length; i++) b.getChannelData(c)[i] = 0.12 * Math.sin((i + 1) * (c + 1) * (part + 1) * 0.037);
    return b;
  });
  const port = r.mixer.port, waits = new Map(); let id = 0; const previous = port.onmessage;
  port.onmessage = (event) => { if (event.data?.type === "sync") { waits.get(event.data.id)?.(); waits.delete(event.data.id); } else previous?.(event); };
  const sync = () => new Promise((resolve) => { const key = ++id; waits.set(key, resolve); port.postMessage({ type: "sync", id: key }); });
  lane.gain.value = 0.6;
  lane.start(buffers[0], 0.01, 0.93, false, 0.011);
  lane.start(buffers[1], 0.10, 1.07, false, 0.017);
  const last = lane.start(buffers[2], 0.15, 1, true, 0.03); last.stop(0.27);
  if (lane.voice) lane.voice.update({ x: -0.8, y: 0.2, z: -0.56 }, 8, 0, { cutoff: 7000, send: 0.4, lfe: 0.2 });
  r.flush(); await sync();
  for (const [time, gain] of [[0.08, 0.3], [0.19, 0.7]]) ctx.suspend(Math.round(time * rate / 128) * 128 / rate).then(async () => {
    lane.gain.setTargetAtTime(gain, ctx.currentTime, 0.02);
    if (lane.voice) lane.voice.update({ x: 0.6, y: -0.1, z: 0.8 }, 12, 0, { cutoff: 2800, send: 0.2, lfe: 0 });
    r.flush(); await sync(); await ctx.resume();
  });
  return encodeAudio(await ctx.startRendering());
}
