// AudioParams set once a frame, without re-sending targets that have
// settled.
//
// The engine moves many params every frame with setTargetAtTime: a voice's
// gain and playback rate, its reverb send, a stream's level. While a param
// has an automation event pending, the browser works it out sample by
// sample, and some nodes take a slower path for it (a buffer source playing
// at another rate costs half as much again), even when the target hasn't
// moved. Chromium ends a SetTarget event once the value is within 4.5e-5 of
// its target (or ten time constants on), sets the value to the target
// exactly, and drops the event; after that, setting the same target again
// changes nothing. So a target that has held still for SETTLE time
// constants, enough to settle from any change the game makes (a factor of
// a million in level takes under 25), is sent again only once it moves. Up
// to then it is re-sent every frame as before, so the param's course is the
// same either way.
const SETTLE = 40;

export class Target {
  constructor() {
    this.value = NaN;
    this.since = -Infinity;
  }

  // Move `param` toward `value` from `now`, with time constant `tau` (s).
  // A non-finite target makes the browser throw; one bad input upstream
  // must not take the frame down, so it is simply not sent.
  set(param, value, now, tau) {
    if (!Number.isFinite(value)) return;
    if (value === this.value) {
      if (now - this.since >= SETTLE * tau) return;
    } else {
      this.value = value;
      this.since = now;
    }
    param.setTargetAtTime(value, now, tau);
  }

  // The param was set outright (its value, or setValueAtTime): it is
  // already there.
  hold(value) {
    this.value = value;
    this.since = -Infinity;
  }

  // The param's course was changed some other way: send the next target.
  forget() {
    this.value = NaN;
    this.since = -Infinity;
  }
}
