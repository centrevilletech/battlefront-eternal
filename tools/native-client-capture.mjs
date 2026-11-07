// capture.mjs: the scene the Rust renderer drew, for a replay outside the
// browser (rust/bfe-native): its registration calls (?rustRecord), each
// registered texture read back as RGBA8, and FRAMES frames of its inputs.
// Writes OUT/calls.json and OUT/blobs.bin. Retail-derived: keep it out of repos.
import fs from 'node:fs';
import { connect, loadBattle } from './renderer-corpus/battle.mjs';
import { expand, readManifest } from './renderer-corpus/manifest.mjs';
const OUT = process.argv[2], FRAMES = Number(process.env.FRAMES || 2);
if (!OUT || !Number.isInteger(FRAMES) || FRAMES < 1 || FRAMES > 240) throw Error('usage: node tools/native-client-capture.mjs OUTPUT_DIR (FRAMES=1..240)');
fs.mkdirSync(OUT, { recursive: true });
const c = await connect();
const testCase = expand(readManifest()).cases.find(c => c.id === (process.env.CASE || 'classic-nab2'));
if (!testCase) throw Error('unknown CASE');
const settings = testCase.settings;
if (!process.env.NOLOAD) console.log('loaded', JSON.stringify(await loadBattle(c, { settings, query: `${testCase.query}&rustRecord` })));
const info = await c.ev(`(async () => { const b = __bf, g = b.graphics;
  for (let i = 0; i < 100 && !g.rust; i++) await new Promise((r) => setTimeout(r, 100));
  const r = g.rust; if (!r?.recording) return { error: 'no recording renderer' };
  const key = (code, down) => window.dispatchEvent(new KeyboardEvent(down ? 'keydown' : 'keyup', { code, key: code, bubbles: true }));
  const look = (x, y) => document.dispatchEvent(new MouseEvent('mousemove', { movementX: x, movementY: y, bubbles: true }));
  for (let i = 0; i < 20; i++) await b.lockstep(1 / 60, true);
  r.recording.keepFrames = ${FRAMES};
  key('KeyW', true);
  for (let i = 0; i < ${FRAMES}; i++) { if (i % 120 === 0) key('KeyA', true); if (i % 120 === 60) key('KeyA', false); look(Math.round(6 * Math.sin(i * 0.05)), Math.round(2 * Math.cos(i * 0.03))); await b.lockstep(1 / 60, true); }
  key('KeyW', false); key('KeyA', false);
  // Blobs: every typed array argument, every texture read back, every frame.
  const blobs = []; const blob = (a) => { blobs.push(a); return blobs.length - 1; };
  const calls = r.recording.calls.map(([name, args, result]) => [name, args.map((a) => ArrayBuffer.isView(a) ? { blob: blob(a), type: a.constructor.name } : a), result]);
  // Textures, level 0, through a copy pass into an RGBA8 target.
  const gl = g.renderer.getContext();
  const vs = '#version 300 es\\nout vec2 uv; void main() { vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2); uv = p; gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0); }';
  const fs = '#version 300 es\\nprecision highp float; uniform sampler2D t; in vec2 uv; out vec4 o; void main() { o = textureLod(t, uv, 0.0); }';
  const prog = gl.createProgram(); for (const [k, s] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) { const sh = gl.createShader(k); gl.shaderSource(sh, s); gl.compileShader(sh); gl.attachShader(prog, sh); } gl.linkProgram(prog);
  const fb = gl.createFramebuffer(); const textures = [];
  for (let id = 0; id < r.recording.textures.length; id++) {
    const t = r.recording.textures[id]; if (!t) { textures.push(null); continue; }
    let { width, height } = t; if (!width || !height) { width = height = 4; }
    const target = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, target); gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, width, height);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
    gl.viewport(0, 0, width, height); gl.disable(gl.DEPTH_TEST); gl.disable(gl.BLEND); gl.disable(gl.CULL_FACE); gl.disable(gl.SCISSOR_TEST); gl.disable(gl.SAMPLE_ALPHA_TO_COVERAGE);
    gl.useProgram(prog); gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, t.handle); gl.uniform1i(gl.getUniformLocation(prog, 't'), 0);
    gl.bindVertexArray(null); gl.drawArrays(gl.TRIANGLES, 0, 3);
    const px = new Uint8Array(width * height * 4); gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, px);
    gl.deleteTexture(target); textures.push({ width, height, blob: blob(px) });
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null); g.renderer.resetState();
  const frames = r.recording.frames.map((f) => ({ records: f.records, input: blob(f.input), bones: blob(f.bones) }));
  self.__captureBlobs = blobs;
  return { calls, textures, frames, blobs: blobs.map((a) => [a.constructor.name, a.byteLength]), size: [g.renderSize.x, g.renderSize.y], samples: g.sceneTarget.samples };
})()`);
if (info.error) { console.log(info.error); process.exit(1); }
// The blobs, a chunk at a time.
const offsets = []; let at = 0; const fd = fs.openSync(`${OUT}/blobs.bin`, 'w');
for (let i = 0; i < info.blobs.length; i++) {
  const [type, bytes] = info.blobs[i]; offsets.push([at, bytes, type]);
  for (let o = 0; o < bytes; o += 4 << 20) {
    const b64 = await c.ev(`(() => { const a = self.__captureBlobs[${i}]; const u = new Uint8Array(a.buffer, a.byteOffset + ${o}, Math.min(${4 << 20}, a.byteLength - ${o})); let s = ''; for (let k = 0; k < u.length; k += 0x8000) s += String.fromCharCode.apply(null, u.subarray(k, k + 0x8000)); return btoa(s); })()`);
    fs.writeSync(fd, Buffer.from(b64, 'base64'));
  }
  at += bytes;
}
fs.closeSync(fd);
await c.ev('(delete self.__captureBlobs, 1)');
fs.writeFileSync(`${OUT}/calls.json`, JSON.stringify({ size: info.size, samples: info.samples, calls: info.calls, textures: info.textures, frames: info.frames, blobs: offsets }));
console.log('calls', info.calls.length, 'textures', info.textures.filter(Boolean).length, 'frames', info.frames.length, 'blob MB', (at / 1e6).toFixed(1), 'size', info.size, 'samples', info.samples);
c.close();
