import { GameAudio } from "../src/audio/game.js";
import { AudioEngine } from "../src/audio/engine.js";
import { SoundData } from "../src/audio/data.js";
import { StreamAssets } from "../src/audio/stream-assets.js";
import { INFO } from "../src/audio/control-layout.js";
import { hashKey } from "../src/audio/hash.js";
import { Object3D, Vector3 } from "three";

// Scripted game facts exercise the complete control phase before rendering.
// All names, PCM and geometry are synthetic. The collision host describes a
// roof and one wall; policy still chooses the room fan and occlusion rays.
export const CONTROL_SCENES = [];
for (const quality of ["original", "enhanced"]) {
  for (const output of ["stereo", "5.1", "7.1", "headphones"]) {
    CONTROL_SCENES.push({ name: `control-${quality}-${output}`, quality, output, seconds: 2 });
  }
  CONTROL_SCENES.push({ name: `control-${quality}-mono`, quality, output: "stereo", mono: true, seconds: 2 });
}
CONTROL_SCENES.push({ name: "control-enhanced-binaural", quality: "enhanced", output: "headphones", binaural: true, seconds: 2 });
for (const quality of ["original","enhanced"]) CONTROL_SCENES.push({name:`control-${quality}-layered-droideka`,quality,output:"stereo",seconds:2,layeredAmbience:true,enemyDroideka:true});

function manifest() {
  // Parent gains and a speech duck exercise the bus tree independently of
  // slider volume. The Classic space also keeps surround reverb audible.
  const objects = [
    { type: "Bus", Name: hashKey("root"), Gain: 0.85 },
    { type: "Bus", Name: hashKey("ingame"), Parent: hashKey("root"), Gain: 0.9 },
    { type: "Bus", Name: hashKey("soundfx"), Parent: hashKey("ingame"), Gain: 0.8 },
    { type: "Bus", Name: hashKey("voiceover"), Parent: hashKey("ingame"), Gain: 0.7, AttenuateBus: hashKey("soundfx"), Attenuation: 0.4, AttenuationLeadIn: 0.05, AttenuationLeadOut: 0.2 },
    { type: "space", Name: hashKey("room"), RoomGain: 0.3, I3DL2ReverbPreset: hashKey("preset") },
    { type: "I3DL2ReverbPreset", Name: hashKey("preset"), RoomGain: 0.3, DecayTime: 0.6, ReverbGain: 0.4 },
  ];
  const names = ["shot", "reload", "hot", "charge", "ouch", "death", "impact", "blast", "rustle", "change", "empty", "scope-in", "scope-out", "hum", "post-charge", "post-dispute", "post-discharge", "capture", "lost", "tick", "turn-on", "turning-off", "turn-off", "engine", "takeoff", "land", "yaw", "pitch", "foley-left", "foley-right", "jump", "step", "hydraulic"];
  // Nonzero deviations make an extra or missing random draw observable in
  // the waveform. Cyclic samples cover the retained per-sound pick state.
  for (let i = 0; i < names.length; i++) objects.push({
    type: "SoundProperties", Name: hashKey(names[i]), Bus: hashKey("soundfx"),
    Gain: 0.25 + (i % 4) * 0.12, GainDev: 0.04, Pitch: 0.8 + i * 0.013, PitchDev: 0.03,
    Mode3D: 1, ModeDoppler: 1, MinDistance: 3, MaxDistance: 90, MuteDistance: 80, RollOff: 0.3,
    Bias: 0.0001, Priority: i / names.length, ReverbGain: 0.3,
    PlayInterval: names[i] === "shot" ? 0.04 : 0,
    CyclePlayback: 1, SampleList: [["0x00000100", 1], ["0x00000101", 2]],
  });
  const classes = {
    rifle: { FireSound: "shot", ReloadSound: "reload", OverheatSound: "hot", OverheatSoundPitch: "0.5", ChargeSound: "charge", ChargeSoundPitch: "0.25", WeaponChangeSound: "change", FireEmptySound: "empty", JumpSound: "jump" },
    trooper: { HurtSound: "ouch", DeathSound: "death", ClothingRustleSound: "rustle", FleeSound: "line", TurnOnSound: "turn-on", TurnOffSound: "turn-off", TurnOffTime: "0.1", EngineSound: "engine" },
    projectile: { OrdnanceSound: "hum", CollisionSound: "impact", CollisionOtherSound: "impact" },
    explosion: { SoundProperty: "blast" },
    post: { AmbientSound: ["rep hum", "cis engine"], ChargeSound: "post-charge", DisputeSound: "post-dispute", DischargeSound: "post-discharge", CapturedSound: "capture", LostSound: "lost" },
    station: { AmbientSound: "hum", PowerupSound: "capture" },
    mine: { TickSound: "tick", TickSoundPitch: "0.5", CollisionOtherSound: "impact" },
    hull: { EngineSound: "engine", TurnOnSound: "turn-on", TurningOffSound: "turning-off", TurnOffSound: "turn-off", TurnOffTime: "0.15", TakeoffSound: "takeoff", LandSound: "land", HurtSound: "ouch", DeathSound: "death", VehicleCollisionSound: "impact", TurretActivateSound: "capture", TurretDeactivateSound: "lost", TurretYawSound: "yaw", TurretPitchSound: "pitch", TurretYawSoundPitch: "0.7" },
  };
  objects.push({ type: "SoundStreamProperties", Name: hashKey("line"), Stream: hashKey("speech"), Bus: hashKey("voiceover"), Gain: 0.6, Mode3D: 1, MinDistance: 5, MuteDistance: 100, MaxDistance: 100, Bias: 0.0001, SegmentList: [["0x00000200", 1]], PlayInterval: 0.08 });
  objects.push({ type: "SoundProperties", Name: hashKey("low"), Gain: 0.1, Bias: 1, Priority: 0, SampleList: [["0x00000100", 1]] });
  return { objects, classes, samples: { "0x00000100": [0, 11025, 22050], "0x00000101": [22050, 11025, 22050] }, mission: { streams: [["speech", "speech"]], effects: { ScopeDisplayZoomIn: "scope-in", ScopeDisplayZoomOut: "scope-out" }, outOfBounds: { 1: "line" } }, world: { emitters: [], regions: [], posts: { p: { class: "post", vo: { VO_Rep_CisCapture: "line" } } } } };
}

export async function renderControlFixture(scene) {
  // Mono remains a fold of the chosen layout, matching the live settings;
  // it does not ask the device to apply a second downmix after rendering.
  const rate = 48000, channels = ({ "5.1": 6, "7.1": 8 })[scene.output] || 2;
  const ctx = new OfflineAudioContext(channels, rate * scene.seconds, rate);
  // Offline contexts are deliberately allowed to start one-shots in this
  // fixture. Live suspended contexts retain their pre-gesture rejection.
  const pcm = new Int16Array(22050);
  for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(5000 * Math.sin(i * 0.071) + 2300 * Math.sin(i * 0.019));
  const bank = manifest();
  if (scene.layeredAmbience) {
    bank.objects.find((object)=>object.Name === hashKey("hum")).RollInDistance = 2;
    bank.objects.push({type:"SoundLayered",Name:hashKey("layered-bed"),Layer:[["bed","hum",0,0]]});
    bank.world.emitters.push({position:[0,0,-17],props:{Sound:"layered-bed",MinDistance:10,MaxDistance:20}});
  }
  // A transport stress scene gives queued speech several simultaneous slots.
  // Its synthetic completions share one microtask turn, as cached decodes do.
  for (let i=1;i<(scene.burstStreams || 1);i++) bank.mission.streams.push(["speech","speech"]);
  const data = new SoundData(bank, pcm.buffer);
  data.streams.set("speech", { name: "speech", channels: 1, segments: { "0x00000200": [0, 10, 2205, 22050] } });
  const audio = new GameAudio();
  // A deliberately small pool forces both virtualisation and replacement.
  // Production limits remain untouched; the same ranking code runs here.
  audio.engine = new AudioEngine({ context: ctx, maxVoices: 4, virtualVoices: 8 });
  const e = audio.engine, r = e.renderer;
  audio.streams = new StreamAssets(e);
  if (scene.binaural) r.decoder = { filters: Array.from({ length: 16 }, (_, c) => {
    const b = ctx.createBuffer(2, 256, rate);
    for (let ear = 0; ear < 2; ear++) b.getChannelData(ear)[c + ear + 1] = 0.1 / (c + 1);
    return b;
  }) };
  const config = { quality: scene.quality, output: scene.output, mono: !!scene.mono, dynamics: "normal", clarity: true };
  await r.load(null, config); await r.configure(config);
  audio.data = data; audio.streams.setData(data);
  // This replaces only asynchronous byte I/O. Segment choice, queueing,
  // timing, gain and rendering still use the production control path.
  audio.streams.segment = async () => {
    const b = ctx.createBuffer(1, 4800, rate);
    for (let i = 0; i < b.length; i++) b.getChannelData(0)[i] = 0.12 * Math.sin(i * 0.023);
    return b;
  };
  const camera = new Object3D(); camera.position.set(0, 1.6, 0);
  // Object identity stays stable while its scalar facts change. Replacing
  // weapon or body objects would reset the very state being exercised.
  const weapon = { def: { class: "rifle", HeatThreshold: 1, ShotDelay: 0.1 }, levels: [{ MaxPressedTime: 1 }], heat: 0, wait: 0, held: 0 };
  const soldier = { position: new Vector3(0, 0, -2), velocity: new Vector3(), posture: "stand", grounded: true, air: null };
  const arsenal = { weapons: [weapon], selected: [0, -1], current: (channel) => channel === 0 ? weapon : null };
  const combat = { soldier, arsenal, health: 100, maxHealth: 100, zoom: 1, zoomStep: 0, firstPerson: true };
  const enemyWeapon = { ...weapon, def: { ...weapon.def } };
  const enemyCombat = { soldier: { position: new Vector3(-8, 0, -12) }, arsenal: { weapons: [enemyWeapon], current: () => enemyWeapon }, health: 100 };
  if (scene.enemyDroideka) enemyCombat.soldier.rolled = false;
  const unit = { className: "trooper", combat: enemyCombat, soldier: enemyCombat.soldier }; enemyCombat.owner = unit;
  const post = { desc: { name: "p" }, position: new Vector3(10, 0, -10), counts: new Map([[1, 1]]), team: 0, progress: 0 };
  const station = { desc: { class: "station" }, position: new Vector3(15, 0, -8), alive: true, serve: true };
  const item = { def: { class: "mine", base: "mine", LifeSpan: 2 }, position: new Vector3(-5, 0, -4), mine: true, landed: false, life: 2 };
  let pilot = false;
  const hull = { name: "hull", aiTarget: { generation: 1 }, alive: true, health: 100, maxHealth: 100, velocity: new Vector3(), position: new Vector3(5, 0, -15), held: (seat) => seat === 0 ? pilot : true, seats: [{}, { aimYaw: 0, pitch: 0 }], def: { settings: { MaxSpeed: 20 } } };
  const collision = { cast(from, dir, reach, hit) {
    // The nearest positive intersection defines an unbounded roof and wall.
    // The game still chooses every direction, budget and source aim itself.
    let distance = Infinity;
    if (dir.y > 0) distance = Math.min(distance, (5 - from.y) / dir.y);
    if (dir.x > 0) distance = Math.min(distance, (12 - from.x) / dir.x);
    if (distance < 0 || distance > reach) return null;
    hit.distance = distance; hit.point.copy(from).addScaledVector(dir, distance); hit.terrain = false;
    return hit;
  } };
  const units = [unit];
  const game = { camera, soldier, combat, unit: "trooper", controls: { held: () => false }, teams: { 1: { classes: [["rep_trooper"]] }, 2: { classes: [["cis_trooper"]] } }, team: 1, battle: { start: {}, reinforcements: { 1: 200, 2: 200 }, bleed: {} }, units: { all: () => units }, posts: { posts: [post] }, live: { buildings: [station], turrets: [] }, items: { items: [item] }, vehicles: { vehicles: [hull] }, ordnance: { bolts: [], rounds: [], grenades: [] }, collision };
  game.soundPlanes = [[0,1,0,5],[1,0,0,12]];
  audio.attach(game); audio.configured(); await audio.prepareControl(123); audio.control.offline = true;
  let seed = 123;
  const previousRandom = Math.random;
  Math.random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
  // Release timers use the scripted context clock. Wall-clock timers can
  // otherwise expire halfway through an offline render on a slower host.
  const previousTimeout = globalThis.setTimeout;
  const timers = [];
  globalThis.setTimeout = (callback, milliseconds) => timers.push({ callback, at: ctx.currentTime + milliseconds / 1000 });
  const trace = [];
  const waits = new Map(); let id = 0;
  const port = r.mixer.port, previousMessage = port.onmessage;
  port.onmessage = (event) => { if (event.data?.type === "sync") { waits.get(event.data.id)?.(); waits.delete(event.data.id); } else previousMessage?.(event); };
  const sync = async () => {
    // A synthetic asset resolves through the same async continuation as a
    // fetched one. Drain those continuations before acknowledging commands,
    // so offline resume cannot overtake a segment's start message.
    for (let i = 0; i < 4; i++) await Promise.resolve();
    r.flush();
    await new Promise((resolve) => { const key = ++id; waits.set(key, resolve); port.postMessage({ type: "sync", id: key }); });
  };
  const step = (frame) => {
    const t = frame / 60;
    for (let i = 0; i < timers.length; i++) {
      const timer = timers[i];
      if (timer && timer.at <= ctx.currentTime) { timers[i] = null; timer.callback(); }
    }
    camera.position.x = 2 * Math.sin(t); camera.rotation.y = t * 0.3;
    // Moving the source and listener separately exercises radial Doppler,
    // the two-metre listener offset, and Classic's retained pitch glide.
    enemyCombat.soldier.position.z = -12 + 8 * Math.sin(t * 2);
    hull.velocity.z = frame < 40 ? 15 : 0; hull.position.z += hull.velocity.z / 60;
    hull.seats[1].aimYaw = frame < 65 ? t * 0.8 : 0.8;
    weapon.reloading = frame >= 12 && frame < 20;
    weapon.overheated = frame >= 25 && frame < 38; weapon.heat = 3 - t;
    weapon.charging = frame >= 45 && frame < 60; weapon.held = (frame - 45) / 60;
    if (scene.enemyDroideka) { enemyWeapon.reloading = frame >= 12 && frame < 20; enemyWeapon.overheated = frame >= 25 && frame < 38; enemyWeapon.heat = 3-t; }
    enemyWeapon.charging = frame >= 50 && frame < 70; enemyWeapon.held = (frame - 50) / 60;
    combat.zoom = frame >= 15 && frame < 35 ? 2 : 1;
    combat.zoomStep = frame >= 25 ? 1 : 0;
    post.progress = Math.min(t, 1); if (frame === 65) post.team = 1;
    pilot = frame >= 5 && frame < 70;
    item.life = 2 - t; if (frame === 7) item.landed = true;
    if (scene.burstStreams && frame === 1) for (let i=0;i<4;i++) audio.on("vo",{source:{unit},position:[8,0,-10],property:"FleeSound"});
    if (frame % 5 === 1) { audio.on("fire", combat, { weapon }); audio.on("actorFire", enemyCombat, { weapon: enemyWeapon }); }
    if (frame === 24) audio.on("actorHurt", enemyCombat, 20);
    if (frame === 31) audio.on("explosion", { class: "explosion" }, soldier.position);
    // A far one-shot must consume no voice. The following burst fills the
    // pool, after which the zero-priority request must fail to steal one.
    if (frame === 40) audio.on("sound", "hum", new Vector3(1000, 0, -3));
    if (frame === 42) for (let i = 0; i < 12; i++) audio.on("sound", "hum", new Vector3(i * 0.4, 0, -3));
    if (frame === 43) audio.on("sound", "low", soldier.position);
    if (frame === 52) audio.on("impact", { class: "projectile" }, enemyCombat.soldier.position);
    if (frame === 55) game.ordnance.bolts.push({ def: { class: "projectile" }, position: new Vector3(-5, 0, -8), life: 1 });
    if (frame === 64) game.ordnance.bolts.length = 0;
    if (frame === 68) audio.on("post", { post, kind: "captured", by: 2 });
    if (frame === 75) audio.on("vo", { source: { unit }, position: [8, 0, -10], property: "FleeSound" });
    if (frame === 82) audio.on("leaving");
    if (frame === 90) { hull.health = 0; hull.alive = false; }
    if (frame === 95) units.length = 0;
    // The engine clock continues while the game's clock is paused. This
    // exposes the pause bus fade without advancing gameplay sound timers.
    if (frame === 100) audio.setPaused(true);
    if (frame === 108) audio.setPaused(false);
    camera.updateMatrixWorld();
    audio.update(1 / 60);
    if (audio.error) throw audio.error;
    trace.push([e.stats.real, e.stats.virtual, e.stats.started, e.stats.stolen, e.stats.dropped || 0, e.stats.culled || 0]);
  };
  try {
    step(0);
    for (let k = 1; k / 60 < scene.seconds; k++) ctx.suspend(Math.round(k / 60 * rate / 128) * 128 / rate).then(async () => { step(k); await sync(); await ctx.resume(); });
    await sync();
    const out = await ctx.startRendering();
    const encoded = []; let peak = 0;
    for (let c = 0; c < out.numberOfChannels; c++) {
      const values = out.getChannelData(c); let text = "";
      for (const v of values) peak = Math.max(peak, Math.abs(v));
      const bytes = new Uint8Array(values.buffer, values.byteOffset, values.byteLength);
      for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
      encoded.push(btoa(text));
    }
    return { encoded, peak, trace, seed: audio.control.info[INFO.SEED] };
  } finally { Math.random = previousRandom; globalThis.setTimeout = previousTimeout; audio.unlisten(); }
}
