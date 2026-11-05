// Desktop checks sample both clocks against wall time. An advancing worker
// with a stuck page is a failure too: Node replay checks cannot observe it.
export function worldProgress(samples, { minimumTps = 1, maximumGapMs = 4000 } = {}) {
  if (samples.length < 2) throw Error('World progress needs at least two samples');
  const first = samples[0], last = samples.at(-1), elapsed = last.at - first.at;
  if (!(elapsed > 0)) throw Error('World progress needs advancing wall time');
  const result = { elapsed, pageTps: (last.pageTick - first.pageTick) * 1000 / elapsed,
    worldTps: (last.worldTick - first.worldTick) * 1000 / elapsed, failures: [] };
  for (const key of ['pageTick', 'worldTick']) {
    let changedAt = first.at;
    for (let i = 1; i < samples.length; i++) {
      const a = samples[i - 1], b = samples[i];
      if (!Number.isFinite(b[key]) || b[key] < a[key]) result.failures.push(`${key} moved backward or is unavailable`);
      if (b[key] > a[key]) changedAt = b.at;
      if (b.at - changedAt >= maximumGapMs) { result.failures.push(`${key} stopped advancing`); break; }
    }
  }
  if (result.pageTps < minimumTps || result.worldTps < minimumTps) result.failures.push(`World rate is below ${minimumTps} ticks per second`);
  // A suspended or lockstep battle cannot provide a live-loop measurement.
  for (const s of samples) {
    if (!s.running || s.stepping) { result.failures.push('World is suspended or in lockstep'); break; }
  }
  return result;
}
