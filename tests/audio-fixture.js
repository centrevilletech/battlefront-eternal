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
  const ports = [r.mixer, r.voiceMixer, r.chain].filter(Boolean).map((n) => n.port);
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
      const gain = ctx.createGain(); gain.gain.value = 0.6;
      const source = ctx.createBufferSource(); source.buffer = b;
      source.connect(r.bed(count, gain, kind).input); source.start(ctx.currentTime);
    }
  };
  // Construct native HRTF panners before starting the offline render.
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
  const encoded = [];
  let peak = 0;
  for (let c = 0; c < channels; c++) {
    const data = out.getChannelData(c);
    for (const v of data) peak = Math.max(peak, Math.abs(v));
    const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    let text = "";
    for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
    encoded.push(btoa(text));
  }
  return { encoded, peak, channels, rate };
}
