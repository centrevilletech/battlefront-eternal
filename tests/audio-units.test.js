import test from "node:test";
import assert from "node:assert/strict";
import { fnv1a, hashKey } from "../src/audio/hash.js";
import { decodeXboxAdpcm, decodeSubstreams } from "../src/audio/adpcm.js";
import { distanceGain, pickSample } from "../src/audio/props.js";
import { dopplerRate, graph } from "../src/audio/engine.js";
import { LAYOUTS, autoLayout, vbap } from "../src/audio/layouts.js";
import { BattleMusic } from "../src/audio/music.js";
import { VoiceOver } from "../src/audio/vo.js";
import { PlayerSounds } from "../src/audio/player.js";

// Pure checks of the audio code, with made-up data (no retail assets).

test("names hash as the engine hashes them (each byte OR 0x20)", () => {
  assert.equal(hashKey("SoundProperties"), "0xee65c519");
  assert.equal(hashKey("MinDistance"), "0xb835cf50");
  // "_" becomes 0x7f, so this isn't the lowercase name's FNV-1a.
  assert.equal(fnv1a("Value_ATK_CIS"), 0x00e73245);
  assert.equal(hashKey("nab2cw"), "0xeb76afde");
});

// A tiny Xbox ADPCM encoder for test signals: the reference IMA step and
// index rules, 64 frames per 36-byte block (header sample, 63 codes).
const STEPS = [7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767];
const INDEX = [-1, -1, -1, -1, 2, 4, 6, 8];
function encode(channels) {
  const frames = channels[0].length;
  const blocks = Math.floor(frames / 64);
  const out = new Uint8Array(blocks * 36 * channels.length);
  const state = channels.map(() => ({ pred: 0, index: 0 }));
  for (let b = 0; b < blocks; b++) {
    const base = b * 36 * channels.length;
    channels.forEach((ch, c) => {
      const s = state[c];
      s.pred = Math.max(-32768, Math.min(32767, Math.round(ch[b * 64])));
      const h = base + 4 * c;
      out[h] = s.pred & 0xff;
      out[h + 1] = (s.pred >> 8) & 0xff;
      out[h + 2] = s.index;
      const codes = [];
      for (let k = 1; k < 64; k++) {
        const target = ch[b * 64 + k];
        const step = STEPS[s.index];
        let diff = target - s.pred;
        let code = 0;
        if (diff < 0) {
          code = 8;
          diff = -diff;
        }
        let vp = step >> 3;
        if (diff >= step) { code |= 4; diff -= step; vp += step; }
        if (diff >= step >> 1) { code |= 2; diff -= step >> 1; vp += step >> 1; }
        if (diff >= step >> 2) { code |= 1; vp += step >> 2; }
        s.pred = Math.max(-32768, Math.min(32767, code & 8 ? s.pred - vp : s.pred + vp));
        s.index = Math.max(0, Math.min(88, s.index + INDEX[code & 7]));
        codes.push(code);
      }
      codes.push(0);
      for (let g = 0; g < 8; g++) {
        for (let k = 0; k < 4; k++) {
          const i = g * 8 + k * 2;
          out[base + 4 * channels.length + (g * channels.length + c) * 4 + k] = codes[i] | (codes[i + 1] << 4);
        }
      }
    });
  }
  return out;
}

test("Xbox ADPCM decodes a stereo test tone back to the tone", () => {
  const n = 64 * 40;
  const L = Float64Array.from({ length: n }, (_, i) => 12000 * Math.sin((2 * Math.PI * 440 * i) / 48000));
  const R = Float64Array.from({ length: n }, (_, i) => 8000 * Math.sin((2 * Math.PI * 660 * i) / 48000 + 1));
  const [dl, dr] = decodeXboxAdpcm(encode([L, R]), 2);
  assert.equal(dl.length, n);
  let err = 0;
  let pow = 0;
  for (let i = 64; i < n; i++) {
    err += (dl[i] * 32768 - L[i]) ** 2 + (dr[i] * 32768 - R[i]) ** 2;
    pow += L[i] ** 2 + R[i] ** 2;
  }
  // 4-bit ADPCM on a clean tone: better than 25 dB SNR.
  assert.ok(10 * Math.log10(pow / err) > 25, `SNR ${10 * Math.log10(pow / err)}`);
  // The header sample is the block's first frame.
  assert.equal(Math.round(dl[64] * 32768), Math.round(L[64]));
});

test("substreams interleave in fixed chunks and come back in order", () => {
  const n = 64 * 32;
  const make = (f) => Float64Array.from({ length: n }, (_, i) => 9000 * Math.sin((2 * Math.PI * f * i) / 44100));
  const front = encode([make(300), make(310)]);
  const rear = encode([make(500), make(510)]);
  const chunk = 72 * 8; // 8 blocks per chunk
  const mixed = new Uint8Array(front.length + rear.length);
  for (let k = 0, at = 0; k * chunk < front.length; k++) {
    mixed.set(front.subarray(k * chunk, (k + 1) * chunk), at);
    at += chunk;
    mixed.set(rear.subarray(k * chunk, (k + 1) * chunk), at);
    at += chunk;
  }
  const out = decodeSubstreams(mixed, 2, 2, chunk);
  assert.equal(out.length, 4);
  const ref = decodeXboxAdpcm(rear, 2);
  for (let i = 0; i < n; i += 97) assert.equal(out[2][i], ref[0][i]);
});

test("distance gain is full inside MinDistance, falls monotonically, and is silent past MaxDistance", () => {
  const sound = { MinDistance: 11, MaxDistance: 400, MuteDistance: 400, RollOff: 3.5 };
  assert.equal(distanceGain(sound, 5), 1);
  assert.equal(distanceGain(sound, 11), 1);
  let last = 1;
  for (let d = 12; d < 400; d += 7) {
    const g = distanceGain(sound, d);
    assert.ok(g <= last + 1e-12 && g >= 0, `d=${d} g=${g}`);
    last = g;
  }
  assert.equal(distanceGain(sound, 400), 0);
  assert.equal(distanceGain(sound, 1000), 0);
});

test("samples are picked by weight", () => {
  const list = [["a", 3], ["b", 1]];
  const counts = { a: 0, b: 0 };
  const real = Math.random;
  let seed = 1;
  Math.random = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  try {
    for (let i = 0; i < 4000; i++) counts[pickSample(list)[0]]++;
  } finally {
    Math.random = real;
  }
  assert.ok(Math.abs(counts.a / 4000 - 0.75) < 0.03, JSON.stringify(counts));
  assert.equal(pickSample(list, (id) => id === "b")[0], "b");
  assert.equal(pickSample([], () => true), null);
});

test("parameter graphs interpolate between points and hold past the ends", () => {
  const points = [[0, 0], [0.5, 0.2], [1, 0.65]];
  assert.equal(graph(points, -1), 0);
  assert.ok(Math.abs(graph(points, 0.25) - 0.1) < 1e-12);
  assert.ok(Math.abs(graph(points, 0.75) - 0.425) < 1e-12);
  assert.equal(graph(points, 2), 0.65);
});

test("pairwise panning keeps unit power and picks the right speakers", () => {
  for (const [name, layout] of Object.entries(LAYOUTS)) {
    for (let az = -180; az < 180; az += 5) {
      const g = vbap(layout, az);
      const power = g.reduce((s, x) => s + x * x, 0);
      assert.ok(Math.abs(power - 1) < 1e-9, `${name} ${az}: power ${power}`);
      if (layout.lfe >= 0) assert.equal(g[layout.lfe], 0, `${name}: nothing to the LFE`);
    }
  }
  const s51 = LAYOUTS["5.1"];
  assert.ok(vbap(s51, 0)[2] > 0.999, "ahead is the centre speaker");
  assert.ok(vbap(s51, 30)[0] > 0.999, "30 left is L");
  assert.ok(vbap(s51, -110)[5] > 0.999, "110 right is SR");
  const s71 = LAYOUTS["7.1"];
  assert.ok(vbap(s71, 90)[6] > 0.999, "7.1: 90 left is SL (channel 7)");
  assert.ok(vbap(s71, -150)[5] > 0.999, "7.1: 150 right is BR (channel 6)");
  const st = vbap(LAYOUTS.stereo, 0);
  assert.ok(Math.abs(st[0] - Math.SQRT1_2) < 1e-9 && Math.abs(st[1] - Math.SQRT1_2) < 1e-9, "stereo centre is -3 dB each side");
});

test("auto layout follows the device's channel count", () => {
  assert.equal(autoLayout(2), "stereo");
  assert.equal(autoLayout(6), "5.1");
  assert.equal(autoLayout(8), "7.1");
  assert.equal(autoLayout(12), "7.1.4");
});

test("Doppler is the original game's 1 + v / 344, and off at 172 m/s", () => {
  // A bolt closing at 150 m/s plays 1.436x; going away at 150 m/s, 0.564x.
  assert.ok(Math.abs(dopplerRate(150) - 1.43605) < 1e-5);
  assert.ok(Math.abs(dopplerRate(-150) - 0.56395) < 1e-5);
  assert.equal(dopplerRate(0), 1);
  assert.equal(dopplerRate(172), 1);
  assert.equal(dopplerRate(-400), 1);
});

// The battle's music with made-up tracks: team 1 listening, each team with a
// start, middle, and end track at 100%, 99%, and 10% of 200 tickets.
function musicRig() {
  const plays = [];
  const battle = { start: { 1: 200, 2: 200 }, reinforcements: { 1: 200, 2: 200 }, over: false };
  const track = (rest) => ({ Priority: 0, FadeInTime: 1, FadeOutTime: 1, MinPlaybackTime: 5, MinInactiveTime: rest });
  const configs = { a_start: track(90), a_middle: track(30), a_end: track(30), b_start: track(90), b_middle: track(30), b_end: track(30), a_win: track(0), a_lose: track(0) };
  const audio = {
    game: { battle },
    data: { object: (kind, name) => configs[name] },
    streams: {
      play(stream, options) {
        const voice = { stream, options, stopped: null, stop(fade) { this.stopped = fade; } };
        plays.push(voice);
        return voice;
      },
    },
    endOfBattle() { audio.over = true; },
  };
  const mission = {
    music: {
      1: [[1, "a_start", 0, 1], [0.99, "a_middle", 1, 1], [0.1, "a_end", 2, 1]],
      2: [[1, "b_start", 0, 1], [0.99, "b_middle", 1, 1], [0.1, "b_end", 2, 1]],
    },
    victory: { 1: "a_win" },
    defeat: { 1: "a_lose" },
  };
  return { music: new BattleMusic(audio, mission, 1), battle, plays, audio };
}

test("music: thresholds are whole tickets, and team 2's tickets set the stage", () => {
  const { music, battle, plays } = musicRig();
  assert.deepEqual(music.slots[1].map((s) => s.threshold), [200, 198, 20]);
  music.update(0.1);
  assert.deepEqual(plays.map((p) => p.stream), ["a_start"]);
  // Team 1 nearly out, team 2 untouched: team 2's start stage still holds.
  battle.reinforcements[1] = 10;
  music.update(0.1);
  assert.equal(music.stage, 0);
  battle.reinforcements[2] = 150;
  music.update(0.1);
  assert.equal(music.stage, 1);
});

test("music: a track plays at least MinPlaybackTime before the next stage takes over", () => {
  const { music, battle, plays } = musicRig();
  music.update(0.1);
  battle.reinforcements[2] = 150;
  music.update(1);
  assert.equal(plays.length, 1, "the start track holds for 5 s");
  assert.equal(plays[0].stopped, null);
  music.update(4);
  assert.equal(plays[0].stopped, 1, "then fades out over FadeOutTime");
  assert.deepEqual(plays.map((p) => p.stream), ["a_start", "a_middle"]);
});

test("music: a track that ends rests MinInactiveTime before it plays again", () => {
  const { music, plays } = musicRig();
  music.update(0.1);
  plays[0].options.onEnd(plays[0]);
  for (let t = 0; t < 89; t++) music.update(1);
  assert.equal(plays.length, 1);
  music.update(1.5);
  assert.deepEqual(plays.map((p) => p.stream), ["a_start", "a_start"]);
});

test("music: 2 s after a side runs out, everything stops and the outcome plays", () => {
  const { music, battle, plays, audio } = musicRig();
  music.update(0.1);
  battle.over = true;
  battle.reinforcements[1] = 0;
  music.update(1.5);
  assert.ok(!audio.over);
  music.update(0.6);
  assert.ok(audio.over);
  assert.equal(plays[0].stopped, 0.05);
  assert.equal(plays.at(-1).stream, "a_lose");
});

function voRig() {
  const lines = [];
  const battle = { bleed: { 1: 0, 2: 0 }, over: false };
  const audio = { game: { battle }, streams: { queue: (name) => lines.push(name) } };
  const world = { posts: { cp1: { vo: { VO_Rep_RepCapture: "we_took_it", VO_Rep_CisCapture: "they_took_it", VO_Rep_RepLost: "we_lost_it" } } } };
  const mission = { bleeding: [[1, 1, "we_are_bleeding", 1], [1, 2, "they_are_bleeding", 1]] };
  return { vo: new VoiceOver(audio, mission, world, 1, { 1: "Rep", 2: "Cis" }), battle, lines };
}

test("voice-over: command post lines by listener side, actor side, and event", () => {
  const { vo, lines } = voRig();
  const post = { desc: { name: "cp1" } };
  vo.post({ kind: "captured", by: 1, post });
  vo.post({ kind: "captured", by: 2, post });
  vo.post({ kind: "neutralized", from: 1, post });
  assert.deepEqual(lines, ["we_took_it", "they_took_it", "we_lost_it"]);
});

test("voice-over: bleeding plays at onset; only our own repeats, every 30 s", () => {
  const { vo, battle, lines } = voRig();
  battle.bleed[2] = 1;
  vo.update(0.1);
  for (let t = 0; t < 40; t++) vo.update(1);
  assert.deepEqual(lines, ["they_are_bleeding"]);
  battle.bleed[2] = 0;
  vo.update(0.1);
  battle.bleed[1] = 1;
  vo.update(0.1);
  for (let t = 0; t < 31; t++) vo.update(1);
  assert.deepEqual(lines, ["they_are_bleeding", "we_are_bleeding", "we_are_bleeding"]);
  // The enemy starting to bleed while we bleed: silent.
  battle.bleed[2] = 1;
  vo.update(0.1);
  assert.equal(lines.length, 3);
});

// The player's feet: no skeleton (the stride alone), or the feet's bones.
function feetRig(heights = null, { enhanced = true } = {}) {
  const soldier = { position: { x: 0, y: 0, z: 0 }, velocity: { x: 0, y: 0, z: 0 }, dive: false, air: null, grounded: true };
  const foot = (h) => ({ getWorldPosition: (v) => (v.y = soldier.position.y + h()) });
  const actor = heights ? { byName: { bone_l_foot: foot(heights.left), bone_r_foot: foot(heights.right) } } : null;
  const rustles = [];
  const audio = {
    game: { soldier, combat: { dead: false, mounted: false }, actor, unit: "x" },
    engine: { renderer: { enhanced } },
    scratch: { y: 0 },
    data: { classSounds: () => ({}) },
    playClass: (cls, key) => rustles.push(key) && null,
  };
  const player = new PlayerSounds(audio);
  const steps = [];
  player.step = (f) => {
    player.feet.flags[f] = false;
    steps.push(f === 0 ? "L" : "R");
  };
  return { player, soldier, steps, rustles };
}

test("footsteps: the stride steps every 1.75 m, right foot first, rustling at each half", () => {
  const { player, soldier, steps, rustles } = feetRig();
  soldier.velocity.x = 5;
  for (let i = 0; i < 120; i++) player.updateFeet(1 / 60);
  assert.deepEqual(steps, ["R", "L", "R", "L", "R"]);
  // Flagged at 0.875 m and every 1.75 m after: six in 10 m.
  assert.equal(rustles.filter((k) => k === "ClothingRustleSound").length, 6);
});

test("footsteps: stopping steps the foot already flagged", () => {
  const { player, soldier, steps } = feetRig();
  soldier.velocity.x = 5;
  for (let i = 0; i < 12; i++) player.updateFeet(1 / 60);
  assert.deepEqual(steps, []);
  soldier.velocity.x = 0;
  player.updateFeet(1 / 60);
  assert.deepEqual(steps, ["R"]);
});

test("footsteps: a foot dropping faster than 0.3 m/s and rising from under 0.17 m steps", () => {
  let t = 0;
  // The left foot falls from 0.3 m to 0.05 m at 1 m/s and rises again;
  // the right foot drifts down at 0.2 m/s, too slow to count.
  const left = () => (t < 0.25 ? 0.3 - t : Math.min(0.05 + (t - 0.25), 0.3));
  const right = () => 0.3 - 0.2 * Math.min(t, 0.5);
  const { player, steps } = feetRig({ left, right });
  for (let i = 0; i < 40; i++) {
    t = i / 60;
    player.updateFeet(1 / 60);
  }
  assert.deepEqual(steps, ["L"]);
});

// A running foot cycle like the port's: each foot planted at about 0.12 m
// for a third of the cycle with a few millimetres of wobble, swinging up to
// 0.5 m in between, the feet half a cycle apart.
function runningFeet(period = 0.62) {
  let t = 0;
  const foot = (phase) => () => {
    const u = ((t / period + phase) % 1 + 1) % 1;
    const wobble = 0.006 * Math.sin(t * 60 * Math.PI); // flips sign every frame at 60 Hz
    if (u < 0.35) return 0.122 + wobble;
    return 0.122 + 0.38 * Math.sin(((u - 0.35) / 0.65) * Math.PI);
  };
  return { left: foot(0), right: foot(0.5), advance: (dt) => (t += dt) };
}

test("footsteps: one step a footfall while the feet animate, wobble and stride notwithstanding", () => {
  const feet = runningFeet();
  const { player, soldier, steps } = feetRig(feet);
  soldier.velocity.x = 6.5;
  for (let i = 0; i < 240; i++) {
    feet.advance(1 / 60);
    player.updateFeet(1 / 60);
  }
  // 4 s at a 0.62 s cycle: about 6.5 cycles, two footfalls each.
  assert.ok(steps.length >= 11 && steps.length <= 14, `${steps.length} steps: ${steps.join("")}`);
  for (let i = 1; i < steps.length; i++) assert.notEqual(steps[i], steps[i - 1], `feet alternate: ${steps.join("")}`);
});

test("footsteps: in the air the enhanced mode is silent; the original's stride steps on", () => {
  for (const enhanced of [true, false]) {
    const { player, soldier, steps } = feetRig(null, { enhanced });
    soldier.air = "jet";
    soldier.grounded = false;
    soldier.velocity.x = 8;
    soldier.velocity.y = 2;
    for (let i = 0; i < 120; i++) player.updateFeet(1 / 60);
    if (enhanced) assert.deepEqual(steps, []);
    // 2 s at |v| = 8.25 m/s: 16.5 m, a step every 1.75 m.
    else assert.equal(steps.length, 9);
  }
});
