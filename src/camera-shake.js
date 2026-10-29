import { falloff } from "./ordnance.js";

// Camera shake, as the 2004 game shakes the view (docs/formats.md,
// "Camera shake"). It is the page's: an explosion shakes the view of the
// player whose camera follows something near it, where the page shows the
// explosion. The world keeps none of it.
//
// - Explode, after the blast has been dealt:
//   what the camera follows (his soldier, the hull he drives, the recon
//   droid he flies) is d metres from the blast at its origin, r its bounding
//   sphere's radius. Past 0.0001 m, f = falloff(d - r / 2, ShakeRadiusInner,
//   ShakeRadiusOuter), and with f > 0 the camera takes a shake of
//   Shake x f that dies away over ShakeLength x f seconds (both also times
//   the class's ShakeScale, which no stock class sets: 1). The PC build
//   asks twice: its loop over the two split-screen views reads view 0 both
//   times, so each explosion takes two shakes.
// - The camera holds at most four shakes; one more is dropped.
// - Each frame the shakes' amounts are summed, then each falls by dt x its
//   amount / its length, and one at 0 is gone.
// - The view then turns about each of its own axes by (2r - 1)^3 x 0.1 x
//   the sum radians, r random in 0..1. The death camera is not shaken.
export const SHAKE_SLOTS = 4;
export const SHAKE_REPEATS = 2;
const SHAKE_ANGLE = 0.1; // rad per unit of shake, at most
const NEAREST = 1e-4; // m: nothing closer to the blast is shaken

// The share of an explosion's shake for a thing `distance` metres from it
// at its origin, with a bounding sphere of `radius`.
export function shakeShare(explosion, distance, radius = 0) {
  if (!(explosion?.Shake > 0) || !(explosion.ShakeRadiusOuter > 0) || !(distance > NEAREST)) return 0;
  return falloff(distance - radius / 2, explosion.ShakeRadiusInner || 0, explosion.ShakeRadiusOuter);
}

export class CameraShake {
  constructor(random = Math.random) {
    this.random = random;
    this.amounts = [];
    this.rates = [];
    this.total = 0;
  }

  // One shake of `amount`, gone after `length` seconds.
  add(amount, length) {
    if (!(amount > 0) || this.amounts.length >= SHAKE_SLOTS) return false;
    this.amounts.push(amount);
    this.rates.push(length > 0 ? amount / length : Infinity);
    return true;
  }

  // An explosion at `position` (an array or a vector) as seen by what the
  // camera follows: { position, radius }. Returns the share it shook by.
  explode(explosion, position, target) {
    if (!target) return 0;
    const at = target.position, [x, y, z] = Array.isArray(position) ? position : [position.x, position.y, position.z];
    const share = shakeShare(explosion, Math.hypot(at.x - x, at.y - y, at.z - z), target.radius || 0);
    if (!(share > 0)) return 0;
    for (let i = 0; i < SHAKE_REPEATS; i++) this.add(explosion.Shake * share, (explosion.ShakeLength ?? 0.5) * share);
    return share;
  }

  // The frame's sum, then the decay.
  update(dt) {
    let total = 0;
    for (let i = 0; i < this.amounts.length; i++) {
      total += this.amounts[i];
      this.amounts[i] -= dt * this.rates[i];
      if (this.amounts[i] <= 0) {
        this.amounts.splice(i, 1);
        this.rates.splice(i, 1);
        i--;
      }
    }
    this.total = total;
    return total;
  }

  // The frame: sum and decay, and the camera turned by the sum unless it
  // is the death camera's (`still`). Paused (dt 0), nothing moves.
  apply(camera, dt, still = false) {
    if (!(dt > 0) || !this.amounts.length) return 0;
    const total = this.update(dt);
    if (still || !(total > 0)) return 0;
    const turn = () => (2 * this.random() - 1) ** 3 * total * SHAKE_ANGLE;
    camera.rotateX(turn());
    camera.rotateY(turn());
    camera.rotateZ(turn());
    return total;
  }

  clear() {
    this.amounts.length = this.rates.length = 0;
    this.total = 0;
  }
}
