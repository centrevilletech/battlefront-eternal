// The spatial mixer: every 3D voice is one mono input; each is panned to the
// output channels and summed. The main thread sends, per input slot, the
// voice's direction in the listener's frame (x right, y up, z back), a
// spread (0 point-like .. 1 everywhere), and an LFE send. Gains ramp across
// each 128-frame block so moving sources don't click.
//
// Panning laws (processorOptions.law):
// - "panpot": stereo by the sine of the angle off the front, equal power,
//   front and back alike (what OpenAL Soft gives a stereo device).
// - "vbap": pairwise amplitude panning over the layout's horizontal ring
//   (Pulkki 1997), power-normalized; with a height ring (7.1.4) the energy is
//   split between the rings by the source's elevation.
// - "alsoft": the original mode on speakers, OpenAL Soft 1.20.1's own
//   law (src/audio/layouts.js, ALSOFT): the direction as 2D ambisonics up
//   to the decoder's order, each speaker its row's sum; on stereo, azimuths
//   ahead widened by processorOptions.decoder.front. No spread.
// - "ambi3": third-order ambisonics for the headphone decoder: 16 channels,
//   ACN order, orthonormal real spherical harmonics (no Condon-Shortley
//   phase), the same closed forms tools/make_binaural.py checks against
//   spaudiopy. Spread tapers the higher orders, keeping the energy.
//
// Output 0 is the speaker feeds (or the ambisonic channels); output 1 (mono)
// is the LFE send, which the main thread low-passes and adds to the LFE
// channel.

const BLOCK = 128;

// The 16 harmonics at a unit vector (x front, y left, z up).
function harmonics(x, y, z, out) {
  out[0] = 0.28209479177387814;
  out[1] = 0.4886025119029199 * y;
  out[2] = 0.4886025119029199 * z;
  out[3] = 0.4886025119029199 * x;
  out[4] = 1.0925484305920792 * x * y;
  out[5] = 1.0925484305920792 * y * z;
  out[6] = 0.31539156525252005 * (3 * z * z - 1);
  out[7] = 1.0925484305920792 * x * z;
  out[8] = 0.5462742152960396 * (x * x - y * y);
  out[9] = 0.5900435899266435 * y * (3 * x * x - y * y);
  out[10] = 2.890611442640554 * x * y * z;
  out[11] = 0.4570457994644658 * y * (5 * z * z - 1);
  out[12] = 0.3731763325901154 * z * (5 * z * z - 3);
  out[13] = 0.4570457994644658 * x * (5 * z * z - 1);
  out[14] = 1.445305721320277 * z * (x * x - y * y);
  out[15] = 0.5900435899266435 * x * (x * x - 3 * y * y);
}

class SpatialMixer extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.slots = options.numberOfInputs;
    this.channels = o.channels || 2;
    this.law = o.law || "panpot";
    this.decoder = o.decoder || null;
    this.setLayout(o.speakers || [], o.lfe ?? -1);
    // Per slot: direction (3), spread, lfe; and the gains in use now.
    this.params = new Float32Array(this.slots * 5);
    this.current = new Float32Array(this.slots * this.channels);
    this.target = new Float32Array(this.slots * this.channels);
    this.lfeNow = new Float32Array(this.slots);
    this.lfeTarget = new Float32Array(this.slots);
    this.fresh = new Uint8Array(this.slots);
    this.dirty = new Uint8Array(this.slots);
    // Slots can start with directions (offline renders, tests).
    if (o.params) this.take(o.params, true);
    this.port.onmessage = (event) => {
      const m = event.data;
      if (m.type === "params") {
        this.take(m.data, false);
      } else if (m.type === "start") {
        // A voice took this slot: its first gains apply at once.
        this.fresh[m.slot] = 1;
      } else if (m.type === "layout") {
        this.law = m.law;
        this.decoder = m.decoder || null;
        this.setLayout(m.speakers, m.lfe);
        this.next = null;
        this.scratch = null;
        this.tmp = null;
      }
    };
  }

  // [slot, x, y, z, spread, lfe] per entry.
  take(p, fresh) {
    for (let i = 0; i + 5 < p.length; i += 6) {
      const slot = p[i];
      const base = slot * 5;
      for (let k = 0; k < 5; k++) this.params[base + k] = p[i + 1 + k];
      this.dirty[slot] = 1;
      if (fresh) this.fresh[slot] = 1;
    }
  }

  setLayout(speakers, lfe) {
    this.lfe = lfe;
    // Ear-level ring and height ring, each sorted by azimuth: [channel, az].
    const ring = [];
    const top = [];
    speakers.forEach((s, channel) => {
      if (!s) return;
      const [az, el] = s;
      (el > 20 ? top : ring).push([channel, (az * Math.PI) / 180]);
    });
    ring.sort((a, b) => a[1] - b[1]);
    top.sort((a, b) => a[1] - b[1]);
    this.ring = ring;
    this.top = top;
    this.allMain = speakers.map((s, c) => (s ? c : -1)).filter((c) => c >= 0);
    if (this.current) this.dirty.fill(1);
  }

  // Pairwise panning of azimuth `az` over `ring` into `out` (scaled by `w`).
  pair(ring, az, out, w) {
    const n = ring.length;
    if (!n || w <= 0) return;
    if (n === 1) {
      out[ring[0][0]] += w;
      return;
    }
    for (let i = 0; i < n; i++) {
      let [c1, a1] = ring[i];
      let [c2, a2] = ring[(i + 1) % n];
      if (i === n - 1) a2 += 2 * Math.PI;
      let a = az;
      if (a < a1) a += 2 * Math.PI;
      if (a < a1 || a > a2) continue;
      if (a2 - a1 > Math.PI) {
        // A pair more than half a turn apart: fold the source onto their
        // front arc, front and back alike.
        const front = (a1 + a2) / 2 - Math.PI;
        a = 2 * (front + Math.PI / 2) - a;
        const t1 = a1;
        const t2 = a2;
        c1 = ring[(i + 1) % n][0];
        c2 = ring[i][0];
        a1 = t2 - 2 * Math.PI;
        a2 = t1;
        while (a > a2) a -= 2 * Math.PI;
        while (a < a1) a += 2 * Math.PI;
      }
      // Solve p = g1 l1 + g2 l2 in the plane.
      const px = Math.cos(a);
      const py = Math.sin(a);
      const x1 = Math.cos(a1);
      const y1 = Math.sin(a1);
      const x2 = Math.cos(a2);
      const y2 = Math.sin(a2);
      const det = x1 * y2 - x2 * y1;
      let g1;
      let g2;
      if (Math.abs(det) < 1e-6) {
        g1 = 1;
        g2 = 0;
      } else {
        g1 = (px * y2 - x2 * py) / det;
        g2 = (x1 * py - px * y1) / det;
      }
      g1 = Math.max(g1, 0);
      g2 = Math.max(g2, 0);
      const norm = Math.sqrt(g1 * g1 + g2 * g2) || 1;
      out[c1] += (w * g1) / norm;
      out[c2] += (w * g2) / norm;
      return;
    }
  }

  gains(slot, out) {
    const p = this.params;
    const base = slot * 5;
    let x = p[base];
    let y = p[base + 1];
    let z = p[base + 2];
    const spread = Math.min(Math.max(p[base + 3], 0), 1);
    const len = Math.sqrt(x * x + y * y + z * z);
    out.fill(0);
    if (len < 1e-6) {
      x = 0;
      y = 0;
      z = -1;
    } else {
      x /= len;
      y /= len;
      z /= len;
    }
    if (this.law === "alsoft" && this.decoder) {
      // OpenAL Soft's azimuth: positive to the right.
      let az = Math.atan2(x, -z);
      const ev = Math.asin(Math.max(-1, Math.min(1, y)));
      const front = this.decoder.front || 0;
      if (front > 0 && Math.abs(az) < Math.PI / 2) az = Math.sign(az) * Math.min(Math.abs(az) * front, Math.PI / 2);
      const ce = Math.cos(ev);
      const X = Math.cos(az) * ce;
      const Y = -Math.sin(az) * ce;
      const h = this.h2d || (this.h2d = new Float64Array(7));
      h[0] = 1;
      h[1] = 1.7320508075688772 * Y;
      h[2] = 1.7320508075688772 * X;
      h[3] = 3.872983346207417 * X * Y;
      h[4] = 1.9364916731037085 * (X * X - Y * Y);
      h[5] = 2.091650066335189 * Y * (3 * X * X - Y * Y);
      h[6] = 2.091650066335189 * X * (X * X - 3 * Y * Y);
      const rows = this.decoder.rows;
      for (let c = 0; c < out.length && c < rows.length; c++) {
        const row = rows[c];
        if (!row) continue;
        let g = 0;
        for (let k = 0; k < row.length; k++) g += row[k] * h[k];
        out[c] = g;
      }
      return;
    }
    if (this.law === "ambi3") {
      // The listener's frame is x right, y up, z back.
      harmonics(-z, -x, y, out);
      if (spread > 0) {
        // Order n weighted by (1 - spread)^n, then scaled back to the
        // energy of a point source (sum over orders of (2n + 1) w^2).
        const w = 1 - spread;
        const k = Math.sqrt(16 / (1 + 3 * w * w + 5 * w ** 4 + 7 * w ** 6));
        out[0] *= k;
        for (let c = 1; c < 16; c++) out[c] *= k * w ** Math.floor(Math.sqrt(c));
      }
      return;
    }
    if (this.law === "panpot") {
      // Equal power by the lateral position; two outputs (L, R).
      const pan = Math.max(-1, Math.min(1, x)) * (1 - spread);
      const t = ((pan + 1) * Math.PI) / 4;
      out[0] = Math.cos(t);
      out[1] = Math.sin(t);
      return;
    }
    // Azimuth: 0 ahead, positive to the left.
    const az = Math.atan2(-x, -z);
    const el = Math.asin(Math.max(-1, Math.min(1, y)));
    const scratch = this.scratch || (this.scratch = new Float32Array(this.channels));
    scratch.fill(0);
    if (this.top.length) {
      const up = Math.min(Math.max(el / (Math.PI / 4), 0), 1);
      this.pair(this.ring, az, scratch, Math.sqrt(1 - up));
      // The height ring takes its share by the same azimuth.
      const tmp = this.tmp || (this.tmp = new Float32Array(this.channels));
      tmp.fill(0);
      this.pair(this.top, az, tmp, Math.sqrt(up));
      for (let c = 0; c < this.channels; c++) scratch[c] += tmp[c];
    } else {
      this.pair(this.ring, az, scratch, 1);
    }
    // Spread: blend toward equal power on every main speaker.
    const n = this.allMain.length || 1;
    for (const c of this.allMain) out[c] = Math.sqrt((1 - spread) * scratch[c] * scratch[c] + spread / n);
  }

  process(inputs, outputs) {
    const out = outputs[0];
    const lfeOut = outputs[1]?.[0];
    const N = this.channels;
    for (let c = 0; c < out.length; c++) out[c].fill(0);
    if (lfeOut) lfeOut.fill(0);
    const target = this.next || (this.next = new Float32Array(N));
    // This voice's channels with a steady gain, and those whose gain moves.
    const steady = this.steady || (this.steady = new Int32Array(N));
    const moving = this.moving || (this.moving = new Int32Array(N));
    const C = Math.min(N, out.length);
    const current = this.current;
    const goal = this.target;
    for (let s = 0; s < this.slots; s++) {
      const input = inputs[s];
      if (!input || !input.length) continue;
      const src = input[0];
      const base = s * N;
      if (this.dirty[s]) {
        this.gains(s, target);
        for (let c = 0; c < N; c++) goal[base + c] = target[c];
        this.lfeTarget[s] = this.params[s * 5 + 4];
        this.dirty[s] = 0;
      }
      if (this.fresh[s]) {
        for (let c = 0; c < N; c++) current[base + c] = goal[base + c];
        this.lfeNow[s] = this.lfeTarget[s];
        this.fresh[s] = 0;
      }
      // A silent input adds nothing; its gains still arrive where they go.
      let silent = true;
      for (let i = 0; i < BLOCK; i++) {
        if (src[i] !== 0) {
          silent = false;
          break;
        }
      }
      if (silent) {
        for (let c = 0; c < C; c++) current[base + c] = goal[base + c];
        if (lfeOut) this.lfeNow[s] = this.lfeTarget[s];
        continue;
      }
      let steadyCount = 0;
      let movingCount = 0;
      for (let c = 0; c < C; c++) {
        const g0 = current[base + c];
        const g1 = goal[base + c];
        if (g0 === 0 && g1 === 0) continue;
        if (g0 === g1) steady[steadyCount++] = c;
        else moving[movingCount++] = c;
      }
      // Two channels a pass: each sample is read once for both.
      let k = 0;
      for (; k + 1 < steadyCount; k += 2) {
        const c0 = steady[k];
        const c1 = steady[k + 1];
        const d0 = out[c0];
        const d1 = out[c1];
        const g0 = goal[base + c0];
        const g1 = goal[base + c1];
        for (let i = 0; i < BLOCK; i++) {
          const x = src[i];
          d0[i] += x * g0;
          d1[i] += x * g1;
        }
      }
      if (k < steadyCount) {
        const c = steady[k];
        const dst = out[c];
        const g1 = goal[base + c];
        for (let i = 0; i < BLOCK; i++) dst[i] += src[i] * g1;
      }
      // Gains that moved ramp across the block.
      for (k = 0; k + 1 < movingCount; k += 2) {
        const c0 = moving[k];
        const c1 = moving[k + 1];
        const d0 = out[c0];
        const d1 = out[c1];
        const a0 = current[base + c0];
        const a1 = current[base + c1];
        const step0 = (goal[base + c0] - a0) / BLOCK;
        const step1 = (goal[base + c1] - a1) / BLOCK;
        for (let i = 0; i < BLOCK; i++) {
          const x = src[i];
          d0[i] += x * (a0 + step0 * (i + 1));
          d1[i] += x * (a1 + step1 * (i + 1));
        }
        current[base + c0] = goal[base + c0];
        current[base + c1] = goal[base + c1];
      }
      if (k < movingCount) {
        const c = moving[k];
        const dst = out[c];
        const g0 = current[base + c];
        const step = (goal[base + c] - g0) / BLOCK;
        for (let i = 0; i < BLOCK; i++) dst[i] += src[i] * (g0 + step * (i + 1));
        current[base + c] = goal[base + c];
      }
      if (lfeOut) {
        const l0 = this.lfeNow[s];
        const l1 = this.lfeTarget[s];
        if (l0 !== 0 || l1 !== 0) {
          const step = (l1 - l0) / BLOCK;
          let g = l0;
          for (let i = 0; i < BLOCK; i++) {
            g += step;
            lfeOut[i] += src[i] * g;
          }
          this.lfeNow[s] = l1;
        }
      }
    }
    return true;
  }
}

registerProcessor("spatial-mixer", SpatialMixer);
