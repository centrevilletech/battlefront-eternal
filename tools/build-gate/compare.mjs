// Compare one case's three sessions. Stage replays (the reference's recorded
// renderer inputs, run again by the reference, a repeat of it and the
// candidate) must be pixel exact. The finished frames are compared too: the
// repeat reference says how much of a frame differs between two runs of the
// same build, and the candidate's pixels that moved while the repeat's did
// not are the ones that need a cause.
import fs from 'node:fs';
import { encodePng } from './png.mjs';

const ROLES = ['reference', 'repeat', 'candidate'];
const GRID = 8;
// A difference this large in any channel is visible; below it the frame differs
// by a few levels, as run-to-run noise usually does.
const VISIBLE = 16;

// Exact RGBA comparison, alpha included, no tolerance. `stable` marks the
// pixels the repeat run left alone, so a pixel in the candidate that differs
// where the repeat did not is counted as residual.
function frameDiff(ref, repeat, candidate, w, h) {
  const out = { pixels: w * h, repeat: { differing: 0, visible: 0, maxDelta: 0 }, candidate: { differing: 0, visible: 0, maxDelta: 0 }, residual: { differing: 0, visible: 0, maxDelta: 0, bbox: null, grid: Array(GRID * GRID).fill(0) } };
  const image = new Uint8Array(w * h * 4);
  let x0 = w, y0 = h, x1 = -1, y1 = -1;
  for (let i = 0, p = 0; i < ref.length; i += 4, p++) {
    let dr = 0, dc = 0;
    for (let j = 0; j < 4; j++) { dr = Math.max(dr, Math.abs(ref[i + j] - repeat[i + j])); dc = Math.max(dc, Math.abs(ref[i + j] - candidate[i + j])); }
    if (dr) { out.repeat.differing++; out.repeat.maxDelta = Math.max(out.repeat.maxDelta, dr); if (dr > VISIBLE) out.repeat.visible++; }
    if (dc) { out.candidate.differing++; out.candidate.maxDelta = Math.max(out.candidate.maxDelta, dc); if (dc > VISIBLE) out.candidate.visible++; }
    // Red: changed where the repeat did not. Yellow: changed, and the repeat is noisy there too.
    if (dc && !dr) {
      const x = p % w, y = (p / w) | 0;
      out.residual.differing++; out.residual.maxDelta = Math.max(out.residual.maxDelta, dc); if (dc > VISIBLE) out.residual.visible++;
      out.residual.grid[Math.min(GRID - 1, (y * GRID / h) | 0) * GRID + Math.min(GRID - 1, (x * GRID / w) | 0)]++;
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
      image.set([255, 0, 0, 255], i);
    } else if (dc) image.set([255, 200, 0, 255], i);
    else image.set([ref[i] >> 2, ref[i + 1] >> 2, ref[i + 2] >> 2, 255], i);
  }
  if (x1 >= 0) out.residual.bbox = [x0, y0, x1, y1];
  return { out, image };
}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const stateDiff = (a, b) => Object.keys({ ...a, ...b }).filter(key => !same(a?.[key], b?.[key]));

// Stage replay pairs per role, and what differed.
function stageReport(runs) {
  const pairs = {
    reference: runs.reference.points.flatMap(p => p.selfReplay || []),
    repeat: runs.repeat.points.flatMap(p => p.replay || []),
    candidate: runs.candidate.points.flatMap(p => p.replay || []),
  };
  const available = pairs.reference.length > 0;
  const bad = {};
  for (const [role, list] of Object.entries(pairs)) for (const p of list) if (p.differing) {
    const row = bad[`${role} ${p.key}`] ||= { pairs: 0, differing: 0, maxDelta: 0 };
    row.pairs++; row.differing += p.differing; row.maxDelta = Math.max(row.maxDelta, p.maxDelta);
  }
  const keys = [...new Set(pairs.reference.map(p => p.key))].sort();
  return { available, pairs: Object.fromEntries(Object.entries(pairs).map(([k, v]) => [k, v.length])), keys,
    exact: available && pairs.reference.length === pairs.repeat.length && pairs.reference.length === pairs.candidate.length && !Object.keys(bad).length, differences: bad };
}

export function compareCase(out, raw, id, { keepFrames = false } = {}) {
  const runs = {};
  for (const role of ROLES) {
    const file = `${out}/${role}/capture.json`;
    runs[role] = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file)) : null;
  }
  const report = { id, kind: runs.reference?.kind, status: 'invalid', roles: {} };
  for (const role of ROLES) report.roles[role] = runs[role] ? { valid: runs[role].valid, error: runs[role].error?.split('\n')[0], gpu: runs[role].setup?.renderer, notApplicable: runs[role].notApplicable } : { missing: true };
  if (runs.reference?.notApplicable) { report.status = 'skipped'; report.reason = runs.reference.notApplicable; return report; }
  if (ROLES.some(role => !runs[role]?.valid)) return report;
  report.stages = runs.reference.stages ? { ...stageReport(runs) } : { available: false, reason: 'not requested' };
  if (runs.reference.stagesUnavailable || runs.candidate.stagesUnavailable) report.stages = { available: false, reason: runs.candidate.stagesUnavailable || runs.reference.stagesUnavailable };
  report.frames = [];
  for (let k = 0; k < runs.reference.points.length; k++) {
    const points = ROLES.map(role => runs[role].points[k]);
    const row = { k, ticks: points.map(p => p.state?.tick ?? null), size: [points[0].w, points[0].h] };
    row.stateRepeat = stateDiff(points[0].state, points[1].state);
    row.stateCandidate = stateDiff(points[0].state, points[2].state);
    if (points.some(p => p.w !== points[0].w || p.h !== points[0].h)) { row.error = 'frame sizes differ'; report.frames.push(row); continue; }
    const [a, b, c] = points.map(p => fs.readFileSync(p.frame.file));
    const { out: d, image } = frameDiff(a, b, c, points[0].w, points[0].h);
    Object.assign(row, d);
    if (d.candidate.differing) fs.writeFileSync(`${out}/diff-${k}.png`, encodePng(image, points[0].w, points[0].h));
    if (keepFrames) [a, b, c].forEach((frame, i) => fs.writeFileSync(`${out}/${ROLES[i]}-${k}.png`, encodePng(frame, points[0].w, points[0].h)));
    report.frames.push(row);
  }
  // Visible differences (over VISIBLE levels) of the candidate against the
  // repeat's own: within 1.5x of it plus 0.1% of the frame is run-to-run noise.
  const visible = (key) => report.frames.reduce((n, f) => n + (f[key]?.visible || 0), 0), total = report.frames.reduce((n, f) => n + (f.pixels || 0), 0);
  report.frame = {
    visibleRepeat: visible('repeat'), visibleCandidate: visible('candidate'), visibleResidual: visible('residual'),
    verdict: report.frames.every(f => !f.error && f.candidate.differing === 0) ? 'exact' : visible('candidate') <= 1.5 * visible('repeat') + total / 1000 ? 'within-noise' : 'beyond-noise',
    exact: report.frames.every(f => !f.error && f.candidate.differing === 0),
    repeatExact: report.frames.every(f => !f.error && f.repeat.differing === 0),
    residualPixels: report.frames.reduce((n, f) => n + (f.residual?.differing || 0), 0),
    stateAgrees: report.frames.every(f => !f.stateCandidate.length),
  };
  report.status = report.stages.available ? (report.stages.exact ? 'pass' : 'fail') : 'frame-only';
  return report;
}

// Retail-derived pixels and inputs stay transient; the report keeps counts.
export function removeRaw(raw) { fs.rmSync(raw, { recursive: true, force: true }); }
