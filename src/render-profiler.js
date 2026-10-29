// CPU submission time and asynchronously sampled GPU elapsed time. Never wait
// for GPU queries: reading an unavailable result would itself cause FPS dips.
export class RenderProfiler {
  constructor(renderer) {
    this.gl = renderer.getContext();
    this.extension = this.gl.getExtension("EXT_disjoint_timer_query_webgl2");
    this.cpu = [];
    this.gpu = [];
    this.pending = [];
    this.frame = 0;
    this.active = null;
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
    return { cpuSubmission: summarize(this.cpu), gpu: summarize(this.gpu), gpuTimerSupported: Boolean(this.extension) };
  }
}
