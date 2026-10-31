import test from 'node:test';
import assert from 'node:assert/strict';
import {FramePacer, ResolutionController} from '../src/runtime/frame-pacing.js';
import {FixedClock} from '../src/runtime/fixed-clock.js';
import {Settings, PERFORMANCE_PRESETS} from '../src/settings.js';

test('30 FPS presentation leaves 60 Hz simulation and elapsed time intact at all display rates', () => {
  for (const hz of [30, 60, 120, 144, 240]) {
    const pacer = new FramePacer(30), clock = new FixedClock();
    let frames = 0, presentedTime = 0;
    for (let i = 0; i < hz * 10; i++) {
      clock.advance(1 / hz, () => {});
      const dt = pacer.advance(1 / hz);
      if (dt !== null) { frames++; presentedTime += dt; }
    }
    assert.equal(clock.tick, 600);
    assert.ok(Math.abs(frames - 300) <= 1, `${hz} Hz produced ${frames} frames`);
    assert.ok(Math.abs(presentedTime - 10) < 1e-8);
  }
});

test('uncapped presentation follows every display callback and resumes without catch-up frames', () => {
  const p = new FramePacer();
  for (let i = 0; i < 240; i++) assert.ok(p.advance(1 / 240) > 0);
  p.setLimit(30); p.advance(1 / 60); assert.ok(p.advance(.25) > 0);
  assert.equal(p.advance(0), null);
  p.reset(); assert.equal(p.advance(0), 0);
});

test('a matching FPS cap tolerates display timestamp jitter without dropping alternate refreshes', () => {
  for (const hz of [30, 60, 120, 144, 240]) {
    const p=new FramePacer(hz), interval=1/hz;
    let previous=0, frames=0;
    for(let i=1;i<=hz*10;i++) {
      const now=i*interval + (i%2 ? .00008 : -.00008);
      if(p.advance(now-previous)!==null) frames++;
      previous=now;
    }
    assert.equal(frames,hz*10,`${hz} Hz lost a display opportunity`);
  }
});

test('adaptive resolution reacts to sustained GPU pressure, recovers slowly, and has bounded scale', () => {
  const r = new ResolutionController(60);
  const sample = (count, dt, cpu, gpu) => { for (let i = 0; i < count; i++) r.sample(dt, cpu, gpu); };
  sample(30, 1000/30, 3, 28); assert.equal(r.scale, 1);
  sample(400, 1000/30, 3, 28); assert.equal(r.scale, .5);
  sample(240, 1000/60, 3, 4); assert.equal(r.scale, .5);
  sample(4000, 1000/60, 3, 4); assert.equal(r.scale, 1);
});

test('CPU pressure, missing GPU timer, refresh caps, and resume gaps cannot shrink resolution', () => {
  for (const [cpu, gpu] of [[28, 5], [3, null], [3, 5]]) {
    const r = new ResolutionController(60);
    for (let i = 0; i < 400; i++) r.sample(1000/30, cpu, gpu);
    assert.equal(r.scale, 1);
    r.sample(3000, 3, 30); assert.equal(r.scale, 1);
  }
});

test('quality presets preserve combat settings and an explicit custom override', () => {
  const s = new Settings({aiMode:'classic',mouseSensitivity:7,frameLimit:'144',masterVolume:.4});
  for (const name of Object.keys(PERFORMANCE_PRESETS)) {
    s.set('performancePreset', name);
    assert.equal(s.get('performancePreset'), name);
    assert.equal(s.get('aiMode'), 'classic'); assert.equal(s.mouseScale, 2 ** .4);
    assert.equal(s.get('frameLimit'), '144'); assert.equal(s.get('masterVolume'), .4);
    assert.equal(s.get('physicsRate'), '60');
  }
  s.set('shadows', 'off'); assert.equal(s.get('performancePreset'), 'custom');
});

test('without a GPU timer, fences left unsignaled by the next frame shrink the resolution and quick ones restore it', () => {
  const r = new ResolutionController(60);
  const sample = (count, dt, cpu, behind, latency) => { for (let i = 0; i < count; i++) r.sample(dt, cpu, null, behind, latency); };
  // 30 FPS with the GPU still on the last frame when the next begins.
  sample(400, 1000/30, 3, true, null); assert.equal(r.scale, .5);
  // Back at 60 with each frame's fence seen done 4 ms after it was sent.
  sample(240, 1000/60, 3, false, 4); assert.equal(r.scale, .5, 'recovery waits for sustained headroom');
  sample(4000, 1000/60, 3, false, 4); assert.equal(r.scale, 1);
});

test('fences that the GPU passed before the next frame are not GPU pressure, and slow ones give no headroom', () => {
  // A 30 Hz display or a CPU-bound page: the GPU had finished.
  for (const cpu of [3, 28]) {
    const r = new ResolutionController(60);
    for (let i = 0; i < 400; i++) r.sample(1000/30, cpu, null, false, 2);
    assert.equal(r.scale, 1);
  }
  // CPU-bound frames with the GPU behind too: the CPU is what to fix.
  const cpuBound = new ResolutionController(60);
  for (let i = 0; i < 400; i++) cpuBound.sample(1000/30, 30, null, true, null);
  assert.equal(cpuBound.scale, 1);
  // At the target, but each fence seen done only as the next frame began:
  // no evidence of headroom, so a lowered resolution stays.
  const r = new ResolutionController(60);
  for (let i = 0; i < 400; i++) r.sample(1000/30, 3, null, true, null);
  assert.equal(r.scale, .5);
  for (let i = 0; i < 4000; i++) r.sample(1000/60, 3, null, false, 15);
  assert.equal(r.scale, .5);
  // A timer, where there is one, outranks the fences.
  const timed = new ResolutionController(60);
  for (let i = 0; i < 400; i++) timed.sample(1000/30, 3, 5, true, null);
  assert.equal(timed.scale, 1);
});

test('the render profiler fences each frame and reads them without waiting', async () => {
  const {RenderProfiler} = await import('../src/render-profiler.js');
  const signaled = new Set(), deleted = [];
  let next = 0, flushes = 0;
  const gl = {
    SYNC_GPU_COMMANDS_COMPLETE: 1, SYNC_STATUS: 2, SIGNALED: 3, UNSIGNALED: 4,
    getExtension: () => null,
    fenceSync: () => ({ id: ++next }),
    getSyncParameter: (sync, name) => (name === 2 && signaled.has(sync.id) ? 3 : 4),
    deleteSync: (sync) => deleted.push(sync.id),
    flush: () => flushes++,
  };
  const p = new RenderProfiler({ getContext: () => gl });
  p.begin(); p.end();
  assert.equal(p.behind, false, 'nothing sent yet');
  p.frameEnd();
  assert.equal(flushes, 1);
  // The next frame begins with the GPU still on the first.
  p.begin(); p.end(); p.frameEnd();
  assert.equal(p.behind, true); assert.equal(p.latency, null);
  // Both done by the third.
  signaled.add(1); signaled.add(2);
  const at = performance.now();
  p.poll(at + 1000);
  assert.deepEqual(deleted, [1, 2]); assert.equal(p.fences.length, 0);
  assert.ok(p.latency >= 1000 - 50 && p.latency <= 1000 + 50, `latency ${p.latency}`);
  p.begin(); assert.equal(p.behind, false);
  // No more than three outstanding: the oldest is let go.
  for (let i = 0; i < 5; i++) p.frameEnd();
  assert.equal(p.fences.length, 3);
  assert.deepEqual(p.report().fences, { behind: false, latencyMs: p.latency });
  // WebGL 1 (no fences): no evidence either way.
  const old = new RenderProfiler({ getContext: () => ({ getExtension: () => null }) });
  old.begin(); old.end(); old.frameEnd(); old.poll();
  assert.equal(old.behind, null); assert.equal(old.report().fences, null);
});
