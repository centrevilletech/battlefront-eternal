// A text's intro, the 2004 interface's "flashy" text, after its start delay:
// 1. the bar behind it (a title's orange, a menu row's blue) grows left to
//    right (0.1 s);
// 2. a white block (the cursor texture) fades in at the text's left (0.05 s);
// 3. the block sweeps across the text (0.05 s);
// 4. the text grows from its centre line, at full strength (0.15 s);
// 5. the text blinks out and fades back in (0.15 s).
// The retail screenshot taken half a second after the screen opened
// (load.png) shows the five class titles at stages 5, 5, 5, 4 and 4 with
// the 0.05 s per slot delays, as this order gives.

export const INTRO = [0.1, 0.05, 0.05, 0.15, 0.15];
const ENDS = INTRO.reduce((ends, length) => [...ends, (ends.at(-1) ?? 0) + length], []);
export const INTRO_LENGTH = ENDS.at(-1);

export class TitleIntro {
  constructor() {
    this.time = INTRO_LENGTH; // finished
  }

  start(delay = 0) {
    this.time = -delay;
  }

  update(dt) {
    if (this.time < INTRO_LENGTH) this.time = Math.min(this.time + dt, INTRO_LENGTH);
  }

  get done() {
    return this.time >= INTRO_LENGTH;
  }

  // What shows: bar (0..1 of its length), block (null, or { x: 0..1 across
  // the text, alpha }), text (its height, 0..1) and alpha (0..1).
  get frame() {
    const t = this.time;
    const at = (stage) => Math.min(Math.max((t - (ENDS[stage - 1] ?? 0)) / INTRO[stage], 0), 1);
    if (t < 0) return { bar: 0, block: null, text: 0, alpha: 0 };
    if (t < ENDS[0]) return { bar: at(0), block: null, text: 0, alpha: 0 };
    if (t < ENDS[1]) return { bar: 1, block: { x: 0, alpha: at(1) }, text: 0, alpha: 0 };
    if (t < ENDS[2]) return { bar: 1, block: { x: at(2), alpha: 1 }, text: 0, alpha: 0 };
    if (t < ENDS[3]) return { bar: 1, block: null, text: at(3), alpha: 1 };
    if (t < ENDS[4]) return { bar: 1, block: null, text: 1, alpha: at(4) };
    return { bar: 1, block: null, text: 1, alpha: 1 };
  }
}
