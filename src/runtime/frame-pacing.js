// Presentation deadlines are independent of the world clock. No timer loop or
// spin wait: the browser still schedules every opportunity with rAF.
export class FramePacer {
  constructor(limit = 0) { this.setLimit(limit); }
  setLimit(limit) {
    this.limit = Number.isFinite(Number(limit)) && Number(limit) > 0 ? Number(limit) : 0;
    this.reset();
  }
  reset() { this.elapsed = 0; this.presented = 0; this.deadline = 0; }
  advance(seconds) {
    this.elapsed += Math.max(0, seconds);
    // rAF timestamps have small jitter even on a display matching the cap.
    // Missing a 60 Hz deadline by 0.1 ms must not throw away a whole refresh.
    const tolerance = this.limit ? Math.min(.0005, .05 / this.limit) : 0;
    if (this.limit && this.elapsed + tolerance + 1e-8 < this.deadline) return null;
    const dt = this.elapsed - this.presented;
    this.presented = this.elapsed;
    // Retain the phase on an irregular display, but don't render catch-up
    // frames after a stall. Only simulation owes catch-up ticks.
    if (this.limit) this.deadline += Math.max(1, Math.floor((this.elapsed + tolerance + 1e-8 - this.deadline) * this.limit) + 1) / this.limit;
    return dt;
  }
}

// Conservative dynamic resolution. Evaluate sustained frame pressure, not a
// single shader compilation or OS hitch. Never repeatedly shrink the image to
// try to fix a CPU-bound simulation. No localStorage writes while adapting.
//
// The GPU's share comes from its timer where the browser offers one (gpuMs),
// else from the render profiler's fences (render-profiler.js): `behind`, the
// last frame's GPU work still unfinished when this one began, and `latency`,
// how soon after a frame was sent its fence was seen signaled (ms, an upper
// bound of the GPU's time past the frame's last command).
export class ResolutionController {
  constructor(target = 0) { this.setTarget(target); }
  setTarget(target) {
    this.target = Math.max(0, Number(target) || 0); this.scale = 1;
    this.reset();
  }
  reset() {
    this.age = 0; this.time = 0; this.frames = 0; this.slow = 0;
    this.cpuBound = 0; this.headroom = 0; this.recover = 0;
  }
  sample(intervalMs, cpuMs, gpuMs = null, behind = null, latency = null) {
    if (!this.target || !(intervalMs > 0) || !Number.isFinite(intervalMs)) return this.scale;
    // Ignore resume gaps and large task stalls. They are not GPU evidence.
    if (intervalMs > 250) { this.reset(); return this.scale; }
    const budget = 1000 / this.target;
    const gpuKnown = Number.isFinite(gpuMs) && gpuMs > 0;
    // Without any GPU evidence, 30 Hz display pacing is indistinguishable
    // from a GPU taking 33 ms. Keep the chosen resolution rather than guessing.
    const fenced = !gpuKnown && typeof behind === "boolean";
    const gpuSlow = gpuKnown ? gpuMs > budget * .85 : fenced && behind;
    const gpuQuick = gpuKnown ? gpuMs < budget * .7 : fenced && !behind && Number.isFinite(latency) && latency < budget * .7;
    this.time += intervalMs; this.age += intervalMs; this.frames++;
    this.slow += Number(intervalMs > budget * 1.15 && gpuSlow);
    this.cpuBound += Number(cpuMs > budget * .85 && (!gpuKnown || cpuMs > gpuMs * 1.2));
    this.headroom += Number(intervalMs < budget * 1.08 && cpuMs < budget * .65 && gpuQuick);
    // A two-second startup/window prevents a low refresh display or one
    // expensive frame from thrashing render targets.
    if (this.time < 2000 || this.frames < 20) return this.scale;
    if (this.slow / this.frames > .35 && this.cpuBound / this.frames < .25) {
      this.scale = Math.max(.5, this.scale - .125); this.recover = 0;
    } else if (this.headroom / this.frames > .9) {
      this.recover += this.time;
      if (this.recover >= 8000) { this.scale = Math.min(1, this.scale + .0625); this.recover = 0; }
    } else this.recover = 0;
    this.time = this.frames = this.slow = this.cpuBound = this.headroom = 0;
    return this.scale;
  }
}
