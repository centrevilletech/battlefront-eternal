// CPU submission time and asynchronously sampled GPU elapsed time. Never wait
// for GPU queries: reading an unavailable result would itself cause FPS dips.
//
// Many older drivers don't offer the timer extension at all. For them a fence
// follows each frame (frameEnd, after the HUD), and its status is read without
// waiting: the browser refreshes it between tasks. A frame whose fence is
// still unsignaled when the next one begins left the GPU behind (`behind`);
// one seen signaled soon after it was sent shows headroom (`latency`, an upper
// bound of how long the GPU took past the frame's last command).
export class RenderProfiler {
  constructor(renderer) {
    this.gl = renderer.getContext();
    this.extension = this.gl.getExtension("EXT_disjoint_timer_query_webgl2");
    this.cpu = [];
    this.gpu = [];
    this.pending = [];
    this.frame = 0;
    this.active = null;
    this.fencing = typeof this.gl.fenceSync === "function";
    this.fences = [];
    this.behind = null;
    this.latency = null;
  }

  begin() {
    const gl = this.gl, ext = this.extension;
    // Poll at the sampling cadence, not once per displayed frame. Even a
    // disjoint-status read can synchronize with the GPU process.
    if (ext && this.frame % 8 === 0) {
      if (gl.getParameter(ext.GPU_DISJOINT_EXT)) {
        for (const query of this.pending) gl.deleteQuery(query);
        this.pending.length = 0;
        this.gpu.length = 0;
      } else {
        while (this.pending.length && gl.getQueryParameter(this.pending[0], gl.QUERY_RESULT_AVAILABLE)) {
          const query = this.pending.shift();
          this.push(this.gpu, gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
          gl.deleteQuery(query);
        }
        if (this.pending.length < 4) {
          this.active = gl.createQuery();
          gl.beginQuery(ext.TIME_ELAPSED_EXT, this.active);
        }
      }
    }
    if (this.fencing) {
      this.poll();
      this.behind = this.fences.length > 0;
    }
    this.frame++;
    this.started = performance.now();
  }

  end() {
    this.push(this.cpu, performance.now() - this.started);
    if (this.active) {
      this.gl.endQuery(this.extension.TIME_ELAPSED_EXT);
      this.pending.push(this.active);
      this.active = null;
    }
  }

  // The whole frame is submitted (the scene, the water, the HUD).
  frameEnd() {
    if (!this.fencing) return;
    const gl = this.gl;
    // A GPU three frames behind is behind either way.
    if (this.fences.length >= 3) gl.deleteSync(this.fences.shift().sync);
    const sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    if (!sync) return;
    this.fences.push({ sync, at: performance.now() });
    // Sent now, so it can signal before the browser's own end-of-task flush.
    gl.flush();
  }

  // The fences the GPU has passed since the last look (cached statuses: no
  // round trip). Cheap enough for every animation frame and a timer.
  poll(now = performance.now()) {
    const gl = this.gl;
    while (this.fences.length) {
      const fence = this.fences[0];
      if (gl.getSyncParameter(fence.sync, gl.SYNC_STATUS) !== gl.SIGNALED) break;
      this.latency = now - fence.at;
      gl.deleteSync(fence.sync);
      this.fences.shift();
    }
  }

  push(samples, value) {
    if (samples.length === 120) samples.shift();
    samples.push(value);
  }

  report() {
    const summarize = (samples) => {
      if (!samples.length) return null;
      const sorted = [...samples].sort((a, b) => a - b);
      return { samples: samples.length, medianMs: sorted[Math.floor(sorted.length / 2)], p95Ms: sorted[Math.ceil(sorted.length * .95) - 1], maxMs: sorted.at(-1) };
    };
    return {
      cpuSubmission: summarize(this.cpu), gpu: summarize(this.gpu), gpuTimerSupported: Boolean(this.extension),
      fences: this.fencing ? { behind: this.behind, latencyMs: this.latency } : null,
    };
  }
}
