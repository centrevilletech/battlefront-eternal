import * as THREE from "three";
const BOB = 0.06; // m
const BOB_RATE = 0.3; // Hz
const WAKE_LIFE = 2;
const WAKE_RADIUS = 0.25;
const SPLASH_SPEED = 5; // m/s down
const SPLASH_GAP = 1; // s
const DROWN_AFTER = 3; // s
const DROWN_DAMAGE = 30; // per second
// Gameplay-only water, shared with the rendered surface.
export class WaterState {
  constructor(data) {
    this.height = data.height; this.cell = data.cell; this.offset = data.offset; this.grid = data.grid;
    this.cells = new Set(data.cells.map(([i, j]) => j * data.grid + i));
    this.bobbing = (data.look.OscillationEnable?.[0] ?? 1) > 0;
    this.simulationTime = 0; this.physicsWave = 0; this.at = new THREE.Vector3();
  }
  step(dt) { this.simulationTime += dt; this.physicsWave = this.bobbing ? BOB * Math.sin(2 * Math.PI * BOB_RATE * this.simulationTime) : 0; }
  ripple() {}
  // The surface's height now, bob included.
  get level() {
    return this.height + this.physicsWave;
  }

  // Whether (x, z) is over a water cell (points off the grid take the
  // nearest edge cell).
  over(x, z) {
    const n = this.grid;
    const i = Math.min(Math.max(Math.floor((x + this.offset[0]) / this.cell), 0), n - 1);
    const j = Math.min(Math.max(Math.floor((z + this.offset[1]) / this.cell), 0), n - 1);
    return this.cells.has(j * n + i);
  }

  // Where a segment from `from` to `to` goes down through the surface over
  // water, or null. Things only meet water coming down through it.
  crossing(from, to, out) {
    const level = this.level;
    if (!(from.y >= level && to.y < level)) return null;
    const t = (from.y - level) / (from.y - to.y);
    out.lerpVectors(from, to, t);
    out.y = level;
    return this.over(out.x, out.z) ? out : null;
  }

  // One tick of a soldier and the water: wakes and the falling splash (the
  // map's watersplash_md), drowning, and whether his gun is under (no
  // firing). feet, head, gun are world points; returns damage to take.
  soldierStep(dt, contact, feet, head, gun, velocity, splash) {
    const level = this.level;
    const wet = level > feet.y && this.over(feet.x, feet.z);
    contact.gunUnder = gun.y < level && this.over(gun.x, gun.z);
    let damage = 0;
    if (wet) {
      contact.wakeClock = (contact.wakeClock || 0) + dt;
      const interval = 1 / (0.3 * velocity.lengthSq() + 4);
      if (contact.wakeClock > interval && velocity.lengthSq() > 0) {
        this.ripple(this.at.copy(feet).addScaledVector(velocity, 0.03), WAKE_LIFE, WAKE_RADIUS, velocity);
        contact.wakeClock = Math.min(contact.wakeClock - interval, interval);
      }
      contact.splashClock = (contact.splashClock || 0) + dt;
      if (velocity.y < -SPLASH_SPEED && contact.splashClock > SPLASH_GAP) {
        contact.splashClock = 0;
        splash?.(this.at.set(feet.x, level, feet.z));
      }
    }
    if (head.y < level && this.over(head.x, head.z)) {
      contact.under = (contact.under || 0) + dt;
      if (contact.under >= DROWN_AFTER) {
        contact.under -= 1;
        damage = DROWN_DAMAGE;
      }
    } else {
      contact.under = 0;
    }
    return damage;
  }

}
