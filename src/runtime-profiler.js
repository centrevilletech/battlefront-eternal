// Opt-in CPU/input diagnostics. Fixed storage while recording; sorting and
// object allocation happen only when report() is requested. Durations end at
// render submission, not GPU completion or physical screen presentation.
const PHASES = ['input', 'simulation', 'presentation', 'audio', 'world', 'hud', 'render'];
const FIELDS = [...PHASES, 'cpuMs', 'frameIntervalMs', 'visualClampMs', 'steps', 'simulatedMs',
  'inputCount', 'eventDispatchDelayMs', 'inputToSubmitMs'];
const INDEX = Object.fromEntries(FIELDS.map((name, index) => [name, index]));

export class RuntimeProfiler {
  constructor({ capacity = 4096, now = () => performance.now(), timeOrigin = performance.timeOrigin } = {}) {
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error('positive profiler capacity required');
    this.capacity = capacity; this.now = now; this.timeOrigin = timeOrigin;
    this.ring = new Float64Array(capacity * FIELDS.length);
    this.current = new Float64Array(FIELDS.length);
    this.reset();
  }

  reset() {
    this.count = 0; this.active = false;
    this.pendingInputs = 0; this.oldestInput = Infinity; this.dispatchDelay = NaN;
  }

  noteInput(event) {
    const received = this.now();
    let timestamp = Number(event.timeStamp);
    if (timestamp > 1e12) timestamp -= this.timeOrigin;
    if (!(timestamp > 0 && timestamp <= received)) timestamp = received;
    this.pendingInputs++;
    this.oldestInput = Math.min(this.oldestInput, timestamp);
    this.dispatchDelay = Math.max(this.dispatchDelay || 0, received - timestamp);
  }

  begin(dt, wallDt = dt) {
    this.active = true;
    this.current.fill(0);
    this.current[INDEX.eventDispatchDelayMs] = this.current[INDEX.inputToSubmitMs] = NaN;
    this.current[INDEX.frameIntervalMs] = wallDt * 1000;
    this.current[INDEX.visualClampMs] = Math.max(0, wallDt - dt) * 1000;
    this.started = this.phaseStarted = this.now();
    this.phaseIndex = INDEX.input;
  }

  phase(name) {
    if (!this.active) return;
    const time = this.now();
    this.current[this.phaseIndex] += time - this.phaseStarted;
    this.phaseStarted = time;
    this.phaseIndex = INDEX[name];
  }

  step(dt) {
    if (!this.active) return;
    this.current[INDEX.steps]++;
    this.current[INDEX.simulatedMs] += dt * 1000;
  }

  end(drawn) {
    if (!this.active) return;
    const time = this.now();
    this.current[this.phaseIndex] += time - this.phaseStarted;
    this.current[INDEX.cpuMs] = time - this.started;
    if (drawn && this.pendingInputs) {
      this.current[INDEX.inputCount] = this.pendingInputs;
      this.current[INDEX.eventDispatchDelayMs] = this.dispatchDelay;
      this.current[INDEX.inputToSubmitMs] = time - this.oldestInput;
      this.pendingInputs = 0; this.oldestInput = Infinity; this.dispatchDelay = NaN;
    }
    this.ring.set(this.current, (this.count % this.capacity) * FIELDS.length);
    this.count++; this.active = false;
  }

  report() {
    const retained = Math.min(this.count, this.capacity);
    const metrics = {};
    for (const [name, column] of Object.entries(INDEX)) {
      const samples = [];
      for (let row = 0; row < retained; row++) {
        const value = this.ring[row * FIELDS.length + column];
        if (Number.isFinite(value)) samples.push(value);
      }
      samples.sort((a, b) => a - b);
      const at = q => samples.length ? samples[Math.max(0, Math.ceil(samples.length * q) - 1)] : null;
      metrics[name] = { samples: samples.length, p50: at(.5), p95: at(.95), p99: at(.99), max: at(1),
        total: samples.reduce((sum, value) => sum + value, 0) };
    }
    return { frames: this.count, retained, overwritten: this.count - retained,
      pendingInputs: this.pendingInputs, metrics,
      note: 'CPU phases are exclusive milliseconds; steps/inputCount are counts. Input ends at CPU render submission, not display. Event timestamps are browser timestamps, not device timestamps.' };
  }
}
